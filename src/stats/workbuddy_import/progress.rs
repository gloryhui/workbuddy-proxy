//! Durable progress over the last observed checkpoint sizes. Never reads source
//! files. Classification work is separate from unread source backlog.
use super::store::StoreError;
use rusqlite::Transaction;

pub(super) fn snapshot(tx: &Transaction<'_>) -> Result<serde_json::Value, StoreError> {
    Ok(tx.query_row(
        "SELECT COUNT(*),
         COALESCE(SUM(CASE WHEN last_error_code IS NOT NULL OR last_completed_at_ms IS NULL
             OR committed_offset<observed_size THEN 1 ELSE 0 END),0),
         COALESCE(SUM(observed_size),0),
         COALESCE(SUM(MIN(committed_offset,observed_size)),0),
         COALESCE(SUM(MAX(observed_size-committed_offset,0)),0)
         FROM workbuddy_import_files",
        [],
        |row| {
            Ok(serde_json::json!({
                "files_total": row.get::<_,i64>(0)?,
                "files_remaining": row.get::<_,i64>(1)?,
                "bytes_total": row.get::<_,i64>(2)?,
                "bytes_committed": row.get::<_,i64>(3)?,
                "bytes_remaining": row.get::<_,i64>(4)?,
                // Only the worker can establish whether discovery has finished.
                "total_known": false,
            }))
        },
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checkpoint_snapshot_handles_partial_error_empty_and_replacement() {
        let mut conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::stats::store::migrate(&mut conn, std::path::Path::new(".")).unwrap();
        let tx = conn.transaction().unwrap();
        assert_eq!(snapshot(&tx).unwrap()["bytes_total"], 0);
        for (index, size, offset, done, error) in [
            (1, 100, 40, None, None),
            (2, 200, 200, Some(1), None),
            (
                3,
                50,
                50,
                Some(1),
                Some("stats_workbuddy_permission_denied"),
            ),
            // A replacement can shrink the observed file before the new read.
            (4, 10, 80, None, None),
        ] {
            tx.execute(
                "INSERT INTO workbuddy_import_files
                (file_key,namespace_id,relative_path,observed_size,committed_offset,
                 last_seen_at_ms,last_completed_at_ms,last_error_code)
                VALUES(?1,?2,?3,?4,?5,1,?6,?7)",
                rusqlite::params![
                    format!("{index:064}"),
                    "a".repeat(48),
                    format!("{index}.jsonl"),
                    size,
                    offset,
                    done,
                    error
                ],
            )
            .unwrap();
        }
        let value = snapshot(&tx).unwrap();
        assert_eq!(value["files_total"], 4);
        assert_eq!(value["files_remaining"], 3);
        assert_eq!(value["bytes_total"], 360);
        assert_eq!(value["bytes_committed"], 300);
        assert_eq!(value["bytes_remaining"], 60);
        assert_eq!(value["total_known"], false);
        tx.commit().unwrap();
        assert_eq!(snapshot(&conn.transaction().unwrap()).unwrap(), value);
    }
}
