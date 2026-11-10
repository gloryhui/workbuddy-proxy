//! 从一个源文件读取有界快照，并交给 writer 提交。
//!
//! 一次 `read_file` 分成三个关键阶段：
//!
//! 1. 从只读 SQLite 连接读取 namespace、settings revision、mapping revision 和
//!    durable checkpoint，同时记录源文件大小/mtime；
//! 2. 根据 checkpoint、全量扫描 cursor 和 fingerprint 决定起点，只读一个有界片；
//! 3. 再核对文件元数据，生成下一轮 fingerprint，构造 `FileBatch` 交给 writer。
//!
//! 文件正文不放进数据库读取事务。数据库事务只读取设置/checkpoint 后立即结束，
//! 随后 writer 在自己的新事务中重新核对版本和 checkpoint，再原子提交候选事实与
//! 新进度。Rust 的 `&mut Connection` 在这里只表示调用方可开始/结束读取事务，连接
//! 本身仍由调度器以只读 flags 打开。
use super::*;
use crate::stats::workbuddy_import::parser;

/// 一次文件读取所依据的不可变元数据快照。
///
/// `settings` 是读取开始时看到的完整配置快照；`checkpoint` 是本批预期覆盖的旧
/// durable 状态。`key` 按 namespace 与相对路径计算，后三者必须一起送到 writer
/// 再验证，不能因为文件随后被改名或配置更新就静默套用。
struct SourceSnapshot {
    path: std::path::PathBuf,
    metadata: std::fs::Metadata,
    size: i64,
    modified: i64,
    settings: store::ImportSettings,
    key: String,
    checkpoint: Option<store::Checkpoint>,
}

/// 本片实际使用的读取起点。
///
/// `primary_offset` 是正常增量 checkpoint 的已提交位置；`offset` 在 full scan 时
/// 还可能来自扫描专用 cursor。`discarding` 表示上次正在丢弃一条超长行，恢复时
/// 要从该状态继续，避免把超长行中段误解析为 JSON。
struct ReadPosition {
    reset: bool,
    primary_offset: i64,
    offset: i64,
    discarding: bool,
}

/// 一个有界读取片产生的内容，而不是已持久化批次。
///
/// 普通记录的 `offset` 只在完整换行后推进；正在排水的超长行是例外，此时
/// `discarding=true`，offset 可推进到本次已扫描位置而无需换行。`eof` 表示读取器
/// 看到了当前快照末尾。候选和诊断都是内存所有权数据，只有 writer 成功提交后才
/// 随 checkpoint 一起生效。
struct Slice {
    offset: i64,
    discarding: bool,
    eof: bool,
    candidates: Vec<types::Candidate>,
    diagnostics: types::DiagnosticCounts,
}

pub(super) fn read_file(
    conn: &mut Connection,
    root: &reader::ReaderRoot,
    relative: &str,
    shared: &Shared,
    full_cursor: Option<&FullCursor>,
) -> Result<Option<Work>, &'static str> {
    // 调度器已选出一个文件，本函数不创建线程也不获得写锁。先检查启停状态，
    // 再从只读连接加载配置和 checkpoint，并确认仍允许导入及 namespace 有效。
    //
    // 未变化且上次成功完成的文件不会重复读正文；只有距 last_seen 满 24 小时才
    // 生成轻量 Touch，让 writer 更新观察时间。返回 None 表示本轮无需提交。
    if !shared.active() {
        return Ok(None);
    }
    telemetry::source_io(shared);
    let snapshot = SourceSnapshot::load(conn, root, relative)?;
    if !snapshot.settings.enabled || snapshot.settings.namespace_id.is_empty() {
        return Ok(None);
    }
    if let Some(old) = snapshot.checkpoint.as_ref()
        && full_cursor.is_none()
        && old.observed_size == snapshot.size
        && old.modified_ms == snapshot.modified
        && old.last_completed_at_ms.is_some()
        && old.last_error_code.is_none()
    {
        return Ok(
            (now_ms() - old.last_seen_at_ms >= 86_400_000).then_some(Work::Touch {
                file_key: snapshot.key,
            }),
        );
    }
    let position = snapshot.position(full_cursor)?;
    let slice = read_slice(&snapshot, &position, shared)?;
    if !shared.active() {
        return Ok(None);
    }
    snapshot
        .finish(relative, position, slice, full_cursor.is_some())
        .map(Some)
}

