//! Process-local monotonic commit receipts, without source paths or content.
use super::*;

const MAX_COMMIT_RECEIPTS: usize = 32;

pub(super) struct Clock {
    origin: Instant,
    id: String,
}

#[derive(Clone)]
pub(super) struct SourceCommit {
    file_key: String,
    offset: i64,
    full: bool,
    elapsed_ns: u64,
}

fn clock(shared: &Shared) -> &Clock {
    shared.clock.get_or_init(|| Clock {
        origin: Instant::now(),
        id: crate::config::token(),
    })
}

fn elapsed(clock: &Clock) -> u64 {
    clock.origin.elapsed().as_nanos().min(u64::MAX as u128) as u64
}

pub(super) fn pending(work: &Work) -> Option<SourceCommit> {
    match work {
        Work::Batch(batch) => Some(SourceCommit {
            file_key: batch.file_key.clone(),
            offset: batch.offset,
            full: batch.scan_offset.is_some(),
            elapsed_ns: 0,
        }),
        _ => None,
    }
}

/// Called only after SQLite commit succeeded, before the writer sends its ACK.
pub(super) fn committed(shared: &Shared, mut receipt: SourceCommit) {
    receipt.elapsed_ns = elapsed(clock(shared));
    let mut progress = shared.progress.lock().unwrap();
    progress
        .recent_source_commits
        .retain(|old| old.file_key != receipt.file_key);
    progress.recent_source_commits.push_back(receipt.clone());
    if progress.recent_source_commits.len() > MAX_COMMIT_RECEIPTS {
        progress.recent_source_commits.pop_front();
    }
    progress.last_source_commit = Some(receipt);
}

fn serialize(receipt: &SourceCommit) -> serde_json::Value {
    serde_json::json!({"file_key":receipt.file_key,"offset":receipt.offset,
        "full":receipt.full,"commit_ack_elapsed_ns":receipt.elapsed_ns.to_string()})
}

pub(super) fn publish(shared: &Shared, progress: &Progress, value: &mut serde_json::Value) {
    let clock = clock(shared);
    value["monotonic_clock_id"] = clock.id.clone().into();
    value["monotonic_elapsed_ns"] = elapsed(clock).to_string().into();
    value["last_source_commit"] = progress
        .last_source_commit
        .as_ref()
        .map(serialize)
        .unwrap_or(serde_json::Value::Null);
    value["source_commit_receipts"] = progress
        .recent_source_commits
        .iter()
        .map(serialize)
        .collect::<Vec<_>>()
        .into();
}

#[cfg(test)]
mod tests {
    use super::{MAX_COMMIT_RECEIPTS, Shared, SourceCommit, committed};

    #[test]
    fn receipt_history_is_bounded_and_updates_one_slot_per_file() {
        let shared = Shared::default();
        for number in 0..40 {
            committed(
                &shared,
                SourceCommit {
                    file_key: format!("{number:064x}"),
                    offset: 128,
                    full: false,
                    elapsed_ns: 0,
                },
            );
        }
        let key = format!("{:064x}", 39);
        committed(
            &shared,
            SourceCommit {
                file_key: key.clone(),
                offset: 256,
                full: true,
                elapsed_ns: 0,
            },
        );
        let progress = shared.progress.lock().unwrap();
        assert_eq!(progress.recent_source_commits.len(), MAX_COMMIT_RECEIPTS);
        assert_eq!(
            progress.recent_source_commits.front().unwrap().file_key,
            format!("{:064x}", 8)
        );
        assert_eq!(
            progress
                .recent_source_commits
                .iter()
                .filter(|row| row.file_key == key)
                .count(),
            1
        );
        let latest = progress.recent_source_commits.back().unwrap();
        assert_eq!(latest.offset, 256);
        assert!(latest.full);
    }
}
