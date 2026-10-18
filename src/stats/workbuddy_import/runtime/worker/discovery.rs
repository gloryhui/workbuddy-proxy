//! Discovery performs bounded stat-only batches. Historical body parsing cannot
//! hold the directory iterator for tens of seconds and hide fresh projects.
use super::*;

pub(super) struct Stage {
    pub(super) batch: DiscoveryBatch,
    pub(super) finished: bool,
    pub(super) error: Option<&'static str>,
}

fn notice(
    tx: &rusqlite::Transaction<'_>,
    root: &Path,
    namespace: &str,
    relative: String,
) -> Result<Option<DiscoveredFile>, &'static str> {
    let path = safe_source(root, &relative)?;
    let metadata =
        std::fs::symlink_metadata(path).map_err(|_| "stats_workbuddy_source_read_failed")?;
    let size = i64::try_from(metadata.len()).map_err(|_| "stats_workbuddy_unsupported_format")?;
    let modified_ms = modified_ms(&metadata)?;
    let key = file_key(namespace, &relative);
    let old = store::load_checkpoint(tx, &key).map_err(|_| "stats_unavailable")?;
    let needs_read = old.as_ref().is_none_or(|old| {
        old.observed_size != size || old.modified_ms != modified_ms || old.last_error_code.is_some()
    });
    let touch = old
        .as_ref()
        .is_some_and(|old| now_ms() - old.last_seen_at_ms >= 86_400_000);
    if !needs_read && !touch {
        return Ok(None);
    }
    Ok(Some(DiscoveredFile {
        file_key: key,
        relative_path: relative,
        size,
        modified_ms,
        expected_revision: old.map(|old| old.revision),
        needs_read,
    }))
}

/// Visits at most DISCOVERY_SLICE entries and emits no source body/candidate.
/// Failed stat entries mark this pass degraded while other entries continue.
pub(super) fn stage(
    conn: &mut Connection,
    root: &reader::ReaderRoot,
    walk: &mut reader::Discovery,
) -> Result<Stage, &'static str> {
    let tx = conn.transaction().map_err(|_| "stats_unavailable")?;
    let settings = store::load_import_settings(&tx).map_err(|_| "stats_unavailable")?;
    let mut stage = Stage {
        batch: DiscoveryBatch {
            namespace_id: settings.namespace_id.clone(),
            settings_revision: settings.revision,
            files: Vec::new(),
        },
        finished: false,
        error: None,
    };
    for _ in 0..reader::DISCOVERY_SLICE {
        let (file, finished) = walk.next_bounded(1);
        if let Some(kind) = walk.take_error() {
            stage.error = Some(if kind == std::io::ErrorKind::PermissionDenied {
                "stats_workbuddy_permission_denied"
            } else {
                "stats_workbuddy_source_read_failed"
            });
        }
        if let Some(file) = file {
            match notice(&tx, root.root(), &settings.namespace_id, file.relative_path) {
                Ok(Some(file)) => stage.batch.files.push(file),
                Ok(None) => {}
                Err(code) => stage.error = Some(code),
            }
        }
        if finished {
            stage.finished = true;
            break;
        }
    }
    tx.commit().map_err(|_| "stats_unavailable")?;
    Ok(stage)
}
