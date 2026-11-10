//! 统计持久化边界：SQLite schema 迁移、唯一写连接、只读查询连接、启动恢复和
//! 有界维护工作的所有者。
//!
//! 上游是由请求处理路径投入 `stats::event::EventQueue` 的 `StatsEvent::Start` 和
//! `StatsEvent::Finish`；下游是同一代理进程内的 `stats-writer` 线程持有一个
//! `Writer` 并调用本模块。`Writer` 内的 rusqlite `Connection` 只由该线程串行使用，
//! 不跨线程共享；统计页的 `stats-reader` 线程会另行调用 `open_connection(..., true)`
//! 建立独立只读连接。数据库文件跨进程重启保留，但这些 Rust 对象和线程内状态只属于
//! 当前进程。
//!
//! 请求路径与 writer 之间还共享 `Arc<Counters>`。`Arc` 只是让多个线程持有同一个计数器
//! 对象，字段本身使用原子类型；克隆 `Arc` 不会复制计数器，也不是 Java Spring 的依赖
//! 注入。请求处理线程不会直接调用 `Writer`。
//!
//! 普通批次中的多个事件通常由一次 SQLite `IMMEDIATE` 事务提交；但确定性坏行会在
//! 首次整批失败后改为“每个事件各自一个事务”隔离，因此一次 `apply_batch` 并不保证
//! 全程只有一个事务。`Start` 与 `Finish` 若恰好处于同一次成功批次，会一起提交；
//! 若分处不同批次，则 `Start` 可以先落库而后续 `Finish` 失败。
//!
//! SQLite 的唯一键和条件更新为重复输入提供“不重复插入、不覆盖已落库终态”的幂等边界；
//! 它不是事件投递的 exactly-once，也不是把 `RequestGuard` 的 at-most-once 发布自动升级
//! 成 exactly-once 持久化。批次被整批拒绝时，调用方不会重放原事件对象；它只记住其中
//! `Finish` 的 `request_id`，后续把可存在的 `in_flight` 请求收敛为中断状态。原
//! `FinishRecord` 中的精确结果、token 和时延可能已经丢失，恢复不能把这些未知值伪装成
//! 原终态或成功。
//!
//! 维护任务在事件批次之间分步运行，避免长时间占据唯一写连接。本文件负责持久化与
//! 生命周期接口；定价规则、WorkBuddy 导入算法和清理筛选细节分别属于各自模块。
use crate::stats::collector::UsageOutcome;
use crate::stats::dto::{
    CostStatus, Currency, ERR_INVALID, ERR_REVISION_CONFLICT, ERR_UNAVAILABLE, InputBreakdown,
    InputMode, MaintenanceKind, MaintenanceStatus, PriceRuleDto, UsageMode, UsageStatus, error,
};
use crate::stats::event::{FinishRecord, StatsEvent};
use crate::stats::pricing::{self, CostParts, PriceRule};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, atomic::Ordering},
    time::{Duration, Instant},
};

/// 当前构建理解的 schema 版本；数据库 `PRAGMA user_version` 保存同一整数。
///
/// v1 建立基础表，v2 引入请求级价格快照，v3 增加持久恢复 backlog，v4 记录协议，
/// v5 调整供应商 usage 请求默认值，v6 保存价格目录，v7-v11 扩展 WorkBuddy 导入、
/// 身份关联、解析/展示元数据及“独立导入费用可无 token”的约束。迁移 SQL 通过
/// `include_str!` 编入二进制；这些是版本化 schema 步骤，不是连接时必须重放的脚本。
pub const SCHEMA_VERSION: i64 = 11;
const MIGRATION_0001: &str = include_str!("migrations/0001.sql");
const MIGRATION_0002: &str = include_str!("migrations/0002.sql");
const MIGRATION_0003: &str = include_str!("migrations/0003.sql");
const MIGRATION_0004: &str = include_str!("migrations/0004.sql");
const MIGRATION_0005: &str = include_str!("migrations/0005.sql");
const MIGRATION_0006: &str = include_str!("migrations/0006.sql");
const MIGRATION_0007: &str = include_str!("migrations/0007.sql");
const MIGRATION_0008: &str = include_str!("migrations/0008.sql");
const MIGRATION_0009: &str = include_str!("migrations/0009.sql");
const MIGRATION_0010: &str = include_str!("migrations/0010.sql");
const MIGRATION_0011: &str = include_str!("migrations/0011.sql");
#[cfg(test)]
#[path = "store/token_applicability_tests.rs"]
mod token_applicability_tests;
const WRITE_RETRIES: usize = 2;
/// 单次 SQLite 忙等待上限（毫秒）。外层 `apply_batch` 在总预算内负责有限重试，
/// 避免一次锁等待本身消耗数秒；它是等待策略，不是事务隔离级别。
pub const BUSY_WAIT_MS: u64 = 250;
/// 一次批次调用及其重试、隔离尝试共用的总时间预算。
pub const BATCH_BUDGET: Duration = Duration::from_millis(1500);

#[derive(Clone, Debug)]
pub struct DbError {
    pub message: String,
    /// 来自 SQLite 时可分类重试/隔离的错误码；本项目自己的校验或拒绝错误为
    /// `None`。`message` 可包含驱动详情，不能直接作为公开 health 文案。
    pub kind: Option<rusqlite::ErrorCode>,
}

impl std::fmt::Display for DbError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl From<rusqlite::Error> for DbError {
    fn from(value: rusqlite::Error) -> Self {
        let kind = match &value {
            rusqlite::Error::SqliteFailure(code, _) => Some(code.code),
            _ => None,
        };
        DbError {
            message: value.to_string(),
            kind,
        }
    }
}

impl DbError {
    /// 构造一个非数据库故障的校验/拒绝错误。
    ///
    /// `DbError::refused` 的 `kind=None`，所以 `is_transient` 不会把它当成锁冲突；
    /// 它只是统一本模块的返回类型，类似 Java 中把业务拒绝和底层异常包进一个结果边界，
    /// 但不会自动回滚已经执行的 SQL，回滚仍由 `Connection`/`Transaction` 决定。
    pub fn refused(message: String) -> Self {
        Self {
            message,
            kind: None,
        }
    }
}

impl From<String> for DbError {
    fn from(value: String) -> Self {
        DbError::refused(value)
    }
}

/// 按调用用途打开连接并应用本项目约定的 PRAGMA。
///
/// `read_only=true` 由统计查询线程使用：以只读/URI 标志打开，设置
/// `query_only=ON`，把单次忙等待限制为 1000 ms，并设置仅用于报告查询页缓存和
/// mmap 虚拟地址预算。这个连接不会执行写事务，也不会验证或切换 journal mode。
///
/// `read_only=false` 由 `Writer` 启动流程使用：启用外键，设置唯一写连接的页缓存、
/// `synchronous=NORMAL` 和自动 WAL checkpoint 触发值，再确认数据库确实进入 WAL。
/// 返回的 `Connection` 归调用线程所有；`&Connection`/`&mut Connection` 是 Rust 的
/// 借用，离开作用域后连接不会像 Spring bean 那样被容器自动注入到别处。
pub fn open_connection(path: &Path, read_only: bool) -> Result<Connection, DbError> {
    let conn = if read_only {
        Connection::open_with_flags(
            path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_URI,
        )?
    } else {
        Connection::open(path)?
    };
    // A bounded single wait: retries are budgeted by the writer loop, so one
    // lock attempt must never consume seconds on its own.
    conn.busy_timeout(Duration::from_millis(if read_only {
        1000
    } else {
        BUSY_WAIT_MS
    }))?;
    if read_only {
        conn.pragma_update(None, "query_only", "ON")?;
        // Only the on-demand report reader gets this bounded SQLite page
        // cache. The million-row covering index exceeds SQLite's default
        // 2 MiB cache, causing every warm report to re-read its pages. This
        // caches database pages, never report results; WAL snapshot rules
        // still invalidate changed pages. The reader exits after 60s idle.
        // Import workers and the sole writer keep their existing budgets.
        conn.pragma_update(None, "cache_size", -131_072_i64)?;
        // Read-only mappings survive SQLite's page-cache reset between WAL
        // snapshots. The virtual-address cap is not an eager allocation; report
        // RSS is measured separately from the import-only worker budget.
        conn.pragma_update(None, "mmap_size", 2_147_483_648_i64)?;
        return Ok(conn);
    }
    conn.pragma_update(None, "foreign_keys", "ON")?;
    // Budget 16 MiB for the sole writer's SQLite page cache so growing fact and
    // ledger indexes can retain more upper pages.
    conn.pragma_update(None, "cache_size", -16_384_i64)?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    // Bound individual checkpoint I/O bursts while reports and imports share
    // the disk. Keep automatic PASSIVE checkpoints enabled, at roughly 4 MiB
    // for the normal 4 KiB page size; this is a trigger, not a hard WAL cap.
    // Transaction/ACK boundaries and synchronous=NORMAL remain unchanged.
    conn.pragma_update(None, "wal_autocheckpoint", 1_024)?;
    let mode: String = conn.query_row("PRAGMA journal_mode=WAL", [], |row| row.get(0))?;
    if !mode.eq_ignore_ascii_case("wal") {
        return Err(DbError::refused(format!(
            "statistics database cannot use WAL journal mode (got {mode})"
        )));
    }
    Ok(conn)
}

/// 启动恢复第一阶段返回的快照。
///
/// `recover` 只在启动 `IMMEDIATE` 事务中检测旧 run、结束未闭合的采集区间并冻结
/// 请求 id 水位；它不会在这个事务里同步扫描全部请求。因此当前实现把
/// `costs_settled` 和 `interrupted_requests` 返回为 0，真正的行级修复由
/// `RecoveryScan` 在启动后分批执行。时间字段单位为 Unix epoch 毫秒；`Default`
/// 表示没有发现旧 run、没有冻结上界且未检测到 unclean。
#[derive(Clone, Debug, Default)]
pub struct Recovery {
    /// 启动事务本次已结算的费用行数；当前实现固定为 0，后续扫描另行报告。
    pub costs_settled: i64,
    /// 本次启动发现并标记为 `stop_reason='unclean'` 的旧 run 数。
    pub unclean_runs: i64,
    /// 启动事务内改写成 `interrupted` 的请求数；当前实现固定为 0，交给扫描阶段。
    pub interrupted_requests: i64,
    /// 启动事务提交前观察到的最大 `usage_requests.id`，作为本次恢复扫描的冻结上界。
    /// 大于它的行属于启动后新请求，不在该扫描范围内。
    pub as_of_id: i64,
    /// 是否检测到旧进程留下未关闭 run。该标志发布给 health/report 判断损失范围；
    /// 当前 `scan_recovery_batch` 固定以 `unclean=true` 调用 `repair_one`，因此行级修复
    /// 原因不由这个布尔值逐行选择。它也不表示历史已经修复完成。
    pub unclean_detected: bool,
}

/// 创建或升级 schema，不删除既有数据库。
///
/// 调用者是 `Writer::open`，发生在唯一写连接建立之后。版本高于本构建时拒绝；
/// 已有非空数据库在升级前通过 SQLite backup API 覆盖写
/// `usage-backup.sqlite3`，随后按版本号逐步骤补齐。每个 `apply_step` 把 SQL 和新的
/// `user_version` 放在同一个事务中，失败时该步回滚。v2、v7、v11 的表重建需要 SQLite
/// 在事务外关闭外键；代码在 `apply_step` 成功或失败后都会执行恢复外键的 PRAGMA，
/// 若恢复语句自身失败，打开流程仍返回错误。迁移不是“打开文件即自动重建”，也不能把
/// 中途已提交的较早版本误认为整个升级失败后完全没发生变化。
pub fn migrate(conn: &mut Connection, dir: &Path) -> Result<(), DbError> {
    let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if version > SCHEMA_VERSION {
        return Err(DbError::refused(error(
            crate::stats::dto::ERR_UNSUPPORTED_SCHEMA,
            "database was written by a newer version",
        )));
    }
    if version == SCHEMA_VERSION {
        conn.execute_batch(include_str!("report_indexes.sql"))?;
        return Ok(());
    }
    if version > 0 {
        backup(conn, dir)?;
    }
    // Apply only the steps the database actually still needs. Replaying 0002
    // against an existing v2 database failed with "table stats_model_prices
    // already exists" and left collection disabled.
    if version == 0 {
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute_batch(MIGRATION_0001)?;
        tx.pragma_update(None, "user_version", 1)?;
        tx.commit()?;
    }
    // v1 -> v2 rebuilds tables, which SQLite only allows with foreign keys
    // disabled outside the transaction; the copy is verified afterwards.
    if version < 2 {
        conn.pragma_update(None, "foreign_keys", "OFF")?;
        let upgraded = apply_step(conn, 2, MIGRATION_0002);
        conn.pragma_update(None, "foreign_keys", "ON")?;
        upgraded?;
    }
    if version < 3 {
        apply_step(conn, 3, MIGRATION_0003)?;
    }
    if version < 4 {
        apply_step(conn, 4, MIGRATION_0004)?;
    }
    if version < 5 {
        apply_step(conn, 5, MIGRATION_0005)?;
    }
    if version < 6 {
        apply_step(conn, 6, MIGRATION_0006)?;
    }
    // v7 rebuilds `usage_requests`, which `usage_costs` references with
    // ON DELETE CASCADE: foreign keys must be disabled outside the transaction
    // and are restored on both the success and the failure path, so a refused
    // upgrade can never leave the connection with checks silently off.
    if version < 7 {
        conn.pragma_update(None, "foreign_keys", "OFF")?;
        let upgraded = apply_step(conn, 7, MIGRATION_0007);
        conn.pragma_update(None, "foreign_keys", "ON")?;
        upgraded?;
    }
    if version < 8 {
        conn.pragma_update(None, "foreign_keys", "ON")?;
        apply_step(conn, 8, MIGRATION_0008)?;
    }
    if version < 9 {
        apply_step(conn, 9, MIGRATION_0009)?;
    }
    if version < 10 {
        apply_step(conn, 10, MIGRATION_0010)?;
    }
    if version < 11 {
        conn.pragma_update(None, "foreign_keys", "OFF")?;
        let upgraded = apply_step(conn, 11, MIGRATION_0011);
        conn.pragma_update(None, "foreign_keys", "ON")?;
        upgraded?;
    }
    conn.execute_batch(include_str!("report_indexes.sql"))?;
    Ok(())
}

/// 在独立 `IMMEDIATE` 事务中执行一个迁移步骤，并原子记录目标版本。
///
/// `conn` 必须是启动时独占的写连接；迁移 SQL 与 `user_version` 一起提交，失败则
/// 这一步的表结构、数据和版本号回滚。事务提交前还会运行 `foreign_key_check`。
/// `Transaction<'_>` 中的 `'_` 借用了连接，事务对象存活期间连接不能被另一个作用域
/// 可变借用，这由编译器强制，不是运行期连接池锁。
fn apply_step(conn: &mut Connection, to_version: i64, sql: &str) -> Result<(), DbError> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    tx.execute_batch(sql)?;
    // Nothing may reference a missing request after a table rebuild.
    let dangling: i64 = {
        let mut statement = tx.prepare("PRAGMA foreign_key_check")?;
        let mut rows = statement.query([])?;
        i64::from(rows.next()?.is_some())
    };
    if dangling != 0 {
        return Err(DbError::refused(error(
            crate::stats::dto::ERR_UNAVAILABLE,
            "migration left a dangling reference",
        )));
    }
    tx.pragma_update(None, "user_version", to_version)?;
    tx.commit()?;
    Ok(())
}

/// 在升级已有数据库前，用 SQLite backup API 覆盖本目录唯一的回滚副本。
///
/// `target` 是 `usage-backup.sqlite3`；本函数会先尝试移除旧副本，再完整复制。
/// 返回成功只表示备份文件已写完，不表示后续迁移一定成功。
fn backup(conn: &Connection, dir: &Path) -> Result<(), DbError> {
    let target = dir.join("usage-backup.sqlite3");
    let _ = std::fs::remove_file(&target);
    let mut destination = Connection::open(&target)?;
    let backup = rusqlite::backup::Backup::new(conn, &mut destination)?;
    backup.run_to_completion(64, Duration::from_millis(5), None)?;
    drop(backup);
    crate::stats::restrict_permissions(&target);
    Ok(())
}

/// 执行启动恢复的短事务阶段：标记旧 run，关闭区间，并冻结请求扫描水位。
///
/// 它不会直接关闭所有 `in_flight` 请求，也不会在本事务中做无界修复。`as_of_id`
/// 是本次恢复允许触及的最大行 id；实际请求/费用收敛由之后有界的
/// `scan_recovery_batch` 完成。调用者必须在同一启动事务里再执行 `start_run`，
/// 两者一起提交或一起回滚，保证打开失败不会吞掉上一次崩溃证据。
fn recover(tx: &rusqlite::Transaction<'_>, run_id: &str, now_ms: i64) -> Result<Recovery, DbError> {
    let unclean_runs = tx.execute(
        "UPDATE stats_runs SET stopped_at_ms=:now, stop_reason='unclean'
         WHERE run_id<>:run AND stopped_at_ms IS NULL AND started_at_ms<=:now",
        rusqlite::named_params! {":now": now_ms, ":run": run_id},
    )? as i64;
    tx.execute(
        "UPDATE stats_collection_intervals SET ended_at_ms=:now, state='unknown', reason='unclean_exit'
         WHERE ended_at_ms IS NULL AND started_at_ms<=:now",
        rusqlite::named_params! {":now": now_ms},
    )?;
    // 修复本身由 writer 后续持有并执行为有界扫描。若在这里同步扫全库，会让启动
    // IMMEDIATE 事务和代理可用时间随历史数据量无界增长；此阶段只冻结上界。
    let as_of_id: i64 = tx.query_row(
        "SELECT COALESCE(MAX(id), 0) FROM usage_requests",
        [],
        |row| row.get(0),
    )?;
    let costs_settled = 0i64;
    Ok(Recovery {
        unclean_runs,
        interrupted_requests: 0,
        costs_settled,
        as_of_id,
        unclean_detected: unclean_runs > 0,
    })
}

/// 在启动事务中登记本次进程 run，并打开对应采集区间。
///
/// `run_id` 来自 `stats::Inner`；`enabled` 来自已提交设置。区间状态是
/// `collecting` 或 `paused`，后者原因记为 `user_disabled`。时间参数均为 epoch 毫秒。
/// 本函数不自行提交，调用者把它与 `recover` 合并为一个启动事务。
fn start_run(
    tx: &rusqlite::Transaction<'_>,
    run_id: &str,
    now_ms: i64,
    enabled: bool,
) -> Result<(), DbError> {
    tx.execute(
        "INSERT INTO stats_runs(run_id, started_at_ms, last_persisted_at_ms)
         VALUES(:run, :now, :now) ON CONFLICT(run_id) DO NOTHING",
        rusqlite::named_params! {":run": run_id, ":now": now_ms},
    )?;
    tx.execute(
        "INSERT INTO stats_collection_intervals(run_id, started_at_ms, state, reason)
         VALUES(:run, :now, :state, :reason)",
        rusqlite::named_params! {
            ":run": run_id,
            ":now": now_ms,
            ":state": if enabled { "collecting" } else { "paused" },
            ":reason": if enabled { None::<&str> } else { Some("user_disabled") },
        },
    )?;
    Ok(())
}

/// 请求准入路径读取的设置快照。
///
/// `enabled=false` 表示统计关闭；`retention_days=0` 在维护接口中表示关闭自动保留清理。
/// `default_currency` 固定为静态字符串 `"CNY"` 或数据库已知值；`revision` 用于乐观
/// 并发控制。`Default` 为启用统计、保留 180 天、默认币种 `"CNY"`、revision 1。
/// `Settings` 实现 `Copy`，复制的是这个小结构体的值，不会复制数据库或连接。
#[derive(Clone, Copy, Debug)]
pub struct Settings {
    /// 是否接受新统计事件；`true` 为默认。
    pub enabled: bool,
    /// 自动保留天数；0 表示关闭自动保留清理。
    pub retention_days: i64,
    /// Currency applied to models without a configured price.
    pub default_currency: &'static str,
    /// 乐观并发版本；每次设置成功修改后递增。
    pub revision: i64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: true,
            retention_days: crate::stats::dto::DEFAULT_RETENTION_DAYS,
            default_currency: "CNY",
            revision: 1,
        }
    }
}

pub fn load_settings(conn: &Connection) -> Result<Settings, DbError> {
    let settings = conn
        .query_row(
            "SELECT enabled, retention_days, revision, default_currency
             FROM stats_settings WHERE singleton=1",
            [],
            |row| {
                Ok(Settings {
                    enabled: row.get::<_, i64>(0)? != 0,
                    retention_days: row.get(1)?,
                    revision: row.get(2)?,
                    default_currency: currency_static(&row.get::<_, String>(3)?),
                })
            },
        )
        .optional()?;
    Ok(settings.unwrap_or_default())
}

/// 在事件批次之间分步执行的维护任务进度。
///
/// 生命周期接口：任务由 writer 创建，`Maintenance` 暂存于 `Writer`，
/// 每次 `maintenance_step` 只走一个有界批次。`cutoff_ms` 和 `started_at_ms` 是 epoch
/// 毫秒，`as_of_id` 冻结允许触及的行 id，`affected` 初始 0、记录已删除数，`cursor_id`
/// 初始 0、记录下一批扫描起点，`batch` 是单批上限。`filter` 只保留在内存任务对象中供
/// 审计意图使用；当前 v2 sweep 收到 `Some(filter)` 也不会按它筛选，`None` 表示没有记录
/// 用户筛选（现有调用者均传 `None`）。
#[derive(Debug)]
pub struct Maintenance {
    pub task_id: i64,
    pub kind: MaintenanceKind,
    pub cutoff_ms: i64,
    pub as_of_id: i64,
    /// Retained for the audit trail of maintenance records; the v2 sweep does
    /// not filter by it.
    #[allow(dead_code)]
    pub filter: Option<crate::stats::dto::StatsFilter>,
    pub affected: i64,
    pub cursor_id: i64,
    pub started_at_ms: i64,
    pub batch: i64,
}

impl Maintenance {
    pub fn new(
        task_id: i64,
        kind: MaintenanceKind,
        cutoff_ms: i64,
        as_of_id: i64,
        filter: Option<crate::stats::dto::StatsFilter>,
        started_at_ms: i64,
        batch: i64,
    ) -> Self {
        Self {
            task_id,
            kind,
            cutoff_ms,
            as_of_id,
            filter,
            affected: 0,
            cursor_id: 0,
            started_at_ms,
            batch,
        }
    }
}

/// 唯一写连接及其线程内状态；由 `stats-writer` 线程可变借用并串行调用。
///
/// 普通批次走一个事务，坏行隔离时每个事件另起事务，所以不能把“单 writer”误解为
/// “一次 `apply_batch` 永远只有一个事务”。本结构没有 `Drop` 关闭协议；通常的
/// `Shutdown` 控制消息会让 writer 在队列仍接收已预约事件时进入 `stats::flush_and_stop`，
/// 先限时排水，最后关闭队列；队列被直接关闭或控制通道断开只是后备触发路径。
/// `flush_and_stop` 还会持久化 counters、标记 run/collection interval 结束并尝试 PASSIVE
/// checkpoint。任一关闭步骤失败可能留下 unclean 证据，下次启动由 `recover` 识别。
pub struct Writer {
    /// 独占写连接；只在 writer 线程使用，字段公开用于同模块控制流程。
    pub conn: Connection,
    /// 已提交价格表的内存发布副本，供请求准入冻结价格而不读 SQLite。
    pub prices: PublishedPrices,
    /// 启动恢复扫描游标；构造 `Writer` 时为 `None`，`open` 提交启动事务后设为
    /// `Some`，即使 `finished()` 为真也保留已完成的游标供检查。
    pub scan: Option<RecoveryScan>,
    /// 当前进程 run 的稳定标识，用于 run、区间和失败记录关联。
    pub run_id: String,
    /// 最近一次数据库设置快照。
    pub settings: Settings,
    /// 当前至多一个分步维护任务；本文件不展开定价、导入和清理算法。
    pub maintenance: Option<Maintenance>,
    /// 已加载的 provider_id 到 usage mode 映射，控制请求是否向上游要求 usage。
    pub provider_modes: std::collections::HashMap<String, UsageMode>,
    /// 与请求路径共享的原子 counters；克隆 `Arc` 只增加引用计数，批次成功时镜像到
    /// `stats_runs`。它不是事务日志，也不保证每个即时计数都已落库。
    pub counters: std::sync::Arc<crate::stats::event::Counters>,
    /// `stats_health` 使用的全表事实缓存，需显式 `refresh_db_stats` 更新。
    pub db_stats: DbStats,
}

/// 一次写入调用、其内部重试和逐事件隔离共享的单调时钟截止点。
///
/// 单位由 `Duration` 表示；`Copy` 只复制截止时刻，不会刷新预算。它约束本进程本次
/// 调用的等待时间，不等同于 SQLite 事务超时，也不保证操作系统 I/O 在截止点立即返回。
#[derive(Clone, Copy, Debug)]
pub struct WriteBudget {
    deadline: Instant,
}

impl WriteBudget {
    pub fn new(limit: Duration) -> Self {
        Self {
            deadline: Instant::now() + limit,
        }
    }

    pub fn expired(&self) -> bool {
        Instant::now() >= self.deadline
    }

    fn remaining(&self) -> Duration {
        self.deadline.saturating_duration_since(Instant::now())
    }
}

/// 判断错误是否属于可能自行恢复的锁冲突、I/O 故障或操作中断。
///
/// 瞬时错误会按整批在当前调用预算内重试，不进入逐事件隔离；违反约束等确定性错误才会
/// 触发坏行隔离，避免一个坏事件丢掉健康邻居。
fn is_transient(failure: &DbError) -> bool {
    matches!(
        failure.kind,
        Some(rusqlite::ErrorCode::DatabaseBusy)
            | Some(rusqlite::ErrorCode::DatabaseLocked)
            | Some(rusqlite::ErrorCode::SystemIoFailure)
            | Some(rusqlite::ErrorCode::OperationInterrupted)
    )
}

/// 一次 `apply_batch` 的逐事件结果。
///
/// `Default` 是 `applied=0`、`dropped` 为空。`applied` 表示被成功处理的事件数，不是
/// 新增/修改的数据库行数：例如重复 `Start` 被 `ON CONFLICT DO NOTHING` 处理，仍可能
/// 计入已处理事件。`dropped` 保留失败事件，供调用方统计 request 数并提取可能需要恢复的
/// `Finish` id；它不承诺保留所有恢复证据，调用方之后也确实可能只留下 request id。
#[derive(Clone, Debug, Default)]
pub struct BatchOutcome {
    pub applied: usize,
    pub dropped: Vec<StatsEvent>,
}

