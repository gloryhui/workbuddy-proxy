//! Restricted directory discovery for the WorkBuddy JSONL reader.
//!
//! Only `projects/<one project level>/*.jsonl` under the WorkBuddy root is ever
//! entered. Tool results, traces, storage, SQLite files and backups are not
//! read, and symlinks are never followed — not for a project directory, not for
//! a file. Iteration is lazy so a large project directory costs one entry at a
//! time, and [`DISCOVERY_SLICE`] bounds how many entries one scheduling slice
//! handles.
//!
//! WB-RT-001: the core background cadence is a fixed [`SYNC_INTERVAL`] of five
//! seconds for both incremental sync and new-file discovery. It is deliberately
//! independent of the page refresh options (off / 5 / 10 / 30 / 60 seconds),
//! there is no "realtime" option, and the period is not user configurable.

use crate::stats::workbuddy_import::types::{DiagnosticKind, ParseDisposition};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// WB-RT-001: the fixed core background cadence.
///
/// This is a compile-time constant on purpose: no setting, profile or UI choice
/// can change it, and it is unrelated to the page refresh interval.
pub const SYNC_INTERVAL: Duration = Duration::from_secs(5);

/// How many discovery entries one scheduling slice handles.
pub const DISCOVERY_SLICE: usize = 100;

/// The only directory under the root that is ever entered.
pub const PROJECTS_DIR: &str = "projects";

/// The only file extension that is ever considered.
pub const JSONL_EXTENSION: &str = ".jsonl";

/// One JSONL file found under the restricted root.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiscoveredFile {
    /// Path relative to the WorkBuddy root, `/` separated.
    pub relative_path: String,
    /// Project directory name (exactly one level).
    pub project: String,
    /// Session id: the file stem, which the parser checks against `sessionId`.
    pub session_id: String,
    /// Size in bytes observed at discovery time.
    pub size_bytes: u64,
}

/// True when a root-relative path is inside the allowed shape.
///
/// `projects/<project>/<session>.jsonl` and nothing else: no second project
/// level, no other top-level directory, no other extension, no empty component.
pub fn is_allowed_relative(relative: &str) -> bool {
    let mut parts = relative.split('/');
    if parts.next() != Some(PROJECTS_DIR) {
        return false;
    }
    let Some(project) = parts.next() else {
        return false;
    };
    if project.is_empty() {
        return false;
    }
    let Some(file) = parts.next() else {
        return false;
    };
    if parts.next().is_some() {
        return false;
    }
    file.len() > JSONL_EXTENSION.len() && file.ends_with(JSONL_EXTENSION)
}

/// The restricted reader root.
#[derive(Clone, Debug)]
pub struct ReaderRoot {
    root: PathBuf,
}

impl ReaderRoot {
    /// The production root: `<home>/.workbuddy`.
    pub fn default_root() -> Result<Self, String> {
        Ok(Self::new(crate::config::home()?.join(".workbuddy")))
    }

    /// A root injected by a test or by the isolated integration harness.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// The root directory.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Starts a lazy discovery walk.
    ///
    /// Nothing is read until the iterator is advanced, and the walk is repeated
    /// from the top on every call, so a project directory created while the
    /// process is running is picked up by the next slice.
    pub fn discover(&self) -> Discovery {
        let projects_path = self.root.join(PROJECTS_DIR);
        // `read_dir` follows a symlinked final component, so the check has to
        // happen on the link itself before opening the directory.
        let projects = match std::fs::symlink_metadata(&projects_path) {
            Ok(metadata) if !metadata.file_type().is_symlink() && metadata.is_dir() => {
                std::fs::read_dir(&projects_path).ok()
            }
            _ => None,
        };
        Discovery {
            root: self.root.clone(),
            projects,
            current: None,
            last_error: None,
        }
    }
}

/// Lazy iterator over the allowed JSONL files.
///
/// Unreadable entries are skipped rather than aborting the walk: one bad
/// directory must not stop the sync of the others.
pub struct Discovery {
    root: PathBuf,
    projects: Option<std::fs::ReadDir>,
    current: Option<(String, std::fs::ReadDir)>,
    last_error: Option<std::io::ErrorKind>,
}

/// Applies every per-entry rule to one candidate path.
///
/// Kept free of iterator state so the same rules run on the walk and in the
/// startup contract check.
fn file_from_entry(root: &Path, project: &str, path: &Path) -> Option<DiscoveredFile> {
    let metadata = std::fs::symlink_metadata(path).ok()?;
    // A symlink is never followed, whether it points at a file or a directory
    // outside the root.
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return None;
    }
    let stem = path.file_stem().and_then(|stem| stem.to_str())?;
    let relative = path
        .strip_prefix(root)
        .ok()?
        .to_string_lossy()
        .replace('\\', "/");
    if !is_allowed_relative(&relative) {
        return None;
    }
    Some(DiscoveredFile {
        relative_path: relative,
        project: project.to_owned(),
        session_id: stem.to_owned(),
        size_bytes: metadata.len(),
    })
}

impl Discovery {
    /// A fixed, non-sensitive I/O outcome. One unreadable entry must neither
    /// abort unrelated files nor let the status claim a successful full check.
    pub fn take_error(&mut self) -> Option<std::io::ErrorKind> {
        self.last_error.take()
    }

    /// Visits at most `budget` entries, including ignored entries. The caller
    /// retains this iterator when `(None,false)` asks it to yield.
    pub fn next_bounded(&mut self, budget: usize) -> (Option<DiscoveredFile>, bool) {
        for _ in 0..budget {
            if let Some((project, mut entries)) = self.current.take() {
                match entries.next() {
                    Some(Ok(entry)) => {
                        let path = entry.path();
                        let readable = match std::fs::symlink_metadata(&path) {
                            Ok(_) => true,
                            Err(error) => {
                                self.last_error = Some(error.kind());
                                false
                            }
                        };
                        let found = if readable {
                            file_from_entry(&self.root, &project, &path)
                        } else {
                            None
                        };
                        self.current = Some((project, entries));
                        if let Some(file) = found {
                            return (Some(file), false);
                        }
                    }
                    Some(Err(error)) => {
                        self.last_error = Some(error.kind());
                        self.current = Some((project, entries));
                    }
                    None => {}
                }
                continue;
            }
            let Some(projects) = self.projects.as_mut() else {
                return (None, true);
            };
            let Some(entry) = projects.next() else {
                self.projects = None;
                return (None, true);
            };
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    self.last_error = Some(error.kind());
                    continue;
                }
            };
            let path = entry.path();
            let metadata = match std::fs::symlink_metadata(&path) {
                Ok(metadata) => metadata,
                Err(error) => {
                    self.last_error = Some(error.kind());
                    continue;
                }
            };
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                continue;
            }
            let Some(project) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            match std::fs::read_dir(&path) {
                Ok(entries) => self.current = Some((project.to_owned(), entries)),
                Err(error) => self.last_error = Some(error.kind()),
            }
        }
        (None, false)
    }
}

impl Iterator for Discovery {
    type Item = DiscoveredFile;
    fn next(&mut self) -> Option<DiscoveredFile> {
        loop {
            let (file, finished) = self.next_bounded(DISCOVERY_SLICE);
            if file.is_some() || finished {
                return file;
            }
        }
    }
}

/// Bytes of the file head that are hashed as the stable prefix.
pub const PREFIX_LEN: u64 = 4096;

/// Bytes immediately before the committed offset that are hashed as the anchor.
///
/// The anchor proves that the already-imported region is still the same bytes,
/// so an append can be told apart from a rewrite of the same length.
pub const ANCHOR_LEN: u64 = 256;

/// What a checkpoint remembers about one file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredFingerprint {
    /// Offset that has already been committed.
    pub committed_offset: u64,
    /// Size observed when the checkpoint was written.
    pub observed_size: u64,
    pub prefix_hash: String,
    pub prefix_len: u64,
    pub anchor_hash: String,
    pub anchor_start: u64,
    pub anchor_len: u64,
}

/// The fingerprint of the file as it is now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileFingerprint {
    pub prefix_hash: String,
    pub prefix_len: u64,
    pub anchor_hash: String,
    pub anchor_start: u64,
    pub anchor_len: u64,
}

/// How a file changed since its checkpoint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileChange {
    /// Same size and same bytes.
    Unchanged,
    /// New bytes exist after the committed offset.
    Appended,
    /// The file is shorter than the checkpoint remembers.
    Truncated,
    /// The head or the committed region now holds different bytes.
    Replaced,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// SHA256 of `len` bytes starting at `start`, read in bounded chunks.
fn hash_range(path: &Path, start: u64, len: u64) -> std::io::Result<String> {
    use std::io::{Read, Seek, SeekFrom};

    let mut file = std::fs::File::open(path)?;
    if start > 0 {
        file.seek(SeekFrom::Start(start))?;
    }
    let mut hasher = Sha256::new();
    let mut remaining = len;
    let mut buffer = [0u8; 8192];
    while remaining > 0 {
        let want = remaining.min(buffer.len() as u64) as usize;
        let read = file.read(&mut buffer[..want])?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        remaining -= read as u64;
    }
    Ok(hex(&hasher.finalize()))
}

/// Reads the fingerprint to store for a file with this committed offset.
///
/// The prefix covers `min(size, PREFIX_LEN)` bytes, so a small file is fully
/// covered; the anchor covers the `ANCHOR_LEN` bytes just before the committed
/// offset (or from the start when less has been committed).
pub fn fingerprint(path: &Path, committed_offset: u64) -> std::io::Result<FileFingerprint> {
    let size = std::fs::symlink_metadata(path)?.len();
    let prefix_len = size.min(PREFIX_LEN);
    let anchor_len = committed_offset.min(ANCHOR_LEN);
    Ok(FileFingerprint {
        prefix_hash: hash_range(path, 0, prefix_len)?,
        prefix_len,
        anchor_hash: hash_range(path, committed_offset - anchor_len, anchor_len)?,
        anchor_start: committed_offset - anchor_len,
        anchor_len,
    })
}

/// The pure decision table, shared by the walk and the startup check.
///
/// `head_matches` and `anchor_matches` are only consulted once the file is long
/// enough to contain the remembered regions.
pub fn decide(
    current_size: u64,
    observed_size: u64,
    head_matches: bool,
    anchor_matches: bool,
) -> FileChange {
    if !head_matches || !anchor_matches {
        return FileChange::Replaced;
    }
    match current_size.cmp(&observed_size) {
        std::cmp::Ordering::Greater => FileChange::Appended,
        std::cmp::Ordering::Less => FileChange::Truncated,
        std::cmp::Ordering::Equal => FileChange::Unchanged,
    }
}

