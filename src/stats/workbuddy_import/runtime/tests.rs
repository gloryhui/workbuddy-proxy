//! Real-thread, isolated SQLite/API regressions for production import wiring.

use super::*;
use crate::stats::{StatsHandle, dto::HealthState};
use serde_json::json;
use std::io::Write;

struct Running(StatsHandle);
impl Drop for Running {
    fn drop(&mut self) {
        self.0.shutdown();
    }
}

async fn command(handle: &StatsHandle, payload: serde_json::Value) -> serde_json::Value {
    serde_json::from_str(&handle.command(&payload).await.unwrap()).unwrap()
}

async fn start(config: &Path, root: &Path, retention: i64) -> Running {
    std::fs::create_dir_all(config).unwrap();
    let handle =
        StatsHandle::start_with_import_root(config.into(), Some(reader::ReaderRoot::new(root)));
    let deadline = Instant::now() + Duration::from_secs(5);
    while handle.state() == HealthState::Migrating && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(handle.state(), HealthState::Ready);
    let settings = command(&handle, json!({"command":"stats_settings_get"})).await;
    let settings = command(
        &handle,
        json!({"command":"stats_settings_update","enabled":true,
            "retention_days":retention,"expected_revision":settings["revision"]}),
    )
    .await;
    command(
        &handle,
        json!({"command":"stats_workbuddy_settings_update","enabled":true,
            "expected_revision":settings["revision"]}),
    )
    .await;
    Running(handle)
}

fn line(session: &str, model: &str, at: i64) -> String {
    json!({"type":"function_call","sessionId":session,"timestamp":at,
            "content":"SYNTHETIC_PROMPT_MUST_NOT_PERSIST",
            "providerData":{"messageId":"response","requestModelId":model,"model":model,
                "rawUsage":{"prompt_tokens":100,"completion_tokens":20,"total_tokens":120,"credit":1.14}}}).to_string()
}