impl BatchOutcome {
    pub fn is_complete(&self) -> bool {
        self.dropped.is_empty()
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct DbStats {
    /// 当前请求事实行数；默认 0。
    pub record_count: i64,
    /// 最早请求开始时间（epoch 毫秒）；空库为 `None`。
    pub earliest_record_at_ms: Option<i64>,
    /// 主数据库文件字节数；读取失败时辅助函数返回 0。
    pub db_bytes: i64,
    /// `-wal` 文件字节数；读取失败时辅助函数返回 0。
    pub wal_bytes: i64,
}

impl Writer {
    /// 打开统计库、完成必要的 schema 准备，并执行一次启动恢复事务。
    ///
    /// 调用者是 `stats::writer_thread`。`dir` 是统计数据目录，数据库固定为其中的
    /// `usage.sqlite3`；`run_id` 标识本次进程运行；`counters` 与请求路径共享，
    /// 这里只是克隆 `Arc`。返回值中的 `Writer` 从此由该线程独占并传给后续批次，
    /// `Recovery` 则只携带启动第一阶段的摘要和冻结水位。
    ///
    /// 执行顺序是：打开唯一写连接、迁移、读取设置/provider mode/价格/健康事实，然后
    /// 开始一个 `IMMEDIATE` 启动事务，在事务中先 `recover` 再 `start_run`。这两步一起
    /// 提交，失败时事务对象被丢弃并回滚，不会消费旧 run 的未闭合证据，也不会只提交
    /// 本次 run 或采集区间。此前已成功提交的 schema 迁移不会因这个事务失败而倒退。
    ///
    /// 提交后只设置 `scan=RecoveryScan::new(recovery.as_of_id)`；此时历史请求尚未全部
    /// 修复。writer 主循环稍后会分批推进扫描，并在扫描完成前向 health 暴露恢复未完成。
    pub fn open(
        dir: &Path,
        run_id: &str,
        counters: std::sync::Arc<crate::stats::event::Counters>,
    ) -> Result<(Self, Recovery), DbError> {
        let path = dir.join("usage.sqlite3");
        let mut conn = open_connection(&path, false)?;
        crate::stats::restrict_permissions(&path);
        migrate(&mut conn, dir)?;
        let settings = load_settings(&conn)?;
        let provider_modes = load_provider_modes(&conn)?;
        let mut writer = Self {
            conn,
            prices: PublishedPrices::default(),
            scan: None,
            run_id: run_id.to_owned(),
            settings,
            maintenance: None,
            provider_modes,
            counters,
            db_stats: DbStats::default(),
        };
        // Load the current prices once at startup; the hot path only ever reads
        // the published copy.
        writer.prices = load_price_cache(&writer.conn)?;
        writer.refresh_db_stats()?;
        // `recover` 和 `start_run` 必须同事务：打开失败时不能先消费旧 crash 证据，
        // 也不能只留下本次 run/区间。提交后不再有会让整个 open 失败的 setup；
        // 已提交的迁移不属于这个启动事务，仍可能保留。
        let now_ms = crate::stats::now_ms();
        let tx = writer
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let recovery = recover(&tx, run_id, now_ms)?;
        start_run(&tx, run_id, now_ms, settings.enabled)?;
        tx.commit()?;
        writer.scan = Some(RecoveryScan::new(recovery.as_of_id));
        Ok((writer, recovery))
    }

    pub fn refresh_db_stats(&mut self) -> Result<(), DbError> {
        let record_count: i64 =
            self.conn
                .query_row("SELECT COUNT(*) FROM usage_requests", [], |row| row.get(0))?;
        let earliest: Option<i64> =
            self.conn
                .query_row("SELECT MIN(started_at_ms) FROM usage_requests", [], |row| {
                    row.get(0)
                })?;
        let (db_bytes, wal_bytes) = self.file_sizes();
        self.db_stats = DbStats {
            record_count,
            earliest_record_at_ms: earliest,
            db_bytes,
            wal_bytes,
        };
        Ok(())
    }

    fn file_sizes(&self) -> (i64, i64) {
        let main = self.conn.path().map(PathBuf::from).unwrap_or_default();
        let size = |path: PathBuf| -> i64 {
            std::fs::metadata(path)
                .map(|meta| meta.len() as i64)
                .unwrap_or(0)
        };
        let wal = PathBuf::from(format!("{}-wal", main.display()));
        (size(main), size(wal))
    }

    /// 在共享截止时间内把一批已从队列取出的统计事件写入 SQLite。
    ///
    /// 调用者是 `stats::writer_thread`；`batch` 来自 `EventQueue::pop_batch`，`budget` 由
    /// 调用者创建并传入。正常路径把整批交给 `apply_batch_once`，在一个 `IMMEDIATE`
    /// 事务内处理 `Start`/`Finish`、请求、费用快照和 run counters。整批遇到锁冲突或
    /// I/O 等瞬时错误时，本函数只在当前预算内有限重试，随后返回 `Err`；它不会把
    /// `batch` 重新放回队列。调用者会标记 degraded，并且只把其中的 `Finish` id 记入
    /// 内存孤儿集或 durable backlog，原始完整 `FinishRecord` 不会自动重放。
    ///
    /// 若首次错误是确定性行错误，则改为逐事件调用 `apply_batch_once`。每个事件各自
    /// 一个事务，成功事件保留，确定性失败事件进入 `BatchOutcome::dropped`；因此不能
    /// 把整个 `apply_batch` 概括成永远只有一个事务，也不能把坏行隔离理解为整批回滚。
    /// 逐事件隔离中再次遇到瞬时错误时，当前实现停止本轮并只会确定地返回某些事件，
    /// 仍未处理的部分进入 `dropped`，不会在当前调用内自动重放。
    ///
    /// 返回 `Ok` 仅表示本函数已完成可跟踪的逐事件处理，不表示每个请求的完整生命周期
    /// 都已持久化；返回 `Err` 表示整批首次事务未提交，或隔离后所有事件都未成功。
    /// 两种失败边界都只依赖调用方的恢复流程，不保证原事件 exactly-once 投递。
    pub fn apply_batch(
        &mut self,
        batch: &[StatsEvent],
        budget: &WriteBudget,
    ) -> Result<BatchOutcome, DbError> {
        if batch.is_empty() {
            return Ok(BatchOutcome::default());
        }
        let mut last_error = None;
        for attempt in 0..=WRITE_RETRIES {
            if budget.expired() {
                break;
            }
            // Shrink the SQLite lock wait to what the budget still allows, so a
            // single blocked statement cannot overshoot the whole batch budget.
            let remaining = budget.remaining();
            let wait = (BUSY_WAIT_MS as u128).min(remaining.as_millis()).max(1) as u64;
            let _ = self.conn.busy_timeout(Duration::from_millis(wait));
            match self.apply_batch_once(batch) {
                Ok(()) => {
                    return Ok(BatchOutcome {
                        applied: batch.len(),
                        dropped: Vec::new(),
                    });
                }
                Err(failure) => {
                    last_error = Some(failure.clone());
                    if !is_transient(&failure) {
                        break;
                    }
                    if attempt < WRITE_RETRIES {
                        let pause = Duration::from_millis(20 * (attempt as u64 + 1));
                        if pause >= budget.remaining() {
                            break;
                        }
                        std::thread::sleep(pause);
                    }
                }
            }
        }
        let Some(last_error) = last_error else {
            return Err(DbError::refused("write budget exhausted".into()));
        };
        if is_transient(&last_error) {
            // 瞬时失败在当前调用内已用完重试预算。调用方不会重放这些已 pop 的
            // 事件；它记录 Finish request id，并由启动扫描/orphan/backlog 收敛中断。
            return Err(last_error);
        }
        // Deterministic failure: isolate the offending events. `index` is the
        // real traversal position: `outcome.applied` only counts successes, so
        // using it as an index skipped or re-counted events after a bad one.
        let mut outcome = BatchOutcome::default();
        let mut index = 0usize;
        while index < batch.len() {
            if budget.expired() {
                outcome.dropped.extend_from_slice(&batch[index..]);
                break;
            }
            let event = &batch[index];
            match self.apply_batch_once(std::slice::from_ref(event)) {
                Ok(()) => outcome.applied += 1,
                Err(failure) => {
                    if is_transient(&failure) {
                        // 数据库在隔离过程中再次瞬时失败；本函数不重放入队，剩余事件
                        // 只能由调用方的 id 级恢复证据尽力收敛。
                        outcome.dropped.extend_from_slice(&batch[index..]);
                        break;
                    }
                    eprintln!(
                        "statistics isolated event failed: {} ({})",
                        failure.message,
                        event.kind()
                    );
                    outcome.dropped.push(event.clone());
                }
            }
            index += 1;
        }
        if outcome.applied == 0 && outcome.dropped.len() == batch.len() {
            return Err(last_error);
        }
        Ok(outcome)
    }

    /// 不等待新流量即可探测写能力的轻量入口。
    ///
    /// 取得 `IMMEDIATE` 写锁后立即 rollback，不修改业务数据。调用者是 writer 空闲/
    /// degraded 恢复循环；成功只证明当下能开始写事务，不代表之后批次一定成功。
    pub fn probe_write(&mut self) -> Result<(), DbError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.rollback()?;
        Ok(())
    }

    /// 把内存原子 counters 显式镜像到当前 run。
    ///
    /// 正常成功批次也会更新同一行；本方法供恢复完成或关闭时处理“之后可能再无请求批次”
    /// 的空闲场景。`now_ms` 单位是 epoch 毫秒；这是 autocommit 的单条 UPDATE，
    /// 不与调用者内存状态组成更强事务。
    pub fn persist_counters(&self, now_ms: i64) -> Result<(), DbError> {
        self.conn.execute(
            "UPDATE stats_runs SET last_persisted_at_ms=?2, admitted_requests=?3,
                dropped_requests=?4, write_failures=?5 WHERE run_id=?1",
            rusqlite::params![
                self.run_id,
                now_ms,
                self.counters.admitted.load(Ordering::Relaxed) as i64,
                self.counters.dropped.load(Ordering::Relaxed) as i64,
                self.counters.write_failures.load(Ordering::Relaxed) as i64
            ],
        )?;
        Ok(())
    }

    /// Records a `degraded` collection window (`database_error`).
    pub fn enter_degraded(&mut self, now_ms: i64) -> Result<(), DbError> {
        self.open_interval(now_ms, "degraded", Some("database_error"))
    }

    /// Backfills a degraded window that could not be recorded while the
    /// database was unavailable: the covering interval is cut at `from_ms`, the
    /// degraded span is inserted and a fresh collecting interval opens at
    /// `to_ms` so the gap is never reported as collecting.
    pub fn mark_degraded_window(&mut self, from_ms: i64, to_ms: i64) -> Result<(), DbError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "UPDATE stats_collection_intervals SET ended_at_ms=:from
             WHERE ended_at_ms IS NULL AND run_id=:run AND started_at_ms<=:from",
            rusqlite::named_params! {":from": from_ms, ":run": self.run_id},
        )?;
        tx.execute(
            "INSERT INTO stats_collection_intervals(run_id, started_at_ms, ended_at_ms, state, reason)
             VALUES(:run, :from, :to, 'degraded', 'database_error')",
            rusqlite::named_params! {":from": from_ms, ":to": to_ms.max(from_ms), ":run": self.run_id},
        )?;
        tx.execute(
            "INSERT INTO stats_collection_intervals(run_id, started_at_ms, state)
             VALUES(:run, :to, 'collecting')",
            rusqlite::named_params! {":to": to_ms, ":run": self.run_id},
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Records that collection resumed after a degraded window.
    pub fn leave_degraded(&mut self, now_ms: i64) -> Result<(), DbError> {
        self.open_interval(now_ms, "collecting", None)
    }

    /// Closes the open interval and opens a new one with the given state.
    fn open_interval(
        &mut self,
        now_ms: i64,
        state: &str,
        reason: Option<&str>,
    ) -> Result<(), DbError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "UPDATE stats_collection_intervals SET ended_at_ms=:now
             WHERE ended_at_ms IS NULL AND run_id=:run AND started_at_ms<=:now",
            rusqlite::named_params! {":now": now_ms, ":run": self.run_id},
        )?;
        tx.execute(
            "INSERT INTO stats_collection_intervals(run_id, started_at_ms, state, reason)
             VALUES(:run, :now, :state, :reason)",
            rusqlite::named_params! {
                ":run": self.run_id,
                ":now": now_ms,
                ":state": state,
                ":reason": reason,
            },
        )?;
        tx.commit()?;
        Ok(())
    }

    /// 把指定 id 中仍为 `in_flight` 的请求收敛为可信的“未知中断”终态。
    ///
    /// 调用者是 writer 线程的 `trim_orphans`。输入只有 request id，没有原始
    /// `FinishRecord`；因此不能恢复真实 outcome、token 或时延。函数在一个事务内更新
    /// 仍为 `in_flight` 的请求，并按 Start 时保存的价格快照尝试结算费用。已经是终态的
    /// 请求不会在这里改动，即使其费用行仍 pending；这种旧数据由 `repair_one` 覆盖。
    /// 返回值是本调用实际从 `in_flight` 转为中断的请求数；它不是“费用行已经终态”的
    /// 数量。`legacy_unknown` 或费用快照无法结算时，请求仍可能已转中断而费用保持原状，
    /// 调用者不能仅凭该返回值推断费用生命周期也已完整收敛。
    pub fn settle_orphans(&mut self, ids: &[String], now_ms: i64) -> Result<usize, DbError> {
        if ids.is_empty() {
            return Ok(0);
        }
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut settled = 0usize;
        for id in ids {
            let changed = tx.execute(
                "UPDATE usage_requests SET outcome='interrupted', error_kind='other',
                     ended_at_ms=:now, updated_at_ms=:now,
                     usage_status='missing', usage_reason=NULL, usage_source='none'
                 WHERE request_id=:id AND outcome='in_flight'",
                rusqlite::named_params! {":now": now_ms, ":id": id},
            )?;
            if changed == 1 {
                // Close the request and its cost in one transaction, from the
                // snapshot captured when it was admitted.
                settle_abandoned_cost(&tx, id, now_ms)?;
                settled += 1;
            }
        }
        tx.commit()?;
        Ok(settled)
    }

    /// 在一个 `IMMEDIATE` 事务中顺序应用一条或多条事件。
    ///
    /// 整批成功时，同一批次内的 `Start`、`Finish`、请求行、费用快照和 run counters
    /// 一起提交；任一 `?` 返回错误则事务未提交，析构时回滚。若 `Start` 和 `Finish`
    /// 分属不同批次，它们自然属于不同事务：前一批可先持久化，后一批仍可能失败。
    ///
    /// 请求和费用以 `request_id` 唯一约束做幂等门禁：重复 `Start` 不新增请求行；重复
    /// `Finish` 因请求已不再 `in_flight` 而直接 no-op。正常费用结算只更新 pending 行，
    /// 没有 pending 行时另有带 `ON CONFLICT DO NOTHING` 的兜底插入。相同 request id 因而
    /// 不会重复计入两条结果，但批次失败仍可能丢失具体 Finish 载荷，不是外部事件的
    /// exactly-once 持久化协议。
    fn apply_batch_once(&mut self, batch: &[StatsEvent]) -> Result<(), DbError> {
        if batch.is_empty() {
            return Ok(());
        }
        let now_ms = crate::stats::now_ms();
        let admitted = self.counters.admitted.load(Ordering::Relaxed) as i64;
        let dropped = self.counters.dropped.load(Ordering::Relaxed) as i64;
        let failures = self.counters.write_failures.load(Ordering::Relaxed) as i64;
        let run_id = self.run_id.clone();
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        for event in batch {
            match event {
                StatsEvent::Start(meta) => {
                    tx.execute(
                        "INSERT INTO usage_requests(
                            request_id, run_id, source, provider_id, route_id, provider_name,
                            route_name, requested_model, upstream_model, started_at_ms, stream,
                            created_at_ms, updated_at_ms, outcome, usage_status, usage_source,upstream_protocol)
                         VALUES(:request_id,:run_id,:source,:provider_id,:route_id,:provider_name,
                            :route_name,:requested_model,:upstream_model,:started_at,:stream,
                            -- `created_at_ms` is the request's own start time, not the
                            -- batch time: a fast request can put Start and Finish in the
                            -- same batch, and the schema requires
                            -- `updated_at_ms >= created_at_ms`.
                            :started_at,:now,'in_flight','missing','none',:protocol)
                         ON CONFLICT(request_id) DO NOTHING",
                        rusqlite::named_params! {
                            ":request_id": meta.request_id,
                            ":run_id": meta.run_id,
                            ":source": meta.source.as_str(),
                            ":provider_id": meta.provider_id,
                            ":route_id": meta.route_id,
                            ":provider_name": meta.provider_name,
                            ":route_name": meta.route_name,
                            ":requested_model": meta.requested_model,
                            ":upstream_model": meta.upstream_model,
                            ":protocol":meta.upstream_protocol,
                            ":started_at": meta.started_at_ms,
                            ":stream": if meta.stream { 1 } else { 0 },
                            ":now": now_ms,
                        },
                    )?;
                    if let Some(identity) = meta.workbuddy_identity.as_ref() {
                        crate::stats::workbuddy_import::store::record_proxy_identity(
                            &tx,
                            &meta.request_id,
                            identity,
                            now_ms,
                        )?;
                    }
                    // The request's own price snapshot is part of the same
                    // transaction, so a later edit or delete cannot change it.
                    tx.execute(
                        "INSERT INTO usage_costs(
                             request_id, source_price_id, source_price_revision, price_source,
                             price_snapshot_at_ms, currency, input_mode, input_rate_nanos,
                             output_rate_nanos, cache_read_rate_nanos, cache_write_rate_nanos,
                             status, pricing_version, calculated_at_ms)
                         VALUES(:request_id,:source_id,:source_revision,:price_source,
                             :snapshot_at,:currency,:mode,:input,:output,:read,:write,
                             'pending',2,:now)
                         ON CONFLICT(request_id) DO NOTHING",
                        rusqlite::named_params! {
                            ":request_id": meta.request_id,
                            ":source_id": meta.price.source_price_id,
                            ":source_revision": meta.price.source_price_revision,
                            ":price_source": meta.price.price_source,
                            ":snapshot_at": meta.price.price_snapshot_at_ms,
                            ":currency": meta.price.currency,
                            ":mode": meta.price.input_mode,
                            ":input": meta.price.input_rate_nanos,
                            ":output": meta.price.output_rate_nanos,
                            ":read": meta.price.cache_read_rate_nanos,
                            ":write": meta.price.cache_write_rate_nanos,
                            ":now": now_ms,
                        },
                    )?;
                }
                StatsEvent::Finish {
                    request_id, finish, ..
                } => {
                    finish_request(&tx, request_id, finish, now_ms)?;
                }
            }
        }
        tx.execute(
            "UPDATE stats_runs SET last_persisted_at_ms=:now,
                 admitted_requests=:admitted, dropped_requests=:dropped, write_failures=:failures
             WHERE run_id=:run",
            rusqlite::named_params! {
                ":now": now_ms,
                ":admitted": admitted,
                ":dropped": dropped,
                ":failures": failures,
                ":run": run_id,
            },
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Executes one bounded maintenance step. Returns `true` while work remains.
    pub fn maintenance_step(&mut self) -> Result<bool, DbError> {
        let Some(mut task) = self.maintenance.take() else {
            return Ok(false);
        };
        let now_ms = crate::stats::now_ms();
        let outcome = self.run_maintenance_step(&mut task, now_ms);
        match outcome {
            Ok((_, true)) => {
                self.maintenance = Some(task);
                Ok(true)
            }
            Ok((_, false)) => {
                self.record_maintenance(&task, MaintenanceStatus::Completed, None, now_ms)?;
                Ok(false)
            }
            Err(failure) => {
                self.record_maintenance(
                    &task,
                    MaintenanceStatus::Failed,
                    Some("maintenance_failed"),
                    now_ms,
                )?;
                Err(failure)
            }
        }
    }

    fn run_maintenance_step(
        &mut self,
        task: &mut Maintenance,
        now_ms: i64,
    ) -> Result<(i64, bool), DbError> {
        let _ = now_ms;
        match task.kind {
            // Repricing history with the current price no longer exists: every
            // request keeps the snapshot it was admitted with.
            MaintenanceKind::Reprice | MaintenanceKind::Migration => Ok((0, false)),
            MaintenanceKind::Retention | MaintenanceKind::Clear => self.delete_finished(task),
        }
    }

    /// Counts the finished rows a clear with this watermark and cutoff removes.
    ///
    /// This is the preview the user confirms. The sweep uses **exactly** the same
    /// predicate, so the number previewed is the number deleted — a row that
    /// arrives after the preview has a larger id and can never be in range, and
    /// a late record (`started_at_ms >= cutoff_ms`) is excluded by the cutoff.
    pub fn preview_clear(
        conn: &rusqlite::Connection,
        as_of_id: i64,
        cutoff_ms: i64,
    ) -> Result<i64, DbError> {
        let count = conn.query_row(
            "SELECT COUNT(*) FROM usage_requests
             WHERE outcome<>'in_flight' AND ended_at_ms IS NOT NULL
               AND id<=?1 AND started_at_ms<?2",
            rusqlite::params![as_of_id, cutoff_ms],
            |row| row.get(0),
        )?;
        Ok(count)
    }

    /// True while a maintenance task holds the importer back.
    ///
    /// A clear takes an immediate write transaction, and an import batch takes
    /// its own transaction, so the two are serialised by the database itself;
    /// this flag lets the importer yield *before* it even tries, instead of
    /// waiting on the write lock.
    pub fn is_maintenance_running(&self) -> bool {
        self.maintenance.is_some()
    }

    /// Deletes finished rows in bounded batches.
    ///
    /// `retention` also applies the retention cutoff, so an automatic sweep is
    /// restricted to rows older than the configured window; a manual clear uses
    /// only the confirmed watermark it was given. `in_flight` rows are never
    /// eligible in either case.
    fn delete_finished(&mut self, task: &mut Maintenance) -> Result<(i64, bool), DbError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        // One eligibility predicate for both the preview and the sweep: finished
        // rows inside the confirmed id watermark and older than the cutoff. A
        // manual clear passes its preview time as the cutoff, so a record that
        // was not counted by the preview can never be deleted.
        if Self::preview_clear(&tx, task.as_of_id, task.cutoff_ms)? == 0 {
            // The preview is empty: there is nothing left in range.
            task.cursor_id = task.as_of_id;
            tx.commit()?;
            return Ok((0, false));
        }
        let mut ids: Vec<i64> = Vec::new();
        {
            let mut statement = tx.prepare(
                "SELECT id FROM usage_requests
                 WHERE outcome<>'in_flight' AND ended_at_ms IS NOT NULL
                   AND id>?1 AND id<=?2 AND started_at_ms<?3
                 ORDER BY id LIMIT ?4",
            )?;
            let mut rows = statement.query(rusqlite::params![
                task.cursor_id,
                task.as_of_id,
                task.cutoff_ms,
                task.batch.max(1)
            ])?;
            while let Some(row) = rows.next()? {
                ids.push(row.get(0)?);
            }
        }
        if ids.is_empty() {
            // Nothing left in range: the sweep is complete.
            task.cursor_id = task.as_of_id;
            tx.commit()?;
            return Ok((0, false));
        }
        // The cursor only advances to the last row this batch actually scanned;
        // taking the table MAX would skip every remaining candidate.
        let last_scanned = *ids.last().expect("non-empty");
        let full_batch = ids.len() as i64 >= task.batch.max(1);
        let mut deleted = 0i64;
        // An imported row that is about to be deleted leaves a tombstone behind:
        // its ledger entry moves off `imported` **before** the fact row goes, so
        // a later rescan of the same message finds the tombstone and cannot
        // resurrect what a clear removed. Proxy rows have no ledger entry, so
        // their original clearing semantics are untouched.
        let tombstone = match task.kind {
            MaintenanceKind::Clear => Some("deleted_manual"),
            // A retention sweep leaves its own tombstone: shortening the window
            // and lengthening it again must not restore what it removed.
            MaintenanceKind::Retention => Some("deleted_retention"),
            _ => None,
        };
        for chunk in ids.chunks(256) {
            let placeholders = chunk.iter().map(|_| "?").collect::<Vec<_>>().join(",");
            if let Some(state) = tombstone {
                let sql = format!(
                    "UPDATE workbuddy_import_ledger
                     SET state = ?1, request_id = NULL, updated_at_ms = updated_at_ms
                     WHERE state = 'imported'
                       AND request_id IN (SELECT request_id FROM usage_requests
                                          WHERE id IN ({placeholders}))"
                );
                let mut params: Vec<rusqlite::types::Value> =
                    vec![rusqlite::types::Value::Text(state.to_owned())];
                params.extend(chunk.iter().map(|id| rusqlite::types::Value::Integer(*id)));
                tx.execute(&sql, rusqlite::params_from_iter(params))?;
            }
            let sql = format!("DELETE FROM usage_requests WHERE id IN ({placeholders})");
            deleted += tx.execute(&sql, rusqlite::params_from_iter(chunk.iter()))? as i64;
        }
        task.cursor_id = last_scanned;
        task.affected += deleted;
        tx.commit()?;
        Ok((deleted, full_batch))
    }

    fn record_maintenance(
        &mut self,
        task: &Maintenance,
        status: MaintenanceStatus,
        error_code: Option<&str>,
        now_ms: i64,
    ) -> Result<(), DbError> {
        self.conn.execute(
            "UPDATE stats_maintenance_events SET status=:status, completed_at_ms=:now,
                 affected_rows=:affected, error_code=:error
             WHERE id=:id",
            rusqlite::named_params! {
                ":status": status.as_str(),
                ":now": now_ms.max(task.started_at_ms),
                ":affected": task.affected,
                ":error": error_code,
                ":id": task.task_id,
            },
        )?;
        self.refresh_db_stats()?;
        Ok(())
    }

    /// Starts an automatic retention sweep when one is due (at most daily) and
    /// no other maintenance task is running. Only finished rows are eligible.
    pub fn maybe_start_retention(&mut self, now_ms: i64) -> Result<bool, DbError> {
        if self.is_maintenance_running() || self.settings.retention_days <= 0 {
            return Ok(false);
        }
        let last: Option<i64> = self.conn.query_row(
            "SELECT MAX(started_at_ms) FROM stats_maintenance_events WHERE kind='retention'",
            [],
            |row| row.get(0),
        )?;
        if last.is_some_and(|last| now_ms - last < 86_400_000) {
            return Ok(false);
        }
        let as_of_id: i64 = self.conn.query_row(
            "SELECT COALESCE(MAX(id),0) FROM usage_requests",
            [],
            |row| row.get(0),
        )?;
        let cutoff = now_ms - self.settings.retention_days * 86_400_000;
        let task_id: i64 = self.conn.query_row(
            "INSERT INTO stats_maintenance_events(kind, started_at_ms, status, affected_rows)
             VALUES('retention', ?1, 'running', 0) RETURNING id",
            [now_ms],
            |row| row.get(0),
        )?;
        self.maintenance = Some(Maintenance::new(
            task_id,
            MaintenanceKind::Retention,
            cutoff,
            as_of_id,
            None,
            now_ms,
            crate::stats::dto::MAINTENANCE_BATCH,
        ));
        Ok(true)
    }

    // ---- control commands -------------------------------------------------

    pub fn update_settings(
        &mut self,
        enabled: bool,
        retention_days: i64,
        default_currency: Option<&str>,
        expected_revision: i64,
        now_ms: i64,
    ) -> Result<Settings, DbError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let changed = tx.execute(
            "UPDATE stats_settings SET enabled=:enabled, retention_days=:retention,
                 default_currency=COALESCE(:currency, default_currency),
                 updated_at_ms=:now, revision=revision+1
             WHERE singleton=1 AND revision=:expected",
            rusqlite::named_params! {
                ":enabled": if enabled { 1 } else { 0 },
                ":retention": retention_days,
                ":currency": default_currency,
                ":now": now_ms,
                ":expected": expected_revision,
            },
        )?;
        if changed != 1 {
            return Err(DbError::refused(error(
                ERR_REVISION_CONFLICT,
                "statistics settings changed; reload and retry",
            )));
        }
        let settings = load_settings(&tx)?;
        if settings.enabled != self.settings.enabled {
            let state = if settings.enabled {
                "collecting"
            } else {
                "paused"
            };
            let reason = if settings.enabled {
                None
            } else {
                Some("user_disabled")
            };
            tx.execute(
                "UPDATE stats_collection_intervals SET ended_at_ms=:now
                 WHERE ended_at_ms IS NULL AND run_id=:run",
                rusqlite::named_params! {":now": now_ms, ":run": self.run_id},
            )?;
            tx.execute(
                "INSERT INTO stats_collection_intervals(run_id, started_at_ms, state, reason)
                 VALUES(:run, :now, :state, :reason)",
                rusqlite::named_params! {
                    ":run": self.run_id,
                    ":now": now_ms,
                    ":state": state,
                    ":reason": reason,
                },
            )?;
        }
        if self.settings.enabled && !settings.enabled {
            super::workbuddy_import::observation::suspend_history(&tx, now_ms).map_err(|_| {
                DbError::refused(error(ERR_UNAVAILABLE, "cannot suspend import history"))
            })?;
        }
        tx.commit()?;
        self.settings = settings;
        Ok(settings)
    }

    pub fn load_provider_mode(&self, provider_id: &str) -> (UsageMode, i64) {
        match self
            .conn
            .query_row(
                "SELECT usage_mode, revision FROM stats_provider_settings WHERE provider_id=?1",
                [provider_id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
            )
            .optional()
        {
            Ok(Some((mode, revision))) => (UsageMode::parse(&mode).unwrap_or_default(), revision),
            _ => (UsageMode::default(), 0),
        }
    }

    pub fn update_provider_mode(
        &mut self,
        provider_id: &str,
        usage_mode: UsageMode,
        expected_revision: i64,
        now_ms: i64,
    ) -> Result<i64, DbError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current: Option<i64> = tx
            .query_row(
                "SELECT revision FROM stats_provider_settings WHERE provider_id=?1",
                [provider_id],
                |row| row.get(0),
            )
            .optional()?;
        let new_revision = match current {
            None => {
                if expected_revision != 0 {
                    return Err(DbError::refused(error(
                        ERR_REVISION_CONFLICT,
                        "provider settings changed; reload and retry",
                    )));
                }
                tx.execute(
                    "INSERT INTO stats_provider_settings(provider_id, usage_mode, updated_at_ms, revision)
                     VALUES(?1, ?2, ?3, 1)",
                    rusqlite::params![provider_id, usage_mode.as_str(), now_ms],
                )?;
                1
            }
            Some(revision) => {
                if revision != expected_revision {
                    return Err(DbError::refused(error(
                        ERR_REVISION_CONFLICT,
                        "provider settings changed; reload and retry",
                    )));
                }
                let changed = tx.execute(
                    "UPDATE stats_provider_settings SET usage_mode=?2, updated_at_ms=?3, revision=revision+1
                     WHERE provider_id=?1 AND revision=?4",
                    rusqlite::params![provider_id, usage_mode.as_str(), now_ms, expected_revision],
                )?;
                if changed != 1 {
                    return Err(DbError::refused(error(
                        ERR_REVISION_CONFLICT,
                        "provider settings changed; reload and retry",
                    )));
                }
                revision + 1
            }
        };
        tx.commit()?;
        self.provider_modes
            .insert(provider_id.to_owned(), usage_mode);
        Ok(new_revision)
    }

    pub fn insert_maintenance(
        &mut self,
        kind: MaintenanceKind,
        now_ms: i64,
    ) -> Result<i64, DbError> {
        self.conn.execute(
            "INSERT INTO stats_maintenance_events(kind, started_at_ms, status, affected_rows)
             VALUES(?1, ?2, 'running', 0)",
            rusqlite::params![kind.as_str(), now_ms],
        )?;
        Ok(self.conn.last_insert_rowid())
    }
}

