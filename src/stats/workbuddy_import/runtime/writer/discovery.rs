//! The existing sole writer durably stages changed files without advancing any
//! source offset, fingerprint or fact. Failed transactions leave no hint ACK.
use super::*;

pub(super) fn apply(
    tx: &rusqlite::Transaction<'_>,
    settings: &store::ImportSettings,
    batch: DiscoveryBatch,
) -> Result<(), &'static str> {
    let bytes: usize = batch
        .files
        .iter()
        .map(|file| file.relative_path.len() + 128)
        .sum();
    if batch.namespace_id != settings.namespace_id
        || batch.settings_revision != settings.revision
        || batch.files.len() > types::MAX_BATCH_ITEMS
        || bytes > types::MAX_BATCH_BYTES
    {
        return Err("stats_workbuddy_stale_batch");
    }
    for file in batch.files {
        let old = store::load_checkpoint(tx, &file.file_key).map_err(|_| "stats_unavailable")?;
        if old.as_ref().map(|old| old.revision) != file.expected_revision {
            return Err("stats_workbuddy_stale_batch");
        }
        if old.is_none() && file.needs_read {
            store::insert_checkpoint(
                tx,
                &store::NewCheckpoint {
                    file_key: file.file_key,
                    namespace_id: batch.namespace_id.clone(),
                    relative_path: file.relative_path,
                    observed_size: file.size,
                    modified_ms: file.modified_ms,
                    now_ms: now_ms(),
                },
            )
            .map_err(|_| "stats_unavailable")?;
        } else if old.is_some() && file.needs_read {
            // Keep old size/mtime/fingerprints until the actual source slice
            // verifies append vs replacement and commits its guarded offset.
            tx.execute(
                "UPDATE workbuddy_import_files SET last_completed_at_ms=NULL,
                last_error_code=NULL,revision=revision+1,last_seen_at_ms=?2 WHERE file_key=?1",
                params![file.file_key, now_ms()],
            )
            .map_err(|_| "stats_unavailable")?;
        } else if old.is_some() {
            store::touch_checkpoint(tx, &file.file_key, now_ms())
                .map_err(|_| "stats_unavailable")?;
        } else {
            return Err("stats_workbuddy_stale_batch");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