fn number(config: &Path, sql: &str) -> i64 {
    Connection::open_with_flags(
        config.join("usage.sqlite3"),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap()
    .query_row(sql, [], |row| row.get(0))
    .unwrap()
}

#[test]
fn progress_total_is_known_after_discovery_before_backfill_finishes() {
    let shared = Shared::default();
    let input = json!({"effective_enabled":true,"complete":false,
        "progress":{"files_remaining":1,"bytes_remaining":100,"total_known":false}})
    .to_string();
    let status = |shared: &Shared| -> serde_json::Value {
        serde_json::from_str(&shared.publish_status(&input).unwrap()).unwrap()
    };
    assert_eq!(status(&shared)["progress"]["total_known"], false);
    {
        let mut progress = shared.progress.lock().unwrap();
        progress.root_found = true;
        progress.discovery_complete = true;
        // There has not yet been a complete incremental pass.
        assert!(progress.last_incremental_at_ms.is_none());
    }
    assert_eq!(status(&shared)["progress"]["total_known"], true);
    assert_eq!(status(&shared)["complete"], false);
    shared.progress.lock().unwrap().discovering = true;
    assert_eq!(status(&shared)["progress"]["total_known"], false);
}

#[tokio::test]
async fn idle_worker_waits_for_work_instead_of_requerying_sqlite_forty_times_a_second() {
    let folder = tempfile::tempdir().unwrap();
    let config = folder.path().join("config");
    let root = folder.path().join("source");
    let project = root.join("projects/idle");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(
        project.join("idle.jsonl"),
        line("idle", "hy3", now_ms()) + "\n",
    )
    .unwrap();
    let running = start(&config, &root, 0).await;
    wait_number(&config, "SELECT COUNT(*) FROM usage_requests", 1).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    let shared = &running.0.inner.import_runtime;
    let before = shared.idle_snapshot_reads.load(Ordering::Relaxed);
    tokio::time::sleep(Duration::from_millis(250)).await;
    let after = shared.idle_snapshot_reads.load(Ordering::Relaxed);
    assert!(
        after - before <= 1,
        "idle SQLite snapshots were reread {} times in 250ms",
        after - before
    );
    let requested = Instant::now();
    shared.request_scan("incremental").unwrap();
    while !shared
        .progress
        .lock()
        .unwrap()
        .scan
        .as_ref()
        .is_some_and(|task| task.state == ScanState::Completed)
    {
        assert!(requested.elapsed() < Duration::from_secs(1));
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

async fn wait_number(config: &Path, sql: &str, expected: i64) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if number(config, sql) == expected {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert_eq!(number(config, sql), expected, "{sql}");
}

#[tokio::test]
async fn real_worker_reaches_all_files_and_preserves_usage_without_idle_body_reads() {
    let folder = tempfile::tempdir().unwrap();
    let root = folder.path().join("workbuddy");
    let project = root.join("projects/轮转 project");
    std::fs::create_dir_all(&project).unwrap();
    for index in 0..110 {
        let session = format!("session-{index}");
        std::fs::write(
            project.join(format!("{session}.jsonl")),
            line(&session, "hy3", now_ms()) + "\n",
        )
        .unwrap();
    }
    let config = folder.path().join("config");
    let running = start(&config, &root, 0).await;
    wait_number(&config, "SELECT COUNT(*) FROM usage_requests", 110).await;
    assert_eq!(
        number(&config, "SELECT COUNT(*) FROM workbuddy_import_files"),
        110
    );
    assert_eq!(
        number(&config, "SELECT SUM(total_tokens) FROM usage_requests"),
        13_200
    );
    assert_eq!(
        number(
            &config,
            "SELECT SUM(workbuddy_credit_nanos) FROM usage_requests"
        ),
        125_400_000_000
    );
    let page = command(
        &running.0,
        json!({"command":"stats_requests","page_size":20,
            "filter":{"from_ms":now_ms()-86_400_000,"to_ms":now_ms()+86_400_000,"timezone":"UTC"}}),
    )
    .await;
    let row = &page["items"][0];
    assert_eq!(row["total_tokens"], 120);
    assert_eq!(row["workbuddy_credit_nanos"], "1140000000");
    assert_eq!(row["source"], "workbuddy_import");
    assert_eq!(row["usage_source"], "workbuddy_jsonl");
    assert_eq!(row["outcome"], "recorded");
    assert!(row["stream"].is_null());
    assert!(row["upstream_protocol"].is_null());
    let detail = command(
        &running.0,
        json!({"command":"stats_request_detail","request_id":row["request_id"]}),
    )
    .await;
    assert_eq!(detail["request"]["workbuddy_credit_nanos"], "1140000000");
    assert!(detail["price_snapshot"].is_null());
    let dashboard = command(
        &running.0,
        json!({"command":"stats_dashboard",
            "filter":{"from_ms":now_ms()-86_400_000,"to_ms":now_ms()+86_400_000,"timezone":"UTC"}}),
    )
    .await;
    assert_eq!(dashboard["summary"]["total_tokens"], 13_200);
    let series = dashboard["series"].as_array().unwrap();
    assert_eq!(
        series
            .iter()
            .filter_map(|point| point["total_tokens"].as_i64())
            .sum::<i64>(),
        13_200,
        "trend and overview must contain the same imported usage"
    );
    assert_eq!(
        series
            .iter()
            .filter_map(|point| point["request_count"].as_i64())
            .sum::<i64>(),
        110
    );

    assert_eq!(
        dashboard["summary"]["workbuddy_credit_nanos"],
        "125400000000"
    );
    assert!(dashboard["summary"]["success_rate"].is_null());
    assert_eq!(dashboard["summary"]["unpriced_request_count"], 110);
    assert_eq!(dashboard["summary"]["finished_count"], 0);
    assert_eq!(dashboard["summary"]["usage_eligible_count"], 110);
    let breakdown = command(
        &running.0,
        json!({"command":"stats_breakdown","group_by":"provider",
            "sort":"total_tokens","limit":20,"filter":{"from_ms":now_ms()-86_400_000,
                "to_ms":now_ms()+86_400_000,"timezone":"UTC"}}),
    )
    .await;
    let group = &breakdown["items"][0]["summary"];
    assert_eq!(group["workbuddy_credit_nanos"], "125400000000");
    assert_eq!(group["workbuddy_record_count"], 110);
    assert_eq!(group["unpriced_request_count"], 110);
    assert_eq!(group["total_tokens"], 13_200);
    tokio::time::sleep(Duration::from_millis(250)).await;
    let read = running
        .0
        .inner
        .import_runtime
        .progress
        .lock()
        .unwrap()
        .body_bytes_read;
    tokio::time::sleep(reader::SYNC_INTERVAL + Duration::from_millis(300)).await;
    assert_eq!(
        running
            .0
            .inner
            .import_runtime
            .progress
            .lock()
            .unwrap()
            .body_bytes_read,
        read,
        "a complete idle discovery pass must not reread unchanged file bodies"
    );
    let started = Instant::now();
    drop(running);
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[tokio::test]
async fn real_worker_waits_for_newline_keeps_unlimited_history_and_stops_on_global_disable() {
    let folder = tempfile::tempdir().unwrap();
    let root = folder.path().join("workbuddy");
    let project = root.join("projects/demo");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(
        project.join("old.jsonl"),
        line("old", "hy3", now_ms() - 400 * 86_400_000) + "\n",
    )
    .unwrap();
    std::fs::write(
        project.join("custom.jsonl"),
        line("custom", "custom-local:uncertain", now_ms()) + "\n",
    )
    .unwrap();
    let tail = project.join("tail.jsonl");
    std::fs::write(&tail, line("tail", "hy3", now_ms())).unwrap();
    let config = folder.path().join("config");
    let running = start(&config, &root, 0).await;
    wait_number(&config, "SELECT COUNT(*) FROM workbuddy_import_files", 3).await;
    // Stat-only registration deliberately precedes source reads. Wait for the
    // two actual durable outcomes, never infer completion from the file count.
    wait_number(
        &config,
        "SELECT
        (SELECT COUNT(*) FROM usage_requests)=2 AND
        (SELECT COUNT(*) FROM workbuddy_import_ledger WHERE state='pending_classification')=0",
        1,
    )
    .await;
    assert_eq!(number(&config, "SELECT COUNT(*) FROM usage_requests"), 2);
    assert_eq!(
        number(
            &config,
            "SELECT COUNT(*) FROM workbuddy_import_ledger WHERE state='pending_classification'"
        ),
        0
    );
    assert_eq!(
        number(
            &config,
            "SELECT committed_offset FROM workbuddy_import_files WHERE relative_path LIKE '%/tail.jsonl'"
        ),
        0
    );
    let mut file = std::fs::OpenOptions::new().append(true).open(tail).unwrap();
    file.write_all(b"\n").unwrap();
    file.sync_all().unwrap();
    drop(file);
    wait_number(&config, "SELECT COUNT(*) FROM usage_requests", 2).await;
    let settings = command(&running.0, json!({"command":"stats_settings_get"})).await;
    command(
        &running.0,
        json!({"command":"stats_settings_update","enabled":false,
            "retention_days":0,"expected_revision":settings["revision"]}),
    )
    .await;
    std::fs::write(
        project.join("disabled.jsonl"),
        line("disabled", "hy3", now_ms()) + "\n",
    )
    .unwrap();
    let read = running
        .0
        .inner
        .import_runtime
        .progress
        .lock()
        .unwrap()
        .body_bytes_read;
    tokio::time::sleep(reader::SYNC_INTERVAL + Duration::from_millis(300)).await;
    assert_eq!(
        number(&config, "SELECT COUNT(*) FROM workbuddy_import_files"),
        3
    );
    assert_eq!(
        running
            .0
            .inner
            .import_runtime
            .progress
            .lock()
            .unwrap()
            .body_bytes_read,
        read
    );
    let status = command(&running.0, json!({"command":"stats_workbuddy_status"})).await;
    assert_eq!(status["effective_enabled"], false);
    assert_eq!(status["complete"], false);
    assert_eq!(status["state"], "paused");
}

#[test]
fn manual_scan_requests_are_coalesced_with_one_stable_id() {
    let shared = Shared::default();
    shared.configure(true);
    let first: serde_json::Value =
        serde_json::from_str(&shared.request_scan("full").unwrap()).unwrap();
    for _ in 0..20 {
        let repeated: serde_json::Value =
            serde_json::from_str(&shared.request_scan("full").unwrap()).unwrap();
        assert_eq!(repeated["scan_id"], first["scan_id"]);
    }
    assert!(shared.progress.lock().unwrap().requested);
    shared.configure(false);
    assert_eq!(
        shared.request_scan("incremental").unwrap_err(),
        "stats_workbuddy_import_disabled"
    );
}

#[tokio::test]
async fn full_scan_rechecks_a_hidden_middle_edit_without_resetting_incremental_offset() {
    let folder = tempfile::tempdir().unwrap();
    let root = folder.path().join("workbuddy");
    let project = root.join("projects/full");
    std::fs::create_dir_all(&project).unwrap();
    let path = project.join("full.jsonl");
    let mut lines = Vec::new();
    for index in 0..100 {
        let mut value: serde_json::Value =
            serde_json::from_str(&line("full", "hy3", now_ms())).unwrap();
        value["providerData"]["messageId"] = format!("m{index}").into();
        lines.push(value.to_string());
    }
    std::fs::write(&path, lines.join("\n") + "\n").unwrap();
    let config = folder.path().join("config");
    let running = start(&config, &root, 0).await;
    wait_number(&config, "SELECT COUNT(*) FROM usage_requests", 100).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
    let offset = number(
        &config,
        "SELECT committed_offset FROM workbuddy_import_files",
    );
    lines[50] = lines[50]
        .replace("\"prompt_tokens\":100", "\"prompt_tokens\":101")
        .replace("\"total_tokens\":120", "\"total_tokens\":121");
    std::fs::write(&path, lines.join("\n") + "\n").unwrap();
    std::fs::File::open(&path)
        .unwrap()
        .set_modified(modified)
        .unwrap();
    assert_eq!(std::fs::metadata(&path).unwrap().len(), offset as u64);
    let scan = command(
        &running.0,
        json!({"command":"stats_workbuddy_scan","mode":"full"}),
    )
    .await;
    assert!(scan["scan_id"].as_str().is_some());
    wait_number(
        &config,
        "SELECT COUNT(*) FROM workbuddy_import_ledger WHERE state='conflict'",
        1,
    )
    .await;
    assert_eq!(number(&config, "SELECT COUNT(*) FROM usage_requests"), 99);
    assert_eq!(
        number(
            &config,
            "SELECT committed_offset FROM workbuddy_import_files"
        ),
        offset
    );
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let status = command(&running.0, json!({"command":"stats_workbuddy_status"})).await;
        if status["scan_state"] == "completed" {
            assert_eq!(status["scan_id"], scan["scan_id"]);
            assert_eq!(status["root_found"], true);
            assert!(
                status["last_full_scan_at_ms"]
                    .as_i64()
                    .is_some_and(|value| value > 0)
            );
            assert_eq!(
                status["complete"], false,
                "the unresolved conflict must remain visible"
            );
            break;
        }
        assert!(
            Instant::now() < deadline,
            "full scan did not complete: {status}"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

mod sync_tests;

mod metrics_tests;

#[tokio::test]
async fn report_guard_defers_all_import_writes_then_discovery_resumes() {
    let folder = tempfile::tempdir().unwrap();
    let root = folder.path().join("workbuddy");
    let project = root.join("projects/report-admission");
    let config = folder.path().join("config");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::create_dir_all(&config).unwrap();
    std::fs::write(
        project.join("one.jsonl"),
        line("one", "hy3", now_ms()) + "\n",
    )
    .unwrap();
    let running = Running(StatsHandle::start_with_import_root(
        config.clone(),
        Some(reader::ReaderRoot::new(&root)),
    ));
    let deadline = Instant::now() + Duration::from_secs(5);
    while running.0.state() == HealthState::Migrating && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(running.0.state(), HealthState::Ready);
    let inner = running.0.inner.clone();
    let report = inner.report_reads.enter();
    let settings = command(&running.0, json!({"command":"stats_settings_get"})).await;
    let settings = command(
        &running.0,
        json!({"command":"stats_settings_update",
        "enabled":true,"retention_days":0,"expected_revision":settings["revision"]}),
    )
    .await;
    command(
        &running.0,
        json!({"command":"stats_workbuddy_settings_update",
        "enabled":true,"expected_revision":settings["revision"]}),
    )
    .await;
    // Every import transaction, including checkpoint metadata, waits for the
    // report. The real worker must resume discovery once that report exits.
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(
        number(&config, "SELECT COUNT(*) FROM workbuddy_import_files"),
        0
    );
    assert_eq!(number(&config, "SELECT COUNT(*) FROM usage_requests"), 0);
    assert!(inner.report_reads.import_reserved());
    drop(report);
    wait_number(&config, "SELECT COUNT(*) FROM workbuddy_import_files", 1).await;
    wait_number(&config, "SELECT COUNT(*) FROM usage_requests", 1).await;
    assert_eq!(
        number(&config, "SELECT SUM(total_tokens) FROM usage_requests"),
        120
    );
    assert_eq!(
        number(
            &config,
            "SELECT SUM(workbuddy_credit_nanos) FROM usage_requests"
        ),
        1_140_000_000
    );
}