fn load_provider_modes(
    conn: &Connection,
) -> Result<std::collections::HashMap<String, UsageMode>, DbError> {
    let mut statement =
        conn.prepare("SELECT provider_id, usage_mode FROM stats_provider_settings")?;
    let mut rows = statement.query([])?;
    let mut modes = std::collections::HashMap::new();
    while let Some(row) = rows.next()? {
        let provider_id: String = row.get(0)?;
        let mode: String = row.get(1)?;
        modes.insert(provider_id, UsageMode::parse(&mode).unwrap_or_default());
    }
    Ok(modes)
}

/// 请求准入时冻结的一组单价，不是带时间范围的历史价格规则。
///
/// 生产调用者是 `StatsHandle::begin`：请求先通过启用/关闭检查，`snapshot_for` 从进程内
/// 发布缓存按 `(provider_id, upstream_model)` 精确复制本结构并写入 `RequestMeta`，之后
/// `RequestGuard::begin` 才原子预约两个队列容量。预约失败时元数据随本次调用丢弃，也不会
/// 产生 Start 行；预约成功后 `apply_batch_once` 在处理 Start 时把同一份价格写入
/// `usage_costs` 的 pending 行，`finish_request` 再使用这条请求自己的快照结算。之后编辑
/// 或删除当前价格不会回到这条请求上。这个文件中的价格操作运行在 Core 的 stats-writer
/// 线程；准入只读共享缓存，不查 SQLite。
///
/// 费率单位统一是“每 1,000,000 token 的 nano 货币单位”，例如
/// `2_000_000_000` 表示 USD/CNY 2/M token。`currency` 只允许 `USD` 或 `CNY`，
/// `input_mode` 为 `flat`、`read_discount` 或 `disjoint`。`price_snapshot_at_ms` 是
/// 本请求取价的 epoch 毫秒，不是价格规则最后编辑时间；`snapshot_for` 会用本次准入时间
/// 覆盖从规则行读出的旧更新时间。
///
/// `Clone` 会复制本结构，包括 `Option<String>` 中拥有的 price id；这不是 Spring 自动
/// 注入，也不是锁对象克隆。`None` 在 id/revision 上表示快照来自“没有匹配规则”的零价格
/// 默认值，而不是一条可编辑规则。
#[derive(Clone, Debug, PartialEq)]
pub struct PriceSnapshot {
    /// 取价时 `stats_model_prices.price_id` 的追踪副本。
    ///
    /// 仅用于把历史费用关联回当时的配置；费用查询不会 join 当前价格表，所以该配置删除后
    /// 历史行仍可解释。显式零默认快照没有来源规则，因此为 `None`。
    pub source_price_id: Option<String>,
    /// 取价时来源价格行的乐观并发 revision；显式零默认快照为 `None`。
    pub source_price_revision: Option<i64>,
    /// 持久化来源类别：普通价格表快照写 `"manual"`，未命中时写 `"default_zero"`。
    ///
    /// `"manual"` 是既有 wire/schema 值，覆盖人工编辑和目录自动导入的价格；不能据此判断
    /// 价格由谁输入。`"default_zero"` 才表示当前发布缓存没有该 provider/model 的规则。
    pub price_source: &'static str,
    /// 该快照使用的计价币种：`USD` 或 `CNY`；不在此处做汇率换算。
    pub currency: &'static str,
    /// 输入 token 的计价方式：`flat`、`read_discount` 或 `disjoint`。
    pub input_mode: &'static str,
    /// 普通输入费率，单位 nano 货币单位/1,000,000 token。
    pub input_rate_nanos: i64,
    /// 输出费率，单位 nano 货币单位/1,000,000 token。
    pub output_rate_nanos: i64,
    /// cache read 费率，单位 nano 货币单位/1,000,000 token；0 也是显式已配置的零价。
    pub cache_read_rate_nanos: i64,
    /// cache write 费率，单位 nano 货币单位/1,000,000 token；0 也是显式已配置的零价。
    pub cache_write_rate_nanos: i64,
    /// 本请求准入取价时的 epoch 毫秒。
    pub price_snapshot_at_ms: i64,
}

impl PriceSnapshot {
    /// 发布缓存没有匹配 `(provider_id, upstream_model)` 时采用的政策性零价快照。
    ///
    /// `now_ms` 是本请求准入时间，写入 `price_snapshot_at_ms`。该类别走独立的结算分支：
    /// 上游从未开始时记为 `not_applicable`，已开始但没有完整 usage 时也会按明确配置的
    /// 默认零价记 `default_zero`。它与价格表中“所有费率恰好配置为 0”的 `manual` 快照
    /// 不同；后者仍要求 Complete usage 后才按普通规则计算为 0。
    pub fn default_zero(currency: &'static str, now_ms: i64) -> Self {
        Self {
            source_price_id: None,
            source_price_revision: None,
            price_source: "default_zero",
            currency,
            input_mode: "flat",
            input_rate_nanos: 0,
            output_rate_nanos: 0,
            cache_read_rate_nanos: 0,
            cache_write_rate_nanos: 0,
            price_snapshot_at_ms: now_ms,
        }
    }
}

/// 请求路径共享的“已发布价格”句柄，由 `Arc<RwLock<PublishedPrices>>` 组成。
///
/// `Arc` 让 Core 请求准入与控制循环持有同一对象的引用；`RwLock` 让准入短暂取得读锁后
/// 克隆一个 `PriceSnapshot`，控制循环在完整 reload 后用写锁替换整张发布表。生产准入再次
/// 依赖该缓存而不访问 SQLite。它不等同于 Spring 单例 Bean，也没有跨进程共享含义；
/// `clone` 这里只增加引用计数。
pub type PriceCache = Arc<std::sync::RwLock<PublishedPrices>>;

/// 当前价格表的一份可发布内存投影，按 provider id 与上游模型精确索引。
///
/// 该结构由 `load_price_cache` 从全部 `stats_model_prices` 行重建。保存、删除或目录导入
/// 的功能函数先提交 SQLite，再 reload；外层 stats 控制循环在功能函数及其返回查询都成功后
/// 才把新副本发布到 [`PriceCache`]。因此这是“最近一次成功加载并发布的副本”，不是与
/// SQLite 联合原子的保证。
#[derive(Clone, Debug)]
pub struct PublishedPrices {
    /// 精确匹配键是 `(provider_id, upstream_model)`；路由别名不会在此层二次解析。
    entries: std::collections::HashMap<(String, String), PriceSnapshot>,
    /// 当前设置中的默认币种，仅用于没有匹配价格时构造 `default_zero` 快照。
    pub default_currency: &'static str,
}

impl Default for PublishedPrices {
    fn default() -> Self {
        // The documented initial default; an empty currency would violate the
        // `usage_costs.currency` constraint on the very first request.
        Self {
            entries: std::collections::HashMap::new(),
            default_currency: "CNY",
        }
    }
}

impl PublishedPrices {
    /// 按 `(provider_id, upstream_model)` 查找当前价格；不做模糊模型匹配或时间匹配。
    ///
    /// 返回的是发布表内借用的快照，调用方 `snapshot_for` 会立即克隆并释放读锁。
    pub fn get(&self, provider_id: &str, upstream_model: &str) -> Option<&PriceSnapshot> {
        self.entries
            .get(&(provider_id.to_owned(), upstream_model.to_owned()))
    }
}

fn currency_static(value: &str) -> &'static str {
    if value == "CNY" { "CNY" } else { "USD" }
}

pub fn price_catalog_status(conn: &Connection) -> Result<crate::stats::dto::PriceSyncDto, DbError> {
    Ok(conn.query_row(
        "SELECT last_success_day,updated_at_ms,updated_count,unmatched_count,catalog_count,targets_key
         FROM stats_price_catalog WHERE singleton=1", [], |r| Ok(crate::stats::dto::PriceSyncDto {
            last_success_day: Some(r.get(0)?), updated_at_ms: Some(r.get(1)?),
            updated_count: r.get(2)?, unmatched_count: r.get(3)?, catalog_count: r.get(4)?, targets_key: r.get(5)?,
        }),
    ).optional()?.unwrap_or_default())
}

/// 把 models.dev 价格目录映射到配置中的 provider/model，更新当前价格表。
///
/// 正常调用链是 `Bridge::sync_prices` 决定是否联网，再把命令交给
/// `stats` 控制循环；控制循环在 `stats-writer` 线程调用本函数。
///
/// - `fresh = Some(prices)` 表示刚刚下载到新目录：本轮可覆盖匹配键的现有价格，包括此前
///   人工编辑或目录导入的规则；还会把数据库中已存在、但当前路由列表已不包含的模型加入
///   匹配目标，以便继续刷新这些可编辑规则。
/// - `fresh = None` 表示复用数据库内同日目录缓存（例如路由列表变化后补齐新增模型）：
///   只插入尚不存在的键，不覆盖已有价格；现有规则即使本轮没有匹配项也不会被删除。
///
/// `targets` 来自配置中的 provider id、upstream endpoint 和 model；`day` 是目录下载完成
/// 后的本地 `YYYY-MM-DD`，`targets_key` 是目标列表的哈希摘要，`now` 是 epoch 毫秒。
/// 价格目录中的候选匹配与费率选择由 `price_catalog::Prices::find` 完成；本函数只消费它
/// 返回的 USD 费率。`updated`/`unmatched` 是本轮目录状态计数，不是删除数。
///
/// 价格行和 `stats_price_catalog` 元数据在同一个 SQLite 事务内写入。提交成功后会重新
/// 加载整张发布缓存并读取同步状态，但这两步不在该事务内：如果后者失败，函数仍可能向
/// 调用者返回 `Err`，而价格事务已经提交，且共享发布缓存可能暂时保留旧值。只有本函数及
/// 其返回状态都成功时，外层控制循环才会发布 `writer.prices`；这不构成数据库与内存的
/// 联合原子保证。
pub fn import_price_catalog(
    writer: &mut Writer,
    fresh: Option<crate::price_catalog::Prices>,
    targets: &[crate::price_catalog::Target],
    day: &str,
    targets_key: &str,
    now: i64,
) -> Result<crate::stats::dto::PriceSyncDto, DbError> {
    let overwrite = fresh.is_some();
    let prices = match fresh {
        Some(prices) => prices,
        None => {
            let text: String = writer.conn.query_row(
                "SELECT catalogue_json FROM stats_price_catalog WHERE singleton=1",
                [],
                |r| r.get(0),
            )?;
            serde_json::from_str(&text)
                .map_err(|_| DbError::refused("Invalid cached price catalogue".into()))?
        }
    };
    let mut targets: std::collections::BTreeMap<_, _> = targets
        .iter()
        .map(|target| {
            (
                (target.provider_id.clone(), target.model.clone()),
                target.clone(),
            )
        })
        .collect();
    let upstreams: std::collections::HashMap<_, _> = targets
        .values()
        .map(|target| (target.provider_id.clone(), target.upstream.clone()))
        .collect();
    // 已有价格行即使已从路由列表移除，仍保留其可编辑规则；下载新目录时按数据库中的
    // 精确 provider/model 键继续匹配。这里不会删除不在 targets 中的旧价格。
    if overwrite {
        let mut statement = writer
            .conn
            .prepare("SELECT provider_id,upstream_model FROM stats_model_prices")?;
        for row in
            statement.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
        {
            let (provider_id, model) = row?;
            targets
                .entry((provider_id.clone(), model.clone()))
                .or_insert(crate::price_catalog::Target {
                    upstream: upstreams.get(&provider_id).cloned().unwrap_or_default(),
                    provider_id,
                    model,
                });
        }
    }
    let tx = writer
        .conn
        .transaction_with_behavior(TransactionBehavior::Immediate)?;
    let mut updated = 0i64;
    let mut unmatched = 0i64;
    for target in targets.values() {
        let Some(entry) = prices.find(&target.upstream, &target.model) else {
            unmatched += 1;
            continue;
        };
        let label: String = format!("models.dev · {}/{}", entry.provider, entry.model)
            .chars()
            .take(160)
            .collect();
        updated += tx.execute(
            "INSERT INTO stats_model_prices(price_id,provider_id,upstream_model,currency,input_mode,
             input_rate_nanos,output_rate_nanos,cache_read_rate_nanos,cache_write_rate_nanos,
             label,revision,created_at_ms,updated_at_ms)
             VALUES(?1,?2,?3,'USD',?4,?5,?6,?7,?8,?9,1,?10,?10)
             ON CONFLICT(provider_id,upstream_model) DO UPDATE SET currency='USD',
             input_mode=excluded.input_mode,input_rate_nanos=excluded.input_rate_nanos,
             output_rate_nanos=excluded.output_rate_nanos,cache_read_rate_nanos=excluded.cache_read_rate_nanos,
             cache_write_rate_nanos=excluded.cache_write_rate_nanos,label=excluded.label,
             revision=stats_model_prices.revision+1,updated_at_ms=MAX(stats_model_prices.created_at_ms,excluded.updated_at_ms)
             WHERE ?11=1",
            rusqlite::params![crate::config::token(), target.provider_id, target.model, entry.rates.mode(),
                entry.rates.input, entry.rates.output, entry.rates.read.unwrap_or(0), entry.rates.write.unwrap_or(0),
                label, now, i64::from(overwrite)],
        )? as i64;
    }
    if overwrite {
        let text = serde_json::to_string(&prices)
            .map_err(|_| DbError::refused("Cannot cache price catalogue".into()))?;
        tx.execute(
            "INSERT INTO stats_price_catalog(singleton,catalogue_json,last_success_day,updated_at_ms,
             updated_count,unmatched_count,catalog_count,targets_key) VALUES(1,?1,?2,?3,?4,?5,?6,?7)
             ON CONFLICT(singleton) DO UPDATE SET catalogue_json=excluded.catalogue_json,
             last_success_day=excluded.last_success_day,updated_at_ms=excluded.updated_at_ms,
             updated_count=excluded.updated_count,unmatched_count=excluded.unmatched_count,
             catalog_count=excluded.catalog_count,targets_key=excluded.targets_key",
            rusqlite::params![text, day, now, updated, unmatched, prices.0.len() as i64, targets_key],
        )?;
    } else {
        tx.execute(
            "UPDATE stats_price_catalog SET targets_key=?1 WHERE singleton=1",
            [targets_key],
        )?;
    }
    tx.commit()?;
    // reload 在 commit 之后。这里失败不会撤销上面的价格更新，只会令调用者收到 Err，
    // 并把发布缓存的替换推迟到后续一次成功 reload 或重启。
    writer.prices = load_price_cache(&writer.conn)?;
    price_catalog_status(&writer.conn)
}

/// 从 SQLite 重建一份可发布价格快照；不会自行替换 [`PriceCache`]。
///
/// 调用者包括 `Writer::open` 以及保存、删除、目录导入后的控制循环。函数先读取设置中的
/// 默认币种，再查询 **全部** `stats_model_prices` 行，既不按当前路由过滤，也不按请求时间
/// 选择历史规则。每行投影成一个 `PriceSnapshot`，键为 `(provider_id, upstream_model)`。
///
/// 行内币种和 input mode 会防御性解析为支持值，未知值分别回退 `USD` 和 `flat`。持久化行
/// 的 `price_source` 在这里固定标成 `"manual"`；该类别同时覆盖人工编辑和目录自动导入，
/// 不是“人工输入”的证明。`price_snapshot_at_ms` 此阶段暂为价格行 `updated_at_ms`，真正
/// 请求采用该规则时会被 `snapshot_for` 改为本次准入时间。
///
/// 返回的 `PublishedPrices` 是独立内存值。生产调用者通常把它赋给 `writer.prices`；控制
/// 循环随后再把 writer 中的副本发布给请求线程。本函数返回 `Err` 只说明本次读取失败，
/// 不能据此推断此前某个价格事务是否提交。
pub fn load_price_cache(conn: &Connection) -> Result<PublishedPrices, DbError> {
    let default_currency = currency_static(&conn.query_row(
        "SELECT default_currency FROM stats_settings WHERE singleton=1",
        [],
        |row| row.get::<_, String>(0),
    )?);
    let mut statement = conn.prepare(
        "SELECT price_id, provider_id, upstream_model, currency, input_mode,
                input_rate_nanos, output_rate_nanos, cache_read_rate_nanos,
                cache_write_rate_nanos, revision, updated_at_ms
         FROM stats_model_prices",
    )?;
    let mut rows = statement.query([])?;
    let mut entries = std::collections::HashMap::new();
    while let Some(row) = rows.next()? {
        let snapshot = PriceSnapshot {
            source_price_id: Some(row.get(0)?),
            source_price_revision: Some(row.get(9)?),
            price_source: "manual",
            currency: currency_static(&row.get::<_, String>(3)?),
            input_mode: match row.get::<_, String>(4)?.as_str() {
                "read_discount" => "read_discount",
                "disjoint" => "disjoint",
                _ => "flat",
            },
            input_rate_nanos: row.get(5)?,
            output_rate_nanos: row.get(6)?,
            cache_read_rate_nanos: row.get(7)?,
            cache_write_rate_nanos: row.get(8)?,
            price_snapshot_at_ms: row.get(10)?,
        };
        entries.insert((row.get(1)?, row.get(2)?), snapshot);
    }
    Ok(PublishedPrices {
        entries,
        default_currency,
    })
}

/// 为一次即将准入的请求冻结价格；只读 [`PriceCache`]，绝不查询 SQLite。
///
/// `provider_id` 和 `upstream_model` 来自已经完成认证与路由解析的 `RequestSpec`，
/// `now_ms` 是 `StatsHandle::begin` 取得的请求开始 epoch 毫秒。函数按两个键精确查找；
/// 命中时克隆规则行再覆盖 `price_snapshot_at_ms = now_ms`，所以同一规则服务多个请求时，
/// 每个请求仍保留各自取价时间。未命中时用当前 `default_currency` 构造 `default_zero`。
///
/// 返回拥有所有权的 `PriceSnapshot`，读锁在返回前释放；后续 Start/Finish 不依赖当前价格
/// 配置。这里的 `clone` 不是跨进程复制，也不代表整个应用状态被克隆；它复制该请求需要的
/// 小结构及其中拥有的字符串。
pub fn snapshot_for(
    cache: &PriceCache,
    provider_id: &str,
    upstream_model: &str,
    now_ms: i64,
) -> PriceSnapshot {
    let published = cache.read().unwrap();
    let mut snapshot = published
        .get(provider_id, upstream_model)
        .cloned()
        .unwrap_or_else(|| PriceSnapshot::default_zero(published.default_currency, now_ms));
    // 时间戳记录本请求取价，而不是规则最后编辑；同一配置可服务许多准入时刻。
    snapshot.price_snapshot_at_ms = now_ms;
    snapshot
}

pub fn price_rule_dto(conn: &Connection, rule_id: &str) -> Result<Option<PriceRuleDto>, DbError> {
    let mut statement = conn.prepare(
        "SELECT price_id, provider_id, upstream_model, currency, input_mode,
                input_rate_nanos, output_rate_nanos, cache_read_rate_nanos,
                cache_write_rate_nanos, revision, label, created_at_ms, updated_at_ms
         FROM stats_model_prices WHERE price_id=?1",
    )?;
    let mut rows = statement.query([rule_id])?;
    match rows.next()? {
        Some(row) => Ok(Some(read_price_rule(row)?)),
        None => Ok(None),
    }
}

fn read_price_rule(row: &rusqlite::Row<'_>) -> rusqlite::Result<PriceRuleDto> {
    let currency = Currency::parse(&row.get::<_, String>(3)?).unwrap_or(Currency::Usd);
    // The API exposes rates exactly as stored: integer nano units.
    Ok(PriceRuleDto {
        price_id: row.get(0)?,
        rule_id: row.get(0)?,
        provider_id: row.get(1)?,
        upstream_model: row.get(2)?,
        currency,
        input_rate_nanos: row.get::<_, i64>(5)?.to_string(),
        output_rate_nanos: row.get::<_, i64>(6)?.to_string(),
        cache_read_rate_nanos: Some(row.get::<_, i64>(7)?.to_string()),
        cache_write_rate_nanos: Some(row.get::<_, i64>(8)?.to_string()),
        input_mode: InputMode::parse(&row.get::<_, String>(4)?).unwrap_or(InputMode::Flat),
        label: row.get(10)?,
        created_at_ms: row.get(11)?,
        revision: row.get(9)?,
        referenced: false,
    })
}

pub fn list_price_rules(
    conn: &Connection,
    provider_id: Option<&str>,
    upstream_model: Option<&str>,
    after_rule_id: Option<&str>,
    limit: i64,
) -> Result<(Vec<PriceRuleDto>, Option<String>), DbError> {
    // Ask for one extra row so "there is another page" is provable rather than
    // guessed. The cursor is the last `price_id` of the page.
    let mut statement = conn.prepare(
        "SELECT price_id, provider_id, upstream_model, currency, input_mode,
                input_rate_nanos, output_rate_nanos, cache_read_rate_nanos,
                cache_write_rate_nanos, revision, label, created_at_ms, updated_at_ms
         FROM stats_model_prices
         WHERE (?1 IS NULL OR provider_id=?1)
           AND (?2 IS NULL OR upstream_model=?2)
           AND (?3 IS NULL OR price_id>?3)
         ORDER BY price_id LIMIT ?4",
    )?;
    let mut rows = statement.query(rusqlite::params![
        provider_id,
        upstream_model,
        after_rule_id,
        limit + 1
    ])?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        out.push(read_price_rule(row)?);
    }
    let next = if out.len() as i64 > limit {
        out.truncate(limit as usize);
        out.last().map(|rule| rule.rule_id.clone())
    } else {
        None
    };
    Ok((out, next))
}