/// Compares a checkpoint with the file on disk.
///
/// The remembered prefix is re-hashed over **its own** length rather than over
/// the current one, so a small file that merely grew is an append and never a
/// replacement. A symlink, a directory or a missing file is reported as
/// `Replaced`: nothing is read from it and the caller must not append.
pub fn classify_change(path: &Path, stored: &StoredFingerprint) -> std::io::Result<FileChange> {
    let metadata = std::fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Ok(FileChange::Replaced);
    }
    let size = metadata.len();
    // Anything shorter than what has already been committed is a truncation.
    if size < stored.committed_offset {
        return Ok(FileChange::Truncated);
    }
    // A shorter file cannot still contain the remembered regions.
    if size < stored.prefix_len || size < stored.anchor_start + stored.anchor_len {
        return Ok(FileChange::Truncated);
    }
    let head_matches = hash_range(path, 0, stored.prefix_len)? == stored.prefix_hash;
    let anchor_matches =
        hash_range(path, stored.anchor_start, stored.anchor_len)? == stored.anchor_hash;
    Ok(decide(
        size,
        stored.observed_size,
        head_matches,
        anchor_matches,
    ))
}

/// Bytes pulled from the file per read.
pub const READ_CHUNK: usize = 64 * 1024;

/// The largest single line that is accepted.
pub const MAX_LINE_BYTES: usize = 4 * 1024 * 1024;

/// UTF-8 byte order mark, skipped only at the very start of a file.
pub const BOM: [u8; 3] = [0xEF, 0xBB, 0xBF];

/// One complete line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    /// Offset of the first byte of the line.
    pub start: u64,
    /// Offset just past the terminating `\n`.
    pub end: u64,
    /// The line without its terminator, a trailing `\r`, or a leading BOM.
    pub bytes: Vec<u8>,
    /// True when the line exceeded [`MAX_LINE_BYTES`]; `bytes` is then empty.
    pub oversized: bool,
}

/// Reads complete lines from a byte offset using a bounded buffer.
///
/// The file is never loaded as a whole: at most [`READ_CHUNK`] bytes are pulled
/// per read, and a line that would exceed [`MAX_LINE_BYTES`] has its content
/// dropped immediately (the reader keeps draining to the next newline so the
/// offset can still advance past it).
///
/// A trailing fragment without a newline is **not** a line: it is never
/// returned, so the committed offset can never move past bytes that may still
/// be growing.
pub struct LineReader {
    file: std::fs::File,
    buffer: Vec<u8>,
    filled: usize,
    cursor: usize,
    position: u64,
    line_start: u64,
    pending: Vec<u8>,
    oversized: bool,
    bytes_read: u64,
}

/// A bounded read either completes one line, exhausts its slice, or reaches EOF.
pub enum LineStep {
    Line(Line),
    Yield { position: u64, discarding: bool },
    End,
}

impl LineReader {
    /// Resumes a previously committed oversized-line drain.
    pub fn resume_discarding(&mut self, discarding: bool) {
        self.oversized = discarding;
    }

    /// Opens a file and positions the reader at `offset`.
    pub fn open(path: &Path, offset: u64) -> std::io::Result<Self> {
        use std::io::{Seek, SeekFrom};

        let mut file = std::fs::File::open(path)?;
        if offset > 0 {
            file.seek(SeekFrom::Start(offset))?;
        }
        Ok(Self {
            file,
            buffer: vec![0u8; READ_CHUNK],
            filled: 0,
            cursor: 0,
            position: offset,
            line_start: offset,
            pending: Vec::new(),
            oversized: false,
            bytes_read: 0,
        })
    }

    /// Bytes currently held for the line in progress.
    ///
    /// Bounded by `MAX_LINE_BYTES + READ_CHUNK` regardless of how large the
    /// line on disk is; exposed so the bound can be asserted.
    pub fn buffered_bytes(&self) -> usize {
        self.pending.capacity() + self.buffer.len()
    }

    /// Actual bytes returned by source read calls, including prefetched bytes.
    pub fn bytes_read(&self) -> u64 {
        self.bytes_read
    }

    /// Adds one segment of the current line, dropping everything past the limit.
    fn push_segment(&mut self, segment: &[u8]) {
        if self.oversized {
            return;
        }
        if self.pending.len() + segment.len() > MAX_LINE_BYTES {
            self.oversized = true;
            // Release the bytes already buffered for this line right away.
            self.pending = Vec::new();
            return;
        }
        self.pending.extend_from_slice(segment);
    }

    /// Completes the current line and advances the line cursor.
    fn finish(&mut self) -> Line {
        let start = self.line_start;
        let end = self.position;
        let oversized = self.oversized;
        let mut bytes = std::mem::take(&mut self.pending);
        self.oversized = false;
        self.line_start = end;
        if !oversized {
            if bytes.last() == Some(&b'\r') {
                bytes.pop();
            }
            if start == 0 && bytes.starts_with(&BOM) {
                bytes.drain(..BOM.len());
            }
        }
        Line {
            start,
            end,
            bytes,
            oversized,
        }
    }

    /// Returns the next complete line, or `None` at end of file.
    pub fn next_line(&mut self) -> std::io::Result<Option<Line>> {
        match self.next_bounded_line(u64::MAX)? {
            LineStep::Line(line) => Ok(Some(line)),
            LineStep::Yield { .. } | LineStep::End => Ok(None),
        }
    }

    /// Limits a single call even when one oversized line is many gigabytes.
    /// Only a drain may advance the durable offset without a newline; ordinary
    /// partial lines remain uncommitted and are reread after the next append.
    pub fn next_bounded_line(&mut self, byte_budget: u64) -> std::io::Result<LineStep> {
        use std::io::Read;

        let started = self.position;
        loop {
            if self.position.saturating_sub(started) >= byte_budget {
                return Ok(LineStep::Yield {
                    position: self.position,
                    discarding: self.oversized,
                });
            }
            if self.cursor == self.filled {
                let remaining = byte_budget.saturating_sub(self.position - started);
                let count = remaining.min(self.buffer.len() as u64) as usize;
                self.filled = self.file.read(&mut self.buffer[..count])?;
                self.bytes_read += self.filled as u64;
                self.cursor = 0;
                if self.filled == 0 {
                    // An unterminated tail is not a line; dropping it keeps the
                    // committed offset behind the last newline.
                    if self.oversized {
                        return Ok(LineStep::Yield {
                            position: self.position,
                            discarding: true,
                        });
                    }
                    self.pending = Vec::new();
                    return Ok(LineStep::End);
                }
            }
            let remaining = byte_budget.saturating_sub(self.position - started);
            let end = self.cursor
                + (self.filled - self.cursor).min(remaining.min(usize::MAX as u64) as usize);
            let window = &self.buffer[self.cursor..end];
            match window.iter().position(|byte| *byte == b'\n') {
                Some(index) => {
                    let segment = self.buffer[self.cursor..self.cursor + index].to_vec();
                    self.cursor += index + 1;
                    self.position += index as u64 + 1;
                    self.push_segment(&segment);
                    return Ok(LineStep::Line(self.finish()));
                }
                None => {
                    let segment = window.to_vec();
                    let consumed = segment.len();
                    self.cursor = end;
                    self.position += consumed as u64;
                    self.push_segment(&segment);
                }
            }
        }
    }
}

/// One line's outcome together with the offset it allows committing.
///
/// `commit_to` is the end of a **complete** line, so committing it can never
/// move the checkpoint past bytes that may still be growing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReadOutcome {
    /// The line's disposition; `None` when the line was oversized.
    pub disposition: Option<ParseDisposition>,
    /// True when the line exceeded [`MAX_LINE_BYTES`] and was drained.
    pub oversized: bool,
    /// Offset to commit once this line has been handled.
    pub commit_to: u64,
}

impl ReadOutcome {
    /// The one diagnostic counter this outcome contributes, if any.
    ///
    /// An oversized line counts as `oversized_line`; a bad complete line counts
    /// as its parse disposition. Either way the line is counted **once**, and
    /// because its offset is committed the count does not repeat after a
    /// restart.
    pub fn diagnostic(&self) -> Option<DiagnosticKind> {
        if self.oversized {
            return Some(DiagnosticKind::OversizedLine);
        }
        self.disposition
            .as_ref()
            .and_then(ParseDisposition::diagnostic)
    }
}

/// Reads up to `limit` complete lines starting at `offset`.
///
/// Every returned line is complete, so every one of them may advance the
/// checkpoint — including a malformed one, which is counted and stepped over
/// instead of blocking the file forever. An unterminated tail is not returned at
/// all, so a record split across several writes is imported exactly once, when
/// its newline finally arrives.
pub fn read_lines(
    path: &Path,
    offset: u64,
    file_stem: &str,
    namespace_id: &str,
    now_ms: i64,
    limit: usize,
) -> std::io::Result<Vec<ReadOutcome>> {
    let mut reader = LineReader::open(path, offset)?;
    let mut outcomes = Vec::new();
    while outcomes.len() < limit {
        let Some(line) = reader.next_line()? else {
            break;
        };
        let disposition = if line.oversized {
            None
        } else {
            Some(crate::stats::workbuddy_import::parser::evaluate(
                &line.bytes,
                file_stem,
                namespace_id,
                now_ms,
            ))
        };
        outcomes.push(ReadOutcome {
            disposition,
            oversized: line.oversized,
            commit_to: line.end,
        });
    }
    Ok(outcomes)
}

/// Bytes one file may contribute to a single slice before yielding.
pub const PER_FILE_BYTES: u64 = 8 * 1024 * 1024;

/// Wall-clock budget one file may consume in a single slice before yielding.
pub const PER_FILE_TIME: Duration = Duration::from_millis(50);

/// How many files may be active in one slice.
pub const ACTIVE_WINDOW: usize = 32;

/// What one file's turn in a slice produced.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TurnResult {
    /// Bytes read during the turn.
    pub bytes: u64,
    /// Time the turn took.
    pub elapsed: Duration,
    /// True when complete lines are still waiting in the file.
    pub has_backlog: bool,
}

impl TurnResult {
    /// True when the turn must yield: either budget is a limit, not a target.
    pub fn exhausted(&self) -> bool {
        self.bytes >= PER_FILE_BYTES || self.elapsed >= PER_FILE_TIME
    }
}

/// Plans one slice fairly across the active files.
///
/// Two rules keep a single growing file from monopolising the importer:
///
/// * a file yields as soon as it has read [`PER_FILE_BYTES`] or spent
///   [`PER_FILE_TIME`], so one huge file cannot consume the whole slice;
/// * files that are already caught up are served **before** the backlog, and the
///   backlog itself is served in rotation, so neither a continuously growing
///   file nor a long backfill can starve the other kind.
///
/// At most [`ACTIVE_WINDOW`] files take part in one slice.
#[derive(Debug, Default)]
pub struct FairScheduler {
    /// Index the next backlog rotation starts from.
    rotation: usize,
    /// Files known to still have unread lines.
    backlog: Vec<String>,
}

impl FairScheduler {
    /// A scheduler with an empty backlog.
    pub fn new() -> Self {
        Self::default()
    }

    /// Files that are currently behind.
    pub fn backlog_len(&self) -> usize {
        self.backlog.len()
    }

