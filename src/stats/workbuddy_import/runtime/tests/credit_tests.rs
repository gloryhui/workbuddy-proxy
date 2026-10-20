use super::*;

#[tokio::test]
async fn upgrading_v9_fills_tool_name_without_recounting_or_changing_the_bill() {
    let folder = tempfile::tempdir().unwrap();
    let config = folder.path().join("config");
    let root = folder.path().join("workbuddy");
    let project = root.join("projects/tools");
    std::fs::create_dir_all(&project).unwrap();
    let at = now_ms();
    let body = format!(
        "{}\n{}\n",
        credit("generation-tool:ImageGen:first", at),
        credit("generation-tool:ImageGen:second", at)
    );
    std::fs::write(project.join("tools.jsonl"), &body).unwrap();
    let running = start(&config, &root, 0).await;
    wait_number(&config, "SELECT COUNT(*) FROM usage_requests", 2).await;
    drop(running);
    let db = Connection::open(config.join("usage.sqlite3")).unwrap();
    let ledger_before: Vec<(String, String)> = db
        .prepare("SELECT entry_key,normalized_hash FROM workbuddy_import_ledger ORDER BY entry_key")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    // Schema v9 persisted both fees, but did not retain their tool display names.
    db.execute(
        "UPDATE usage_requests SET route_name=requested_model,parser_version=2",
        [],
    )
    .unwrap();
    db.pragma_update(None, "user_version", 9).unwrap();
    drop(db);
    let running = start(&config, &root, 0).await;
    wait_number(
        &config,
        "SELECT COUNT(*) FROM usage_requests WHERE route_name='ImageGen'",
        2,
    )
    .await;
    assert_eq!(number(&config, "SELECT COUNT(*) FROM usage_requests"), 2);
    assert_eq!(
        number(
            &config,
            "SELECT SUM(workbuddy_credit_nanos) FROM usage_requests"
        ),
        11_420_000_000
    );
    assert_eq!(
        number(
            &config,
            "SELECT COUNT(*) FROM usage_requests WHERE total_tokens IS NOT NULL"
        ),
        0
    );
    let db = Connection::open(config.join("usage.sqlite3")).unwrap();
    let ledger_after: Vec<(String, String)> = db
        .prepare("SELECT entry_key,normalized_hash FROM workbuddy_import_ledger ORDER BY entry_key")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(ledger_before, ledger_after);
    drop(db);
    full_scan(&running).await;
    assert_eq!(
        number(
            &config,
            "SELECT COUNT(*) FROM usage_requests WHERE route_name='ImageGen'"
        ),
        2
    );
    assert_eq!(
        number(
            &config,
            "SELECT COUNT(*) FROM workbuddy_import_ledger WHERE state='conflict'"
        ),
        0
    );
    drop(running);
    let restarted = start(&config, &root, 0).await;
    full_scan(&restarted).await;
    assert_eq!(
        number(
            &config,
            "SELECT SUM(workbuddy_credit_nanos) FROM usage_requests"
        ),
        11_420_000_000
    );
}

fn credit(stage: &str, at: i64) -> String {
    json!({"type":"credit-usage","id":format!("history-{stage}"),"timestamp":at,
        "providerData":{"conversationRequestId":"request","messageId":"parent-response",
            "source":"generation_tool","stageId":stage,"credit":5.71}})
    .to_string()
}

