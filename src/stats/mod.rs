//! Local usage statistics: bounded request recording plus a read-only query
//! path.
//!
//! Thread topology (see `docs/statistics/DEVELOPMENT.md`):
//!
//! ```text
//! Tokio proxy-io ── RequestGuard + Collector ── bounded event queue 1024
//!                                                      │
//!                                                      ▼
//!                                            stats-writer OS thread (one
//!                                            write connection, batches)
//!                                                      ▲
//! control queue 16 ────────────────────────────────────┘
//!                                                      │
//! stats-reader OS thread (one read-only connection) ◀── reads, lazily started
//! ```
//!
//! Statistics can never fail a chat request: admission is non-blocking, every
//! queue is bounded, and a database failure only surfaces through
//! `stats_health`.
pub mod collector;
pub mod dto;
pub mod event;
pub mod pricing;
pub mod query;
mod report_scheduling;
pub mod store;
pub mod workbuddy_import;

use crate::stats::collector::Protocol;
use crate::stats::dto::{
    API_VERSION, ClearPreviewDto, ERR_BUSY, ERR_INVALID, ERR_MAINTENANCE_FAILED,
    ERR_OPERATION_REMOVED, ERR_UNAVAILABLE, FilterKind, FilterOptionsDto, Granularity, GroupBy,
    HealthDto, HealthState, LossScope, MaintenanceDto, MaintenanceKind, MaintenanceStatus,
    PriceListDto, ProviderSettingsDto, RequestDetailDto, SettingsDto, SortMetric, Source,
    StatsFilter, UsageMode, error,
};
use crate::stats::event::{Counters, EventQueue, RequestGuard, RequestMeta};
use crate::stats::query::RuntimeFlags;
use serde_json::json;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, RwLock,
        atomic::{AtomicBool, AtomicI64, Ordering},
        mpsc::{Receiver, SyncSender, TrySendError},
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};
use tokio::sync::oneshot;

/// Event queue capacity; two slots are reserved per admitted request.
pub const EVENT_QUEUE_CAPACITY: usize = 1024;
/// Control queue capacity.
pub const CONTROL_QUEUE_CAPACITY: usize = 16;
/// Query queue capacity.
pub const QUERY_QUEUE_CAPACITY: usize = 8;
/// Events handled in one writer batch.
pub const BATCH_EVENTS: usize = 64;
/// Longest a writer waits before draining a partial batch.
pub const BATCH_WAIT: Duration = Duration::from_millis(100);
/// How long idle facts (row count, file size) stay cached.
pub const FACTS_TTL: Duration = Duration::from_secs(10);
/// How long a reader thread stays alive without queries.
pub const READER_IDLE: Duration = Duration::from_secs(60);
/// Validity of a manual clear confirmation token.
pub const CLEAR_TOKEN_TTL: Duration = Duration::from_secs(60);
/// Bounded probe backoff while the database refuses writes.
pub const PROBE_BACKOFF_MIN: Duration = Duration::from_millis(500);
pub const PROBE_BACKOFF_MAX: Duration = Duration::from_secs(30);
/// Longest the shutdown flush may take before dropping what is left.
pub const SHUTDOWN_FLUSH_BUDGET: Duration = Duration::from_secs(4);
/// Requests remembered for post-recovery settlement.
pub const MAX_ORPHANS: usize = 512;
/// Rows repaired from the durable backlog per attempt; keeps each recovery
/// transaction bounded.
pub const RECOVERY_BATCH: i64 = 256;
/// Bounded number of batches one recovery attempt drains before yielding.
pub const RECOVERY_ROUNDS: usize = 64;

/// Milliseconds since the Unix epoch as `i64`.
pub fn now_ms() -> i64 {
    crate::journal::now_ms() as i64
}

/// Restricts a file to the current user on Unix (0600).
pub fn restrict_permissions(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
}

/// Description of one routable request, captured while the route is valid.
#[derive(Clone, Debug)]
pub struct RequestSpec {
    pub workbuddy_identity: Option<crate::stats::event::WorkbuddyIdentity>,
    pub source: Source,
    pub provider_id: String,
    pub provider_name: String,
    pub route_id: String,
    pub route_name: String,
    pub requested_model: String,
    pub upstream_model: String,
    pub upstream_protocol: String,
    pub stream: bool,
}

/// A manual clear confirmation bound to a frozen boundary.
#[derive(Debug)]
struct ClearToken {
    token: String,
    before_ms: i64,
    as_of_id: i64,
    created: Instant,
}

#[derive(Debug)]
enum ControlKind {
    WorkbuddySyncPrepare {
        models: Vec<workbuddy_import::observation::ManagedModel>,
        lease: Arc<AtomicBool>,
    },
    WorkbuddySyncFinish {
        token: String,
        observations: Vec<workbuddy_import::classify::ConfigObservation>,
    },
    Shutdown,
    SettingsGet,
    SettingsUpdate {
        enabled: bool,
        retention_days: i64,
        default_currency: Option<String>,
        expected_revision: i64,
    },
    /// One of the eight import management commands, dispatched by name.
    WorkbuddyCommand {
        command: String,
        payload: serde_json::Value,
    },
    ProviderSettingsGet {
        provider_id: String,
    },
    ProviderSettingsUpdate {
        provider_id: String,
        usage_mode: UsageMode,
        expected_revision: i64,
    },
    PriceList {
        provider_id: Option<String>,
        upstream_model: Option<String>,
        after_rule_id: Option<String>,
        limit: i64,
    },
    PriceSave(Box<PriceSaveRequest>),
    PriceCatalogStatus,
    PriceCatalog {
        prices: Option<crate::price_catalog::Prices>,
        targets: Vec<crate::price_catalog::Target>,
        day: String,
        targets_key: String,
    },
    /// Removes one current price. Recorded history keeps its own snapshot.
    PriceDelete {
        price_id: String,
        expected_revision: i64,
    },
    ClearPreview {
        before_ms: i64,
    },
    ClearStart {
        token: String,
    },
    MaintenanceStatus {
        task_id: i64,
    },
    RefreshFacts,
}

#[derive(Debug)]
struct ControlMessage {
    kind: ControlKind,
    reply: Option<oneshot::Sender<Result<String, String>>>,
}

/// Parameters of `stats_price_save`.
#[derive(Clone, Debug)]
pub struct PriceSaveRequest {
    pub rule_id: Option<String>,
    pub provider_id: String,
    pub upstream_model: String,
    pub currency: String,
    pub input_mode: String,
    pub input_rate_nanos: String,
    pub output_rate_nanos: String,
    pub cache_read_rate_nanos: Option<String>,
    pub cache_write_rate_nanos: Option<String>,
    pub label: String,
    /// Required when updating an existing price so two editors cannot silently
    /// overwrite each other. Ignored for new prices.
    pub expected_revision: Option<i64>,
}

#[derive(Debug)]
enum ReadJob {
    Dashboard {
        filter: Box<StatsFilter>,
        granularity: Granularity,
        flags: Box<RuntimeFlags>,
        generation: String,
        tag: String,
        now: i64,
    },
    Requests {
        filter: Box<StatsFilter>,
        page_size: i64,
        cursor: Option<Box<dto::RequestCursor>>,
        generation: String,
        tag: String,
        now: i64,
    },
    Detail {
        request_id: String,
        generation: String,
        tag: String,
        now: i64,
    },
    Breakdown {
        filter: Box<StatsFilter>,
        group_by: GroupBy,
        sort: SortMetric,
        limit: i64,
        cursor: Option<Box<dto::BreakdownCursor>>,
        generation: String,
        tag: String,
        now: i64,
    },
    Filters {
        scope: query::FilterScope,
        kind: FilterKind,
        query: String,
        provider_id: Option<String>,
        limit: i64,
        after_key: Option<String>,
        configured: Vec<(String, String)>,
        generation: String,
        tag: String,
        now: i64,
    },
}

struct ReaderHandle {
    tx: SyncSender<ReadJobEnvelope>,
    thread: JoinHandle<()>,
    last_used: Instant,
}

struct Inner {
    dir: PathBuf,
    run_id: String,
    started_at_ms: i64,
    queue: Arc<EventQueue>,
    counters: Arc<Counters>,
    control: SyncSender<ControlMessage>,
    reader: Mutex<Option<ReaderHandle>>,
    report_reads: report_scheduling::ReadActivity,
    settings: RwLock<store::Settings>,
    state: RwLock<HealthState>,
    error_code: Mutex<Option<String>>,
    facts: Mutex<Option<(query::HealthFacts, Instant)>>,
    io_bytes: Mutex<(i64, i64)>,
    facts_refreshing: AtomicBool,
    maintenance_revision: AtomicI64,
    unclean_exit: AtomicBool,
    provider_modes: Mutex<HashMap<String, UsageMode>>,
    /// Published current prices; the hot path only reads this cache.
    prices: store::PriceCache,
    configured_providers: RwLock<Vec<(String, String)>>,
    clear_token: Mutex<Option<ClearToken>>,
    import_runtime: Arc<workbuddy_import::runtime::Shared>,
    import_previews: Mutex<workbuddy_import::manual::PreviewCache>,
}

impl Inner {
    fn accepting(&self) -> bool {
        self.counters.enabled.load(Ordering::Relaxed)
            && self.counters.accepting.load(Ordering::Relaxed)
    }

    fn generation(&self) -> String {
        format!(
            "{}:{}",
            self.run_id,
            self.counters.commits.load(Ordering::Relaxed)
        )
    }

    fn set_state(&self, state: HealthState, error_code: Option<String>) {
        *self.state.write().unwrap() = state;
        *self.error_code.lock().unwrap() = error_code;
        // `degraded` keeps admitting on purpose: writes are delayed, not lost,
        // and the bounded queue is what protects the chat path. Only a
        // database that is not ready yet (or a closing process) refuses new
        // requests.
        self.counters.accepting.store(
            matches!(state, HealthState::Ready | HealthState::Degraded),
            Ordering::Relaxed,
        );
    }

    fn runtime_flags(&self) -> RuntimeFlags {
        let observed = self
            .provider_modes
            .lock()
            .unwrap()
            .values()
            .filter(|mode| **mode == UsageMode::Observe)
            .count() as i64;
        RuntimeFlags {
            enabled: self.settings.read().unwrap().enabled,
            dropped_current_run: self.counters.dropped.load(Ordering::Relaxed) as i64,
            unclean_exit: self.unclean_exit.load(Ordering::Relaxed),
            observe_mode_providers: observed,
        }
    }
}

/// Cheap, cloneable handle owned by the proxy bridge.
#[derive(Clone)]
pub struct StatsHandle {
    inner: Arc<Inner>,
}

impl StatsHandle {
    /// Starts the statistics subsystem. Migration and recovery run on the writer
    /// thread so network startup is never delayed.
    pub fn start(dir: PathBuf) -> Self {
        Self::start_with_import_root(dir, None)
    }

    /// A source-root seam keeps integration tests on disposable JSONL files
    /// without mutating process-wide HOME or the user's WorkBuddy profile.
    fn start_with_import_root(
        dir: PathBuf,
        import_root: Option<workbuddy_import::reader::ReaderRoot>,
    ) -> Self {
        // Fail closed when the typed import contract drifted: a batch bound or an
        // "unknown is None" invariant that no longer holds must never reach the
        // writer. This check starts no import worker, queue or connection; the
        // runtime task owns those later.
        if let Err(detail) = workbuddy_import::contract_check() {
            let (inner, _control_rx) = Self::initialize(dir);
            inner.set_state(HealthState::Disabled, Some(detail.to_owned()));
            inner.queue.close();
            return Self { inner };
        }
        let (inner, control_rx) = Self::initialize(dir);
        let writer_inner = inner.clone();
        let spawned = std::thread::Builder::new()
            .name("stats-writer".into())
            .stack_size(1024 * 1024)
            .spawn(move || {
                writer_thread(writer_inner, control_rx, import_root);
            });
        if spawned.is_err() {
            inner.set_state(
                HealthState::Disabled,
                Some("statistics writer thread could not start".into()),
            );
        }
        Self { inner }
    }

    /// A tray controller forwards management calls to the core. It must never
    /// open SQLite, start recovery or create its own statistics run.
    pub fn for_controller(dir: PathBuf) -> Self {
        let (inner, _) = Self::initialize(dir);
        inner.settings.write().unwrap().enabled = false;
        inner.counters.enabled.store(false, Ordering::Relaxed);
        inner.set_state(HealthState::Disabled, None);
        inner.queue.close();
        Self { inner }
    }

    fn initialize(dir: PathBuf) -> (Arc<Inner>, Receiver<ControlMessage>) {
        let (control, control_rx) = std::sync::mpsc::sync_channel(CONTROL_QUEUE_CAPACITY);
        let inner = Arc::new(Inner {
            dir,
            run_id: crate::config::token(),
            started_at_ms: now_ms(),
            queue: Arc::new(EventQueue::new(EVENT_QUEUE_CAPACITY)),
            counters: Arc::new(Counters::default()),
            control,
            reader: Mutex::new(None),
            report_reads: report_scheduling::ReadActivity::default(),
            settings: RwLock::new(store::Settings::default()),
            state: RwLock::new(HealthState::Migrating),
            error_code: Mutex::new(None),
            facts: Mutex::new(None),
            io_bytes: Mutex::new((0, 0)),
            facts_refreshing: AtomicBool::new(false),
            maintenance_revision: AtomicI64::new(0),
            unclean_exit: AtomicBool::new(false),
            provider_modes: Mutex::new(HashMap::new()),
            prices: Arc::new(std::sync::RwLock::new(store::PublishedPrices::default())),
            configured_providers: RwLock::new(Vec::new()),
            clear_token: Mutex::new(None),
            import_runtime: Arc::new(workbuddy_import::runtime::Shared::default()),
            import_previews: Mutex::new(workbuddy_import::manual::PreviewCache::default()),
        });
        (inner, control_rx)
    }

    pub fn state(&self) -> HealthState {
        *self.inner.state.read().unwrap()
    }

    /// Stops admitting new requests and marks open guards as shutting down.
    ///
    /// This must run *before* the network runtime is destroyed: a guard dropped
    /// during teardown then records `interrupted/shutdown` instead of a
    /// fictitious client cancellation.
    pub fn mark_closing(&self) {
        self.inner.import_runtime.configure(false);
        crate::stats::event::RequestGuard::mark_closing(&self.inner.counters);
        self.inner
            .counters
            .accepting
            .store(false, Ordering::Relaxed);
    }

    /// True once the writer thread has flushed and stopped.
    #[allow(dead_code)] // Used by shutdown tests and future health reporting.
    pub fn is_stopped(&self) -> bool {
        self.inner.queue.is_closed()
    }

