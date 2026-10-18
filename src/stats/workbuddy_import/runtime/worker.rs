//! Source I/O and scheduling on the dedicated WorkBuddy import thread.
use super::*;

fn file_key(namespace: &str, relative: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hash = Sha256::new();
    for part in [
        b"wbp-wb-file-v1".as_slice(),
        namespace.as_bytes(),
        relative.as_bytes(),
    ] {
        hash.update((part.len() as u32).to_be_bytes());
        hash.update(part);
    }
    format!("{:x}", hash.finalize())
}

pub(super) fn submit(
    inner: &Inner,
    sender: &SyncSender<Envelope>,
    shared: &Shared,
    work: Work,
) -> Result<(), &'static str> {
    if !shared.active() {
        return Err("stats_workbuddy_import_disabled");
    }
    let (reply, ack) = mpsc::sync_channel(1);
    telemetry::offer(shared, sender, Envelope { work, reply })?;
    inner.queue.wake();
    loop {
        match ack.recv_timeout(Duration::from_millis(100)) {
            Ok(result) => return result,
            Err(mpsc::RecvTimeoutError::Disconnected) => return Err("stats_unavailable"),
            Err(mpsc::RecvTimeoutError::Timeout) if !shared.active() => {
                return Err("stats_workbuddy_import_disabled");
            }
            Err(mpsc::RecvTimeoutError::Timeout) => inner.queue.wake(),
        }
    }
}

pub(super) fn run_worker(
    inner: &Inner,
    root: &reader::ReaderRoot,
    sender: &SyncSender<Envelope>,
    shared: &Shared,
) -> Result<(), &'static str> {
    scheduler::Worker::new(inner, root, sender, shared)?.run()
}

mod discovery;
mod priority;
mod scheduler;

fn safe_source(root: &Path, relative: &str) -> Result<std::path::PathBuf, &'static str> {
    if !reader::is_allowed_relative(relative) {
        return Err("stats_workbuddy_invalid_path");
    }
    let mut component = root.to_path_buf();
    for part in relative.split('/') {
        component.push(part);
        let metadata = std::fs::symlink_metadata(&component)
            .map_err(|_| "stats_workbuddy_source_read_failed")?;
        if metadata.file_type().is_symlink() {
            return Err("stats_workbuddy_invalid_path");
        }
    }
    let path = root.join(relative);
    let metadata =
        std::fs::symlink_metadata(&path).map_err(|_| "stats_workbuddy_source_read_failed")?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err("stats_workbuddy_invalid_path");
    }
    let canonical_root = root
        .canonicalize()
        .map_err(|_| "stats_workbuddy_root_missing")?;
    let canonical = path
        .canonicalize()
        .map_err(|_| "stats_workbuddy_source_read_failed")?;
    if !canonical.starts_with(canonical_root) {
        return Err("stats_workbuddy_invalid_path");
    }
    Ok(path)
}

fn modified_ms(metadata: &std::fs::Metadata) -> Result<i64, &'static str> {
    let time = metadata
        .modified()
        .map_err(|_| "stats_workbuddy_source_read_failed")?
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| "stats_workbuddy_source_read_failed")?;
    i64::try_from(time.as_millis()).map_err(|_| "stats_workbuddy_unsupported_format")
}

mod file;
use file::read_file;