impl SourceSnapshot {
    fn load(
        conn: &mut Connection,
        root: &reader::ReaderRoot,
        relative: &str,
    ) -> Result<Self, &'static str> {
        // 路径安全检查与文件 metadata 发生在数据库读取之前。随后在只读连接上开
        // transaction，读取 import settings 和新 checkpoint 后立即 commit；commit
        // 在这里只是结束一致的读取窗口，没有修改任何行。
        let path = safe_source(root.root(), relative)?;
        let metadata =
            std::fs::symlink_metadata(&path).map_err(|_| "stats_workbuddy_source_read_failed")?;
        let size =
            i64::try_from(metadata.len()).map_err(|_| "stats_workbuddy_unsupported_format")?;
        let modified = modified_ms(&metadata)?;
        let tx = conn.transaction().map_err(|_| "stats_unavailable")?;
        let settings = store::load_import_settings(&tx).map_err(|_| "stats_unavailable")?;
        let key = file_key(&settings.namespace_id, relative);
        let checkpoint = store::load_checkpoint(&tx, &key).map_err(|_| "stats_unavailable")?;
        tx.commit().map_err(|_| "stats_unavailable")?;
        Ok(Self {
            path,
            metadata,
            size,
            modified,
            settings,
            key,
            checkpoint,
        })
    }

    fn reset_required(&self) -> Result<bool, &'static str> {
        // 有 fingerprint 的旧 checkpoint 用 classify_change 区分替换、截断和可续读。
        // 旧格式若 committed_offset>0 却没有 fingerprint，保守地从文件头重读，最终
        // 依赖稳定 entry_key 去重。文件比现有 offset 短同样要求 reset。
        let Some(old) = self.checkpoint.as_ref() else {
            return Ok(false);
        };
        let replaced = if let (Some(prefix), Some(anchor)) = (&old.prefix_hash, &old.anchor_hash) {
            let stored = reader::StoredFingerprint {
                committed_offset: old.committed_offset as u64,
                observed_size: old.observed_size as u64,
                prefix_hash: prefix.clone(),
                prefix_len: old.prefix_len as u64,
                anchor_hash: anchor.clone(),
                anchor_start: old.anchor_start as u64,
                anchor_len: old.anchor_len as u64,
            };
            matches!(
                reader::classify_change(&self.path, &stored)
                    .map_err(|_| "stats_workbuddy_source_changed")?,
                reader::FileChange::Replaced | reader::FileChange::Truncated
            )
        } else {
            old.committed_offset > 0
        };
        Ok(replaced || old.committed_offset > self.size)
    }

    fn position(&self, cursor: Option<&FullCursor>) -> Result<ReadPosition, &'static str> {
        // 增量扫描从 committed_offset 继续；替换/截断时从 0 开始。full scan 不复用
        // committed_offset 来前进，而优先使用 scan cursor；没有匹配 cursor 时再退回
        // checkpoint 的 scan_offset。
        //
        // 若恢复点落在非换行边界，普通数据保守回到 0，避免把半行当完整记录；超长行
        // 的 discarding 状态可以由 LineReader 继续安全排水。
        let reset = self.reset_required()?;
        let primary_offset = if reset {
            0
        } else {
            self.checkpoint
                .as_ref()
                .map_or(0, |old| old.committed_offset)
        };
        let mut position = ReadPosition {
            reset,
            primary_offset,
            offset: primary_offset,
            discarding: cursor.is_none()
                && !reset
                && self
                    .checkpoint
                    .as_ref()
                    .is_some_and(|old| old.discarding_oversized),
        };
        if let (Some(cursor), Some(old)) = (cursor, self.checkpoint.as_ref()) {
            position.offset = if reset {
                0
            } else {
                old.scan_offset.min(self.size)
            };
            if !reset && cursor.generation == old.generation && cursor.offset.is_some() {
                position.offset = cursor.offset.ok_or("stats_workbuddy_stale_batch")?;
                position.discarding = cursor.discarding;
            } else if position.offset > 0 && !self.at_line_boundary(position.offset)? {
                // A crash inside an oversized drain cannot fabricate a record.
                position.offset = 0;
            }
        }
        Ok(position)
    }

    fn at_line_boundary(&self, offset: i64) -> Result<bool, &'static str> {
        use std::io::{Read, Seek, SeekFrom};
        let mut file =
            std::fs::File::open(&self.path).map_err(|_| "stats_workbuddy_source_read_failed")?;
        file.seek(SeekFrom::Start(offset as u64 - 1))
            .map_err(|_| "stats_workbuddy_source_read_failed")?;
        let mut last = [0];
        file.read_exact(&mut last)
            .map_err(|_| "stats_workbuddy_source_read_failed")?;
        Ok(last[0] == b'\n')
    }

    fn finish(
        self,
        relative: &str,
        position: ReadPosition,
        slice: Slice,
        full: bool,
    ) -> Result<Work, &'static str> {
        // 读取结束后重新检查 size 与 mtime，变化则拒绝把这一片交给 writer。随后按
        // 普通提交 offset 或 full scan 的主 offset 生成 prefix/anchor fingerprint。
        // 这个有限窗口用于识别替换和截断，并不扫描整个文件，也不能保证发现任意
        // 同大小中段改写；writer 提交时仍会另做 generation、revision、base offset
        // 和双配置版本校验。
        //
        // full scan 的 `offset` 选择 primary_offset，因为验证读取不能直接推进增量
        // 提交位置；此片验证到哪里由 scan_offset 单独携带。
        let after =
            std::fs::symlink_metadata(&self.path).map_err(|_| "stats_workbuddy_source_changed")?;
        if after.len() != self.metadata.len() || modified_ms(&after)? != self.modified {
            return Err("stats_workbuddy_source_changed");
        }
        let offset = if full {
            position.primary_offset
        } else {
            slice.offset
        };
        let fingerprint = reader::fingerprint(&self.path, offset as u64)
            .map_err(|_| "stats_workbuddy_source_changed")?;
        Ok(Work::Batch(Box::new(FileBatch {
            namespace_id: self.settings.namespace_id,
            settings_revision: self.settings.revision,
            mapping_revision: self.settings.mapping_revision,
            file_key: self.key,
            relative_path: relative.into(),
            expected: self.checkpoint,
            reset: position.reset,
            offset,
            observed_size: self.size,
            modified_ms: self.modified,
            fingerprint,
            discarding: slice.discarding,
            at_eof: slice.eof || slice.offset == self.size,
            candidates: slice.candidates,
            diagnostics: slice.diagnostics,
            scan_offset: full.then_some(slice.offset),
        })))
    }
}