    /// Orders this slice: caught-up increments first, then the rotating backlog.
    pub fn plan<'a>(&mut self, files: &'a [DiscoveredFile]) -> Vec<&'a DiscoveredFile> {
        let window = &files[..files.len().min(ACTIVE_WINDOW)];
        let (caught_up, behind): (Vec<&DiscoveredFile>, Vec<&DiscoveredFile>) = window
            .iter()
            .partition(|file| !self.backlog.iter().any(|path| path == &file.relative_path));
        let mut plan = caught_up;
        if behind.is_empty() {
            self.rotation = 0;
            return plan;
        }
        let start = self.rotation % behind.len();
        plan.extend(
            behind
                .iter()
                .cycle()
                .skip(start)
                .take(behind.len())
                .copied(),
        );
        self.rotation = (start + 1) % behind.len();
        plan
    }

    /// Records the outcome of one turn, updating who is behind.
    pub fn record(&mut self, relative_path: &str, result: &TurnResult) {
        let known = self.backlog.iter().any(|path| path == relative_path);
        match (result.has_backlog, known) {
            (true, false) => self.backlog.push(relative_path.to_owned()),
            (false, true) => self.backlog.retain(|path| path != relative_path),
            _ => {}
        }
    }
}

/// One batch read from a file, ready to be verified and committed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReadBatch {
    /// Offset the batch started from.
    pub base_offset: u64,
    /// Offset to commit once the batch is accepted.
    pub end_offset: u64,
    pub outcomes: Vec<ReadOutcome>,
}

impl ReadBatch {
    /// True when the file had no complete new line.
    pub fn is_empty(&self) -> bool {
        self.outcomes.is_empty()
    }

    /// How many candidates the batch carries.
    pub fn imported(&self) -> usize {
        self.outcomes
            .iter()
            .filter(|outcome| {
                outcome
                    .disposition
                    .as_ref()
                    .is_some_and(ParseDisposition::is_candidate)
            })
            .count()
    }

    /// How many diagnostic counters the batch contributes.
    pub fn diagnostics(&self) -> usize {
        self.outcomes
            .iter()
            .filter(|outcome| outcome.diagnostic().is_some())
            .count()
    }
}

/// Why a batch must not be committed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BatchRejection {
    /// The file was replaced or truncated: the bytes are not the same file.
    SourceChanged,
    /// The file could not be read (permissions, missing, not a regular file).
    Unreadable,
}

/// Re-checks that the file still is the one the checkpoint describes.
///
/// Called both before and after reading, so a rename, a replacement or a
/// truncation that happens **during** the read also discards the batch instead
/// of letting half of the old bytes reach the writer. The offset is only ever
/// advanced by a batch that passed both checks.
pub fn verify_batch(path: &Path, stored: &StoredFingerprint) -> Result<(), BatchRejection> {
    match classify_change(path, stored) {
        Ok(FileChange::Unchanged) | Ok(FileChange::Appended) => Ok(()),
        Ok(FileChange::Truncated) | Ok(FileChange::Replaced) => Err(BatchRejection::SourceChanged),
        Err(_) => Err(BatchRejection::Unreadable),
    }
}

/// Reads the next batch of one file and verifies it twice.
///
/// A permission failure, a missing file, a rename or a truncation returns a
/// rejection and **no** batch, so the caller keeps the old offset and re-reads
/// later: no half batch of stale bytes can be committed, and no line is lost
/// because nothing was advanced.
pub fn read_verified_batch(
    path: &Path,
    stored: &StoredFingerprint,
    file_stem: &str,
    namespace_id: &str,
    now_ms: i64,
    limit: usize,
) -> Result<ReadBatch, BatchRejection> {
    verify_batch(path, stored)?;
    let outcomes = read_lines(
        path,
        stored.committed_offset,
        file_stem,
        namespace_id,
        now_ms,
        limit,
    )
    .map_err(|_| BatchRejection::Unreadable)?;
    // The file may have been replaced while it was being read.
    verify_batch(path, stored)?;
    let end_offset = outcomes
        .last()
        .map(|outcome| outcome.commit_to)
        .unwrap_or(stored.committed_offset);
    Ok(ReadBatch {
        base_offset: stored.committed_offset,
        end_offset,
        outcomes,
    })
}

/// How often a scheduled full rescan comes due.
pub const FULL_RESCAN_INTERVAL_MS: i64 = 24 * 60 * 60 * 1000;

/// What started a full rescan.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RescanTrigger {
    /// The daily reconciliation came due.
    Daily,
    /// The user asked for a full check.
    Manual,
}

impl RescanTrigger {
    /// True when this trigger should start a pass now.
    pub fn is_due(self, cursor: &ScanCursor, now_ms: i64) -> bool {
        if cursor.is_running() {
            return false;
        }
        match self {
            RescanTrigger::Manual => true,
            RescanTrigger::Daily => cursor.is_due(now_ms),
        }
    }
}

/// The cursor a full rescan walks with.
///
/// It is deliberately independent of the incremental checkpoint: a rescan only
/// *reads*, so it can be interrupted, repeated or abandoned without moving
/// `committed_offset` by a single byte. Re-read rows are handled by the same
/// parser and the same ledger as normal syncing, so a rescan can never
/// double-count.
#[derive(Debug, Default)]
pub struct ScanCursor {
    /// Offset reached inside the file currently being rescanned.
    offset: u64,
    /// File index the next pass starts from, so repeated passes rotate.
    rotation: usize,
    /// When the last completed pass finished.
    last_full_ms: Option<i64>,
    /// True while a pass is in progress.
    running: bool,
}

impl ScanCursor {
    /// A cursor that has never completed a pass.
    pub fn new() -> Self {
        Self::default()
    }

    /// Offset reached inside the current file.
    pub fn offset(&self) -> u64 {
        self.offset
    }

    /// True while a pass is in progress.
    pub fn is_running(&self) -> bool {
        self.running
    }

    /// When the last completed pass finished.
    pub fn last_full_ms(&self) -> Option<i64> {
        self.last_full_ms
    }

    /// File index the next pass starts from.
    pub fn rotation(&self) -> usize {
        self.rotation
    }

    /// True when a scheduled pass is due.
    ///
    /// A cursor that has never completed a pass is due immediately; after that
    /// it comes due once [`FULL_RESCAN_INTERVAL_MS`] has elapsed.
    pub fn is_due(&self, now_ms: i64) -> bool {
        if self.running {
            return false;
        }
        match self.last_full_ms {
            None => true,
            Some(last) => now_ms.saturating_sub(last) >= FULL_RESCAN_INTERVAL_MS,
        }
    }

    /// Starts a pass; returns false when one is already running.
    pub fn begin(&mut self) -> bool {
        if self.running {
            return false;
        }
        self.running = true;
        self.offset = 0;
        true
    }

    /// Records how far the pass has read inside the current file.
    pub fn advance(&mut self, offset: u64) {
        self.offset = offset;
    }

    /// Marks the pass finished and rotates the starting file.
    pub fn finish(&mut self, now_ms: i64) {
        self.running = false;
        self.offset = 0;
        self.last_full_ms = Some(now_ms);
        self.rotation = self.rotation.wrapping_add(1);
    }
}

/// Reads one slice of a full rescan.
///
/// It starts from the rescan's own offset and never consults — let alone
/// updates — the incremental checkpoint, so the two cursors cannot interfere.
/// Every line is parsed exactly like normal syncing, so the ledger, not this
/// function, decides what is new.
pub fn read_full_slice(
    path: &Path,
    scan_offset: u64,
    file_stem: &str,
    namespace_id: &str,
    now_ms: i64,
    limit: usize,
) -> std::io::Result<ReadBatch> {
    let outcomes = read_lines(path, scan_offset, file_stem, namespace_id, now_ms, limit)?;
    let end_offset = outcomes
        .last()
        .map(|outcome| outcome.commit_to)
        .unwrap_or(scan_offset);
    Ok(ReadBatch {
        base_offset: scan_offset,
        end_offset,
        outcomes,
    })
}

