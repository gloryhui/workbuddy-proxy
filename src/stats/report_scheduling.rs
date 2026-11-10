//! Reports and historical import polls hand off without overlapping. A
//! continuous stream of reports still grants imports regular opportunities.
//! Proxy events, controls and shutdown never consult this gate.
//!
//! 这里协调的是 Core 内“报表读取”和“历史导入的一次有界写入”，不是所有 SQLite
//! 读写的全局锁。`stats-reader` 在执行查询前取得 `ReadGuard`；stats-writer 内的导入
//! runtime 尝试取得 `ImportGuard`。代理事件、管理控制和关闭流程仍按原调度执行，
//! 不必等待这道门禁。Java 读者可类比 `Lock + Condition` 管理的准入协议：
//! 短锁只保护计数和预约状态，真正的查询/导入 I/O 在释放 Mutex 后进行。
//!
//! 两层策略各负其责：`ImportYield` 在持续有报表时每 400ms 提供一次导入机会，
//! `ReadActivity` 再确保已有 reader 完成后才准入导入。“机会”不保证导入在 400ms
//! 内完成或一定被调度；若 writer 忙于代理/控制，空闲后的 reader 最多为预约交接
//! 等待 75ms，随后撤销预约，避免报表永远等一个尚未开始的导入。
use std::sync::{Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

const MAX_IMPORT_QUIET: Duration = Duration::from_millis(400);
const IMPORT_HANDOFF_WAIT: Duration = Duration::from_millis(75);
const IMPORT_HANDOFF_POLL: Duration = Duration::from_millis(25);

#[derive(Default)]
struct Activity {
    /// 已获准执行的报表数；包含已准入、尚在打开只读连接的调用。
    readers: usize,
    /// 一次导入已取得守卫；新 reader 必须等它释放。
    import_active: bool,
    /// 导入曾遇到活跃 reader，预约下一次交接；不代表导入 I/O 已开始。
    import_requested: bool,
    /// 最后一个 reader 退出后开始的交接截止时刻；使用单调时钟。
    handoff_deadline: Option<Instant>,
}

/// reader 与 writer 共享的准入状态，守卫借用它，不拥有数据库连接或事务。
#[derive(Default)]
pub(super) struct ReadActivity {
    state: Mutex<Activity>,
    ready: Condvar,
}

impl ReadActivity {
    fn state(&self) -> MutexGuard<'_, Activity> {
        self.state.lock().unwrap_or_else(|error| error.into_inner())
    }

    /// 报表线程等待准入，成功后增加 reader 计数并返回 RAII 守卫。
    ///
    /// `Condvar::wait` 在等待时释放 Mutex，醒来后重新检查条件，允许虚假唤醒。
    /// 导入运行中必须等待；预约存在且尚有 reader 时也先等最后一个 reader 退出。
    /// 已无 reader 但 writer 未接手时，只为交接保留有界窗口，超时后允许报表继续。
    /// 这不是 async 等待，所以调用位置是专用标准线程，不应搬到 GPUI 渲染线程。
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

    /// 有预约时把 writer 的普通空闲等待缩到最多 25ms，以便在 75ms 交接窗口内再尝试。
    /// 未预约时保留原等待时长；不主动 sleep，也不改变代理/控制优先级。
    pub(super) fn writer_wait(&self, ordinary: Duration) -> Duration {
        if self.import_reserved() {
            ordinary.min(IMPORT_HANDOFF_POLL)
        } else {
            ordinary
        }
    }

    /// 导入暂停、禁用或不再需要写入时撤销“尚未接手”的预约，唤醒等候报表。
    /// 不会强行终止已持有 ImportGuard 的导入。
    pub(super) fn cancel_import_reservation(&self) {
        let mut state = self.state();
        state.import_requested = false;
        state.handoff_deadline = None;
        self.ready.notify_all();
    }

    /// writer 非阻塞尝试准入：已有导入时返回 None；有 reader 时只预约并返回 None。
    /// 没有 reader 才设置 import_active 并返回守卫，因此本函数从不等待报表结束。
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
        // 正常返回、? 提前返回和 panic 展开都会释放计数；进程被直接杀死不走 Rust Drop。
        // 最后一个 reader 结束才开启 75ms 交接窗口，不截断其他已获准 reader。
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
        // 释放准入状态并唤醒 reader；事务提交/回滚由实际导入函数负责。
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
    /// 只判断本次是否提供导入机会，不执行 I/O，也不绕过 ReadActivity 的互斥准入。
    /// 没有报表就立即允许并重置窗口；持续报表时按当前时刻开启下一窗口，长停顿后
    /// 不补发积累的多次机会。修改事实的导入使用这层节流，元数据导入仍需准入门禁。
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
