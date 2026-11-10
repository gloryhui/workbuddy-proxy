//! 历史导入命令到现有统计 writer 的事务适配层。
//!
//! worker 线程不把文件读到的事实直接写库；它把 `Work` 放入 Envelope，由统计
//! writer 线程调用本文件的 `apply_work`。每个 Work 使用 writer 自己的连接和独立的
//! SQLite 事务：事务开始后，事实/ledger 变更、checkpoint 和扫描元数据在成功时一起
//! 提交；提前返回的错误会让已开启事务随 Drop 回滚。事务创建本身失败或命令在进入
//! `apply_work` 前被拒绝时，不能声称发生过一次回滚。
//!
//! `apply_work` 返回 Ok 前已经执行 `commit`；随后 runtime 才更新进程内计数并发送
//! Envelope 结果。ACK 若未被调用方观察到，不能据此证明没有提交。这里接近 Spring
//! 的事务服务方法，但调用方不复用 HTTP 请求级事务：源读取发生在事务前，恢复时应
//! 重新读取 durable checkpoint 和 ledger。
use super::*;

mod discovery;

/// 一次 Work 对统计状态造成的影响摘要。
///
/// `revised` 表示事实或历史归属发生了会改变已发布结果的修订；`catchup_more`
/// 表示本次补处理达到批次上限，调度器应尽快继续。ACK 成功但两者都为 false 仍算
/// 成功提交，例如只 touch checkpoint 或没有新候选的空片。
#[derive(Clone, Copy, Default)]
pub(super) struct WriteResult {
    pub(super) revised: bool,
    pub(super) catchup_more: bool,
}

pub(super) fn apply_work(conn: &mut Connection, work: Work) -> Result<WriteResult, &'static str> {
    // 整个 Work 使用同一 writer 事务。先重新读取 import settings 和全局 stats
    // settings；任一功能已禁用就返回错误，已创建的 transaction 随 Drop 回滚。
    // `conn.transaction()` 本身失败时事务尚未开始，这条错误路径没有可回滚事务。
    //
    // 不同 Work 在 match 中分派到发现、catch-up 补处理、模型观察、全量扫描控制、错误记录
    // 或文件批次。只有在全部数据库操作成功后 `tx.commit()` 才返回 report；worker
    // 此时仍在等待 ACK，无法抢在提交前把来源 offset 当作成功。
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
    // 文件批次的验证和提交顺序：
    //
    // 1. 当前 namespace、settings revision、mapping revision 必须仍等于读取快照；
    // 2. 当前 durable checkpoint 必须仍等于 batch.expected；
    // 3. 必要时在事务内新建/reset checkpoint，形成 generation/revision/base offset
    //    的乐观保护值；
    // 4. 归一化候选并保留历史 event time，再交给 `store::commit_batch`；
    // 5. 同一事务更新 fingerprint、诊断和 scan cursor 细节。
    //
    // 任一比较或写入失败都不会单独推进 committed_offset。ACK 发送的是整笔结果，
    // 因而没有“事实已写、checkpoint 未写”的已提交中间状态。
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
    .map_err(batch_write_error)?;
    store_checkpoint_details(tx, &batch, &checkpoint, now)?;
    Ok(report.conflicted > 0)
}

fn batch_write_error(error: store::StoreError) -> &'static str {
    // 错误码决定 worker 的重试语义。配置版本、mapping 冲突、来源变化和乐观 guard
    // 过期属于 stale batch，源线程应重新读取 checkpoint；数据库 busy/locked 返回
    // scan_busy，也可稍后重试。存储损坏或其他无效数据返回 unavailable，不能被
    // 误报成“只是快照过期”，否则会形成无进展重试。
    use types::BatchRejectReason;
    match error {
        store::StoreError::SettingsStale
        | store::StoreError::BatchRejected(
            BatchRejectReason::StaleBatch
            | BatchRejectReason::MappingConflict
            | BatchRejectReason::SourceChanged,
        ) => "stats_workbuddy_stale_batch",
        store::StoreError::BatchRejected(BatchRejectReason::ImportDisabled) => {
            "stats_workbuddy_import_disabled"
        }
        store::StoreError::Sqlite(rusqlite::Error::SqliteFailure(error, _))
            if matches!(
                error.code,
                rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked
            ) =>
        {
            "stats_workbuddy_scan_busy"
        }
        // Storage and invalid-data errors must still make the scan fail.
        _ => "stats_unavailable",
    }
}

fn prepare_checkpoint(
    tx: &rusqlite::Transaction<'_>,
    batch: &FileBatch,
) -> Result<store::Checkpoint, &'static str> {
    // `current != batch.expected` 说明排队期间已有另一个成功提交或维护操作改变了
    // 文件水位，本片必须作废。首次发现文件时在事务内插入 generation=1/revision=1
    // 的 checkpoint；replace/truncate 则递增 generation 和 revision 并把读取位置
    // 归零。
    //
    // 这里重新读取 checkpoint，而不是继续信任 FileBatch 中的 expected；同时把它
    // 转成 CheckpointGuard，由后续 commit_checkpoint 再做事务内条件更新。
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
    // Candidate 到 CommitItem 的转换仍在本 writer 事务内完成。重复入口的 event time
    // 使用数据库中已有值，避免相同消息因再次解析时间不同而改变统计归属。配置版本
    // 已由外层 `apply_batch` 与读取快照比较；当前 `attribute_candidate` 不读取历史
    // mapping 区间，实际分类只查询原生重复身份并按 requested model 分类。
    //
    // 所有候选先完成转换，并检查条目数与 whitelisted candidate JSON 的累计字节数；
    // 超限返回 unsupported_format。由于尚未 commit，任何一条失败都会放弃整批，不会
    // 留下前半批事实。
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
    // `store::commit_batch` 只借用当前事务写入事实、ledger 和基础 checkpoint，本函数
    // 继续在同一未提交事务中补充 prefix/anchor 长度、超长行排水状态、完成时间和
    // full scan cursor；最终提交点是 `apply_work` 末尾的 `tx.commit()`。
    // scan_offset 与 committed_offset 是两条不同进度：验证扫描可以走得更远，但增量
    // 读取是否前进仍由这次外层事务的最终提交结果决定。
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

#[cfg(test)]
mod failure_tests {
    use super::*;

    #[test]
    fn storage_failures_are_not_misreported_as_retryable_stale_batches() {
        // 本测试覆盖错误分类边界：真正可重新读取的快照冲突返回 stale，SQLite
        // busy/locked 返回 scan_busy，普通存储或无效偏移不能伪装成可重试冲突。
        assert_eq!(
            batch_write_error(store::StoreError::SettingsStale),
            "stats_workbuddy_stale_batch"
        );
        assert_eq!(
            batch_write_error(store::StoreError::BatchRejected(
                types::BatchRejectReason::StaleBatch
            )),
            "stats_workbuddy_stale_batch"
        );
        assert_eq!(
            batch_write_error(store::StoreError::Sqlite(rusqlite::Error::InvalidQuery)),
            "stats_unavailable"
        );
        for code in [rusqlite::ffi::SQLITE_BUSY, rusqlite::ffi::SQLITE_LOCKED] {
            assert_eq!(
                batch_write_error(store::StoreError::Sqlite(rusqlite::Error::SqliteFailure(
                    rusqlite::ffi::Error::new(code),
                    None
                ))),
                "stats_workbuddy_scan_busy"
            );
        }
        assert_eq!(
            batch_write_error(store::StoreError::InvalidOffset),
            "stats_unavailable"
        );
    }
}
