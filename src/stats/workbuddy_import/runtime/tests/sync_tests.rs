//! Actual production wrapper plus worker/writer, isolated from user profiles.
use super::*;
use crate::{config::Config, proxy::Bridge};

async fn bridge(config: &Path, root: &Path) -> (Bridge, Running, String) {
    std::fs::create_dir_all(config).unwrap();
    std::fs::create_dir_all(root.join("projects/demo")).unwrap();
    let mut cfg = Config::default();
    cfg.providers[0].name = "synthetic".into();
    cfg.providers[0].upstream = "https://example.invalid/v1".into();
    cfg.providers[0].api_key = "SYNTHETIC_UPSTREAM_SECRET".into();
    cfg.providers[0].set_models("model-one", "default").unwrap();
    let alias = cfg.workbuddy_connections().unwrap()[0].alias.clone();
    let mut bridge = Bridge::new(cfg, config.into()).unwrap();
    bridge.stats.shutdown();
    let running = start(config, root, 0).await;
    bridge.stats = running.0.clone();
    (bridge, running, format!("custom-local:{alias}"))
}

#[tokio::test]
async fn production_sync_imports_custom_logs_when_no_matching_native_fact_exists() {
    let folder = tempfile::tempdir().unwrap();
    let config = folder.path().join("config");
    let root = folder.path().join("workbuddy");
    let (bridge, _running, model) = bridge(&config, &root).await;
    let outcome = bridge
        .sync_workbuddy_with_history(&root.join("models.json"), false)
        .await
        .unwrap();
    assert!(outcome.is_confirmed(), "{outcome:?}");
    tokio::time::sleep(Duration::from_millis(2)).await;
    std::fs::write(
        root.join("projects/demo/mirror.jsonl"),
        line("mirror", &model, now_ms()) + "\n",
    )
    .unwrap();
    wait_number(
        &config,
        "SELECT COUNT(*) FROM workbuddy_import_ledger WHERE state='imported'",
        1,
    )
    .await;
    assert_eq!(number(&config, "SELECT COUNT(*) FROM usage_requests"), 1);
    assert!(
        number(
            &config,
            "SELECT COUNT(*) FROM workbuddy_model_history WHERE evidence='managed_sync' AND state='verified'"
        ) > 0
    );
    let text = std::fs::read(config.join("usage.sqlite3")).unwrap();
    assert!(!String::from_utf8_lossy(&text).contains("SYNTHETIC_UPSTREAM_SECRET"));
    assert!(
        bridge
            .sync_workbuddy_with_history(&root.join("models.json"), true)
            .await
            .unwrap()
            .is_confirmed()
    );
    tokio::time::sleep(Duration::from_millis(2)).await;
    std::fs::write(
        root.join("projects/demo/after.jsonl"),
        line("after", &model, now_ms()) + "\n",
    )
    .unwrap();
    wait_number(
        &config,
        "SELECT COUNT(*) FROM workbuddy_import_ledger WHERE state='imported'",
        2,
    )
    .await;
    assert_eq!(number(&config, "SELECT COUNT(*) FROM usage_requests"), 2);
}

#[tokio::test]
async fn production_prepare_failure_changes_neither_models_nor_proxy_config() {
    let folder = tempfile::tempdir().unwrap();
    let config = folder.path().join("config");
    let root = folder.path().join("workbuddy");
    let (bridge, running, _) = bridge(&config, &root).await;
    let path = root.join("models.json");
    let original = b"[{\"id\":\"user-owned\",\"extra\":42}]";
    std::fs::write(&path, original).unwrap();
    let saved = serde_json::to_vec(&*bridge.config.lock().unwrap()).unwrap();
    running.0.shutdown();
    assert!(
        bridge
            .sync_workbuddy_with_history(&path, false)
            .await
            .is_err()
    );
    assert_eq!(std::fs::read(&path).unwrap(), original);
    assert_eq!(
        serde_json::to_vec(&*bridge.config.lock().unwrap()).unwrap(),
        saved
    );
    assert!(!config.join("config.json").exists());
    assert_eq!(
        std::fs::read_dir(&root).unwrap().count(),
        2,
        "no backup or profile replacement"
    );
}

