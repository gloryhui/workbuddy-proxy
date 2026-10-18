//! Automatic classification and exact cross-source precedence.
use super::super::{
    classify::Attribution,
    parser, reconcile, store,
    types::{Candidate, ParseDisposition},
};
use rusqlite::{Connection, params};
use serde_json::json;
fn database() -> (tempfile::TempDir, Connection, String) {
    let folder = tempfile::tempdir().unwrap();
    let mut conn = Connection::open(folder.path().join("usage.sqlite3")).unwrap();
    crate::stats::store::migrate(&mut conn, folder.path()).unwrap();
    conn.execute("UPDATE stats_settings SET enabled=1,retention_days=0", [])
        .unwrap();
    conn.execute(
        "INSERT INTO stats_runs(run_id,started_at_ms) VALUES('capture',1000)",
        [],
    )
    .unwrap();
    let tx = conn.transaction().unwrap();
    let current = store::load_import_settings(&tx).unwrap();
    let store::SettingsOutcome::Updated(settings) =
        store::set_import_enabled(&tx, true, current.revision, 1).unwrap()
    else {
        panic!("enable")
    };
    let ns = settings.namespace_id.clone();
    tx.commit().unwrap();
    (folder, conn, ns)
}
fn candidate(ns: &str, session: &str, message: &str, model: &str) -> Candidate {
    let bytes=serde_json::to_vec(&json!({"type":"function_call","sessionId":session,"timestamp":110,
 "providerData":{"messageId":message,"requestModelId":model,"rawUsage":{"prompt_tokens":100,"completion_tokens":20,"total_tokens":120,"credit":1.14}}})).unwrap();
    let ParseDisposition::Candidate(value) = parser::evaluate(&bytes, session, ns, 2000) else {
        panic!("candidate")
    };
    *value
}
fn import(conn: &mut Connection, ns: &str, c: &Candidate) {
    let tx = conn.transaction().unwrap();
    let (attribution, mapping_id) = store::attribute_candidate(&tx, ns, c).unwrap();
    let mut item = store::CommitItem::from_candidate(c.clone(), attribution).unwrap();
    item.mapping_id = mapping_id;
    let settings = store::load_import_settings(&tx).unwrap();
    if store::load(&tx, &c.entry_key).unwrap().is_none() {
        store::insert(
            &tx,
            &store::NewLedgerEntry {
                entry_key: c.entry_key.clone(),
                namespace_id: ns.into(),
                session_hash: c.session_hash.clone(),
                message_hash: c.message_hash.clone(),
                requested_model: c.requested_model.clone(),
                event_at_ms: c.event_at_ms,
                normalized_hash: item.normalized_hash.clone(),
                state: store::LedgerState::PendingClassification,
                candidate_json: None,
                mapping_id: None,
                mapping_revision: settings.mapping_revision,
                now_ms: 2000,
            },
        )
        .unwrap();
    }
    store::resolve_replay(&tx, &item, ns, settings.mapping_revision, 2000).unwrap();
    tx.commit().unwrap();
}
fn native(conn: &mut Connection, id: &str, c: &Candidate) {
    let tx = conn.transaction().unwrap();
    tx.execute("INSERT INTO usage_requests(request_id,run_id,source,provider_id,route_id,provider_name,route_name,requested_model,upstream_model,started_at_ms,ended_at_ms,stream,outcome,created_at_ms,updated_at_ms) VALUES(?1,'capture','relay','p','r','p','r','m','m',100,200,0,'completed',100,200)",params![id]).unwrap();
    store::record_proxy_identity(
        &tx,
        id,
        &crate::stats::event::WorkbuddyIdentity {
            session_hash: c.session_hash.clone(),
            message_hash: c.message_hash.clone(),
        },
        2000,
    )
    .unwrap();
    tx.commit().unwrap();
}
fn count(conn: &Connection, sql: &str) -> i64 {
    conn.query_row(sql, [], |r| r.get(0)).unwrap()
}
#[test]
fn custom_logs_import_automatically_without_creator_attribution() {
    let (_dir, mut conn, ns) = database();
    let c = candidate(&ns, "s", "m", "custom-local:whatever");
    import(&mut conn, &ns, &c);
    assert_eq!(
        count(
            &conn,
            "SELECT COUNT(*) FROM usage_requests WHERE record_origin='workbuddy_custom' AND http_status IS NULL AND duration_ms IS NULL AND run_id IS NULL"
        ),
        1
    );
    assert_eq!(
        count(&conn, "SELECT SUM(total_tokens) FROM usage_requests"),
        120
    );
    assert_eq!(
        count(
            &conn,
            "SELECT SUM(workbuddy_credit_nanos) FROM usage_requests"
        ),
        1_140_000_000
    );
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM usage_costs"), 0);
    let tx = conn.transaction().unwrap();
    store::insert_mapping(
        &tx,
        &store::NewMapping {
            mapping_id: "legacy-proxy-rule".into(),
            namespace_id: ns.clone(),
            requested_model: "custom-local:whatever".into(),
            classification: store::MappingClass::Proxy,
            valid_from_ms: 0,
            valid_to_ms: None,
            evidence: store::MappingEvidence::Manual,
            state: store::MappingState::Verified,
            route_id: None,
            provider_id: None,
            now_ms: 2000,
        },
    )
    .unwrap();
    assert_eq!(
        store::attribute_candidate(&tx, &ns, &c).unwrap(),
        (Attribution::Direct, None)
    );
    tx.commit().unwrap();
    import(&mut conn, &ns, &c);
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM usage_requests"), 1);
}
#[test]
fn exact_native_identity_wins_in_both_arrival_orders_and_after_restart() {
    for native_first in [true, false] {
        let (dir, mut conn, ns) = database();
        let c = candidate(&ns, "s", "m", "custom-local:model");
        if native_first {
            native(&mut conn, "native-original", &c);
        }
        import(&mut conn, &ns, &c);
        if !native_first {
            native(&mut conn, "native-original", &c);
        }
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM usage_requests"), 1);
        assert_eq!(
            count(
                &conn,
                "SELECT COUNT(*) FROM usage_requests WHERE request_id='native-original' AND record_origin='proxy'"
            ),
            1
        );
        assert_eq!(
            count(
                &conn,
                "SELECT COUNT(*) FROM workbuddy_import_ledger WHERE state='excluded_proxy'"
            ),
            1
        );
        drop(conn);
        let mut conn = Connection::open(dir.path().join("usage.sqlite3")).unwrap();
        crate::stats::store::migrate(&mut conn, dir.path()).unwrap();
        import(&mut conn, &ns, &c);
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM usage_requests"), 1);
    }
}
#[test]
fn same_model_time_and_tokens_are_not_a_duplicate_without_exact_identity() {
    let (_dir, mut conn, ns) = database();
    let a = candidate(&ns, "s", "m", "custom-local:model");
    native(&mut conn, "native-original", &a);
    for c in [
        candidate(&ns, "other-session", "m", "custom-local:model"),
        candidate(&ns, "s", "other-message", "custom-local:model"),
    ] {
        import(&mut conn, &ns, &c);
    }
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM usage_requests"), 3);
}
#[test]
fn migration_style_exclusion_is_reread_and_tombstones_are_terminal() {
    let (_dir, mut conn, ns) = database();
    let c = candidate(&ns, "s", "m", "custom-local:model");
    import(&mut conn, &ns, &c);
    let tx = conn.transaction().unwrap();
    store::withdraw_workbuddy_fact(&tx, &c.entry_key).unwrap();
    store::set_state(
        &tx,
        &c.entry_key,
        store::LedgerState::ExcludedProxy,
        None,
        2000,
    )
    .unwrap();
    tx.commit().unwrap();
    let tx = conn.transaction().unwrap();
    assert_eq!(reconcile::catch_up(&tx, 0, 2000).unwrap(), 1);
    tx.commit().unwrap();
    import(&mut conn, &ns, &c);
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM usage_requests"), 1);
    let tx = conn.transaction().unwrap();
    store::withdraw_workbuddy_fact(&tx, &c.entry_key).unwrap();
    store::set_state(
        &tx,
        &c.entry_key,
        store::LedgerState::DeletedManual,
        None,
        2000,
    )
    .unwrap();
    tx.commit().unwrap();
    import(&mut conn, &ns, &c);
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM usage_requests"), 0);
}
#[test]
fn retired_manual_commands_cannot_reintroduce_source_classification() {
    let (_dir, mut conn, _ns) = database();
    for command in [
        "stats_workbuddy_mapping_preview",
        "stats_workbuddy_mapping_apply",
        "stats_workbuddy_mapping_revoke_preview",
        "stats_workbuddy_mapping_revoke",
    ] {
        assert!(
            super::super::handle_command(
                &mut conn,
                command,
                &json!({"model_id":"custom-local:model","classification":"proxy","from_ms":0}),
                2000,
                &mut super::PreviewCache::default()
            )
            .is_err()
        );
    }
    assert_eq!(
        count(&conn, "SELECT COUNT(*) FROM workbuddy_model_history"),
        0
    );
}