/// Startup contract check for the reader's discovery rules.
///
/// Pure: it asserts the fixed cadence and the allowed path shape without
/// touching the filesystem, so it is safe on the startup path.
pub fn reader_contract_check() -> Result<usize, &'static str> {
    if SYNC_INTERVAL != Duration::from_secs(5) {
        return Err("workbuddy_import_sync_interval_drifted");
    }
    if DISCOVERY_SLICE != 100 {
        return Err("workbuddy_import_discovery_slice_drifted");
    }
    for allowed in [
        "projects/demo/session-a.jsonl",
        "projects/我的 项目/会话 一.jsonl",
    ] {
        if !is_allowed_relative(allowed) {
            return Err("workbuddy_import_reader_path_refused");
        }
    }
    // The production root is derived from the home directory, exactly like the
    // rest of the application.
    if let Ok(root) = ReaderRoot::default_root()
        && !root.root().ends_with(".workbuddy")
    {
        return Err("workbuddy_import_reader_root_drifted");
    }
    // Exercise the walk and the per-entry rule against a root that cannot
    // exist, so every discovery symbol is reachable on the real startup path
    // without touching the user's filesystem.
    let probe = ReaderRoot::new(std::path::Path::new("/nonexistent-workbuddy-root"));
    if probe.discover().next().is_some() {
        return Err("workbuddy_import_reader_unexpected_entry");
    }
    if file_from_entry(
        probe.root(),
        "demo",
        &probe.root().join("projects/demo/session.jsonl"),
    )
    .is_some()
    {
        return Err("workbuddy_import_reader_unexpected_file");
    }
    // The line reader must be reachable on the startup path without touching a
    // user file: `/dev/null` reads as an immediate end of file.
    if let Ok(mut reader) = LineReader::open(std::path::Path::new("/dev/null"), 0) {
        if reader
            .next_line()
            .map_err(|_| "workbuddy_import_reader_dev_null")?
            .is_some()
        {
            return Err("workbuddy_import_reader_dev_null_yielded");
        }
        if reader.buffered_bytes() > MAX_LINE_BYTES + READ_CHUNK {
            return Err("workbuddy_import_reader_buffer_unbounded");
        }
    }
    if MAX_LINE_BYTES != 4 * 1024 * 1024 || READ_CHUNK != 64 * 1024 {
        return Err("workbuddy_import_reader_bounds_drifted");
    }
    // The batch reader must also be reachable without touching a user file.
    let missing = std::path::Path::new("/nonexistent-workbuddy-batch.jsonl");
    if read_lines(missing, 0, "s", &"0".repeat(48), 0, 8).is_ok() {
        return Err("workbuddy_import_reader_missing_batch_accepted");
    }
    // The fair scheduler must be reachable on the startup path too.
    let mut scheduler = FairScheduler::new();
    let probe_files = [
        DiscoveredFile {
            relative_path: "projects/demo/a.jsonl".into(),
            project: "demo".into(),
            session_id: "a".into(),
            size_bytes: 0,
        },
        DiscoveredFile {
            relative_path: "projects/demo/b.jsonl".into(),
            project: "demo".into(),
            session_id: "b".into(),
            size_bytes: 0,
        },
    ];
    if scheduler.plan(&probe_files).len() != 2 {
        return Err("workbuddy_import_reader_plan_incomplete");
    }
    scheduler.record(
        "projects/demo/a.jsonl",
        &TurnResult {
            bytes: PER_FILE_BYTES,
            elapsed: Duration::ZERO,
            has_backlog: true,
        },
    );
    if scheduler.backlog_len() != 1 {
        return Err("workbuddy_import_reader_backlog_not_tracked");
    }
    // A caught-up file is served before the one that is behind.
    let plan = scheduler.plan(&probe_files);
    if plan.first().map(|file| file.relative_path.as_str()) != Some("projects/demo/b.jsonl") {
        return Err("workbuddy_import_reader_priority_drifted");
    }
    scheduler.record(
        "projects/demo/a.jsonl",
        &TurnResult {
            bytes: 1,
            elapsed: PER_FILE_TIME,
            has_backlog: false,
        },
    );
    if scheduler.backlog_len() != 0 {
        return Err("workbuddy_import_reader_backlog_not_cleared");
    }
    let by_bytes = TurnResult {
        bytes: PER_FILE_BYTES,
        elapsed: Duration::ZERO,
        has_backlog: false,
    };
    let by_time = TurnResult {
        bytes: 0,
        elapsed: PER_FILE_TIME,
        has_backlog: false,
    };
    let within = TurnResult {
        bytes: 0,
        elapsed: Duration::ZERO,
        has_backlog: false,
    };
    if !by_bytes.exhausted() || !by_time.exhausted() || within.exhausted() {
        return Err("workbuddy_import_reader_budget_drifted");
    }
    if PER_FILE_BYTES != 8 * 1024 * 1024
        || PER_FILE_TIME != Duration::from_millis(50)
        || ACTIVE_WINDOW != 32
    {
        return Err("workbuddy_import_reader_window_drifted");
    }
    // The verified-batch path must be reachable without touching a user file:
    // an unreadable path is a rejection, never an empty batch.
    let probe = StoredFingerprint {
        committed_offset: 0,
        observed_size: 0,
        prefix_hash: String::new(),
        prefix_len: 0,
        anchor_hash: String::new(),
        anchor_start: 0,
        anchor_len: 0,
    };
    if verify_batch(missing, &probe) != Err(BatchRejection::Unreadable) {
        return Err("workbuddy_import_reader_unreadable_accepted");
    }
    if read_verified_batch(missing, &probe, "s", &"0".repeat(48), 0, 8)
        != Err(BatchRejection::Unreadable)
    {
        return Err("workbuddy_import_reader_unreadable_batch_accepted");
    }
    // The rescan cursor is independent of the incremental checkpoint.
    let mut cursor = ScanCursor::new();
    if !RescanTrigger::Daily.is_due(&cursor, 0) || !cursor.is_due(0) {
        return Err("workbuddy_import_reader_rescan_not_due");
    }
    if !cursor.begin() || cursor.begin() {
        return Err("workbuddy_import_reader_rescan_overlap");
    }
    cursor.advance(4096);
    if cursor.offset() != 4096 || !cursor.is_running() {
        return Err("workbuddy_import_reader_rescan_cursor_drifted");
    }
    if RescanTrigger::Manual.is_due(&cursor, 0) {
        return Err("workbuddy_import_reader_rescan_overlap_accepted");
    }
    cursor.finish(FULL_RESCAN_INTERVAL_MS);
    if cursor.is_running() || cursor.offset() != 0 || cursor.rotation() != 1 {
        return Err("workbuddy_import_reader_rescan_finish_drifted");
    }
    if cursor.last_full_ms() != Some(FULL_RESCAN_INTERVAL_MS) {
        return Err("workbuddy_import_reader_rescan_time_drifted");
    }
    if cursor.is_due(FULL_RESCAN_INTERVAL_MS) {
        return Err("workbuddy_import_reader_rescan_due_too_soon");
    }
    if !cursor.is_due(FULL_RESCAN_INTERVAL_MS * 2) {
        return Err("workbuddy_import_reader_rescan_never_due");
    }
    if read_full_slice(missing, 0, "s", &"0".repeat(48), 0, 8).is_ok() {
        return Err("workbuddy_import_reader_missing_rescan_accepted");
    }
    let empty = ReadBatch {
        base_offset: 0,
        end_offset: 0,
        outcomes: Vec::new(),
    };
    if !empty.is_empty() || empty.imported() != 0 || empty.diagnostics() != 0 {
        return Err("workbuddy_import_reader_empty_batch_drifted");
    }
    let drained = ReadOutcome {
        disposition: None,
        oversized: true,
        commit_to: 0,
    };
    if drained.diagnostic() != Some(DiagnosticKind::OversizedLine) {
        return Err("workbuddy_import_reader_oversized_diagnostic_drifted");
    }
    let plain = ReadOutcome {
        disposition: Some(ParseDisposition::Ignored),
        oversized: false,
        commit_to: 0,
    };
    if plain.diagnostic() != Some(DiagnosticKind::Ignored) {
        return Err("workbuddy_import_reader_disposition_diagnostic_drifted");
    }
    // The fingerprint path must exist on the startup path, but it must never
    // touch a real file: a path that cannot exist has to fail, not succeed.
    let missing = std::path::Path::new("/nonexistent-workbuddy-file.jsonl");
    if fingerprint(missing, 0).is_ok() {
        return Err("workbuddy_import_reader_missing_fingerprint");
    }
    if classify_change(
        missing,
        &StoredFingerprint {
            committed_offset: 0,
            observed_size: 0,
            prefix_hash: String::new(),
            prefix_len: 0,
            anchor_hash: String::new(),
            anchor_start: 0,
            anchor_len: 0,
        },
    )
    .is_ok()
    {
        return Err("workbuddy_import_reader_missing_file_accepted");
    }
    if decide(10, 10, true, true) != FileChange::Unchanged
        || decide(11, 10, true, true) != FileChange::Appended
        || decide(9, 10, true, true) != FileChange::Truncated
        || decide(10, 10, false, true) != FileChange::Replaced
    {
        return Err("workbuddy_import_reader_change_table_drifted");
    }
    for refused in [
        "traces/trace.jsonl",
        "tool-results/tool.jsonl",
        "storage/session.jsonl",
        "backups/session.jsonl",
        "projects/demo/nested/session.jsonl",
        "projects/session.txt",
        "projects/.jsonl",
        "projects//session.jsonl",
        "projects/",
        "session.jsonl",
        "",
    ] {
        if is_allowed_relative(refused) {
            return Err("workbuddy_import_reader_path_accepted");
        }
    }
    Ok(1)
}

#[cfg(test)]
mod discovery_tests {
    use super::*;
    use std::fs;

    fn root_with(structure: &[(&str, &str)]) -> (tempfile::TempDir, ReaderRoot) {
        let folder = tempfile::tempdir().unwrap();
        for (relative, body) in structure {
            let path = folder.path().join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, body).unwrap();
        }
        let root = ReaderRoot::new(folder.path());
        (folder, root)
    }

    fn found(root: &ReaderRoot) -> Vec<String> {
        let mut paths: Vec<String> = root.discover().map(|file| file.relative_path).collect();
        paths.sort();
        paths
    }

    #[test]
    fn a_chinese_path_with_spaces_is_read() {
        // AC025: a path with CJK characters and spaces must be readable.
        let (_folder, root) = root_with(&[
            ("projects/我的 项目/会话 一.jsonl", "{}\n"),
            ("projects/demo/session-a.jsonl", "{}\n"),
        ]);
        let paths = found(&root);
        assert_eq!(paths.len(), 2, "{paths:?}");
        assert!(paths.contains(&"projects/我的 项目/会话 一.jsonl".to_string()));

        let files: Vec<DiscoveredFile> = root.discover().collect();
        let chinese = files
            .iter()
            .find(|file| file.project == "我的 项目")
            .expect("the CJK project must be discovered");
        assert_eq!(chinese.session_id, "会话 一");
        assert_eq!(chinese.size_bytes, 3);
    }

    #[test]
    fn traces_tool_results_and_storage_are_never_entered() {
        // AC025: these trees are outside the contract and must not be read even
        // when they contain perfectly valid-looking JSONL files.
        let (_folder, root) = root_with(&[
            ("projects/demo/session-a.jsonl", "{}\n"),
            ("traces/trace.jsonl", "{}\n"),
            ("tool-results/tool.jsonl", "{}\n"),
            ("storage/session.jsonl", "{}\n"),
            ("backups/session.jsonl", "{}\n"),
            ("projects/demo/nested/deeper.jsonl", "{}\n"),
            ("projects/notes.txt", "not jsonl\n"),
            ("session.jsonl", "{}\n"),
        ]);
        assert_eq!(
            found(&root),
            vec!["projects/demo/session-a.jsonl".to_string()]
        );
    }

    #[test]
    fn symlinks_are_not_followed_for_files_or_directories() {
        let folder = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        // A real file outside the root, reachable through two kinds of link.
        fs::write(outside.path().join("external.jsonl"), "{}\n").unwrap();
        fs::create_dir_all(folder.path().join("projects/demo")).unwrap();
        fs::write(folder.path().join("projects/demo/real.jsonl"), "{}\n").unwrap();
        std::os::unix::fs::symlink(
            outside.path().join("external.jsonl"),
            folder.path().join("projects/demo/linked.jsonl"),
        )
        .unwrap();
        std::os::unix::fs::symlink(outside.path(), folder.path().join("projects/external"))
            .unwrap();

        let root = ReaderRoot::new(folder.path());
        assert_eq!(found(&root), vec!["projects/demo/real.jsonl".to_string()]);

        // A symlinked `projects` directory itself is not entered either.
        let aliased = tempfile::tempdir().unwrap();
        fs::create_dir_all(aliased.path().join("elsewhere")).unwrap();
        std::os::unix::fs::symlink(
            folder.path().join("projects"),
            aliased.path().join("projects"),
        )
        .unwrap();
        assert!(found(&ReaderRoot::new(aliased.path())).is_empty());
    }

    #[test]
    fn new_files_and_directories_appear_on_the_next_walk() {
        // WB-RT-001: discovery repeats on every slice, so a project directory or
        // session file created while the process runs is picked up.
        let (_folder, root) = root_with(&[("projects/demo/session-a.jsonl", "{}\n")]);
        assert_eq!(found(&root).len(), 1);

        let folder = root.root().to_path_buf();
        fs::create_dir_all(folder.join("projects/late")).unwrap();
        fs::write(folder.join("projects/late/session-b.jsonl"), "{}\n").unwrap();
        assert_eq!(
            found(&root),
            vec![
                "projects/demo/session-a.jsonl".to_string(),
                "projects/late/session-b.jsonl".to_string()
            ]
        );
    }

    #[test]
    fn a_missing_projects_directory_yields_nothing() {
        let folder = tempfile::tempdir().unwrap();
        let root = ReaderRoot::new(folder.path());
        assert!(found(&root).is_empty());
        // A root that does not exist at all is equally quiet.
        assert!(found(&ReaderRoot::new(folder.path().join("missing"))).is_empty());
    }

    #[test]
    fn discovery_is_lazy_and_sliced() {
        let (_folder, root) = root_with(&[
            ("projects/a/one.jsonl", "{}\n"),
            ("projects/b/two.jsonl", "{}\n"),
        ]);
        // Taking one entry must not walk the whole tree.
        let first: Vec<DiscoveredFile> = root.discover().take(1).collect();
        assert_eq!(first.len(), 1);
        // The slice bound is the documented batch size.
        let all: Vec<DiscoveredFile> = root.discover().take(DISCOVERY_SLICE).collect();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn the_sync_interval_is_fixed_and_unrelated_to_refresh_options() {
        // WB-RT-001: five seconds, compile-time, no setting can move it.
        assert_eq!(SYNC_INTERVAL, Duration::from_secs(5));
        assert_eq!(SYNC_INTERVAL.as_secs(), 5);
        // The reader takes no settings argument at all: nothing in this module
        // can read a page refresh choice.
        // The needles are assembled from fragments on purpose: this file is
        // inspected as text, so a literal needle would match itself.
        let source = include_str!("reader.rs");
        let needles = [
            ["stats", "settings"].concat(),
            ["Sett", "ings"].concat(),
            ["refresh", "_interval"].concat(),
            ["interval", "_ms"].concat(),
        ];
        for needle in needles {
            assert!(
                !source.contains(&needle),
                "the reader must not depend on {needle}"
            );
        }
    }

    #[test]
    fn the_allowed_shape_is_exactly_one_project_level() {
        assert!(is_allowed_relative("projects/demo/session.jsonl"));
        assert!(is_allowed_relative("projects/我的 项目/会话 一.jsonl"));
        assert!(!is_allowed_relative("projects/a/b/session.jsonl"));
        assert!(!is_allowed_relative("projects/.jsonl"));
        assert!(!is_allowed_relative("projects/demo/"));
        assert!(!is_allowed_relative("traces/session.jsonl"));
        assert!(!is_allowed_relative("/projects/demo/session.jsonl"));
    }

    #[test]
    fn the_reader_contract_holds() {
        assert!(reader_contract_check().is_ok());
    }
}

