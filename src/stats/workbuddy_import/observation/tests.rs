//! Real-SQLite journal boundaries, recovery and sanitized profile regression.
use super::super::{classify::Attribution, parser, types::ParseDisposition};
use super::*;
use rusqlite::Connection;
use serde_json::json;

fn database() -> (tempfile::TempDir, Connection, String) {
    let folder = tempfile::tempdir().unwrap();
    let mut conn = Connection::open(folder.path().join("usage.sqlite3")).unwrap();
    crate::stats::store::migrate(&mut conn, folder.path()).unwrap();
    let tx = conn.transaction().unwrap();
    let current = store::load_import_settings(&tx).unwrap();
    let store::SettingsOutcome::Updated(settings) =
        store::set_import_enabled(&tx, true, current.revision, 1).unwrap()
    else {
        panic!("enable");
    };
    tx.commit().unwrap();
    (folder, conn, settings.namespace_id.clone())
}

fn plan(route: Option<&str>) -> Vec<ManagedModel> {
    vec![ManagedModel {
        requested_model: "custom-local:supplier/model".into(),
        route_id: route.map(str::to_owned),
    }]
}

fn observations(route: &str, at: i64) -> Vec<ConfigObservation> {
    vec![ConfigObservation {
        managed: true,
        model_id: Some("custom-local:supplier/model".into()),
        route_id: Some(route.into()),
        observed_at_ms: at,
    }]
}

fn journal_attribution(tx: &Transaction<'_>, ns: &str, at: i64) -> Attribution {
    let bytes = serde_json::to_vec(&json!({"type":"function_call","sessionId":"session","timestamp":at,
        "providerData":{"messageId":"message","requestModelId":"custom-local:supplier/model","rawUsage":{"prompt_tokens":1}}})).unwrap();
    let ParseDisposition::Candidate(candidate) = parser::evaluate(&bytes, "session", ns, 1_000)
    else {
        panic!("candidate");
    };
    // Journal provenance remains available for sync recovery, but no longer
    // classifies or excludes custom usage. Assert that independence each time.
    assert_eq!(
        store::attribute_candidate(tx, ns, &candidate).unwrap(),
        (Attribution::Direct, None)
    );
    let unresolved: bool = tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM workbuddy_model_history
        WHERE namespace_id=?1 AND requested_model=?2 AND state IN ('prepared','conflict')
        AND valid_from_ms<=?3 AND (valid_to_ms IS NULL OR ?3<valid_to_ms))",
            params![ns, candidate.requested_model, at],
            |r| r.get(0),
        )
        .unwrap();
    if unresolved {
        return Attribution::Pending;
    }
    match store::match_mapping(tx, ns, candidate.requested_model.as_deref().unwrap(), at).unwrap() {
        store::MappingMatch::One(mapping) => match mapping.classification {
            store::MappingClass::Proxy => Attribution::Proxy,
            store::MappingClass::Direct => Attribution::Direct,
        },
        _ => Attribution::Pending,
    }
}

fn direct(tx: &Transaction<'_>, ns: &str) {
    store::insert_mapping(
        tx,
        &store::NewMapping {
            mapping_id: "direct".into(),
            namespace_id: ns.into(),
            requested_model: "custom-local:supplier/model".into(),
            classification: store::MappingClass::Direct,
            valid_from_ms: 1,
            valid_to_ms: None,
            evidence: store::MappingEvidence::Manual,
            state: store::MappingState::Verified,
            route_id: None,
            provider_id: None,
            now_ms: 1,
        },
    )
    .unwrap();
}

