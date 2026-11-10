//! Statistics DTOs shared by the core proxy and the native settings binary.
//!
//! This module is deliberately limited to `serde`/`std` so `gpui` can include it
//! with `#[path = "../../src/stats/dto.rs"]` without linking SQLite, chrono-tz or
//! any other core-only dependency. SQL, timezone resolution and pricing live in
//! the sibling modules of `crate::stats`.
//!
//! `#[path]` 只是让 Core 与 GPUI 设置进程在各自编译时包含同一份源码，保证 DTO 字段和
//! wire 枚举一致；它不表示两个操作系统进程共享内存或数据库连接。实际数据仍经命令 JSON、
//! SQLite 行或各自的内存对象传递。对 Java 读者，这些 DTO 类似使用 Jackson 映射的接口
//! record，但这里没有 Spring 容器、依赖注入或对象托管，只有显式构造与 Serde 序列化。
use serde::{Deserialize, Serialize};

/// Fixed error codes for the import management commands.
///
/// Every code is a constant: nothing derived from a file, a path or SQL ever
/// reaches the caller, so a response cannot leak content or a query.
pub const ERR_WORKBUDDY_IMPORT_DISABLED: &str = "stats_workbuddy_import_disabled";
pub const ERR_WORKBUDDY_ROOT_MISSING: &str = "stats_workbuddy_root_missing";
pub const ERR_WORKBUDDY_PERMISSION_DENIED: &str = "stats_workbuddy_permission_denied";
pub const ERR_WORKBUDDY_INVALID_PATH: &str = "stats_workbuddy_invalid_path";
pub const ERR_WORKBUDDY_UNSUPPORTED_FORMAT: &str = "stats_workbuddy_unsupported_format";
pub const ERR_WORKBUDDY_STALE_BATCH: &str = "stats_workbuddy_stale_batch";
pub const ERR_WORKBUDDY_MAPPING_CONFLICT: &str = "stats_workbuddy_mapping_conflict";
pub const ERR_WORKBUDDY_SOURCE_CHANGED: &str = "stats_workbuddy_source_changed";
pub const ERR_WORKBUDDY_SOURCE_READ_FAILED: &str = "stats_workbuddy_source_read_failed";
pub const ERR_WORKBUDDY_LEDGER_CONFLICT: &str = "stats_workbuddy_ledger_conflict";
pub const ERR_WORKBUDDY_SCAN_BUSY: &str = "stats_workbuddy_scan_busy";

/// Inner protocol version for every statistics response.
/// Statistics API version. Bumped to 2 with the per-call price snapshot: the
/// request detail no longer returns a mutable `rule` and every place is a
/// decimal string of nano units.
/// Group totals above this are reported as a lower bound to bound counting work.
pub const MAX_COUNTED_GROUPS: i64 = 1000;

pub const API_VERSION: u32 = 4;
/// Longest accepted `[from_ms, to_ms)` range.
pub const MAX_RANGE_MS: i64 = 366 * 86_400_000;
/// Upper bound for a single statistical query, above which the core asks the
/// caller to narrow the range instead of degrading to floating point.
pub const MAX_MATCHING_ROWS: i64 = 1_000_000;
/// Largest allowed serialized statistics response.
pub const MAX_RESULT_BYTES: usize = 1024 * 1024;
/// Largest accepted single token field.
pub const MAX_TOKEN_VALUE: i64 = 1_000_000_000_000;
/// Largest accepted `total_tokens`.
pub const MAX_TOTAL_TOKENS: i64 = 2_000_000_000_000;
/// Bucket ceiling for the trend series.
pub const MAX_BUCKETS: usize = 400;
/// Default retention window in days; `0` means keep everything.
pub const DEFAULT_RETENTION_DAYS: i64 = 180;
/// Row batch size for maintenance work.
pub const MAINTENANCE_BATCH: i64 = 1000;
pub const ERR_INVALID: &str = "stats_invalid_argument";
pub const ERR_TIMEOUT: &str = "stats_query_timeout";
pub const ERR_BUSY: &str = "stats_busy";
pub const ERR_UNAVAILABLE: &str = "stats_unavailable";
pub const ERR_MAINTENANCE_FAILED: &str = "stats_maintenance_failed";
pub const ERR_CURSOR_EXPIRED: &str = "stats_cursor_expired";
/// A command that used to exist and was deliberately withdrawn: repricing
/// history with the *current* price contradicts the per-call price snapshot.
pub const ERR_OPERATION_REMOVED: &str = "stats_operation_removed";
pub const ERR_REVISION_CONFLICT: &str = "stats_revision_conflict";
pub const ERR_NOT_FOUND: &str = "stats_not_found";
pub const ERR_RESULT_TOO_LARGE: &str = "stats_result_too_large";
pub const ERR_QUERY_TOO_LARGE: &str = "stats_query_too_large";
pub const ERR_UNSUPPORTED_SCHEMA: &str = "stats_unsupported_schema";

/// 把 `i64` nano 金额格式化成界面金额字符串。
///
/// 输入单位是 `1 / 1_000_000_000` 货币单位。该便捷函数先转成十进制字符串并交给
/// [`format_amount_text`]；0 显示 `"0"`，整数省略小数，非整数至少保留 4 位、去掉多余尾零，
/// 正的极小非零值显示 `"<0.0001"`。它只格式化，不做币种换算或四舍五入到数据库金额。
#[allow(dead_code)] // Shared with the native settings UI.
pub fn format_amount(nanos: i64) -> String {
    format_amount_text(&nanos.to_string())
}