#[cfg(test)]
mod fingerprint_tests {
    use super::*;
    use std::fs;
    use std::io::Write;

    fn write(folder: &tempfile::TempDir, body: &str) -> PathBuf {
        let path = folder.path().join("session.jsonl");
        fs::write(&path, body).unwrap();
        path
    }

    fn checkpoint(path: &Path, committed_offset: u64) -> StoredFingerprint {
        let found = fingerprint(path, committed_offset).unwrap();
        StoredFingerprint {
            committed_offset,
            observed_size: fs::metadata(path).unwrap().len(),
            prefix_hash: found.prefix_hash,
            prefix_len: found.prefix_len,
            anchor_hash: found.anchor_hash,
            anchor_start: found.anchor_start,
            anchor_len: found.anchor_len,
        }
    }

    fn append(path: &Path, extra: &str) {
        let mut file = fs::OpenOptions::new().append(true).open(path).unwrap();
        file.write_all(extra.as_bytes()).unwrap();
        file.flush().unwrap();
    }

    #[test]
    fn a_small_file_that_grew_is_an_append_not_a_replacement() {
        // AC026: the whole file is smaller than the prefix window, so a naive
        // "hash the first N bytes" comparison would see a different hash.
        let folder = tempfile::tempdir().unwrap();
        let path = write(&folder, "line-one\n");
        let stored = checkpoint(&path, 9);
        assert_eq!(
            stored.prefix_len, 9,
            "the prefix covers the whole small file"
        );

        append(&path, "line-two\n");
        assert_eq!(
            classify_change(&path, &stored).unwrap(),
            FileChange::Appended
        );
    }

    #[test]
    fn repeated_growth_of_a_small_file_stays_an_append() {
        let folder = tempfile::tempdir().unwrap();
        let path = write(&folder, "a");
        let mut stored = checkpoint(&path, 1);
        for step in ["bb", "ccc", "dddd"] {
            append(&path, step);
            assert_eq!(
                classify_change(&path, &stored).unwrap(),
                FileChange::Appended
            );
            // Re-checkpoint at the new committed offset, as the writer would.
            stored = checkpoint(&path, fs::metadata(&path).unwrap().len() - 1);
        }
    }

    #[test]
    fn an_append_past_the_prefix_window_is_an_append() {
        let folder = tempfile::tempdir().unwrap();
        let head = "h".repeat(PREFIX_LEN as usize + 500);
        let path = write(&folder, &head);
        let stored = checkpoint(&path, head.len() as u64);
        assert_eq!(stored.prefix_len, PREFIX_LEN);

        append(&path, "tail\n");
        assert_eq!(
            classify_change(&path, &stored).unwrap(),
            FileChange::Appended
        );
    }

    #[test]
    fn a_truncated_file_is_reported() {
        let folder = tempfile::tempdir().unwrap();
        let path = write(&folder, "0123456789");
        let stored = checkpoint(&path, 10);

        fs::write(&path, "012").unwrap();
        assert_eq!(
            classify_change(&path, &stored).unwrap(),
            FileChange::Truncated
        );

        // Truncating inside the committed range but still covering the anchor is
        // a truncation too, never an append.
        let folder = tempfile::tempdir().unwrap();
        let path = write(&folder, "0123456789");
        let stored = checkpoint(&path, 4);
        fs::write(&path, "01234567").unwrap();
        assert_eq!(
            classify_change(&path, &stored).unwrap(),
            FileChange::Truncated
        );
    }

    #[test]
    fn a_replaced_head_is_reported_as_replacement() {
        let folder = tempfile::tempdir().unwrap();
        let path = write(&folder, "0123456789");
        let stored = checkpoint(&path, 10);

        // Same length, different bytes: only the hashes can tell.
        fs::write(&path, "9876543210").unwrap();
        assert_eq!(
            classify_change(&path, &stored).unwrap(),
            FileChange::Replaced
        );
    }

    #[test]
    fn a_rewrite_inside_the_committed_region_is_reported_as_replacement() {
        // The head is intact and the size only grew, but the already-committed
        // bytes were rewritten: the anchor catches it.
        let folder = tempfile::tempdir().unwrap();
        let body = "x".repeat(PREFIX_LEN as usize + 100);
        let path = write(&folder, &body);
        let stored = checkpoint(&path, body.len() as u64);

        let mut rewritten = body.clone();
        rewritten.replace_range(600..610, "0123456789");
        fs::write(&path, format!("{rewritten}extra")).unwrap();
        assert_eq!(
            classify_change(&path, &stored).unwrap(),
            FileChange::Replaced
        );
    }

    #[test]
    fn an_untouched_file_is_unchanged() {
        let folder = tempfile::tempdir().unwrap();
        let path = write(&folder, "0123456789");
        let stored = checkpoint(&path, 10);
        assert_eq!(
            classify_change(&path, &stored).unwrap(),
            FileChange::Unchanged
        );
        // Touching the mtime without changing bytes must not fake a change.
        let later = std::time::SystemTime::now() + std::time::Duration::from_secs(60);
        fs::File::open(&path).unwrap().set_modified(later).unwrap();
        assert_eq!(
            classify_change(&path, &stored).unwrap(),
            FileChange::Unchanged
        );
    }

    #[test]
    fn the_anchor_window_follows_the_committed_offset() {
        let folder = tempfile::tempdir().unwrap();
        let path = write(&folder, &"y".repeat(1000));

        let early = fingerprint(&path, 100).unwrap();
        assert_eq!((early.anchor_start, early.anchor_len), (0, 100));
        let later = fingerprint(&path, 1000).unwrap();
        assert_eq!(
            (later.anchor_start, later.anchor_len),
            (1000 - ANCHOR_LEN, ANCHOR_LEN)
        );
        let huge = fingerprint(&path, 10_000).unwrap();
        assert_eq!(
            huge.anchor_len, ANCHOR_LEN,
            "the anchor never exceeds its window"
        );
    }

    #[test]
    fn a_file_shorter_than_the_remembered_regions_is_a_truncation() {
        let folder = tempfile::tempdir().unwrap();
        let path = write(&folder, &"z".repeat(5000));
        let stored = checkpoint(&path, 5000);
        fs::write(&path, "z".repeat(10)).unwrap();
        assert_eq!(
            classify_change(&path, &stored).unwrap(),
            FileChange::Truncated
        );
    }

    #[test]
    fn a_symlink_or_a_missing_file_is_never_read() {
        let folder = tempfile::tempdir().unwrap();
        let real = write(&folder, "0123456789");
        let stored = checkpoint(&real, 10);

        let link = folder.path().join("linked.jsonl");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        assert_eq!(
            classify_change(&link, &stored).unwrap(),
            FileChange::Replaced
        );

        fs::remove_file(&real).unwrap();
        assert!(
            classify_change(&real, &stored).is_err(),
            "a missing file is an error"
        );
    }

    #[test]
    fn the_decision_table_is_the_contract() {
        assert_eq!(decide(5, 5, true, true), FileChange::Unchanged);
        assert_eq!(decide(6, 5, true, true), FileChange::Appended);
        assert_eq!(decide(4, 5, true, true), FileChange::Truncated);
        assert_eq!(decide(5, 5, false, true), FileChange::Replaced);
        assert_eq!(decide(5, 5, true, false), FileChange::Replaced);
        assert_eq!(decide(9, 5, false, false), FileChange::Replaced);
    }

    #[test]
    fn the_fingerprint_contract_holds() {
        assert!(reader_contract_check().is_ok());
    }
}

#[cfg(test)]
mod line_tests {
    use super::*;
    use std::fs;
    use std::io::Write;

    fn file_with(folder: &tempfile::TempDir, body: &[u8]) -> PathBuf {
        let path = folder.path().join("session.jsonl");
        let mut file = fs::File::create(&path).unwrap();
        file.write_all(body).unwrap();
        file.flush().unwrap();
        path
    }

    fn lines(path: &Path) -> Vec<Line> {
        let mut reader = LineReader::open(path, 0).unwrap();
        let mut found = Vec::new();
        while let Some(line) = reader.next_line().unwrap() {
            found.push(line);
        }
        found
    }

    fn texts(path: &Path) -> Vec<String> {
        lines(path)
            .into_iter()
            .map(|line| String::from_utf8(line.bytes).unwrap())
            .collect()
    }

    #[test]
    fn lf_lines_are_read_one_by_one() {
        let folder = tempfile::tempdir().unwrap();
        let path = file_with(&folder, b"one\ntwo\nthree\n");
        assert_eq!(texts(&path), vec!["one", "two", "three"]);
    }

