//! Reports and historical import polls hand off without overlapping. A
//! continuous stream of reports still grants imports regular opportunities.
//! Proxy events, controls and shutdown never consult this gate.
use std::sync::{Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

const MAX_IMPORT_QUIET: Duration = Duration::from_millis(400);
const IMPORT_HANDOFF_WAIT: Duration = Duration::from_millis(75);
const IMPORT_HANDOFF_POLL: Duration = Duration::from_millis(25);

#[derive(Default)]
struct Activity {
    readers: usize,
    import_active: bool,
    import_requested: bool,
    handoff_deadline: Option<Instant>,
}

#[derive(Default)]
pub(super) struct ReadActivity {
    state: Mutex<Activity>,
    ready: Condvar,
}

impl ReadActivity {
    fn state(&self) -> MutexGuard<'_, Activity> {
        self.state.lock().unwrap_or_else(|error| error.into_inner())
    }

    pub(super) fn enter(&self) -> ReadGuard<'_> {
        let mut state = self.state();
        loop {
            if state.import_active || (state.import_requested && state.readers != 0) {
                state = self
                    .ready
                    .wait(state)
                    .unwrap_or_else(|error| error.into_inner());
                continue;
            }
            if !state.import_requested {
                state.readers += 1;
                break;
            }
            // The writer never waits for us: it continues handling proxy and
            // control work. Abandon its handoff if that work keeps it busy.
            let now = Instant::now();
            let deadline = *state
                .handoff_deadline
                .get_or_insert(now + IMPORT_HANDOFF_WAIT);
            if now >= deadline {
                state.import_requested = false;
                state.handoff_deadline = None;
                continue;
            }
            state = self
                .ready
                .wait_timeout(state, deadline - now)
                .unwrap_or_else(|error| error.into_inner())
                .0;
        }
        ReadGuard(self)
    }

    pub(super) fn is_active(&self) -> bool {
        self.state().readers != 0
    }

    pub(super) fn import_reserved(&self) -> bool {
        self.state().import_requested
    }

    pub(super) fn writer_wait(&self, ordinary: Duration) -> Duration {
        if self.import_reserved() {
            ordinary.min(IMPORT_HANDOFF_POLL)
        } else {
            ordinary
        }
    }

    pub(super) fn cancel_import_reservation(&self) {
        let mut state = self.state();
        state.import_requested = false;
        state.handoff_deadline = None;
        self.ready.notify_all();
    }

    pub(super) fn try_enter_import(&self) -> Option<ImportGuard<'_>> {
        let mut state = self.state();
        if state.import_active {
            return None;
        }
        if state.readers != 0 {
            state.import_requested = true;
            return None;
        }
        state.import_active = true;
        state.import_requested = false;
        state.handoff_deadline = None;
        Some(ImportGuard(self))
    }
}

pub(super) struct ReadGuard<'a>(&'a ReadActivity);

impl Drop for ReadGuard<'_> {
    fn drop(&mut self) {
        let mut state = self.0.state();
        state.readers -= 1;
        if state.readers == 0 && state.import_requested {
            state.handoff_deadline = Some(Instant::now() + IMPORT_HANDOFF_WAIT);
        }
        self.0.ready.notify_all();
    }
}

pub(super) struct ImportGuard<'a>(&'a ReadActivity);

impl Drop for ImportGuard<'_> {
    fn drop(&mut self) {
        let mut state = self.0.state();
        state.import_active = false;
        self.0.ready.notify_all();
    }
}

#[derive(Default)]
pub(super) struct ImportYield {
    next_opportunity: Option<Instant>,
}

impl ImportYield {
    pub(super) fn allows_import(&mut self, reports_active: bool, now: Instant) -> bool {
        if !reports_active {
            self.next_opportunity = None;
            return true;
        }
        let deadline = self.next_opportunity.get_or_insert(now + MAX_IMPORT_QUIET);
        if now < *deadline {
            return false;
        }
        // Reserve one normal bounded poll, then begin another quiet window.
        // ReadActivity must still finish the current report before granting
        // the poll; this opportunity never authorizes concurrent import I/O.
        *deadline = now + MAX_IMPORT_QUIET;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, mpsc};

    #[test]
    fn an_active_report_cannot_be_overlapped_by_an_import_opportunity() {
        let activity = ReadActivity::default();
        let report = activity.enter();
        let start = Instant::now();
        let mut policy = ImportYield::default();
        assert!(!policy.allows_import(true, start));
        assert!(policy.allows_import(true, start + MAX_IMPORT_QUIET));
        assert!(activity.try_enter_import().is_none());
        assert!(activity.import_reserved());
        drop(report);
        let import = activity.try_enter_import().unwrap();
        assert!(!activity.import_reserved());
        assert!(activity.try_enter_import().is_none());
        drop(import);
        assert!(activity.try_enter_import().is_some());
    }

    #[test]
    fn an_inflight_import_finishes_before_a_report_enters() {
        let activity = Arc::new(ReadActivity::default());
        let import = activity.try_enter_import().unwrap();
        let other = activity.clone();
        let (attempted, attempt) = mpsc::channel();
        let (entered, enter) = mpsc::channel();
        let reader = std::thread::spawn(move || {
            attempted.send(()).unwrap();
            let _guard = other.enter();
            entered.send(()).unwrap();
        });
        attempt.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(enter.recv_timeout(Duration::from_millis(20)).is_err());
        assert!(!activity.is_active());
        drop(import);
        enter.recv_timeout(Duration::from_secs(1)).unwrap();
        reader.join().unwrap();
        assert!(!activity.is_active());
    }