/// 格式化任意大小的十进制 nano 金额字符串。
///
/// 统计 API 用字符串传金额，因为跨行汇总可能超过 `i64`；本函数用 `i128` 解析，避免退回
/// `i64` 失败后把真实金额显示成 0/未知。接受的文本是可选负号加 ASCII 数字，不接受小数点
/// 或指数等字符；非法输入显示 `"—"`。
///
/// 展示精度固定四位起步：0 仍显示 `"0"`，正的 `1..99999` nano 显示 `"<0.0001"`，对应负值
/// 显示 `">-0.0001"`；非整数在去掉多余尾零后仍至少保留四位小数，整额才省略小数部分。
/// 这里的阈值是 UI 展示策略，不改变持久化的 nano 整数。
pub fn format_amount_text(text: &str) -> String {
    let text = text.trim();
    let (negative, digits) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return "—".into();
    }
    let Ok(value) = digits.parse::<i128>() else {
        return "—".into();
    };
    if value > 0 && value < 100_000 {
        return if negative {
            ">-0.0001".into()
        } else {
            "<0.0001".into()
        };
    }
    let whole = value / 1_000_000_000;
    let fraction = value % 1_000_000_000;
    if fraction == 0 {
        // A whole amount is shown as a whole number: "0", never "0.0000".
        return if negative {
            format!("-{whole}")
        } else {
            whole.to_string()
        };
    }
    let mut rendered = format!("{whole}.{fraction:09}");
    while rendered.ends_with('0') && rendered.len() - rendered.find('.').unwrap_or(0) > 5 {
        rendered.pop();
    }
    if negative {
        format!("-{rendered}")
    } else {
        rendered
    }
}

/// 把设置页输入的“每百万 token 的人类金额”解析成 API 的 nano 整数费率。
///
/// 例如 `"2"` 转为 `2_000_000_000`，`"0.2"` 转为 `200_000_000`。函数先去掉首尾空白，
/// 只接受普通十进制数字（可省略整数或小数一侧），最多 9 位小数；负值、指数、科学计数法、
/// 其他符号和溢出都返回稳定的英文错误文本。转换使用 `checked_mul`/`checked_add`，不经过
/// `f64`。
///
/// 设置表单只在提交时调用一次，结果直接作为 `*_rate_nanos` 请求字段；Core 随后用
/// `pricing::parse_rate` 校验它已是无符号整数字符串。货币语义由同一请求的 `currency`
/// 决定，函数本身不附加币种。
#[allow(dead_code)] // Shared with the native settings UI.
pub fn parse_human_amount(text: &str) -> Result<i64, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("amount must not be empty".into());
    }
    let (whole, fraction) = match text.split_once('.') {
        Some((whole, fraction)) => (whole, fraction),
        None => (text, ""),
    };
    if whole.is_empty() && fraction.is_empty() {
        return Err("amount is not a number".into());
    }
    if !whole.chars().all(|c| c.is_ascii_digit()) || !fraction.chars().all(|c| c.is_ascii_digit()) {
        return Err("amount must be a plain decimal number".into());
    }
    if fraction.len() > 9 {
        return Err("amount accepts at most 9 decimals".into());
    }
    let whole: i64 = if whole.is_empty() {
        0
    } else {
        whole
            .parse()
            .map_err(|_| "amount is too large".to_string())?
    };
    let mut padded = fraction.to_string();
    while padded.len() < 9 {
        padded.push('0');
    }
    let fraction: i64 = if padded.is_empty() {
        0
    } else {
        padded
            .parse()
            .map_err(|_| "amount is too large".to_string())?
    };
    let nanos = whole
        .checked_mul(1_000_000_000)
        .and_then(|value| value.checked_add(fraction))
        .ok_or("amount is too large")?;
    if nanos < 0 {
        return Err("amount must not be negative".into());
    }
    Ok(nanos)
}

/// Formats a localized, non-leaking error message with a stable machine prefix.
pub fn error(code: &str, detail: &str) -> String {
    format!("{code}: {detail}")
}

