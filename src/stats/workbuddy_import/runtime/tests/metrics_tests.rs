//! A failed writer transaction must not advance the source commit receipt.
use super::*;

#[tokio::test]
async fn monotonic_source_receipt_tracks_only_committed_offsets_and_survives_rollback() {
    let folder = tempfile::tempdir().unwrap();
    let config = folder.path().join("config");
    let root = folder.path().join("workbuddy");
    let project = root.join("projects/demo");
    std::fs::create_dir_all(&project).unwrap();
    let path = project.join("receipt.jsonl");
    let first = line("receipt", "hy3", now_ms()) + "\n";
    std::fs::write(&path, &first).unwrap();
    let running = start(&config, &root, 0).await;
    wait_number(&config, "SELECT COUNT(*) FROM usage_requests", 1).await;
    let first_status = command(&running.0, json!({"command":"stats_workbuddy_status"})).await;
    assert!(
        !first_status["monotonic_clock_id"]
            .as_str()
            .unwrap()
            .is_empty()
    );
    assert_eq!(first_status["last_source_commit"]["offset"], first.len());
    let initial = &first_status["runtime_metrics"];
    assert!(
        initial["queue_peak"]
            .as_u64()
            .is_some_and(|n| (1..=2).contains(&n)),
        "must measure actual enqueued messages, not publish channel capacity"
    );
    assert!(initial["writer_batch_count"].as_u64().unwrap() >= 1);
    assert_eq!(initial["writer_batch_failures"], 0);
    assert!(initial["writer_batch_max_us"].as_u64().unwrap() > 0);
    assert!(initial["source_io_thread_checks"].as_u64().unwrap() > 0);
    assert_eq!(initial["source_io_thread_violations"], 0);
    let first_receipt = first_status["last_source_commit"].clone();
    let mut row: serde_json::Value = serde_json::from_str(first.trim()).unwrap();
    row["providerData"]["messageId"] = "second".into();
    row["providerData"]["model"] = "reject-model".into();
    let conn = Connection::open(config.join("usage.sqlite3")).unwrap();
    conn.execute_batch("CREATE TRIGGER reject_receipt BEFORE INSERT ON usage_requests
        WHEN NEW.upstream_model='reject-model' BEGIN SELECT RAISE(ABORT,'synthetic rollback'); END;").unwrap();
    let second = row.to_string() + "\n";
    std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(second.as_bytes())
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let status = command(&running.0, json!({"command":"stats_workbuddy_status"})).await;
        if !status["last_error_code"].is_null() {
            break;
        }
        assert!(Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    let failed = command(&running.0, json!({"command":"stats_workbuddy_status"})).await;
    assert!(
        failed["runtime_metrics"]["writer_batch_failures"]
            .as_u64()
            .unwrap()
            >= 1
    );
    assert!(
        failed["runtime_metrics"]["recent_batches"]
            .as_array()
            .unwrap()
            .iter()
            .any(|batch| batch["committed"] == false && batch["candidates"] == 1)
    );
    assert_eq!(failed["last_source_commit"], first_receipt);
    assert_eq!(
        failed["source_commit_receipts"],
        first_status["source_commit_receipts"]
    );
    assert_eq!(
        failed["monotonic_clock_id"],
        first_status["monotonic_clock_id"]
    );
    assert!(nanos(&failed["monotonic_elapsed_ns"]) > nanos(&first_status["monotonic_elapsed_ns"]));
    assert_eq!(number(&config, "SELECT COUNT(*) FROM usage_requests"), 1);
    conn.execute_batch("DROP TRIGGER reject_receipt").unwrap();
    wait_number(&config, "SELECT COUNT(*) FROM usage_requests", 2).await;
    let recovered = command(&running.0, json!({"command":"stats_workbuddy_status"})).await;
    assert_eq!(
        recovered["last_source_commit"]["offset"],
        first.len() + second.len()
    );
    let observed = &recovered["runtime_metrics"];
    assert!(
        observed["writer_batch_count"].as_u64().unwrap()
            > initial["writer_batch_count"].as_u64().unwrap()
    );
    assert!(
        observed["recent_batches"]
            .as_array()
            .unwrap()
            .iter()
            .any(|batch| batch["committed"] == true && batch["candidates"] == 1)
    );
    assert_eq!(observed["source_io_thread_violations"], 0);
    let committed = nanos(&recovered["last_source_commit"]["commit_ack_elapsed_ns"]);
    assert!(committed > nanos(&first_receipt["commit_ack_elapsed_ns"]));
    assert!(committed <= nanos(&recovered["monotonic_elapsed_ns"]));
}

fn nanos(value: &serde_json::Value) -> u64 {
    value.as_str().unwrap().parse().unwrap()
}