    #[test]
    fn crlf_terminators_are_trimmed() {
        let folder = tempfile::tempdir().unwrap();
        let path = file_with(&folder, b"one\r\ntwo\r\n");
        assert_eq!(texts(&path), vec!["one", "two"]);
        // A bare CR inside a line is data, not a terminator.
        let path = file_with(&folder, b"a\rb\n");
        assert_eq!(texts(&path), vec!["a\rb"]);
    }

    #[test]
    fn a_utf8_bom_is_skipped_only_at_the_very_start() {
        let folder = tempfile::tempdir().unwrap();
        let path = file_with(&folder, b"\xef\xbb\xbf{\"a\":1}\n{\"b\":2}\n");
        assert_eq!(texts(&path), vec!["{\"a\":1}", "{\"b\":2}"]);

        // The same bytes on a later line are content and must survive.
        let path = file_with(&folder, b"first\n\xef\xbb\xbfsecond\n");
        assert_eq!(texts(&path).len(), 2);
        assert!(
            texts(&path)[1].starts_with('\u{feff}'),
            "only offset 0 is a BOM"
        );
    }

    #[test]
    fn an_unterminated_tail_is_never_returned() {
        // DATA-API section 5 / X11: the committed offset must never pass an
        // unterminated line, so the fragment is not a line at all.
        let folder = tempfile::tempdir().unwrap();
        let path = file_with(&folder, b"one\ntwo\npartial-without-newline");
        let found = lines(&path);
        assert_eq!(found.len(), 2);
        assert_eq!(found[1].end, 8, "the offset stops after the last newline");

        // Appending the newline makes the same bytes importable, exactly once.
        let mut file = fs::OpenOptions::new().append(true).open(&path).unwrap();
        file.write_all(b"\n").unwrap();
        file.flush().unwrap();
        let found = lines(&path);
        assert_eq!(found.len(), 3);
        assert_eq!(
            String::from_utf8(found[2].bytes.clone()).unwrap(),
            "partial-without-newline"
        );
        assert_eq!(found[2].end, 32, "the offset now covers the whole file");
    }

    #[test]
    fn the_four_mebibyte_boundary_is_inclusive() {
        let folder = tempfile::tempdir().unwrap();
        let mut body = vec![b'x'; MAX_LINE_BYTES];
        body.push(b'\n');
        let path = file_with(&folder, &body);
        let found = lines(&path);
        assert_eq!(found.len(), 1);
        assert!(!found[0].oversized);
        assert_eq!(found[0].bytes.len(), MAX_LINE_BYTES);

        let mut body = vec![b'x'; MAX_LINE_BYTES + 1];
        body.push(b'\n');
        let path = file_with(&folder, &body);
        let found = lines(&path);
        assert_eq!(found.len(), 1);
        assert!(found[0].oversized, "one byte past the limit is oversized");
        assert!(
            found[0].bytes.is_empty(),
            "an oversized line carries no bytes"
        );
        assert_eq!(
            found[0].end,
            MAX_LINE_BYTES as u64 + 2,
            "but the offset still advances"
        );
    }

    #[test]
    fn an_oversized_line_is_drained_and_the_next_line_is_readable() {
        let folder = tempfile::tempdir().unwrap();
        let mut body = vec![b'x'; MAX_LINE_BYTES + 10];
        body.extend_from_slice(b"\ngood\n");
        let path = file_with(&folder, &body);
        let found = lines(&path);
        assert_eq!(found.len(), 2);
        assert!(found[0].oversized);
        assert_eq!(String::from_utf8(found[1].bytes.clone()).unwrap(), "good");
    }

    #[test]
    fn a_fifty_mebibyte_line_stays_bounded_in_memory() {
        // AC027: a 50 MiB single line must not be buffered.
        let folder = tempfile::tempdir().unwrap();
        let chunk = vec![b'y'; 1024 * 1024];
        let path = folder.path().join("session.jsonl");
        let mut file = fs::File::create(&path).unwrap();
        for _ in 0..50 {
            file.write_all(&chunk).unwrap();
        }
        file.write_all(b"\nafter\n").unwrap();
        file.flush().unwrap();
        drop(file);

        let mut reader = LineReader::open(&path, 0).unwrap();
        let first = reader.next_line().unwrap().unwrap();
        assert!(first.oversized);
        assert_eq!(first.end, 50 * 1024 * 1024 + 1);
        assert!(
            reader.buffered_bytes() <= MAX_LINE_BYTES + READ_CHUNK,
            "buffered {} bytes for a 50 MiB line",
            reader.buffered_bytes()
        );
        let second = reader.next_line().unwrap().unwrap();
        assert_eq!(String::from_utf8(second.bytes).unwrap(), "after");
    }

    #[test]
    fn reading_starts_at_the_given_offset() {
        let folder = tempfile::tempdir().unwrap();
        let path = file_with(&folder, b"one\ntwo\nthree\n");
        let mut reader = LineReader::open(&path, 4).unwrap();
        let line = reader.next_line().unwrap().unwrap();
        assert_eq!(String::from_utf8(line.bytes).unwrap(), "two");
        assert_eq!((line.start, line.end), (4, 8));
    }

    #[test]
    fn line_offsets_cover_the_terminator() {
        let folder = tempfile::tempdir().unwrap();
        let path = file_with(&folder, b"a\nbb\n\n");
        let found = lines(&path);
        assert_eq!(
            found
                .iter()
                .map(|line| (line.start, line.end))
                .collect::<Vec<_>>(),
            vec![(0, 2), (2, 5), (5, 6)]
        );
        assert_eq!(
            found[2].bytes,
            Vec::<u8>::new(),
            "an empty line stays empty"
        );
    }

    #[test]
    fn a_long_file_is_read_line_by_line_without_loading_it() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("session.jsonl");
        let mut file = fs::File::create(&path).unwrap();
        for index in 0..5000 {
            writeln!(file, "{{\"n\":{index}}}").unwrap();
        }
        file.flush().unwrap();
        drop(file);

        let mut reader = LineReader::open(&path, 0).unwrap();
        let mut count = 0;
        let mut peak = 0;
        while let Some(line) = reader.next_line().unwrap() {
            assert!(!line.oversized);
            peak = peak.max(reader.buffered_bytes());
            count += 1;
        }
        assert_eq!(count, 5000);
        assert!(peak <= MAX_LINE_BYTES + READ_CHUNK, "peak {peak}");
    }

    #[test]
    fn the_line_contract_holds() {
        assert!(reader_contract_check().is_ok());
    }
}

#[cfg(test)]
mod advance_tests {
    use super::*;
    use std::fs;
    use std::io::Write;

    const NOW: i64 = 1_792_108_800_000;
    const NAMESPACE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    fn fixture(folder: &tempfile::TempDir) -> PathBuf {
        folder.path().join("session-a.jsonl")
    }

    fn append(path: &Path, body: &str) {
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .unwrap();
        file.write_all(body.as_bytes()).unwrap();
        file.flush().unwrap();
    }

    fn batch(path: &Path, offset: u64) -> Vec<ReadOutcome> {
        read_lines(path, offset, "session-a", NAMESPACE, NOW, 128).unwrap()
    }

    fn record(message_id: &str) -> String {
        format!(
            r#"{{"type":"message","role":"assistant","sessionId":"session-a","timestamp":1,"providerData":{{"messageId":"{message_id}","rawUsage":{{"prompt_tokens":5}}}}}}"#
        )
    }

    #[test]
    fn a_record_written_in_three_writes_is_imported_exactly_once() {
        // AC028 / X11: the offset must never pass an unterminated line, so the
        // partial writes are invisible and the finished line lands once.
        let folder = tempfile::tempdir().unwrap();
        let path = fixture(&folder);
        let body = record("b1");
        let mut offset = 0u64;
        let mut imported = 0;

        for part in [&body[..10], &body[10..25], &format!("{}\n", &body[25..])] {
            append(&path, part);
            let outcomes = batch(&path, offset);
            imported += outcomes
                .iter()
                .filter(|o| o.disposition.as_ref().is_some_and(|d| d.is_candidate()))
                .count();
            if let Some(last) = outcomes.last() {
                offset = last.commit_to;
            }
        }
        assert_eq!(imported, 1, "the record must be imported exactly once");
        assert_eq!(
            offset,
            (body.len() + 1) as u64,
            "the offset ends at the newline"
        );
    }

    #[test]
    fn an_unterminated_tail_never_advances_the_offset() {
        let folder = tempfile::tempdir().unwrap();
        let path = fixture(&folder);
        append(&path, &format!("{}\n", record("b1")));
        append(&path, &record("b2"));

        let outcomes = batch(&path, 0);
        assert_eq!(outcomes.len(), 1, "only the terminated line is visible");
        assert!(outcomes[0].commit_to < fs::metadata(&path).unwrap().len());
    }

    #[test]
    fn a_bad_complete_line_is_counted_once_and_still_advances() {
        // AC028: a malformed line must not block the file; the good line after
        // it stays visible.
        let folder = tempfile::tempdir().unwrap();
        let path = fixture(&folder);
        append(&path, "{not json\n");
        append(&path, &format!("{}\n", record("b1")));

        let outcomes = batch(&path, 0);
        assert_eq!(outcomes.len(), 2);
        assert_eq!(
            outcomes[0].disposition,
            Some(ParseDisposition::JsonSyntaxError)
        );
        assert_eq!(
            outcomes[0].diagnostic(),
            Some(DiagnosticKind::JsonSyntaxError)
        );
        assert!(outcomes[1].disposition.as_ref().unwrap().is_candidate());
        assert_eq!(outcomes[1].diagnostic(), None);
        assert_eq!(outcomes[1].commit_to, fs::metadata(&path).unwrap().len());
    }

    #[test]
    fn an_oversized_line_is_counted_once_and_survives_a_restart() {
        // The oversized line is a complete line, so its offset is committed and
        // the diagnostic cannot repeat on the next slice or after a restart.
        let folder = tempfile::tempdir().unwrap();
        let path = fixture(&folder);
        let mut body = vec![b'x'; MAX_LINE_BYTES + 32];
        body.push(b'\n');
        fs::write(&path, &body).unwrap();
        append(&path, &format!("{}\n", record("b1")));

        let first = batch(&path, 0);
        assert_eq!(first.len(), 2);
        assert!(first[0].oversized);
        assert_eq!(first[0].diagnostic(), Some(DiagnosticKind::OversizedLine));
        let committed = first[1].commit_to;

        // A restart resumes after the drained line and counts nothing again.
        let second = batch(&path, committed);
        assert!(
            second.is_empty(),
            "nothing is left after the committed offset"
        );
        assert_eq!(fs::metadata(&path).unwrap().len(), committed);
    }