macro_rules! stats_enum {
    ($(#[$enum_attr:meta])* $name:ident { $($(#[$variant_attr:meta])* $variant:ident => $text:literal),+ $(,)? }) => {
        $(#[$enum_attr])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(rename_all = "snake_case")]
        pub enum $name {
            $($(#[$variant_attr])* $variant),+
        }
        impl $name {
            /// Every variant, in declaration order (used by UI option lists).
            #[allow(dead_code)]
            pub const ALL: &'static [$name] = &[$($name::$variant),+];
            pub fn as_str(self) -> &'static str {
                match self { $($name::$variant => $text),+ }
            }
            /// Parses the wire form; used by SQL row mapping and the settings UI.
            #[allow(dead_code)]
            pub fn parse(value: &str) -> Option<Self> {
                match value { $($text => Some($name::$variant),)+ _ => None }
            }
        }
        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(self.as_str())
            }
        }
    };
}

stats_enum!(Source {
    Relay => "relay",
    ConnectionTest => "connection_test",
    /// A record read out of a WorkBuddy JSONL file.
    #[serde(rename = "workbuddy_import")]
    JsonlImport => "workbuddy_import",
});

stats_enum!(Outcome {
    InFlight => "in_flight",
    Completed => "completed",
    Failed => "failed",
    Cancelled => "cancelled",
    Interrupted => "interrupted",
    Rejected => "rejected",
    Recorded => "recorded",
});

stats_enum!(ErrorKind {
    MissingKey => "missing_key",
    Connect => "connect",
    UpstreamHttp => "upstream_http",
    UpstreamStream => "upstream_stream",
    SseError => "sse_error",
    ClientDrop => "client_drop",
    UncleanShutdown => "unclean_shutdown",
    Shutdown => "shutdown",
    Other => "other",
});

/// 一次请求最终 usage 数值的完整程度。
///
/// 这个枚举决定 Token 聚合是否可计入总量，也决定普通价格规则能否结算费用；它描述数值事实，
/// 不是 HTTP 成功状态。Rust 的枚举变体像 Java enum，但 `match` 在编译期还能提示未处理分支。
stats_enum!(UsageStatus {
    /// input/output 等结算所需核心字段均已确认；全零的确认值也属于 Complete。
    Complete => "complete",
    /// 只读取到部分 usage 字段；未出现的字段仍是 `None`，不能补 0。
    Partial => "partial",
    /// 上游调用结束但没有获得可用 usage。
    Missing => "missing",
    /// 收到了 usage，但负值、超界、互相矛盾或格式无效，整组数值不可信。
    Invalid => "invalid",
    /// 请求本地拒绝或证明未发送上游，Token usage 不适用；费用另按 not_applicable 结算。
    NotApplicable => "not_applicable",
});

/// usage 缺失或无效时的稳定原因标签，供查询筛选和诊断显示。
///
/// `None` 通常表示 usage 完整或没有额外失败原因；具体原因应与 [`UsageStatus`] 一起读，
/// 不能仅凭 reason 重算 Token。
stats_enum!(UsageReason {
    /// 上游正常结束，但没有提供 usage 对象。
    NoUsage => "no_usage",
    /// 客户端提前断开，后续 usage 可能不会再到达。
    ClientDrop => "client_drop",
    /// 流式传输/上游流处理失败。
    StreamError => "stream_error",
    /// 协议内容可读但 usage 结构或字段解析失败。
    ParseError => "parse_error",
    /// 单个 SSE/JSON 事件超过解析边界。
    EventTooLarge => "event_too_large",
    /// 非流式或聚合响应体超过边界。
    BodyTooLarge => "body_too_large",
    /// usage 使用了不受支持的编码或压缩表示。
    UnsupportedEncoding => "unsupported_encoding",
    /// 数值为负、非整数或超过允许范围。
    InvalidNumbers => "invalid_numbers",
    /// 同组字段之间不满足协议的一致性约束。
    Inconsistent => "inconsistent",
    /// 多个 usage 观察给出互相冲突的值。
    ConflictingUsage => "conflicting_usage",
    /// 请求被归为 Interrupted 且用量不完整时使用，也用于非正常关闭恢复；不只表示进程关闭。
    UncleanShutdown => "unclean_shutdown",
    /// 请求被证明未发送到上游，因此没有上游 usage。
    NotSent => "not_sent",
});

/// usage 数字的实际证据来源。
stats_enum!(UsageSource {
    /// 没有可归属的 usage 来源；常用于 Missing、Invalid、NotApplicable。
    None => "none",
    /// 由代理的协议解析/usage collector 从上游响应观察得到。
    Upstream => "upstream",
    /// 由 WorkBuddy JSONL 导入器读取历史记录得到。
    WorkbuddyJsonl => "workbuddy_jsonl",
});

/// 输入字段之间可被证明的组成关系。
stats_enum!(InputBreakdown {
    /// input 的组成关系未知；这不保证 input 总量或其他 token 字段已经已知。
    Unknown => "unknown",
    /// `cache_read` 是 `input_tokens` 的已观察子集；不证明 cache write 也包含在内。
    ReadSubset => "read_subset",
    /// read、write、uncached 是分别已知的互斥输入部分，可按各自费率计算。
    Disjoint => "disjoint",
});

/// 是否主动修改上游请求来索取 usage。
///
/// 默认是 `Request`（数据库升级也以它为新默认）。`Observe` 只是不主动索取 usage，仍会
/// 经历现有的代理模型/参数重写；直通不代表请求字段全部逐字不变。Java 类比上，这是显式
/// 配置值，不是由 Spring 根据环境自动切换的开关。
stats_enum!(#[derive(Default)] UsageMode {
    /// 只观察上游本来提供的 usage，不主动索取；直通仍不表示请求字段绝对原样不变。
    Observe => "observe",
    /// 默认值：在协议支持时尝试注入 usage 请求（例如 stream `include_usage`），但不保证
    /// 上游一定返回完整结构；字段名保留 wire 文本 `request`。
    #[default]
    Request => "request",
});

/// 请求准入时价格快照的来源类别。
///
/// 这是持久化/wire 分类，不一定说明是谁创建了规则。尤其 `"manual"` 同时用于人工编辑和
/// models.dev 目录自动导入的 `stats_model_prices` 行，不能显示成“人工录入”。
stats_enum!(PriceSource {
    /// 来自 `stats_model_prices` 的当前规则，包括人工编辑和目录导入。
    Manual => "manual",
    /// 当前发布缓存没有匹配规则，系统按默认币种采用的政策性零价。
    DefaultZero => "default_zero",
    /// 旧数据库迁移时无法还原来源的兼容标记；不能据此重新定价。
    LegacyUnknown => "legacy_unknown",
});

/// 单次请求费用结算结果的业务状态。
stats_enum!(CostStatus {
    /// 普通价格规则加完整 usage 后得到确定金额。
    Priced => "priced",
    /// 没有匹配规则，按明确配置的默认零价得到确定 0。
    DefaultZero => "default_zero",
    /// 存在已开始的上游调用但没有可应用规则；历史/导入记录可保留此状态，金额未知。
    Unpriced => "unpriced",
    /// 旧数据无法确认原始价格，金额来源不可恢复。
    LegacyUnknown => "legacy_unknown",
    /// Start 已建立费用快照但 Finish 尚未完成。
    Pending => "pending",
    /// usage 非 Complete，普通规则不能计算金额；不能把差额补 0。
    UsageIncomplete => "usage_incomplete",
    /// read discount/disjoint 缺少证明输入分解所需的 token 或费率。
    UnsupportedBreakdown => "unsupported_breakdown",
    /// 请求证明未进入上游，费用确定为 0。
    NotApplicable => "not_applicable",
    /// 合法大整数相乘或汇总超出 `i64`，金额保持未知。
    Overflow => "overflow",
});

/// 普通价格规则如何解释 input token 与 cache 字段。
stats_enum!(InputMode {
    /// input 费率覆盖全部 input token；cache 分项不另计。
    Flat => "flat",
    /// `input_tokens` 包含 cache read；非缓存部分与 cache read 分别按 input/cache 费率计。
    ReadDiscount => "read_discount",
    /// read、write、uncached 是互斥部分，分别使用 cache read/cache write/input 费率。
    Disjoint => "disjoint",
});

/// 当前支持计价的币种；wire 形式固定为大写三字母码。
///
/// 不同币种在汇总时保持分桶，系统不做汇率换算。Java 读者可把它看作只有两个合法值的强类型
/// money code；Serde 的 rename 定义实际 JSON 字符串。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Currency {
    #[serde(rename = "USD")]
    Usd,
    #[serde(rename = "CNY")]
    Cny,
}