#[tokio::test]
async fn production_finalize_write_failure_returns_synced_unconfirmed() {
    let folder = tempfile::tempdir().unwrap();
    let config = folder.path().join("config");
    let root = folder.path().join("workbuddy");
    let (bridge, _running, _) = bridge(&config, &root).await;
    // A test-only SQLite trigger injects a real writer transaction failure at
    // finalize. Production never opens this second writing connection.
    let conn = Connection::open(config.join("usage.sqlite3")).unwrap();
    conn.execute_batch(
        "CREATE TRIGGER inject_finalize_failure BEFORE UPDATE ON workbuddy_model_history
        WHEN OLD.state='prepared' AND NEW.state='conflict'
        BEGIN SELECT RAISE(ABORT,'injected finalize failure'); END;",
    )
    .unwrap();
    let outcome = bridge
        .sync_workbuddy_with_history(&root.join("models.json"), false)
        .await
        .unwrap();
    assert_eq!(outcome.error_code(), Some("attribution_unconfirmed"));
    assert_eq!(outcome.changed(), 1);
    assert!(root.join("models.json").exists());
    assert!(
        number(
            &config,
            "SELECT COUNT(*) FROM workbuddy_model_history WHERE state='prepared'"
        ) > 0
    );
    conn.execute_batch("DROP TRIGGER inject_finalize_failure")
        .unwrap();
    wait_number(
        &config,
        "SELECT COUNT(*) FROM workbuddy_model_history WHERE state='prepared'",
        0,
    )
    .await;
    assert!(
        number(
            &config,
            "SELECT COUNT(*) FROM workbuddy_model_history WHERE state='verified'"
        ) > 0
    );
}

#[tokio::test]
async fn observer_model_ownership_does_not_suppress_old_or_new_custom_logs() {
    let folder = tempfile::tempdir().unwrap();
    let root = folder.path().join("workbuddy");
    let config = folder.path().join("config");
    std::fs::create_dir_all(root.join("projects/demo")).unwrap();
    crate::config::write_json(&root.join("models.json"), &json!([
        {"id":"supplier/model","model":"different-upstream","_workbuddyProxy":{"managed":true,"route":"route"}}
    ])).unwrap();
    std::fs::write(
        root.join("projects/demo/old.jsonl"),
        line("old", "custom-local:supplier/model", now_ms() - 60_000) + "\n",
    )
    .unwrap();
    let running = start(&config, &root, 0).await;
    wait_number(
        &config,
        "SELECT COUNT(*) FROM workbuddy_import_ledger WHERE state='imported'",
        1,
    )
    .await;
    tokio::time::sleep(Duration::from_millis(2)).await;
    std::fs::write(
        root.join("projects/demo/new.jsonl"),
        line("new", "custom-local:supplier/model", now_ms()) + "\n",
    )
    .unwrap();
    wait_number(
        &config,
        "SELECT COUNT(*) FROM workbuddy_import_ledger WHERE state='imported'",
        2,
    )
    .await;
    assert_eq!(number(&config, "SELECT COUNT(*) FROM usage_requests"), 2);
    let status = command(&running.0, json!({"command":"stats_workbuddy_status"})).await;
    assert_eq!(status["effective_enabled"], true);
}

#[tokio::test]
async fn live_sync_guard_prevents_background_recovery_until_caller_releases_it() {
    use crate::stats::workbuddy_import::observation::ManagedModel;
    let folder = tempfile::tempdir().unwrap();
    let config = folder.path().join("config");
    let root = folder.path().join("workbuddy");
    let (_bridge, running, model) = bridge(&config, &root).await;
    let session = running
        .0
        .prepare_workbuddy_sync(vec![ManagedModel {
            requested_model: model.clone(),
            route_id: Some("route".into()),
        }])
        .await
        .unwrap();
    crate::config::write_json(&root.join("models.json"), &json!([
        {"id":model.strip_prefix("custom-local:").unwrap(),"_workbuddyProxy":{"managed":true,"route":"route"}}
    ])).unwrap();
    tokio::time::sleep(Duration::from_millis(5_200)).await;
    let shared = &running.0.inner.import_runtime;
    let before = shared.idle_snapshot_reads.load(Ordering::Relaxed);
    tokio::time::sleep(Duration::from_millis(250)).await;
    assert!(
        shared.idle_snapshot_reads.load(Ordering::Relaxed) - before <= 1,
        "a deferred observation must not spin while the sync lease is live"
    );
    assert_eq!(
        number(
            &config,
            "SELECT COUNT(*) FROM workbuddy_model_history WHERE state='prepared'"
        ),
        1
    );
    assert_eq!(
        number(
            &config,
            "SELECT COUNT(*) FROM workbuddy_model_history WHERE state='verified'"
        ),
        0
    );
    drop(session);
    wait_number(
        &config,
        "SELECT COUNT(*) FROM workbuddy_model_history WHERE state='prepared'",
        0,
    )
    .await;
    assert_eq!(
        number(
            &config,
            "SELECT COUNT(*) FROM workbuddy_model_history WHERE state='verified'"
        ),
        1
    );
}