fn read_slice(
    snapshot: &SourceSnapshot,
    position: &ReadPosition,
    shared: &Shared,
) -> Result<Slice, &'static str> {
    // 本循环用 MAX_BATCH_ITEMS、SLICE_BYTES 和 SLICE_TIME 决定何时切批：候选数、
    // 已消费字节和约 50ms 时间都在循环边界参与判断，不是数据库式硬 deadline。
    // 单次 next_bounded_line 或 JSON 处理可能跨过时间目标；8 MiB 也是这里的切批
    // 目标，具体跨度和单次读取行为以 LineReader::next_bounded_line 为准。
    //
    // LineReader 只有读到换行才给出完整 LineStep::Line。文件末尾没有换行时返回
    // End，slice.offset 仍停在最后一个完整换行，下一轮从同一位置重读并等待追加；
    // 唯一允许跨过无换行位置的是明确记录中的超长行排水状态。
    let stem = snapshot
        .path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .ok_or("stats_workbuddy_invalid_path")?;
    let mut source = reader::LineReader::open(&snapshot.path, position.offset as u64)
        .map_err(|_| "stats_workbuddy_source_read_failed")?;
    source.resume_discarding(position.discarding);
    let started = Instant::now();
    let mut slice = Slice {
        offset: position.offset,
        discarding: position.discarding,
        eof: false,
        candidates: Vec::new(),
        diagnostics: types::DiagnosticCounts::default(),
    };
    while shared.active()
        && slice.candidates.len() < types::MAX_BATCH_ITEMS
        && started.elapsed() < SLICE_TIME
    {
        let consumed = (slice.offset - position.offset) as u64;
        if consumed >= SLICE_BYTES {
            break;
        }
        match source
            .next_bounded_line(SLICE_BYTES - consumed)
            .map_err(|_| "stats_workbuddy_source_read_failed")?
        {
            reader::LineStep::End => {
                // 普通未终止尾行没有形成记录，不能推进可提交 offset。
                slice.eof = true;
                break;
            }
            reader::LineStep::Yield {
                position,
                discarding,
            } => {
                if discarding {
                    slice.offset = position as i64;
                    slice.discarding = true;
                }
                slice.eof = position >= snapshot.size as u64;
                break;
            }
            reader::LineStep::Line(line) => {
                slice.offset = line.end as i64;
                slice.discarding = false;
                if line.oversized {
                    // 超长完整行只增加诊断计数；其 waste/drain 位置由读取器状态和
                    // 后续 checkpoint 处理，候选不会进入 writer。
                    slice
                        .diagnostics
                        .add(types::DiagnosticKind::OversizedLine, 1);
                    continue;
                }
                let parsed =
                    parser::evaluate(&line.bytes, stem, &snapshot.settings.namespace_id, now_ms());
                if let Some(diagnostic) = parsed.diagnostic() {
                    slice.diagnostics.add(diagnostic, 1);
                }
                if let types::ParseDisposition::Candidate(candidate) = parsed {
                    slice.candidates.push(*candidate);
                }
            }
        }
    }
    shared.progress.lock().unwrap().body_bytes_read += source.bytes_read();
    Ok(slice)
}
