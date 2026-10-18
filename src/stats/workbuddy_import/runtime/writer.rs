//! Bounded transaction adapter for the existing statistics writer.
use super::*;

mod discovery;

#[derive(Clone, Copy, Default)]
pub(super) struct WriteResult {
    pub(super) revised: bool,
    pub(super) catchup_more: bool,
}

pub(super) fn apply_work(conn: &mut Connection, work: Work) -> Result<WriteResult, &'static str> {
    let mut report = WriteResult::default();
    let tx = conn.transaction().map_err(|_| "stats_unavailable")?;
    let settings = store::load_import_settings(&tx).map_err(|_| "stats_unavailable")?;
    let global = crate::stats::store::load_settings(&tx).map_err(|_| "stats_unavailable")?;
    if !settings.enabled || !global.enabled {
        return Err("stats_workbuddy_import_disabled");
    }
    match work {
        Work::Discover(batch) => discovery::apply(&tx, &settings, batch)?,
        Work::CatchUp => {
            let processed = super::super::reconcile::catch_up(&tx, global.retention_days, now_ms())
                .map_err(|_| "stats_unavailable")?;
            report.catchup_more = processed >= types::MAX_BATCH_ITEMS;
            report.revised = processed > 0;
        }
        Work::Reconcile { mapping_id } => {
            report.revised =
                super::super::reconcile::run(&tx, &mapping_id, global.retention_days, now_ms())
                    .map_err(|_| "stats_unavailable")?;
        }
        Work::Observe {
            observations,
            at_ms,
            final_batch,
            ..
        } => {
            super::super::observation::observe(&tx, &observations, at_ms, final_batch)
                .map_err(|_| "stats_unavailable")?;
        }
        Work::BeginFull => {
            tx.execute("UPDATE stats_settings SET workbuddy_last_full_scan_ms=COALESCE(workbuddy_last_full_scan_ms,0) WHERE singleton=1",[])
                .map_err(|_| "stats_unavailable")?;
            tx.execute(
                "UPDATE workbuddy_import_files SET scan_offset=0,scan_generation=0",
                [],
            )
            .map_err(|_| "stats_unavailable")?;
        }
        Work::CompleteFull => {
            let errors:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM workbuddy_import_files WHERE last_error_code IS NOT NULL)",[],|row|row.get(0)).map_err(|_|"stats_unavailable")?;
            if errors {
                return Err("stats_workbuddy_source_read_failed");
            }
            tx.execute(
                "UPDATE workbuddy_import_files SET scan_generation=generation,
                scan_offset=committed_offset WHERE scan_generation<>generation",
                [],
            )
            .map_err(|_| "stats_unavailable")?;
            tx.execute(
                "UPDATE stats_settings SET workbuddy_last_full_scan_ms=?1 WHERE singleton=1",
                params![now_ms()],
            )
            .map_err(|_| "stats_unavailable")?;
        }
        Work::Error { file_key, code } => {
            tx.execute(
                "UPDATE workbuddy_import_files SET last_error_code=?2 WHERE file_key=?1",
                params![file_key, code],
            )
            .map_err(|_| "stats_unavailable")?;
        }
        Work::Touch { file_key } => {
            store::touch_checkpoint(&tx, &file_key, now_ms()).map_err(|_| "stats_unavailable")?;
        }
        Work::Batch(batch) => report.revised = apply_batch(&tx, &settings, &global, *batch)?,
    }
    tx.commit().map(|_| report).map_err(|_| "stats_unavailable")
}

/// Validates every snapshot revision before any fact/checkpoint write. The
/// caller owns the encompassing transaction, so all errors roll back together.
fn apply_batch(
    tx: &rusqlite::Transaction<'_>,
    settings: &store::ImportSettings,
    global: &crate::stats::store::Settings,
    mut batch: FileBatch,
) -> Result<bool, &'static str> {
    if settings.namespace_id != batch.namespace_id
        || settings.revision != batch.settings_revision
        || settings.mapping_revision != batch.mapping_revision
    {
        return Err("stats_workbuddy_stale_batch");
    }
    let checkpoint = prepare_checkpoint(tx, &batch)?;
    let now = now_ms();
    let mut diagnostics: types::DiagnosticCounts =
        serde_json::from_str(&checkpoint.diagnostic_counts_json)
            .map_err(|_| "stats_workbuddy_unsupported_format")?;
    for (kind, count) in batch.diagnostics.iter() {
        diagnostics.add(kind, count);
    }
    let guard = store::CheckpointGuard {
        file_key: batch.file_key.clone(),
        namespace_id: batch.namespace_id.clone(),
        generation: checkpoint.generation,
        revision: checkpoint.revision,
        base_offset: checkpoint.committed_offset,
    };
    let candidates = std::mem::take(&mut batch.candidates);
    let items = normalized_items(tx, &batch.namespace_id, candidates)?;
    let update = store::CheckpointUpdate {
        next_offset: batch.offset,
        observed_size: batch.observed_size,
        modified_ms: batch.modified_ms,
        prefix_hash: Some(batch.fingerprint.prefix_hash.clone()),
        anchor_hash: Some(batch.fingerprint.anchor_hash.clone()),
        diagnostics,
        last_error_code: None,
        now_ms: now,
    };
    let report = store::commit_batch(
        tx,
        &batch.namespace_id,
        &guard,
        &update,
        store::BatchPolicy {
            settings_revision: settings.revision,
            retention_days: global.retention_days,
        },
        &items,
        now,
    )
    .map_err(|_| "stats_workbuddy_stale_batch")?;
    store_checkpoint_details(tx, &batch, &checkpoint, now)?;
    Ok(report.conflicted > 0)
}