impl Currency {
    /// Every currency, in declaration order (used by UI option lists).
    #[allow(dead_code)]
    pub const ALL: &'static [Currency] = &[Currency::Usd, Currency::Cny];
    pub fn as_str(self) -> &'static str {
        match self {
            Currency::Usd => "USD",
            Currency::Cny => "CNY",
        }
    }
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "USD" => Some(Currency::Usd),
            "CNY" => Some(Currency::Cny),
            _ => None,
        }
    }
}

impl std::fmt::Display for Currency {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

stats_enum!(Granularity {
    Auto => "auto",
    Hour => "hour",
    Day => "day",
    Week => "week",
});

stats_enum!(SeriesQuality {
    Known => "known",
    Partial => "partial",
    Unknown => "unknown",
    Empty => "empty",
});

stats_enum!(GroupBy {
    Provider => "provider",
    Model => "model",
});

stats_enum!(SortMetric {
    TotalTokens => "total_tokens",
    RequestCount => "request_count",
});

stats_enum!(WarningCode {
    UsageMissing => "usage_missing",
    UsagePartial => "usage_partial",
    UsageInvalid => "usage_invalid",
    CollectionDisabled => "collection_disabled",
    CollectorLoss => "collector_loss",
    UncleanExit => "unclean_exit",
    RetentionLimited => "retention_limited",
    ProviderObserveMode => "provider_observe_mode",
});

stats_enum!(HealthState {
    Ready => "ready",
    Disabled => "disabled",
    Degraded => "degraded",
    Migrating => "migrating",
});

stats_enum!(LossScope {
    NoLossObservedInCurrentRun => "no_loss_observed_in_current_run",
    LossObserved => "loss_observed",
    UnknownAfterUncleanExit => "unknown_after_unclean_exit",
});

stats_enum!(MaintenanceKind {
    Migration => "migration",
    Retention => "retention",
    Clear => "clear",
    Reprice => "reprice",
});

stats_enum!(MaintenanceStatus {
    Running => "running",
    Completed => "completed",
    Failed => "failed",
});

stats_enum!(FilterKind {
    Provider => "provider",
    Model => "model",
});

/// Maximum accepted length for identifier-like strings (`provider_id`, ...).
pub const MAX_ID_CHARS: usize = 160;
fn clean_id(value: &str) -> Result<String, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err("must not be empty".into());
    }
    if trimmed.chars().count() > MAX_ID_CHARS {
        return Err(format!("must be at most {MAX_ID_CHARS} characters"));
    }
    if trimmed.chars().any(char::is_control) {
        return Err("must not contain control characters".into());
    }
    Ok(trimmed.to_owned())
}

stats_enum!(#[derive(Default)] RecordOrigin {
    /// Traffic that went through this proxy.
    #[default]
    Proxy => "proxy",
    /// A record imported from WorkBuddy's own JSONL log.
    WorkbuddyBuiltin => "workbuddy_builtin",
    /// A record imported from a custom-vendor WorkBuddy log.
    WorkbuddyCustom => "workbuddy_custom",
});

stats_enum!(#[derive(Default)] TimeBasis {
    #[default]
    RequestStart => "request_start",
    MessageTimestamp => "message_timestamp",
});

impl RecordOrigin {
    /// True for every log-imported origin, including explicit proxy history.
    pub fn is_imported(self) -> bool {
        !matches!(self, RecordOrigin::Proxy)
    }
}

stats_enum!(#[derive(Default)] SourceScope {
    /// Everything: the proxy's own traffic plus every imported record.
    #[default]
    All => "all",
    /// Only what the supplier saw: relayed requests and confirmed direct use.
    Supplier => "supplier",
    /// Only WorkBuddy built-in model records.
    Builtin => "builtin",
});

impl SourceScope {
    /// True when proxy rows are part of this scope.
    pub fn includes_proxy(self) -> bool {
        matches!(self, SourceScope::All | SourceScope::Supplier)
    }

    /// True when imported rows are part of this scope.
    pub fn includes_imported(self) -> bool {
        true
    }
}

stats_enum!(#[derive(Default)] CaptureMethod {
    #[default]
    All => "all",
    Proxy => "proxy",
    Workbuddy => "workbuddy",
});

// Record categories are orthogonal to model origin and capture method.
stats_enum!(#[derive(Default)] RecordKind {
    #[default]
    All => "all",
    Model => "model",
    Tool => "tool",
    OtherCharge => "other_charge",
});

/// Common read filter shared by dashboard, requests, breakdown and reprice.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StatsFilter {
    pub from_ms: i64,
    pub to_ms: i64,
    /// Which origins this report includes. Defaults to `all`, so every existing
    /// caller keeps its current meaning.
    #[serde(default)]
    pub source_scope: SourceScope,
    pub capture_method: CaptureMethod,
    pub record_kind: RecordKind,
    /// IANA timezone name. Empty asks the core for the local zone.
    pub timezone: String,
    pub provider_id: Option<String>,
    pub upstream_model: Option<String>,
    pub route_id: Option<String>,
    pub outcome: Option<Outcome>,
    pub usage_status: Option<UsageStatus>,
    pub include_tests: bool,
}

impl StatsFilter {
    /// Validates every field once; the result is reused by the SQL builder.
    pub fn validate(&self) -> Result<(), String> {
        if self.source_scope == SourceScope::Builtin && self.capture_method == CaptureMethod::Proxy
        {
            return Err(error(
                ERR_INVALID,
                "built-in models cannot use proxy-only capture",
            ));
        }
        // The scope must name a real scope and include at least one origin.
        if SourceScope::parse(self.source_scope.as_str()) != Some(self.source_scope) {
            return Err(error(ERR_INVALID, "source_scope is not a known scope"));
        }
        if !self.source_scope.includes_proxy() && !self.source_scope.includes_imported() {
            return Err(error(
                ERR_INVALID,
                "source_scope must include at least one origin",
            ));
        }
        if self.from_ms < 0 || self.to_ms < 0 {
            return Err(error(ERR_INVALID, "from_ms and to_ms must not be negative"));
        }
        if self.from_ms >= self.to_ms {
            return Err(error(ERR_INVALID, "from_ms must be smaller than to_ms"));
        }
        if self.to_ms - self.from_ms > MAX_RANGE_MS {
            return Err(error(
                ERR_INVALID,
                "range must not exceed 366 days; narrow the range",
            ));
        }
        if self.to_ms / 1000 > i32::MAX as i64 * 4 {
            return Err(error(ERR_INVALID, "to_ms is out of range"));
        }
        if self.timezone.chars().count() > 64 || self.timezone.chars().any(char::is_control) {
            return Err(error(ERR_INVALID, "timezone is not a valid IANA name"));
        }
        for (name, value) in [
            ("provider_id", &self.provider_id),
            ("upstream_model", &self.upstream_model),
            ("route_id", &self.route_id),
        ] {
            if let Some(value) = value {
                clean_id(value).map_err(|e| error(ERR_INVALID, &format!("{name} {e}")))?;
            }
        }
        Ok(())
    }