    #[test]
    fn a_reserved_import_gets_a_turn_before_the_next_report() {
        let activity = Arc::new(ReadActivity::default());
        let first = activity.enter();
        assert!(activity.try_enter_import().is_none());
        let other = activity.clone();
        let (attempted, attempt) = mpsc::channel();
        let (entered, enter) = mpsc::channel();
        let reader = std::thread::spawn(move || {
            attempted.send(()).unwrap();
            let _guard = other.enter();
            entered.send(()).unwrap();
        });
        attempt.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(enter.recv_timeout(Duration::from_millis(20)).is_err());
        drop(first);
        let import = activity.try_enter_import().unwrap();
        assert!(enter.recv_timeout(Duration::from_millis(20)).is_err());
        drop(import);
        enter.recv_timeout(Duration::from_secs(1)).unwrap();
        reader.join().unwrap();
    }

    #[test]
    fn proxy_or_control_work_cannot_leave_reports_waiting_for_an_abandoned_import() {
        let activity = ReadActivity::default();
        let first = activity.enter();
        assert!(activity.try_enter_import().is_none());
        let start = Instant::now();
        drop(first);
        // No writer poll: proxy/control priority keeps the writer busy.
        let next = activity.enter();
        assert!(start.elapsed() >= IMPORT_HANDOFF_WAIT);
        assert!(start.elapsed() < Duration::from_secs(1));
        assert!(!activity.import_reserved());
        assert!(activity.is_active());
        drop(next);
        assert!(activity.try_enter_import().is_some());
    }

    #[test]
    fn an_import_error_or_unwind_releases_the_report_admission() {
        let activity = ReadActivity::default();
        let result = std::panic::catch_unwind(|| {
            let _guard = activity.try_enter_import().unwrap();
            panic!("controlled import failure");
        });
        assert!(result.is_err());
        let report = activity.enter();
        assert!(activity.is_active());
        drop(report);
        assert!(activity.try_enter_import().is_some());
    }

    #[test]
    fn cancelled_import_reservations_do_not_block_following_reports() {
        let activity = ReadActivity::default();
        let first = activity.enter();
        assert!(activity.try_enter_import().is_none());
        activity.cancel_import_reservation();
        assert!(!activity.import_reserved());
        let next = activity.enter();
        drop(first);
        assert!(activity.is_active());
        drop(next);
        assert!(!activity.is_active());
    }

    #[test]
    fn writer_handoff_wait_fits_the_reservation_without_shortening_idle_waits() {
        let activity = ReadActivity::default();
        let ordinary = Duration::from_millis(100);
        assert_eq!(activity.writer_wait(ordinary), ordinary);
        let report = activity.enter();
        assert!(activity.try_enter_import().is_none());
        assert_eq!(activity.writer_wait(ordinary), IMPORT_HANDOFF_POLL);
        assert!(activity.writer_wait(ordinary) < IMPORT_HANDOFF_WAIT);
        let shorter = Duration::from_millis(5);
        assert_eq!(activity.writer_wait(shorter), shorter);
        activity.cancel_import_reservation();
        assert_eq!(activity.writer_wait(ordinary), ordinary);
        drop(report);
    }

    #[test]
    fn overlapping_report_guards_release_only_after_the_last_reader() {
        let activity = ReadActivity::default();
        assert!(!activity.is_active());
        let first = activity.enter();
        let second = activity.enter();
        assert!(activity.is_active());
        drop(first);
        assert!(activity.is_active());
        drop(second);
        assert!(!activity.is_active());
    }

    #[test]
    fn a_report_error_or_unwind_releases_the_activity_guard() {
        let activity = ReadActivity::default();
        let result = std::panic::catch_unwind(|| {
            let _guard = activity.enter();
            assert!(activity.is_active());
            panic!("controlled report failure");
        });
        assert!(result.is_err());
        assert!(!activity.is_active());
    }

    #[test]
    fn reports_get_a_bounded_quiet_window_and_imports_keep_progressing() {
        let start = Instant::now();
        let mut policy = ImportYield::default();
        let mut granted = Vec::new();
        for ms in 0..=4000 {
            if policy.allows_import(true, start + Duration::from_millis(ms)) {
                granted.push(ms);
            }
        }
        assert_eq!(granted, (1..=10).map(|i| i * 400).collect::<Vec<_>>());
    }

    #[test]
    fn completed_reports_release_imports_immediately_and_reset_the_window() {
        let start = Instant::now();
        let mut policy = ImportYield::default();
        assert!(!policy.allows_import(true, start));
        assert!(policy.allows_import(false, start + Duration::from_millis(10)));
        assert!(policy.allows_import(false, start + Duration::from_millis(11)));
        assert!(!policy.allows_import(true, start + Duration::from_millis(12)));
        assert!(!policy.allows_import(true, start + Duration::from_millis(411)));
        assert!(policy.allows_import(true, start + Duration::from_millis(412)));
    }

    #[test]
    fn a_long_scheduler_gap_grants_one_opportunity_without_a_catchup_burst() {
        let start = Instant::now();
        let mut policy = ImportYield::default();
        assert!(!policy.allows_import(true, start));
        let resumed = start + Duration::from_secs(60);
        assert!(policy.allows_import(true, resumed));
        assert!(!policy.allows_import(true, resumed));
        assert!(!policy.allows_import(true, resumed + Duration::from_millis(399)));
        assert!(policy.allows_import(true, resumed + Duration::from_millis(400)));
    }
}