    #[test]
    fn an_oversized_line_without_a_newline_yet_is_not_committed() {
        // A giant line that is still growing is an unterminated tail: it must
        // not be drained-and-committed before its newline arrives.
        let folder = tempfile::tempdir().unwrap();
        let path = fixture(&folder);
        let mut body = vec![b'x'; MAX_LINE_BYTES + 32];
        fs::write(&path, &body).unwrap();
        assert!(batch(&path, 0).is_empty());

        body.push(b'\n');
        fs::write(&path, &body).unwrap();
        let outcomes = batch(&path, 0);
        assert_eq!(outcomes.len(), 1);
        assert!(outcomes[0].oversized);
    }

    #[test]
    fn the_batch_limit_bounds_one_slice() {
        let folder = tempfile::tempdir().unwrap();
        let path = fixture(&folder);
        for index in 0..10 {
            append(&path, &format!("{}\n", record(&format!("b{index}"))));
        }
        let outcomes = read_lines(&path, 0, "session-a", NAMESPACE, NOW, 4).unwrap();
        assert_eq!(outcomes.len(), 4);
        // Resuming from the last committed offset continues where it stopped.
        let next = read_lines(
            &path,
            outcomes.last().unwrap().commit_to,
            "session-a",
            NAMESPACE,
            NOW,
            4,
        )
        .unwrap();
        assert_eq!(next.len(), 4);
        assert_ne!(outcomes[0].commit_to, next[0].commit_to);
    }

    #[test]
    fn bad_lines_do_not_hide_the_lines_after_them() {
        let folder = tempfile::tempdir().unwrap();
        let path = fixture(&folder);
        append(&path, "{}\n");
        append(&path, "[1,2]\n");
        append(&path, &format!("{}\n", record("b1")));
        append(&path, "{}\n");

        let outcomes = batch(&path, 0);
        assert_eq!(outcomes.len(), 4);
        let candidates = outcomes
            .iter()
            .filter(|outcome| {
                outcome
                    .disposition
                    .as_ref()
                    .is_some_and(|d| d.is_candidate())
            })
            .count();
        assert_eq!(candidates, 1, "the one usage record is still found");
        assert_eq!(
            outcomes
                .iter()
                .filter(|outcome| outcome.disposition == Some(ParseDisposition::JsonSyntaxError))
                .count(),
            1
        );
    }

    #[test]
    fn every_outcome_commits_to_the_end_of_a_complete_line() {
        let folder = tempfile::tempdir().unwrap();
        let path = fixture(&folder);
        append(&path, "one\ntwo\n");
        let outcomes = batch(&path, 0);
        assert_eq!(
            outcomes.iter().map(|o| o.commit_to).collect::<Vec<_>>(),
            vec![4, 8]
        );
        for outcome in &outcomes {
            assert!(outcome.commit_to <= fs::metadata(&path).unwrap().len());
        }
    }

    #[test]
    fn the_advance_contract_holds() {
        assert!(reader_contract_check().is_ok());
    }
}

#[cfg(test)]
mod fairness_tests {
    use super::*;

    fn files(count: usize) -> Vec<DiscoveredFile> {
        (0..count)
            .map(|index| DiscoveredFile {
                relative_path: format!("projects/demo/session-{index}.jsonl"),
                project: "demo".into(),
                session_id: format!("session-{index}"),
                size_bytes: 0,
            })
            .collect()
    }

    fn names(plan: &[&DiscoveredFile]) -> Vec<String> {
        plan.iter().map(|file| file.relative_path.clone()).collect()
    }

    fn turn(bytes: u64, elapsed_ms: u64, has_backlog: bool) -> TurnResult {
        TurnResult {
            bytes,
            elapsed: Duration::from_millis(elapsed_ms),
            has_backlog,
        }
    }

    #[test]
    fn a_growing_large_file_cannot_starve_the_others() {
        // AC029: one file is permanently behind (it always has more lines);
        // every slice must still include the other two.
        let all = files(3);
        let mut scheduler = FairScheduler::new();
        scheduler.record(&all[0].relative_path, &turn(PER_FILE_BYTES, 0, true));

        for slice in 0..10 {
            let plan = scheduler.plan(&all);
            let planned = names(&plan);
            assert_eq!(planned.len(), 3, "slice {slice}: {planned:?}");
            assert!(planned.contains(&all[1].relative_path));
            assert!(planned.contains(&all[2].relative_path));
            // The big file yields as soon as its budget is spent.
            assert!(turn(PER_FILE_BYTES, 0, true).exhausted());
            scheduler.record(&all[0].relative_path, &turn(PER_FILE_BYTES, 0, true));
        }
        assert_eq!(scheduler.backlog_len(), 1, "only the big file stays behind");
    }

    #[test]
    fn caught_up_increments_are_served_before_the_backlog() {
        // AC029: while another file is being backfilled, a caught-up file's
        // increment must not wait behind it.
        let all = files(3);
        let mut scheduler = FairScheduler::new();
        scheduler.record(&all[2].relative_path, &turn(0, 0, true));

        let plan = scheduler.plan(&all);
        let planned = names(&plan);
        assert_eq!(
            planned[..2].to_vec(),
            vec![all[0].relative_path.clone(), all[1].relative_path.clone()],
            "caught-up files come first: {planned:?}"
        );
        assert_eq!(
            planned[2], all[2].relative_path,
            "the backlog is served last"
        );
    }

    #[test]
    fn the_backlog_rotation_moves_every_slice() {
        let all = files(3);
        let mut scheduler = FairScheduler::new();
        for file in &all {
            scheduler.record(&file.relative_path, &turn(0, 0, true));
        }
        let first = names(&scheduler.plan(&all));
        let second = names(&scheduler.plan(&all));
        let third = names(&scheduler.plan(&all));
        assert_ne!(first, second, "the rotation must move");
        assert_ne!(second, third);
        assert_ne!(first, third, "three files give three distinct rotations");
        let fourth = names(&scheduler.plan(&all));
        assert_eq!(first, fourth, "and come back around after a full cycle");
    }

    #[test]
    fn the_per_file_budget_yields_on_either_limit() {
        assert!(turn(PER_FILE_BYTES, 0, false).exhausted(), "bytes limit");
        assert!(turn(0, 50, false).exhausted(), "time limit");
        assert!(!turn(PER_FILE_BYTES - 1, 49, false).exhausted());
        assert!(
            !turn(0, 0, true).exhausted(),
            "having backlog is not exhaustion"
        );
    }

    #[test]
    fn the_active_window_is_bounded() {
        let all = files(ACTIVE_WINDOW + 5);
        let mut scheduler = FairScheduler::new();
        let plan = scheduler.plan(&all);
        assert_eq!(
            plan.len(),
            ACTIVE_WINDOW,
            "one slice never exceeds the window"
        );
        // The files outside the window are not lost: they appear once the
        // window's files are caught up and the slice is planned again.
        assert_eq!(scheduler.plan(&all).len(), ACTIVE_WINDOW);
    }

    #[test]
    fn a_file_that_catches_up_leaves_the_backlog() {
        let all = files(2);
        let mut scheduler = FairScheduler::new();
        scheduler.record(&all[0].relative_path, &turn(0, 0, true));
        assert_eq!(scheduler.backlog_len(), 1);
        scheduler.record(&all[0].relative_path, &turn(0, 0, false));
        assert_eq!(
            scheduler.backlog_len(),
            0,
            "a caught-up file is no longer behind"
        );
        // Recording the same state twice does not duplicate entries.
        scheduler.record(&all[1].relative_path, &turn(0, 0, true));
        scheduler.record(&all[1].relative_path, &turn(0, 0, true));
        assert_eq!(scheduler.backlog_len(), 1);
    }

    #[test]
    fn a_new_file_joins_the_next_slice() {
        let mut scheduler = FairScheduler::new();
        let before = files(1);
        assert_eq!(scheduler.plan(&before).len(), 1);
        let after = files(3);
        let planned = names(&scheduler.plan(&after));
        assert_eq!(planned.len(), 3, "discovery feeds the plan every slice");
    }

    #[test]
    fn an_empty_discovery_plans_nothing_and_keeps_the_rotation_sane() {
        let mut scheduler = FairScheduler::new();
        assert!(scheduler.plan(&[]).is_empty());
        let all = files(2);
        scheduler.record(&all[0].relative_path, &turn(0, 0, true));
        assert_eq!(scheduler.plan(&all).len(), 2);
    }

    #[test]
    fn the_cadence_stays_five_seconds_and_independent_of_settings() {
        // WB-RT-001: the fairness window lives inside the same fixed cadence.
        assert_eq!(SYNC_INTERVAL, Duration::from_secs(5));
        assert_eq!(PER_FILE_BYTES, 8 * 1024 * 1024);
        assert_eq!(PER_FILE_TIME, Duration::from_millis(50));
        assert_eq!(ACTIVE_WINDOW, 32);
    }

    #[test]
    fn the_fairness_contract_holds() {
        assert!(reader_contract_check().is_ok());
    }
}

#[cfg(test)]
mod verify_tests {
    use super::*;
    use std::fs;
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;

    const NOW: i64 = 1_792_108_800_000;
    const NAMESPACE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    fn record(message_id: &str) -> String {
        format!(
            r#"{{"type":"message","role":"assistant","sessionId":"session-a","timestamp":1,"providerData":{{"messageId":"{message_id}","rawUsage":{{"prompt_tokens":5}}}}}}"#
        )
    }

    fn setup(folder: &tempfile::TempDir, body: &str) -> (PathBuf, StoredFingerprint) {
        let path = folder.path().join("session-a.jsonl");
        fs::write(&path, body).unwrap();
        let found = fingerprint(&path, 0).unwrap();
        let stored = StoredFingerprint {
            committed_offset: 0,
            observed_size: fs::metadata(&path).unwrap().len(),
            prefix_hash: found.prefix_hash,
            prefix_len: found.prefix_len,
            anchor_hash: found.anchor_hash,
            anchor_start: found.anchor_start,
            anchor_len: found.anchor_len,
        };
        (path, stored)
    }

    fn batch(path: &Path, stored: &StoredFingerprint) -> Result<ReadBatch, BatchRejection> {
        read_verified_batch(path, stored, "session-a", NAMESPACE, NOW, 128)
    }

    #[test]
    fn a_batch_from_a_stable_file_is_accepted() {
        let folder = tempfile::tempdir().unwrap();
        let (path, stored) = setup(&folder, &format!("{}\n{}\n", record("b1"), record("b2")));
        let found = batch(&path, &stored).unwrap();
        assert_eq!(found.imported(), 2);
        assert_eq!(found.base_offset, 0);
        assert_eq!(found.end_offset, fs::metadata(&path).unwrap().len());
        assert_eq!(found.diagnostics(), 0);
    }

    #[test]
    fn a_rename_that_replaces_the_file_discards_the_batch() {
        // AC030: the bytes now belong to another file; nothing may be committed.
        let folder = tempfile::tempdir().unwrap();
        let (path, stored) = setup(&folder, &format!("{}\n", record("b1")));
        let moved = folder.path().join("moved.jsonl");
        fs::rename(&path, &moved).unwrap();
        fs::write(&path, format!("{}\n", record("other"))).unwrap();

        assert_eq!(batch(&path, &stored), Err(BatchRejection::SourceChanged));
        // The original bytes are still intact under the new name: nothing lost.
        assert_eq!(
            fs::read_to_string(&moved).unwrap(),
            format!("{}\n", record("b1"))
        );
    }