#[tokio::test]
async fn disabled_import_keeps_legacy_sync_available_when_statistics_writer_stops() {
    let folder = tempfile::tempdir().unwrap();
    let config = folder.path().join("config");
    let root = folder.path().join("workbuddy");
    let (bridge, running, _) = bridge(&config, &root).await;
    let settings = command(&running.0, json!({"command":"stats_workbuddy_status"})).await;
    command(
        &running.0,
        json!({"command":"stats_workbuddy_settings_update","enabled":false,
        "expected_revision":settings["settings_revision"]}),
    )
    .await;
    running.0.shutdown();
    let outcome = bridge
        .sync_workbuddy_with_history(&root.join("models.json"), false)
        .await
        .unwrap();
    assert!(outcome.is_confirmed());
    assert!(root.join("models.json").exists());
}

#[tokio::test]
async fn corrupt_profile_is_reported_and_never_treated_as_empty_or_successful() {
    let folder = tempfile::tempdir().unwrap();
    let config = folder.path().join("config");
    let root = folder.path().join("workbuddy");
    std::fs::create_dir_all(root.join("projects/demo")).unwrap();
    std::fs::write(root.join("models.json"), b"invalid JSON").unwrap();
    std::fs::write(
        root.join("projects/demo/one.jsonl"),
        line("one", "hy3", now_ms()) + "\n",
    )
    .unwrap();
    let running = start(&config, &root, 0).await;
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let status = command(&running.0, json!({"command":"stats_workbuddy_status"})).await;
        if !status["last_error_code"].is_null() {
            assert_eq!(
                status["last_error_code"],
                "stats_workbuddy_unsupported_format"
            );
            assert_eq!(status["complete"], false);
            break;
        }
        assert!(Instant::now() < deadline, "missing profile error: {status}");
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert_eq!(number(&config, "SELECT COUNT(*) FROM usage_requests"), 0);
    crate::config::write_json(&root.join("models.json"), &json!([])).unwrap();
    command(
        &running.0,
        json!({"command":"stats_workbuddy_scan","mode":"full"}),
    )
    .await;
    wait_number(&config, "SELECT COUNT(*) FROM usage_requests", 1).await;
}