/// 新建或按乐观并发 revision 编辑一条当前价格。
///
/// 入口由 stats 控制循环的 `PriceSave` 分支调用，输入来自设置界面的保存命令。到达本函数
/// 前，设置表单已把人类金额转换成 nano 整数字符串，管理 API 的 `parse_price_save` 再做字段归一；缺失或空白
/// 的 input/output/cache 费率字段会成为 `"0"`。因此本函数看到的是已归一 DTO：普通
/// input/output 仍会做非空、范围等防御校验，缺失字段的“默认零”语义由管理层建立，而不是
/// 在本函数内再次把 `None` 猜成 0。单位均为 nano 货币单位/1,000,000 token。
///
/// `rule_id = None` 是新建流程，生成新 id；若同一 `(provider_id, upstream_model)` 已存在，
/// 普通新建的 revision 条件不会匹配，返回 `stats_revision_conflict`，调用方应重新读取后
/// 走编辑。`rule_id = Some` 是编辑：provider/model 不可改变，SQL 以
/// `price_id + expected_revision` 条件更新并使 revision 加一；id 不存在返回 not found，
/// revision 不一致返回冲突。这与 Spring 中显式 CAS/乐观锁更新相似，但 revision 来自 DTO
/// 请求参数，不是框架自动管理。
///
/// 校验、写入和 revision 检查在同一个 `IMMEDIATE` 事务内。提交后会单独读取返回 DTO 并
/// reload 整张价格缓存；任一后续步骤失败都可能返回 `Err`，但事务已经提交，因此这里没有
/// “Err 必然回滚”或数据库/内存联合原子保证。reload 成功时本函数更新 `writer.prices`；
/// 外层控制分支随后才发布给请求线程。历史请求仍使用自己在 Start 时冻结的快照。
pub fn save_price_rule(
    writer: &mut Writer,
    request: &crate::stats::PriceSaveRequest,
    now_ms: i64,
) -> Result<PriceRuleDto, DbError> {
    let conn = &mut writer.conn;
    let currency = Currency::parse(&request.currency)
        .ok_or_else(|| DbError::refused(error(ERR_INVALID, "currency must be USD or CNY")))?;
    let input_mode = InputMode::parse(&request.input_mode)
        .ok_or_else(|| DbError::refused(error(ERR_INVALID, "input_mode is not supported")))?;
    let input_rate = pricing::parse_rate(&request.input_rate_nanos)
        .map_err(|detail| DbError::refused(error(ERR_INVALID, &format!("input rate: {detail}"))))?;
    let output_rate = pricing::parse_rate(&request.output_rate_nanos).map_err(|detail| {
        DbError::refused(error(ERR_INVALID, &format!("output rate: {detail}")))
    })?;
    let optional_rate = |value: Option<&str>| -> Result<i64, DbError> {
        match value.map(str::trim).filter(|text| !text.is_empty()) {
            None => Ok(0),
            // A blank box means the explicit zero default, not "unset".
            Some(text) => pricing::parse_rate(text).map_err(|detail| {
                DbError::refused(error(ERR_INVALID, &format!("cache rate: {detail}")))
            }),
        }
    };
    let cache_read_rate = optional_rate(request.cache_read_rate_nanos.as_deref())?;
    let cache_write_rate = optional_rate(request.cache_write_rate_nanos.as_deref())?;
    if request.provider_id.trim().is_empty() || request.upstream_model.trim().is_empty() {
        return Err(DbError::refused(error(
            ERR_INVALID,
            "provider and model are required",
        )));
    }
    // 空输入框在本 API 中表示显式 0，而不是未知；所以 0 是合法 cache 费率，
    // 例如明确配置免费的 cache read/write。
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let price_id = match &request.rule_id {
        Some(price_id) => {
            // provider/model 是价格身份的键；编辑时若静默忽略改键，列表仍显示旧键而
            // 用户会误以为改键成功，因此明确拒绝。
            let current: Option<(String, String)> = tx
                .query_row(
                    "SELECT provider_id, upstream_model FROM stats_model_prices WHERE price_id=?1",
                    [price_id.as_str()],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()?;
            if let Some((provider_id, upstream_model)) = current
                && (provider_id != request.provider_id || upstream_model != request.upstream_model)
            {
                return Err(DbError::refused(error(
                    ERR_INVALID,
                    "a price keeps its provider and model; create a new price instead",
                )));
            }
            let changed = tx.execute(
                "UPDATE stats_model_prices SET currency=:currency, input_mode=:mode,
                     input_rate_nanos=:input, output_rate_nanos=:output,
                     cache_read_rate_nanos=:read, cache_write_rate_nanos=:write,
                     label=:label, revision=revision+1, updated_at_ms=:now
                 WHERE price_id=:price_id AND revision=:revision",
                rusqlite::named_params! {
                    ":currency": currency.as_str(),
                    ":mode": input_mode.as_str(),
                    ":input": input_rate,
                    ":output": output_rate,
                    ":read": cache_read_rate,
                    ":write": cache_write_rate,
                    ":label": request.label,
                    ":now": now_ms,
                    ":price_id": price_id,
                    ":revision": request.expected_revision.unwrap_or(0),
                },
            )?;
            if changed != 1 {
                // 区分“id 已不存在”和“读到的 revision 已过期”，避免未知 id 被误当
                // 新行插入，也帮助 UI 选择重新加载还是提示并发修改。
                let exists: Option<i64> = tx
                    .query_row(
                        "SELECT revision FROM stats_model_prices WHERE price_id=?1",
                        [price_id.as_str()],
                        |row| row.get(0),
                    )
                    .optional()?;
                return Err(DbError::refused(error(
                    if exists.is_some() {
                        ERR_REVISION_CONFLICT
                    } else {
                        crate::stats::dto::ERR_NOT_FOUND
                    },
                    if exists.is_some() {
                        "the price was changed by someone else; reload and retry"
                    } else {
                        "the price no longer exists; reload the list"
                    },
                )));
            }
            price_id.clone()
        }
        None => {
            let price_id = crate::config::token()[..24].to_owned();
            let changed = tx.execute(
                "INSERT INTO stats_model_prices(
                     price_id, provider_id, upstream_model, currency, input_mode,
                     input_rate_nanos, output_rate_nanos, cache_read_rate_nanos,
                     cache_write_rate_nanos, label, revision, created_at_ms, updated_at_ms)
                 VALUES(:price_id,:provider,:model,:currency,:mode,:input,:output,
                     :read,:write,:label,1,:now,:now)
                 ON CONFLICT(provider_id, upstream_model) DO UPDATE SET
                     currency=excluded.currency, input_mode=excluded.input_mode,
                     input_rate_nanos=excluded.input_rate_nanos,
                     output_rate_nanos=excluded.output_rate_nanos,
                     cache_read_rate_nanos=excluded.cache_read_rate_nanos,
                     cache_write_rate_nanos=excluded.cache_write_rate_nanos,
                     label=excluded.label, revision=stats_model_prices.revision+1,
                     updated_at_ms=excluded.updated_at_ms
                 WHERE stats_model_prices.revision=:expected_revision",
                rusqlite::named_params! {
                    ":price_id": price_id,
                    ":provider": request.provider_id,
                    ":model": request.upstream_model,
                    ":currency": currency.as_str(),
                    ":mode": input_mode.as_str(),
                    ":input": input_rate,
                    ":output": output_rate,
                    ":read": cache_read_rate,
                    ":write": cache_write_rate,
                    ":label": request.label,
                    ":now": now_ms,
                    ":expected_revision": request.expected_revision.unwrap_or(0),
                },
            )?;
            // 普通新建若撞到已有 provider/model，upsert 的 revision WHERE 不匹配会更新
            // 0 行。必须把冲突返回给调用方，不能把数据库仍保留的旧价格报成创建成功。
            if changed == 0 {
                return Err(DbError::refused(error(
                    ERR_REVISION_CONFLICT,
                    "a price already exists for this provider and model; edit it instead",
                )));
            }
            // upsert 可能保留原有 price_id，因此重新查询实际键而不是假定新生成的 id。
            tx.query_row(
                "SELECT price_id FROM stats_model_prices WHERE provider_id=?1 AND upstream_model=?2",
                rusqlite::params![request.provider_id, request.upstream_model],
                |row| row.get(0),
            )?
        }
    };
    tx.commit()?;
    // 以下读取和 reload 都发生在 commit 之后。此时即使函数返回 Err，SQLite 中的
    // 新价格也可能已经生效；不能用 Err 推断事务未提交。
    let dto = price_rule_dto(conn, &price_id)?
        .ok_or_else(|| DbError::refused(error(ERR_UNAVAILABLE, "price was not stored")))?;
    // 只有 reload 成功才替换 writer 的内存副本；发布到共享 PriceCache 还由外层控制
    // 分支完成。若本步失败，数据库已提交而共享缓存可能暂时仍是旧值。
    writer.prices = load_price_cache(conn)?;
    Ok(dto)
}

/// 按 `price_id + expected_revision` 删除一条当前价格。
///
/// 调用者是 stats 控制循环的 `PriceDelete` 分支；`expected_revision` 来自设置界面最近一次
/// 读取的 DTO，用来防止另一轮编辑先发生后被旧页面覆盖。删除只影响后续请求取价；历史费用
/// 行保留自己的价格快照和可追踪的 `source_price_id`，不依赖该行继续存在。
///
/// 删除与 revision 条件在同一事务内。commit 之后的缓存 reload 或后续发布仍可能失败，
/// 所以函数返回 `Err` 不等于删除一定未发生；只有完整成功路径才会让新请求立即看到新缓存。
pub fn delete_price_rule(
    writer: &mut Writer,
    price_id: &str,
    expected_revision: i64,
) -> Result<(), DbError> {
    let conn = &mut writer.conn;
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let changed = tx.execute(
        "DELETE FROM stats_model_prices WHERE price_id=?1 AND revision=?2",
        rusqlite::params![price_id, expected_revision],
    )?;
    if changed != 1 {
        return Err(DbError::refused(error(
            ERR_REVISION_CONFLICT,
            "the price was changed by someone else; reload and retry",
        )));
    }
    tx.commit()?;
    // 历史费用保留自己的快照，删除仅用于改变之后的准入取价。reload 在 commit 后，
    // 因而失败时数据库删除可能已经生效，共享缓存则可能保持旧值。
    writer.prices = load_price_cache(conn)?;
    Ok(())
}

// 恢复 backlog 的职责边界：`apply_batch` 的整批失败或隔离丢弃不会重放原事件；
// writer 线程只保留 `Finish` 的 request id。内存列表溢出时，`trim_orphans` 调用
// `push_backlog` 把尚未收敛的 id 持久化；`recover_collection` 再通过 `backlog_len`
// 和 `repair_backlog_batch` 有界排空。backlog 保存的是“待核对 id”，不是原始事件载荷，
// 所以它能避免键重复，却不能还原已丢失的 FinishRecord。

/// 把未能落库的终态 request id 写入同进程可重试的持久 backlog。
///
/// 调用者和输入边界见上方恢复说明。主键与 `ON CONFLICT DO NOTHING` 使同一 id 可在
/// 多次失败中被重复尝试排队，但不会产生重复 backlog 行；返回值是本次实际新增的 id 数，
/// 时间单位为 epoch 毫秒。提交失败时整批新增回滚，调用者必须保留或重新获得这些 id。
pub fn push_backlog(writer: &mut Writer, ids: &[String], now_ms: i64) -> Result<usize, DbError> {
    let tx = writer
        .conn
        .transaction_with_behavior(TransactionBehavior::Immediate)?;
    let mut stored = 0usize;
    {
        let mut statement = tx.prepare(
            "INSERT INTO stats_recovery_backlog(request_id, created_at_ms) VALUES(?1, ?2)
             ON CONFLICT(request_id) DO NOTHING",
        )?;
        for id in ids {
            stored += statement.execute(rusqlite::params![id, now_ms])?;
        }
    }
    tx.commit()?;
    Ok(stored)
}

/// 读取当前 durable backlog 中的 id 数量。
///
/// 调用者是 writer 恢复循环；这是读取瞬间的 COUNT，不是事件载荷数量，也不代表所有
/// id 都能修复（请求行可能已不存在）。返回错误不能被当作空 backlog。
pub fn backlog_len(conn: &Connection) -> Result<i64, DbError> {
    Ok(
        conn.query_row("SELECT COUNT(*) FROM stats_recovery_backlog", [], |row| {
            row.get(0)
        })?,
    )
}

/// 启动恢复扫描的冻结范围和游标。
///
/// `cursor_id` 是已成功提交扫描到的最大请求行 id；`as_of_id` 是启动事务冻结的上界。
/// 两者都是 SQLite 自增行 id，不是请求开始时间。扫描不会触碰大于 `as_of_id` 的新请求。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecoveryScan {
    /// 已成功检查并提交的最大 `usage_requests.id`；初值 0。
    pub cursor_id: i64,
    /// 启动恢复的冻结上界；本扫描完成后也不会越过它。
    pub as_of_id: i64,
}

impl RecoveryScan {
    pub fn new(as_of_id: i64) -> Self {
        Self {
            cursor_id: 0,
            as_of_id,
        }
    }

    pub fn finished(&self) -> bool {
        self.cursor_id >= self.as_of_id
    }
}

/// 修复启动水位内下一批无法再收到 `Finish` 的请求/费用行。
///
/// 调用者是 `stats::recover_collection`。输入 `limit` 限制本批候选数，`now_ms` 是
/// 修复时写入的中断时间（epoch 毫秒）。函数在一个 `IMMEDIATE` 事务里选择
/// `id > cursor_id && id <= as_of_id` 的候选，包括仍 open 的请求、pending 费用和需要
/// 修正的旧版零费形态，逐行尝试 `repair_one`；无法立即修复的 id 会在推进游标前先写入
/// durable backlog，随后 repair 和 backlog 一起提交。
///
/// 非空候选路径会先把本批修复和待重试 id 的 backlog 新增放入同一事务，计算下一游标但
/// 暂不发布，只有 `commit` 成功后才写回 `scan.cursor_id`；失败时数据库修改回滚且内存
/// 游标仍停在旧位置。空候选分支是例外：它先把本地游标设为 `as_of_id`，再提交没有
/// 执行行级修复的事务（仍以 IMMEDIATE 开启，并非只读连接上的事务）；
/// 若该提交失败，函数返回错误但传入的本地游标已经推进。返回
/// `(repaired, remaining_id_distance)`：第一项包含已修复或已确认不存在的候选数；第二项
/// 在有候选时只是 `as_of_id - last_examined_id` 的 id 距离，不是精确剩余行数，没有候选
/// 时返回 0。
pub fn scan_recovery_batch(
    writer: &mut Writer,
    scan: &mut RecoveryScan,
    limit: i64,
    now_ms: i64,
) -> Result<(i64, i64), DbError> {
    if scan.finished() {
        return Ok((0, 0));
    }
    let tx = writer
        .conn
        .transaction_with_behavior(TransactionBehavior::Immediate)?;
    let ids: Vec<(i64, String)> = {
        let mut statement = tx.prepare(
            "SELECT r.id, r.request_id FROM usage_requests r
             LEFT JOIN usage_costs c ON c.request_id=r.request_id
             WHERE r.id>?1 AND r.id<=?2
               AND (r.outcome='in_flight'
                    OR c.status='pending'
                    OR (c.status='not_applicable' AND c.price_source='manual'
                        AND c.total_cost_nanos=0 AND r.upstream_started IS NULL))
             ORDER BY r.id LIMIT ?3",
        )?;
        let mut rows = statement.query(rusqlite::params![scan.cursor_id, scan.as_of_id, limit])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push((row.get(0)?, row.get(1)?));
        }
        out
    };
    if ids.is_empty() {
        // Nothing left inside the frozen scope.
        scan.cursor_id = scan.as_of_id;
        tx.commit()?;
        return Ok((0, 0));
    }
    let mut repaired = 0i64;
    let mut still: Vec<String> = Vec::new();
    for (_, request_id) in &ids {
        match repair_one(&tx, request_id, now_ms, true) {
            Ok(RepairOutcome::Done) | Ok(RepairOutcome::Missing) => repaired += 1,
            Ok(RepairOutcome::Retry) => still.push(request_id.clone()),
            Err(failure) => {
                eprintln!(
                    "statistics could not repair {request_id}: {}",
                    failure.message
                );
                still.push(request_id.clone());
            }
        }
    }
    // A row this batch could not repair is queued durably *before* the cursor
    // moves past it. Advancing without that let the scan report "finished"
    // while the row was still `pending` and nothing remembered it.
    for id in &still {
        tx.execute(
            "INSERT INTO stats_recovery_backlog(request_id, created_at_ms) VALUES(?1, ?2)
             ON CONFLICT(request_id) DO NOTHING",
            rusqlite::params![id, now_ms],
        )?;
    }
    // The new watermark is computed here but published only once the
    // transaction is durable: a failed commit leaves the scan where it was.
    let last = ids.last().map(|(id, _)| *id).unwrap_or(scan.cursor_id);
    let next_cursor = if ids.len() as i64 >= limit {
        last
    } else {
        scan.as_of_id
    };
    tx.commit()?;
    scan.cursor_id = next_cursor;
    Ok((repaired, scan.as_of_id - last))
}

/// 一个待恢复 request id 本次检查后的分类。
#[derive(Debug, PartialEq, Eq)]
pub enum RepairOutcome {
    /// 请求不再 `in_flight` 且费用不再 pending；可以从 backlog/内存 retry 集移除。
    Done,
    /// 没有对应请求行：该 id 已失效，可以从恢复队列移除。
    Missing,
    /// 请求或费用仍待收敛；调用者必须保留 id 等待下一轮。
    Retry,
}

/// 在调用者提供的事务中修复一个 request id。
///
/// 调用者包括启动扫描、内存 orphan 修复和 durable backlog 修复。`unclean=true` 只用于
/// 冻结水位内的启动扫描：仍为 `in_flight` 的请求改写为 `interrupted`，原因标成
/// `unclean_shutdown`；当前 run 中丢失 Finish 的 orphan 使用 `unclean=false`，原因标成
/// `other`。随后函数从 Start 时保存的费用快照尝试结算，再检查请求和费用是否都离开
/// open/pending 状态。
///
/// 与 `settle_orphans` 不同，本函数也处理“请求已终态但费用仍 pending”的旧数据形态。
/// 它接收 `&Transaction`，不会自行 commit/rollback；单个 id 内部若某条 SQL 返回错误，
/// 调用者通常记录并重试该 id，但同一外层事务先前为它写下的语句并不保证逐 id 自动回滚。
/// 只有整个事务最终未提交时，数据库才会回滚全部未提交修改。
fn repair_one(
    tx: &rusqlite::Transaction<'_>,
    request_id: &str,
    now_ms: i64,
    unclean: bool,
) -> Result<RepairOutcome, DbError> {
    let outcome: Option<String> = tx
        .query_row(
            "SELECT outcome FROM usage_requests WHERE request_id=?1",
            [request_id],
            |row| row.get(0),
        )
        .optional()?;
    let Some(outcome) = outcome else {
        return Ok(RepairOutcome::Missing);
    };
    if outcome == "in_flight" {
        tx.execute(
            // A row inside the startup scope was cut off by the crash; a row
            // abandoned while running lost its terminal event instead.
            "UPDATE usage_requests SET outcome='interrupted',
                 error_kind=CASE WHEN :unclean=1 THEN 'unclean_shutdown' ELSE 'other' END,
                 ended_at_ms=:now, updated_at_ms=:now,
                 usage_status=CASE WHEN usage_status='complete' THEN usage_status ELSE 'missing' END,
                 usage_reason=CASE WHEN :unclean=1 THEN 'unclean_shutdown' ELSE NULL END,
                 usage_source='none'
             WHERE request_id=:id AND outcome='in_flight'",
            rusqlite::named_params! {
                ":now": now_ms,
                ":id": request_id,
                ":unclean": if unclean { 1 } else { 0 },
            },
        )?;
    }
    settle_abandoned_cost(tx, request_id, now_ms)?;
    // The id only leaves the queue once its cost row is genuinely terminal; a
    // snapshot that still cannot be settled stays queued and is retried.
    let still_pending: i64 = tx.query_row(
        "SELECT COUNT(*) FROM usage_costs WHERE request_id=?1 AND status='pending'",
        [request_id],
        |row| row.get(0),
    )?;
    let open: i64 = tx.query_row(
        "SELECT COUNT(*) FROM usage_requests WHERE request_id=?1 AND outcome='in_flight'",
        [request_id],
        |row| row.get(0),
    )?;
    Ok(if still_pending == 0 && open == 0 {
        RepairOutcome::Done
    } else {
        RepairOutcome::Retry
    })
}

/// 在一个事务中修复调用者明确给出的 id 列表。
///
/// 调用者是 `recover_collection` 的内存 orphan 列表。输出 `(done, missing, retry)` 精确
/// 对应本输入的分类，调用者据此保留 retry id、丢弃 stale id 并累计可见失败。`Missing`
/// 不是“修复成功”，而是请求行根本不存在；`repair_one` 的错误会记录到 stderr 并归入
/// retry，不会从列表静默消失。
pub fn repair_ids(
    writer: &mut Writer,
    ids: &[String],
    now_ms: i64,
) -> Result<(i64, i64, Vec<String>), DbError> {
    if ids.is_empty() {
        return Ok((0, 0, Vec::new()));
    }
    let tx = writer
        .conn
        .transaction_with_behavior(TransactionBehavior::Immediate)?;
    let mut done = 0i64;
    let mut missing = 0i64;
    let mut retry = Vec::new();
    for id in ids {
        match repair_one(&tx, id, now_ms, false) {
            Ok(RepairOutcome::Done) => done += 1,
            Ok(RepairOutcome::Missing) => missing += 1,
            Ok(RepairOutcome::Retry) => retry.push(id.clone()),
            Err(failure) => {
                eprintln!("statistics could not repair {id}: {}", failure.message);
                retry.push(id.clone());
            }
        }
    }
    tx.commit()?;
    Ok((done, missing, retry))
}

/// 从 durable backlog 取至多 `limit` 个 id 修复，并返回本次结果及提交后的剩余数量。
///
/// 调用者是 `recover_collection`。候选 id 先在事务外读取；由于唯一写连接由 writer
/// 线程串行使用，本次调用期间不会由本进程另一个写者修改 backlog。随后一个
/// `IMMEDIATE` 事务修复候选，只删除 `Done` 和 `Missing`，`Retry` 保留。
///
/// 返回 `(done, missing, remaining_count)`；`remaining_count` 在提交后重新 COUNT，是
/// 精确 backlog 行数。若事务失败，删除与部分修复都不提交。函数内部对单个 id 的错误
/// 采取“保留 id 供下次再试”，但这不保证该 id 在此前 SQL 上从未留下未提交修改。
pub fn repair_backlog_batch(
    writer: &mut Writer,
    limit: i64,
    now_ms: i64,
) -> Result<(i64, i64, i64), DbError> {
    let ids: Vec<String> = {
        let mut statement = writer.conn.prepare(
            "SELECT request_id FROM stats_recovery_backlog
             ORDER BY created_at_ms, request_id LIMIT ?1",
        )?;
        let mut rows = statement.query([limit])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(row.get(0)?);
        }
        out
    };
    if ids.is_empty() {
        return Ok((0, 0, 0));
    }
    let tx = writer
        .conn
        .transaction_with_behavior(TransactionBehavior::Immediate)?;
    let mut done = 0i64;
    let mut missing = 0i64;
    let mut retry = Vec::new();
    for id in &ids {
        match repair_one(&tx, id, now_ms, false) {
            Ok(RepairOutcome::Done) => done += 1,
            Ok(RepairOutcome::Missing) => missing += 1,
            Ok(RepairOutcome::Retry) => retry.push(id.clone()),
            Err(failure) => {
                // 当前函数没有 per-id savepoint；记录失败并把 id 放进 retry，避免从
                // backlog 删除。外层事务最终成功时，先前同 id 的未提交部分修改仍可能
                // 一起提交，因此这里只承诺“不移出队列”，不承诺逐 id 完整回滚。
                eprintln!("statistics could not repair {id}: {}", failure.message);
                retry.push(id.clone());
            }
        }
    }
    {
        let mut statement = tx.prepare("DELETE FROM stats_recovery_backlog WHERE request_id=?1")?;
        for id in ids.iter().filter(|id| !retry.contains(id)) {
            statement.execute([id.as_str()])?;
        }
    }
    tx.commit()?;
    let remaining = backlog_len(&writer.conn)?;
    Ok((done, missing, remaining))
}

