use super::*;
use rusqlite::types::Value;

fn rows(conn: &Connection, table: &str) -> Vec<Vec<Value>> {
    let mut statement = conn
        .prepare(&format!("SELECT * FROM {table} ORDER BY rowid"))
        .unwrap();
    let columns = statement.column_count();
    statement
        .query_map([], |row| (0..columns).map(|i| row.get(i)).collect())
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

#[test]
fn v11_repairs_existing_receipts_without_logs_or_changing_money_and_ledger() {
    let folder = tempfile::tempdir().unwrap();
    let mut conn = Connection::open(folder.path().join("usage.sqlite3")).unwrap();
    conn.pragma_update(None, "foreign_keys", "OFF").unwrap();
    for (version, sql) in [
        (1, MIGRATION_0001),
        (2, MIGRATION_0002),
        (3, MIGRATION_0003),
        (4, MIGRATION_0004),
        (5, MIGRATION_0005),
        (6, MIGRATION_0006),
        (7, MIGRATION_0007),
        (8, MIGRATION_0008),
        (9, MIGRATION_0009),
        (10, MIGRATION_0010),
    ] {
        apply_step(&mut conn, version, sql).unwrap();
    }
    conn.pragma_update(None, "foreign_keys", "ON").unwrap();
    for (id, model, credit) in [
        ("a", "workbuddy-credit:generation_tool", 5_710_000_000_i64),
        ("b", "workbuddy-credit:team_member", 1_140_000_000),
        ("c", "deepseek", 19_070_000_000),
    ] {
        conn.execute("INSERT INTO usage_requests(request_id,source,provider_id,route_id,provider_name,route_name,requested_model,upstream_model,started_at_ms,ended_at_ms,outcome,usage_status,usage_source,created_at_ms,updated_at_ms,upstream_protocol,record_origin,time_basis,workbuddy_credit_nanos) VALUES(?1,'workbuddy_import','wb:builtin','r','WorkBuddy','ImageGen',?2,?2,100,100,'recorded','missing','workbuddy_jsonl',100,100,NULL,'workbuddy_builtin','message_timestamp',?3)",rusqlite::params![id.repeat(64), model, credit]).unwrap();
        conn.execute("INSERT INTO workbuddy_import_ledger(entry_key,namespace_id,session_hash,message_hash,requested_model,event_at_ms,normalized_hash,state,request_id,mapping_revision,created_at_ms,updated_at_ms) VALUES(?1,?2,?3,?4,?5,100,?6,'imported',?1,1,100,100)",rusqlite::params![id.repeat(64),"d".repeat(48),"e".repeat(64),id.repeat(64),model,"1".repeat(64)]).unwrap();
    }
    conn.execute(
        "INSERT INTO stats_runs(run_id,started_at_ms) VALUES('proxy',0)",
        [],
    )
    .unwrap();
    conn.execute("INSERT INTO usage_requests(request_id,run_id,source,provider_id,route_id,provider_name,route_name,requested_model,upstream_model,started_at_ms,ended_at_ms,outcome,stream,upstream_started,usage_status,usage_source,created_at_ms,updated_at_ms) VALUES('proxy','proxy','relay','p','r','P','R','workbuddy-credit:generation_tool','m',100,100,'completed',0,1,'missing','none',100,100)",[]).unwrap();
    let ledger = rows(&conn, "workbuddy_import_ledger");
    let before = rows(&conn, "usage_requests");
    // A failed rebuild must roll back both table replacement and repair.
    conn.pragma_update(None, "foreign_keys", "OFF").unwrap();
    assert!(
        apply_step(
            &mut conn,
            11,
            &format!("{MIGRATION_0011} SELECT * FROM missing_table;")
        )
        .is_err()
    );
    assert_eq!(rows(&conn, "usage_requests"), before);
    assert_eq!(
        conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        10
    );
    conn.pragma_update(None, "foreign_keys", "ON").unwrap();
    migrate(&mut conn, folder.path()).unwrap();
    let mut expected = before;
    // Only applicability and its explanation may change; ids/amounts stay exact.
    for row in expected.iter_mut().take(2) {
        row[21] = Value::Text("not_applicable".into());
        row[22] = Value::Null;
    }
    assert_eq!(rows(&conn, "usage_requests"), expected);
    assert_eq!(rows(&conn, "workbuddy_import_ledger"), ledger);
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |r| r
            .get::<_, i64>(
            0
        ))
        .unwrap(),
        0
    );
    assert_eq!(
        conn.query_row("PRAGMA foreign_keys", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert!(folder.path().join("usage-backup.sqlite3").is_file());
    // Ordinary imported model rows and same-named proxy models cannot be N/A.
    for id in ["c".repeat(64), "proxy".into()] {
        assert!(
            conn.execute(
                "UPDATE usage_requests SET usage_status='not_applicable' WHERE request_id=?1",
                [id]
            )
            .is_err()
        );
    }
    migrate(&mut conn, folder.path()).unwrap();
    assert_eq!(rows(&conn, "usage_requests"), expected);
}