fn prepare_checkpoint(
    tx: &rusqlite::Transaction<'_>,
    batch: &FileBatch,
) -> Result<store::Checkpoint, &'static str> {
    let current = store::load_checkpoint(tx, &batch.file_key).map_err(|_| "stats_unavailable")?;
    if current != batch.expected {
        return Err("stats_workbuddy_stale_batch");
    }
    if current.is_none() {
        store::insert_checkpoint(
            tx,
            &store::NewCheckpoint {
                file_key: batch.file_key.clone(),
                namespace_id: batch.namespace_id.clone(),
                relative_path: batch.relative_path.clone(),
                observed_size: batch.observed_size,
                modified_ms: batch.modified_ms,
                now_ms: now_ms(),
            },
        )
        .map_err(|_| "stats_unavailable")?;
    } else if batch.reset {
        tx.execute(
            "UPDATE workbuddy_import_files SET generation=generation+1,revision=revision+1,
                    committed_offset=0,scan_offset=0,scan_generation=0,discarding_oversized=0,
                    prefix_hash=NULL,anchor_hash=NULL WHERE file_key=?1",
            params![batch.file_key],
        )
        .map_err(|_| "stats_unavailable")?;
    }
    let checkpoint = store::load_checkpoint(tx, &batch.file_key)
        .map_err(|_| "stats_unavailable")?
        .ok_or("stats_workbuddy_stale_batch")?;

    Ok(checkpoint)
}

fn normalized_items(
    tx: &rusqlite::Transaction<'_>,
    namespace_id: &str,
    candidates: Vec<types::Candidate>,
) -> Result<Vec<store::CommitItem>, &'static str> {
    let mut items = Vec::with_capacity(candidates.len());
    let mut payload_bytes = 0;
    for mut candidate in candidates {
        if let Some(existing) =
            store::load(tx, &candidate.entry_key).map_err(|_| "stats_unavailable")?
        {
            candidate.event_at_ms = existing.event_at_ms;
        }
        let (attribution, mapping_id) = store::attribute_candidate(tx, namespace_id, &candidate)
            .map_err(|_| "stats_unavailable")?;
        let mut item = store::CommitItem::from_candidate(candidate, attribution)
            .map_err(|_| "stats_workbuddy_unsupported_format")?;
        item.mapping_id = mapping_id;
        payload_bytes += item.candidate_json.as_ref().map_or(0, String::len);
        items.push(item);
    }
    if items.len() > types::MAX_BATCH_ITEMS || payload_bytes > types::MAX_BATCH_BYTES {
        return Err("stats_workbuddy_unsupported_format");
    }

    Ok(items)
}

fn store_checkpoint_details(
    tx: &rusqlite::Transaction<'_>,
    batch: &FileBatch,
    checkpoint: &store::Checkpoint,
    now: i64,
) -> Result<(), &'static str> {
    tx.execute(
        "UPDATE workbuddy_import_files SET prefix_len=?2,anchor_start=?3,anchor_len=?4,
                discarding_oversized=?5,last_completed_at_ms=?6 WHERE file_key=?1",
        params![
            batch.file_key,
            batch.fingerprint.prefix_len as i64,
            batch.fingerprint.anchor_start as i64,
            batch.fingerprint.anchor_len as i64,
            if batch.scan_offset.is_some() {
                checkpoint.discarding_oversized
            } else {
                batch.discarding
            },
            if batch.offset == batch.observed_size || (batch.scan_offset.is_none() && batch.at_eof)
            {
                Some(now)
            } else {
                None
            }
        ],
    )
    .map_err(|_| "stats_unavailable")?;
    if let Some(offset) = batch.scan_offset {
        tx.execute(
            "UPDATE workbuddy_import_files SET scan_offset=?2,scan_generation=?3 WHERE file_key=?1",
            params![
                batch.file_key,
                offset,
                if batch.at_eof {
                    checkpoint.generation
                } else {
                    0
                }
            ],
        )
        .map_err(|_| "stats_unavailable")?;
    }

    Ok(())
}