    #[test]
    fn a_truncation_discards_the_batch() {
        let folder = tempfile::tempdir().unwrap();
        let (path, stored) = setup(&folder, &format!("{}\n{}\n", record("b1"), record("b2")));
        fs::write(&path, "").unwrap();
        assert_eq!(batch(&path, &stored), Err(BatchRejection::SourceChanged));
    }

    #[test]
    fn a_change_during_the_read_discards_the_batch() {
        // The post-read check catches a replacement that happened after the
        // pre-read check passed.
        let folder = tempfile::tempdir().unwrap();
        let (path, stored) = setup(&folder, &format!("{}\n", record("b1")));
        assert!(verify_batch(&path, &stored).is_ok());
        // The file is rewritten between the two checks.
        fs::write(&path, format!("{}\n", record("b9"))).unwrap();
        assert_eq!(
            verify_batch(&path, &stored),
            Err(BatchRejection::SourceChanged)
        );
    }

    #[test]
    fn a_permission_failure_does_not_advance_the_offset() {
        // AC030: a permission error must leave the offset alone so the lines are
        // read later instead of being skipped.
        let folder = tempfile::tempdir().unwrap();
        let (path, stored) = setup(&folder, &format!("{}\n", record("b1")));
        let original = fs::metadata(&path).unwrap().permissions();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap();

        let outcome = batch(&path, &stored);
        // Restore before asserting so a failure cannot leave the directory in a
        // state the tempdir cleanup would choke on.
        fs::set_permissions(&path, original).unwrap();
        assert_eq!(outcome, Err(BatchRejection::Unreadable));

        // Once readable again the same line is still there, exactly once.
        let found = batch(&path, &stored).unwrap();
        assert_eq!(found.imported(), 1);
        assert_eq!(found.base_offset, 0, "the offset never moved");
    }

    #[test]
    fn a_missing_file_or_a_symlink_is_unreadable() {
        let folder = tempfile::tempdir().unwrap();
        let (path, stored) = setup(&folder, &format!("{}\n", record("b1")));
        let link = folder.path().join("linked.jsonl");
        std::os::unix::fs::symlink(&path, &link).unwrap();
        assert_eq!(batch(&link, &stored), Err(BatchRejection::SourceChanged));

        fs::remove_file(&path).unwrap();
        assert_eq!(batch(&path, &stored), Err(BatchRejection::Unreadable));
    }

    #[test]
    fn an_unterminated_tail_yields_an_empty_batch_without_moving_the_offset() {
        let folder = tempfile::tempdir().unwrap();
        let (path, stored) = setup(&folder, &record("b1"));
        let found = batch(&path, &stored).unwrap();
        assert!(found.is_empty());
        assert_eq!(
            found.end_offset, found.base_offset,
            "nothing may be committed yet"
        );

        let mut file = fs::OpenOptions::new().append(true).open(&path).unwrap();
        file.write_all(b"\n").unwrap();
        file.flush().unwrap();
        let found = batch(&path, &stored).unwrap();
        assert_eq!(
            found.imported(),
            1,
            "the same bytes land once the newline arrives"
        );
    }

    #[test]
    fn a_batch_reports_its_candidates_and_diagnostics_separately() {
        let folder = tempfile::tempdir().unwrap();
        let (path, stored) = setup(&folder, &format!("{{not json\n{}\n{{}}\n", record("b1")));
        let found = batch(&path, &stored).unwrap();
        assert_eq!(found.outcomes.len(), 3);
        assert_eq!(found.imported(), 1);
        assert_eq!(
            found.diagnostics(),
            2,
            "one syntax error and one ignored line"
        );
    }

    #[test]
    fn the_verification_contract_holds() {
        assert!(reader_contract_check().is_ok());
    }
}

#[cfg(test)]
mod rescan_tests {
    use super::*;
    use std::fs;

    const NOW: i64 = 1_792_108_800_000;
    const NAMESPACE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    fn record(message_id: &str, tokens: i64) -> String {
        format!(
            r#"{{"type":"message","role":"assistant","sessionId":"session-a","timestamp":1,"providerData":{{"messageId":"{message_id}","rawUsage":{{"prompt_tokens":{tokens}}}}}}}"#
        )
    }

    /// A file with one record that sits **between** the prefix window and the
    /// anchor window, which is exactly where an incremental check is blind.
    ///
    /// Returns the path, the index of that record, and its byte offset.
    fn long_file(folder: &tempfile::TempDir) -> (PathBuf, usize, usize) {
        let path = folder.path().join("session-a.jsonl");
        let mut body = String::new();
        let mut offsets = Vec::new();
        for index in 0..40 {
            offsets.push(body.len());
            body.push_str(&record(&format!("b{index:02}"), 10));
            body.push('\n');
        }
        fs::write(&path, &body).unwrap();
        let size = body.len();
        let chosen = (0..40)
            .find(|index| {
                let start = offsets[*index];
                let len = record(&format!("b{index:02}"), 10).len();
                start > PREFIX_LEN as usize && start + len + ANCHOR_LEN as usize <= size
            })
            .expect("a record must sit between the prefix and the anchor windows");
        (path, chosen, offsets[chosen])
    }

    fn checkpoint(path: &Path) -> StoredFingerprint {
        let size = fs::metadata(path).unwrap().len();
        let found = fingerprint(path, size).unwrap();
        StoredFingerprint {
            committed_offset: size,
            observed_size: size,
            prefix_hash: found.prefix_hash,
            prefix_len: found.prefix_len,
            anchor_hash: found.anchor_hash,
            anchor_start: found.anchor_start,
            anchor_len: found.anchor_len,
        }
    }

    #[test]
    fn a_middle_edit_is_invisible_to_incremental_but_found_by_a_full_rescan() {
        // AC031: same size, untouched head and tail, so the incremental
        // fingerprint says "unchanged" — only a full read sees the edit.
        let folder = tempfile::tempdir().unwrap();
        let (path, chosen, offset) = long_file(&folder);
        let stored = checkpoint(&path);

        let original = fs::read_to_string(&path).unwrap();
        let mut edited = original.clone();
        let replacement = record(&format!("b{chosen:02}"), 99);
        assert_eq!(
            replacement.len(),
            record(&format!("b{chosen:02}"), 10).len()
        );
        edited.replace_range(offset..offset + replacement.len(), &replacement);
        assert_eq!(
            edited.len(),
            original.len(),
            "the edit must not change the size"
        );
        fs::write(&path, &edited).unwrap();

        // Incremental: prefix, anchor and size all match, so nothing is noticed.
        assert_eq!(
            classify_change(&path, &stored).unwrap(),
            FileChange::Unchanged,
            "the incremental check is blind to a middle edit by design"
        );

        // Full rescan: the same bytes are read from zero and the edit is visible.
        let found = read_full_slice(&path, 0, "session-a", NAMESPACE, NOW, 128).unwrap();
        assert_eq!(found.outcomes.len(), 40);
        let edited_line = &found.outcomes[chosen];
        assert!(
            edited_line
                .disposition
                .as_ref()
                .is_some_and(|d| d.is_candidate()),
            "the edited record is read again"
        );
        assert_eq!(found.base_offset, 0, "a rescan starts at the beginning");
    }

    #[test]
    fn a_full_rescan_never_moves_the_incremental_offset() {
        // AC031: the rescan has its own cursor; the checkpoint is read-only here.
        let folder = tempfile::tempdir().unwrap();
        let (path, _, _) = long_file(&folder);
        let stored = checkpoint(&path);
        let before = stored.committed_offset;

        let mut cursor = ScanCursor::new();
        assert!(cursor.begin());
        let found =
            read_full_slice(&path, cursor.offset(), "session-a", NAMESPACE, NOW, 8).unwrap();
        cursor.advance(found.end_offset);
        cursor.finish(NOW);

        assert_eq!(
            stored.committed_offset, before,
            "the incremental offset is untouched"
        );
        assert_eq!(
            cursor.offset(),
            0,
            "and the rescan cursor reset for the next pass"
        );
        assert_eq!(
            classify_change(&path, &stored).unwrap(),
            FileChange::Unchanged
        );
    }

    #[test]
    fn a_rescan_is_sliced_by_its_own_offset() {
        let folder = tempfile::tempdir().unwrap();
        let (path, _, _) = long_file(&folder);
        let mut cursor = ScanCursor::new();
        assert!(cursor.begin());

        let first =
            read_full_slice(&path, cursor.offset(), "session-a", NAMESPACE, NOW, 10).unwrap();
        assert_eq!(first.outcomes.len(), 10);
        cursor.advance(first.end_offset);
        let second =
            read_full_slice(&path, cursor.offset(), "session-a", NAMESPACE, NOW, 10).unwrap();
        assert_eq!(second.outcomes.len(), 10);
        assert_eq!(
            second.base_offset, first.end_offset,
            "the slices do not overlap"
        );
    }

    #[test]
    fn the_daily_trigger_comes_due_on_the_interval() {
        let mut cursor = ScanCursor::new();
        assert!(
            RescanTrigger::Daily.is_due(&cursor, NOW),
            "never scanned is due at once"
        );
        assert!(cursor.begin());
        assert!(
            !RescanTrigger::Daily.is_due(&cursor, NOW),
            "no overlap while running"
        );
        assert!(
            !RescanTrigger::Manual.is_due(&cursor, NOW),
            "manual cannot overlap either"
        );
        cursor.finish(NOW);

        assert!(!cursor.is_due(NOW));
        assert!(!cursor.is_due(NOW + FULL_RESCAN_INTERVAL_MS - 1));
        assert!(cursor.is_due(NOW + FULL_RESCAN_INTERVAL_MS));
        assert_eq!(cursor.last_full_ms(), Some(NOW));
    }

    #[test]
    fn the_manual_trigger_is_due_whenever_no_pass_is_running() {
        let mut cursor = ScanCursor::new();
        assert!(RescanTrigger::Manual.is_due(&cursor, NOW));
        assert!(cursor.begin());
        cursor.finish(NOW);
        assert!(
            RescanTrigger::Manual.is_due(&cursor, NOW),
            "a manual check does not wait for the daily interval"
        );
    }

    #[test]
    fn repeated_passes_rotate_the_starting_file() {
        let mut cursor = ScanCursor::new();
        assert_eq!(cursor.rotation(), 0);
        assert!(cursor.begin());
        cursor.finish(NOW);
        assert_eq!(cursor.rotation(), 1);
        assert!(cursor.begin());
        cursor.finish(NOW);
        assert_eq!(
            cursor.rotation(),
            2,
            "successive passes do not always start at the first file"
        );
    }

    #[test]
    fn the_rescan_contract_holds() {
        assert!(reader_contract_check().is_ok());
    }
}
