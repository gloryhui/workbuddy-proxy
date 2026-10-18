//! A bounded source slice, with separate snapshot, resume and commit phases.
use super::*;
use crate::stats::workbuddy_import::parser;

struct SourceSnapshot {
    path: std::path::PathBuf,
    metadata: std::fs::Metadata,
    size: i64,
    modified: i64,
    settings: store::ImportSettings,
    key: String,
    checkpoint: Option<store::Checkpoint>,
}

struct ReadPosition {
    reset: bool,
    primary_offset: i64,
    offset: i64,
    discarding: bool,
}

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
