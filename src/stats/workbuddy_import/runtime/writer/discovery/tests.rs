use super::*;

fn database() -> Connection {
    let mut conn = Connection::open_in_memory().unwrap();
    crate::stats::store::migrate(&mut conn, Path::new(".")).unwrap();
    conn.execute(
        "UPDATE stats_settings SET workbuddy_import_enabled=1,
        workbuddy_namespace_id=?1",
        params!["b".repeat(64)],
    )
    .unwrap();
    conn
}

fn note(key: char, revision: Option<i64>) -> DiscoveredFile {
    DiscoveredFile {
        file_key: key.to_string().repeat(64),
        relative_path: format!("projects/demo/{key}.jsonl"),
        size: 1200,
        modified_ms: 2,
        expected_revision: revision,
        needs_read: true,
    }
}

fn batch(conn: &mut Connection, files: Vec<DiscoveredFile>) -> Work {
    let tx = conn.transaction().unwrap();
    let settings = store::load_import_settings(&tx).unwrap();
    let batch = DiscoveryBatch {
        namespace_id: settings.namespace_id,
        settings_revision: settings.revision,
        files,
    };
    tx.commit().unwrap();
    Work::Discover(batch)
}

fn load(conn: &mut Connection, key: char) -> store::Checkpoint {
    let tx = conn.transaction().unwrap();
    let value = store::load_checkpoint(&tx, &key.to_string().repeat(64))
        .unwrap()
        .unwrap();
    tx.commit().unwrap();
    value
}

#[test]
fn metadata_staging_preserves_offsets_fingerprints_and_marks_same_size_rewrite_pending() {
    let mut conn = database();
    let work = batch(&mut conn, vec![note('a', None)]);
    apply_work(&mut conn, work).unwrap();
    conn.execute(
        "UPDATE workbuddy_import_files SET committed_offset=1200,
        observed_size=1200,modified_ms=1,last_completed_at_ms=1,
        scan_offset=640,scan_generation=1,prefix_hash=?1,anchor_hash=?2,
        prefix_len=1200,anchor_start=944,anchor_len=256",
        params!["c".repeat(64), "d".repeat(64)],
    )
    .unwrap();
    let before = load(&mut conn, 'a');
    let work = batch(&mut conn, vec![note('a', Some(before.revision))]);
    apply_work(&mut conn, work).unwrap();
    let after = load(&mut conn, 'a');
    assert_eq!(after.committed_offset, before.committed_offset);
    assert_eq!(after.observed_size, before.observed_size);
    assert_eq!(after.modified_ms, before.modified_ms);
    assert_eq!(after.prefix_hash, before.prefix_hash);
    assert_eq!(after.anchor_hash, before.anchor_hash);
    assert_eq!(after.scan_offset, before.scan_offset);
    assert_eq!(after.scan_generation, before.scan_generation);
    assert_eq!(after.generation, before.generation);
    assert_eq!(after.revision, before.revision + 1);
    assert_eq!(after.last_completed_at_ms, None);
    let tx = conn.transaction().unwrap();
    assert_eq!(store::load_import_counts(&tx).unwrap().files_pending, 1);
    assert_eq!(
        tx.query_row("SELECT COUNT(*) FROM usage_requests", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[test]
fn a_stale_metadata_batch_rolls_back_earlier_files_and_can_be_retried() {
    let mut conn = database();
    let work = batch(&mut conn, vec![note('a', None)]);
    apply_work(&mut conn, work).unwrap();
    let old = load(&mut conn, 'a');
    let work = batch(
        &mut conn,
        vec![note('e', None), note('a', Some(old.revision + 1))],
    );
    assert!(matches!(
        apply_work(&mut conn, work),
        Err("stats_workbuddy_stale_batch")
    ));
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM workbuddy_import_files", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(load(&mut conn, 'a'), old);
    let work = batch(
        &mut conn,
        vec![note('e', None), note('a', Some(old.revision))],
    );
    apply_work(&mut conn, work).unwrap();
    assert_eq!(load(&mut conn, 'e').committed_offset, 0);
    assert_eq!(load(&mut conn, 'a').committed_offset, 0);
}