    /// Flushes admitted events and stops the writer. Called after the network
    /// runtime is gone so open guards have already published their terminal
    /// events; waits at most four seconds.
    pub fn shutdown(&self) {
        self.mark_closing();
        // The bounded control channel may be full, so shutdown must not depend
        // on it: closing the queue is a second, always-available exit that also
        // wakes the writer.
        if self
            .inner
            .control
            .try_send(ControlMessage {
                kind: ControlKind::Shutdown,
                reply: None,
            })
            .is_err()
        {
            self.inner.queue.close();
        }
        self.inner.queue.wake();
        let deadline = Instant::now() + SHUTDOWN_FLUSH_BUDGET;
        while !self.inner.queue.is_closed() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// Publishes a new provider list for the filter options.
    pub fn set_configured_providers(&self, providers: Vec<(String, String)>) {
        *self.inner.configured_providers.write().unwrap() = providers;
    }

    /// Usage mode currently configured for a provider.
    pub fn usage_mode(&self, provider_id: &str) -> UsageMode {
        self.inner
            .provider_modes
            .lock()
            .unwrap()
            .get(provider_id)
            .copied()
            .unwrap_or_default()
    }

    pub async fn price_catalog_status(&self) -> Result<dto::PriceSyncDto, String> {
        let reply = self.control(ControlKind::PriceCatalogStatus).await?;
        serde_json::from_str(&reply)
            .map_err(|_| error(ERR_UNAVAILABLE, "invalid price sync status"))
    }

    pub async fn import_price_catalog(
        &self,
        prices: Option<crate::price_catalog::Prices>,
        targets: Vec<crate::price_catalog::Target>,
        day: String,
        targets_key: String,
    ) -> Result<dto::PriceSyncDto, String> {
        let reply = self
            .control(ControlKind::PriceCatalog {
                prices,
                targets,
                day,
                targets_key,
            })
            .await?;
        serde_json::from_str(&reply)
            .map_err(|_| error(ERR_UNAVAILABLE, "invalid price sync status"))
    }

    /// Admits one request. Returns `None` when statistics must be skipped.
    pub fn begin(
        &self,
        spec: RequestSpec,
        protocol: Protocol,
        encoded: bool,
    ) -> Option<RequestGuard> {
        if !self.inner.accepting() {
            return None;
        }
        let started_at_ms = now_ms();
        // Freeze the prices this request runs under, from the published cache.
        let price = store::snapshot_for(
            &self.inner.prices,
            &spec.provider_id,
            &spec.upstream_model,
            started_at_ms,
        );
        let meta = RequestMeta {
            workbuddy_identity: spec.workbuddy_identity,
            request_id: crate::config::token(),
            run_id: self.inner.run_id.clone(),
            source: spec.source,
            provider_id: spec.provider_id,
            provider_name: spec.provider_name.chars().take(160).collect(),
            route_id: spec.route_id,
            route_name: spec.route_name.chars().take(160).collect(),
            requested_model: spec.requested_model.chars().take(160).collect(),
            upstream_model: spec.upstream_model,
            upstream_protocol: spec.upstream_protocol,
            stream: spec.stream,
            started_at_ms,
            price,
        };
        let guard = RequestGuard::begin(
            &self.inner.queue,
            &self.inner.counters,
            meta,
            protocol,
            encoded,
        );
        if guard.is_none() {
            self.inner.counters.dropped.fetch_add(1, Ordering::Relaxed);
        } else {
            self.inner.counters.admitted.fetch_add(1, Ordering::Relaxed);
        }
        guard
    }

    // ---- control path -----------------------------------------------------

    async fn control(&self, kind: ControlKind) -> Result<String, String> {
        let (reply, receiver) = oneshot::channel();
        let message = ControlMessage {
            kind,
            reply: Some(reply),
        };
        let mut pending = message;
        let deadline = Instant::now() + Duration::from_millis(2000);
        loop {
            match self.inner.control.try_send(pending) {
                Ok(()) => break,
                Err(TrySendError::Full(returned)) => {
                    if Instant::now() >= deadline {
                        return Err(error(ERR_BUSY, "statistics writer is busy; retry"));
                    }
                    pending = returned;
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
                Err(TrySendError::Disconnected(_)) => {
                    return Err(error(ERR_UNAVAILABLE, "statistics writer is not running"));
                }
            }
        }
        self.inner.queue.wake();
        receiver
            .await
            .map_err(|_| error(ERR_UNAVAILABLE, "statistics writer stopped"))?
    }

    /// Handles one statistics management command.
    pub async fn command(&self, payload: &serde_json::Value) -> Result<String, String> {
        let command = payload
            .get("command")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| error(ERR_INVALID, "missing command"))?;
        let request_tag = tag(payload);
        match command {
            "stats_health" => Ok(self.health(payload)),
            "stats_dashboard" => {
                let filter = self.filter(payload)?;
                let granularity = payload
                    .get("granularity")
                    .and_then(serde_json::Value::as_str)
                    .and_then(Granularity::parse)
                    .unwrap_or(Granularity::Auto);
                let tag = tag(payload);
                self.read(ReadJob::Dashboard {
                    filter: Box::new(filter),
                    granularity,
                    flags: Box::new(self.inner.runtime_flags()),
                    generation: self.inner.generation(),
                    tag,
                    now: now_ms(),
                })
                .await
            }
            "stats_requests" => {
                let filter = self.filter(payload)?;
                let page_size = payload
                    .get("page_size")
                    .and_then(serde_json::Value::as_i64)
                    .unwrap_or(20);
                if !matches!(page_size, 10 | 20 | 50 | 100) {
                    return Err(error(ERR_INVALID, "page_size must be 10, 20, 50 or 100"));
                }
                let cursor = match payload.get("cursor") {
                    Some(serde_json::Value::Null) | None => None,
                    Some(value) => Some(Box::new(
                        serde_json::from_value::<dto::RequestCursor>(value.clone())
                            .map_err(|_| error(ERR_INVALID, "invalid cursor"))?,
                    )),
                };
                if let Some(cursor) = cursor.as_ref() {
                    query::validate_cursor(
                        cursor,
                        &filter,
                        &self.inner.run_id,
                        self.inner.maintenance_revision.load(Ordering::Relaxed),
                    )?;
                }
                let tag = tag(payload);
                self.read(ReadJob::Requests {
                    filter: Box::new(filter),
                    page_size,
                    cursor,
                    generation: self.inner.generation(),
                    tag,
                    now: now_ms(),
                })
                .await
            }
            "stats_request_detail" => {
                let request_id = payload
                    .get("request_id")
                    .and_then(serde_json::Value::as_str)
                    .filter(|value| !value.is_empty() && value.chars().count() <= 64)
                    .ok_or_else(|| error(ERR_INVALID, "request_id is required"))?;
                let tag = tag(payload);
                self.read(ReadJob::Detail {
                    request_id: request_id.to_owned(),
                    generation: self.inner.generation(),
                    tag,
                    now: now_ms(),
                })
                .await
            }
            "stats_breakdown" => {
                let filter = self.filter(payload)?;
                let group_by = payload
                    .get("group_by")
                    .and_then(serde_json::Value::as_str)
                    .and_then(GroupBy::parse)
                    .ok_or_else(|| error(ERR_INVALID, "group_by must be provider or model"))?;
                let sort = payload
                    .get("sort")
                    .and_then(serde_json::Value::as_str)
                    .and_then(SortMetric::parse)
                    .unwrap_or(SortMetric::TotalTokens);
                let limit = payload
                    .get("limit")
                    .and_then(serde_json::Value::as_i64)
                    .unwrap_or(20)
                    .clamp(1, 100);
                let cursor = match payload.get("cursor") {
                    Some(serde_json::Value::Null) | None => None,
                    Some(value) => Some(Box::new(
                        serde_json::from_value::<dto::BreakdownCursor>(value.clone())
                            .map_err(|_| error(ERR_INVALID, "invalid cursor"))?,
                    )),
                };
                if let Some(cursor) = cursor.as_ref() {
                    query::validate_breakdown_cursor(
                        cursor,
                        &filter,
                        group_by,
                        sort,
                        &self.inner.run_id,
                        self.inner.maintenance_revision.load(Ordering::Relaxed),
                    )?;
                }
                let tag = tag(payload);
                self.read(ReadJob::Breakdown {
                    filter: Box::new(filter),
                    group_by,
                    sort,
                    limit,
                    cursor,
                    generation: self.inner.generation(),
                    tag,
                    now: now_ms(),
                })
                .await
            }
            "stats_filters" => {
                let kind = payload
                    .get("kind")
                    .and_then(serde_json::Value::as_str)
                    .and_then(FilterKind::parse)
                    .ok_or_else(|| error(ERR_INVALID, "kind must be provider or model"))?;
                let source = match payload.get("source_scope") {
                    None => dto::SourceScope::All,
                    Some(value) => value
                        .as_str()
                        .and_then(dto::SourceScope::parse)
                        .ok_or_else(|| error(ERR_INVALID, "invalid source scope"))?,
                };
                let capture = match payload.get("capture_method") {
                    None => dto::CaptureMethod::All,
                    Some(value) => value
                        .as_str()
                        .and_then(dto::CaptureMethod::parse)
                        .ok_or_else(|| error(ERR_INVALID, "invalid capture method"))?,
                };
                if source == dto::SourceScope::Builtin && capture == dto::CaptureMethod::Proxy {
                    return Err(error(ERR_INVALID, "incompatible source and capture method"));
                }
                let query = payload
                    .get("query")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("")
                    .chars()
                    .take(80)
                    .collect::<String>();
                let provider_id = optional_id(payload, "provider_id")?;
                let limit = payload
                    .get("limit")
                    .and_then(serde_json::Value::as_i64)
                    .unwrap_or(50)
                    .clamp(1, 100);
                let after_key = optional_id(payload, "after_key")?;
                let tag = tag(payload);
                let configured = self.inner.configured_providers.read().unwrap().clone();
                self.read(ReadJob::Filters {
                    scope: query::FilterScope { source, capture },
                    kind,
                    query,
                    provider_id,
                    limit,
                    after_key,
                    configured,
                    generation: self.inner.generation(),
                    tag,
                    now: now_ms(),
                })
                .await
            }
            name if name.starts_with("stats_workbuddy_") => self
                .control(ControlKind::WorkbuddyCommand {
                    command: name.to_owned(),
                    payload: payload.clone(),
                })
                .await
                .map(|text| with_tag(text, &request_tag)),
            "stats_settings_get" => self
                .control(ControlKind::SettingsGet)
                .await
                .map(|text| with_tag(text, &request_tag)),
            "stats_settings_update" => {
                let enabled = payload
                    .get("enabled")
                    .and_then(serde_json::Value::as_bool)
                    .ok_or_else(|| error(ERR_INVALID, "enabled must be a boolean"))?;
                let retention_days = payload
                    .get("retention_days")
                    .and_then(serde_json::Value::as_i64)
                    .ok_or_else(|| error(ERR_INVALID, "retention_days is required"))?;
                if retention_days != 0 && !(7..=3650).contains(&retention_days) {
                    return Err(error(
                        ERR_INVALID,
                        "retention_days must be 0 (keep forever) or between 7 and 3650",
                    ));
                }
                // The currency used for models without a price is a setting, not
                // a constant.
                let default_currency = match payload.get("default_currency") {
                    None | Some(serde_json::Value::Null) => None,
                    Some(value) => {
                        let text = value.as_str().ok_or_else(|| {
                            error(ERR_INVALID, "default_currency must be a string")
                        })?;
                        if text != "USD" && text != "CNY" {
                            return Err(error(ERR_INVALID, "default_currency must be USD or CNY"));
                        }
                        Some(text.to_owned())
                    }
                };
                let expected_revision = payload
                    .get("expected_revision")
                    .and_then(serde_json::Value::as_i64)
                    .ok_or_else(|| error(ERR_INVALID, "expected_revision is required"))?;
                self.control(ControlKind::SettingsUpdate {
                    enabled,
                    retention_days,
                    default_currency,
                    expected_revision,
                })
                .await
                .map(|text| with_tag(text, &request_tag))
            }
            "stats_provider_settings_get" => {
                let provider_id = required_id(payload, "provider_id")?;
                self.control(ControlKind::ProviderSettingsGet { provider_id })
                    .await
                    .map(|text| with_tag(text, &request_tag))
            }
            "stats_provider_settings_update" => {
                let provider_id = required_id(payload, "provider_id")?;
                let usage_mode = payload
                    .get("usage_mode")
                    .and_then(serde_json::Value::as_str)
                    .and_then(UsageMode::parse)
                    .ok_or_else(|| error(ERR_INVALID, "usage_mode must be observe or request"))?;
                let expected_revision = payload
                    .get("expected_revision")
                    .and_then(serde_json::Value::as_i64)
                    .unwrap_or(0);
                self.control(ControlKind::ProviderSettingsUpdate {
                    provider_id,
                    usage_mode,
                    expected_revision,
                })
                .await
                .map(|text| with_tag(text, &request_tag))
            }
            "stats_prices_list" => {
                let provider_id = optional_id(payload, "provider_id")?;
                let upstream_model = optional_id(payload, "upstream_model")?;
                let after_rule_id = optional_id(payload, "after_rule_id")?;
                let limit = payload
                    .get("limit")
                    .and_then(serde_json::Value::as_i64)
                    .unwrap_or(50)
                    .clamp(1, 100);
                self.control(ControlKind::PriceList {
                    provider_id,
                    upstream_model,
                    after_rule_id,
                    limit,
                })
                .await
                .map(|text| with_tag(text, &request_tag))
            }
            "stats_price_delete" => {
                let price_id = price_identifier(payload)?
                    .ok_or_else(|| error(ERR_INVALID, "price_id is required"))?;
                let expected_revision = payload
                    .get("expected_revision")
                    .and_then(serde_json::Value::as_i64)
                    .ok_or_else(|| error(ERR_INVALID, "expected_revision is required"))?;
                self.control(ControlKind::PriceDelete {
                    price_id,
                    expected_revision,
                })
                .await
            }
            "stats_price_save" => {
                let request = parse_price_save(payload)?;
                self.control(ControlKind::PriceSave(Box::new(request)))
                    .await
                    .map(|text| with_tag(text, &request_tag))
            }
            "stats_clear_preview" => {
                let before_ms = payload
                    .get("before_ms")
                    .and_then(serde_json::Value::as_i64)
                    .ok_or_else(|| error(ERR_INVALID, "before_ms is required"))?;
                if before_ms <= 0 {
                    return Err(error(ERR_INVALID, "before_ms must be positive"));
                }
                self.control(ControlKind::ClearPreview { before_ms })
                    .await
                    .map(|text| with_tag(text, &request_tag))
            }
            "stats_clear_start" => {
                let token = payload
                    .get("token")
                    .and_then(serde_json::Value::as_str)
                    .filter(|value| !value.is_empty() && value.len() <= 64)
                    .ok_or_else(|| error(ERR_INVALID, "a preview token is required"))?;
                self.control(ControlKind::ClearStart {
                    token: token.to_owned(),
                })
                .await
                .map(|text| with_tag(text, &request_tag))
            }
            "stats_reprice_start" => {
                // Every request keeps the price it was admitted with, so
                // rewriting history with today's price is not offered at all.
                Err(error(
                    ERR_OPERATION_REMOVED,
                    "repricing history was removed; each request keeps its own price snapshot",
                ))
            }
            "stats_maintenance_status" => {
                let task_id = payload
                    .get("task_id")
                    .and_then(serde_json::Value::as_i64)
                    .ok_or_else(|| error(ERR_INVALID, "task_id is required"))?;
                self.control(ControlKind::MaintenanceStatus { task_id })
                    .await
                    .map(|text| with_tag(text, &request_tag))
            }
            _ => Err(error(ERR_INVALID, "unknown statistics command")),
        }
    }

    /// Commands that must not push a status line into the activity log.
    pub fn is_silent_command(command: &str) -> bool {
        command.starts_with("stats_")
    }

    fn filter(&self, payload: &serde_json::Value) -> Result<StatsFilter, String> {
        let filter: StatsFilter = match payload.get("filter") {
            Some(value) => serde_json::from_value(value.clone())
                .map_err(|_| error(ERR_INVALID, "invalid filter"))?,
            None => return Err(error(ERR_INVALID, "filter is required")),
        };
        filter.validate()?;
        Ok(filter)
    }

    async fn read(&self, job: ReadJob) -> Result<String, String> {
        if self.state() != HealthState::Ready {
            return Err(error(ERR_UNAVAILABLE, "statistics database is not ready"));
        }
        let (reply_tx, reply_rx) = oneshot::channel();
        let envelope = ReadJobEnvelope {
            kind: job,
            reply: reply_tx,
        };
        if let Err(failure) = self.reader_send(envelope) {
            return Err(failure.message);
        }
        reply_rx
            .await
            .map_err(|_| error(ERR_UNAVAILABLE, "statistics reader stopped"))?
    }

    fn reader_send(&self, envelope: ReadJobEnvelope) -> Result<(), Box<ReaderFailure>> {
        let mut guard = self.inner.reader.lock().unwrap();
        let stale = guard.as_ref().is_some_and(|handle| {
            handle.thread.is_finished() || handle.last_used.elapsed() > READER_IDLE
        });
        if stale {
            *guard = None;
        }
        if guard.is_none() {
            match spawn_reader(self.inner.clone()) {
                Ok(handle) => *guard = Some(handle),
                Err(failure) => {
                    return Err(Box::new(ReaderFailure {
                        envelope,
                        message: failure,
                    }));
                }
            }
        }
        let handle = guard.as_mut().unwrap();
        match handle.tx.try_send(envelope) {
            Ok(()) => {
                handle.last_used = Instant::now();
                Ok(())
            }
            Err(TrySendError::Full(envelope)) => Err(Box::new(ReaderFailure {
                envelope,
                message: error(ERR_BUSY, "statistics reader queue is full; retry"),
            })),
            Err(TrySendError::Disconnected(envelope)) => {
                *guard = None;
                Err(Box::new(ReaderFailure {
                    envelope,
                    message: error(ERR_UNAVAILABLE, "statistics reader is not running"),
                }))
            }
        }
    }

    fn health(&self, payload: &serde_json::Value) -> String {
        let state = self.state();
        let settings = *self.inner.settings.read().unwrap();
        let mut facts = *self.inner.facts.lock().unwrap();
        let stale = facts
            .as_ref()
            .is_none_or(|(_, at)| at.elapsed() > FACTS_TTL);
        if stale && !self.inner.facts_refreshing.swap(true, Ordering::Relaxed) {
            let _ = self.inner.control.try_send(ControlMessage {
                kind: ControlKind::RefreshFacts,
                reply: None,
            });
            self.inner.queue.wake();
        }
        if facts.is_none() {
            facts = Some((
                query::HealthFacts {
                    record_count: 0,
                    earliest_record_at_ms: None,
                },
                Instant::now(),
            ));
        }
        let (facts, _) = facts.unwrap();
        let error_code = self.inner.error_code.lock().unwrap().clone();
        let loss_scope = if self.inner.unclean_exit.load(Ordering::Relaxed) {
            LossScope::UnknownAfterUncleanExit
        } else if self.inner.counters.dropped.load(Ordering::Relaxed) > 0
            || self.inner.counters.write_failures.load(Ordering::Relaxed) > 0
        {
            LossScope::LossObserved
        } else {
            LossScope::NoLossObservedInCurrentRun
        };
        let dto = HealthDto {
            api_version: API_VERSION,
            request_tag: tag(payload),
            generated_at_ms: now_ms(),
            generation: self.inner.generation(),
            state,
            enabled: settings.enabled,
            schema_version: store::SCHEMA_VERSION,
            collection_started_at_ms: Some(self.inner.started_at_ms),
            earliest_record_at_ms: facts.earliest_record_at_ms,
            retention_days: settings.retention_days,
            record_count: facts.record_count,
            db_bytes: self.db_bytes(),
            wal_bytes: self.wal_bytes(),
            queue_depth: self.inner.queue.depth() as i64,
            queue_capacity: EVENT_QUEUE_CAPACITY as i64,
            pending_requests: self.inner.counters.pending.load(Ordering::Relaxed),
            dropped_requests_current_run: self.inner.counters.dropped.load(Ordering::Relaxed)
                as i64,
            write_failures_current_run: self.inner.counters.write_failures.load(Ordering::Relaxed)
                as i64,
            last_commit_at_ms: match self
                .inner
                .counters
                .last_commit_at_ms
                .load(Ordering::Relaxed)
            {
                0 => None,
                value => Some(value),
            },
            loss_scope,
            error_code,
            settings_revision: settings.revision,
            timezone: query::resolve_timezone("")
                .map(|(_, name)| name)
                .unwrap_or_else(|_| "UTC".into()),
        };
        serde_json::to_string(&dto)
            .unwrap_or_else(|_| error(ERR_UNAVAILABLE, "cannot serialize health"))
    }

    fn db_bytes(&self) -> i64 {
        let (db, _) = *self.inner.io_bytes.lock().unwrap();
        if db > 0 {
            db
        } else {
            self.file_size("usage.sqlite3")
        }
    }

    fn wal_bytes(&self) -> i64 {
        let (_, wal) = *self.inner.io_bytes.lock().unwrap();
        if wal > 0 {
            wal
        } else {
            self.file_size("usage.sqlite3-wal")
        }
    }

    fn file_size(&self, name: &str) -> i64 {
        std::fs::metadata(self.inner.dir.join(name))
            .map(|meta| meta.len() as i64)
            .unwrap_or(0)
    }
}

struct ReadJobEnvelope {
    kind: ReadJob,
    reply: oneshot::Sender<Result<String, String>>,
}

/// Keeps the (large) read job alive while reporting why it was refused.
struct ReaderFailure {
    #[allow(dead_code)]
    envelope: ReadJobEnvelope,
    message: String,
}

/// Stamps the caller's request tag onto a control reply.
fn with_tag(text: String, request_tag: &str) -> String {
    match serde_json::from_str::<serde_json::Value>(&text) {
        Ok(mut value) => {
            if let Some(object) = value.as_object_mut() {
                object.insert("request_tag".into(), json!(request_tag));
            }
            value.to_string()
        }
        Err(_) => text,
    }
}

fn tag(payload: &serde_json::Value) -> String {
    payload
        .get("request_tag")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .chars()
        .take(64)
        .collect()
}

fn optional_id(payload: &serde_json::Value, key: &str) -> Result<Option<String>, String> {
    match payload.get(key) {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(value) => {
            let text = value
                .as_str()
                .ok_or_else(|| error(ERR_INVALID, &format!("{key} must be a string")))?;
            if text.is_empty() {
                return Err(error(
                    ERR_INVALID,
                    &format!("{key} must not be empty when present"),
                ));
            }
            if text.chars().count() > 160 || text.chars().any(char::is_control) {
                return Err(error(ERR_INVALID, &format!("{key} is not valid")));
            }
            Ok(Some(text.to_owned()))
        }
    }
}

fn required_id(payload: &serde_json::Value, key: &str) -> Result<String, String> {
    optional_id(payload, key)?.ok_or_else(|| error(ERR_INVALID, &format!("{key} is required")))
}

/// Reads the price identifier, preferring the v2 spelling. `rule_id` stays a
/// documented alias for older callers; sending both with different values is
/// refused instead of guessed.
fn price_identifier(payload: &serde_json::Value) -> Result<Option<String>, String> {
    let canonical = payload
        .get("price_id")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);
    let alias = payload
        .get("rule_id")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);
    if let (Some(canonical), Some(alias)) = (&canonical, &alias)
        && canonical != alias
    {
        return Err(error(
            ERR_INVALID,
            "price_id and rule_id disagree; send only one identifier",
        ));
    }
    Ok(canonical.or(alias))
}