#[tokio::test]
async fn managed_sync_preserves_automatically_imported_custom_history() {
    let folder = tempfile::tempdir().unwrap();
    let config = folder.path().join("config");
    let root = folder.path().join("workbuddy");
    let (bridge, _running, model) = bridge(&config, &root).await;
    std::fs::write(
        root.join("projects/demo/future.jsonl"),
        line("future", &model, now_ms()) + "\n",
    )
    .unwrap();
    wait_number(
        &config,
        "SELECT COUNT(*) FROM usage_requests WHERE record_origin='workbuddy_custom'",
        1,
    )
    .await;
    assert!(
        bridge
            .sync_workbuddy_with_history(&root.join("models.json"), false)
            .await
            .unwrap()
            .is_confirmed()
    );
    assert_eq!(number(&config, "SELECT COUNT(*) FROM usage_requests"), 1);
}
#[tokio::test]
async fn automatic_custom_import_preserves_usage_and_restarts_without_confirmation() {
    let folder = tempfile::tempdir().unwrap();
    let config = folder.path().join("config");
    let root = folder.path().join("workbuddy");
    std::fs::create_dir_all(root.join("projects/demo")).unwrap();
    for i in 0..3 {
        let id = format!("auto-{i}");
        std::fs::write(
            root.join("projects/demo").join(format!("{id}.jsonl")),
            line(&id, "custom-local:model", now_ms()) + "\n",
        )
        .unwrap();
    }
    let running = start(&config, &root, 0).await;
    wait_number(
        &config,
        "SELECT COUNT(*) FROM usage_requests WHERE record_origin='workbuddy_custom'",
        3,
    )
    .await;
    assert_eq!(
        number(&config, "SELECT SUM(total_tokens) FROM usage_requests"),
        360
    );
    assert_eq!(
        number(
            &config,
            "SELECT SUM(workbuddy_credit_nanos) FROM usage_requests"
        ),
        3_420_000_000
    );
    drop(running);
    let _restarted = start(&config, &root, 0).await;
    wait_number(&config, "SELECT COUNT(*) FROM usage_requests", 3).await;
    assert_eq!(
        number(
            &config,
            "SELECT COUNT(*) FROM workbuddy_model_history WHERE evidence='manual'"
        ),
        0
    );
}
#[tokio::test]
async fn automatic_import_write_failure_preserves_checkpoint_and_retries() {
    let folder = tempfile::tempdir().unwrap();
    let config = folder.path().join("config");
    let root = folder.path().join("workbuddy");
    std::fs::create_dir_all(root.join("projects/demo")).unwrap();
    let path = root.join("projects/demo/fault.jsonl");
    std::fs::write(&path, "").unwrap();
    let running = start(&config, &root, 0).await;
    // Establish the checkpoint before injecting the failed transaction. A new
    // file's first batch may otherwise roll back its checkpoint creation too.
    wait_number(&config, "SELECT COUNT(*) FROM workbuddy_import_files", 1).await;
    let conn = Connection::open(config.join("usage.sqlite3")).unwrap();
    conn.execute_batch("CREATE TRIGGER inject_import_failure BEFORE INSERT ON usage_requests WHEN NEW.record_origin<>'proxy' BEGIN SELECT RAISE(ABORT,'injected'); END;").unwrap();
    std::fs::write(&path, line("fault", "custom-local:model", now_ms()) + "\n").unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let status = command(&running.0, json!({"command":"stats_workbuddy_status"})).await;
        if status["last_error_code"].is_string() {
            break;
        }
        assert!(Instant::now() < deadline, "writer failure not reported");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(number(&config, "SELECT COUNT(*) FROM usage_requests"), 0);
    assert_eq!(
        number(
            &config,
            "SELECT committed_offset FROM workbuddy_import_files"
        ),
        0
    );
    conn.execute_batch("DROP TRIGGER inject_import_failure")
        .unwrap();
    command(
        &running.0,
        json!({"command":"stats_workbuddy_scan","mode":"full"}),
    )
    .await;
    wait_number(&config, "SELECT COUNT(*) FROM usage_requests", 1).await;
}

#[tokio::test]
async fn status_reports_skipped_anomalies_without_counting_ordinary_ignored_messages() {
    let folder = tempfile::tempdir().unwrap();
    let config = folder.path().join("config");
    let root = folder.path().join("workbuddy");
    std::fs::create_dir_all(root.join("projects/demo")).unwrap();
    let bad_time = json!({"type":"function_call","sessionId":"errors","timestamp":-1,
        "providerData":{"messageId":"bad-time","requestModelId":"hy3","rawUsage":{"prompt_tokens":1}}});
    let missing_identity = json!({"type":"function_call","sessionId":"errors","timestamp":now_ms(),
        "providerData":{"requestModelId":"hy3","rawUsage":{"prompt_tokens":1}}});
    let body = format!(
        "{}\n{{broken}}\n{{}}\n{}\n{}\n",
        line("errors", "hy3", now_ms()),
        bad_time,
        missing_identity
    );
    let bytes = body.len();
    std::fs::write(root.join("projects/demo/errors.jsonl"), body).unwrap();
    let running = start(&config, &root, 0).await;
    wait_number(&config, "SELECT COUNT(*) FROM usage_requests", 1).await;
    let status = command(&running.0, json!({"command":"stats_workbuddy_status"})).await;
    assert_eq!(
        status["skipped"], 3,
        "syntax, timestamp and identity errors only"
    );
    assert_eq!(status["imported"], 1);
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let status = command(&running.0, json!({"command":"stats_workbuddy_status"})).await;
        if status["progress"]["files_remaining"] == 0 && status["progress"]["total_known"] == true {
            assert_eq!(status["progress"]["files_total"], 1);
            assert_eq!(status["progress"]["bytes_total"], bytes);
            assert_eq!(status["progress"]["bytes_committed"], bytes);
            assert_eq!(status["progress"]["bytes_remaining"], 0);
            break;
        }
        assert!(
            Instant::now() < deadline,
            "checkpoint progress did not converge: {status}"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}