    /// Normalized key used for cursor consistency checks, never for auth.
    pub fn key(&self) -> String {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(b"wbp-stats-filter-v1\n");
        let mut push = |name: &str, value: &str| {
            hasher.update(name.as_bytes());
            hasher.update(b"=");
            hasher.update(value.as_bytes());
            hasher.update(b"\n");
        };
        push("from", &self.from_ms.to_string());
        push("to", &self.to_ms.to_string());
        push("tz", &self.timezone);
        push("provider", self.provider_id.as_deref().unwrap_or(""));
        push("model", self.upstream_model.as_deref().unwrap_or(""));
        push("route", self.route_id.as_deref().unwrap_or(""));
        push("outcome", self.outcome.map_or("", |v| v.as_str()));
        push("usage", self.usage_status.map_or("", |v| v.as_str()));
        push("tests", if self.include_tests { "1" } else { "0" });
        // The source scope is part of the query's identity: switching it must
        // invalidate every cursor issued under the old one.
        push("scope", self.source_scope.as_str());
        push("capture", self.capture_method.as_str());
        if self.record_kind != RecordKind::All {
            push("record_kind", self.record_kind.as_str());
        }
        hasher
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }
}

/// One pending-candidate group, as the pending command returns it.
///
/// Only the model, the counts and the time bounds: never a message body, never
/// a file path.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PendingGroupDto {
    pub model_id: String,
    pub count: i64,
    pub first_at_ms: Option<i64>,
    pub last_at_ms: Option<i64>,
    pub reason: String,
}

/// A cursor over the pending-candidate groups.
///
/// It carries the mapping revision it was issued under, so a rule change (or a
/// withdrawal) invalidates it instead of silently returning a different set of
/// groups under the same page number.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingCursor {
    /// The mapping revision in force when the page was issued.
    pub mapping_revision: i64,
    /// The `requested_model` the next page starts after.
    pub after_model: String,
    /// The size the page was issued with.
    pub limit: i64,
}

impl PendingCursor {
    /// Validates the cursor against the revision currently in force.
    pub fn validate(&self, mapping_revision: i64) -> Result<(), String> {
        if self.limit != 20 && self.limit != 50 && self.limit != 100 {
            return Err(error(ERR_INVALID, "cursor limit is not one of 20, 50, 100"));
        }
        if self.mapping_revision != mapping_revision {
            return Err(error(
                ERR_CURSOR_EXPIRED,
                "cursor was invalidated by a mapping change",
            ));
        }
        Ok(())
    }
}