/// 当请求已不可能再收到 `Finish` 时，为费用行尝试写入终态。
///
/// 调用者是 `settle_orphans` 和 `repair_one`，因此运行在当前恢复事务中；函数不会自行
/// commit/rollback。它读取 Start 时已经保存的价格快照及请求上的 usage 事实，不使用
/// 当前价格配置。定价公式本身属于 `pricing` 模块，这里只负责生命周期和持久化边界：
/// 明确的零价可以是已确认零；manual 价格但 usage 不可用时保持未知，不能伪装成零。
///
/// 返回 `true` 表示找到匹配费用行并完成了本函数的结算尝试；`false` 表示没有需要处理的
/// 行，或旧版 `legacy_unknown` 明确无法推断。是否真正可离开 backlog 仍由 `repair_one`
/// 再次查询请求 open 与费用 pending 状态决定。
fn settle_abandoned_cost(
    tx: &rusqlite::Transaction<'_>,
    request_id: &str,
    now_ms: i64,
) -> Result<bool, DbError> {
    let row = tx
        .query_row(
            "SELECT c.price_source, c.currency, c.input_mode, c.input_rate_nanos,
                    c.output_rate_nanos, c.cache_read_rate_nanos, c.cache_write_rate_nanos,
                    r.upstream_started, r.usage_status, r.input_tokens, r.output_tokens,
                    r.total_tokens, r.cache_read_tokens, r.cache_write_tokens,
                    r.reasoning_tokens, r.input_breakdown, r.input_uncached_tokens
             FROM usage_costs c JOIN usage_requests r ON r.request_id=c.request_id
             WHERE c.request_id=?1
               AND (c.status='pending'
                    OR (c.status='not_applicable' AND c.price_source='manual'
                        AND c.total_cost_nanos=0))",
            [request_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<i64>>(3)?,
                    row.get::<_, Option<i64>>(4)?,
                    row.get::<_, Option<i64>>(5)?,
                    row.get::<_, Option<i64>>(6)?,
                    row.get::<_, Option<i64>>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, Option<i64>>(9)?,
                    row.get::<_, Option<i64>>(10)?,
                    row.get::<_, Option<i64>>(11)?,
                    row.get::<_, Option<i64>>(12)?,
                    row.get::<_, Option<i64>>(13)?,
                    row.get::<_, Option<i64>>(14)?,
                    row.get::<_, String>(15)?,
                    row.get::<_, Option<i64>>(16)?,
                ))
            },
        )
        .optional()?;
    let Some((
        price_source,
        currency,
        mode,
        input_rate,
        output_rate,
        read_rate,
        write_rate,
        upstream_started,
        usage_status,
        input,
        output,
        total,
        cache_read,
        cache_write,
        reasoning,
        breakdown,
        uncached,
    )) = row
    else {
        // Already settled (or never started): nothing to close.
        return Ok(false);
    };
    if price_source == "default_zero" {
        // The unit prices are an explicit zero, so the amount is a confirmed
        // zero no matter whether the upstream was reached.
        write_cost_maybe_correcting(
            tx,
            request_id,
            CostStatus::DefaultZero,
            Some(CostParts::zero()),
            now_ms,
            true,
        )?;
        return Ok(true);
    }
    if price_source == "legacy_unknown" {
        // The old amount is unknown and must not be replaced by an invented one.
        return Ok(false);
    }
    // `upstream_started` is a three-state fact: only an explicit 0 proves the
    // request never reached the upstream. A NULL means "unknown" and must never
    // be read as "not sent", which would record a priced request as free.
    if upstream_started == Some(0) {
        write_cost_maybe_correcting(
            tx,
            request_id,
            CostStatus::NotApplicable,
            Some(CostParts::zero()),
            now_ms,
            true,
        )?;
        return Ok(true);
    }
    let usage = UsageOutcome {
        status: UsageStatus::parse(&usage_status).unwrap_or(UsageStatus::Missing),
        reason: None,
        numbers: crate::stats::collector::UsageNumbers {
            input,
            output,
            total,
            cache_read,
            cache_write,
            reasoning,
        },
        breakdown: InputBreakdown::parse(&breakdown).unwrap_or(InputBreakdown::Unknown),
        uncached,
    };
    let rule = PriceRule {
        rule_id: String::new(),
        currency: Currency::parse(&currency).unwrap_or(Currency::Usd),
        input_mode: InputMode::parse(mode.as_deref().unwrap_or("flat")).unwrap_or(InputMode::Flat),
        input_rate_nanos: input_rate.unwrap_or(0),
        output_rate_nanos: output_rate.unwrap_or(0),
        cache_read_rate_nanos: read_rate,
        cache_write_rate_nanos: write_rate,
    };
    // Reaching this point means the upstream was either reached or is unknown,
    // so a non-zero price with no usable usage stays an unknown amount instead
    // of becoming a fake zero.
    let settlement = pricing::settle(Some(&rule), &usage, true);
    // A row that was wrongly settled as a free `not_applicable` is rewritten
    // from the same snapshot, so the correction never invents a new price.
    write_cost_maybe_correcting(
        tx,
        request_id,
        settlement.status,
        settlement.parts,
        now_ms,
        true,
    )?;
    Ok(true)
}

/// 把一条 `FinishRecord` 合并到已存在的请求行，并结算其费用行。
///
/// 调用者是 `apply_batch_once`；`tx` 由该写入路径持有，`request_id` 来自 Finish 事件，
/// `finish` 来自请求 guard 形成的终态记录，`now_ms` 是写入时的 epoch 毫秒。函数先用
/// `outcome='in_flight'` 条件更新请求；重复 Finish、Start 未落库或请求已终态时更新 0 行，
/// 立即 no-op，因此不会覆盖已经结算的快照/金额或再次累计费用。
///
/// 更新成功后会读取 Start 写入的费用 pending 行，取其中的价格快照。该查询是完成流程的
/// 前提：目标行缺失会返回 SQLite 错误，本函数不会在这里另行插入费用行。随后：
///
/// - `price_source='default_zero'` 是“没有匹配到当前价格”的政策性零价，不要求 Complete
///   usage。上游从未开始时写 `NotApplicable/0`，已开始时写 `DefaultZero/0`。
/// - 其他来源目前都写作 `'manual'`，包括人工编辑和目录自动导入。它们交给
///   `pricing::settle`；即使规则中所有费率均为 0，也必须先有 `UsageStatus::Complete`，
///   否则写 `UsageIncomplete`，不会像 `default_zero` 一样绕过 usage 完整性。
///
/// 金额未知或分解不受支持时，`parts` 为 `None`，数据库金额列为 NULL；这里不把未知伪造为
/// 0。本函数不拥有事务提交权：数据库错误向上返回，由调用者的整批事务或逐事件隔离路径
/// 决定回滚当前事务。若同批 Start/Finish 位于同一事务，它们会一起回滚；若 Start 已在先前
/// 事务提交，则它会保留，并由后续恢复流程处理未完成生命周期。
fn finish_request(
    tx: &rusqlite::Transaction<'_>,
    request_id: &str,
    finish: &FinishRecord,
    now_ms: i64,
) -> Result<(), DbError> {
    let updated = tx.execute(
        "UPDATE usage_requests SET ended_at_ms=:ended, duration_ms=:duration,
             ttfb_ms=:ttfb, first_content_ms=:first_content, upstream_started=:upstream,
             response_model=:response_model, http_status=:http_status, outcome=:outcome,
             error_kind=:error_kind, usage_status=:usage_status, usage_reason=:usage_reason,
             usage_source=:usage_source, input_tokens=:input, output_tokens=:output,
             total_tokens=:total, cache_read_tokens=:cache_read,
             cache_write_tokens=:cache_write, reasoning_tokens=:reasoning,
             input_uncached_tokens=:uncached, input_breakdown=:breakdown,
             updated_at_ms=:now
         WHERE request_id=:request_id AND outcome='in_flight'",
        rusqlite::named_params! {
            ":ended": finish.ended_at_ms,
            ":duration": finish.duration_ms,
            ":ttfb": finish.ttfb_ms,
            ":first_content": finish.first_content_ms,
            ":upstream": if finish.upstream_started { 1 } else { 0 },
            ":response_model": finish.response_model,
            ":http_status": finish.http_status,
            ":outcome": finish.outcome.as_str(),
            ":error_kind": finish.error_kind.map(|kind| kind.as_str()),
            ":usage_status": finish.usage_status.as_str(),
            ":usage_reason": finish.usage_reason.map(|reason| reason.as_str()),
            ":usage_source": finish.usage_source.as_str(),
            ":input": finish.input_tokens,
            ":output": finish.output_tokens,
            ":total": finish.total_tokens,
            ":cache_read": finish.cache_read_tokens,
            ":cache_write": finish.cache_write_tokens,
            ":reasoning": finish.reasoning_tokens,
            ":uncached": finish.input_uncached_tokens,
            ":breakdown": finish.input_breakdown.as_str(),
            ":now": now_ms,
            ":request_id": request_id,
        },
    )?;
    if updated != 1 {
        // 重复 Finish、缺失 Start 或已有终态必须是 no-op，不能覆盖已结算快照/金额。
        return Ok(());
    }
    // Start 写入的 pending 快照是唯一取价来源；当前配置可能已经编辑或删除。
    // query_row 不是 optional：pending 行缺失会报错，而不是在这里兜底创建费用行。
    let snapshot = tx.query_row(
        "SELECT price_source, currency, input_mode, input_rate_nanos, output_rate_nanos,
                cache_read_rate_nanos, cache_write_rate_nanos
         FROM usage_costs WHERE request_id=?1 AND status='pending'",
        [request_id],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<i64>>(3)?,
                row.get::<_, Option<i64>>(4)?,
                row.get::<_, Option<i64>>(5)?,
                row.get::<_, Option<i64>>(6)?,
            ))
        },
    )?;
    let (price_source, currency, mode, input_rate, output_rate, read_rate, write_rate) = snapshot;
    let usage = UsageOutcome {
        status: finish.usage_status,
        reason: finish.usage_reason,
        numbers: crate::stats::collector::UsageNumbers {
            input: finish.input_tokens,
            output: finish.output_tokens,
            total: finish.total_tokens,
            cache_read: finish.cache_read_tokens,
            cache_write: finish.cache_write_tokens,
            reasoning: finish.reasoning_tokens,
        },
        breakdown: finish.input_breakdown,
        uncached: finish.input_uncached_tokens,
    };
    let (status, parts) = if price_source == "default_zero" {
        // 这是“没有匹配规则”的政策性零价，不依赖 usage Complete。上游未开始与已开始
        // 分别表达为 not_applicable 和 default_zero；它不是价格表里全 0 的 manual 规则。
        if !finish.upstream_started {
            (CostStatus::NotApplicable, Some(CostParts::zero()))
        } else {
            (CostStatus::DefaultZero, Some(CostParts::zero()))
        }
    } else {
        let rule = PriceRule {
            rule_id: String::new(),
            currency: Currency::parse(&currency).unwrap_or(Currency::Usd),
            input_mode: InputMode::parse(mode.as_deref().unwrap_or("flat"))
                .unwrap_or(InputMode::Flat),
            input_rate_nanos: input_rate.ok_or_else(|| {
                DbError::refused(error(ERR_UNAVAILABLE, "price snapshot is incomplete"))
            })?,
            output_rate_nanos: output_rate.ok_or_else(|| {
                DbError::refused(error(ERR_UNAVAILABLE, "price snapshot is incomplete"))
            })?,
            cache_read_rate_nanos: read_rate,
            cache_write_rate_nanos: write_rate,
        };
        let settlement = pricing::settle(Some(&rule), &usage, finish.upstream_started);
        // 普通价格规则（含全 0 的 manual 规则）由 settle 要求 Complete usage；
        // 未知或无法支持的分解返回 None，金额必须保持 NULL，不能变成假 0。
        (settlement.status, settlement.parts)
    };
    write_cost(tx, request_id, status, parts, now_ms)
}

#[allow(clippy::too_many_arguments)]
fn write_cost(
    tx: &rusqlite::Transaction<'_>,
    request_id: &str,
    status: CostStatus,
    parts: Option<pricing::CostParts>,
    now_ms: i64,
) -> Result<(), DbError> {
    write_cost_maybe_correcting(tx, request_id, status, parts, now_ms, false)
}

