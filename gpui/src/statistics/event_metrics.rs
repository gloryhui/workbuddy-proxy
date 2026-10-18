//! Optional QA measurements. Event handlers only append to a bounded memory
//! buffer; the trace is emitted after the statistics view is dropped on exit.
use std::{collections::VecDeque, time::Instant};

const CAPACITY: usize = 128;

#[derive(serde::Serialize)]
struct Event {
    kind: &'static str,
    elapsed_us: u64,
    selection_matches: Option<bool>,
}

pub(super) struct EventMetrics {
    enabled: bool,
    dropped: usize,
    events: VecDeque<Event>,
}

impl EventMetrics {
    pub(super) fn new() -> Self {
        let enabled = std::env::var("WORKBUDDY_PROXY_UI_EVENT_METRICS").as_deref() == Ok("1");
        Self {
            enabled,
            dropped: 0,
            events: VecDeque::with_capacity(if enabled { CAPACITY } else { 0 }),
        }
    }

    pub(super) fn begin(&self) -> Option<Instant> {
        self.enabled.then(Instant::now)
    }

    pub(super) fn record(
        &mut self,
        kind: &'static str,
        started: Option<Instant>,
        selection_matches: Option<bool>,
    ) {
        let Some(started) = started else { return };
        let elapsed_us = started.elapsed().as_micros().min(u64::MAX as u128) as u64;
        if self.events.len() == CAPACITY {
            self.dropped += 1;
            return;
        }
        self.events.push_back(Event {
            kind,
            elapsed_us,
            selection_matches,
        });
    }
}

impl Drop for EventMetrics {
    fn drop(&mut self) {
        if self.enabled {
            eprintln!(
                "WB_UI_EVENT_METRICS {}",
                serde_json::json!({
                    "pid": std::process::id(), "dropped": self.dropped, "events": self.events
                })
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_disabled_recorder_does_no_work_and_enabled_samples_are_bounded() {
        let mut recorder = EventMetrics {
            enabled: false,
            dropped: 0,
            events: VecDeque::new(),
        };
        recorder.record("disabled", recorder.begin(), None);
        assert!(recorder.events.is_empty());
        recorder.enabled = true;
        for _ in 0..CAPACITY + 3 {
            recorder.record("synthetic", recorder.begin(), Some(true));
        }
        assert_eq!(recorder.events.len(), CAPACITY);
        assert_eq!(recorder.dropped, 3);
    }
}
