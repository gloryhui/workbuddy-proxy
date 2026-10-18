//! Bounded process-local measurements from actual reader/channel/writer calls.
use super::*;

const RECENT_BATCHES: usize = 32;

#[derive(Default, serde::Serialize)]
pub(super) struct Telemetry {
    queue_depth: u64,
    queue_peak: u64,
    writer_batch_count: u64,
    writer_batch_failures: u64,
    writer_batch_total_us: u64,
    writer_batch_max_us: u64,
    source_io_thread_checks: u64,
    source_io_thread_violations: u64,
    recent_batches: std::collections::VecDeque<BatchSample>,
}

#[derive(serde::Serialize)]
struct BatchSample {
    elapsed_us: u64,
    candidates: usize,
    committed: bool,
}

/// Serialize offer/accounting against receive/accounting. The send never waits
/// for capacity; failed offers do not raise the observed queue high-water mark.
pub(super) fn offer(
    shared: &Shared,
    sender: &SyncSender<Envelope>,
    envelope: Envelope,
) -> Result<(), &'static str> {
    let mut observed = shared.telemetry.lock().unwrap();
    sender
        .try_send(envelope)
        .map_err(|_| "stats_workbuddy_scan_busy")?;
    observed.queue_depth += 1;
    observed.queue_peak = observed.queue_peak.max(observed.queue_depth);
    Ok(())
}

pub(super) fn received(shared: &Shared) {
    shared.telemetry.lock().unwrap().queue_depth -= 1;
}

/// Called at production source I/O boundaries, on the calling OS thread.
pub(super) fn source_io(shared: &Shared) {
    let expected = std::thread::current().name() == Some("workbuddy-import");
    let mut observed = shared.telemetry.lock().unwrap();
    observed.source_io_thread_checks += 1;
    observed.source_io_thread_violations += u64::from(!expected);
}

/// Measures the actual writer transaction, including failures, before its ACK.
/// Last 32 samples are bounded; totals/maxima include all source batches.
pub(super) fn batch_finished(
    shared: &Shared,
    elapsed: Duration,
    candidates: usize,
    committed: bool,
) {
    let elapsed_us = elapsed.as_micros().min(u64::MAX as u128) as u64;
    let mut observed = shared.telemetry.lock().unwrap();
    observed.writer_batch_count += 1;
    observed.writer_batch_failures += u64::from(!committed);
    observed.writer_batch_total_us = observed.writer_batch_total_us.saturating_add(elapsed_us);
    observed.writer_batch_max_us = observed.writer_batch_max_us.max(elapsed_us);
    observed.recent_batches.push_back(BatchSample {
        elapsed_us,
        candidates,
        committed,
    });
    if observed.recent_batches.len() > RECENT_BATCHES {
        observed.recent_batches.pop_front();
    }
}

pub(super) fn snapshot(shared: &Shared) -> serde_json::Value {
    serde_json::to_value(&*shared.telemetry.lock().unwrap())
        .expect("fixed telemetry fields always serialize")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejected_offer_does_not_invent_a_higher_queue_peak() {
        let shared = Shared::default();
        let (sender, receiver) = mpsc::sync_channel(super::super::super::IMPORT_CHANNEL_CAPACITY);
        for _ in 0..2 {
            let (reply, _) = mpsc::sync_channel(1);
            offer(
                &shared,
                &sender,
                Envelope {
                    work: Work::CatchUp,
                    reply,
                },
            )
            .unwrap();
        }
        let (reply, _) = mpsc::sync_channel(1);
        assert_eq!(
            offer(
                &shared,
                &sender,
                Envelope {
                    work: Work::CatchUp,
                    reply
                }
            ),
            Err("stats_workbuddy_scan_busy")
        );
        assert_eq!(snapshot(&shared)["queue_peak"], 2);
        assert_eq!(snapshot(&shared)["queue_depth"], 2);
        receiver.try_recv().unwrap();
        received(&shared);
        assert_eq!(snapshot(&shared)["queue_depth"], 1);
    }

    #[test]
    fn off_worker_calls_are_detected_and_batch_history_is_bounded() {
        let shared = Arc::new(Shared::default());
        source_io(&shared);
        let worker = shared.clone();
        std::thread::Builder::new()
            .name("workbuddy-import".into())
            .spawn(move || source_io(&worker))
            .unwrap()
            .join()
            .unwrap();
        for number in 0..40 {
            batch_finished(&shared, Duration::from_micros(number + 1), 1, number != 0);
        }
        let value = snapshot(&shared);
        assert_eq!(value["source_io_thread_checks"], 2);
        assert_eq!(value["source_io_thread_violations"], 1);
        assert_eq!(value["writer_batch_count"], 40);
        assert_eq!(value["writer_batch_failures"], 1);
        assert_eq!(value["writer_batch_max_us"], 40);
        assert_eq!(value["writer_batch_total_us"], 820);
        assert_eq!(
            value["recent_batches"].as_array().unwrap().len(),
            RECENT_BATCHES
        );
        assert_eq!(value["recent_batches"][0]["elapsed_us"], 9);
    }
}