/// `allow_correction` lets startup repair overwrite a row an earlier release
/// settled wrongly as a free `not_applicable`; the normal settle path never
/// rewrites a finished row.
fn write_cost_maybe_correcting(
    tx: &rusqlite::Transaction<'_>,
    request_id: &str,
    status: CostStatus,
    parts: Option<pricing::CostParts>,
    now_ms: i64,
    allow_correction: bool,
) -> Result<(), DbError> {
    // A status whose amount is unknown stores NULL, never zero: the schema
    // distinguishes "confirmed zero" from "not known".
    let (input, output, read, write, total) = match &parts {
        Some(parts) => (
            Some(parts.input),
            Some(parts.output),
            Some(parts.cache_read),
            Some(parts.cache_write),
            Some(parts.total),
        ),
        None => (None, None, None, None, None),
    };
    // Only the amounts and the settle status are written here. The unit prices,
    // currency and mode stay exactly as `Start` recorded them, and the row is
    // only updated while it is still `pending`, so a repeated `Finish` can never
    // rewrite a settled snapshot.
    let changed = tx.execute(
        "UPDATE usage_costs SET status=:status,
             input_cost_nanos=:input, output_cost_nanos=:output,
             cache_read_cost_nanos=:read, cache_write_cost_nanos=:write,
             total_cost_nanos=:total, calculated_at_ms=:now
         WHERE request_id=:request_id
           AND (status='pending'
                OR (:correct=1 AND status='not_applicable' AND price_source='manual'
                    AND total_cost_nanos=0))",
        rusqlite::named_params! {
            ":request_id": request_id,
            ":status": status.as_str(),
            ":input": input,
            ":output": output,
            ":read": read,
            ":write": write,
            ":total": total,
            ":now": now_ms,
            ":correct": if allow_correction { 1 } else { 0 },
        },
    )?;
    if changed == 1 {
        return Ok(());
    }
    // No pending row (a `Finish` whose `Start` never landed): store the terminal
    // state with whatever snapshot the caller can prove.
    tx.execute(
        "INSERT INTO usage_costs(request_id, price_source, currency, input_mode,
             input_rate_nanos, output_rate_nanos, cache_read_rate_nanos, cache_write_rate_nanos,
             status, input_cost_nanos, output_cost_nanos, cache_read_cost_nanos,
             cache_write_cost_nanos, total_cost_nanos, pricing_version, calculated_at_ms)
         VALUES(:request_id,'default_zero',(SELECT default_currency FROM stats_settings WHERE singleton=1),
             'flat',0,0,0,0,:status,:input,:output,:read,:write,:total,2,:now)
         ON CONFLICT(request_id) DO NOTHING",
        rusqlite::named_params! {
            ":request_id": request_id,
            ":status": status.as_str(),
            ":input": input,
            ":output": output,
            ":read": read,
            ":write": write,
            ":total": total,
            ":now": now_ms,
        },
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_startup_rolls_back_recovery_and_run_bookkeeping_together() {
        let folder = tempfile::tempdir().unwrap();
        let counters = std::sync::Arc::new(crate::stats::event::Counters::default());
        let (old, _) = Writer::open(folder.path(), "prior-unclean", counters.clone()).unwrap();
        drop(old);
        let db = Connection::open(folder.path().join("usage.sqlite3")).unwrap();
        db.execute_batch("CREATE TRIGGER refuse_new_interval BEFORE INSERT ON stats_collection_intervals WHEN NEW.run_id='new-run' BEGIN SELECT RAISE(ABORT,'synthetic startup rejection'); END;").unwrap();
        assert!(Writer::open(folder.path(), "new-run", counters.clone()).is_err());
        assert_eq!(db.query_row("SELECT COUNT(*) FROM stats_runs WHERE run_id='prior-unclean' AND stopped_at_ms IS NULL", [], |r| r.get::<_, i64>(0)).unwrap(), 1, "failed open consumed crash recovery");
        assert_eq!(db.query_row("SELECT COUNT(*) FROM stats_collection_intervals WHERE run_id='prior-unclean' AND ended_at_ms IS NULL", [], |r| r.get::<_, i64>(0)).unwrap(), 1);
        assert_eq!(
            db.query_row(
                "SELECT COUNT(*) FROM stats_runs WHERE run_id='new-run'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
        db.execute_batch("DROP TRIGGER refuse_new_interval")
            .unwrap();
        let (_writer, recovery) = Writer::open(folder.path(), "new-run", counters).unwrap();
        assert_eq!(recovery.unclean_runs, 1);
        assert!(recovery.unclean_detected);
        assert_eq!(
            db.query_row(
                "SELECT COUNT(*) FROM stats_collection_intervals WHERE run_id='new-run'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            1
        );
    }

    #[test]
    fn bounded_report_page_cache_keeps_wal_snapshots_fresh_and_reader_read_only() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("usage.sqlite3");
        let mut writer = open_connection(&path, false).unwrap();
        migrate(&mut writer, folder.path()).unwrap();
        let checkpoint = || {
            writer
                .execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
                .unwrap()
        };
        checkpoint();
        let reader = open_connection(&path, true).unwrap();
        assert!(
            reader
                .query_row("PRAGMA mmap_size", [], |row| row.get::<_, i64>(0))
                .unwrap()
                > 0
        );
        let cache = |conn: &Connection| {
            conn.query_row("PRAGMA cache_size", [], |row| row.get::<_, i64>(0))
                .unwrap()
        };
        assert_eq!(cache(&reader), -131_072);
        assert_ne!(cache(&writer), cache(&reader));
        // Import throughput tuning must retain WAL, NORMAL synchronization
        // and an enabled, bounded automatic checkpoint trigger.
        assert_eq!(
            writer
                .query_row("PRAGMA journal_mode", [], |row| row.get::<_, String>(0))
                .unwrap(),
            "wal"
        );
        assert_eq!(
            writer
                .query_row("PRAGMA synchronous", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            1
        );
        let checkpoint_frames = writer
            .query_row("PRAGMA wal_autocheckpoint", [], |row| row.get::<_, i64>(0))
            .unwrap();
        assert!((1..=16_384).contains(&checkpoint_frames));
        let count = || {
            reader
                .query_row("SELECT COUNT(*) FROM stats_runs", [], |row| {
                    row.get::<_, i64>(0)
                })
                .unwrap()
        };
        reader.execute_batch("BEGIN").unwrap();
        assert_eq!(count(), 0);
        writer
            .execute(
                "INSERT INTO stats_runs(run_id,started_at_ms) VALUES('one',1)",
                [],
            )
            .unwrap();
        assert_eq!(count(), 0, "a read transaction retains its WAL snapshot");
        reader.execute_batch("ROLLBACK").unwrap();
        checkpoint();
        assert_eq!(count(), 1, "the next report sees committed changes");
        writer.execute("DELETE FROM stats_runs", []).unwrap();
        assert_eq!(count(), 0, "maintenance deletion invalidates cached pages");
        checkpoint();
        assert_eq!(
            count(),
            0,
            "checkpointed mapped pages also see the deletion"
        );
        assert!(reader.execute("DELETE FROM stats_runs", []).is_err());
    }

    /// Inserts one imported workbuddy fact row plus its ledger entry.
    fn seed_imported_row(writer: &Writer, entry_key: &str) -> i64 {
        writer
            .conn
            .execute(
                "INSERT INTO usage_requests(request_id, source, provider_id, route_id,
                 provider_name, route_name, requested_model, upstream_model, started_at_ms,
                 ended_at_ms, outcome, usage_status, usage_source, created_at_ms, updated_at_ms,
                 upstream_protocol, record_origin, time_basis)
                 VALUES(?1,'workbuddy_import','wb','wb','WorkBuddy','WorkBuddy','m','m',
                 1000, 1000, 'recorded', 'missing', 'workbuddy_jsonl', 1000, 1000, NULL,
                 'workbuddy_builtin', 'message_timestamp')",
                rusqlite::params![entry_key],
            )
            .unwrap();
        let id: i64 = writer
            .conn
            .query_row(
                "SELECT id FROM usage_requests WHERE request_id = ?1",
                rusqlite::params![entry_key],
                |row| row.get(0),
            )
            .unwrap();
        writer
            .conn
            .execute(
                "INSERT INTO workbuddy_import_ledger(entry_key, namespace_id, session_hash,
                 message_hash, requested_model, event_at_ms, normalized_hash, state, request_id,
                 mapping_revision, created_at_ms, updated_at_ms)
                 VALUES(?1, ?2, ?3, ?4, 'm', 1000, ?5, 'imported', ?1, 1, 1000, 1000)",
                rusqlite::params![
                    entry_key,
                    "a".repeat(48),
                    // Derived from the key so several rows can coexist.
                    entry_key,
                    entry_key.chars().rev().collect::<String>(),
                    "d".repeat(64)
                ],
            )
            .unwrap();
        id
    }

    fn seed_proxy_row(writer: &Writer) -> i64 {
        writer
            .conn
            .execute(
                "INSERT INTO stats_runs(run_id, started_at_ms) VALUES('run-clear', 1000)",
                [],
            )
            .unwrap();
        writer
            .conn
            .execute(
                "INSERT INTO usage_requests(request_id, run_id, source, provider_id, route_id,
                 provider_name, route_name, requested_model, upstream_model, started_at_ms,
                 ended_at_ms, duration_ms, stream, upstream_started, outcome, usage_status,
                 usage_source, created_at_ms, updated_at_ms, record_origin, upstream_protocol)
                 VALUES('proxy-clear','run-clear','relay','p','r','P','R','m','m',1000,1000,0,1,1,
                 'completed','missing','none',1000,1000,'proxy','openai_chat')",
                [],
            )
            .unwrap();
        writer
            .conn
            .query_row(
                "SELECT id FROM usage_requests WHERE request_id = 'proxy-clear'",
                [],
                |row| row.get(0),
            )
            .unwrap()
    }

    fn clear_task() -> Maintenance {
        Maintenance {
            task_id: 0,
            kind: MaintenanceKind::Clear,
            cutoff_ms: i64::MAX,
            as_of_id: i64::MAX,
            filter: None,
            affected: 0,
            cursor_id: 0,
            started_at_ms: 0,
            batch: 256,
        }
    }

    /// Inserts one proxy request that never finished.
    fn seed_in_flight_proxy_row(writer: &Writer, request_id: &str) -> i64 {
        writer
            .conn
            .execute(
                "INSERT OR IGNORE INTO stats_runs(run_id, started_at_ms) VALUES('run-orphan', 1000)",
                [],
            )
            .unwrap();
        writer
            .conn
            .execute(
                "INSERT INTO usage_requests(request_id, run_id, source, provider_id, route_id,
                 provider_name, route_name, requested_model, upstream_model, started_at_ms, stream,
                 outcome, usage_status, usage_source, created_at_ms, updated_at_ms,
                 record_origin, upstream_protocol)
                 VALUES(?1,'run-orphan','relay','p','r','P','R','m','m',1000,1,
                 'in_flight','missing','none',1000,1000,'proxy','openai_chat')",
                rusqlite::params![request_id],
            )
            .unwrap();
        writer
            .conn
            .query_row(
                "SELECT id FROM usage_requests WHERE request_id = ?1",
                rusqlite::params![request_id],
                |row| row.get(0),
            )
            .unwrap()
    }

    fn cost_rows(writer: &Writer) -> i64 {
        writer
            .conn
            .query_row("SELECT COUNT(*) FROM usage_costs", [], |row| row.get(0))
            .unwrap()
    }

    #[test]
    fn an_imported_row_never_creates_a_cost_row() {
        // AC055: the importer writes facts, never money.
        let folder = tempfile::tempdir().unwrap();
        let (writer, _) = Writer::open(
            folder.path(),
            "import-cost-isolation",
            Arc::new(Counters::default()),
        )
        .unwrap();
        let before = cost_rows(&writer);
        seed_imported_row(&writer, &"7".repeat(64));
        assert_eq!(
            cost_rows(&writer),
            before,
            "an imported fact row creates no cost row"
        );
    }

    #[test]
    fn orphan_recovery_only_settles_proxy_in_flight_rows() {
        // AC055: an imported row is `recorded`, so it can never be an orphan;
        // even when its id is offered, nothing happens to it.
        let folder = tempfile::tempdir().unwrap();
        let (mut writer, _) = Writer::open(
            folder.path(),
            "orphan-isolation",
            Arc::new(Counters::default()),
        )
        .unwrap();
        seed_in_flight_proxy_row(&writer, "proxy-orphan");
        let entry_key = "6".repeat(64);
        seed_imported_row(&writer, &entry_key);

        let settled = writer
            .settle_orphans(&["proxy-orphan".to_owned(), entry_key.clone()], 5000)
            .unwrap();
        assert_eq!(settled, 1, "only the proxy row was settled");

        let proxy_outcome: String = writer
            .conn
            .query_row(
                "SELECT outcome FROM usage_requests WHERE request_id = 'proxy-orphan'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(proxy_outcome, "interrupted");
        let imported_outcome: String = writer
            .conn
            .query_row(
                "SELECT outcome FROM usage_requests WHERE request_id = ?1",
                rusqlite::params![entry_key],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            imported_outcome, "recorded",
            "an imported row is never interrupted by recovery"
        );
    }

    #[test]
    fn an_import_error_does_not_change_proxy_health() {
        // AC055: what the proxy reports about itself is built from its own
        // facts — `DbStats` carries counts and sizes only, so an import error
        // code stored on a checkpoint cannot reach it.
        let folder = tempfile::tempdir().unwrap();
        let (mut writer, _) = Writer::open(
            folder.path(),
            "import-health-isolation",
            Arc::new(Counters::default()),
        )
        .unwrap();
        writer.refresh_db_stats().unwrap();
        let before = writer.db_stats;
        writer
            .conn
            .execute(
                "INSERT INTO workbuddy_import_files(file_key, namespace_id, relative_path,
                 observed_size, modified_ms, last_seen_at_ms, last_error_code)
                 VALUES(?1, ?2, 'projects/demo/session.jsonl', 0, 5, 5, 'permission_denied')",
                rusqlite::params![format!("{:0<64}", "health"), "a".repeat(48)],
            )
            .unwrap();
        writer.refresh_db_stats().unwrap();
        assert_eq!(
            writer.db_stats.record_count, before.record_count,
            "an import error adds no proxy record"
        );
        assert_eq!(
            writer.db_stats.db_bytes, before.db_bytes,
            "and changes nothing else the proxy reports"
        );
        assert_eq!(
            writer.db_stats.earliest_record_at_ms,
            before.earliest_record_at_ms
        );
    }

    #[test]
    fn the_preview_counts_exactly_what_the_sweep_deletes() {
        // AC051: what was previewed is what gets deleted — no more, no less.
        let folder = tempfile::tempdir().unwrap();
        let (mut writer, _) = Writer::open(
            folder.path(),
            "preview-exact",
            Arc::new(Counters::default()),
        )
        .unwrap();
        for index in 0..3 {
            seed_imported_row(&writer, &format!("{index}").repeat(64));
        }
        let watermark: i64 = writer
            .conn
            .query_row("SELECT MAX(id) FROM usage_requests", [], |row| row.get(0))
            .unwrap();
        let preview = Writer::preview_clear(&writer.conn, watermark, i64::MAX).unwrap();
        assert_eq!(preview, 3);

        let mut task = clear_task();
        task.as_of_id = watermark;
        let (deleted, _) = writer.delete_finished(&mut task).unwrap();
        assert_eq!(deleted, preview, "the sweep deleted exactly the preview");
    }

    #[test]
    fn rows_added_after_the_preview_survive_the_sweep() {
        // AC051: a record that arrives after the preview has a larger id and is
        // outside the confirmed watermark, so it is never deleted.
        let folder = tempfile::tempdir().unwrap();
        let (mut writer, _) =
            Writer::open(folder.path(), "preview-race", Arc::new(Counters::default())).unwrap();
        seed_imported_row(&writer, &"1".repeat(64));
        let watermark: i64 = writer
            .conn
            .query_row("SELECT MAX(id) FROM usage_requests", [], |row| row.get(0))
            .unwrap();
        let preview = Writer::preview_clear(&writer.conn, watermark, i64::MAX).unwrap();
        assert_eq!(preview, 1);

        // The importer commits another row while the user is confirming.
        seed_imported_row(&writer, &"2".repeat(64));

        let mut task = clear_task();
        task.as_of_id = watermark;
        let (deleted, _) = writer.delete_finished(&mut task).unwrap();
        assert_eq!(deleted, preview, "only the previewed row went");
        let survivors: i64 = writer
            .conn
            .query_row(
                "SELECT COUNT(*) FROM usage_requests WHERE request_id = ?1",
                rusqlite::params!["2".repeat(64)],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(survivors, 1, "the late row is still there");
    }

    #[test]
    fn a_late_record_inside_the_watermark_survives_the_cutoff() {
        // AC051: a record whose id is in range but which started after the
        // preview time is excluded by the cutoff.
        let folder = tempfile::tempdir().unwrap();
        let (mut writer, _) =
            Writer::open(folder.path(), "preview-late", Arc::new(Counters::default())).unwrap();
        seed_imported_row(&writer, &"3".repeat(64));
        writer
            .conn
            .execute(
                "UPDATE usage_requests SET started_at_ms = 9000, ended_at_ms = 9000",
                [],
            )
            .unwrap();
        let watermark: i64 = writer
            .conn
            .query_row("SELECT MAX(id) FROM usage_requests", [], |row| row.get(0))
            .unwrap();
        // The preview was taken at 5000 ms, before the record started.
        assert_eq!(
            Writer::preview_clear(&writer.conn, watermark, 5000).unwrap(),
            0
        );

        let mut task = clear_task();
        task.as_of_id = watermark;
        task.cutoff_ms = 5000;
        let (deleted, _) = writer.delete_finished(&mut task).unwrap();
        assert_eq!(deleted, 0, "a late record is not swept");
        let survivors: i64 = writer
            .conn
            .query_row("SELECT COUNT(*) FROM usage_requests", [], |row| row.get(0))
            .unwrap();
        assert_eq!(survivors, 1);
    }

    #[test]
    fn the_importer_is_held_back_while_maintenance_runs() {
        // AC051: the importer yields before it even tries to take the write lock.
        let folder = tempfile::tempdir().unwrap();
        let (mut writer, _) = Writer::open(
            folder.path(),
            "maintenance-gate",
            Arc::new(Counters::default()),
        )
        .unwrap();
        assert!(!writer.is_maintenance_running());
        writer.maintenance = Some(clear_task());
        assert!(writer.is_maintenance_running());
        assert!(
            !writer.maybe_start_retention(1_000_000).unwrap(),
            "a second task never starts on top of a running one"
        );
    }

    #[test]
    fn maintenance_failure_keeps_rows_and_retry_can_complete() {
        let folder = tempfile::tempdir().unwrap();
        let (mut writer, _) = Writer::open(
            folder.path(),
            "maintenance-failure",
            Arc::new(Counters::default()),
        )
        .unwrap();
        seed_imported_row(&writer, &"7".repeat(64));
        writer
            .conn
            .execute(
                "CREATE TRIGGER r06_abort_delete BEFORE DELETE ON usage_requests \
                 BEGIN SELECT RAISE(ABORT, 'synthetic delete abort'); END",
                [],
            )
            .unwrap();
        let task_id = writer
            .insert_maintenance(MaintenanceKind::Clear, 1000)
            .unwrap();
        let mut task = clear_task();
        task.task_id = task_id;
        writer.maintenance = Some(task);

        assert!(writer.maintenance_step().is_err());
        let rows: i64 = writer
            .conn
            .query_row("SELECT COUNT(*) FROM usage_requests", [], |row| row.get(0))
            .unwrap();
        assert_eq!(rows, 1, "failed clear preserves history");
        let (status, error): (String, Option<String>) = writer
            .conn
            .query_row(
                "SELECT status, error_code FROM stats_maintenance_events WHERE id=?1",
                rusqlite::params![task_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(status, "failed");
        assert_eq!(error.as_deref(), Some("maintenance_failed"));

        writer
            .conn
            .execute("DROP TRIGGER r06_abort_delete", [])
            .unwrap();
        let retry_id = writer
            .insert_maintenance(MaintenanceKind::Clear, 1001)
            .unwrap();
        let mut retry = clear_task();
        retry.task_id = retry_id;
        writer.maintenance = Some(retry);
        assert!(!writer.maintenance_step().unwrap());
        let rows: i64 = writer
            .conn
            .query_row("SELECT COUNT(*) FROM usage_requests", [], |row| row.get(0))
            .unwrap();
        assert_eq!(rows, 0, "retry removes the preserved history");
        let status: String = writer
            .conn
            .query_row(
                "SELECT status FROM stats_maintenance_events WHERE id=?1",
                rusqlite::params![retry_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(status, "completed");
    }

    #[test]
    fn a_retention_sweep_tombstones_imported_rows_with_its_own_state() {
        // AC050: the automatic sweep marks `deleted_retention`, so it is
        // distinguishable from a manual clear and equally permanent.
        let folder = tempfile::tempdir().unwrap();
        let (mut writer, _) = Writer::open(
            folder.path(),
            "retention-tombstone",
            Arc::new(Counters::default()),
        )
        .unwrap();
        let entry_key = "9".repeat(64);
        seed_imported_row(&writer, &entry_key);
        let mut task = clear_task();
        task.kind = MaintenanceKind::Retention;
        let (deleted, _) = writer.delete_finished(&mut task).unwrap();
        assert_eq!(deleted, 1);

        let state: String = writer
            .conn
            .query_row(
                "SELECT state FROM workbuddy_import_ledger WHERE entry_key = ?1",
                rusqlite::params![entry_key],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(state, "deleted_retention");
    }

    #[test]
    fn a_manual_clear_tombstones_imported_rows_so_a_rescan_cannot_resurrect_them() {
        // AC049: the ledger entry becomes `deleted_manual` before the fact row
        // goes, so the same message coming back from a rescan is a tombstone,
        // not a new import.
        let folder = tempfile::tempdir().unwrap();
        let (mut writer, _) = Writer::open(
            folder.path(),
            "clear-tombstone",
            Arc::new(Counters::default()),
        )
        .unwrap();
        let entry_key = "e".repeat(64);
        seed_imported_row(&writer, &entry_key);
        let mut task = clear_task();
        let (deleted, _) = writer.delete_finished(&mut task).unwrap();
        assert_eq!(deleted, 1, "the fact row is gone");

        let (state, request_id): (String, Option<String>) = writer
            .conn
            .query_row(
                "SELECT state, request_id FROM workbuddy_import_ledger WHERE entry_key = ?1",
                rusqlite::params![entry_key],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(state, "deleted_manual", "the tombstone is kept");
        assert_eq!(request_id, None, "and holds no request id");
        let facts: i64 = writer
            .conn
            .query_row(
                "SELECT COUNT(*) FROM usage_requests WHERE request_id = ?1",
                rusqlite::params![entry_key],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(facts, 0);
    }

    #[test]
    fn a_manual_clear_leaves_proxy_rows_and_their_semantics_alone() {
        // AC049: proxy rows have no ledger entry, so nothing about the original
        // clearing semantics changes for them.
        let folder = tempfile::tempdir().unwrap();
        let (mut writer, _) =
            Writer::open(folder.path(), "clear-proxy", Arc::new(Counters::default())).unwrap();
        let entry_key = "f".repeat(64);
        let imported_id = seed_imported_row(&writer, &entry_key);
        let proxy_id = seed_proxy_row(&writer);
        assert!(proxy_id > imported_id, "the proxy row was inserted later");

        // A clear whose watermark stops at the imported row leaves the proxy
        // row alone — the original clearing semantics are untouched.
        let mut narrow = clear_task();
        narrow.as_of_id = imported_id;
        let (deleted, _) = writer.delete_finished(&mut narrow).unwrap();
        assert_eq!(deleted, 1, "only the imported row was in range");
        let proxy_rows: i64 = writer
            .conn
            .query_row(
                "SELECT COUNT(*) FROM usage_requests WHERE record_origin = 'proxy'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(proxy_rows, 1, "the proxy row is untouched");
        let ledger: i64 = writer
            .conn
            .query_row(
                "SELECT COUNT(*) FROM workbuddy_import_ledger WHERE state = 'deleted_manual'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(ledger, 1, "and the imported row still left its tombstone");
    }

    #[test]
    fn catalogue_updates_overwrite_prices_but_cached_route_seeding_preserves_edits() {
        let folder = tempfile::tempdir().unwrap();
        let (mut writer, _) = Writer::open(
            folder.path(),
            "catalogue-test",
            Arc::new(Counters::default()),
        )
        .unwrap();
        let prices = crate::price_catalog::Prices::parse(
            br#"{"lab":{"models":{"m":{"cost":{"input":2,"output":4}}}}}"#,
        )
        .unwrap();
        let target = |provider: &str| crate::price_catalog::Target {
            provider_id: provider.into(),
            model: "m".into(),
            upstream: "https://custom.test/v1".into(),
        };
        import_price_catalog(
            &mut writer,
            Some(prices.clone()),
            &[target("p")],
            "2026-10-16",
            "one",
            1000,
        )
        .unwrap();
        writer
            .conn
            .execute(
                "UPDATE stats_model_prices SET input_rate_nanos=9000000000",
                [],
            )
            .unwrap();
        import_price_catalog(
            &mut writer,
            None,
            &[target("p"), target("new")],
            "2026-10-16",
            "two",
            2000,
        )
        .unwrap();
        assert_eq!(
            writer.prices.get("p", "m").unwrap().input_rate_nanos,
            9_000_000_000
        );
        assert_eq!(
            writer.prices.get("new", "m").unwrap().input_rate_nanos,
            2_000_000_000
        );
        let cached = price_catalog_status(&writer.conn).unwrap();
        assert_eq!(cached.updated_at_ms, Some(1000));
        assert_eq!(cached.targets_key, "two");
        import_price_catalog(
            &mut writer,
            Some(prices),
            &[target("p")],
            "2026-10-17",
            "one",
            3000,
        )
        .unwrap();
        assert_eq!(
            writer.prices.get("p", "m").unwrap().input_rate_nanos,
            2_000_000_000
        );
        assert_eq!(
            price_catalog_status(&writer.conn)
                .unwrap()
                .last_success_day
                .as_deref(),
            Some("2026-10-17")
        );
    }

    #[test]
    fn catalogue_import_failure_rolls_back_prices_and_daily_marker() {
        let folder = tempfile::tempdir().unwrap();
        let (mut writer, _) = Writer::open(
            folder.path(),
            "catalogue-rollback",
            Arc::new(Counters::default()),
        )
        .unwrap();
        let prices = crate::price_catalog::Prices::parse(
            br#"{"lab":{"models":{"m":{"cost":{"input":2,"output":4}}}}}"#,
        )
        .unwrap();
        let targets: Vec<_> = ["p".to_owned(), "z".repeat(161)]
            .into_iter()
            .map(|provider_id| crate::price_catalog::Target {
                provider_id,
                model: "m".into(),
                upstream: String::new(),
            })
            .collect();
        assert!(
            import_price_catalog(
                &mut writer,
                Some(prices),
                &targets,
                "2026-10-16",
                "bad",
                1000
            )
            .is_err()
        );
        let count: i64 = writer
            .conn
            .query_row("SELECT COUNT(*) FROM stats_model_prices", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
        assert_eq!(
            price_catalog_status(&writer.conn).unwrap().last_success_day,
            None
        );
        assert!(writer.prices.get("p", "m").is_none());
    }

    #[test]
    fn idle_loss_counters_are_durable_without_a_later_request_batch() {
        let folder = tempfile::tempdir().unwrap();
        let counters = Arc::new(Counters::default());
        let (writer, _) =
            Writer::open(folder.path(), "run-idle-counters", counters.clone()).unwrap();
        counters.admitted.store(3, Ordering::Relaxed);
        counters.dropped.store(2, Ordering::Relaxed);
        counters.write_failures.store(1, Ordering::Relaxed);
        writer.persist_counters(crate::stats::now_ms()).unwrap();
        let reader = Connection::open(folder.path().join("usage.sqlite3")).unwrap();
        let values:(i64,i64,i64)=reader.query_row("SELECT admitted_requests,dropped_requests,write_failures FROM stats_runs WHERE run_id='run-idle-counters'",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap();
        assert_eq!(values, (3, 2, 1));
        assert_eq!(
            reader
                .query_row("SELECT COUNT(*) FROM usage_requests", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }

    /// Drives the bounded startup scan to completion, exactly as the writer's
    /// recovery probe does between live batches.
    fn run_recovery_scan(writer: &mut Writer, now_ms: i64) -> i64 {
        let Some(mut scan) = writer.scan else {
            return 0;
        };
        let mut rounds = 0;
        let mut repaired = 0i64;
        while !scan.finished() && rounds < 10_000 {
            let (done, _) =
                scan_recovery_batch(writer, &mut scan, crate::stats::RECOVERY_BATCH, now_ms)
                    .unwrap();
            repaired += done;
            rounds += 1;
        }
        writer.scan = Some(scan);
        assert!(scan.finished(), "the scan must terminate");
        repaired
    }
    use crate::stats::dto::{InputBreakdown, Outcome, Source, UsageStatus};
    use crate::stats::event::{Counters, FinishRecord, RequestMeta, StatsEvent};
    use std::sync::Arc;

    fn meta(request_id: &str, run_id: &str) -> RequestMeta {
        RequestMeta {
            workbuddy_identity: None,
            request_id: request_id.into(),
            run_id: run_id.to_owned(),
            source: Source::Relay,
            provider_id: "provider-a".into(),
            provider_name: "Provider".into(),
            route_id: "route-0".into(),
            route_name: "Route".into(),
            requested_model: "model-x".into(),
            upstream_model: "model-x".into(),
            upstream_protocol: "openai_chat".into(),
            stream: true,
            started_at_ms: crate::stats::now_ms(),
            price: PriceSnapshot::default_zero("CNY", crate::stats::now_ms()),
        }
    }

    fn finish(
        usage_status: UsageStatus,
        numbers: (Option<i64>, Option<i64>, Option<i64>),
    ) -> FinishRecord {
        let now = crate::stats::now_ms();
        FinishRecord {
            ended_at_ms: now,
            duration_ms: 12,
            ttfb_ms: Some(3),
            first_content_ms: Some(4),
            upstream_started: true,
            response_model: None,
            http_status: Some(200),
            outcome: Outcome::Completed,
            error_kind: None,
            usage_status,
            usage_reason: None,
            usage_source: if usage_status == UsageStatus::Complete {
                crate::stats::dto::UsageSource::Upstream
            } else {
                crate::stats::dto::UsageSource::None
            },
            input_tokens: numbers.0,
            output_tokens: numbers.1,
            total_tokens: numbers.2,
            cache_read_tokens: None,
            cache_write_tokens: None,
            reasoning_tokens: None,
            input_uncached_tokens: None,
            input_breakdown: InputBreakdown::Unknown,
            updated_at_ms: now,
        }
    }

    /// P01: the v1 -> v2 migration must keep every recorded token and amount,
    /// turn v1 `unpriced` into the new explicit zero, and leave no dangling
    /// reference behind.
    #[test]
    fn the_v1_to_v2_migration_preserves_history() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("usage.sqlite3");
        // Build a v1 database by hand, exactly as the previous schema wrote it.
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(MIGRATION_0001).unwrap();
            conn.pragma_update(None, "user_version", 1).unwrap();
            conn.execute(
                "INSERT INTO stats_runs(run_id, started_at_ms, stopped_at_ms, stop_reason,
                     admitted_requests, dropped_requests, write_failures, last_persisted_at_ms)
                 VALUES('run-v1', 1000, 1000, 'normal', 3, 0, 0, 1000)",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO stats_price_rules(rule_id, provider_id, upstream_model, currency,
                     effective_from_ms, input_rate_nanos, output_rate_nanos, input_mode, label,
                     created_at_ms)
                 VALUES('rule-v1','provider-a','model-x','USD',0,2000000000,8000000000,'flat','v1',900)",
                [],
            )
            .unwrap();
            for (id, status) in [
                ("req-priced", "priced"),
                ("req-unpriced", "unpriced"),
                ("req-incomplete", "usage_incomplete"),
            ] {
                conn.execute(
                    "INSERT INTO usage_requests(request_id, run_id, source, provider_id, route_id,
                         provider_name, route_name, requested_model, upstream_model, started_at_ms,
                         stream, outcome, usage_status, input_tokens, output_tokens, total_tokens,
                         ended_at_ms, duration_ms, usage_source, created_at_ms, updated_at_ms)
                     VALUES(?1,'run-v1','relay','provider-a','route-0','P','R','model-x','model-x',
                         1000, 1, 'completed', 'complete', 1000, 100, 1100, 1000, 0, 'upstream', 1000, 1000)",
                    [id],
                )
                .unwrap();
                let (rule, currency, total): (Option<&str>, Option<&str>, Option<i64>) =
                    match status {
                        "priced" => (Some("rule-v1"), Some("USD"), Some(1_360_000)),
                        _ => (None, None, None),
                    };
                conn.execute(
                    "INSERT INTO usage_costs(request_id, rule_id, status, currency,
                         input_cost_nanos, output_cost_nanos, cache_read_cost_nanos,
                         cache_write_cost_nanos, total_cost_nanos, calculated_at_ms)
                     VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,1000)",
                    rusqlite::params![
                        id,
                        rule,
                        status,
                        currency,
                        total.map(|_| 1_000_000i64),
                        total.map(|_| 360_000i64),
                        total.map(|_| 0i64),
                        total.map(|_| 0i64),
                        total,
                    ],
                )
                .unwrap();
            }
            conn.close().unwrap();
        }

        // Migrate, then assert the v2 shape and the preserved history.
        {
            let mut conn = Connection::open(&path).unwrap();
            conn.pragma_update(None, "foreign_keys", "ON").unwrap();
            migrate(&mut conn, folder.path()).unwrap();
            let version: i64 = conn
                .query_row("PRAGMA user_version", [], |row| row.get(0))
                .unwrap();
            // v1 upgrades through v2 to v3 in one pass.
            assert_eq!(version, SCHEMA_VERSION);
            let default_currency: String = conn
                .query_row(
                    "SELECT default_currency FROM stats_settings WHERE singleton=1",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(default_currency, "CNY");
            // The interval rules are replaced by one editable current price.
            let prices: i64 = conn
                .query_row("SELECT COUNT(*) FROM stats_model_prices", [], |row| {
                    row.get(0)
                })
                .unwrap();
            assert_eq!(prices, 1);
            let (mode, input): (String, i64) = conn
                .query_row(
                    "SELECT input_mode, input_rate_nanos FROM stats_model_prices",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .unwrap();
            assert_eq!(mode, "flat");
            // The stored rates are nano units already; they must not be rescaled.
            assert_eq!(input, 2_000_000_000);

            let row = |id: &str| -> (String, String, Option<i64>, Option<i64>) {
                conn.query_row(
                    "SELECT c.status, c.price_source, c.input_rate_nanos, c.total_cost_nanos
                     FROM usage_costs c WHERE c.request_id=?1",
                    [id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                )
                .unwrap()
            };
            // A priced row keeps its amount and gains the rule's unit prices.
            assert_eq!(
                row("req-priced"),
                (
                    "priced".into(),
                    "manual".into(),
                    Some(2_000_000_000),
                    Some(1_360_000)
                )
            );
            // v1 `unpriced` becomes the new explicit zero.
            assert_eq!(
                row("req-unpriced"),
                (
                    "default_zero".into(),
                    "default_zero".into(),
                    Some(0),
                    Some(0)
                )
            );
            // An incomplete row keeps its status and its unknown amount.
            let incomplete = row("req-incomplete");
            assert_eq!(incomplete.0, "usage_incomplete");
            assert!(incomplete.3.is_none(), "{incomplete:?}");
            // Tokens were never touched by the migration.
            let tokens: (Option<i64>, Option<i64>) = conn
                .query_row(
                    "SELECT input_tokens, total_tokens FROM usage_requests WHERE request_id='req-unpriced'",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .unwrap();
            assert_eq!(tokens, (Some(1000), Some(1100)));
            // No dangling reference survived the table rebuild.
            let mut statement = conn.prepare("PRAGMA foreign_key_check").unwrap();
            assert!(statement.query([]).unwrap().next().unwrap().is_none());
            // The reporting view still resolves against the rebuilt table.
            let counted: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM stats_request_facts WHERE cost_status IS NOT NULL",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(counted, 3);
        }
    }

    /// U01/U02: startup repair must keep `upstream_started` three-state and fix
    /// rows an earlier release left `pending` or wrongly free.
    #[test]
    fn startup_repair_settles_abandoned_costs_from_their_own_snapshot() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("usage.sqlite3");
        {
            let mut conn = Connection::open(&path).unwrap();
            conn.pragma_update(None, "foreign_keys", "ON").unwrap();
            migrate(&mut conn, folder.path()).unwrap();
            conn.execute(
                "INSERT INTO stats_runs(run_id, started_at_ms, stopped_at_ms, stop_reason,
                     admitted_requests, dropped_requests, write_failures, last_persisted_at_ms)
                 VALUES('run-old', 1000, 1000, 'unclean', 5, 0, 0, 1000)",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO stats_model_prices(price_id, provider_id, upstream_model, currency,
                     input_mode, input_rate_nanos, output_rate_nanos, revision, label,
                     created_at_ms, updated_at_ms)
                 VALUES('price-manual','p1','model-priced','USD','flat',2000000000,8000000000,1,'m',900,900)",
                [],
            )
            .unwrap();
            // (id, outcome, upstream_started, usage_status, cost status, price_source,
            //  total, expected status, expected total)
            #[allow(clippy::type_complexity)]
            let rows: [(&str, &str, Option<i64>, &str, &str, &str, Option<i64>); 5] = [
                // An earlier release never wrote the upstream bit, so it is unknown.
                (
                    "a-unknown",
                    "interrupted",
                    None,
                    "missing",
                    "pending",
                    "manual",
                    None,
                ),
                // Already (wrongly) marked free although the upstream bit is unknown.
                (
                    "b-wrong-free",
                    "interrupted",
                    None,
                    "missing",
                    "not_applicable",
                    "manual",
                    Some(0),
                ),
                // A priced request that provably never started stays free.
                (
                    "c-never-sent",
                    "interrupted",
                    Some(0),
                    "missing",
                    "pending",
                    "manual",
                    None,
                ),
                // No configured price: a confirmed zero.
                (
                    "d-default",
                    "interrupted",
                    None,
                    "missing",
                    "pending",
                    "default_zero",
                    None,
                ),
                // Finished and settled: untouched.
                (
                    "e-done",
                    "completed",
                    Some(1),
                    "complete",
                    "priced",
                    "manual",
                    Some(1_360_000),
                ),
            ];
            for (id, outcome, started, usage, status, source, total) in rows {
                let ended = if outcome == "in_flight" {
                    None
                } else {
                    Some(1000)
                };
                conn.execute(
                    "INSERT INTO usage_requests(request_id, run_id, source, provider_id, route_id,
                         provider_name, route_name, requested_model, upstream_model, started_at_ms,
                         stream, outcome, upstream_started, ended_at_ms, duration_ms, usage_status,
                         usage_source, input_tokens, output_tokens, total_tokens, created_at_ms,
                         updated_at_ms)
                     VALUES(?1,'run-old','relay','p1','r0','P','R','model-priced','model-priced',
                         1000, 1, ?2, ?3, ?4, 0, ?5, 'upstream', 1000, 100, 1100, 1000, 1000)",
                    rusqlite::params![id, outcome, started, ended, usage],
                )
                .unwrap();
                // A `default_zero` row must carry all-zero rates and no source id.
                let (currency, input_rate, output_rate) = if source == "default_zero" {
                    (Some("CNY"), 0i64, 0i64)
                } else {
                    (Some("USD"), 2_000_000_000i64, 8_000_000_000i64)
                };
                // A settled row must balance, an unknown amount is all NULL and
                // a confirmed zero is all zero.
                let (input, output, total): (Option<i64>, Option<i64>, Option<i64>) =
                    if status == "pending" {
                        (None, None, None)
                    } else {
                        match total {
                            Some(0) | None => (Some(0), Some(0), Some(0)),
                            Some(value) => (Some(value - 360_000), Some(360_000), Some(value)),
                        }
                    };
                conn.execute(
                    "INSERT INTO usage_costs(request_id, status, currency, price_source,
                         input_mode, input_rate_nanos, output_rate_nanos,
                         cache_read_rate_nanos, cache_write_rate_nanos, price_snapshot_at_ms,
                         input_cost_nanos, output_cost_nanos, cache_read_cost_nanos,
                         cache_write_cost_nanos, total_cost_nanos, calculated_at_ms,
                         pricing_version)
                     VALUES(?1,?2,?3,?4,'flat',?5,?10,0,0,1000,?6,?7,?9,?9,?8,1000,2)",
                    rusqlite::params![
                        id,
                        status,
                        currency,
                        source,
                        input_rate,
                        input,
                        output,
                        total,
                        input.map(|_| 0i64),
                        output_rate,
                    ],
                )
                .unwrap();
            }
            let counters = Arc::new(Counters::default());
            let (mut writer, recovery) = Writer::open(folder.path(), "run-new", counters).unwrap();
            assert_eq!(recovery.as_of_id, 5, "{recovery:?}");
            assert_eq!(run_recovery_scan(&mut writer, 2000), 4);
            fn read(conn: &Connection, id: &str) -> (String, Option<i64>) {
                conn.query_row(
                    "SELECT status, total_cost_nanos FROM usage_costs WHERE request_id=?1",
                    [id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .unwrap()
            }
            {
                let conn = &writer.conn;
                // Unknown upstream plus a manual price and no usage: unknown amount.
                assert_eq!(read(conn, "a-unknown"), ("usage_incomplete".into(), None));
                // A row an earlier release wrongly marked free is corrected too.
                assert_eq!(
                    read(conn, "b-wrong-free"),
                    ("usage_incomplete".into(), None)
                );
                // A provable "never sent" stays a confirmed zero.
                assert_eq!(
                    read(conn, "c-never-sent"),
                    ("not_applicable".into(), Some(0))
                );
                // No configured price is still a confirmed zero.
                assert_eq!(read(conn, "d-default"), ("default_zero".into(), Some(0)));
                // Settled history is untouched.
                assert_eq!(read(conn, "e-done"), ("priced".into(), Some(1_360_000)));
            }
            // Repairs are idempotent: a second scan has nothing left to do.
            assert_eq!(run_recovery_scan(&mut writer, 3000), 0);
            {
                let conn = &writer.conn;
                assert_eq!(read(conn, "a-unknown"), ("usage_incomplete".into(), None));
                assert_eq!(read(conn, "e-done"), ("priced".into(), Some(1_360_000)));
                assert!(
                    conn.query_row("PRAGMA foreign_key_check", [], |_| Ok(()))
                        .is_err()
                );
            }
        }
    }

    /// R8-04: a batch that cannot repair a row must queue it before the scan
    /// watermark moves past it, and a failed commit must not advance at all.
    #[test]
    fn a_failed_repair_is_queued_before_the_scan_advances() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("usage.sqlite3");
        {
            let mut conn = Connection::open(&path).unwrap();
            conn.pragma_update(None, "foreign_keys", "ON").unwrap();
            migrate(&mut conn, folder.path()).unwrap();
            conn.execute(
                "INSERT INTO stats_runs(run_id, started_at_ms, stopped_at_ms, stop_reason,
                     admitted_requests, dropped_requests, write_failures, last_persisted_at_ms)
                 VALUES('run-r8', 1000, 1000, 'unclean', 4, 0, 0, 1000)",
                [],
            )
            .unwrap();
            for index in 0..4 {
                let id = format!("r8-{index}");
                conn.execute(
                    "INSERT INTO usage_requests(request_id, run_id, source, provider_id,
                         route_id, provider_name, route_name, requested_model, upstream_model,
                         started_at_ms, stream, outcome, ended_at_ms, duration_ms, usage_status,
                         usage_source, created_at_ms, updated_at_ms)
                     VALUES(?1,'run-r8','relay','p1','r0','P','R','m','m',
                         1000, 1, 'interrupted', 1000, 0, 'missing', 'none', 1000, 1000)",
                    [&id],
                )
                .unwrap();
                conn.execute(
                    "INSERT INTO usage_costs(request_id, status, currency, price_source,
                         input_mode, input_rate_nanos, output_rate_nanos,
                         cache_read_rate_nanos, cache_write_rate_nanos, price_snapshot_at_ms,
                         pricing_version, calculated_at_ms)
                     VALUES(?1,'pending','CNY','default_zero','flat',0,0,0,0,1000,2,1000)",
                    [&id],
                )
                .unwrap();
            }
        }
        let counters = Arc::new(Counters::default());
        let (mut writer, recovery) = Writer::open(folder.path(), "run-new", counters).unwrap();
        assert_eq!(recovery.as_of_id, 4);
        // A trigger refuses the cost update, exactly like a locked table.
        writer
            .conn
            .execute_batch(
                "CREATE TRIGGER refuse_cost BEFORE UPDATE OF status ON usage_costs
                 BEGIN SELECT RAISE(ABORT, 'cost writes refused'); END;",
            )
            .unwrap();
        let mut scan = RecoveryScan::new(recovery.as_of_id);
        let (repaired, _) = scan_recovery_batch(&mut writer, &mut scan, 2, 2000).unwrap();
        assert_eq!(repaired, 0, "nothing could be repaired");
        // The two rows this batch touched are queued, not forgotten.
        let queued: i64 = writer
            .conn
            .query_row("SELECT COUNT(*) FROM stats_recovery_backlog", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(queued, 2, "an unrepairable row must be queued durably");
        let pending: i64 = writer
            .conn
            .query_row(
                "SELECT COUNT(*) FROM usage_costs WHERE status='pending'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(pending, 4, "the refused rows stay pending");
        // The scan cannot claim to be finished while rows still owe a state.
        let scan_done = scan.finished();
        let backlog = backlog_len(&writer.conn).unwrap();
        assert!(
            !scan_done || backlog > 0,
            "a finished scan with pending rows and an empty queue would be a lie"
        );
        // Once the fault is gone the same run finishes the job with no new
        // traffic: the scan walks the rest and the queue drains.
        writer
            .conn
            .execute_batch("DROP TRIGGER refuse_cost")
            .unwrap();
        let mut rounds = 0;
        while (!scan.finished() || backlog_len(&writer.conn).unwrap() > 0) && rounds < 50 {
            if !scan.finished() {
                let _ = scan_recovery_batch(&mut writer, &mut scan, 2, 3000).unwrap();
            }
            if backlog_len(&writer.conn).unwrap() > 0 {
                repair_backlog_batch(&mut writer, 256, 3000).unwrap();
            }
            rounds += 1;
        }
        assert!(
            scan.finished(),
            "the scan must finish once the fault is gone"
        );
        assert_eq!(backlog_len(&writer.conn).unwrap(), 0);
        let pending: i64 = writer
            .conn
            .query_row(
                "SELECT COUNT(*) FROM usage_costs WHERE status='pending'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(pending, 0, "the queued rows must really be repaired");
    }

    /// S02: a backlog larger than one batch must be *fully* repaired at
    /// startup. Taking only the first 256 rows and clearing the queue made the
    /// database look healthy while 944 costs stayed `pending` forever.
    #[test]
    fn a_large_legacy_backlog_is_fully_repaired_at_startup() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("usage.sqlite3");
        {
            let mut conn = Connection::open(&path).unwrap();
            conn.pragma_update(None, "foreign_keys", "ON").unwrap();
            migrate(&mut conn, folder.path()).unwrap();
            conn.execute(
                "INSERT INTO stats_runs(run_id, started_at_ms, stopped_at_ms, stop_reason,
                     admitted_requests, dropped_requests, write_failures, last_persisted_at_ms)
                 VALUES('run-legacy', 1000, 1000, 'unclean', 20000, 0, 0, 1000)",
                [],
            )
            .unwrap();
            conn.execute_batch("BEGIN").unwrap();
            // Well past any single-pass batch count: an earlier release repaired
            // 16640 of these, left 3360 `pending` and still reported `ready`.
            for index in 0..20_000 {
                let id = format!("legacy-{index}");
                conn.execute(
                    "INSERT INTO usage_requests(request_id, run_id, source, provider_id,
                         route_id, provider_name, route_name, requested_model, upstream_model,
                         started_at_ms, stream, outcome, ended_at_ms, duration_ms, usage_status,
                         usage_source, created_at_ms, updated_at_ms)
                     VALUES(?1,'run-legacy','relay','p1','r0','P','R','m','m',
                         1000, 1, 'interrupted', 1000, 0, 'missing', 'none', 1000, 1000)",
                    [&id],
                )
                .unwrap();
                // A terminal request whose cost never settled: exactly the shape
                // an earlier release left behind.
                conn.execute(
                    "INSERT INTO usage_costs(request_id, status, currency, price_source,
                         input_mode, input_rate_nanos, output_rate_nanos,
                         cache_read_rate_nanos, cache_write_rate_nanos, price_snapshot_at_ms,
                         pricing_version, calculated_at_ms)
                     VALUES(?1,'pending','CNY','default_zero','flat',0,0,0,0,1000,2,1000)",
                    [&id],
                )
                .unwrap();
            }
            conn.execute_batch("COMMIT").unwrap();
            let counters = Arc::new(Counters::default());
            let (mut writer, recovery) = Writer::open(folder.path(), "run-new", counters).unwrap();
            assert_eq!(recovery.as_of_id, 20_000, "{recovery:?}");
            let repaired = run_recovery_scan(&mut writer, 2000);
            assert!(repaired >= 20_000, "only {repaired} of 20000 were repaired");
            let conn = &writer.conn;
            let _ = &conn;
            let pending: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM usage_costs WHERE status='pending'",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(pending, 0, "every legacy pending cost must be settled");
            let queued: i64 = conn
                .query_row("SELECT COUNT(*) FROM stats_recovery_backlog", [], |row| {
                    row.get(0)
                })
                .unwrap();
            assert_eq!(queued, 0, "the queue must be empty when the scan finished");
            // Everything is a confirmed zero: the snapshot had no price.
            let zeros: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM usage_costs WHERE status='default_zero'
                       AND total_cost_nanos=0",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(zeros, 20_000);
            assert!(
                writer.scan.is_some_and(|scan| scan.finished()),
                "a finished scan is what makes `ready` honest"
            );
        }
    }

    /// S01: an existing v2 database must upgrade by running only the missing
    /// step. Replaying 0002 made `migrate` fail with "table stats_model_prices
    /// already exists" and left collection disabled.
    #[test]
    fn every_schema_generation_upgrades_in_place_and_is_idempotent() {
        for start in 0..=6i64 {
            let folder = tempfile::tempdir().unwrap();
            let path = folder.path().join("usage.sqlite3");
            {
                let mut conn = Connection::open(&path).unwrap();
                if start >= 1 {
                    conn.execute_batch(MIGRATION_0001).unwrap();
                    conn.pragma_update(None, "user_version", 1).unwrap();
                }
                if start >= 2 {
                    // Build a genuine v2 database exactly as the released
                    // migration wrote it.
                    conn.pragma_update(None, "foreign_keys", "OFF").unwrap();
                    let tx = conn
                        .transaction_with_behavior(TransactionBehavior::Immediate)
                        .unwrap();
                    tx.execute_batch(MIGRATION_0002).unwrap();
                    tx.pragma_update(None, "user_version", 2).unwrap();
                    tx.commit().unwrap();
                    conn.pragma_update(None, "foreign_keys", "ON").unwrap();
                }
                if start >= 3 {
                    apply_step(&mut conn, 3, MIGRATION_0003).unwrap();
                }
                if start >= 4 {
                    apply_step(&mut conn, 4, MIGRATION_0004).unwrap();
                    conn.execute(
                        "INSERT INTO stats_provider_settings(provider_id, usage_mode, updated_at_ms, revision)
                         VALUES('opted-out', 'observe', 1234, 7)", [],
                    ).unwrap();
                }
                if start >= 5 {
                    apply_step(&mut conn, 5, MIGRATION_0005).unwrap();
                }
                if start >= 6 {
                    // A genuine v6 database: the catalog table must not be
                    // rebuilt when a v6 file upgrades to v7.
                    apply_step(&mut conn, 6, MIGRATION_0006).unwrap();
                }
            }
            let mut conn = Connection::open(&path).unwrap();
            migrate(&mut conn, folder.path())
                .unwrap_or_else(|failure| panic!("v{start} did not upgrade: {}", failure.message));
            let version: i64 = conn
                .query_row("PRAGMA user_version", [], |row| row.get(0))
                .unwrap();
            assert_eq!(version, SCHEMA_VERSION, "started at v{start}");
            // Every migration step is idempotent: a second start is a no-op.
            migrate(&mut conn, folder.path()).unwrap();
            let again: i64 = conn
                .query_row("PRAGMA user_version", [], |row| row.get(0))
                .unwrap();
            assert_eq!(again, SCHEMA_VERSION);
            // The v3 step exists and the v2 tables survived untouched.
            let backlog: i64 = conn
                .query_row("SELECT COUNT(*) FROM stats_recovery_backlog", [], |row| {
                    row.get(0)
                })
                .unwrap();
            assert_eq!(backlog, 0);
            let prices: i64 = conn
                .query_row("SELECT COUNT(*) FROM stats_model_prices", [], |row| {
                    row.get(0)
                })
                .unwrap();
            let costs: i64 = conn
                .query_row("SELECT COUNT(*) FROM usage_costs", [], |row| row.get(0))
                .unwrap();
            assert_eq!((prices, costs), (0, 0));
            conn.execute(
                "INSERT INTO stats_provider_settings(provider_id, updated_at_ms) VALUES('new', 0)",
                [],
            )
            .unwrap();
            let mode: String = conn
                .query_row(
                    "SELECT usage_mode FROM stats_provider_settings WHERE provider_id='new'",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(mode, "request", "SQL default after upgrade from v{start}");
            if start >= 4 {
                let preserved: (String, i64, i64) = conn.query_row(
                    "SELECT usage_mode, updated_at_ms, revision FROM stats_provider_settings WHERE provider_id='opted-out'", [],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                ).unwrap();
                assert_eq!(preserved, ("observe".into(), 1234, 7));
            }
        }
    }

    /// S02.02 / AC010: a genuine v6 database upgrades to v7 without losing the
    /// cost rows whose foreign key points at the rebuilt table, a repeated start
    /// is a no-op, foreign keys are restored, and a refused step never advances
    /// `user_version`.
    #[test]
    fn v7_upgrade_keeps_cost_rows_and_a_refused_step_never_advances_the_version() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("usage.sqlite3");
        {
            let mut conn = Connection::open(&path).unwrap();
            conn.execute_batch(MIGRATION_0001).unwrap();
            conn.pragma_update(None, "user_version", 1).unwrap();
            conn.pragma_update(None, "foreign_keys", "OFF").unwrap();
            let tx = conn
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .unwrap();
            tx.execute_batch(MIGRATION_0002).unwrap();
            tx.pragma_update(None, "user_version", 2).unwrap();
            tx.commit().unwrap();
            conn.pragma_update(None, "foreign_keys", "ON").unwrap();
            apply_step(&mut conn, 3, MIGRATION_0003).unwrap();
            apply_step(&mut conn, 4, MIGRATION_0004).unwrap();
            apply_step(&mut conn, 5, MIGRATION_0005).unwrap();
            apply_step(&mut conn, 6, MIGRATION_0006).unwrap();
            conn.execute(
                "INSERT INTO stats_runs(run_id, started_at_ms) VALUES('run-v6', 1000)",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO usage_requests(request_id,run_id,source,provider_id,route_id,provider_name,
                 route_name,requested_model,upstream_model,started_at_ms,ended_at_ms,duration_ms,stream,
                 outcome,usage_status,usage_source,created_at_ms,updated_at_ms,upstream_protocol)
                 VALUES('req-v6','run-v6','relay','p','r','P','R','m','m',1000,2000,1000,1,'completed',
                 'missing','none',1000,2000,'openai_chat')",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO usage_costs(request_id,price_source,price_snapshot_at_ms,currency,input_mode,
                 input_rate_nanos,output_rate_nanos,cache_read_rate_nanos,cache_write_rate_nanos,status,
                 input_cost_nanos,output_cost_nanos,cache_read_cost_nanos,cache_write_cost_nanos,
                 total_cost_nanos,calculated_at_ms)
                 VALUES('req-v6','manual',1500,'CNY','flat',1000000,2000000,0,0,'priced',
                 100000,40000,0,0,140000,2000)",
                [],
            )
            .unwrap();
        }

        let mut conn = Connection::open(&path).unwrap();
        migrate(&mut conn, folder.path()).unwrap();
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(
            version, SCHEMA_VERSION,
            "v6 must upgrade to the current schema"
        );
        let cost: (String, String, i64) = conn
            .query_row(
                "SELECT request_id, currency, total_cost_nanos FROM usage_costs",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(cost, ("req-v6".into(), "CNY".into(), 140_000));
        let targets: Vec<String> = {
            let mut statement = conn
                .prepare("PRAGMA foreign_key_list(usage_costs)")
                .unwrap();
            statement
                .query_map([], |row| row.get::<_, String>(2))
                .unwrap()
                .map(|row| row.unwrap())
                .collect()
        };
        assert_eq!(
            targets,
            vec!["usage_requests".to_string()],
            "the cost FK follows the rebuilt table"
        );
        let foreign_keys: i64 = conn
            .query_row("PRAGMA foreign_keys", [], |row| row.get(0))
            .unwrap();
        assert_eq!(
            foreign_keys, 1,
            "foreign keys must be restored after the rebuild"
        );
        let dangling: i64 = {
            let mut statement = conn.prepare("PRAGMA foreign_key_check").unwrap();
            let mut rows = statement.query([]).unwrap();
            i64::from(rows.next().unwrap().is_some())
        };
        assert_eq!(
            dangling, 0,
            "the rebuilt schema must have no dangling references"
        );

        // Idempotent: a second start on v7 is a no-op and keeps the rows.
        migrate(&mut conn, folder.path()).unwrap();
        let again: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(again, SCHEMA_VERSION);
        let costs: i64 = conn
            .query_row("SELECT COUNT(*) FROM usage_costs", [], |row| row.get(0))
            .unwrap();
        assert_eq!(costs, 1);

        // A refused step rolls back completely: no version bump, no partial table.
        let failed = apply_step(&mut conn, 99, "CREATE TABLE broken (;");
        assert!(failed.is_err(), "an invalid step must fail");
        let still: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(
            still, SCHEMA_VERSION,
            "a failed step must never advance user_version"
        );
        let broken: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE name='broken'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(broken, 0, "a failed step must not leave a partial table");
    }

    /// S02.03 / AC011: after a real v7 migration the mutual-exclusion CHECK must
    /// reject a proxy row that lost its transport shape and an imported row that
    /// carries HTTP, timing or transport evidence. An imported row may carry
    /// credit, and it may never receive a cost row.
    #[test]
    fn proxy_and_import_rows_cannot_mix_transport_or_cost_columns() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("usage.sqlite3");
        let mut conn = open_connection(&path, false).unwrap();
        migrate(&mut conn, folder.path()).unwrap();
        conn.execute(
            "INSERT INTO stats_runs(run_id, started_at_ms) VALUES('run-x', 1)",
            [],
        )
        .unwrap();

        // Shared columns are fixed; each case supplies the varying ones.
        let insert = |id: &str, columns: &str, values: &str| -> String {
            let sql = format!(
                "INSERT INTO usage_requests(request_id,provider_id,route_id,provider_name,route_name,\
                 requested_model,upstream_model,started_at_ms,created_at_ms,updated_at_ms,{columns}) \
                 VALUES('{id}','p','r','P','R','m','m',1,1,2,{values})"
            );
            match conn.execute(&sql, []) {
                Ok(_) => String::new(),
                Err(failure) => failure.to_string(),
            }
        };
        let proxy_columns = "source,run_id,ended_at_ms,stream,outcome,usage_status,usage_source";
        let import_columns = "source,ended_at_ms,outcome,usage_status,usage_source,record_origin,time_basis,upstream_protocol";

        // Baselines that must be accepted.
        assert_eq!(
            insert(
                "ok-proxy",
                proxy_columns,
                "'relay','run-x',2,1,'completed','missing','none'"
            ),
            ""
        );
        assert_eq!(
            insert(
                "ok-import",
                import_columns,
                "'workbuddy_import',1,'recorded','missing','workbuddy_jsonl','workbuddy_builtin','message_timestamp',NULL"
            ),
            ""
        );
        assert_eq!(
            insert(
                "ok-credit",
                &format!("{import_columns},workbuddy_credit_nanos"),
                "'workbuddy_import',1,'recorded','missing','workbuddy_jsonl','workbuddy_builtin','message_timestamp',NULL,1500000000"
            ),
            "",
            "an imported row may carry an exact credit"
        );

        // Proxy rows must keep run/stream/protocol and must not claim import-only states.
        for (label, columns, values) in [
            (
                "proxy without run_id",
                proxy_columns.to_string(),
                "'relay',NULL,2,1,'completed','missing','none'".to_string(),
            ),
            (
                "proxy without stream",
                proxy_columns.to_string(),
                "'relay','run-x',2,NULL,'completed','missing','none'".to_string(),
            ),
            (
                "proxy marked recorded",
                proxy_columns.to_string(),
                "'relay','run-x',2,1,'recorded','missing','none'".to_string(),
            ),
            (
                "proxy with import time basis",
                format!("{proxy_columns},time_basis"),
                "'relay','run-x',2,1,'completed','missing','none','message_timestamp'".to_string(),
            ),
            (
                "proxy with workbuddy credit",
                format!("{proxy_columns},workbuddy_credit_nanos"),
                "'relay','run-x',2,1,'completed','missing','none',1".to_string(),
            ),
            (
                "proxy with NULL protocol",
                format!("{proxy_columns},upstream_protocol"),
                "'relay','run-x',2,1,'completed','missing','none',NULL".to_string(),
            ),
        ] {
            let failure = insert("bad-proxy", &columns, &values);
            assert!(
                failure.contains("CHECK constraint failed"),
                "{label} must be refused, got: {failure}"
            );
        }

        // Imported rows must stay free of HTTP, timing and transport evidence.
        for (label, columns, values) in [
            (
                "import with http_status",
                format!("{import_columns},http_status"),
                "'workbuddy_import',1,'recorded','missing','workbuddy_jsonl','workbuddy_builtin','message_timestamp',NULL,200".to_string(),
            ),
            (
                "import with duration",
                format!("{import_columns},duration_ms"),
                "'workbuddy_import',1,'recorded','missing','workbuddy_jsonl','workbuddy_builtin','message_timestamp',NULL,0".to_string(),
            ),
            (
                "import with ttfb",
                format!("{import_columns},ttfb_ms"),
                "'workbuddy_import',1,'recorded','missing','workbuddy_jsonl','workbuddy_builtin','message_timestamp',NULL,1".to_string(),
            ),
            (
                "import with error_kind",
                format!("{import_columns},error_kind"),
                "'workbuddy_import',1,'recorded','missing','workbuddy_jsonl','workbuddy_builtin','message_timestamp',NULL,'connect'".to_string(),
            ),
            (
                "import with run_id",
                format!("{import_columns},run_id"),
                "'workbuddy_import',1,'recorded','missing','workbuddy_jsonl','workbuddy_builtin','message_timestamp',NULL,'run-x'".to_string(),
            ),
            (
                "import with stream",
                format!("{import_columns},stream"),
                "'workbuddy_import',1,'recorded','missing','workbuddy_jsonl','workbuddy_builtin','message_timestamp',NULL,1".to_string(),
            ),
            (
                "import completed",
                import_columns.to_string(),
                "'workbuddy_import',1,'completed','missing','workbuddy_jsonl','workbuddy_builtin','message_timestamp',NULL".to_string(),
            ),
            (
                "import claiming upstream usage",
                import_columns.to_string(),
                "'workbuddy_import',1,'recorded','missing','upstream','workbuddy_builtin','message_timestamp',NULL".to_string(),
            ),
            (
                "import claiming proxy origin",
                import_columns.to_string(),
                "'workbuddy_import',1,'recorded','missing','workbuddy_jsonl','proxy','message_timestamp',NULL".to_string(),
            ),
            (
                "import relying on the default protocol",
                "source,ended_at_ms,outcome,usage_status,usage_source,record_origin,time_basis".to_string(),
                "'workbuddy_import',1,'recorded','missing','workbuddy_jsonl','workbuddy_builtin','message_timestamp'".to_string(),
            ),
        ] {
            let failure = insert("bad-import", &columns, &values);
            assert!(
                failure.contains("CHECK constraint failed"),
                "{label} must be refused, got: {failure}"
            );
        }

        // The cost trigger keeps imported history out of the currency totals.
        let failure = conn
            .execute(
                "INSERT INTO usage_costs(request_id,price_source,price_snapshot_at_ms,currency,input_mode,
                 input_rate_nanos,output_rate_nanos,cache_read_rate_nanos,cache_write_rate_nanos,status,
                 input_cost_nanos,output_cost_nanos,cache_read_cost_nanos,cache_write_cost_nanos,
                 total_cost_nanos,calculated_at_ms)
                 VALUES('ok-import','manual',1,'CNY','flat',1,1,1,1,'priced',1,1,1,1,4,1)",
                [],
            )
            .expect_err("an imported row must never receive a cost row");
        assert!(failure.to_string().contains("imported cost is unknown"));
        let costs: i64 = conn
            .query_row("SELECT COUNT(*) FROM usage_costs", [], |row| row.get(0))
            .unwrap();
        assert_eq!(costs, 0);
    }

    /// S02.07 / AC015: a v6 database with every protocol, every terminal state,
    /// both currencies and a full price snapshot must survive the v7 rebuild with
    /// rows, ids, tokens, snapshots and amounts unchanged, and its indexes and
    /// view must still answer.
    #[test]
    fn a_v6_database_upgrades_without_changing_rows_ids_tokens_or_costs() {
        const LEGACY_COLUMNS: &str = "id, request_id, run_id, source, provider_id, route_id,
            provider_name, route_name, requested_model, upstream_model, response_model,
            started_at_ms, ended_at_ms, duration_ms, ttfb_ms, first_content_ms, stream,
            upstream_started, http_status, outcome, error_kind, usage_status, usage_reason,
            usage_source, parser_version, input_tokens, output_tokens, total_tokens,
            cache_read_tokens, cache_write_tokens, reasoning_tokens, input_uncached_tokens,
            input_breakdown, created_at_ms, updated_at_ms, upstream_protocol";

        fn dump(conn: &Connection, sql: &str) -> Vec<Vec<rusqlite::types::Value>> {
            let mut statement = conn.prepare(sql).unwrap();
            let columns = statement.column_count();
            let rows = statement
                .query_map([], |row| {
                    let mut values = Vec::with_capacity(columns);
                    for index in 0..columns {
                        values.push(row.get::<_, rusqlite::types::Value>(index)?);
                    }
                    Ok(values)
                })
                .unwrap();
            rows.map(|row| row.unwrap()).collect()
        }

        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("usage.sqlite3");
        {
            let mut conn = Connection::open(&path).unwrap();
            conn.execute_batch(MIGRATION_0001).unwrap();
            conn.pragma_update(None, "user_version", 1).unwrap();
            conn.pragma_update(None, "foreign_keys", "OFF").unwrap();
            let tx = conn
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .unwrap();
            tx.execute_batch(MIGRATION_0002).unwrap();
            tx.pragma_update(None, "user_version", 2).unwrap();
            tx.commit().unwrap();
            conn.pragma_update(None, "foreign_keys", "ON").unwrap();
            apply_step(&mut conn, 3, MIGRATION_0003).unwrap();
            apply_step(&mut conn, 4, MIGRATION_0004).unwrap();
            apply_step(&mut conn, 5, MIGRATION_0005).unwrap();
            apply_step(&mut conn, 6, MIGRATION_0006).unwrap();

            conn.execute(
                "INSERT INTO stats_runs(run_id, started_at_ms, stopped_at_ms, stop_reason)
                 VALUES('run-ok', 1000, 9000, 'normal')",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO stats_runs(run_id, started_at_ms) VALUES('run-crash', 1000)",
                [],
            )
            .unwrap();

            // One row per protocol, plus a failed, an in-flight and a probe row.
            let protocols = [
                "openai_chat",
                "openai_responses",
                "anthropic_messages",
                "gemini_generate_content",
            ];
            for (index, protocol) in protocols.iter().enumerate() {
                let request = format!("req-{index}");
                conn.execute(
                    "INSERT INTO usage_requests(request_id,run_id,source,provider_id,route_id,
                     provider_name,route_name,requested_model,upstream_model,response_model,
                     started_at_ms,ended_at_ms,duration_ms,ttfb_ms,first_content_ms,stream,
                     upstream_started,http_status,outcome,usage_status,usage_source,parser_version,
                     input_tokens,output_tokens,total_tokens,cache_read_tokens,cache_write_tokens,
                     reasoning_tokens,input_uncached_tokens,input_breakdown,created_at_ms,
                     updated_at_ms,upstream_protocol)
                     VALUES(?1,'run-ok','relay','p','r','P','R','m','m','m',1000,2000,1000,100,150,1,1,
                            200,'completed','complete','upstream',1,100,20,120,10,5,3,85,'disjoint',
                            1000,2000,?2)",
                    rusqlite::params![request, protocol],
                )
                .unwrap();
            }
            conn.execute(
                "INSERT INTO usage_requests(request_id,run_id,source,provider_id,route_id,
                 provider_name,route_name,requested_model,upstream_model,started_at_ms,ended_at_ms,
                 duration_ms,stream,upstream_started,http_status,outcome,error_kind,usage_status,
                 usage_source,created_at_ms,updated_at_ms,upstream_protocol)
                 VALUES('req-failed','run-ok','relay','p','r','P','R','m','m',1000,1500,500,1,1,502,
                        'failed','upstream_http','missing','none',1000,1500,'openai_chat')",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO usage_requests(request_id,run_id,source,provider_id,route_id,
                 provider_name,route_name,requested_model,upstream_model,started_at_ms,stream,
                 upstream_started,outcome,usage_status,usage_source,created_at_ms,updated_at_ms,
                 upstream_protocol)
                 VALUES('req-inflight','run-crash','relay','p','r','P','R','m','m',1000,1,1,
                        'in_flight','missing','none',1000,1000,'openai_chat')",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO usage_requests(request_id,run_id,source,provider_id,route_id,
                 provider_name,route_name,requested_model,upstream_model,started_at_ms,ended_at_ms,
                 duration_ms,stream,upstream_started,outcome,usage_status,usage_source,created_at_ms,
                 updated_at_ms,upstream_protocol)
                 VALUES('req-probe','run-ok','connection_test','p','r','P','R','m','m',1000,1000,0,0,0,
                        'completed','not_applicable','none',1000,1000,'openai_chat')",
                [],
            )
            .unwrap();

            let costs = [
                (
                    "req-0",
                    "manual",
                    "priced",
                    "USD",
                    2_000_000_000i64,
                    8_000_000_000i64,
                ),
                (
                    "req-1",
                    "manual",
                    "priced",
                    "CNY",
                    1_000_000_000,
                    4_000_000_000,
                ),
                ("req-2", "default_zero", "default_zero", "CNY", 0, 0),
                (
                    "req-3",
                    "manual",
                    "usage_incomplete",
                    "USD",
                    3_000_000_000,
                    6_000_000_000,
                ),
                (
                    "req-probe",
                    "manual",
                    "not_applicable",
                    "CNY",
                    5_000_000_000,
                    7_000_000_000,
                ),
            ];
            for (request, source, status, currency, input_rate, output_rate) in costs {
                let (input_cost, output_cost, total) = if status == "priced" {
                    (100_000i64, 40_000i64, 140_000i64)
                } else {
                    (0, 0, 0)
                };
                let (cost_input, cost_output, cost_total) = match status {
                    "priced" => (Some(input_cost), Some(output_cost), Some(total)),
                    "default_zero" | "not_applicable" => (Some(0), Some(0), Some(0)),
                    _ => (None, None, None),
                };
                conn.execute(
                    "INSERT INTO usage_costs(request_id,source_price_id,source_price_revision,
                     price_source,price_snapshot_at_ms,currency,input_mode,input_rate_nanos,
                     output_rate_nanos,cache_read_rate_nanos,cache_write_rate_nanos,status,
                     input_cost_nanos,output_cost_nanos,cache_read_cost_nanos,cache_write_cost_nanos,
                     total_cost_nanos,pricing_version,calculated_at_ms)
                     VALUES(?1,?2,?3,?4,1500,?5,'flat',?6,?7,0,0,?8,?9,?10,?11,?12,?13,2,2000)",
                    rusqlite::params![
                        request,
                        if source == "manual" { Some("price-1") } else { None },
                        if source == "manual" { Some(1i64) } else { None },
                        source,
                        currency,
                        input_rate,
                        output_rate,
                        status,
                        cost_input,
                        cost_output,
                        if cost_input.is_some() { Some(0i64) } else { None },
                        if cost_input.is_some() { Some(0i64) } else { None },
                        cost_total,
                    ],
                )
                .unwrap();
            }
        }

        let before_requests = {
            let conn = Connection::open(&path).unwrap();
            dump(
                &conn,
                &format!("SELECT {LEGACY_COLUMNS} FROM usage_requests ORDER BY id"),
            )
        };
        let before_costs = {
            let conn = Connection::open(&path).unwrap();
            dump(&conn, "SELECT * FROM usage_costs ORDER BY request_id")
        };
        assert_eq!(before_requests.len(), 7);
        assert_eq!(before_costs.len(), 5);

        let mut conn = Connection::open(&path).unwrap();
        migrate(&mut conn, folder.path()).unwrap();
        assert_eq!(
            conn.query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            SCHEMA_VERSION
        );

        let after_requests = dump(
            &conn,
            &format!("SELECT {LEGACY_COLUMNS} FROM usage_requests ORDER BY id"),
        );
        let after_costs = dump(&conn, "SELECT * FROM usage_costs ORDER BY request_id");
        assert_eq!(
            before_requests.len(),
            after_requests.len(),
            "row count changed"
        );
        assert_eq!(before_costs, after_costs, "cost rows changed");
        for (index, (before, after)) in before_requests.iter().zip(&after_requests).enumerate() {
            assert_eq!(before, after, "request row {index} changed field by field");
        }

        // Ids are the same values, in the same order.
        let ids: Vec<i64> = conn
            .prepare("SELECT id FROM usage_requests ORDER BY id")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .map(|row| row.unwrap())
            .collect();
        assert_eq!(ids, (1..=7).collect::<Vec<i64>>());

        // The new columns carry the documented defaults for legacy rows.
        let legacy_defaults: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM usage_requests WHERE record_origin='proxy'
                   AND time_basis='request_start' AND total_tokens_derived=0
                   AND workbuddy_credit_nanos IS NULL",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(legacy_defaults, 7);

        // The view still answers and keeps the cost columns.
        let facts: i64 = conn
            .query_row("SELECT COUNT(*) FROM stats_request_facts", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(facts, 7);
        let priced: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM stats_request_facts WHERE cost_status='priced'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(priced, 2);

        // The rebuilt indexes are usable: the time index serves the ordered scan.
        let plan: String = conn
            .query_row(
                "EXPLAIN QUERY PLAN SELECT id FROM usage_requests
                 WHERE started_at_ms >= 0 ORDER BY started_at_ms DESC, id DESC LIMIT 10",
                [],
                |row| row.get(3),
            )
            .unwrap();
        assert!(
            plan.contains("usage_requests_time"),
            "time index unused: {plan}"
        );
        let plan: String = conn
            .query_row(
                "EXPLAIN QUERY PLAN SELECT id FROM usage_requests WHERE provider_id='p'",
                [],
                |row| row.get(3),
            )
            .unwrap();
        assert!(
            plan.contains("usage_requests_provider_time"),
            "provider index unused: {plan}"
        );
    }

    #[test]
    fn v3_protocol_migration_preserves_historical_unit_prices_and_total() {
        let folder = tempfile::tempdir().unwrap();
        let mut conn = open_connection(&folder.path().join("usage.sqlite3"), false).unwrap();
        conn.execute_batch(MIGRATION_0001).unwrap();
        conn.pragma_update(None, "user_version", 1).unwrap();
        conn.pragma_update(None, "foreign_keys", "OFF").unwrap();
        apply_step(&mut conn, 2, MIGRATION_0002).unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        apply_step(&mut conn, 3, MIGRATION_0003).unwrap();
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        start_run(&tx, "run-v3", 1000, true).unwrap();
        tx.commit().unwrap();
        conn.execute_batch("INSERT INTO usage_requests(request_id,run_id,source,provider_id,provider_name,route_id,route_name,requested_model,upstream_model,started_at_ms,ended_at_ms,duration_ms,stream,upstream_started,outcome,usage_status,usage_source,input_tokens,output_tokens,total_tokens,created_at_ms,updated_at_ms)
            VALUES('historical-v3','run-v3','relay','provider','Provider','route','Route','alias','actual-model',1000,1001,1,0,1,'completed','complete','upstream',10,5,15,1000,1001);
            INSERT INTO usage_costs(request_id,source_price_id,source_price_revision,price_source,price_snapshot_at_ms,currency,input_mode,input_rate_nanos,output_rate_nanos,cache_read_rate_nanos,cache_write_rate_nanos,status,input_cost_nanos,output_cost_nanos,cache_read_cost_nanos,cache_write_cost_nanos,total_cost_nanos,pricing_version,calculated_at_ms)
            VALUES('historical-v3','deleted-old-price',3,'manual',1000,'USD','flat',2000000000,8000000000,0,0,'priced',20000,40000,0,0,60000,2,1001);").unwrap();
        migrate(&mut conn, folder.path()).unwrap();
        let values:(String,String,i64,i64,i64)=conn.query_row("SELECT r.upstream_protocol,c.currency,c.input_rate_nanos,c.output_rate_nanos,c.total_cost_nanos FROM usage_requests r JOIN usage_costs c USING(request_id)",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).unwrap();
        assert_eq!(
            values,
            (
                "openai_chat".into(),
                "USD".into(),
                2_000_000_000,
                8_000_000_000,
                60_000
            )
        );
        let detail = crate::stats::query::request_detail(&conn, "historical-v3").unwrap();
        assert_eq!(detail.row.upstream_protocol.as_deref(), Some("openai_chat"));
        assert_eq!(detail.parser_version, 1);
        assert_eq!(detail.created_at_ms, 1000);
        assert_eq!(detail.updated_at_ms, 1001);
    }

    /// U05: a tiny budget must bound the whole attempt, including the SQLite
    /// lock wait. Leaving `busy_timeout` at its default let a 1 ms budget take
    /// hundreds of milliseconds.
    #[test]
    fn a_one_millisecond_budget_is_not_multiplied_by_the_lock_wait() {
        let folder = tempfile::tempdir().unwrap();
        let counters = Arc::new(Counters::default());
        let (mut writer, _recovery) = Writer::open(folder.path(), "run-budget", counters).unwrap();
        let blocker = Connection::open(folder.path().join("usage.sqlite3")).unwrap();
        blocker.busy_timeout(Duration::from_millis(50)).unwrap();
        blocker.execute_batch("BEGIN IMMEDIATE").unwrap();
        let mut batch = Vec::new();
        for index in 0..8 {
            batch.push(StatsEvent::Start(Box::new(meta(
                &format!("budget-{index}"),
                &writer.run_id,
            ))));
        }
        let started = Instant::now();
        let outcome = writer.apply_batch(&batch, &WriteBudget::new(Duration::from_millis(1)));
        let elapsed = started.elapsed();
        assert!(outcome.is_err(), "a held lock cannot succeed");
        assert!(
            elapsed < Duration::from_millis(150),
            "a 1 ms budget took {elapsed:?}"
        );
        blocker.execute_batch("ROLLBACK").unwrap();
    }

    /// U05: ids that cannot be settled now must survive in the durable backlog
    /// and be retried by the same run, in bounded batches.
    #[test]
    fn the_recovery_backlog_is_durable_bounded_and_idempotent() {
        let folder = tempfile::tempdir().unwrap();
        let counters = Arc::new(Counters::default());
        let (mut writer, _recovery) = Writer::open(folder.path(), "run-backlog", counters).unwrap();
        let run_id = writer.run_id.clone();
        let now = crate::stats::now_ms();
        let mut batch = Vec::new();
        for index in 0..600 {
            batch.push(StatsEvent::Start(Box::new(meta(
                &format!("backlog-{index}"),
                &run_id,
            ))));
        }
        for chunk in batch.chunks(64) {
            writer
                .apply_batch(chunk, &WriteBudget::new(BATCH_BUDGET))
                .expect("fixture rows must land");
        }
        // The terminal timestamp must not precede any request start.
        let settle_at = crate::stats::now_ms() + 10_000;
        let ids: Vec<String> = (0..600).map(|index| format!("backlog-{index}")).collect();
        assert_eq!(push_backlog(&mut writer, &ids, now).unwrap(), 600);
        // Re-queueing is harmless.
        assert_eq!(push_backlog(&mut writer, &ids, now).unwrap(), 0);
        assert_eq!(backlog_len(&writer.conn).unwrap(), 600);
        // Bounded batches: one call repairs at most the given limit.
        let (done, missing, remaining) = repair_backlog_batch(&mut writer, 256, settle_at).unwrap();
        assert_eq!((done, missing), (256, 0));
        assert_eq!(remaining, 344);
        let open: i64 = writer
            .conn
            .query_row(
                "SELECT COUNT(*) FROM usage_requests WHERE outcome='in_flight'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(open, 344, "every drained request must be terminal");
        let (done, _, remaining) = repair_backlog_batch(&mut writer, 256, settle_at).unwrap();
        assert_eq!((done, remaining), (256, 88));
        let (done, _, remaining) = repair_backlog_batch(&mut writer, 256, settle_at).unwrap();
        assert_eq!((done, remaining), (88, 0));
        assert_eq!(backlog_len(&writer.conn).unwrap(), 0);
        // S03: an id with no request row (both `Start` and `Finish` were lost)
        // leaves the queue without inventing a request to fill the hole.
        let _ = now;
        assert_eq!(
            push_backlog(&mut writer, &["never-existed".to_owned()], now).unwrap(),
            1
        );
        let before: i64 = writer
            .conn
            .query_row("SELECT COUNT(*) FROM usage_requests", [], |row| row.get(0))
            .unwrap();
        let (done, missing, remaining) = repair_backlog_batch(&mut writer, 256, settle_at).unwrap();
        assert_eq!((done, missing, remaining), (0, 1, 0));
        let after: i64 = writer
            .conn
            .query_row("SELECT COUNT(*) FROM usage_requests", [], |row| row.get(0))
            .unwrap();
        assert_eq!(before, after, "a missing id must not fabricate a request");
        // An empty backlog is a no-op, and a repeat is idempotent.
        assert_eq!(
            repair_backlog_batch(&mut writer, 256, settle_at).unwrap(),
            (0, 0, 0)
        );
        let open: i64 = writer
            .conn
            .query_row(
                "SELECT COUNT(*) FROM usage_requests WHERE outcome='in_flight'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(open, 0);
    }

    /// T01: a sweep must delete exactly the eligible rows, across as many
    /// batches as it takes, and never a row the preview did not count.
    #[test]
    fn a_sweep_is_bounded_but_completes_and_respects_its_cutoff() {
        let folder = tempfile::tempdir().unwrap();
        let counters = Arc::new(Counters::default());
        let (mut writer, _recovery) = Writer::open(folder.path(), "run-sweep", counters).unwrap();
        let run_id = writer.run_id.clone();
        // Every fixture row is in the past; only the cutoff separates the two
        // groups.
        let base = crate::stats::now_ms() - 10_000_000;
        // 1200 finished rows before the cutoff, 40 after it, plus 5 in flight.
        let mut batch = Vec::new();
        for index in 0..1240i64 {
            let mut meta = meta(&format!("sweep-{index}"), &run_id);
            meta.started_at_ms = if index < 1200 {
                base + index
            } else {
                base + 6_000_000 + index
            };
            let mut finish = finish(UsageStatus::Complete, (Some(4), Some(1), Some(5)));
            finish.ended_at_ms = meta.started_at_ms + 10;
            finish.updated_at_ms = meta.started_at_ms + 10;
            batch.push(StatsEvent::Start(Box::new(meta)));
            batch.push(StatsEvent::Finish {
                request_id: format!("sweep-{index}"),
                finish: Box::new(finish),
            });
        }
        for index in 0..5i64 {
            let mut meta = meta(&format!("live-{index}"), &run_id);
            meta.started_at_ms = base + index;
            batch.push(StatsEvent::Start(Box::new(meta)));
        }
        for chunk in batch.chunks(64) {
            writer
                .apply_batch(chunk, &WriteBudget::new(BATCH_BUDGET))
                .expect("fixture rows must land");
        }
        let count = |writer: &Writer, clause: &str| -> i64 {
            writer
                .conn
                .query_row(
                    &format!("SELECT COUNT(*) FROM usage_requests WHERE {clause}"),
                    [],
                    |row| row.get(0),
                )
                .unwrap()
        };
        assert_eq!(count(&writer, "outcome<>'in_flight'"), 1240);
        assert_eq!(count(&writer, "outcome='in_flight'"), 5);

        let cutoff = base + 5_000_000;
        let mut task =
            Maintenance::new(1, MaintenanceKind::Clear, cutoff, i64::MAX, None, base, 500);
        let mut rounds = 0;
        loop {
            let (deleted, more) = writer.delete_finished(&mut task).unwrap();
            assert!(deleted <= 500, "a batch must stay bounded: {deleted}");
            rounds += 1;
            if !more {
                break;
            }
            assert!(rounds < 20, "the sweep must converge");
        }
        // 1200 eligible rows in three bounded batches, and nothing else.
        assert_eq!(task.affected, 1200, "affected_rows must match reality");
        assert_eq!(rounds, 3, "three batches of at most 500");
        assert_eq!(count(&writer, "outcome<>'in_flight'"), 40);
        assert_eq!(count(&writer, "outcome='in_flight'"), 5);
        assert!(
            count(&writer, "ended_at_ms IS NOT NULL AND started_at_ms>=0") >= 40,
            "rows after the cutoff must survive"
        );
        // Cost rows cascade with their request.
        let orphan_costs: i64 = writer
            .conn
            .query_row(
                "SELECT COUNT(*) FROM usage_costs c
                 LEFT JOIN usage_requests r ON r.request_id=c.request_id
                 WHERE r.request_id IS NULL",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(orphan_costs, 0);
        // A second sweep with the same cutoff has nothing left to do.
        let mut again =
            Maintenance::new(2, MaintenanceKind::Clear, cutoff, i64::MAX, None, base, 500);
        assert_eq!(writer.delete_finished(&mut again).unwrap(), (0, false));
    }

    /// R05: a held write lock must fail one batch inside its budget instead of
    /// multiplying the lock wait by the batch size.
    #[test]
    fn a_held_lock_fails_one_batch_within_the_budget() {
        let folder = tempfile::tempdir().unwrap();
        let counters = Arc::new(Counters::default());
        let (mut writer, _recovery) = Writer::open(folder.path(), "run-lock", counters).unwrap();
        let run_id = writer.run_id.clone();
        let batch: Vec<StatsEvent> = (0..64)
            .map(|index| StatsEvent::Start(Box::new(meta(&format!("req-{index}"), &run_id))))
            .collect();
        let blocker = rusqlite::Connection::open(folder.path().join("usage.sqlite3")).unwrap();
        blocker.busy_timeout(Duration::from_millis(50)).unwrap();
        blocker.execute_batch("BEGIN IMMEDIATE").unwrap();
        let started = Instant::now();
        let budget = WriteBudget::new(BATCH_BUDGET);
        let failure = writer
            .apply_batch(&batch, &budget)
            .expect_err("a held lock must be reported");
        let elapsed = started.elapsed();
        assert!(
            is_transient(&failure),
            "a lock must stay transient, got {failure:?}"
        );
        assert!(
            elapsed < BATCH_BUDGET + Duration::from_millis(750),
            "one batch took {elapsed:?}, which is outside its budget"
        );
        blocker.execute_batch("ROLLBACK").unwrap();
        // The batch is retryable once the lock is gone.
        let outcome = writer
            .apply_batch(&batch, &WriteBudget::new(BATCH_BUDGET))
            .expect("the retry must succeed");
        assert!(outcome.is_complete(), "{outcome:?}");
    }

    /// R05: overflowed orphan ids are settled rather than silently dropped.
    #[test]
    fn orphan_settlement_covers_more_than_the_backlog_limit() {
        let folder = tempfile::tempdir().unwrap();
        let counters = Arc::new(Counters::default());
        let (mut writer, _recovery) = Writer::open(folder.path(), "run-orphan", counters).unwrap();
        let run_id = writer.run_id.clone();
        let ids: Vec<String> = (0..crate::stats::MAX_ORPHANS + 120)
            .map(|index| format!("orphan-{index}"))
            .collect();
        let batch: Vec<StatsEvent> = ids
            .iter()
            .map(|id| StatsEvent::Start(Box::new(meta(id, &run_id))))
            .collect();
        writer
            .apply_batch(&batch, &WriteBudget::new(BATCH_BUDGET))
            .expect("starts must land");
        let settled = writer.settle_orphans(&ids, crate::stats::now_ms()).unwrap();
        assert_eq!(
            settled,
            ids.len(),
            "every orphan must reach a terminal state"
        );
        let remaining: i64 = writer
            .conn
            .query_row(
                "SELECT COUNT(*) FROM usage_requests WHERE outcome='in_flight'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(remaining, 0);
    }

    /// A fast request can put `Start` and `Finish` in the same batch; the
    /// schema requires `updated_at_ms >= created_at_ms`, so `created_at_ms` must
    /// be the request's own start time rather than the batch time.
    #[test]
    fn start_and_finish_in_one_batch_keep_the_timestamp_order() {
        let folder = tempfile::tempdir().unwrap();
        let counters = Arc::new(Counters::default());
        let (mut writer, _recovery) = Writer::open(folder.path(), "run-test", counters).unwrap();
        let run_id = writer.run_id.clone();
        let mut meta = meta("req-fast", &run_id);
        // The request started before the batch is written.
        meta.started_at_ms = crate::stats::now_ms() - 5;
        let mut finish = finish(UsageStatus::Complete, (Some(4), Some(1), Some(5)));
        finish.ended_at_ms = meta.started_at_ms + 1;
        finish.updated_at_ms = meta.started_at_ms + 1;
        let batch = vec![
            StatsEvent::Start(Box::new(meta)),
            StatsEvent::Finish {
                request_id: "req-fast".into(),
                finish: Box::new(finish),
            },
        ];
        let outcome = writer
            .apply_batch(&batch, &WriteBudget::new(BATCH_BUDGET))
            .expect("batch must succeed");
        assert!(outcome.is_complete(), "{outcome:?}");
        let row: (String, i64, i64, Option<i64>) = writer
            .conn
            .query_row(
                "SELECT outcome, created_at_ms, updated_at_ms, total_tokens FROM usage_requests
                 WHERE request_id='req-fast'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(row.0, "completed");
        assert!(
            row.2 >= row.1,
            "updated_at_ms must not precede created_at_ms"
        );
        assert_eq!(row.3, Some(5));
    }

    /// F01: a request whose usage was rejected must not roll back the batch that
    /// also carries a healthy request.
    #[test]
    fn an_invalid_usage_never_rolls_back_the_whole_batch() {
        let folder = tempfile::tempdir().unwrap();
        let counters = Arc::new(Counters::default());
        let (mut writer, _recovery) = Writer::open(folder.path(), "run-test", counters).unwrap();
        let run_id = writer.run_id.clone();
        let batch = vec![
            StatsEvent::Start(Box::new(meta("req-invalid", &run_id))),
            StatsEvent::Start(Box::new(meta("req-healthy", &run_id))),
            StatsEvent::Finish {
                request_id: "req-invalid".into(),
                finish: Box::new(finish(UsageStatus::Invalid, (None, None, None))),
            },
            StatsEvent::Finish {
                request_id: "req-healthy".into(),
                finish: Box::new(finish(
                    UsageStatus::Complete,
                    (Some(1000), Some(100), Some(1100)),
                )),
            },
        ];
        writer
            .apply_batch(&batch, &WriteBudget::new(BATCH_BUDGET))
            .expect("batch must succeed");
        let total: i64 = writer
            .conn
            .query_row(
                "SELECT COUNT(*) FROM usage_requests WHERE outcome<>'in_flight'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(total, 2, "both terminal rows must be persisted");
        let invalid: (String, Option<i64>) = writer
            .conn
            .query_row(
                "SELECT usage_status, total_tokens FROM usage_requests WHERE request_id='req-invalid'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(invalid.0, "invalid");
        assert_eq!(invalid.1, None, "an invalid usage must not store numbers");
        let healthy: Option<i64> = writer
            .conn
            .query_row(
                "SELECT total_tokens FROM usage_requests WHERE request_id='req-healthy'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(healthy, Some(1100));
    }
    #[test]
    fn v8_migration_preserves_v7_facts_ledger_and_rolls_back_failed_rebuild() {
        let folder = tempfile::tempdir().unwrap();
        let mut conn = Connection::open(folder.path().join("usage.sqlite3")).unwrap();
        conn.pragma_update(None, "foreign_keys", "OFF").unwrap();
        for (version, sql) in [
            (1, MIGRATION_0001),
            (2, MIGRATION_0002),
            (3, MIGRATION_0003),
            (4, MIGRATION_0004),
            (5, MIGRATION_0005),
            (6, MIGRATION_0006),
            (7, MIGRATION_0007),
        ] {
            apply_step(&mut conn, version, sql).unwrap();
        }
        conn.execute("INSERT INTO usage_requests(request_id,source,provider_id,route_id,provider_name,route_name,requested_model,upstream_model,started_at_ms,ended_at_ms,outcome,usage_status,usage_source,created_at_ms,updated_at_ms,upstream_protocol,record_origin,time_basis,input_tokens,output_tokens,total_tokens,workbuddy_credit_nanos) VALUES(?1,'workbuddy_import','wb:builtin','r','WorkBuddy','m','m','m',100,100,'recorded','complete','workbuddy_jsonl',100,100,NULL,'workbuddy_builtin','message_timestamp',10,2,12,1140000000)",rusqlite::params!["a".repeat(64)]).unwrap();
        conn.execute("INSERT INTO workbuddy_import_ledger(entry_key,namespace_id,session_hash,message_hash,requested_model,event_at_ms,normalized_hash,state,request_id,mapping_revision,created_at_ms,updated_at_ms) VALUES(?1,?2,?3,?4,'m',100,?5,'imported',?1,1,100,100)",rusqlite::params!["a".repeat(64),"b".repeat(48),"c".repeat(64),"d".repeat(64),"e".repeat(64)]).unwrap();
        fn rows(conn: &Connection, table: &str) -> Vec<Vec<rusqlite::types::Value>> {
            let mut stmt = conn
                .prepare(&format!("SELECT * FROM {table} ORDER BY rowid"))
                .unwrap();
            let columns = stmt.column_count();
            stmt.query_map([], |row| (0..columns).map(|i| row.get(i)).collect())
                .unwrap()
                .map(Result::unwrap)
                .collect()
        }
        let facts = rows(&conn, "usage_requests");
        let ledger = rows(&conn, "workbuddy_import_ledger");
        let bad = format!("{MIGRATION_0008} SELECT * FROM deliberate_missing_table;");
        assert!(apply_step(&mut conn, 8, &bad).is_err());
        assert_eq!(
            conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            7
        );
        assert_eq!(rows(&conn, "usage_requests"), facts);
        assert_eq!(rows(&conn, "workbuddy_import_ledger"), ledger);
        migrate(&mut conn, folder.path()).unwrap();
        assert_eq!(rows(&conn, "usage_requests"), facts);
        assert_eq!(rows(&conn, "workbuddy_import_ledger"), ledger);
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |r| r
                .get::<_, i64>(
                0
            ))
            .unwrap(),
            0
        );
        assert_eq!(
            conn.query_row("PRAGMA foreign_keys", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            1
        );
        migrate(&mut conn, folder.path()).unwrap();
        assert_eq!(rows(&conn, "usage_requests"), facts);
    }
}