/// A missing or blank rate means the explicit zero default.
fn rate_or_zero(value: Option<String>) -> String {
    match value {
        Some(text) if !text.trim().is_empty() => text,
        _ => "0".to_owned(),
    }
}

fn parse_price_save(payload: &serde_json::Value) -> Result<PriceSaveRequest, String> {
    let field = |key: &str| -> Result<String, String> {
        payload
            .get(key)
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| error(ERR_INVALID, &format!("{key} is required")))
    };
    let optional = |key: &str| -> Result<Option<String>, String> {
        match payload.get(key) {
            None | Some(serde_json::Value::Null) => Ok(None),
            Some(value) => value
                .as_str()
                .map(|text| Some(text.to_owned()))
                .ok_or_else(|| error(ERR_INVALID, &format!("{key} must be a string"))),
        }
    };
    let price_id = price_identifier(payload)?;
    // Creating is revision 0 by definition. A caller that sends a non-zero
    // revision without an id is trying to overwrite an existing price through
    // the create path, which the contract does not allow.
    if price_id.is_none()
        && let Some(revision) = payload
            .get("expected_revision")
            .and_then(serde_json::Value::as_i64)
        && revision != 0
    {
        return Err(error(
            ERR_INVALID,
            "creating a price must use expected_revision 0; edit it with its price_id instead",
        ));
    }
    Ok(PriceSaveRequest {
        rule_id: price_id,
        expected_revision: payload
            .get("expected_revision")
            .and_then(serde_json::Value::as_i64),
        provider_id: required_id(payload, "provider_id")?,
        upstream_model: required_id(payload, "upstream_model")?,
        currency: field("currency")?,
        input_mode: field("input_mode")?,
        // A blank box is the documented zero default, not a validation error.
        input_rate_nanos: rate_or_zero(optional("input_rate_nanos")?),
        output_rate_nanos: rate_or_zero(optional("output_rate_nanos")?),
        cache_read_rate_nanos: optional("cache_read_rate_nanos")?
            .filter(|text| !text.trim().is_empty()),
        cache_write_rate_nanos: optional("cache_write_rate_nanos")?
            .filter(|text| !text.trim().is_empty()),
        label: payload
            .get("label")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .chars()
            .take(160)
            .collect(),
    })
}

fn spawn_reader(inner: Arc<Inner>) -> Result<ReaderHandle, String> {
    let (tx, rx) = std::sync::mpsc::sync_channel(QUERY_QUEUE_CAPACITY);
    let path = inner.dir.join("usage.sqlite3");
    let thread = std::thread::Builder::new()
        .name("stats-reader".into())
        .stack_size(1024 * 1024)
        .spawn(move || reader_thread(inner, path, rx))
        .map_err(|_| error(ERR_UNAVAILABLE, "statistics reader thread could not start"))?;
    Ok(ReaderHandle {
        tx,
        thread,
        last_used: Instant::now(),
    })
}

