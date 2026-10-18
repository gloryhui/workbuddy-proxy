//! A report gets a short quiet period from historical import writes. A
//! continuous stream of reports still grants imports regular opportunities.
//! Proxy events, controls and shutdown never consult this gate.
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

const MAX_IMPORT_QUIET: Duration = Duration::from_millis(400);

#[derive(Default)]
pub(super) struct ReadActivity {
    active: AtomicUsize,
}

impl ReadActivity {
    pub(super) fn enter(&self) -> ReadGuard<'_> {
        self.active.fetch_add(1, Ordering::AcqRel);
        ReadGuard(self)
    }

    pub(super) fn is_active(&self) -> bool {
        self.active.load(Ordering::Acquire) != 0
    }
}

pub(super) struct ReadGuard<'a>(&'a ReadActivity);

impl Drop for ReadGuard<'_> {
    fn drop(&mut self) {
        self.0.active.fetch_sub(1, Ordering::AcqRel);
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
        // A report that outlasts the quiet window cannot starve imports.
        // Grant one normal bounded poll, then begin another quiet window.
        *deadline = now + MAX_IMPORT_QUIET;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
