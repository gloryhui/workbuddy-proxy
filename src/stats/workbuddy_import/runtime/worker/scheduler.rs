//! Scheduling state for the dedicated reader; source I/O remains in worker.rs.
use super::*;

enum Selection {
    File(String, bool),
    Retry,
    Idle,
}

pub(super) struct Worker<'a> {
    inner: &'a Inner,
    root: &'a reader::ReaderRoot,
    sender: &'a SyncSender<Envelope>,
    shared: &'a Shared,
    conn: Connection,
    catchup_due: Instant,
    observer: super::super::observer::Observer,
    discovery: Option<reader::Discovery>,
    due: Instant,
    backlog_after: String,
    backoff: Duration,
    pass_failed: bool,
    checked_pass: bool,
    full_cursor: Option<FullCursor>,
    full_active: bool,
    priority: priority::Queue,
    selected_priority: Option<priority::Pending>,
}

impl<'a> Worker<'a> {
    pub(super) fn new(
        inner: &'a Inner,
        root: &'a reader::ReaderRoot,
        sender: &'a SyncSender<Envelope>,
        shared: &'a Shared,
    ) -> Result<Self, &'static str> {
        let conn = Connection::open_with_flags(
            inner.dir.join("usage.sqlite3"),
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(|_| "stats_unavailable")?;
        conn.busy_timeout(Duration::from_millis(100))
            .map_err(|_| "stats_unavailable")?;
        let full_active = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM workbuddy_import_files WHERE scan_generation<>generation)
             AND EXISTS(SELECT 1 FROM stats_settings WHERE singleton=1 AND workbuddy_last_full_scan_ms IS NOT NULL)",
            [], |row|row.get(0)).map_err(|_|"stats_unavailable")?;
        if full_active {
            shared.progress.lock().unwrap().scan = Some(ScanTask {
                id: crate::config::token(),
                mode: ScanMode::Full,
                state: ScanState::Running,
            });
        }
        Ok(Self {
            inner,
            root,
            sender,
            shared,
            conn,
            full_active,
            full_cursor: None,
            priority: priority::Queue::default(),
            selected_priority: None,
            catchup_due: Instant::now(),
            observer: super::super::observer::Observer::default(),
            discovery: None,
            due: Instant::now(),
            backlog_after: String::new(),
            backoff: Duration::from_secs(1),
            pass_failed: false,
            checked_pass: false,
        })
    }

    pub(super) fn run(&mut self) -> Result<(), &'static str> {
        while !self.shared.stopped.load(Ordering::Acquire) {
            let epoch = self.shared.wake_epoch.load(Ordering::Acquire);
            if self.pause() || !self.observe() {
                continue;
            }
            let reconciled = self.reconcile()?;
            let requested = self.begin_tasks();
            self.discover_if_due(requested);
            match self.select_file()? {
                Selection::File(relative, is_full) => self.process_file(relative, is_full)?,
                Selection::Retry => continue,
                Selection::Idle if reconciled => continue,
                Selection::Idle if self.discovery.is_none() => self.idle(epoch)?,
                Selection::Idle => {}
            }
        }
        Ok(())
    }

    fn pause(&mut self) -> bool {
        if !self.shared.active() {
            self.discovery = None;
            self.shared.progress.lock().unwrap().discovering = false;
            let guard = self.shared.progress.lock().unwrap();
            let _ = self
                .shared
                .wake
                .wait_timeout(guard, Duration::from_millis(100));
            self.due = Instant::now();
            return true;
        }
        false
    }

    fn observe(&mut self) -> bool {
        let observation_ok =
            match self
                .observer
                .poll(self.inner, self.root, self.sender, self.shared)
            {
                Ok(()) => true,
                Err(code) => {
                    self.shared.error(code);
                    false
                }
            };
        self.pass_failed |= !observation_ok;
        if !observation_ok {
            let guard = self.shared.progress.lock().unwrap();
            let _ = self
                .shared
                .wake
                .wait_timeout(guard, Duration::from_millis(100));
            return false;
        }
        true
    }

    fn reconcile(&mut self) -> Result<bool, &'static str> {
        let reconciliation = self.conn.query_row("SELECT mapping_id FROM workbuddy_model_history
            WHERE reconcile_state IN ('pending','running') ORDER BY evidence='manual' DESC,created_at_ms,mapping_id LIMIT 1",
            [],|row|row.get::<_,String>(0)).optional().map_err(|_|"stats_unavailable")?;
        let mut progressed = false;
        if let Some(mapping_id) = reconciliation {
            match submit(
                self.inner,
                self.sender,
                self.shared,
                Work::Reconcile { mapping_id },
            ) {
                Ok(()) => progressed = true,
                Err(code) => self.shared.error(code),
            }
        }
        if Instant::now() >= self.catchup_due || self.shared.catchup_backlog.load(Ordering::Acquire)
        {
            submit(self.inner, self.sender, self.shared, Work::CatchUp)?;
            self.catchup_due = Instant::now() + reader::SYNC_INTERVAL;
        }
        Ok(progressed || self.shared.catchup_backlog.load(Ordering::Acquire))
    }

    fn begin_tasks(&mut self) -> bool {
        let (requested, begin_full) = {
            let mut state = self.shared.progress.lock().unwrap();
            let requested = std::mem::take(&mut state.requested);
            let begin_full = state
                .scan
                .as_ref()
                .is_some_and(|task| task.state == ScanState::Queued && task.mode == ScanMode::Full);
            if let Some(task) = state.scan.as_mut()
                && task.state == ScanState::Queued
            {
                task.state = ScanState::Running;
            }
            (requested, begin_full)
        };
        if begin_full {
            match submit(self.inner, self.sender, self.shared, Work::BeginFull) {
                Ok(()) => {
                    self.full_active = true;
                    self.full_cursor = None;
                }
                Err(code) => {
                    self.shared.error(code);
                    if let Some(task) = self.shared.progress.lock().unwrap().scan.as_mut() {
                        task.state = ScanState::Failed;
                    }
                }
            }
        }
        requested
    }

    fn discover_if_due(&mut self, requested: bool) {
        if self.discovery.is_none() && (requested || Instant::now() >= self.due) {
            telemetry::source_io(self.shared);
            let projects = self.root.root().join(reader::PROJECTS_DIR);
            let found = std::fs::symlink_metadata(&projects)
                .is_ok_and(|m| m.is_dir() && !m.file_type().is_symlink());
            // Discovery only begins after a successful configuration observation.
            self.pass_failed = !found;
            let mut state = self.shared.progress.lock().unwrap();
            state.root_found = found;
            state.discovering = true;
            state.discovery_complete = false;
            if !found {
                state.last_error_code = Some("stats_workbuddy_root_missing");
            } else if std::fs::read_dir(&projects).is_err() {
                state.last_error_code = Some("stats_workbuddy_permission_denied");
                self.pass_failed = true;
            }
            self.discovery = Some(self.root.discover());
            self.due = Instant::now() + reader::SYNC_INTERVAL;
        }
    }

    fn discover_slice(&mut self) -> Result<(), &'static str> {
        let Some(walk) = self.discovery.as_mut() else {
            return Ok(());
        };
        telemetry::source_io(self.shared);
        let stage = discovery::stage(&mut self.conn, self.root, walk)?;
        if let Some(code) = stage.error {
            self.pass_failed = true;
            self.shared.error(code);
        }
        if !stage.batch.files.is_empty() {
            let hints = stage
                .batch
                .files
                .iter()
                .filter(|file| file.needs_read)
                .map(|file| file.relative_path.clone())
                .collect::<Vec<_>>();
            match submit(
                self.inner,
                self.sender,
                self.shared,
                Work::Discover(stage.batch),
            ) {
                Ok(()) => {
                    for relative in hints {
                        self.priority.enqueue(relative);
                    }
                }
                Err(code) => self.submission_failed(code),
            }
        }
        if stage.finished {
            self.discovery = None;
            let mut state = self.shared.progress.lock().unwrap();
            state.discovering = false;
            state.discovery_complete = !self.pass_failed;
            self.checked_pass = true;
        }
        Ok(())
    }

    fn select_file(&mut self) -> Result<Selection, &'static str> {
        if self.discovery.is_some() {
            self.discover_slice()?;
            // A completed discovery slice may have queued changed files.
            return Ok(Selection::Retry);
        }
        if let Some(pending) = self.priority.pop() {
            let relative = pending.relative.clone();
            self.selected_priority = Some(pending);
            return Ok(Selection::File(relative, false));
        }
        match self.next_backlog()? {
            Selection::Idle if self.full_active => self.next_full(),
            selection => Ok(selection),
        }
    }

    fn next_backlog(&mut self) -> Result<Selection, &'static str> {
        let file = self
            .conn
            .query_row(
                "SELECT relative_path FROM workbuddy_import_files
                WHERE file_key>?1 AND last_completed_at_ms IS NULL
                  AND last_error_code IS NULL
                ORDER BY file_key LIMIT 1",
                params![self.backlog_after],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|_| "stats_unavailable")?;
        if let Some(relative) = file.as_ref() {
            let tx = self.conn.transaction().map_err(|_| "stats_unavailable")?;
            let settings = store::load_import_settings(&tx).map_err(|_| "stats_unavailable")?;
            self.backlog_after = file_key(&settings.namespace_id, relative);
            tx.commit().map_err(|_| "stats_unavailable")?;
        } else if !self.backlog_after.is_empty() {
            // Finish one round before declaring it empty: earlier keys may
            // still contain the next batch of a large historical file.
            self.backlog_after.clear();
            return Ok(Selection::Retry);
        } else {
            self.complete_incremental()?;
        }
        Ok(file.map_or(Selection::Idle, |path| Selection::File(path, false)))
    }

    fn complete_incremental(&mut self) -> Result<(), &'static str> {
        if self.checked_pass {
            let errors:bool=self.conn.query_row("SELECT EXISTS(SELECT 1 FROM workbuddy_import_files WHERE last_error_code IS NOT NULL)",[],|row|row.get(0))
                    .map_err(|_|"stats_unavailable")?;
            let mut state = self.shared.progress.lock().unwrap();
            if !self.pass_failed && !errors && state.root_found {
                state.last_incremental_at_ms = Some(now_ms());
                state.last_error_code = None;
            }
            if let Some(task) = state.scan.as_mut()
                && task.mode == ScanMode::Incremental
                && task.state == ScanState::Running
            {
                task.state = if self.pass_failed || errors {
                    ScanState::Failed
                } else {
                    ScanState::Completed
                };
            }
            self.checked_pass = false;
        }
        Ok(())
    }

    fn next_full(&mut self) -> Result<Selection, &'static str> {
        if self.full_cursor.is_none() {
            let next = self
                .conn
                .query_row(
                    "SELECT relative_path FROM workbuddy_import_files
                    WHERE scan_generation<>generation AND last_error_code IS NULL
                    ORDER BY file_key LIMIT 1",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .optional()
                .map_err(|_| "stats_unavailable")?;
            self.full_cursor = next.map(|relative_path| FullCursor {
                relative_path,
                offset: None,
                discarding: false,
                generation: 0,
            });
        }
        if let Some(cursor) = self.full_cursor.as_ref() {
            return Ok(Selection::File(cursor.relative_path.clone(), true));
        } else {
            let result = if self.pass_failed {
                Err("stats_workbuddy_source_read_failed")
            } else {
                submit(self.inner, self.sender, self.shared, Work::CompleteFull)
            };
            self.full_active = false;
            let mut state = self.shared.progress.lock().unwrap();
            if let Err(code) = result {
                state.last_error_code = Some(code);
            }
            if let Some(task) = state.scan.as_mut() {
                task.state = if result.is_ok() {
                    ScanState::Completed
                } else {
                    ScanState::Failed
                };
            }
        }
        Ok(Selection::Idle)
    }

    fn full_progress(work: &Work, relative: &str, is_full: bool) -> Option<(FullCursor, bool)> {
        match work {
            Work::Batch(batch) if is_full => Some((
                FullCursor {
                    relative_path: relative.into(),
                    offset: batch.scan_offset,
                    discarding: batch.discarding,
                    generation: batch
                        .expected
                        .as_ref()
                        .map_or(1, |old| old.generation + i64::from(batch.reset)),
                },
                batch.at_eof,
            )),
            _ => None,
        }
    }

    fn process_file(&mut self, relative: String, is_full: bool) -> Result<(), &'static str> {
        let pending = self.selected_priority.take();
        let cursor = if is_full {
            self.full_cursor.as_ref()
        } else {
            None
        };
        match read_file(&mut self.conn, self.root, &relative, self.shared, cursor) {
            Ok(Some(work)) => self.submit_file(work, &relative, is_full, pending),
            Ok(None) => {}
            Err(code) => self.record_read_error(&relative, is_full, code)?,
        }
        Ok(())
    }

    fn submit_file(
        &mut self,
        work: Work,
        relative: &str,
        full: bool,
        pending: Option<priority::Pending>,
    ) {
        let next_full = Self::full_progress(&work, relative, full);
        let next_priority = pending.as_ref().and_then(|pending| pending.next(&work));
        match submit(self.inner, self.sender, self.shared, work) {
            Ok(()) => {
                self.backoff = Duration::from_secs(1);
                if let Some((cursor, finished)) = next_full {
                    self.full_cursor = if finished { None } else { Some(cursor) };
                }
                if let Some(next) = next_priority {
                    self.priority.retry(next);
                }
            }
            Err(code) => {
                if let Some(pending) = pending {
                    self.priority.retry(pending);
                }
                self.submission_failed(code);
            }
        }
    }

    fn submission_failed(&mut self, code: &'static str) {
        if self.shared.active() {
            self.shared.error(code);
            self.pass_failed = true;
            let guard = self.shared.progress.lock().unwrap();
            let _ = self.shared.wake.wait_timeout(guard, self.backoff);
            self.backoff = (self.backoff * 2).min(Duration::from_secs(30));
        }
    }

    fn record_read_error(
        &mut self,
        relative: &str,
        is_full: bool,
        code: &'static str,
    ) -> Result<(), &'static str> {
        self.shared.error(code);
        self.pass_failed = true;
        if is_full {
            self.full_cursor = None;
        }
        let tx = self.conn.transaction().map_err(|_| "stats_unavailable")?;
        let settings = store::load_import_settings(&tx).map_err(|_| "stats_unavailable")?;
        tx.commit().map_err(|_| "stats_unavailable")?;
        let _ = submit(
            self.inner,
            self.sender,
            self.shared,
            Work::Error {
                file_key: file_key(&settings.namespace_id, relative),
                code,
            },
        );
        Ok(())
    }

    fn idle(&mut self, epoch: u64) -> Result<(), &'static str> {
        #[cfg(test)]
        self.shared
            .idle_snapshot_reads
            .fetch_add(1, Ordering::Relaxed);
        // The first successful backfill is already a full read. Afterwards
        // only the low-priority daily reconciliation rereads unchanged data.
        let tx = self.conn.transaction().map_err(|_| "stats_unavailable")?;
        let settings = store::load_import_settings(&tx).map_err(|_| "stats_unavailable")?;
        tx.commit().map_err(|_| "stats_unavailable")?;
        let last_check = self.shared.progress.lock().unwrap().last_incremental_at_ms;
        if !self.full_active && last_check.is_some() && !self.pass_failed {
            if settings.last_full_scan_ms.is_none() {
                let _ = submit(self.inner, self.sender, self.shared, Work::CompleteFull);
            } else if settings
                .last_full_scan_ms
                .is_some_and(|last| now_ms() - last >= 86_400_000)
            {
                let _ = self.shared.request_scan("full");
            }
        }
        self.wait_until_due(epoch);
        Ok(())
    }

    fn wait_until_due(&self, epoch: u64) {
        let now = Instant::now();
        let deadline = self
            .observer
            .next_due()
            .filter(|due| *due > now)
            .map_or(self.due, |due| due.min(self.due))
            .min(self.catchup_due);
        let mut guard = self.shared.progress.lock().unwrap();
        // Atomic checks also cover a notification arriving before wait_timeout.
        // Timed slices never return to the SQL loop before work is actually due.
        while self.shared.active()
            && self.shared.wake_epoch.load(Ordering::Acquire) == epoch
            && !guard.requested
            && Instant::now() < deadline
        {
            // A live sync defers observation. Detect its release without
            // returning to SQL while its requested flag remains set.
            self.shared.sync_active();
            if self.shared.wake_epoch.load(Ordering::Acquire) != epoch {
                break;
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            let (next, _) = self
                .shared
                .wake
                .wait_timeout(guard, remaining.min(Duration::from_millis(100)))
                .unwrap();
            guard = next;
        }
    }
}