#[test]
fn prepare_and_real_confirmation_never_backdate_proxy_evidence() {
    let (_folder, mut conn, ns) = database();
    let tx = conn.transaction().unwrap();
    direct(&tx, &ns);
    let token = prepare_sync(&tx, &plan(Some("route")), 10)
        .unwrap()
        .unwrap();
    assert_eq!(journal_attribution(&tx, &ns, 9), Attribution::Direct);
    assert_eq!(journal_attribution(&tx, &ns, 10), Attribution::Pending);
    assert!(finish_sync(&tx, &token, &observations("route", 20), 20).unwrap());
    assert_eq!(journal_attribution(&tx, &ns, 19), Attribution::Pending);
    assert_eq!(journal_attribution(&tx, &ns, 20), Attribution::Proxy);
    assert_eq!(
        tx.query_row("SELECT COUNT(*) FROM usage_requests", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    tx.commit().unwrap();
}

#[test]
fn same_millisecond_confirmation_leaves_one_millisecond_uncertainty() {
    let (_folder, mut conn, ns) = database();
    let tx = conn.transaction().unwrap();
    let token = prepare_sync(&tx, &plan(Some("route")), 10)
        .unwrap()
        .unwrap();
    assert!(finish_sync(&tx, &token, &observations("route", 10), 10).unwrap());
    assert_eq!(journal_attribution(&tx, &ns, 10), Attribution::Pending);
    assert_eq!(journal_attribution(&tx, &ns, 11), Attribution::Proxy);
}

#[test]
fn clock_regression_cannot_confirm_an_intent() {
    let (_folder, mut conn, ns) = database();
    let tx = conn.transaction().unwrap();
    let token = prepare_sync(&tx, &plan(Some("route")), 20)
        .unwrap()
        .unwrap();
    assert!(!finish_sync(&tx, &token, &observations("route", 10), 10).unwrap());
    assert_eq!(journal_attribution(&tx, &ns, 21), Attribution::Pending);
}

#[test]
fn restart_after_write_recovers_future_only_and_preserves_uncertain_gap() {
    let (_folder, mut conn, ns) = database();
    let tx = conn.transaction().unwrap();
    prepare_sync(&tx, &plan(Some("route")), 10).unwrap();
    tx.commit().unwrap();
    let tx = conn.transaction().unwrap();
    observe(&tx, &observations("route", 20), 20, false).unwrap();
    observe(&tx, &[], 20, true).unwrap();
    assert_eq!(journal_attribution(&tx, &ns, 9), Attribution::Pending);
    assert_eq!(journal_attribution(&tx, &ns, 19), Attribution::Pending);
    assert_eq!(journal_attribution(&tx, &ns, 20), Attribution::Proxy);
    assert!(
        store::load_mappings_in_state(&tx, store::MappingState::Prepared)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn restart_before_write_does_not_restore_old_direct_rule() {
    let (_folder, mut conn, ns) = database();
    let tx = conn.transaction().unwrap();
    direct(&tx, &ns);
    prepare_sync(&tx, &plan(Some("route")), 10).unwrap();
    observe(&tx, &[], 20, true).unwrap();
    assert_eq!(journal_attribution(&tx, &ns, 9), Attribution::Direct);
    assert_eq!(journal_attribution(&tx, &ns, 15), Attribution::Pending);
    assert_eq!(journal_attribution(&tx, &ns, 30), Attribution::Pending);
}

#[test]
fn removal_ends_proxy_evidence_and_does_not_guess_direct() {
    let (_folder, mut conn, ns) = database();
    let tx = conn.transaction().unwrap();
    observe(&tx, &observations("route", 10), 10, true).unwrap();
    let token = prepare_sync(&tx, &plan(None), 20).unwrap().unwrap();
    assert!(finish_sync(&tx, &token, &[], 30).unwrap());
    assert_eq!(journal_attribution(&tx, &ns, 19), Attribution::Proxy);
    assert_eq!(journal_attribution(&tx, &ns, 20), Attribution::Pending);
    assert_eq!(journal_attribution(&tx, &ns, 30), Attribution::Pending);
}

#[test]
fn absent_snapshot_closes_at_last_successful_observation_not_current_time() {
    let (_folder, mut conn, ns) = database();
    let tx = conn.transaction().unwrap();
    observe(&tx, &observations("route", 10), 10, true).unwrap();
    observe(&tx, &observations("route", 20), 20, true).unwrap();
    observe(&tx, &[], 30, true).unwrap();
    assert_eq!(journal_attribution(&tx, &ns, 20), Attribution::Proxy);
    assert_eq!(journal_attribution(&tx, &ns, 21), Attribution::Pending);
}

#[test]
fn configuration_projection_uses_id_and_never_exports_keys_or_urls() {
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("models.json");
    crate::config::write_json(&path,&json!([
        {"id":"supplier/model","model":"upstream-different","_workbuddyProxy":{"managed":true,"route":"route"},"apiKey":"SYNTHETIC_SECRET","url":"https://private.invalid/value"},
        {"id":"user-direct","vendor":"Custom","apiKey":"USER_SECRET","url":"https://direct.invalid"}
    ])).unwrap();
    let found = crate::workbuddy::import_observations(&path, "", 10).unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(
        found[0].model_id.as_deref(),
        Some("custom-local:supplier/model")
    );
    let text = format!("{found:?}");
    for forbidden in [
        "SYNTHETIC_SECRET",
        "USER_SECRET",
        "private.invalid",
        "direct.invalid",
        "upstream-different",
    ] {
        assert!(!text.contains(forbidden));
    }
}

#[test]
fn existing_namespace_journals_sync_even_while_import_is_disabled() {
    let (_folder, mut conn, ns) = database();
    let tx = conn.transaction().unwrap();
    direct(&tx, &ns);
    let settings = store::load_import_settings(&tx).unwrap();
    store::set_import_enabled(&tx, false, settings.revision, 5).unwrap();
    let token = prepare_sync(&tx, &plan(Some("route")), 10)
        .unwrap()
        .unwrap();
    assert!(finish_sync(&tx, &token, &observations("route", 20), 20).unwrap());
    assert_eq!(journal_attribution(&tx, &ns, 15), Attribution::Pending);
    assert_eq!(journal_attribution(&tx, &ns, 20), Attribution::Proxy);
}

#[test]
fn pausing_closes_direct_history_and_resume_cannot_reuse_it_for_the_gap() {
    let (_folder, mut conn, ns) = database();
    let tx = conn.transaction().unwrap();
    direct(&tx, &ns);
    let settings = store::load_import_settings(&tx).unwrap();
    store::set_import_enabled(&tx, false, settings.revision, 10).unwrap();
    let settings = store::load_import_settings(&tx).unwrap();
    store::set_import_enabled(&tx, true, settings.revision, 20).unwrap();
    assert_eq!(journal_attribution(&tx, &ns, 9), Attribution::Direct);
    assert_eq!(journal_attribution(&tx, &ns, 10), Attribution::Pending);
    assert_eq!(journal_attribution(&tx, &ns, 21), Attribution::Pending);
}
