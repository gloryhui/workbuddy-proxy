//! Bounded, disposable scheduling hints; durable unfinished checkpoints remain
//! the authority after restart or eviction. No candidates or file bodies here.
use super::*;
use std::collections::VecDeque;

const MAX_PRIORITY_FILES: usize = 1024;

#[derive(Clone)]
pub(super) struct Pending {
    pub(super) relative: String,
    items_left: usize,
    bytes_left: u64,
}

impl Pending {
    pub(super) fn next(&self, work: &Work) -> Option<Self> {
        let Work::Batch(batch) = work else {
            return None;
        };
        if batch.at_eof {
            return None;
        }
        let before = if batch.reset {
            0
        } else {
            batch
                .expected
                .as_ref()
                .map_or(0, |old| old.committed_offset)
        };
        let consumed = batch.offset.saturating_sub(before) as u64;
        let next = Self {
            relative: self.relative.clone(),
            items_left: self.items_left.saturating_sub(batch.candidates.len()),
            bytes_left: self.bytes_left.saturating_sub(consumed),
        };
        (next.items_left > 0 && next.bytes_left > 0).then_some(next)
    }
}

#[derive(Default)]
pub(super) struct Queue(VecDeque<Pending>);

impl Queue {
    pub(super) fn enqueue(&mut self, relative: String) {
        self.0.retain(|old| old.relative != relative);
        self.retry(Pending {
            relative,
            items_left: 500,
            bytes_left: SLICE_BYTES,
        });
    }

    pub(super) fn retry(&mut self, pending: Pending) {
        self.0.push_front(pending);
        if self.0.len() > MAX_PRIORITY_FILES {
            self.0.pop_back();
        }
    }

    pub(super) fn pop(&mut self) -> Option<Pending> {
        self.0.pop_front()
    }
}