/// 一个币种下的费用小计。
///
/// 不同币种不会被相加或换算，因此同一汇总里可能有多条 `CostTotalDto`。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CostTotalDto {
    /// 该小计的币种；金额只与同币种值相加。
    pub currency: Currency,
    /// 十进制字符串编码的 nano 货币单位总量；字符串可承载超过 `i64` 的汇总值。
    pub total_cost_nanos: String,
    /// 被计入该金额的已定价请求数，不是查询范围内全部请求数。
    pub priced_request_count: i64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SummaryDto {
    pub request_count: i64,
    pub finished_count: i64,
    pub in_flight_count: i64,
    pub completed_count: i64,
    pub failed_count: i64,
    pub rejected_count: i64,
    pub cancelled_count: i64,
    pub interrupted_count: i64,
    pub usage_eligible_count: i64,
    pub usage_complete_count: i64,
    pub usage_partial_count: i64,
    pub usage_missing_count: i64,
    pub usage_invalid_count: i64,
    pub usage_not_applicable_count: i64,
    /// `None` means "no confirmed usage", never zero.
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
    pub cache_read_tokens: Option<i64>,
    pub cache_read_known_count: i64,
    pub cache_read_denominator_input_tokens: Option<i64>,
    pub cache_write_tokens: Option<i64>,
    pub cache_write_known_count: i64,
    pub input_uncached_tokens: Option<i64>,
    pub input_breakdown_known_count: i64,
    pub reasoning_tokens: Option<i64>,
    pub reasoning_known_count: i64,
    pub success_rate: Option<f64>,
    pub usage_coverage: Option<f64>,
    pub cache_hit_rate: Option<f64>,
    pub avg_duration_ms: Option<f64>,
    pub costs: Vec<CostTotalDto>,
    pub unpriced_request_count: i64,
    /// Requests priced by the explicit "no price configured" default.
    pub default_zero_request_count: i64,
    /// Requests whose amount is decided and therefore included in `costs`.
    pub settled_cost_request_count: i64,
    pub cost_incomplete_request_count: i64,
    /// Records that came through the proxy.
    #[serde(rename = "proxy_request_count", alias = "proxy_record_count")]
    pub proxy_record_count: i64,
    /// Records imported from WorkBuddy's log.
    #[serde(rename = "workbuddy_record_count", alias = "imported_record_count")]
    pub imported_record_count: i64,
    /// Imported records that carry no decided price.
    pub imported_unpriced_count: i64,
    /// The denominator of the pricing rates: every row whose cost state is
    /// decided (settled, or explicitly defaulted to zero).
    pub cost_known_count: i64,
    /// WorkBuddy credit, summed on its own. Credit is a different unit from
    /// money, so it is **never** added into `costs` — and the per-currency
    /// totals keep different currencies apart for the same reason.
    #[serde(rename = "workbuddy_credit_nanos", alias = "credit_nanos")]
    pub credit_nanos: Option<String>,
    #[serde(rename = "credit_known_count", alias = "credit_record_count")]
    pub credit_record_count: i64,
    #[serde(default)]
    pub status_known_count: i64,
    #[serde(default)]
    pub pending_import_count: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SeriesPointDto {
    pub start_ms: i64,
    pub end_ms: i64,
    pub request_count: i64,
    pub finished_count: i64,
    pub usage_complete_count: i64,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
    pub cache_read_tokens: Option<i64>,
    pub cache_write_tokens: Option<i64>,
    pub cache_read_known_count: i64,
    pub cache_write_known_count: i64,
    pub cache_read_denominator_input_tokens: Option<i64>,
    pub costs: Vec<CostTotalDto>,
    pub quality: SeriesQuality,
    pub collection_warning: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WarningDto {
    pub code: WarningCode,
    pub request_count: i64,
    pub from_ms: Option<i64>,
    pub to_ms: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DashboardDto {
    pub api_version: u32,
    pub request_tag: String,
    pub generated_at_ms: i64,
    pub generation: String,
    /// Timezone actually used for bucketing (resolved from the filter).
    pub timezone: String,
    pub filter_key: String,
    pub granularity: Granularity,
    pub summary: SummaryDto,
    pub series: Vec<SeriesPointDto>,
    pub warnings: Vec<WarningDto>,
}

/// 单条请求在 Finish 或恢复收敛时形成的费用结果。
///
/// 金额来自 Start 冻结的价格快照，不是查询时临时 join 当前价格。`None` 必须和状态一起
/// 解释：已确定状态有金额，`UsageIncomplete`/`UnsupportedBreakdown`/`Overflow` 等未知
/// 状态不伪造 0。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CostDto {
    /// 费用是否已确定、为何未知以及是否属于默认零价。
    pub status: CostStatus,
    /// 费用币种；旧记录或无法识别的费用可能没有币种。
    pub currency: Option<Currency>,
    /// nano 货币单位金额的十进制字符串；未知为 `None`，确认零时可为 `"0"`。
    pub total_cost_nanos: Option<String>,
    /// 仅供追踪的取价配置 id。
    ///
    /// 费用计算不会 join 当前配置，因此编辑或删除规则后历史值仍可解释；默认零价没有来源
    /// 规则，字段可为 `None`。
    pub source_price_id: Option<String>,
    /// 费用公式/快照结构版本，用于解释历史记录，不是价格行 revision。
    pub pricing_version: Option<i64>,
}

/// 请求 Start 时写入自身费用行的单价快照。
///
/// 所有费率都是 nano 货币单位/1,000,000 token 的十进制字符串。它是历史事实，不指向当前
/// 价格列表；`price_source=manual` 同时包含人工编辑和目录导入，不能当作“人工来源”信号。
/// 字段 `None` 多见于旧记录或来源无法还原的数据。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PriceSnapshotDto {
    /// 快照来源类别；`manual` 是存储分类，不保证由人工创建。
    pub price_source: PriceSource,
    /// 该请求准入取价的 epoch 毫秒。
    pub price_snapshot_at_ms: Option<i64>,
    /// 输入计价方式；`None` 表示旧/不完整快照无法确认。
    pub input_mode: Option<InputMode>,
    /// 普通输入费率，单位 nano 货币单位/1,000,000 token。
    pub input_rate_nanos: Option<String>,
    /// 输出费率，单位 nano 货币单位/1,000,000 token。
    pub output_rate_nanos: Option<String>,
    /// cache read 费率；`Some("0")` 是确认零价，`None` 是未知。
    pub cache_read_rate_nanos: Option<String>,
    /// cache write 费率；`Some("0")` 是确认零价，`None` 是未知。
    pub cache_write_rate_nanos: Option<String>,
    /// 快照复制的来源价格 id，仅用于追踪当时配置。
    pub source_price_id: Option<String>,
    /// 取价时来源价格行的 revision；默认零价或旧记录可为 `None`。
    pub source_price_revision: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]

pub struct RequestRowDto {
    pub id: i64,
    pub request_id: String,
    pub source: Source,
    pub provider_id: String,
    pub provider_name: String,
    pub route_id: String,
    pub route_name: String,
    pub requested_model: String,
    pub upstream_model: String,
    #[serde(default)]
    pub upstream_protocol: Option<String>,
    pub response_model: Option<String>,
    pub started_at_ms: i64,
    pub ended_at_ms: Option<i64>,
    pub stream: Option<bool>,
    /// `None` while a started record cannot prove whether the call was sent.
    pub upstream_started: Option<bool>,
    pub http_status: Option<i64>,
    pub outcome: Outcome,
    pub error_kind: Option<ErrorKind>,
    pub duration_ms: Option<i64>,
    pub ttfb_ms: Option<i64>,
    pub first_content_ms: Option<i64>,
    pub usage_status: UsageStatus,
    pub usage_reason: Option<UsageReason>,
    pub usage_source: UsageSource,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
    pub cache_read_tokens: Option<i64>,
    pub cache_write_tokens: Option<i64>,
    pub reasoning_tokens: Option<i64>,
    pub input_uncached_tokens: Option<i64>,
    pub input_breakdown: InputBreakdown,
    pub cost: Option<CostDto>,
    /// Which side wrote this row.
    #[serde(default)]
    #[serde(rename = "record_origin", alias = "origin")]
    pub origin: RecordOrigin,
    #[serde(default)]
    pub time_basis: TimeBasis,
    #[serde(default)]
    pub total_tokens_derived: bool,
    #[serde(default)]
    pub workbuddy_credit_nanos: Option<String>,
    /// True when the record is a finished import rather than live traffic.
    #[serde(default)]
    pub recorded: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RequestCursor {
    pub last_started_at_ms: i64,
    pub last_id: i64,
    pub as_of_id: i64,
    pub filter_key: String,
    pub run_id: String,
    pub maintenance_revision: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RequestPageDto {
    pub api_version: u32,
    pub request_tag: String,
    pub generated_at_ms: i64,
    pub generation: String,
    pub timezone: String,
    pub filter_key: String,
    /// Echo a direct-page request so a new UI cannot silently show the wrong
    /// page when connected to an older core that ignores unknown fields.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<i64>,
    pub as_of_id: i64,
    pub total_rows: i64,
    pub page_size: i64,
    pub items: Vec<RequestRowDto>,
    pub next_cursor: Option<RequestCursor>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PriceRuleDto {
    /// Canonical v2 identifier.
    pub price_id: String,
    /// Transitional alias of `price_id`, kept so older callers keep working.
    /// Both fields always carry the same value.
    pub rule_id: String,
    /// 配置中的 provider id，与 `upstream_model` 一起构成不可重复的当前价格键。
    pub provider_id: String,
    /// 上游模型名；编辑现有规则时该键不可改变。
    pub upstream_model: String,
    /// 该规则使用的币种；不同币种费用不合并。
    pub currency: Currency,
    /// 普通 input 费率，整数 nano 货币单位/1,000,000 token 的字符串。
    pub input_rate_nanos: String,
    /// 普通 output 费率，单位同 `input_rate_nanos`。
    pub output_rate_nanos: String,
    /// cache read 费率；当前读取路径会给出字符串，`None` 保留旧 wire 兼容。
    pub cache_read_rate_nanos: Option<String>,
    /// cache write 费率；当前读取路径会给出字符串，`None` 保留旧 wire 兼容。
    pub cache_write_rate_nanos: Option<String>,
    /// input token 的计价方式。
    pub input_mode: InputMode,
    /// 设置列表显示标签；可能来自人工填写或 models.dev 自动目录。
    pub label: String,
    /// 当前规则创建时间的 epoch 毫秒。
    pub created_at_ms: i64,
    /// 当前价格行乐观并发令牌；保存/删除必须带回读取时的值，每次编辑递增。
    pub revision: i64,
    /// Kept for wire compatibility; v2 prices are never "in use".
    pub referenced: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RequestDetailDto {
    pub api_version: u32,
    pub request_tag: String,
    pub generated_at_ms: i64,
    pub generation: String,
    pub request: RequestRowDto,
    pub parser_version: i64,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub run_id: String,
    pub cost_parts: Option<CostPartsDto>,
    /// The prices this request was admitted with, read from its own row.
    pub price_snapshot: Option<PriceSnapshotDto>,
}

/// 单次请求费用的分项与总额，来自该请求自身快照的结算。
///
/// 金额均为 nano 货币单位字符串。对于 UsageIncomplete、UnsupportedBreakdown 或 Overflow，
/// 各分项和 total 保持 `None`；对于确认的零价，分项和 total 可以是 `Some("0")`。不能把
/// `None` 当 0 参与统计。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CostPartsDto {
    /// 这组分项对应的费用状态。
    pub status: CostStatus,
    /// 结算币种；通常来自请求快照，`not_applicable` 也可以有币种。只有旧记录或整组费用
    /// 不可得时才可能为 `None`。
    pub currency: Option<Currency>,
    /// 输入侧费用：`flat` 包含全部 input；`read_discount` 只含非缓存输入；
    /// `disjoint` 含显式的 uncached 部分。
    pub input_cost_nanos: Option<String>,
    /// 输出 token 费用。
    pub output_cost_nanos: Option<String>,
    /// cache read 费用；模式不单独收费时通常是确认的 `Some("0")`，只有整组费用不可得或
    /// 旧记录无法还原时才为 `None`。
    pub cache_read_cost_nanos: Option<String>,
    /// cache write 费用；模式不单独收费时通常是确认的 `Some("0")`，只有整组费用不可得或
    /// 旧记录无法还原时才为 `None`。
    pub cache_write_cost_nanos: Option<String>,
    /// 各分项费用之和；未知时保持 `None`，确认零时可显示 `"0"`。
    pub total_cost_nanos: Option<String>,
    /// 本次结算计算完成的 epoch 毫秒。
    pub calculated_at_ms: i64,
    /// 费用公式/快照结构版本，用于解释历史分项。
    pub pricing_version: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BreakdownItemDto {
    pub key: String,
    pub label: String,
    pub provider_count: i64,
    pub summary: SummaryDto,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BreakdownCursor {
    /// The grouping this cursor was issued for; a page from another grouping is
    /// not reproducible.
    pub group_by: GroupBy,
    pub sort: SortMetric,
    /// The last group *value* (a provider id or a model name), never a column
    /// name: the keyset compares it against the aggregated group key.
    pub last_group_key: String,
    /// The last group's sort metric; `None` means the NULL tail.
    pub last_metric: Option<i64>,
    pub as_of_id: i64,
    pub filter_key: String,
    pub run_id: String,
    pub maintenance_revision: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BreakdownDto {
    pub api_version: u32,
    pub request_tag: String,
    pub generated_at_ms: i64,
    pub generation: String,
    pub timezone: String,
    pub filter_key: String,
    pub group_by: GroupBy,
    pub sort: SortMetric,
    /// Echo a direct-page request so a new UI cannot silently show the wrong
    /// page when connected to an older core that ignores unknown fields.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<i64>,
    pub as_of_id: i64,
    pub total_groups: i64,
    pub items: Vec<BreakdownItemDto>,
    pub next_cursor: Option<BreakdownCursor>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FilterOptionDto {
    pub key: String,
    pub label: String,
    /// True when the option only exists in historical rows.
    pub historical: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FilterOptionsDto {
    pub api_version: u32,
    pub request_tag: String,
    pub generated_at_ms: i64,
    pub generation: String,
    pub kind: FilterKind,
    pub items: Vec<FilterOptionDto>,
    pub next_key: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HealthDto {
    pub api_version: u32,
    pub request_tag: String,
    pub generated_at_ms: i64,
    pub generation: String,
    pub state: HealthState,
    pub enabled: bool,
    pub schema_version: i64,
    pub collection_started_at_ms: Option<i64>,
    pub earliest_record_at_ms: Option<i64>,
    pub retention_days: i64,
    pub record_count: i64,
    pub db_bytes: i64,
    pub wal_bytes: i64,
    pub queue_depth: i64,
    pub queue_capacity: i64,
    pub pending_requests: i64,
    pub dropped_requests_current_run: i64,
    pub write_failures_current_run: i64,
    pub last_commit_at_ms: Option<i64>,
    pub loss_scope: LossScope,
    pub error_code: Option<String>,
    pub settings_revision: i64,
    pub timezone: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SettingsDto {
    pub api_version: u32,
    pub request_tag: String,
    pub generated_at_ms: i64,
    pub generation: String,
    pub enabled: bool,
    pub retention_days: i64,
    /// Currency used for models that have no configured price.
    pub default_currency: Currency,
    pub revision: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProviderSettingsDto {
    pub api_version: u32,
    pub request_tag: String,
    pub generated_at_ms: i64,
    pub generation: String,
    pub provider_id: String,
    pub usage_mode: UsageMode,
    pub revision: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PriceListDto {
    #[serde(default)]
    pub catalog_sync: Option<PriceSyncDto>,
    pub api_version: u32,
    pub request_tag: String,
    pub generated_at_ms: i64,
    pub generation: String,
    pub items: Vec<PriceRuleDto>,
    pub next_rule_id: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PriceSyncDto {
    /// 最近一次成功下载并提交目录的本地 `YYYY-MM-DD`；仅补齐同日缓存时不会推进。
    pub last_success_day: Option<String>,
    /// store 收到调用者传入 `now` 的目录导入写入时间，epoch 毫秒；不直接等同网络下载
    /// 完成时刻，也不是当前价格规则更新时间。仅补齐同日缓存时该字段不会改写。
    pub updated_at_ms: Option<i64>,
    /// 最近一次下载实际插入或覆盖的价格行数。
    pub updated_count: i64,
    /// 最近一次下载遍历目标中未找到唯一可信目录价格的键数。
    pub unmatched_count: i64,
    /// 下载目录中通过解析的有效条目总数。
    pub catalog_count: i64,
    /// 生成状态时的目标列表键，用于判断同日是否只需补齐新增 provider/model。
    pub targets_key: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MaintenanceDto {
    pub api_version: u32,
    pub request_tag: String,
    pub generated_at_ms: i64,
    pub generation: String,
    pub task_id: i64,
    pub kind: MaintenanceKind,
    pub status: MaintenanceStatus,
    pub affected_rows: i64,
    pub started_at_ms: i64,
    pub completed_at_ms: Option<i64>,
    pub error_code: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ClearPreviewDto {
    pub api_version: u32,
    pub request_tag: String,
    pub generated_at_ms: i64,
    pub generation: String,
    pub before_ms: i64,
    pub as_of_id: i64,
    pub row_count: i64,
    pub confirmed: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// R8-06: amounts travel as decimal strings because a total can exceed
    /// `i64`. Parsing them into `i64` and falling back turned a real amount into
    /// "0" or "—" on the page.
    #[test]
    fn amounts_beyond_i64_still_format_exactly() {
        // 2^63 nano units is already past `i64::MAX`.
        let huge = "9223372036854775808";
        assert_eq!(format_amount_text(huge), "9223372036.854775808");
        let bigger = "123456789012345678901234567890";
        assert_eq!(format_amount_text(bigger), "123456789012345678901.23456789");
        assert_eq!(format_amount_text("0"), "0");
        // Below the display floor the page says so instead of showing 0.
        assert_eq!(format_amount_text("1"), "<0.0001");
        assert_eq!(format_amount_text("99999"), "<0.0001");
        assert_eq!(format_amount_text("100000"), "0.0001");
        assert_eq!(format_amount_text("-1000000000"), "-1");
        assert_eq!(format_amount_text("-1"), ">-0.0001");
        // Anything that is not a decimal amount is unknown, never zero.
        assert_eq!(format_amount_text(""), "—");
        assert_eq!(format_amount_text("1.5"), "—");
        assert_eq!(format_amount_text("abc"), "—");
        // The i64 entry point agrees with the shared formatter.
        assert_eq!(format_amount(1_360_000), format_amount_text("1360000"));
        assert_eq!(
            format_amount(i64::MAX),
            format_amount_text(&i64::MAX.to_string())
        );
    }

    fn filter() -> StatsFilter {
        StatsFilter {
            from_ms: 1_790_784_000_000,
            to_ms: 1_791_993_600_000,
            timezone: "Asia/Shanghai".into(),
            ..Default::default()
        }
    }

    #[test]
    fn filter_validation_bounds_range_and_identifiers() {
        let mut f = filter();
        assert!(f.validate().is_ok());
        f.to_ms = f.from_ms;
        assert!(f.validate().unwrap_err().starts_with(ERR_INVALID));
        let mut f = filter();
        f.to_ms = f.from_ms + MAX_RANGE_MS + 1;
        assert!(f.validate().unwrap_err().contains("366 days"));
        let mut f = filter();
        f.provider_id = Some("bad\u{7}id".into());
        assert!(f.validate().is_err());
        let mut f = filter();
        f.provider_id = Some("  ".into());
        assert!(f.validate().is_err());
    }

    #[test]
    fn filter_key_is_stable_and_field_sensitive() {
        let a = filter();
        let b = filter();
        assert_eq!(a.key(), b.key());
        assert_eq!(a.key().len(), 64);
        let mut c = filter();
        c.include_tests = true;
        assert_ne!(a.key(), c.key());
        let mut d = filter();
        d.timezone = "UTC".into();
        assert_ne!(a.key(), d.key());
    }

    #[test]
    fn enum_text_round_trips() {
        for outcome in Outcome::ALL {
            assert_eq!(Outcome::parse(outcome.as_str()), Some(*outcome));
        }
        for status in UsageStatus::ALL {
            assert_eq!(UsageStatus::parse(status.as_str()), Some(*status));
        }
        assert_eq!(
            serde_json::to_string(&Outcome::InFlight).unwrap(),
            "\"in_flight\""
        );
        assert_eq!(serde_json::to_string(&Currency::Usd).unwrap(), "\"USD\"");
        assert!(Outcome::parse("unknown").is_none());
    }

    #[test]
    fn unknown_usage_stays_null_through_serialization() {
        let summary = SummaryDto::default();
        let json = serde_json::to_value(&summary).unwrap();
        assert!(json["input_tokens"].is_null());
        assert_eq!(json["request_count"], 0);
    }
}