#[test]
fn native_precedence_is_bounded_and_resumes_after_restart() {
    let (folder, mut conn, _) = database();
    let mut original = None;
    for index in 0..130 {
        let ns = format!("{index:048x}");
        let c = candidate(
            &ns,
            "shared-session",
            "shared-message",
            "custom-local:model",
        );
        import(&mut conn, &ns, &c);
        original = Some(c);
    }
    native(&mut conn, "native-original", &original.unwrap());
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM usage_requests"), 3);
    assert_eq!(
        count(
            &conn,
            "SELECT COUNT(*) FROM workbuddy_proxy_identity WHERE pending_dedup=1"
        ),
        1
    );
    drop(conn);
    let mut conn = Connection::open(folder.path().join("usage.sqlite3")).unwrap();
    crate::stats::store::migrate(&mut conn, folder.path()).unwrap();
    let tx = conn.transaction().unwrap();
    assert_eq!(store::resume_proxy_dedup(&tx, 2000).unwrap(), 2);
    tx.commit().unwrap();
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM usage_requests"), 1);
    assert_eq!(
        count(
            &conn,
            "SELECT COUNT(*) FROM workbuddy_proxy_identity WHERE pending_dedup=1"
        ),
        0
    );
    assert_eq!(
        count(
            &conn,
            "SELECT COUNT(*) FROM workbuddy_import_ledger WHERE state='excluded_proxy'"
        ),
        130
    );
}