async fn full_scan(running: &Running) {
    let scan = command(
        &running.0,
        json!({"command":"stats_workbuddy_scan","mode":"full"}),
    )
    .await;
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let status = command(&running.0, json!({"command":"stats_workbuddy_status"})).await;
        if status["scan_id"] == scan["scan_id"] && status["scan_state"] == "completed" {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "full credit rescan did not complete: {status}"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

#[tokio::test]
async fn a_v8_eof_checkpoint_backfills_3049_without_changing_model_tokens_or_replaying_charges() {
    let folder = tempfile::tempdir().unwrap();
    let config = folder.path().join("config");
    let root = folder.path().join("workbuddy");
    let project = root.join("projects/credit");
    std::fs::create_dir_all(&project).unwrap();
    let path = project.join("credit.jsonl");
    let at = now_ms();
    let model = json!({"type":"message","role":"assistant","sessionId":"credit","timestamp":at,
        "providerData":{"messageId":"model-response","requestModelId":"deepseek-v4.1-flash",
            "rawUsage":{"prompt_tokens":100,"completion_tokens":20,"credit":19.07}}})
    .to_string();
    std::fs::write(&path, format!("{model}\n")).unwrap();
    let running = start(&config, &root, 0).await;
    wait_number(&config, "SELECT COUNT(*) FROM usage_requests", 1).await;
    drop(running);

    let mut body = format!("{model}\n");
    for (index, stage) in ["image:first", "image:second"].iter().enumerate() {
        body.push_str(&credit(stage, at + index as i64 + 1));
        body.push('\n');
        body.push_str(&json!({"type":"function_call_result","timestamp":at,
            "providerData":{"messageId":format!("tool-{index}"),"requestModelId":"deepseek-v4.1-flash",
                "toolResult":{"rawResponse":{"type":"image_gen_tool_result","status":"completed","credit":5.71}}}}).to_string());
        body.push('\n');
    }
    std::fs::write(&path, &body).unwrap();
    // Simulate the old parser having reached EOF while ignoring credit stages.
    let fingerprint = reader::fingerprint(&path, body.len() as u64).unwrap();
    let modified = std::fs::metadata(&path)
        .unwrap()
        .modified()
        .unwrap()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    let conn = Connection::open(config.join("usage.sqlite3")).unwrap();
    conn.execute(
        "UPDATE workbuddy_import_files SET committed_offset=?1,observed_size=?1,
        prefix_hash=?2,prefix_len=?3,anchor_hash=?4,anchor_start=?5,anchor_len=?6,
        modified_ms=?7,last_completed_at_ms=?8,scan_offset=?1,scan_generation=generation",
        params![
            body.len() as i64,
            fingerprint.prefix_hash,
            fingerprint.prefix_len as i64,
            fingerprint.anchor_hash,
            fingerprint.anchor_start as i64,
            fingerprint.anchor_len as i64,
            modified,
            at
        ],
    )
    .unwrap();
    conn.execute(
        "UPDATE stats_settings SET workbuddy_last_full_scan_ms=?1",
        params![at],
    )
    .unwrap();
    conn.pragma_update(None, "user_version", 8).unwrap();
    drop(conn);

    let running = start(&config, &root, 0).await;
    wait_number(&config, "SELECT COUNT(*) FROM usage_requests", 3).await;
    assert_eq!(
        number(&config, "PRAGMA user_version"),
        crate::stats::store::SCHEMA_VERSION
    );
    assert_eq!(
        number(
            &config,
            "SELECT SUM(workbuddy_credit_nanos) FROM usage_requests"
        ),
        30_490_000_000
    );
    assert_eq!(
        number(&config, "SELECT SUM(total_tokens) FROM usage_requests"),
        120
    );
    assert_eq!(
        number(
            &config,
            "SELECT COUNT(*) FROM usage_requests WHERE requested_model='workbuddy-credit:generation_tool' AND total_tokens IS NULL AND record_origin='workbuddy_builtin'"
        ),
        2
    );
    assert_eq!(number(&config, "SELECT COUNT(*) FROM usage_costs"), 0);
    full_scan(&running).await;
    assert_eq!(number(&config, "SELECT COUNT(*) FROM usage_requests"), 3);

    // A copied file, changed history ids and repeated scans are not new bills.
    std::fs::write(
        project.join("copied.jsonl"),
        format!(
            "{}\n{}\n",
            credit("image:first", at + 20),
            credit("image:second", at + 21)
        ),
    )
    .unwrap();
    full_scan(&running).await;
    assert_eq!(
        number(
            &config,
            "SELECT SUM(workbuddy_credit_nanos) FROM usage_requests"
        ),
        30_490_000_000
    );
    drop(running);
    let restarted = start(&config, &root, 0).await;
    full_scan(&restarted).await;
    assert_eq!(number(&config, "SELECT COUNT(*) FROM usage_requests"), 3);
    assert_eq!(
        number(
            &config,
            "SELECT SUM(workbuddy_credit_nanos) FROM usage_requests"
        ),
        30_490_000_000
    );
}

#[tokio::test]
async fn upgrading_fills_only_unknown_normalized_credit_without_overwriting_known_usage() {
    let folder = tempfile::tempdir().unwrap();
    let config = folder.path().join("config");
    let root = folder.path().join("workbuddy");
    let project = root.join("projects/normalized");
    std::fs::create_dir_all(&project).unwrap();
    let path = project.join("normalized.jsonl");
    let mut model = json!({"type":"message","role":"assistant","sessionId":"normalized","timestamp":now_ms(),
        "providerData":{"messageId":"response","requestModelId":"deepseek-v4.1-flash",
            "usage":{"inputTokens":100,"outputTokens":20}}});
    std::fs::write(&path, format!("{model}\n")).unwrap();
    let running = start(&config, &root, 0).await;
    wait_number(&config, "SELECT COUNT(*) FROM usage_requests", 1).await;
    drop(running);
    let conn = Connection::open(config.join("usage.sqlite3")).unwrap();
    conn.execute("UPDATE usage_requests SET parser_version=1", [])
        .unwrap();
    conn.pragma_update(None, "user_version", 8).unwrap();
    drop(conn);
    model["providerData"]["usage"]["credit"] = json!(1.14);
    std::fs::write(&path, format!("{model}\n")).unwrap();
    let running = start(&config, &root, 0).await;
    wait_number(
        &config,
        "SELECT COUNT(*) FROM usage_requests WHERE workbuddy_credit_nanos=1140000000",
        1,
    )
    .await;
    assert_eq!(number(&config, "SELECT COUNT(*) FROM usage_requests"), 1);
    assert_eq!(
        number(&config, "SELECT SUM(total_tokens) FROM usage_requests"),
        120
    );
    assert_eq!(
        number(
            &config,
            "SELECT COUNT(*) FROM workbuddy_import_ledger WHERE state='conflict'"
        ),
        0
    );
    full_scan(&running).await;
    assert_eq!(
        number(
            &config,
            "SELECT SUM(workbuddy_credit_nanos) FROM usage_requests"
        ),
        1_140_000_000
    );
    model["providerData"]["usage"]["credit"] = json!(2.13);
    std::fs::write(&path, format!("{model}\n")).unwrap();
    full_scan(&running).await;
    assert_eq!(
        number(
            &config,
            "SELECT COUNT(*) FROM workbuddy_import_ledger WHERE state='conflict'"
        ),
        1,
        "a new, conflicting amount is not a parser-version enrichment"
    );
    assert_eq!(number(&config, "SELECT COUNT(*) FROM usage_requests"), 0);
}

#[tokio::test]
async fn upgrading_v10_receipts_repairs_tokens_without_logs_and_replays_without_conflicts() {
    let folder = tempfile::tempdir().unwrap();
    let config = folder.path().join("config");
    let root = folder.path().join("workbuddy");
    let project = root.join("projects/applicability");
    std::fs::create_dir_all(&project).unwrap();
    let path = project.join("applicability.jsonl");
    let at = now_ms() - 1000;
    let model = json!({"type":"model-usage","timestamp":at,"providerData":{"messageId":"model","requestModelId":"deepseek","rawUsage":{"prompt_tokens":100,"completion_tokens":20,"credit":19.07}}});
    let body = format!(
        "{model}\n{}\n{}\n",
        credit("image:first", at + 1),
        credit("image:second", at + 2)
    );
    std::fs::write(&path, &body).unwrap();
    let running = start(&config, &root, 0).await;
    wait_number(&config, "SELECT COUNT(*) FROM usage_requests", 3).await;
    drop(running);
    let db = Connection::open(config.join("usage.sqlite3")).unwrap();
    db.execute("UPDATE usage_requests SET usage_status='missing' WHERE requested_model GLOB 'workbuddy-credit:*'",[]).unwrap();
    db.pragma_update(None, "user_version", 10).unwrap();
    let hashes: Vec<(String, String)> = db
        .prepare("SELECT entry_key,normalized_hash FROM workbuddy_import_ledger ORDER BY entry_key")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    drop(db);
    std::fs::remove_file(&path).unwrap();
    let running = start(&config, &root, 0).await;
    assert_eq!(
        number(
            &config,
            "SELECT COUNT(*) FROM usage_requests WHERE usage_status='not_applicable'"
        ),
        2
    );
    let fields = json!({"from_ms":at-1,"to_ms":at+100,"timezone":"UTC","record_kind":"tool"});
    let dashboard = command(
        &running.0,
        json!({"command":"stats_dashboard","filter":fields,"granularity":"hour"}),
    )
    .await;
    assert_eq!(dashboard["summary"]["usage_missing_count"], 0);
    assert_eq!(dashboard["summary"]["usage_eligible_count"], 0);
    assert_eq!(
        dashboard["summary"]["workbuddy_credit_nanos"],
        "11420000000"
    );
    assert!(
        !dashboard["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["code"] == "usage_missing")
    );
    std::fs::write(&path, &body).unwrap();
    full_scan(&running).await;
    assert_eq!(number(&config, "SELECT COUNT(*) FROM usage_requests"), 3);
    assert_eq!(
        number(
            &config,
            "SELECT SUM(workbuddy_credit_nanos) FROM usage_requests"
        ),
        30_490_000_000
    );
    assert_eq!(
        number(&config, "SELECT SUM(total_tokens) FROM usage_requests"),
        120
    );
    assert_eq!(
        number(
            &config,
            "SELECT COUNT(*) FROM workbuddy_import_ledger WHERE state='conflict'"
        ),
        0
    );
    let db = Connection::open(config.join("usage.sqlite3")).unwrap();
    let after: Vec<(String, String)> = db
        .prepare("SELECT entry_key,normalized_hash FROM workbuddy_import_ledger ORDER BY entry_key")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(after, hashes);
}