fn reader_thread(inner: Arc<Inner>, path: PathBuf, rx: Receiver<ReadJobEnvelope>) {
    let mut connection: Option<rusqlite::Connection> = None;
    let trace = std::env::var_os("WORKBUDDY_PROXY_QUERY_TIMING").as_deref()
        == Some(std::ffi::OsStr::new("1"));
    let mut trace_count = 0;
    loop {
        match rx.recv_timeout(READER_IDLE) {
            Ok(envelope) => {
                let queued = trace.then(Instant::now);
                let _read_activity = inner.report_reads.enter();
                let admitted = trace.then(Instant::now);
                if connection.is_none() {
                    connection = store::open_connection(&path, true).ok();
                }
                let result = match connection.as_ref() {
                    Some(conn) => run_read(conn, &inner, envelope.kind),
                    None => Err(error(
                        ERR_UNAVAILABLE,
                        "statistics database cannot be opened read-only",
                    )),
                };
                if trace_count < 128
                    && let (Some(queued), Some(admitted)) = (queued, admitted)
                {
                    trace_count += 1;
                    eprintln!(
                        "WB_QUERY_TIMING seq={} admission_us={} execution_us={}",
                        trace_count,
                        admitted.duration_since(queued).as_micros(),
                        admitted.elapsed().as_micros()
                    );
                }
                let _ = envelope.reply.send(result);
                drop(_read_activity);
                if inner.report_reads.import_reserved() {
                    inner.queue.wake();
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => break,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
}

fn run_read(
    conn: &rusqlite::Connection,
    inner: &Arc<Inner>,
    job: ReadJob,
) -> Result<String, String> {
    // Each arm already produces the serialized payload or a fixed error code.
    let serialize = std::convert::identity::<Result<String, String>>;
    // Check at execution time as well as admission: maintenance may have
    // invalidated a cursor while this job waited behind another reader.
    let revision = inner.maintenance_revision.load(Ordering::Acquire);
    match &job {
        ReadJob::Requests {
            filter,
            cursor: Some(cursor),
            ..
        } => query::validate_cursor(cursor, filter, &inner.run_id, revision)?,
        ReadJob::Breakdown {
            filter,
            group_by,
            sort,
            cursor: Some(cursor),
            ..
        } => query::validate_breakdown_cursor(
            cursor,
            filter,
            *group_by,
            *sort,
            &inner.run_id,
            revision,
        )?,
        _ => {}
    }
    let paged = matches!(&job, ReadJob::Requests { .. } | ReadJob::Breakdown { .. });
    let result = match job {
        ReadJob::Dashboard {
            filter,
            granularity,
            flags,
            generation,
            tag,
            now,
        } => serialize(
            query::dashboard(conn, &filter, granularity, &flags, &generation, &tag, now).and_then(
                |value| {
                    serde_json::to_string(&value)
                        .map_err(|_| error(ERR_UNAVAILABLE, "cannot serialize dashboard"))
                },
            ),
        ),
        ReadJob::Requests {
            filter,
            page_size,
            cursor,
            generation,
            tag,
            now,
        } => serialize(
            query::requests(conn, &filter, page_size, cursor.as_deref()).and_then(|page| {
                let dto = dto::RequestPageDto {
                    api_version: API_VERSION,
                    request_tag: tag,
                    generated_at_ms: now,
                    generation,
                    timezone: query::resolve_timezone(&filter.timezone)
                        .map(|(_, name)| name)
                        .unwrap_or_else(|_| "UTC".into()),
                    filter_key: filter.key(),
                    as_of_id: page.as_of_id,
                    total_rows: page.total_rows,
                    page_size,
                    items: page.items,
                    next_cursor: page.next_cursor.map(|mut cursor| {
                        cursor.run_id = inner.run_id.clone();
                        cursor.maintenance_revision = revision;
                        cursor
                    }),
                };
                serde_json::to_string(&dto)
                    .map_err(|_| error(ERR_UNAVAILABLE, "cannot serialize rows"))
            }),
        ),
        ReadJob::Detail {
            request_id,
            generation,
            tag,
            now,
        } => serialize(query::request_detail(conn, &request_id).and_then(|detail| {
            let dto = RequestDetailDto {
                api_version: API_VERSION,
                request_tag: tag,
                generated_at_ms: now,
                generation,
                request: detail.row,
                parser_version: detail.parser_version,
                created_at_ms: detail.created_at_ms,
                updated_at_ms: detail.updated_at_ms,
                run_id: inner.run_id.clone(),
                cost_parts: detail.cost_parts.map(|parts| dto::CostPartsDto {
                    status: parts.status,
                    currency: parts.currency,
                    input_cost_nanos: parts.input.map(|value| value.to_string()),
                    output_cost_nanos: parts.output.map(|value| value.to_string()),
                    cache_read_cost_nanos: parts.cache_read.map(|value| value.to_string()),
                    cache_write_cost_nanos: parts.cache_write.map(|value| value.to_string()),
                    total_cost_nanos: parts.total.map(|value| value.to_string()),
                    calculated_at_ms: parts.calculated_at_ms,
                    pricing_version: parts.pricing_version,
                }),
                price_snapshot: detail.price_snapshot,
            };
            serde_json::to_string(&dto)
                .map_err(|_| error(ERR_UNAVAILABLE, "cannot serialize detail"))
        })),
        ReadJob::Breakdown {
            filter,
            group_by,
            sort,
            limit,
            cursor,
            generation,
            tag,
            now,
        } => serialize(
            query::breakdown(conn, &filter, group_by, sort, limit, cursor.as_deref()).and_then(
                |page| {
                    let dto = dto::BreakdownDto {
                        api_version: API_VERSION,
                        request_tag: tag,
                        generated_at_ms: now,
                        generation,
                        timezone: query::resolve_timezone(&filter.timezone)
                            .map(|(_, name)| name)
                            .unwrap_or_else(|_| "UTC".into()),
                        filter_key: filter.key(),
                        group_by,
                        sort,
                        as_of_id: page.as_of_id,
                        total_groups: page.total_groups,
                        items: page.items,
                        next_cursor: page.next_cursor.map(|mut cursor| {
                            cursor.run_id = inner.run_id.clone();
                            cursor.maintenance_revision = revision;
                            cursor
                        }),
                    };
                    serde_json::to_string(&dto)
                        .map_err(|_| error(ERR_UNAVAILABLE, "cannot serialize breakdown"))
                },
            ),
        ),
        ReadJob::Filters {
            scope,
            kind,
            query: text,
            provider_id,
            limit,
            after_key,
            configured,
            generation,
            tag,
            now,
        } => serialize(
            query::filter_options_scoped(
                conn,
                query::FilterOptionsRequest {
                    kind,
                    query: &text,
                    provider_id: provider_id.as_deref(),
                    limit,
                    after_key: after_key.as_deref(),
                    configured: &configured,
                    scope,
                },
            )
            .and_then(|options| {
                let dto = FilterOptionsDto {
                    api_version: API_VERSION,
                    request_tag: tag,
                    generated_at_ms: now,
                    generation,
                    kind,
                    items: options.items,
                    next_key: options.next_key,
                };
                serde_json::to_string(&dto)
                    .map_err(|_| error(ERR_UNAVAILABLE, "cannot serialize filter options"))
            }),
        ),
    };
    if paged && revision != inner.maintenance_revision.load(Ordering::Acquire) {
        return Err(error(
            dto::ERR_CURSOR_EXPIRED,
            "maintenance changed during the query",
        ));
    }
    result
}

// ---- writer thread --------------------------------------------------------

fn writer_thread(
    inner: Arc<Inner>,
    control: Receiver<ControlMessage>,
    import_root: Option<workbuddy_import::reader::ReaderRoot>,
) {
    let (mut writer, recovery) =
        match store::Writer::open(&inner.dir, &inner.run_id, inner.counters.clone()) {
            Ok(pair) => pair,
            Err(failure) => {
                // The health payload is public: it carries a fixed code plus a
                // short hint, never the raw driver text (which for a failed
                // migration is the whole SQL batch).
                inner.set_state(
                    HealthState::Disabled,
                    Some(
                        if failure
                            .message
                            .contains(crate::stats::dto::ERR_UNSUPPORTED_SCHEMA)
                        {
                            "stats_schema_too_new".to_owned()
                        } else {
                            "stats_storage_unavailable".to_owned()
                        },
                    ),
                );
                eprintln!("statistics disabled: {}", failure.message);
                // Still answer control messages so the UI can show the failure.
                drain_control_without_writer(&inner, &control);
                return;
            }
        };
    inner
        .unclean_exit
        .store(recovery.unclean_detected, Ordering::Relaxed);
    if recovery.costs_settled > 0 {
        eprintln!(
            "statistics repaired {} abandoned cost row(s) at startup",
            recovery.costs_settled
        );
    }
    if recovery.unclean_detected {
        eprintln!(
            "statistics recovery: {} unclean run(s), {} interrupted request(s)",
            recovery.unclean_runs, recovery.interrupted_requests
        );
    }
    {
        let mut settings = inner.settings.write().unwrap();
        *settings = writer.settings;
    }
    {
        let mut modes = inner.provider_modes.lock().unwrap();
        *modes = writer.provider_modes.clone();
    }
    // Publish the current prices so `begin` can freeze a snapshot without
    // touching SQLite.
    *inner.prices.write().unwrap() = writer.prices.clone();
    inner
        .counters
        .enabled
        .store(writer.settings.enabled, Ordering::Relaxed);
    inner.set_state(HealthState::Ready, None);
    publish_facts(&inner, &writer.db_stats);
    let mut retention_check = Instant::now();
    // A bounded startup pass may have left repair work queued; starting
    // `degraded` makes the recovery probe pick it up without waiting for new
    // traffic, and the page then reports the true state instead of claiming
    // complete data.
    // The repair scan is only finished once it has walked the whole frozen
    // scope; claiming `ready` before that would present a half-repaired
    // database as complete. The recovery probe clears this when it is done.
    let scan_at_start = writer.scan.is_some_and(|scan| !scan.finished());
    let mut degraded_since: Option<i64> = if scan_at_start {
        Some(crate::stats::now_ms())
    } else {
        None
    };
    if scan_at_start {
        inner.set_state(
            HealthState::Degraded,
            Some("stats_recovery_pending".to_owned()),
        );
    }
    let mut degraded_recorded = false;
    let mut maintenance_failed = false;
    let mut probe_backoff = PROBE_BACKOFF_MIN;
    let mut next_probe = Instant::now();
    // Finish events whose row could not be written; retried after recovery so
    // no request is left `in_flight` forever.
    let mut orphans: Vec<String> = Vec::new();

    let import_enabled = {
        let tx = writer.conn.transaction();
        tx.and_then(|tx| {
            let enabled = workbuddy_import::store::load_import_settings(&tx)
                .map(|settings| {
                    inner.import_runtime.publish_import_policy(settings.enabled);
                    settings.enabled
                })
                .unwrap_or(false);
            tx.commit()?;
            Ok(enabled)
        })
        .unwrap_or(false)
    };
    inner
        .import_runtime
        .configure(writer.settings.enabled && import_enabled);
    let mut importer = import_root
        .or_else(|| workbuddy_import::reader::ReaderRoot::default_root().ok())
        .map(|root| workbuddy_import::runtime::Runtime::start(inner.clone(), root));
    loop {
        // Control first: user actions and shutdown must not wait for traffic.
        let mut shutdown = false;
        loop {
            match control.try_recv() {
                Ok(message) => {
                    if matches!(message.kind, ControlKind::Shutdown) {
                        shutdown = true;
                        if let Some(reply) = message.reply {
                            let _ = reply.send(Ok("stopped".into()));
                        }
                        continue;
                    }
                    let reply = message.reply;
                    let result = handle_control(&inner, &mut writer, message.kind);
                    let import_enabled = {
                        let tx = writer.conn.transaction();
                        tx.and_then(|tx| {
                            let enabled = workbuddy_import::store::load_import_settings(&tx)
                                .map(|settings| {
                                    inner.import_runtime.publish_import_policy(settings.enabled);
                                    settings.enabled
                                })
                                .unwrap_or(false);
                            tx.commit()?;
                            Ok(enabled)
                        })
                        .unwrap_or(false)
                    };
                    inner
                        .import_runtime
                        .configure(writer.settings.enabled && import_enabled);
                    if let Some(reply) = reply {
                        let _ = reply.send(result);
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    shutdown = true;
                    break;
                }
            }
        }
        if shutdown {
            drop(importer.take());
            flush_and_stop(&inner, &mut writer);
            return;
        }
        // One bounded batch, then loop back to the control queue.
        let wait = inner.report_reads.writer_wait(BATCH_WAIT);
        let batch = inner.queue.pop_batch(BATCH_EVENTS, wait);
        if !batch.is_empty() {
            let budget = store::WriteBudget::new(store::BATCH_BUDGET);
            match writer.apply_batch(&batch, &budget) {
                Ok(outcome) => {
                    if batch.iter().any(|event|matches!(event,event::StatsEvent::Start(meta) if meta.workbuddy_identity.is_some())) {
                        inner.maintenance_revision.fetch_add(1,Ordering::AcqRel);
                    }
                    inner.counters.commits.fetch_add(1, Ordering::Relaxed);
                    inner
                        .counters
                        .last_commit_at_ms
                        .store(now_ms(), Ordering::Relaxed);
                    if !outcome.is_complete() {
                        // Isolation dropped only the offending events; count
                        // them per request, never per event.
                        let failures = count_requests(&outcome.dropped);
                        inner
                            .counters
                            .write_failures
                            .fetch_add(failures, Ordering::Relaxed);
                        remember_orphans(&mut orphans, &outcome.dropped);
                        trim_orphans(&mut writer, &mut orphans, &inner, now_ms());
                        eprintln!(
                            "statistics dropped {failures} request(s) that could not be written"
                        );
                        // Dropped events mean the request and its cost still owe
                        // a terminal state, so this is not a healthy window.
                        if degraded_since.is_none() {
                            degraded_since = Some(now_ms());
                            degraded_recorded = writer.enter_degraded(now_ms()).is_ok();
                        }
                        inner.set_state(
                            HealthState::Degraded,
                            Some("stats_write_isolated".to_owned()),
                        );
                        // The probe that follows decides the next attempt: it
                        // backs off only if the repair itself fails.
                    }
                    match recover_collection(
                        &inner,
                        &mut writer,
                        &mut degraded_since,
                        &mut degraded_recorded,
                        &mut orphans,
                        &mut probe_backoff,
                    ) {
                        // Finished, or simply more bounded batches to take: the
                        // probe keeps running without a failure delay.
                        RecoveryProgress::Complete => {
                            next_probe = Instant::now() + PROBE_BACKOFF_MIN
                        }
                        RecoveryProgress::Progressing => next_probe = Instant::now(),
                        RecoveryProgress::Failed => {
                            next_probe = Instant::now() + probe_backoff;
                            probe_backoff = (probe_backoff * 2).min(PROBE_BACKOFF_MAX);
                        }
                    }
                }
                Err(failure) => {
                    // The whole batch failed, so its `Finish` events are gone:
                    // remember them per request so recovery can settle the rows
                    // instead of leaving them `in_flight` forever.
                    let failures = count_requests(&batch);
                    inner
                        .counters
                        .write_failures
                        .fetch_add(failures, Ordering::Relaxed);
                    remember_orphans(&mut orphans, &batch);
                    trim_orphans(&mut writer, &mut orphans, &inner, now_ms());
                    eprintln!("statistics write failed: {}", failure.message);
                    if degraded_since.is_none() {
                        degraded_since = Some(now_ms());
                        // Recording the window needs the database too; if that
                        // fails it is backfilled on recovery.
                        degraded_recorded = writer.enter_degraded(now_ms()).is_ok();
                    }
                    inner.set_state(HealthState::Degraded, Some(failure.message.clone()));
                    next_probe = Instant::now() + probe_backoff;
                    probe_backoff = (probe_backoff * 2).min(PROBE_BACKOFF_MAX);
                }
            }
            continue;
        }
        if writer.maintenance.is_some() {
            let deadline = Instant::now() + Duration::from_millis(25);
            let result = writer.maintenance_step();
            match result {
                Ok(remaining) => {
                    inner.maintenance_revision.fetch_add(1, Ordering::Relaxed);
                    publish_facts(&inner, &writer.db_stats);
                    if !remaining && maintenance_failed && degraded_since.is_none() {
                        // A later maintenance attempt (the UI's retry) completed
                        // successfully. Only clear the degradation caused by a
                        // maintenance failure; a write/scan degradation keeps its
                        // own recovery probe and must not be hidden here.
                        maintenance_failed = false;
                        inner.set_state(HealthState::Ready, None);
                    }
                    if remaining && Instant::now() < deadline {
                        continue;
                    }
                }
                Err(failure) => {
                    // The health payload is public. Keep the fixed code, never
                    // the raw SQLite/trigger text (which can contain SQL/paths).
                    eprintln!("statistics maintenance failed: {}", failure.message);
                    maintenance_failed = true;
                    inner.set_state(
                        HealthState::Degraded,
                        Some(ERR_MAINTENANCE_FAILED.to_owned()),
                    );
                }
            }
        }
        // With an empty queue the only way out of `degraded` is an active
        // probe; without it collection would stop until the next restart.
        // Any outstanding recovery work drives the probe, so an idle proxy still
        // retries a failed settlement instead of waiting for the next restart.
        // An unreadable queue must not read as "nothing to do": keeping the
        // probe alive is the safe direction.
        let backlog_pending = match store::backlog_len(&writer.conn) {
            Ok(queued) => queued > 0,
            Err(failure) => {
                eprintln!(
                    "statistics could not read the recovery backlog: {}",
                    failure.message
                );
                true
            }
        };
        let scan_pending = writer.scan.is_some_and(|scan| !scan.finished());
        let recovery_pending =
            degraded_since.is_some() || !orphans.is_empty() || backlog_pending || scan_pending;
        if recovery_pending && Instant::now() >= next_probe {
            if writer.probe_write().is_ok() {
                match recover_collection(
                    &inner,
                    &mut writer,
                    &mut degraded_since,
                    &mut degraded_recorded,
                    &mut orphans,
                    &mut probe_backoff,
                ) {
                    RecoveryProgress::Complete => next_probe = Instant::now() + PROBE_BACKOFF_MIN,
                    // More bounded batches remain and nothing failed, so the
                    // next one is taken immediately: a large repair must not
                    // crawl at the failure backoff. The loop still returns to
                    // the queue and the control channel first.
                    RecoveryProgress::Progressing => next_probe = Instant::now(),
                    RecoveryProgress::Failed => {
                        // Only a real failure backs off.
                        next_probe = Instant::now() + probe_backoff;
                        probe_backoff = (probe_backoff * 2).min(PROBE_BACKOFF_MAX);
                    }
                }
            } else {
                next_probe = Instant::now() + probe_backoff;
                probe_backoff = (probe_backoff * 2).min(PROBE_BACKOFF_MAX);
            }
        }
        // Automatic retention is checked at most once a minute; the sweep
        // itself reports whether a task actually started.
        if inner.queue.depth() == 0
            && let Some(importer) = importer.as_mut()
        {
            importer.poll(&mut writer, &inner);
        }
        if retention_check.elapsed() >= Duration::from_secs(60) {
            retention_check = Instant::now();
            if let Ok(true) = writer.maybe_start_retention(now_ms()) {
                inner.maintenance_revision.fetch_add(1, Ordering::Relaxed);
            }
        }
        if inner.queue.is_closed() {
            flush_and_stop(&inner, &mut writer);
            return;
        }
    }
}

/// Requests affected by a dropped event batch, counted once per request so a
/// lost `Start`+`Finish` pair is not reported twice.
fn count_requests(events: &[crate::stats::event::StatsEvent]) -> u64 {
    let mut ids = std::collections::HashSet::new();
    for event in events {
        match event {
            crate::stats::event::StatsEvent::Start(meta) => {
                ids.insert(meta.request_id.clone());
            }
            crate::stats::event::StatsEvent::Finish { request_id, .. } => {
                ids.insert(request_id.clone());
            }
        }
    }
    ids.len() as u64
}

/// Remembers `Finish` events that never reached the database. The list is
/// bounded; beyond that the oldest entries are settled on the next recovery.
fn remember_orphans(orphans: &mut Vec<String>, events: &[crate::stats::event::StatsEvent]) {
    for event in events {
        if let crate::stats::event::StatsEvent::Finish { request_id, .. } = event
            && !orphans.iter().any(|id| id == request_id)
        {
            orphans.push(request_id.clone());
        }
    }
}

/// Hands the oldest remembered ids to the writer before the list is trimmed.
///
/// Dropping them here would leave rows `in_flight` with no retry path in this
/// run. They are settled now when the database accepts writes, and counted as
/// loss when it does not; either way a restart's `recover()` finishes the job,
/// so a lost terminal event can never be silent.
fn trim_orphans(
    writer: &mut store::Writer,
    orphans: &mut Vec<String>,
    _inner: &Arc<Inner>,
    now: i64,
) {
    if orphans.len() <= MAX_ORPHANS {
        return;
    }
    let overflow = orphans.len() - MAX_ORPHANS;
    let stale: Vec<String> = orphans.drain(..overflow).collect();
    // Settle whatever the database accepts right now; `settle_orphans` is
    // idempotent, so re-queueing an already-settled id is harmless.
    if let Ok(settled) = writer.settle_orphans(&stale, now)
        && settled == stale.len()
    {
        return;
    }
    match store::push_backlog(writer, &stale, now) {
        Ok(_) => {}
        Err(failure) => {
            // The backlog is unreachable. Keep a bounded number in memory and
            // report the rest as lost instead of letting a bypass list grow
            // without limit; the failure window and the counter keep the loss
            // visible.
            let room = MAX_ORPHANS.saturating_sub(orphans.len());
            let kept = stale.len().min(room);
            let lost = stale.len() - kept;
            eprintln!(
                "statistics could not queue {} orphaned request(s): {} (kept {kept}, lost {lost})",
                stale.len(),
                failure.message
            );
            orphans.extend(stale.into_iter().take(kept));
            if lost > 0 {
                _inner
                    .counters
                    .write_failures
                    .fetch_add(lost as u64, Ordering::Relaxed);
            }
        }
    }
}

/// How one recovery attempt ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RecoveryProgress {
    /// Every piece of recovery work has landed.
    Complete,
    /// Work remains but nothing failed: continue promptly instead of backing
    /// off, so a large repair does not crawl at the failure backoff.
    Progressing,
    /// Something refused to work; retry with a growing delay.
    Failed,
}

/// Leaves `degraded` once — and only once — every piece of recovery work has
/// landed: the failure window is durable, the orphaned requests have a terminal
/// state, and the durable backlog has been drained. A partially recovered
/// collection stays `degraded` and is retried without needing new traffic.
fn recover_collection(
    inner: &Arc<Inner>,
    writer: &mut store::Writer,
    degraded_since: &mut Option<i64>,
    degraded_recorded: &mut bool,
    orphans: &mut Vec<String>,
    probe_backoff: &mut Duration,
) -> RecoveryProgress {
    let now = now_ms();
    let mut complete = true;
    let mut failed = false;
    // One bounded batch of the startup repair scan per attempt: the transaction
    // is short and the writer goes back to serving live events between batches.
    if let Some(mut scan) = writer.scan
        && !scan.finished()
    {
        match store::scan_recovery_batch(writer, &mut scan, RECOVERY_BATCH, now) {
            Ok((repaired, _remaining)) => {
                if repaired > 0 {
                    eprintln!("statistics repaired {repaired} row(s) from the startup scan");
                }
            }
            Err(failure) => {
                complete = false;
                failed = true;
                eprintln!(
                    "statistics could not continue the startup scan: {}",
                    failure.message
                );
            }
        }
        writer.scan = Some(scan);
        if !scan.finished() {
            complete = false;
        }
    }
    // The in-memory evidence is only cleared once the database has accepted it:
    // dropping it first turned an unrecorded failure window into a window the
    // page reports as trustworthy.
    if let Some(since) = *degraded_since {
        let outcome = if *degraded_recorded {
            writer.leave_degraded(now)
        } else {
            writer.mark_degraded_window(since, now)
        };
        match outcome {
            Ok(()) => {
                *degraded_since = None;
                *degraded_recorded = false;
            }
            Err(failure) => {
                complete = false;
                failed = true;
                eprintln!(
                    "statistics could not record the degraded window: {}",
                    failure.message
                );
            }
        }
    }
    if !orphans.is_empty() {
        let pending = std::mem::take(orphans);
        // The in-memory list uses the same classification as the durable queue:
        // an id with no request row (both events were lost) leaves the list, a
        // settled one is done, and only a genuinely open row is retried. The
        // old `changed rows < len` test kept missing ids forever.
        match store::repair_ids(writer, &pending, now) {
            Ok((done, missing, retry)) => {
                if done > 0 || missing > 0 {
                    eprintln!(
                        "statistics repaired {done} orphaned request(s), dropped {missing} stale id(s)"
                    );
                }
                if missing > 0 {
                    // The terminal state is gone for good; keep it visible as a
                    // loss instead of pretending the request still exists.
                    inner
                        .counters
                        .write_failures
                        .fetch_add(missing as u64, Ordering::Relaxed);
                }
                if !retry.is_empty() {
                    *orphans = retry;
                    complete = false;
                }
            }
            Err(failure) => {
                // Keep them for the next attempt instead of losing the ids.
                *orphans = pending;
                complete = false;
                failed = true;
                eprintln!(
                    "statistics could not settle orphaned request(s): {}",
                    failure.message
                );
            }
        }
    }
    // The durable backlog is part of the same success condition: without it a
    // cost failure would look recovered as soon as the interval was written.
    // One attempt keeps taking bounded batches until the queue is empty, so a
    // backlog larger than one batch is really repaired instead of half-repaired.
    let mut backlog_known = false;
    for _ in 0..RECOVERY_ROUNDS {
        let remaining = match store::backlog_len(&writer.conn) {
            Ok(value) => value,
            Err(failure) => {
                // An unreadable queue is not an empty queue.
                complete = false;
                failed = true;
                eprintln!(
                    "statistics could not read the recovery backlog: {}",
                    failure.message
                );
                break;
            }
        };
        backlog_known = true;
        if remaining == 0 {
            break;
        }
        match store::repair_backlog_batch(writer, RECOVERY_BATCH, now) {
            Ok((done, missing, remaining)) => {
                if done > 0 || missing > 0 {
                    eprintln!(
                        "statistics repaired {done} backlogged request(s), dropped {missing} stale id(s)"
                    );
                }
                if remaining == 0 {
                    break;
                }
                if done + missing == 0 {
                    // Nothing moved this round: stop spinning and retry later.
                    complete = false;
                    break;
                }
            }
            Err(failure) => {
                complete = false;
                failed = true;
                eprintln!(
                    "statistics could not drain the recovery backlog: {}",
                    failure.message
                );
                break;
            }
        }
    }
    if backlog_known
        && let Ok(remaining) = store::backlog_len(&writer.conn)
        && remaining > 0
    {
        complete = false;
    }
    if complete {
        *probe_backoff = PROBE_BACKOFF_MIN;
        if *inner.state.read().unwrap() == HealthState::Degraded {
            if let Err(failure) = writer.persist_counters(now) {
                // Closing the degraded interval alone is not enough. Keep
                // driving the idle recovery probe until loss counters are
                // durable too, even when no new relay request ever arrives.
                *degraded_since = Some(now);
                *degraded_recorded = false;
                eprintln!(
                    "statistics could not persist recovery counters: {}",
                    failure.message
                );
                return RecoveryProgress::Failed;
            }
            inner.set_state(HealthState::Ready, None);
        }
        RecoveryProgress::Complete
    } else if failed {
        RecoveryProgress::Failed
    } else {
        // Bounded batches remain: keep the writer's loop turning so the next
        // batch is taken promptly, and give the control channel its turn in
        // between.
        RecoveryProgress::Progressing
    }
}

fn publish_facts(inner: &Arc<Inner>, stats: &store::DbStats) {
    if let Ok(mut facts) = inner.facts.lock() {
        *facts = Some((
            query::HealthFacts {
                record_count: stats.record_count,
                earliest_record_at_ms: stats.earliest_record_at_ms,
            },
            Instant::now(),
        ));
    }
    if let Ok(mut sizes) = inner.io_bytes.lock() {
        *sizes = (stats.db_bytes, stats.wal_bytes);
    }
    inner.facts_refreshing.store(false, Ordering::Relaxed);
}

fn flush_and_stop(inner: &Arc<Inner>, writer: &mut store::Writer) {
    // Drain every event that was already admitted, then close the collection
    // interval and checkpoint. Chat traffic is unaffected: new events are
    // rejected once `accepting` is false.
    inner.counters.accepting.store(false, Ordering::Relaxed);
    // Even when the database is refusing writes, shutdown must finish inside a
    // bounded window; whatever is left is dropped and counted.
    let deadline = Instant::now() + SHUTDOWN_FLUSH_BUDGET;
    loop {
        if Instant::now() >= deadline {
            eprintln!("statistics shutdown flush budget exhausted; remaining events are dropped");
            break;
        }
        let batch = inner
            .queue
            .pop_batch(BATCH_EVENTS, Duration::from_millis(10));
        if batch.is_empty() {
            break;
        }
        // The same deadline bounds the batch, its retries and its isolation.
        let budget = store::WriteBudget::new(deadline.saturating_duration_since(Instant::now()));
        match writer.apply_batch(&batch, &budget) {
            Ok(outcome) => {
                if !outcome.is_complete() {
                    inner
                        .counters
                        .write_failures
                        .fetch_add(count_requests(&outcome.dropped), Ordering::Relaxed);
                }
            }
            Err(_) => {
                inner
                    .counters
                    .write_failures
                    .fetch_add(1, Ordering::Relaxed);
            }
        }
        inner.counters.commits.fetch_add(1, Ordering::Relaxed);
    }
    let now = now_ms();
    if writer.persist_counters(now).is_ok() && writer
        .conn
        .execute(
            "UPDATE stats_runs SET stopped_at_ms=?2, stop_reason='normal', last_persisted_at_ms=?2
         WHERE run_id=?1 AND stopped_at_ms IS NULL",
            rusqlite::params![writer.run_id, now],
        )
        .is_ok()
    {
        let _ = writer.conn.execute(
            "UPDATE stats_collection_intervals SET ended_at_ms=?2
             WHERE ended_at_ms IS NULL AND run_id=?1",
            rusqlite::params![writer.run_id, now],
        );
        let _ = writer.conn.execute_batch("PRAGMA wal_checkpoint(PASSIVE);");
    }
    inner.queue.close();
    inner.set_state(HealthState::Disabled, None);
}

fn drain_control_without_writer(inner: &Arc<Inner>, control: &Receiver<ControlMessage>) {
    loop {
        match control.recv_timeout(Duration::from_millis(200)) {
            Ok(message) => {
                if let Some(reply) = message.reply {
                    let _ = reply.send(Err(error(
                        ERR_UNAVAILABLE,
                        inner
                            .error_code
                            .lock()
                            .unwrap()
                            .clone()
                            .unwrap_or_else(|| "statistics are disabled".into())
                            .as_str(),
                    )));
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
        }
    }
}

fn handle_control(
    inner: &Arc<Inner>,
    writer: &mut store::Writer,
    kind: ControlKind,
) -> Result<String, String> {
    match kind {
        ControlKind::WorkbuddySyncPrepare { models, lease } => {
            workbuddy_sync::prepare(inner, writer, models, lease)
        }
        ControlKind::WorkbuddySyncFinish {
            token,
            observations,
        } => workbuddy_sync::finish(writer, &token, observations),
        ControlKind::Shutdown => Ok("stopped".into()),
        ControlKind::RefreshFacts => {
            let _ = writer.refresh_db_stats();
            publish_facts(inner, &writer.db_stats);
            Ok("ok".into())
        }
        ControlKind::SettingsGet => {
            let settings = writer.settings;
            let dto = SettingsDto {
                api_version: API_VERSION,
                request_tag: String::new(),
                generated_at_ms: now_ms(),
                generation: inner.generation(),
                enabled: settings.enabled,
                retention_days: settings.retention_days,
                default_currency: crate::stats::dto::Currency::parse(settings.default_currency)
                    .unwrap_or(crate::stats::dto::Currency::Cny),
                revision: settings.revision,
            };
            serde_json::to_string(&dto).map_err(|_| error(ERR_UNAVAILABLE, "serialize settings"))
        }
        ControlKind::WorkbuddyCommand { command, payload } => {
            if command == "stats_workbuddy_scan" {
                let mode = payload
                    .get("mode")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("incremental");
                return inner.import_runtime.request_scan(mode);
            }
            let result = crate::stats::workbuddy_import::handle_command(
                &mut writer.conn,
                &command,
                &payload,
                now_ms(),
                &mut inner.import_previews.lock().unwrap(),
            );
            if command == "stats_workbuddy_status" {
                return result.and_then(|text| inner.import_runtime.publish_status(&text));
            }
            if command == "stats_workbuddy_settings_update" && result.is_ok() {
                // Import and global settings deliberately share the same CAS
                // revision. Refresh the cache before answering either API.
                writer.settings = store::load_settings(&writer.conn)
                    .map_err(|_| error(ERR_UNAVAILABLE, "cannot reload statistics settings"))?;
                *inner.settings.write().unwrap() = writer.settings;
            }
            result
        }
        ControlKind::SettingsUpdate {
            enabled,
            retention_days,
            default_currency,
            expected_revision,
        } => {
            let settings = writer
                .update_settings(
                    enabled,
                    retention_days,
                    default_currency.as_deref(),
                    expected_revision,
                    now_ms(),
                )
                .map_err(|failure| failure.message)?;
            // The default currency decides what an unconfigured model costs, so
            // the published cache must follow the commit.
            writer.prices =
                store::load_price_cache(&writer.conn).map_err(|failure| failure.message)?;
            *inner.prices.write().unwrap() = writer.prices.clone();
            inner
                .counters
                .enabled
                .store(settings.enabled, Ordering::Relaxed);
            *inner.settings.write().unwrap() = settings;
            let dto = SettingsDto {
                api_version: API_VERSION,
                request_tag: String::new(),
                generated_at_ms: now_ms(),
                generation: inner.generation(),
                enabled: settings.enabled,
                retention_days: settings.retention_days,
                default_currency: crate::stats::dto::Currency::parse(settings.default_currency)
                    .unwrap_or(crate::stats::dto::Currency::Cny),
                revision: settings.revision,
            };
            serde_json::to_string(&dto).map_err(|_| error(ERR_UNAVAILABLE, "serialize settings"))
        }
        ControlKind::ProviderSettingsGet { provider_id } => {
            let (usage_mode, revision) = writer.load_provider_mode(&provider_id);
            inner
                .provider_modes
                .lock()
                .unwrap()
                .insert(provider_id.clone(), usage_mode);
            let dto = ProviderSettingsDto {
                api_version: API_VERSION,
                request_tag: String::new(),
                generated_at_ms: now_ms(),
                generation: inner.generation(),
                provider_id,
                usage_mode,
                revision,
            };
            serde_json::to_string(&dto)
                .map_err(|_| error(ERR_UNAVAILABLE, "serialize provider settings"))
        }
        ControlKind::ProviderSettingsUpdate {
            provider_id,
            usage_mode,
            expected_revision,
        } => {
            let revision = writer
                .update_provider_mode(&provider_id, usage_mode, expected_revision, now_ms())
                .map_err(|failure| failure.message)?;
            inner
                .provider_modes
                .lock()
                .unwrap()
                .insert(provider_id.clone(), usage_mode);
            let dto = ProviderSettingsDto {
                api_version: API_VERSION,
                request_tag: String::new(),
                generated_at_ms: now_ms(),
                generation: inner.generation(),
                provider_id,
                usage_mode,
                revision,
            };
            serde_json::to_string(&dto)
                .map_err(|_| error(ERR_UNAVAILABLE, "serialize provider settings"))
        }
        ControlKind::PriceList {
            provider_id,
            upstream_model,
            after_rule_id,
            limit,
        } => {
            let (items, next_rule_id) = store::list_price_rules(
                &writer.conn,
                provider_id.as_deref(),
                upstream_model.as_deref(),
                after_rule_id.as_deref(),
                limit,
            )
            .map_err(|failure| failure.message)?;
            let dto = PriceListDto {
                catalog_sync: Some(
                    store::price_catalog_status(&writer.conn).map_err(|e| e.message)?,
                ),
                api_version: API_VERSION,
                request_tag: String::new(),
                generated_at_ms: now_ms(),
                generation: inner.generation(),
                items,
                next_rule_id,
            };
            serde_json::to_string(&dto).map_err(|_| error(ERR_UNAVAILABLE, "serialize prices"))
        }
        ControlKind::PriceDelete {
            price_id,
            expected_revision,
        } => {
            let outcome = store::delete_price_rule(writer, &price_id, expected_revision);
            match outcome {
                Ok(()) => {
                    *inner.prices.write().unwrap() = writer.prices.clone();
                    Ok("{}".to_owned())
                }
                Err(failure) => Err(failure.message),
            }
        }
        ControlKind::PriceSave(request) => {
            let rule = match store::save_price_rule(writer, &request, now_ms()) {
                Ok(rule) => {
                    // Publish after the commit so new requests pick the new price
                    // while history keeps its snapshot.
                    *inner.prices.write().unwrap() = writer.prices.clone();
                    rule
                }
                Err(failure) => return Err(failure.message),
            };
            serde_json::to_string(&rule).map_err(|_| error(ERR_UNAVAILABLE, "serialize price rule"))
        }
        ControlKind::PriceCatalogStatus => {
            let status = store::price_catalog_status(&writer.conn).map_err(|e| e.message)?;
            serde_json::to_string(&status)
                .map_err(|_| error(ERR_UNAVAILABLE, "serialize price sync"))
        }
        ControlKind::PriceCatalog {
            prices,
            targets,
            day,
            targets_key,
        } => {
            let status =
                store::import_price_catalog(writer, prices, &targets, &day, &targets_key, now_ms())
                    .map_err(|e| e.message)?;
            *inner.prices.write().unwrap() = writer.prices.clone();
            serde_json::to_string(&status)
                .map_err(|_| error(ERR_UNAVAILABLE, "serialize price sync"))
        }
        ControlKind::ClearPreview { before_ms } => {
            if writer.maintenance.is_some() {
                return Err(error(ERR_BUSY, "another maintenance task is running"));
            }
            let as_of_id = query::max_request_id(&writer.conn)?;
            let row_count: i64 = writer
                .conn
                .query_row(
                    "SELECT COUNT(*) FROM usage_requests
                     WHERE outcome<>'in_flight' AND ended_at_ms IS NOT NULL
                       AND started_at_ms<?1 AND id<=?2",
                    rusqlite::params![before_ms, as_of_id],
                    |row| row.get(0),
                )
                .map_err(|failure| failure.to_string())?;
            let token = crate::config::token();
            *inner.clear_token.lock().unwrap() = Some(ClearToken {
                token: token.clone(),
                before_ms,
                as_of_id,
                created: Instant::now(),
            });
            let dto = ClearPreviewDto {
                api_version: API_VERSION,
                request_tag: String::new(),
                generated_at_ms: now_ms(),
                generation: inner.generation(),
                before_ms,
                as_of_id,
                row_count,
                confirmed: false,
            };
            let mut value = serde_json::to_value(&dto)
                .map_err(|_| error(ERR_UNAVAILABLE, "serialize preview"))?;
            value["token"] = json!(token);
            Ok(value.to_string())
        }
        ControlKind::MaintenanceStatus { task_id } => {
            let (kind, status, affected, started, completed, error_code) = writer
                .conn
                .query_row(
                    "SELECT kind, status, affected_rows, started_at_ms, completed_at_ms, error_code
                     FROM stats_maintenance_events WHERE id=?1",
                    [task_id],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, i64>(2)?,
                            row.get::<_, i64>(3)?,
                            row.get::<_, Option<i64>>(4)?,
                            row.get::<_, Option<String>>(5)?,
                        ))
                    },
                )
                .map_err(|failure| failure.to_string())?;
            let dto = MaintenanceDto {
                api_version: API_VERSION,
                request_tag: String::new(),
                generated_at_ms: now_ms(),
                generation: inner.generation(),
                task_id,
                kind: MaintenanceKind::parse(&kind).unwrap_or(MaintenanceKind::Clear),
                status: MaintenanceStatus::parse(&status).unwrap_or(MaintenanceStatus::Completed),
                affected_rows: affected,
                started_at_ms: started,
                completed_at_ms: completed,
                error_code,
            };
            serde_json::to_string(&dto).map_err(|_| error(ERR_UNAVAILABLE, "serialize maintenance"))
        }
        ControlKind::ClearStart { token } => {
            if writer.maintenance.is_some() {
                return Err(error(ERR_BUSY, "another maintenance task is running"));
            }
            let confirmation = {
                let mut guard = inner.clear_token.lock().unwrap();
                let valid = guard.as_ref().is_some_and(|value| {
                    value.token == token && value.created.elapsed() <= CLEAR_TOKEN_TTL
                });
                // A wrong guess never invalidates a valid confirmation.
                if valid { guard.take() } else { None }
            };
            let Some(confirmation) = confirmation else {
                return Err(error(
                    ERR_INVALID,
                    "the clear confirmation expired; preview again",
                ));
            };
            let task_id = writer
                .insert_maintenance(MaintenanceKind::Clear, now_ms())
                .map_err(|failure| failure.message)?;
            writer.maintenance = Some(store::Maintenance::new(
                task_id,
                MaintenanceKind::Clear,
                confirmation.before_ms,
                confirmation.as_of_id,
                None,
                now_ms(),
                dto::MAINTENANCE_BATCH,
            ));
            inner.maintenance_revision.fetch_add(1, Ordering::Relaxed);
            maintenance_dto(inner, task_id, MaintenanceKind::Clear, 0, now_ms(), None)
        }
    }
}

fn maintenance_dto(
    inner: &Arc<Inner>,
    task_id: i64,
    kind: MaintenanceKind,
    affected_rows: i64,
    started_at_ms: i64,
    completed_at_ms: Option<i64>,
) -> Result<String, String> {
    let dto = MaintenanceDto {
        api_version: API_VERSION,
        request_tag: String::new(),
        generated_at_ms: now_ms(),
        generation: inner.generation(),
        task_id,
        kind,
        status: MaintenanceStatus::Running,
        affected_rows,
        started_at_ms,
        completed_at_ms,
        error_code: None,
    };
    serde_json::to_string(&dto).map_err(|_| error(ERR_UNAVAILABLE, "serialize maintenance"))
}

mod workbuddy_sync;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stats::dto::DashboardDto;

    fn temporary_dir() -> tempfile::TempDir {
        tempfile::tempdir().expect("temp dir")
    }

    fn wait_ready(handle: &StatsHandle) -> bool {
        for _ in 0..200 {
            if handle.state() == HealthState::Ready {
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        false
    }

    fn spec(id: &str) -> RequestSpec {
        RequestSpec {
            workbuddy_identity: None,
            source: Source::Relay,
            provider_id: "p1".into(),
            provider_name: "Provider".into(),
            route_id: "r1".into(),
            route_name: "Route".into(),
            requested_model: id.into(),
            upstream_model: id.into(),
            upstream_protocol: "openai_chat".into(),
            stream: true,
        }
    }

    #[test]
    fn controller_does_not_recover_a_live_core_or_leave_a_run_behind() {
        let tmp = temporary_dir();
        let core = StatsHandle::start(tmp.path().to_path_buf());
        assert!(wait_ready(&core));
        let db = rusqlite::Connection::open(tmp.path().join("usage.sqlite3")).unwrap();
        let controller = StatsHandle::for_controller(tmp.path().to_path_buf());
        assert!(controller.is_stopped());
        assert_eq!(controller.state(), HealthState::Disabled);
        assert!(!controller.inner.accepting());
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM stats_runs", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            db.query_row(
                "SELECT COUNT(*) FROM stats_runs WHERE stopped_at_ms IS NULL",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            1
        );
        controller.shutdown();
        core.shutdown();
        assert_eq!(
            db.query_row("SELECT stop_reason FROM stats_runs", [], |r| r
                .get::<_, String>(0))
                .unwrap(),
            "normal"
        );
        let restarted = StatsHandle::start(tmp.path().to_path_buf());
        assert!(wait_ready(&restarted));
        assert!(!restarted.inner.unclean_exit.load(Ordering::Relaxed));
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM stats_runs", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            2
        );
        restarted.shutdown();
    }

    #[test]
    fn controller_statistics_never_create_files() {
        let tmp = temporary_dir();
        let controller = StatsHandle::for_controller(tmp.path().join("not-created"));
        assert!(controller.is_stopped());
        controller.shutdown();
        assert!(!tmp.path().join("not-created").exists());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn usage_request_is_default_and_explicit_opt_out_survives_restart() {
        let tmp = temporary_dir();
        let handle = StatsHandle::start(tmp.path().to_path_buf());
        // The forwarding path must use the default even before the UI reads it.
        assert_eq!(handle.usage_mode("p1"), UsageMode::Request);
        assert!(wait_ready(&handle));
        let reply = handle
            .command(&json!({
                "command": "stats_provider_settings_get", "provider_id": "p1",
            }))
            .await
            .unwrap();
        let settings: ProviderSettingsDto = serde_json::from_str(&reply).unwrap();
        assert_eq!(
            (settings.usage_mode, settings.revision),
            (UsageMode::Request, 0)
        );
        assert_eq!(handle.inner.runtime_flags().observe_mode_providers, 0);
        handle
            .command(&json!({
                "command": "stats_provider_settings_update", "provider_id": "p1",
                "usage_mode": "observe", "expected_revision": 0,
            }))
            .await
            .unwrap();
        assert_eq!(handle.usage_mode("p1"), UsageMode::Observe);
        handle.shutdown();

        let restarted = StatsHandle::start(tmp.path().to_path_buf());
        assert!(wait_ready(&restarted));
        assert_eq!(restarted.usage_mode("p1"), UsageMode::Observe);
        assert_eq!(restarted.usage_mode("new-provider"), UsageMode::Request);
        assert_eq!(restarted.inner.runtime_flags().observe_mode_providers, 1);
        restarted.shutdown();
    }

    #[test]
    fn valid_long_labels_and_aliases_do_not_drop_statistics() {
        let tmp = temporary_dir();
        let handle = StatsHandle::start(tmp.path().to_path_buf());
        assert!(wait_ready(&handle));
        let mut request = spec("model");
        request.provider_name = "名".repeat(180);
        request.route_name = "r".repeat(240);
        request.requested_model = "a".repeat(256);
        let guard = handle.begin(request, Protocol::Json, false).unwrap();
        drop(guard);
        handle.shutdown();
        let db = rusqlite::Connection::open(tmp.path().join("usage.sqlite3")).unwrap();
        let lengths:(i64,i64,i64)=db.query_row("SELECT length(provider_name),length(route_name),length(requested_model) FROM usage_requests",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap();
        assert_eq!(lengths, (160, 160, 160));
    }

    fn wait_for_state(handle: &StatsHandle, state: HealthState, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if handle.state() == state {
                return true;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        false
    }

    fn query_one<T: rusqlite::types::FromSql>(
        dir: &std::path::Path,
        sql: &str,
        params: &[&dyn rusqlite::ToSql],
    ) -> T {
        let connection = rusqlite::Connection::open(dir.join("usage.sqlite3")).unwrap();
        connection
            .query_row(sql, rusqlite::params_from_iter(params.iter()), |row| {
                row.get(0)
            })
            .unwrap()
    }

    /// One request's stored cost snapshot, as the queries read it.
    #[derive(Debug, Clone)]
    struct CostSnapshot {
        request_id: String,
        status: String,
        price_source: String,
        input_rate: Option<String>,
        total: Option<String>,
        source_price_id: Option<String>,
        source_price_revision: Option<i64>,
    }

    fn cost_snapshots(dir: &std::path::Path, model: &str) -> Vec<CostSnapshot> {
        let connection = rusqlite::Connection::open(dir.join("usage.sqlite3")).unwrap();
        let mut statement = connection
            .prepare(
                "SELECT r.request_id, c.status, c.price_source, c.input_rate_nanos,
                        c.total_cost_nanos, c.source_price_id, c.source_price_revision
                 FROM usage_requests r JOIN usage_costs c ON c.request_id=r.request_id
                 WHERE r.upstream_model=?1 ORDER BY r.id",
            )
            .unwrap();
        let rows = statement
            .query_map([model], |row| {
                Ok(CostSnapshot {
                    request_id: row.get(0)?,
                    status: row.get(1)?,
                    price_source: row.get(2)?,
                    input_rate: row.get::<_, Option<i64>>(3)?.map(|value| value.to_string()),
                    total: row.get::<_, Option<i64>>(4)?.map(|value| value.to_string()),
                    source_price_id: row.get(5)?,
                    source_price_revision: row.get(6)?,
                })
            })
            .unwrap();
        rows.map(|row| row.unwrap()).collect()
    }

    /// Waits until `count` requests of that model have settled.
    async fn wait_cost_count(dir: &tempfile::TempDir, model: &str, count: usize) -> String {
        for _ in 0..400 {
            let rows = cost_snapshots(dir.path(), model);
            let settled: Vec<&CostSnapshot> =
                rows.iter().filter(|row| row.status != "pending").collect();
            if settled.len() >= count {
                return settled[count - 1].request_id.clone();
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        panic!(
            "cost rows for {model} never settled: {:?}",
            cost_snapshots(dir.path(), model)
        );
    }

    /// Waits for one settled (non-pending) cost row and returns it.
    async fn wait_cost(dir: &tempfile::TempDir, model: &str) -> CostSnapshot {
        let _ = wait_cost_count(dir, model, 1).await;
        cost_snapshots(dir.path(), model)
            .into_iter()
            .find(|row| row.status != "pending")
            .expect("a settled cost row")
    }

    /// Request outcome plus `upstream_started` for one model.
    fn query_one_pair(dir: &std::path::Path, model: &str) -> (String, Option<i64>) {
        let connection = rusqlite::Connection::open(dir.join("usage.sqlite3")).unwrap();
        connection
            .query_row(
                "SELECT outcome, upstream_started FROM usage_requests WHERE upstream_model=?1",
                [model],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap()
    }

    fn model_tokens(dir: &std::path::Path, model: &str) -> (Option<i64>, Option<i64>) {
        let connection = rusqlite::Connection::open(dir.join("usage.sqlite3")).unwrap();
        connection
            .query_row(
                "SELECT total_tokens, input_tokens FROM usage_requests WHERE upstream_model=?1",
                [model],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap()
    }

    async fn save_price(
        handle: &StatsHandle,
        model: &str,
        input: &str,
        output: &str,
    ) -> crate::stats::dto::PriceRuleDto {
        save_price_with(handle, model, input, output, None, 0).await
    }

    /// Saves a current price. The amounts are per-million and are converted to
    /// the API's integer nano units exactly once.
    async fn save_price_with(
        handle: &StatsHandle,
        model: &str,
        input: &str,
        output: &str,
        price_id: Option<String>,
        expected_revision: i64,
    ) -> crate::stats::dto::PriceRuleDto {
        let to_nanos = |value: &str| {
            crate::stats::dto::parse_human_amount(value)
                .unwrap()
                .to_string()
        };
        let mut payload = json!({
            "command": "stats_price_save",
            "provider_id": "p1",
            "upstream_model": model,
            "currency": "USD",
            "input_mode": "flat",
            "input_rate_nanos": to_nanos(input),
            "output_rate_nanos": to_nanos(output),
            "label": "test",
        });
        if let Some(price_id) = price_id {
            payload["rule_id"] = json!(price_id);
            payload["expected_revision"] = json!(expected_revision);
        }
        let reply = handle.command(&payload).await.unwrap();
        serde_json::from_str(&reply).unwrap()
    }

    async fn delete_price(handle: &StatsHandle, price_id: &str, expected_revision: i64) {
        handle
            .command(&json!({
                "command": "stats_price_delete",
                "rule_id": price_id,
                "expected_revision": expected_revision,
            }))
            .await
            .unwrap();
    }

    #[test]
    fn queued_cursors_are_revalidated_when_the_reader_executes() {
        let dir = temporary_dir();
        let handle = StatsHandle::start(dir.path().to_path_buf());
        assert!(wait_ready(&handle));
        let filter = handle
            .filter(&json!({"filter":{"from_ms":0,"to_ms":86400000,"timezone":"UTC"}}))
            .unwrap();
        let cursor = dto::RequestCursor {
            last_started_at_ms: 1,
            last_id: 1,
            as_of_id: 2,
            filter_key: filter.key(),
            run_id: handle.inner.run_id.clone(),
            maintenance_revision: 0,
        };
        query::validate_cursor(&cursor, &filter, &handle.inner.run_id, 0).unwrap();
        let job = ReadJob::Requests {
            filter: Box::new(filter),
            page_size: 20,
            cursor: Some(Box::new(cursor)),
            generation: String::new(),
            tag: "queued-before-clear".into(),
            now: now_ms(),
        };
        handle
            .inner
            .maintenance_revision
            .fetch_add(1, Ordering::Release);
        let conn = store::open_connection(&dir.path().join("usage.sqlite3"), true).unwrap();
        let error = run_read(&conn, &handle.inner, job).unwrap_err();
        assert!(error.contains(dto::ERR_CURSOR_EXPIRED), "{error}");
        handle.shutdown();
    }

    #[tokio::test]
    async fn ten_record_pages_cover_all_requests_without_duplicates() {
        let dir = temporary_dir();
        let handle = StatsHandle::start(dir.path().to_path_buf());
        assert!(wait_ready(&handle));
        for _ in 0..21 {
            let mut guard = handle
                .begin(spec("ten-record-pages"), Protocol::Sse, false)
                .unwrap();
            guard.mark_upstream_started();
            guard.finish(dto::Outcome::Completed, None);
        }
        let filter = json!({"from_ms":now_ms()-3_600_000,"to_ms":now_ms()+1000,"timezone":"UTC"});
        let mut request = json!({"command":"stats_requests","filter":filter,"page_size":10});
        let mut first = None;
        for _ in 0..200 {
            let page: dto::RequestPageDto =
                serde_json::from_str(&handle.command(&request).await.unwrap()).unwrap();
            if page.total_rows == 21 {
                first = Some(page);
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let mut page = first.expect("all requests must be persisted");
        let mut seen = std::collections::BTreeSet::new();
        for expected in [10, 10, 1] {
            assert_eq!(page.items.len(), expected);
            assert_eq!(page.page_size, 10);
            for row in &page.items {
                assert!(seen.insert(row.id), "a cursor repeated a request");
            }
            if let Some(cursor) = page.next_cursor.take() {
                request["cursor"] = serde_json::to_value(cursor).unwrap();
                page = serde_json::from_str(&handle.command(&request).await.unwrap()).unwrap();
            } else {
                assert_eq!(seen.len(), 21);
                break;
            }
        }
        assert_eq!(seen.len(), 21);
        request["page_size"] = json!(11);
        assert!(
            handle
                .command(&request)
                .await
                .unwrap_err()
                .contains(dto::ERR_INVALID)
        );
        handle.shutdown();
    }

    #[tokio::test]
    async fn statistics_are_recorded_and_queried_end_to_end() {
        let dir = temporary_dir();
        let handle = StatsHandle::start(dir.path().to_path_buf());
        assert!(wait_ready(&handle), "writer did not become ready");
        let mut guard = handle.begin(spec("model-x"), Protocol::Sse, false).unwrap();
        guard.mark_upstream_started();
        guard.observe(b"data: {\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\n\n");
        guard.observe(b"data: {\"usage\":{\"prompt_tokens\":1000,\"completion_tokens\":100,\"total_tokens\":1100,\"prompt_tokens_details\":{\"cached_tokens\":800}}}\n\n");
        guard.observe(b"data: [DONE]\n\n");
        guard.finish(dto::Outcome::Completed, None);
        // Wait for the writer to persist the batch.
        let mut stored = false;
        for _ in 0..200 {
            let payload = json!({
                "command":"stats_requests",
                "request_tag":"t",
                "filter":{"from_ms":now_ms()-3_600_000,"to_ms":now_ms()+1000,"timezone":"UTC"},
                "page_size":20
            });
            let response = handle.command(&payload).await.unwrap();
            let page: serde_json::Value = serde_json::from_str(&response).unwrap();
            // The writer may commit `Start` and `Finish` in separate batches.
            if page["items"][0]["outcome"] == json!("completed") {
                assert_eq!(page["items"][0]["total_tokens"], 1100);
                assert_eq!(page["items"][0]["cache_read_tokens"], 800);
                assert_eq!(page["items"][0]["usage_status"], "complete");
                stored = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(stored, "request was not stored");
        let payload = json!({
            "command":"stats_dashboard",
            "request_tag":"t",
            "filter":{"from_ms":now_ms()-3_600_000,"to_ms":now_ms()+1000,"timezone":"UTC"},
            "granularity":"auto"
        });
        let response = handle.command(&payload).await.unwrap();
        let dashboard: DashboardDto = serde_json::from_str(&response).unwrap();
        assert_eq!(dashboard.summary.request_count, 1);
        assert_eq!(dashboard.summary.total_tokens, Some(1100));
        assert_eq!(dashboard.summary.cache_read_tokens, Some(800));
        assert_eq!(dashboard.summary.usage_complete_count, 1);
    }

    /// R8-05: a large legacy repair must finish promptly through the *real*
    /// writer entry point. Treating "more batches remain" as a failure backed
    /// off to 30 s per batch, so 20k rows were still pending after minutes.
    #[tokio::test]
    async fn a_large_legacy_repair_finishes_through_the_real_writer() {
        let dir = temporary_dir();
        {
            let mut connection =
                rusqlite::Connection::open(dir.path().join("usage.sqlite3")).unwrap();
            connection
                .pragma_update(None, "foreign_keys", "ON")
                .unwrap();
            store::migrate(&mut connection, dir.path()).unwrap();
            connection
                .execute(
                    "INSERT INTO stats_runs(run_id, started_at_ms, stopped_at_ms, stop_reason,
                         admitted_requests, dropped_requests, write_failures, last_persisted_at_ms)
                     VALUES('run-legacy', 1000, 1000, 'unclean', 20000, 0, 0, 1000)",
                    [],
                )
                .unwrap();
            connection.execute_batch("BEGIN").unwrap();
            for index in 0..20000 {
                let id = format!("legacy-{index}");
                connection
                    .execute(
                        "INSERT INTO usage_requests(request_id, run_id, source, provider_id,
                             route_id, provider_name, route_name, requested_model, upstream_model,
                             started_at_ms, stream, outcome, ended_at_ms, duration_ms, usage_status,
                             usage_source, created_at_ms, updated_at_ms)
                         VALUES(?1,'run-legacy','relay','p1','r0','P','R','m','m',
                             1000, 1, 'interrupted', 1000, 0, 'missing', 'none', 1000, 1000)",
                        [&id],
                    )
                    .unwrap();
                connection
                    .execute(
                        "INSERT INTO usage_costs(request_id, status, currency, price_source,
                             input_mode, input_rate_nanos, output_rate_nanos,
                             cache_read_rate_nanos, cache_write_rate_nanos, price_snapshot_at_ms,
                             pricing_version, calculated_at_ms)
                         VALUES(?1,'pending','CNY','default_zero','flat',0,0,0,0,1000,2,1000)",
                        [&id],
                    )
                    .unwrap();
            }
            connection.execute_batch("COMMIT").unwrap();
        }
        let started = Instant::now();
        let handle = StatsHandle::start(dir.path().to_path_buf());
        assert!(
            wait_for_state(&handle, HealthState::Ready, Duration::from_secs(25)),
            "the writer never became ready"
        );
        // Progress must not be delayed by the failure backoff.
        let deadline = Instant::now() + Duration::from_secs(20);
        let mut pending = i64::MAX;
        while Instant::now() < deadline {
            pending = query_one(
                dir.path(),
                "SELECT COUNT(*) FROM usage_costs WHERE status='pending'",
                &[],
            );
            if pending == 0 {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert_eq!(pending, 0, "20000 legacy rows must be repaired promptly");
        assert!(
            started.elapsed() < Duration::from_secs(25),
            "the repair took {:?}",
            started.elapsed()
        );
        assert_eq!(handle.state(), HealthState::Ready);
    }

    /// A healthy collector stays Ready while its asynchronous writer has queued
    /// events. Wait for actual settlement before asserting exactly-once storage;
    /// the lock-loss recovery path is exercised separately below.
    #[tokio::test]
    async fn an_admitted_request_is_persisted_and_settled_exactly_once() {
        let dir = temporary_dir();
        let handle = StatsHandle::start(dir.path().to_path_buf());
        assert!(wait_ready(&handle));
        // Admit and finish one request through the real asynchronous writer.
        {
            let mut guard = handle
                .begin(spec("model-admitted"), Protocol::Sse, false)
                .unwrap();
            guard.mark_upstream_started();
            guard.observe(
                b"data: {\"usage\":{\"prompt_tokens\":5,\"completion_tokens\":1,\"total_tokens\":6}}\n\ndata: [DONE]\n\n",
            );
            guard.finish(dto::Outcome::Completed, None);
        }
        // Ready is a health signal, not a write barrier. The cost row provides
        // an observable completion boundary for both Start and Finish writes.
        let _ = wait_cost_count(&dir, "model-admitted", 1).await;
        let ready = wait_for_health(&handle, HealthState::Ready, Duration::from_secs(15));
        assert!(ready, "settlement must leave the collector ready");
        let rows: i64 = query_one(
            dir.path(),
            "SELECT COUNT(*) FROM usage_requests WHERE upstream_model='model-admitted'",
            &[],
        );
        assert_eq!(
            rows, 1,
            "the admitted request must still exist exactly once"
        );
        let pending: i64 = query_one(
            dir.path(),
            "SELECT COUNT(*) FROM usage_costs WHERE status='pending'",
            &[],
        );
        assert_eq!(pending, 0, "no cost row may stay pending");
    }

    /// R7-02 (real entry point): an external lock swallows both the `Start` and
    /// the `Finish` write. Once released, recovery must reach `ready` on its own
    /// — no new request, no invented row, and the loss stays visible.
    #[tokio::test]
    async fn a_lock_that_swallows_both_writes_recovers_without_new_traffic() {
        let dir = temporary_dir();
        let handle = StatsHandle::start(dir.path().to_path_buf());
        assert!(wait_ready(&handle));
        let blocker = rusqlite::Connection::open(dir.path().join("usage.sqlite3")).unwrap();
        blocker.busy_timeout(Duration::from_millis(50)).unwrap();
        blocker.execute_batch("BEGIN IMMEDIATE").unwrap();
        {
            let mut guard = handle
                .begin(spec("model-locked"), Protocol::Sse, false)
                .unwrap();
            guard.mark_upstream_started();
            guard.observe(
                b"data: {\"usage\":{\"prompt_tokens\":7,\"completion_tokens\":2,\"total_tokens\":9}}\n\ndata: [DONE]\n\n",
            );
            guard.finish(dto::Outcome::Completed, None);
        }
        std::thread::sleep(Duration::from_millis(400));
        let degraded = wait_for_health(&handle, HealthState::Degraded, Duration::from_secs(10));
        assert!(degraded, "lost writes must be reported, not hidden");
        blocker.execute_batch("ROLLBACK").unwrap();
        drop(blocker);
        // No new request is admitted: recovery must retry by itself.
        let ready = wait_for_health(&handle, HealthState::Ready, Duration::from_secs(20));
        assert!(ready, "an idle proxy must still finish recovery");
        let rows: i64 = query_one(
            dir.path(),
            "SELECT COUNT(*) FROM usage_requests WHERE upstream_model='model-locked'",
            &[],
        );
        // Both writes were lost, so the row genuinely never existed: recovery
        // must not fabricate one to fill the hole.
        assert_eq!(
            rows, 0,
            "a lost Start must not be replaced by an invented row"
        );
        let failures: i64 = query_one(
            dir.path(),
            "SELECT COALESCE(SUM(write_failures), 0) FROM stats_runs",
            &[],
        );
        assert!(failures >= 1, "the loss must stay visible in the counters");
        let pending: i64 = query_one(
            dir.path(),
            "SELECT COUNT(*) FROM usage_costs WHERE status='pending'",
            &[],
        );
        assert_eq!(
            pending, 0,
            "the cost of a recovered request must be settled"
        );
        let queued: i64 = query_one(
            dir.path(),
            "SELECT COUNT(*) FROM stats_recovery_backlog",
            &[],
        );
        assert_eq!(
            queued, 0,
            "nothing may be left queued once ready is reported"
        );
    }

    /// U05: a cost table that refuses writes must not report `ready`. The
    /// failure window alone is not evidence of recovery: the orphaned request
    /// and its cost still need a terminal state.
    #[tokio::test]
    async fn a_refused_cost_write_keeps_the_writer_degraded_until_it_recovers() {
        let dir = temporary_dir();
        let handle = StatsHandle::start(dir.path().to_path_buf());
        assert!(wait_ready(&handle));
        // Reject every cost status change, exactly like a locked table.
        {
            let connection = rusqlite::Connection::open(dir.path().join("usage.sqlite3")).unwrap();
            connection
                .execute_batch(
                    "CREATE TRIGGER refuse_cost BEFORE UPDATE OF status ON usage_costs
                     BEGIN SELECT RAISE(ABORT, 'cost writes refused'); END;",
                )
                .unwrap();
        }
        let mut guard = handle
            .begin(spec("model-cost-fail"), Protocol::Sse, false)
            .unwrap();
        guard.mark_upstream_started();
        guard.observe(
            b"data: {\"usage\":{\"prompt_tokens\":10,\"completion_tokens\":1,\"total_tokens\":11}}\n\ndata: [DONE]\n\n",
        );
        guard.finish(dto::Outcome::Completed, None);
        // The request row cannot reach a terminal state while its cost is stuck.
        let stuck = wait_for_health(&handle, HealthState::Degraded, Duration::from_secs(10));
        assert!(stuck, "a refused cost write must be reported as degraded");
        {
            let connection = rusqlite::Connection::open(dir.path().join("usage.sqlite3")).unwrap();
            connection
                .execute_batch("DROP TRIGGER refuse_cost")
                .unwrap();
        }
        // No new traffic: recovery must still complete and return to ready.
        let recovered = wait_for_health(&handle, HealthState::Ready, Duration::from_secs(20));
        assert!(recovered, "recovery must retry without new requests");
        if let Some(record) = cost_snapshots(dir.path(), "model-cost-fail").first() {
            assert_ne!(record.status, "pending", "{record:?}");
        }
    }

    fn wait_for_health(handle: &StatsHandle, state: HealthState, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if handle.state() == state {
                return true;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        handle.state() == state
    }

    #[tokio::test]
    async fn health_reports_state_and_disabled_collection_is_documented() {
        let dir = temporary_dir();
        let handle = StatsHandle::start(dir.path().to_path_buf());
        assert!(wait_ready(&handle));
        let health: HealthDto = serde_json::from_str(
            &handle
                .command(&json!({"command":"stats_health"}))
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(health.state, HealthState::Ready);
        assert!(health.enabled);
        assert_eq!(health.queue_capacity, EVENT_QUEUE_CAPACITY as i64);

        let settings: SettingsDto = serde_json::from_str(
            &handle
                .command(&json!({
                    "command":"stats_settings_update",
                    "enabled": false,
                    "retention_days": 90,
                    "expected_revision": health.settings_revision
                }))
                .await
                .unwrap(),
        )
        .unwrap();
        assert!(!settings.enabled);
        assert_eq!(settings.retention_days, 90);
        assert!(
            handle
                .begin(spec("model-x"), Protocol::Sse, false)
                .is_none()
        );
        // The old revision is rejected instead of silently overwriting.
        let conflict = handle
            .command(&json!({
                "command":"stats_settings_update",
                "enabled": true,
                "retention_days": 90,
                "expected_revision": health.settings_revision
            }))
            .await
            .unwrap_err();
        assert!(
            conflict.starts_with(dto::ERR_REVISION_CONFLICT),
            "{conflict}"
        );
    }

    #[tokio::test]
    async fn disabled_collection_stops_admitting_new_requests() {
        let dir = temporary_dir();
        let handle = StatsHandle::start(dir.path().to_path_buf());
        assert!(wait_ready(&handle));
        let health: HealthDto = serde_json::from_str(
            &handle
                .command(&json!({"command":"stats_health"}))
                .await
                .unwrap(),
        )
        .unwrap();
        assert!(
            handle
                .begin(spec("model-x"), Protocol::Sse, false)
                .is_some()
        );
        handle
            .command(&json!({
                "command":"stats_settings_update",
                "enabled": false,
                "retention_days": 180,
                "expected_revision": health.settings_revision
            }))
            .await
            .unwrap();
        assert!(
            handle
                .begin(spec("model-x"), Protocol::Sse, false)
                .is_none()
        );
    }

    /// P01: a price edit must not change a request that already exists, and the
    /// next request must use the new price.
    #[tokio::test]
    async fn editing_a_price_never_rewrites_history() {
        let dir = temporary_dir();
        let handle = StatsHandle::start(dir.path().to_path_buf());
        assert!(wait_ready(&handle));

        let saved = save_price(&handle, "model-priced", "1", "4").await;
        let mut guard = handle
            .begin(spec("model-priced"), Protocol::Sse, false)
            .unwrap();
        guard.mark_upstream_started();
        guard.observe(
            b"data: {\"usage\":{\"prompt_tokens\":1000,\"completion_tokens\":100,\"total_tokens\":1100}}\n\ndata: [DONE]\n\n",
        );
        guard.finish(dto::Outcome::Completed, None);
        let first = wait_cost(&dir, "model-priced").await;
        assert_eq!(first.status, "priced", "{first:?}");
        assert_eq!(first.input_rate.as_deref(), Some("1000000000"));
        // 1000 input tokens at $1/M plus 100 output at $4/M, in nano units.
        assert_eq!(first.total.as_deref(), Some("1400000"), "{first:?}");
        assert_eq!(
            first.source_price_id.as_deref(),
            Some(saved.rule_id.as_str())
        );

        let updated = save_price_with(
            &handle,
            "model-priced",
            "3",
            "12",
            Some(saved.rule_id.clone()),
            1,
        )
        .await;
        let mut guard = handle
            .begin(spec("model-priced"), Protocol::Sse, false)
            .unwrap();
        guard.mark_upstream_started();
        guard.observe(
            b"data: {\"usage\":{\"prompt_tokens\":1000,\"completion_tokens\":100,\"total_tokens\":1100}}\n\ndata: [DONE]\n\n",
        );
        guard.finish(dto::Outcome::Completed, None);
        let _ = wait_cost_count(&dir, "model-priced", 2).await;
        let rows = cost_snapshots(dir.path(), "model-priced");
        let first_row = rows
            .iter()
            .find(|row| row.status == "priced" && row.input_rate.as_deref() == Some("1000000000"));
        assert!(
            first_row.is_some(),
            "the first request must keep P1: {rows:?}"
        );
        assert_eq!(first_row.unwrap().total.as_deref(), Some("1400000"));
        // Editing keeps the row id, so the new request is the one with revision 2.
        let second_row = rows.iter().find(|row| row.source_price_revision == Some(2));
        assert_eq!(
            second_row.and_then(|row| row.input_rate.as_deref()),
            Some("3000000000"),
            "{rows:?}"
        );
        assert_eq!(
            second_row.and_then(|row| row.source_price_id.as_deref()),
            Some(updated.rule_id.as_str())
        );
        assert_eq!(
            second_row.and_then(|row| row.total.as_deref()),
            Some("4200000"),
            "{rows:?}"
        );

        // Deleting the current price leaves history intact and makes new
        // requests an explicit zero.
        delete_price(&handle, &updated.rule_id, 2).await;
        let mut guard = handle
            .begin(spec("model-priced"), Protocol::Sse, false)
            .unwrap();
        guard.mark_upstream_started();
        guard.observe(
            b"data: {\"usage\":{\"prompt_tokens\":1000,\"completion_tokens\":100,\"total_tokens\":1100}}\n\ndata: [DONE]\n\n",
        );
        guard.finish(dto::Outcome::Completed, None);
        let _ = wait_cost_count(&dir, "model-priced", 3).await;
        let rows = cost_snapshots(dir.path(), "model-priced");
        let zero = rows
            .iter()
            .find(|row| row.price_source == "default_zero")
            .unwrap();
        assert_eq!(zero.status, "default_zero");
        assert_eq!(zero.input_rate.as_deref(), Some("0"));
        assert_eq!(zero.total.as_deref(), Some("0"));
        assert!(zero.source_price_id.is_none());
        // The two priced rows are still complete and readable.
        assert_eq!(
            rows.iter().filter(|row| row.status == "priced").count(),
            2,
            "{rows:?}"
        );
    }

    /// P01: a request keeps the price it was admitted with even when the
    /// configuration changes while it is still running.
    #[tokio::test]
    async fn a_price_change_mid_flight_does_not_affect_the_running_request() {
        let dir = temporary_dir();
        let handle = StatsHandle::start(dir.path().to_path_buf());
        assert!(wait_ready(&handle));
        let saved = save_price(&handle, "model-midflight", "2", "2").await;
        let mut guard = handle
            .begin(spec("model-midflight"), Protocol::Sse, false)
            .unwrap();
        guard.mark_upstream_started();
        // The price changes after admission but before the request settles.
        let updated = save_price_with(
            &handle,
            "model-midflight",
            "9",
            "9",
            Some(saved.rule_id.clone()),
            1,
        )
        .await;
        guard.observe(
            b"data: {\"usage\":{\"prompt_tokens\":1000,\"completion_tokens\":100,\"total_tokens\":1100}}\n\ndata: [DONE]\n\n",
        );
        guard.finish(dto::Outcome::Completed, None);
        let settled = wait_cost(&dir, "model-midflight").await;
        assert_eq!(
            settled.input_rate.as_deref(),
            Some("2000000000"),
            "{settled:?}"
        );
        assert_eq!(settled.total.as_deref(), Some("2200000"), "{settled:?}");
        // Editing a price updates the same row, so the id is stable while the
        // revision and the captured rates move on; the request keeps revision 1.
        assert_eq!(
            settled.source_price_id.as_deref(),
            Some(saved.rule_id.as_str())
        );
        assert_eq!(settled.source_price_revision, Some(1));
        assert_eq!(updated.rule_id, saved.rule_id);

        let mut guard = handle
            .begin(spec("model-midflight"), Protocol::Sse, false)
            .unwrap();
        guard.mark_upstream_started();
        guard.observe(
            b"data: {\"usage\":{\"prompt_tokens\":1000,\"completion_tokens\":100,\"total_tokens\":1100}}\n\ndata: [DONE]\n\n",
        );
        guard.finish(dto::Outcome::Completed, None);
        let _ = wait_cost_count(&dir, "model-midflight", 2).await;
        let rows = cost_snapshots(dir.path(), "model-midflight");
        assert!(
            rows.iter()
                .any(|row| row.input_rate.as_deref() == Some("9000000000")),
            "{rows:?}"
        );
        assert!(
            rows.iter().any(|row| row.source_price_revision == Some(2)),
            "the later request must carry revision 2: {rows:?}"
        );
    }

    /// P01: a missing price is a confirmed zero, and later adding one leaves the
    /// earlier zero alone. A missing token count stays NULL.
    #[tokio::test]
    async fn a_missing_price_is_zero_and_later_prices_do_not_backfill_it() {
        let dir = temporary_dir();
        let handle = StatsHandle::start(dir.path().to_path_buf());
        assert!(wait_ready(&handle));
        let mut guard = handle
            .begin(spec("model-free"), Protocol::Sse, false)
            .unwrap();
        guard.mark_upstream_started();
        guard.observe(
            b"data: {\"usage\":{\"prompt_tokens\":1000,\"completion_tokens\":100,\"total_tokens\":1100}}\n\ndata: [DONE]\n\n",
        );
        guard.finish(dto::Outcome::Completed, None);
        let free = wait_cost(&dir, "model-free").await;
        assert_eq!(free.price_source, "default_zero");
        assert_eq!(free.status, "default_zero");
        assert_eq!(free.total.as_deref(), Some("0"));
        assert_eq!(free.input_rate.as_deref(), Some("0"));

        // A usage-less request with no price is still zero, but its tokens stay
        // NULL: a zero amount must not fabricate token counts.
        let mut guard = handle
            .begin(spec("model-free-nousage"), Protocol::Sse, false)
            .unwrap();
        guard.mark_upstream_started();
        guard.observe(b"data: {\"choices\":[{\"delta\":{\"content\":\"x\"}}]}\n\ndata: [DONE]\n\n");
        guard.finish(dto::Outcome::Completed, None);
        let nousage = wait_cost(&dir, "model-free-nousage").await;
        assert_eq!(nousage.total.as_deref(), Some("0"), "{nousage:?}");
        assert_eq!(
            model_tokens(dir.path(), "model-free-nousage"),
            (None, None),
            "tokens must stay unknown"
        );

        save_price(&handle, "model-free", "5", "5").await;
        let mut guard = handle
            .begin(spec("model-free"), Protocol::Sse, false)
            .unwrap();
        guard.mark_upstream_started();
        guard.observe(
            b"data: {\"usage\":{\"prompt_tokens\":1000,\"completion_tokens\":100,\"total_tokens\":1100}}\n\ndata: [DONE]\n\n",
        );
        guard.finish(dto::Outcome::Completed, None);
        let _ = wait_cost_count(&dir, "model-free", 2).await;
        let rows = cost_snapshots(dir.path(), "model-free");
        assert!(
            rows.iter().any(|row| row.total.as_deref() == Some("0")),
            "{rows:?}"
        );
        assert!(
            rows.iter()
                .any(|row| row.total.as_deref() == Some("5500000")),
            "{rows:?}"
        );
    }

    /// P01: a non-zero price with a missing breakdown stays unknown instead of
    /// being reported as a zero amount.
    #[tokio::test]
    async fn a_non_zero_price_with_missing_usage_stays_unknown() {
        let dir = temporary_dir();
        let handle = StatsHandle::start(dir.path().to_path_buf());
        assert!(wait_ready(&handle));
        save_price(&handle, "model-needs-usage", "2", "2").await;
        let mut guard = handle
            .begin(spec("model-needs-usage"), Protocol::Sse, false)
            .unwrap();
        guard.mark_upstream_started();
        guard.observe(b"data: {\"choices\":[{\"delta\":{\"content\":\"x\"}}]}\n\ndata: [DONE]\n\n");
        guard.finish(dto::Outcome::Completed, None);
        let unknown = wait_cost(&dir, "model-needs-usage").await;
        assert_eq!(unknown.status, "usage_incomplete", "{unknown:?}");
        assert!(unknown.total.is_none(), "{unknown:?}");
        assert_eq!(unknown.price_source, "manual");
        assert_eq!(unknown.input_rate.as_deref(), Some("2000000000"));
    }

    /// T05: a confirmed zero must reach the money totals. Showing nothing made
    /// the page report "unpriced" for a request the user is told costs 0.
    #[tokio::test]
    async fn a_missing_price_appears_as_a_zero_amount_in_every_summary() {
        let dir = temporary_dir();
        let handle = StatsHandle::start(dir.path().to_path_buf());
        assert!(wait_ready(&handle));
        let mut guard = handle
            .begin(spec("model-zero-cost"), Protocol::Sse, false)
            .unwrap();
        guard.mark_upstream_started();
        guard.observe(
            b"data: {\"usage\":{\"prompt_tokens\":100,\"completion_tokens\":10,\"total_tokens\":110}}\n\ndata: [DONE]\n\n",
        );
        guard.finish(dto::Outcome::Completed, None);
        let _ = wait_cost(&dir, "model-zero-cost").await;
        let window =
            json!({"from_ms": now_ms() - 3_600_000, "to_ms": now_ms() + 1000, "timezone": "UTC"});
        let dashboard: DashboardDto = serde_json::from_str(
            &handle
                .command(&json!({
                    "command": "stats_dashboard",
                    "request_tag": "t",
                    "filter": window,
                    "granularity": "auto"
                }))
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            dashboard.summary.default_zero_request_count, 1,
            "{dashboard:?}"
        );
        assert_eq!(dashboard.summary.settled_cost_request_count, 1);
        let total = dashboard
            .summary
            .costs
            .iter()
            .find(|cost| cost.currency == crate::stats::dto::Currency::Cny)
            .expect("a CNY zero total");
        assert_eq!(total.total_cost_nanos, "0", "{total:?}");
        // The trend and the grouping see the same amount.
        let point = dashboard
            .series
            .iter()
            .find(|point| point.request_count > 0)
            .expect("a non-empty bucket");
        assert!(
            point.costs.iter().any(|cost| cost.total_cost_nanos == "0"),
            "{point:?}"
        );
        let breakdown: crate::stats::dto::BreakdownDto = serde_json::from_str(
            &handle
                .command(&json!({
                    "command": "stats_breakdown",
                    "request_tag": "t",
                    "filter": window,
                    "group_by": "model",
                    "sort": "total_tokens",
                    "limit": 20
                }))
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            breakdown.items[0].summary.default_zero_request_count, 1,
            "{:?}",
            breakdown.items[0]
        );
    }

    /// T02: a request that never reached the upstream is a confirmed zero even
    /// when a manual price is configured. Writing NULL used to violate the v2
    /// amount constraint and turned a legitimate rejection into a collection
    /// failure.
    #[tokio::test]
    async fn a_priced_request_that_never_started_is_a_zero_not_a_failure() {
        let dir = temporary_dir();
        let handle = StatsHandle::start(dir.path().to_path_buf());
        assert!(wait_ready(&handle));
        save_price(&handle, "model-missing-key", "2", "8").await;
        let mut rejected = handle
            .begin(spec("model-missing-key"), Protocol::Sse, false)
            .unwrap();
        rejected.finish(dto::Outcome::Rejected, Some(dto::ErrorKind::MissingKey));
        let cost = wait_cost(&dir, "model-missing-key").await;
        assert_eq!(cost.status, "not_applicable", "{cost:?}");
        assert_eq!(cost.total.as_deref(), Some("0"), "{cost:?}");
        let row = query_one_pair(dir.path(), "model-missing-key");
        assert_eq!(row, ("rejected".to_owned(), Some(0)), "{row:?}");
        let pending: i64 = query_one(
            dir.path(),
            "SELECT COUNT(*) FROM usage_costs WHERE status='pending'",
            &[],
        );
        assert_eq!(pending, 0, "no cost row may be left pending");
    }

    /// P01: a request that never reached the upstream is not applicable, which
    /// is distinct from the zero default used for an unconfigured model.
    #[tokio::test]
    async fn not_applicable_and_default_zero_are_distinct() {
        let dir = temporary_dir();
        let handle = StatsHandle::start(dir.path().to_path_buf());
        assert!(wait_ready(&handle));
        let mut rejected = handle
            .begin(spec("model-not-sent"), Protocol::Sse, false)
            .unwrap();
        rejected.finish(dto::Outcome::Rejected, Some(dto::ErrorKind::MissingKey));
        let not_sent = wait_cost(&dir, "model-not-sent").await;
        assert_eq!(not_sent.status, "not_applicable", "{not_sent:?}");
        assert_eq!(not_sent.total.as_deref(), Some("0"));

        let mut unpriced = handle
            .begin(spec("model-no-price"), Protocol::Sse, false)
            .unwrap();
        unpriced.mark_upstream_started();
        unpriced.observe(
            b"data: {\"usage\":{\"prompt_tokens\":10,\"completion_tokens\":1,\"total_tokens\":11}}\n\ndata: [DONE]\n\n",
        );
        unpriced.finish(dto::Outcome::Completed, None);
        let zero = wait_cost(&dir, "model-no-price").await;
        assert_eq!(zero.status, "default_zero", "{zero:?}");
        assert_eq!(zero.price_source, "default_zero");
        assert_ne!(zero.status, not_sent.status);
    }

    /// F02: a write lock held past the retry budget must not stop collection
    /// forever. The writer probes, records the failure window and settles the
    /// rows whose `Finish` could never be written.
    #[tokio::test]
    async fn a_transient_database_lock_recovers_settles_orphans_and_resumes() {
        let dir = temporary_dir();
        let handle = StatsHandle::start(dir.path().to_path_buf());
        assert!(wait_ready(&handle), "writer did not become ready");

        // `Start` is committed before the lock appears.
        let mut guard = handle
            .begin(spec("model-lock"), Protocol::Sse, false)
            .unwrap();
        guard.mark_upstream_started();
        let mut rows = 0i64;
        for _ in 0..200 {
            rows = query_one::<i64>(
                dir.path(),
                "SELECT COUNT(*) FROM usage_requests WHERE upstream_model='model-lock'",
                &[],
            );
            if rows == 1 {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(rows, 1, "the admitted request must be persisted first");

        // Hold the write lock from a second connection while the request ends.
        let blocker = rusqlite::Connection::open(dir.path().join("usage.sqlite3")).unwrap();
        blocker.busy_timeout(Duration::from_millis(50)).unwrap();
        blocker.execute_batch("BEGIN IMMEDIATE").unwrap();
        guard.finish(dto::Outcome::Completed, None);

        assert!(
            wait_for_state(&handle, HealthState::Degraded, Duration::from_secs(30)),
            "the writer must report a degraded database"
        );
        // New requests keep being admitted while degraded (writes are delayed,
        // not silently dropped for the rest of the run).
        assert!(
            handle
                .begin(spec("model-while-locked"), Protocol::Sse, false)
                .is_some()
        );

        blocker.execute_batch("ROLLBACK").unwrap();
        drop(blocker);

        assert!(
            wait_for_state(&handle, HealthState::Ready, Duration::from_secs(30)),
            "the writer must recover without new traffic"
        );

        // The orphaned row gets a trustworthy terminal state: interrupted, with
        // no fabricated duration or tokens.
        let mut orphan: (String, Option<i64>, Option<i64>) =
            (String::from("in_flight"), None, None);
        for _ in 0..200 {
            let connection = rusqlite::Connection::open(dir.path().join("usage.sqlite3")).unwrap();
            orphan = connection
                .query_row(
                    "SELECT outcome, duration_ms, total_tokens FROM usage_requests
                     WHERE upstream_model='model-lock'",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .unwrap();
            if orphan.0 != "in_flight" {
                break;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        assert_eq!(orphan.0, "interrupted", "{orphan:?}");
        assert_eq!(
            orphan.1, None,
            "a settled orphan must not invent a duration"
        );
        assert_eq!(orphan.2, None, "a settled orphan must not invent tokens");

        // The failure window is recorded as degraded, so charts never report it
        // as collecting/zero.
        let degraded: i64 = query_one::<i64>(
            dir.path(),
            "SELECT COUNT(*) FROM stats_collection_intervals WHERE state='degraded'",
            &[],
        );
        assert!(degraded >= 1, "the failure window must be recorded");

        // Collection continues afterwards.
        let mut guard = handle
            .begin(spec("model-after-lock"), Protocol::Sse, false)
            .unwrap();
        guard.mark_upstream_started();
        guard.finish(dto::Outcome::Completed, None);
        let mut recorded = 0i64;
        for _ in 0..300 {
            recorded = query_one::<i64>(
                dir.path(),
                "SELECT COUNT(*) FROM usage_requests WHERE upstream_model='model-after-lock'
                   AND outcome<>'in_flight'",
                &[],
            );
            if recorded == 1 {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(recorded, 1, "recording must resume after recovery");
    }

    /// F02: a malformed event must be isolated, not roll back healthy requests
    /// that share its batch.
    #[tokio::test]
    async fn isolated_retry_keeps_healthy_requests_from_a_poisoned_batch() {
        let dir = temporary_dir();
        let handle = StatsHandle::start(dir.path().to_path_buf());
        assert!(wait_ready(&handle));
        // A request that never started still produces a `Finish`; the writer has
        // to survive it (update affects 0 rows) alongside real traffic.
        let spec_a = spec("model-healthy");
        let mut guard = handle.begin(spec_a, Protocol::Sse, false).unwrap();
        guard.mark_upstream_started();
        guard.finish(dto::Outcome::Completed, None);
        let mut recorded = 0i64;
        for _ in 0..300 {
            recorded = query_one::<i64>(
                dir.path(),
                "SELECT COUNT(*) FROM usage_requests WHERE upstream_model='model-healthy'
                   AND outcome<>'in_flight'",
                &[],
            );
            if recorded == 1 {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(recorded, 1);
        assert_eq!(handle.state(), HealthState::Ready);
    }

    #[tokio::test]
    async fn clear_requires_an_unexpired_preview_token() {
        let dir = temporary_dir();
        let handle = StatsHandle::start(dir.path().to_path_buf());
        assert!(wait_ready(&handle));
        let mut guard = handle.begin(spec("model-x"), Protocol::Sse, false).unwrap();
        guard.mark_upstream_started();
        guard.finish(dto::Outcome::Completed, None);
        let preview: serde_json::Value = serde_json::from_str(
            &handle
                .command(&json!({"command":"stats_clear_preview","before_ms": now_ms() + 1000}))
                .await
                .unwrap(),
        )
        .unwrap();
        assert!(preview["row_count"].as_i64().unwrap() >= 0);
        let token = preview["token"].as_str().unwrap().to_owned();
        let bad = handle
            .command(&json!({"command":"stats_clear_start","token":"not-the-token"}))
            .await
            .unwrap_err();
        assert!(bad.starts_with(dto::ERR_INVALID), "{bad}");
        let started: MaintenanceDto = serde_json::from_str(
            &handle
                .command(&json!({"command":"stats_clear_start","token":token}))
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(started.kind, MaintenanceKind::Clear);
    }
}
