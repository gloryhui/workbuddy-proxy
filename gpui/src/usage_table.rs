//! Table delegates for the statistics page: the request log plus the provider
//! and model summaries. All rows are already server-aggregated; the table only
//! renders the current page; per-request display ratios use the stored counters.
use crate::stats_dto::RecordOrigin;
use crate::{
    statistics::Statistics,
    stats_dto::{
        BreakdownItemDto, CostStatus, Currency, Outcome, RequestRowDto, Source, UsageStatus,
    },
};
use chrono::{Local, TimeZone as _};
use gpui_kit::component::{
    Sizable,
    label::Label,
    table::{Column, TableDelegate, TableState},
    tag::Tag,
};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

mod charge_labels;
pub use charge_labels::{
    is_charge, is_tool_charge, model_label, model_text, requested_text, route_text,
};

pub fn muted(text: impl Into<SharedString>) -> Label {
    Label::new(text)
        .text_size(px(crate::theme::font::LABEL))
        .text_color(rgb(0x6b7c90))
}

pub fn strong(text: impl Into<SharedString>) -> Label {
    Label::new(text)
        .text_size(px(13.))
        .text_color(rgb(0x19334d))
}

/// Compact token/number formatting. Chinese uses 万/亿, English k/M/B.
pub fn compact(value: Option<i64>, zh: bool) -> String {
    let Some(value) = value else {
        return "—".into();
    };
    let magnitude = value.unsigned_abs();
    let (unit, scale) = if zh {
        if magnitude >= 100_000_000 {
            ("亿", 100_000_000f64)
        } else if magnitude >= 10_000 {
            ("万", 10_000f64)
        } else {
            return value.to_string();
        }
    } else if magnitude >= 1_000_000_000 {
        ("B", 1_000_000_000f64)
    } else if magnitude >= 1_000_000 {
        ("M", 1_000_000f64)
    } else if magnitude >= 1_000 {
        ("k", 1_000f64)
    } else {
        return value.to_string();
    };
    let scaled = value as f64 / scale;
    let text = if scaled.abs() >= 100. {
        format!("{scaled:.0}")
    } else if scaled.abs() >= 10. {
        format!("{scaled:.1}")
    } else {
        format!("{scaled:.2}")
    };
    format!("{text}{unit}")
}

/// Model-source classification, matching the heading's built-in/custom filter.
/// Proxy capture and imported custom-vendor records are both non-built-in;
/// capture method stays separate in the request details.
pub fn origin_text(origin: RecordOrigin, zh: bool) -> &'static str {
    match (origin, zh) {
        (RecordOrigin::WorkbuddyBuiltin, true) => "内置",
        (RecordOrigin::WorkbuddyBuiltin, false) => "Built-in",
        (RecordOrigin::Proxy | RecordOrigin::WorkbuddyCustom, true) => "自定义",
        (RecordOrigin::Proxy | RecordOrigin::WorkbuddyCustom, false) => "Custom",
    }
}

/// The status column's text.
///
/// An imported record reads as "recorded": the log it came from says the message
/// exists, not that a request succeeded, so the row never borrows an HTTP result
/// it does not have.
pub fn status_text(recorded: bool, outcome: Outcome, zh: bool) -> &'static str {
    if recorded {
        return if zh { "已记录" } else { "Recorded" };
    }
    match (outcome, zh) {
        (Outcome::InFlight, true) => "进行中",
        (Outcome::InFlight, false) => "In flight",
        (Outcome::Completed, true) => "完成",
        (Outcome::Completed, false) => "Completed",
        (Outcome::Failed, true) => "失败",
        (Outcome::Failed, false) => "Failed",
        (Outcome::Cancelled, true) => "已取消",
        (Outcome::Cancelled, false) => "Cancelled",
        (Outcome::Interrupted, true) => "中断",
        (Outcome::Interrupted, false) => "Interrupted",
        (Outcome::Rejected, true) => "拒绝",
        (Outcome::Rejected, false) => "Rejected",
        (Outcome::Recorded, true) => "已记录",
        (Outcome::Recorded, false) => "Recorded",
    }
}

/// The HTTP column's text.
///
/// A record without an HTTP status shows a dash. Rendering `0` or a default
/// `200` would invent a result that was never observed.
pub fn http_text(status: Option<i64>) -> String {
    exact(status)
}

/// Exact integer formatting for tooltips and details.
pub fn exact(value: Option<i64>) -> String {
    value.map_or_else(|| "—".into(), |value| value.to_string())
}

/// Amount formatting shared with the core (`<0.0001` below the display floor).
pub fn amount(nanos: Option<&str>, currency: Option<Currency>) -> String {
    let Some(text) = nanos else {
        return "—".into();
    };
    let symbol = match currency {
        Some(Currency::Usd) => "$",
        Some(Currency::Cny) => "¥",
        None => "",
    };
    // Amounts travel as decimal strings and may exceed `i64`; the shared
    // formatter keeps them exact instead of collapsing to "—".
    format!("{symbol}{}", crate::stats_dto::format_amount_text(text))
}

pub fn percent(value: Option<f64>) -> String {
    value.map_or_else(|| "—".into(), |value| format!("{:.1}%", value * 100.))
}

/// Input includes cache reads in all normalized protocol usage records.
pub fn cache_rate(read: Option<i64>, input: Option<i64>) -> Option<f64> {
    match (read, input) {
        (Some(read), Some(input)) if input > 0 && read >= 0 && read <= input => {
            Some(read as f64 / input as f64)
        }
        _ => None,
    }
}

/// Streaming output rate excludes the wait for first content. Non-streaming
/// responses have no observable generation interval, so use end-to-end time.
/// Never replace a missing/invalid streaming interval with a different metric.
pub fn output_rate(
    output: Option<i64>,
    duration_ms: Option<i64>,
    first_content_ms: Option<i64>,
    stream: bool,
) -> Option<f64> {
    let output = output.filter(|value| *value >= 0)?;
    let duration = duration_ms.filter(|value| *value > 0)?;
    let elapsed = if stream {
        let first = first_content_ms.filter(|value| *value >= 0)?;
        duration.checked_sub(first).filter(|value| *value > 0)?
    } else {
        duration
    };
    Some(output as f64 * 1000. / elapsed as f64)
}

/// Displays WorkBuddy credits exactly, independently of currency formatting.
/// Aggregates can exceed i64, and even one nano-credit is a known value.
pub fn format_credit_nanos(text: &str) -> Option<String> {
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let nanos = text.parse::<i128>().ok()?;
    let whole = nanos / 1_000_000_000;
    let fraction = nanos % 1_000_000_000;
    if fraction == 0 {
        return Some(whole.to_string());
    }
    Some(
        format!("{whole}.{fraction:09}")
            .trim_end_matches('0')
            .to_owned(),
    )
}

pub fn rate_text(item: &RequestRowDto) -> String {
    item.stream
        .and_then(|stream| {
            output_rate(
                item.output_tokens,
                item.duration_ms,
                item.first_content_ms,
                stream,
            )
        })
        .map_or_else(|| "—".into(), |rate| format!("{rate:.1}"))
}

pub fn rate_hint(stream: impl Into<Option<bool>>, zh: bool) -> &'static str {
    match (stream.into(), zh) {
        (Some(true), true) => {
            "流式输出 tok/s = 输出 Token ÷（总耗时 − 首内容耗时）秒。包含输出中的推理 Token；仅为平均速率，尾部传输耗时也计入。数据缺失或时间区间无效时显示 —。"
        }
        (Some(true), false) => {
            "Streaming tok/s = output tokens / (duration − time to first content) in seconds. Includes reasoning counted in output and tail transfer time; an average, not instantaneous speed. Missing or invalid intervals show —."
        }
        (Some(false), true) => {
            "非流式 tok/s = 输出 Token ÷ 总耗时秒，包含等待时间的端到端平均速率。数据缺失或耗时无效时显示 —。"
        }
        (Some(false), false) => {
            "Non-streaming tok/s = output tokens / total seconds, an end-to-end average including wait time. Missing data or invalid duration shows —."
        }
        (None, true) => "日志未记录流式状态或可靠耗时，无法计算 tok/s。",
        (None, false) => {
            "The log has no streaming state or reliable duration; tok/s is unavailable."
        }
    }
}

pub fn timestamp(ms: i64, _zh: bool) -> String {
    match Local.timestamp_millis_opt(ms).single() {
        Some(value) => value.format("%m-%d %H:%M:%S").to_string(),
        None => "—".into(),
    }
}

/// Parses a local-time stamp typed by the user (`YYYY-MM-DD HH:MM`, seconds
/// optional) using this machine's timezone. Returns `None` for anything else so
/// the caller can show an error instead of guessing.
pub fn parse_local_time(text: &str) -> Option<i64> {
    let text = text.trim();
    let formats = ["%Y-%m-%d %H:%M:%S", "%Y-%m-%d %H:%M", "%Y/%m/%d %H:%M"];
    for format in formats {
        if let Ok(value) = chrono::NaiveDateTime::parse_from_str(text, format)
            && let chrono::LocalResult::Single(local) = value.and_local_timezone(chrono::Local)
        {
            return Some(local.timestamp_millis());
        }
    }
    None
}

/// The same conversion as [`rate_amount`], for callers that own the currency.
pub fn rate_input(nanos: &str) -> String {
    rate_amount(nanos, crate::stats_dto::Currency::Cny)
}

/// Renders a stored nano rate as the human per-million amount the form edits.
/// The conversion happens exactly once, for display only.
pub fn rate_amount(nanos: &str, _currency: crate::stats_dto::Currency) -> String {
    match nanos.parse::<i64>() {
        Ok(value) => {
            let whole = value / 1_000_000_000;
            let fraction = value % 1_000_000_000;
            if fraction == 0 {
                return whole.to_string();
            }
            let mut text = format!("{whole}.{fraction:09}");
            while text.ends_with('0') {
                text.pop();
            }
            text
        }
        Err(_) => String::new(),
    }
}

pub fn duration(ms: Option<i64>) -> String {
    match ms {
        None => "—".into(),
        Some(value) if value < 1000 => format!("{value} ms"),
        Some(value) => format!("{:.2} s", value as f64 / 1000.),
    }
}

/// Localized outcome label; the HTTP status is shown separately.
pub fn outcome_label(
    outcome: Outcome,
    error_kind: Option<crate::stats_dto::ErrorKind>,
    zh: bool,
) -> String {
    let key = match (outcome, error_kind) {
        (Outcome::Completed, _) => ("已完成", "Completed"),
        (Outcome::Failed, Some(crate::stats_dto::ErrorKind::UpstreamHttp)) => {
            ("上游拒绝", "Upstream rejected")
        }
        (Outcome::Failed, Some(crate::stats_dto::ErrorKind::SseError)) => {
            ("流内错误", "Stream error")
        }
        (Outcome::Failed, Some(crate::stats_dto::ErrorKind::Connect)) => {
            ("连接失败", "Connect failed")
        }
        (Outcome::Failed, Some(crate::stats_dto::ErrorKind::MissingKey)) => {
            ("缺少密钥", "Missing key")
        }
        (Outcome::Failed, _) => ("失败", "Failed"),
        (Outcome::Cancelled, _) => ("已取消", "Cancelled"),
        (Outcome::Interrupted, _) => ("被中断", "Interrupted"),
        (Outcome::Rejected, Some(crate::stats_dto::ErrorKind::MissingKey)) => {
            ("未发上游", "Not sent")
        }
        (Outcome::Rejected, _) => ("本地拒绝", "Rejected"),
        (Outcome::InFlight, _) => ("进行中", "In flight"),
        (Outcome::Recorded, _) => ("已记录", "Recorded"),
    };
    if zh { key.0 } else { key.1 }.to_owned()
}

pub fn usage_label(status: UsageStatus, zh: bool) -> (String, Tag) {
    let (text, tag) = match status {
        UsageStatus::Complete => (if zh { "完整" } else { "Complete" }, Tag::success()),
        UsageStatus::Partial => (if zh { "部分" } else { "Partial" }, Tag::warning()),
        UsageStatus::Missing => (if zh { "缺失" } else { "Missing" }, Tag::secondary()),
        UsageStatus::Invalid => (if zh { "无效" } else { "Invalid" }, Tag::danger()),
        UsageStatus::NotApplicable => (
            if zh { "Token 不适用" } else { "Tokens N/A" },
            Tag::secondary(),
        ),
    };
    (
        text.to_owned(),
        tag.small()
            .text_size(px(crate::theme::font::TAG))
            .h(px(20.))
            .px(px(5.)),
    )
}

pub fn cost_label(status: CostStatus, zh: bool) -> String {
    match status {
        CostStatus::Priced => String::new(),
        // No price configured: the amount is a confirmed zero, not "unknown".
        CostStatus::DefaultZero => if zh {
            "未配置价格，按 0 计算"
        } else {
            "No price configured · counted as 0"
        }
        .to_owned(),
        CostStatus::Pending => if zh { "结算中" } else { "Settling" }.to_owned(),
        CostStatus::LegacyUnknown => if zh {
            "历史金额（来源不明）"
        } else {
            "Legacy amount"
        }
        .to_owned(),
        CostStatus::Unpriced => if zh { "未定价" } else { "Unpriced" }.to_owned(),
        CostStatus::UsageIncomplete => if zh {
            "用量不全"
        } else {
            "Usage incomplete"
        }
        .to_owned(),
        CostStatus::UnsupportedBreakdown => if zh {
            "缓存计费数据不足"
        } else {
            "Cache pricing data missing"
        }
        .to_owned(),
        CostStatus::NotApplicable => if zh { "未发上游" } else { "Not sent" }.to_owned(),
        CostStatus::Overflow => if zh {
            "金额溢出"
        } else {
            "Amount overflow"
        }
        .to_owned(),
    }
}

pub fn cost_hint(status: CostStatus, zh: bool) -> String {
    if status == CostStatus::UnsupportedBreakdown {
        if zh {
            "本次价格快照按普通输入、缓存读取或缓存写入分别计费，但本次调用的用量拆分或缓存单价不足/不兼容，无法可靠估算费用。这不代表请求失败，也不代表费用为 0。修改当前价格不会重算历史明细。"
        } else {
            "The price snapshot bills ordinary input and cache reads/writes separately, but the usage breakdown or cache rates are missing/incompatible. Cost cannot be estimated reliably. This does not mean the request failed or was free. Editing current prices does not recalculate history."
        }.into()
    } else {
        cost_label(status, zh)
    }
}

/// Text roles from Figma56:8/57:8. Keep names and localized explanations in
/// Noto; only the explicitly Latin/number roles use the embedded Inter faces.
#[derive(Clone, Copy)]
enum CellFont {
    Header,
    Body,
    Timestamp,
    Latency,
    GroupName,
    GroupCount,
    Ratio,
    TokenTotal,
    Breakdown,
}

fn table_label(text: impl Into<SharedString>, role: CellFont, color: u32, cx: &App) -> Label {
    let (size, leading, weight, latin) = match role {
        CellFont::Header => (11., 16., FontWeight::MEDIUM, false),
        CellFont::Body => (12., 17., FontWeight::MEDIUM, false),
        CellFont::Timestamp => (11., 16., FontWeight::MEDIUM, true),
        CellFont::Latency => (12., 17., FontWeight::NORMAL, false),
        CellFont::GroupName => (13., 18., FontWeight::MEDIUM, false),
        CellFont::GroupCount => (12., 17., FontWeight::SEMIBOLD, true),
        CellFont::Ratio => (12., 17., FontWeight::MEDIUM, true),
        CellFont::TokenTotal => (15., 21., FontWeight::BOLD, true),
        CellFont::Breakdown => (11., 16., FontWeight::NORMAL, false),
    };
    let label = Label::new(text)
        .text_size(px(size))
        .line_height(px(leading))
        .font_weight(weight)
        .text_color(rgb(color));
    if latin {
        crate::studio_fonts::latin(label, cx)
    } else {
        label
    }
}

fn header_cell(title: SharedString, cx: &App) -> Div {
    div()
        .size_full()
        .min_w_0()
        .overflow_hidden()
        .flex()
        .items_center()
        .child(
            table_label(title, CellFont::Header, crate::theme::color::TEXT_3, cx)
                .w_full()
                .text_ellipsis(),
        )
}

/// Single-line cell with an ellipsis and an optional hover tooltip.
fn cell(
    primary: String,
    hint: Option<String>,
    color: u32,
    id: (&'static str, usize),
    font: CellFont,
    cx: &App,
) -> AnyElement {
    let text = table_label(primary, font, color, cx)
        .w_full()
        .text_ellipsis();
    div()
        .id(id)
        .w_full()
        .h_full()
        .flex()
        .items_center()
        .min_w_0()
        .overflow_hidden()
        .when_some(hint, |this, hint| {
            this.tooltip(move |window, cx| {
                gpui_kit::component::tooltip::Tooltip::new(hint.clone()).build(window, cx)
            })
        })
        .child(text)
        .into_any_element()
}

/// Studio V2 keeps totals and the input/output split in their own column.
fn token_cell(item: &RequestRowDto, row: usize, zh: bool, cx: &App) -> AnyElement {
    if is_charge(item) {
        return cell(
            if zh { "Token 不适用" } else { "Tokens N/A" }.into(),
            Some(
                if zh {
                    "独立收费项"
                } else {
                    "Standalone charge"
                }
                .into(),
            ),
            crate::theme::color::TEXT_3,
            ("usage-tokens", row),
            CellFont::Body,
            cx,
        );
    }
    let hint = format!(
        "{}: {}\n{}: {}\n{}: {}",
        if zh { "总 Token" } else { "Total tokens" },
        exact(item.total_tokens),
        if zh { "输入" } else { "Input" },
        exact(item.input_tokens),
        if zh { "输出" } else { "Output" },
        exact(item.output_tokens)
    );
    div()
        .id(("usage-tokens", row))
        .w_full()
        .h_full()
        .min_w_0()
        .overflow_hidden()
        .flex()
        .flex_col()
        .justify_center()
        .gap_0()
        .tooltip(move |window, cx| {
            gpui_kit::component::tooltip::Tooltip::new(hint.clone()).build(window, cx)
        })
        .child(
            table_label(
                compact(item.total_tokens, zh),
                CellFont::TokenTotal,
                crate::theme::color::TEXT_1,
                cx,
            )
            .w_full()
            .text_ellipsis(),
        )
        .child(
            table_label(
                format!(
                    "↓ {}  ↑ {}",
                    compact(item.input_tokens, zh),
                    compact(item.output_tokens, zh)
                ),
                CellFont::Breakdown,
                crate::theme::color::TEXT_3,
                cx,
            )
            .w_full()
            .text_ellipsis(),
        )
        .into_any_element()
}

/// Ratios are calculated only from compatible, known normalized counters.
fn cache_cell(item: &RequestRowDto, row: usize, zh: bool, cx: &App) -> AnyElement {
    if is_charge(item) {
        return cell(
            "—".into(),
            None,
            crate::theme::color::TEXT_3,
            ("usage-cache", row),
            CellFont::Body,
            cx,
        );
    }
    let hit = cache_rate(item.cache_read_tokens, item.input_tokens);
    let ink = match hit {
        Some(value) if value >= 0.8 => crate::theme::color::SUCCESS,
        Some(_) => crate::theme::color::WARNING,
        None => crate::theme::color::TEXT_3,
    };
    let hint = format!(
        "{}: {}\n{}: {}\n{}: {}\n{}",
        if zh { "缓存读取" } else { "Cache read" },
        exact(item.cache_read_tokens),
        if zh { "缓存创建" } else { "Cache write" },
        exact(item.cache_write_tokens),
        if zh { "输入" } else { "Input" },
        exact(item.input_tokens),
        if zh {
            "命中率 = 缓存读 ÷ 输入；缺失或输入为 0 显示 —"
        } else {
            "Hit rate = cache read / input; missing or zero input: —"
        }
    );
    div()
        .id(("usage-cache", row))
        .w_full()
        .h_full()
        .min_w_0()
        .overflow_hidden()
        .flex()
        .items_center()
        .gap(px(8.))
        .tooltip(move |window, cx| {
            gpui_kit::component::tooltip::Tooltip::new(hint.clone()).build(window, cx)
        })
        .child(
            div()
                .w(px(44.))
                .h(px(4.))
                .flex_shrink_0()
                .rounded_full()
                .bg(rgb(crate::theme::color::BORDER))
                .overflow_hidden()
                .when_some(hit.filter(|value| *value > 0.), |bar, value| {
                    bar.child(
                        div()
                            .w(px((value * 44.) as f32))
                            .h_full()
                            .rounded_full()
                            .bg(rgb(ink)),
                    )
                }),
        )
        .child(table_label(percent(hit), CellFont::Body, ink, cx))
        .into_any_element()
}

/// Provider cell: the captured provider name plus the origin pill the design
/// puts inside this column. Both values keep their own tooltip text.
fn provider_cell(item: &RequestRowDto, row: usize, zh: bool, cx: &App) -> AnyElement {
    let hint = format!(
        "{}\nID: {}\n{}\n{}",
        item.provider_name,
        item.provider_id,
        origin_text(item.origin, zh),
        if zh {
            "请求发生时的名称快照，改名或删除供应商不会改变历史记录"
        } else {
            "Name captured at request time; renaming or deleting the provider does not change history"
        }
    );
    div()
        .id(("usage-provider", row))
        .w_full()
        .min_w_0()
        .h_full()
        .overflow_hidden()
        .flex()
        .items_center()
        .gap(px(6.))
        .tooltip(move |window, cx| {
            gpui_kit::component::tooltip::Tooltip::new(hint.clone()).build(window, cx)
        })
        .child(
            table_label(
                item.provider_name.clone(),
                CellFont::Body,
                crate::theme::color::TEXT_1,
                cx,
            )
            .flex_1()
            .min_w_0()
            .text_ellipsis(),
        )
        .when(is_tool_charge(item), |this| {
            this.child(
                crate::ui_components::tag(
                    crate::ui_components::TagKind::Secondary,
                    if zh { "工具" } else { "Tool" },
                )
                .text_size(px(crate::theme::font::TAG))
                .h(px(20.))
                .px(px(5.))
                .flex_shrink_0(),
            )
        })
        .child(
            crate::ui_components::tag(
                match item.origin {
                    RecordOrigin::WorkbuddyBuiltin => crate::ui_components::TagKind::Primary,
                    RecordOrigin::Proxy | RecordOrigin::WorkbuddyCustom => {
                        crate::ui_components::TagKind::Secondary
                    }
                },
                origin_text(item.origin, zh),
            )
            .text_size(px(crate::theme::font::TAG))
            .h(px(20.))
            .px(px(5.))
            .flex_shrink_0(),
        )
        .into_any_element()
}

/// Estimated cost cell: the amount on the first line and the WorkBuddy credit
/// on the second, preserving independent currency and credit values.
fn cost_cell(item: &RequestRowDto, row: usize, zh: bool, cx: &App) -> AnyElement {
    if item.source == Source::JsonlImport && item.origin.is_imported() {
        let credit = item
            .workbuddy_credit_nanos
            .as_deref()
            .and_then(format_credit_nanos);
        let value = credit.map_or_else(
            || {
                if zh {
                    "积分未记录"
                } else {
                    "Credits not recorded"
                }
                .to_owned()
            },
            |credit| format!("{credit} {}", if zh { "积分" } else { "credits" }),
        );
        return div()
            .id(("usage-cost", row))
            .flex()
            .flex_col()
            .justify_center()
            .h_full()
            .gap_1()
            .child(table_label(
                value,
                CellFont::Body,
                crate::theme::color::TEXT_2,
                cx,
            ))
            .child(table_label(
                if zh {
                    "日志记录值"
                } else {
                    "Logged amount"
                },
                CellFont::Breakdown,
                crate::theme::color::TEXT_3,
                cx,
            ))
            .into_any_element();
    }
    let status = item
        .cost
        .as_ref()
        .map_or(CostStatus::Unpriced, |cost| cost.status);
    let label = cost_label(status, zh);
    let (amount_text, muted) = if label.is_empty()
        || matches!(status, CostStatus::DefaultZero | CostStatus::NotApplicable)
    {
        (
            amount(
                item.cost
                    .as_ref()
                    .and_then(|cost| cost.total_cost_nanos.as_deref()),
                item.cost.as_ref().and_then(|cost| cost.currency),
            ),
            false,
        )
    } else {
        (label, true)
    };
    let credit = item
        .workbuddy_credit_nanos
        .as_deref()
        .and_then(format_credit_nanos)
        .unwrap_or_else(|| "—".into());
    let cost_note = if muted {
        cost_hint(status, zh)
    } else if zh {
        "估算值，不是实际账单".to_owned()
    } else {
        "Estimate, not a real invoice".to_owned()
    };
    let hint = format!(
        "{}\n{} {}\n{}",
        cost_note,
        if zh { "点数" } else { "Credits" },
        credit,
        if zh {
            "点数来自日志记录值，不代表账号余额"
        } else {
            "Credits are logged values, not the account balance"
        }
    );
    div()
        .id(("usage-cost", row))
        .w_full()
        .min_w_0()
        .h_full()
        .overflow_hidden()
        .flex()
        .flex_col()
        .justify_center()
        .gap(px(2.))
        .whitespace_nowrap()
        .aria_label(format!("{amount_text} · {credit}"))
        .tooltip(move |window, cx| {
            gpui_kit::component::tooltip::Tooltip::new(hint.clone()).build(window, cx)
        })
        .child(
            table_label(
                amount_text,
                CellFont::Body,
                if muted {
                    crate::theme::color::TEXT_3
                } else {
                    crate::theme::color::TEXT_2
                },
                cx,
            )
            .w_full()
            .min_w_0()
            .text_ellipsis(),
        )
        .child(
            table_label(
                format!("{} {credit}", if zh { "点数" } else { "Credits" }),
                CellFont::Breakdown,
                crate::theme::color::TEXT_3,
                cx,
            )
            .w_full()
            .min_w_0()
            .text_ellipsis(),
        )
        .into_any_element()
}

/// Studio V2's eight visible columns. Request IDs remain in the time tooltip
/// and detail dialog; generation rate remains in the latency tooltip/dialog.
pub const REQUEST_COLUMNS: usize = 8;
const PREFERRED_WIDTHS: [f32; REQUEST_COLUMNS] = [110., 168., 156., 180., 126., 112., 80., 144.];
const MAX_AUTO_WIDTHS: [f32; REQUEST_COLUMNS] = [110., 240., 320., 228., 145., 148., 96., 160.];
const FLEX_WEIGHTS: [f32; REQUEST_COLUMNS] = [0., 3., 4., 1., 0.5, 0.25, 0.1, 0.4];
/// Preserve readable cells at the minimum window size; horizontal scrolling
/// and explicit first/last-column buttons expose the remaining columns.
const DRAG_FLOORS: [f32; REQUEST_COLUMNS] = [110., 110., 100., 140., 114., 96., 74., 144.];

/// Fit automatic columns into the measured host. Explicit user widths win;
/// when minimum readable widths cannot fit, keep horizontal scrolling.
fn fitted_widths(
    available: f32,
    custom: &[Option<f32>; REQUEST_COLUMNS],
) -> [f32; REQUEST_COLUMNS] {
    let mut widths = PREFERRED_WIDTHS;
    for i in 1..REQUEST_COLUMNS {
        if let Some(width) = custom[i] {
            widths[i] = width.max(DRAG_FLOORS[i]);
        }
    }
    // Between the preferred sizes and the readable floors, shrink unlocked
    // columns in proportion to their available room. Keep explicit user sizes.
    let deficit = (widths.iter().sum::<f32>() - available).max(0.);
    if deficit > 0. {
        let capacity: f32 = (1..REQUEST_COLUMNS)
            .filter(|&i| custom[i].is_none())
            .map(|i| (widths[i] - DRAG_FLOORS[i]).max(0.))
            .sum();
        if capacity > 0. {
            let shrink = deficit.min(capacity);
            for i in 1..REQUEST_COLUMNS {
                if custom[i].is_none() {
                    let room = (widths[i] - DRAG_FLOORS[i]).max(0.);
                    widths[i] -= shrink * room / capacity;
                }
            }
        }
    }
    let mut remaining = (available - widths.iter().sum::<f32>()).max(0.);
    for _ in 0..REQUEST_COLUMNS {
        let weight: f32 = (1..REQUEST_COLUMNS)
            .filter(|&i| custom[i].is_none() && widths[i] < MAX_AUTO_WIDTHS[i])
            .map(|i| FLEX_WEIGHTS[i])
            .sum();
        if remaining < 0.01 || weight == 0. {
            break;
        }
        let budget = remaining;
        for i in 1..REQUEST_COLUMNS {
            if custom[i].is_none() && widths[i] < MAX_AUTO_WIDTHS[i] {
                let extra = (budget * FLEX_WEIGHTS[i] / weight).min(MAX_AUTO_WIDTHS[i] - widths[i]);
                widths[i] += extra;
                remaining -= extra;
            }
        }
    }
    // Once numeric columns reach their useful widths, share very wide-window
    // surplus between unlocked name columns rather than stretching Token.
    let name_weight: f32 = [1, 2]
        .into_iter()
        .filter(|&i| custom[i].is_none())
        .map(|i| FLEX_WEIGHTS[i])
        .sum();
    if remaining > 0. && name_weight > 0. {
        for i in [1, 2] {
            if custom[i].is_none() {
                widths[i] += remaining * FLEX_WEIGHTS[i] / name_weight;
            }
        }
    }
    widths
}

pub struct UsageRows {
    pub owner: WeakEntity<Statistics>,
    pub rows: Vec<RequestRowDto>,
    pub zh: bool,
    pub loading: bool,
    pub available_width: f32,
    /// Only dragged columns are overridden; other columns continue to fit.
    pub custom_widths: [Option<f32>; REQUEST_COLUMNS],
}

impl UsageRows {
    pub fn resize(&mut self, widths: &[Pixels]) {
        let previous = fitted_widths(self.available_width, &self.custom_widths);
        for (i, width) in widths.iter().enumerate().take(REQUEST_COLUMNS).skip(1) {
            let width = f32::from(*width);
            if (width - previous[i]).abs() > 0.5 {
                self.custom_widths[i] = Some(width);
            }
        }
    }

    fn columns(&self) -> [(&'static str, &'static str); REQUEST_COLUMNS] {
        if self.zh {
            [
                ("time", "时间"),
                ("provider", "供应商"),
                ("model", "模型"),
                ("tokens", "Token 使用量"),
                ("cache", "缓存命中"),
                ("cost", "费用 / 积分"),
                ("latency", "耗时"),
                ("status", "状态"),
            ]
        } else {
            [
                ("time", "Time"),
                ("provider", "Provider"),
                ("model", "Model"),
                ("tokens", "Tokens"),
                ("cache", "Cache hit"),
                ("cost", "Cost / credits"),
                ("latency", "Latency"),
                ("status", "Status"),
            ]
        }
    }
}

impl TableDelegate for UsageRows {
    fn columns_count(&self, _: &App) -> usize {
        self.columns().len()
    }

    fn rows_count(&self, _: &App) -> usize {
        self.rows.len()
    }

    fn column(&self, index: usize, _: &App) -> Column {
        let (id, title) = self.columns()[index];
        let width = fitted_widths(self.available_width, &self.custom_widths)[index];
        Column::new(id, title)
            .width(px(width))
            .min_width(px(DRAG_FLOORS[index]))
            .resizable(index != 0)
            .selectable(false)
    }

    fn render_th(
        &mut self,
        index: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        // Kit still owns header clicks, sorting, resizing and accessibility.
        header_cell(self.column(index, cx).name, cx)
    }

    fn render_tr(
        &mut self,
        row: usize,
        _: &mut Window,
        _cx: &mut Context<TableState<Self>>,
    ) -> Stateful<Div> {
        let owner = self.owner.clone();
        div()
            .id(("usage-row", row))
            .accessibility_id(format!("usage-row-{row}"))
            .bg(rgb(if row.is_multiple_of(2) {
                0xffffff
            } else {
                crate::theme::color::BG_NOTICE
            }))
            .on_click(move |_, window, cx| {
                cx.stop_propagation();
                let _ = owner.update(cx, |this, cx| this.open_detail(row, window, cx));
            })
    }

    fn render_td(
        &mut self,
        row: usize,
        col: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let zh = self.zh;
        let item = &self.rows[row];
        let content = match col {
            0 => cell(
                timestamp(item.started_at_ms, zh),
                Some(format!(
                    "{}{}",
                    if item.source == Source::ConnectionTest {
                        if zh {
                            "测试连接 · "
                        } else {
                            "Connection test · "
                        }
                    } else {
                        ""
                    },
                    item.request_id
                )),
                crate::theme::color::TEXT_2,
                ("usage-time", row),
                CellFont::Timestamp,
                cx,
            ),
            1 => provider_cell(item, row, zh, cx),
            2 => cell(
                model_text(item, zh),
                Some(format!(
                    "{}\n{}\n{}{}",
                    item.provider_name,
                    route_text(item, zh),
                    if is_charge(item) {
                        if zh { "收费项 " } else { "Charge " }
                    } else if zh {
                        "请求模型 "
                    } else {
                        "requested "
                    },
                    requested_text(item, zh)
                )),
                crate::theme::color::TEXT_2,
                ("usage-model", row),
                CellFont::Body,
                cx,
            ),
            3 => token_cell(item, row, zh, cx),
            4 => cache_cell(item, row, zh, cx),
            5 => cost_cell(item, row, zh, cx),
            6 => cell(
                duration(item.duration_ms),
                Some(format!(
                    "{} {}\n{} {}\n{} {}\ntok/s {}\n{}",
                    if zh { "总耗时" } else { "Duration" },
                    duration(item.duration_ms),
                    if zh { "首内容" } else { "First content" },
                    duration(item.first_content_ms),
                    "TTFB",
                    duration(item.ttfb_ms),
                    rate_text(item),
                    rate_hint(item.stream, zh)
                )),
                crate::theme::color::TEXT_2,
                ("usage-latency", row),
                CellFont::Latency,
                cx,
            ),
            _ => {
                let (text, _) = usage_label(item.usage_status, zh);
                // An imported row reads as "recorded": it has no HTTP result to
                // report and must not borrow one.
                let outcome = status_text(item.recorded, item.outcome, zh);
                let http = http_text(item.http_status);
                cell(
                    format!("{http} · {outcome}"),
                    Some(format!(
                        "HTTP {http}\n{outcome}\n{} {text}",
                        if zh { "用量" } else { "Usage" }
                    )),
                    match item.outcome {
                        Outcome::Completed => crate::theme::color::SUCCESS,
                        Outcome::InFlight | Outcome::Recorded => crate::theme::color::TEXT_3,
                        _ => 0xa4424a,
                    },
                    ("usage-status", row),
                    CellFont::Body,
                    cx,
                )
            }
        };
        // Kit cells include horizontal padding inside the declared column
        // width. Clamp delegate content to that inner width so labels cannot
        // paint into the gutter or appear joined to the next column.
        let padding = gpui_kit::component::Size::Size(px(crate::theme::size::TABLE_ROW_HEIGHT))
            .table_cell_padding();
        let inner_width = (fitted_widths(self.available_width, &self.custom_widths)[col]
            - f32::from(padding.left + padding.right))
        .max(0.);
        div()
            .w(px(inner_width))
            .h_full()
            .min_w_0()
            .overflow_hidden()
            .child(content)
    }

    fn render_empty(
        &mut self,
        _: &mut Window,
        _: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let (title, hint) = if self.loading {
            (
                if self.zh {
                    "正在加载…"
                } else {
                    "Loading…"
                },
                if self.zh {
                    "正在读取本机统计数据库"
                } else {
                    "Reading the local statistics database"
                },
            )
        } else {
            (
                if self.zh {
                    "没有符合条件的请求"
                } else {
                    "No requests match"
                },
                if self.zh {
                    "调整筛选条件或时间范围后再试"
                } else {
                    "Change the filters or the time range"
                },
            )
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_1()
            .child(strong(title))
            .child(muted(hint))
    }
}

/// Provider/model summary table.
pub struct GroupRows {
    pub rows: Vec<BreakdownItemDto>,
    pub zh: bool,
    pub group_by: crate::stats_dto::GroupBy,
    pub loading: bool,
}

impl TableDelegate for GroupRows {
    fn columns_count(&self, _: &App) -> usize {
        6
    }

    fn rows_count(&self, _: &App) -> usize {
        self.rows.len()
    }

    fn column(&self, index: usize, _: &App) -> Column {
        let zh = self.zh;
        let (id, title, width) = match index {
            0 => (
                "key",
                if self.group_by == crate::stats_dto::GroupBy::Provider {
                    if zh { "供应商" } else { "Provider" }
                } else if zh {
                    "模型"
                } else {
                    "Model"
                },
                if zh { 258. } else { 244. },
            ),
            1 => ("requests", if zh { "请求数" } else { "Requests" }, 110.),
            2 => (
                "tokens",
                if zh {
                    "输入 / 输出"
                } else {
                    "Input / Output"
                },
                if zh { 254. } else { 235. },
            ),
            3 => (
                "cache",
                if zh {
                    "缓存命中率"
                } else {
                    "Cache hit rate"
                },
                if zh { 210. } else { 180. },
            ),
            4 => (
                "cost",
                if zh { "估算费用" } else { "Est. cost" },
                if zh { 149. } else { 140. },
            ),
            _ => (
                "coverage",
                if zh { "用量覆盖" } else { "Usage coverage" },
                if zh { 104. } else { 150. },
            ),
        };
        Column::new(id, title)
            .width(px(width))
            .min_width(px(width))
            .resizable(false)
            .selectable(false)
    }

    fn render_th(
        &mut self,
        index: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        header_cell(self.column(index, cx).name, cx)
    }

    fn render_tr(
        &mut self,
        row: usize,
        _: &mut Window,
        _: &mut Context<TableState<Self>>,
    ) -> Stateful<Div> {
        div()
            .id(("group-row", row))
            .bg(rgb(if row.is_multiple_of(2) {
                0xffffff
            } else {
                0xf4f6ff
            }))
    }

    fn render_td(
        &mut self,
        row: usize,
        col: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let zh = self.zh;
        let item = &self.rows[row];
        let summary = &item.summary;
        match col {
            0 => cell(
                model_label(&item.label, zh),
                Some(format!(
                    "{}\n{}",
                    model_label(&item.key, zh),
                    item.provider_count
                )),
                crate::theme::color::TEXT_1,
                ("group-key", row),
                CellFont::GroupName,
                cx,
            ),
            1 => cell(
                summary.request_count.to_string(),
                Some(format!(
                    "{} {}\n{} {}\n{} {}",
                    if zh { "成功" } else { "Completed" },
                    summary.completed_count,
                    if zh { "失败" } else { "Failed" },
                    summary.failed_count,
                    if zh { "取消" } else { "Cancelled" },
                    summary.cancelled_count
                )),
                crate::theme::color::TEXT_2,
                ("group-requests", row),
                CellFont::GroupCount,
                cx,
            ),
            2 => cell(
                format!(
                    "↓{} ↑{}",
                    compact(summary.input_tokens, zh),
                    compact(summary.output_tokens, zh)
                ),
                Some(format!(
                    "{} {}\n{} {}\nΣ {}",
                    if zh { "输入" } else { "Input" },
                    exact(summary.input_tokens),
                    if zh { "输出" } else { "Output" },
                    exact(summary.output_tokens),
                    exact(summary.total_tokens)
                )),
                crate::theme::color::TEXT_2,
                ("group-tokens", row),
                CellFont::Body,
                cx,
            ),
            3 => cell(
                percent(summary.cache_hit_rate),
                Some(format!(
                    "{} {}/{}",
                    if zh {
                        "已知缓存读"
                    } else {
                        "known cache read"
                    },
                    summary.cache_read_known_count,
                    summary.request_count
                )),
                crate::theme::palette().dark,
                ("group-cache", row),
                CellFont::Ratio,
                cx,
            ),
            4 => {
                if summary.proxy_record_count == 0
                    && let Some(credit) = summary
                        .credit_nanos
                        .as_deref()
                        .and_then(format_credit_nanos)
                {
                    return cell(
                        format!("{credit} {}", if zh { "积分" } else { "credits" }),
                        Some(
                            if zh {
                                "日志记录值"
                            } else {
                                "Logged amount"
                            }
                            .into(),
                        ),
                        crate::theme::color::TEXT_2,
                        ("group-cost", row),
                        CellFont::Body,
                        cx,
                    );
                }
                let text = if summary.costs.is_empty() {
                    if summary.unpriced_request_count > 0 {
                        if zh { "未定价" } else { "Unpriced" }.to_owned()
                    } else {
                        "—".into()
                    }
                } else {
                    summary
                        .costs
                        .iter()
                        .map(|cost| amount(Some(&cost.total_cost_nanos), Some(cost.currency)))
                        .collect::<Vec<_>>()
                        .join(" · ")
                };
                cell(
                    text.clone(),
                    Some(format!(
                        "{text}\n{} {}\n{} {}",
                        if zh {
                            "已定价请求"
                        } else {
                            "Priced requests"
                        },
                        summary
                            .costs
                            .iter()
                            .map(|c| c.priced_request_count)
                            .sum::<i64>(),
                        if zh { "未定价" } else { "Unpriced" },
                        summary.unpriced_request_count
                    )),
                    if summary.costs.is_empty() {
                        crate::theme::color::TEXT_3
                    } else {
                        crate::theme::color::TEXT_2
                    },
                    ("group-cost", row),
                    CellFont::Body,
                    cx,
                )
            }
            _ => cell(
                percent(summary.usage_coverage),
                Some(format!(
                    "{}/{}",
                    summary.usage_complete_count, summary.usage_eligible_count
                )),
                if summary.usage_coverage.is_some() {
                    crate::theme::color::SUCCESS
                } else {
                    crate::theme::color::TEXT_3
                },
                ("group-coverage", row),
                CellFont::Ratio,
                cx,
            ),
        }
    }

    fn render_empty(
        &mut self,
        _: &mut Window,
        _: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .child(strong(if self.loading {
                if self.zh {
                    "正在加载…"
                } else {
                    "Loading…"
                }
            } else if self.zh {
                "没有符合条件的分组"
            } else {
                "No groups match"
            }))
    }
}

#[cfg(test)]
mod tests {
    // Explicit imports only: `use super::*` would pull GPUI's `#[gpui::test]`
    // attribute into scope and shadow the built-in `#[test]` macro.
    use super::{
        PREFERRED_WIDTHS, amount, cache_rate, compact, duration, fitted_widths,
        format_credit_nanos, outcome_label, output_rate, percent,
    };
    use crate::stats_dto::{Currency, ErrorKind, Outcome};

    #[test]
    fn credits_are_exact_points_rather_than_nano_unit_counts() {
        assert_eq!(format_credit_nanos("1140000000").as_deref(), Some("1.14"));
        assert_eq!(format_credit_nanos("0").as_deref(), Some("0"));
        assert_eq!(format_credit_nanos("1").as_deref(), Some("0.000000001"));
        assert_eq!(
            format_credit_nanos("18446744073709551614").as_deref(),
            Some("18446744073.709551614")
        );
        for invalid in ["", "-1", "1e9", "unknown"] {
            assert!(format_credit_nanos(invalid).is_none());
        }
    }

    #[test]
    fn compact_numbers_follow_the_interface_language() {
        assert_eq!(compact(Some(999), false), "999");
        assert_eq!(compact(Some(1_500), false), "1.50k");
        assert_eq!(compact(Some(2_500_000), false), "2.50M");
        assert_eq!(compact(Some(12_000), true), "1.20万");
        assert_eq!(compact(Some(230_000_000), true), "2.30亿");
        assert_eq!(compact(None, false), "—");
    }

    #[test]
    fn amounts_use_currency_symbols_and_never_fake_zero() {
        assert_eq!(amount(Some("1360000"), Some(Currency::Usd)), "$0.00136");
        // A whole amount is shown as a whole number, not "0.0000".
        assert_eq!(amount(Some("0"), Some(Currency::Cny)), "¥0");
        assert_eq!(amount(Some("1000000000"), Some(Currency::Cny)), "¥1");
        assert_eq!(amount(Some("10"), Some(Currency::Usd)), "$<0.0001");
        assert_eq!(amount(None, Some(Currency::Usd)), "—");
        assert_eq!(amount(Some("not-a-number"), None), "—");
        // R8-06: a total beyond `i64` keeps its exact value instead of "—".
        assert_eq!(
            amount(Some("9223372036854775808"), Some(Currency::Usd)),
            "$9223372036.854775808"
        );
        assert_eq!(amount(Some("-1000000000"), Some(Currency::Cny)), "¥-1");
    }

    #[test]
    fn durations_and_percentages_show_unknown_as_dash() {
        assert_eq!(duration(None), "—");
        assert_eq!(duration(Some(120)), "120 ms");
        assert_eq!(duration(Some(2500)), "2.50 s");
        assert_eq!(percent(None), "—");
        assert_eq!(percent(Some(0.5)), "50.0%");
    }

    #[test]
    fn cache_percentage_requires_a_known_valid_input_split() {
        assert_eq!(cache_rate(Some(800), Some(1000)), Some(0.8));
        assert_eq!(cache_rate(Some(0), Some(1000)), Some(0.));
        assert_eq!(cache_rate(Some(1000), Some(1000)), Some(1.));
        for (read, input) in [
            (None, Some(1000)),
            (Some(800), None),
            (Some(0), Some(0)),
            (Some(1001), Some(1000)),
            (Some(-1), Some(1000)),
            (Some(0), Some(-1)),
        ] {
            assert_eq!(cache_rate(read, input), None);
        }
    }

    #[test]
    fn output_rate_distinguishes_streaming_generation_from_end_to_end_time() {
        assert_eq!(
            output_rate(Some(100), Some(3000), Some(1000), true),
            Some(50.)
        );
        assert_eq!(output_rate(Some(100), Some(2000), None, false), Some(50.));
        assert_eq!(output_rate(Some(0), Some(2000), Some(1000), true), Some(0.));
        assert_eq!(output_rate(Some(100), Some(2000), Some(0), true), Some(50.));
        for (output, duration, first, stream) in [
            (None, Some(2000), Some(1000), true),
            (Some(-1), Some(2000), Some(1000), true),
            (Some(100), None, Some(1000), true),
            (Some(100), Some(0), None, false),
            (Some(100), Some(-1), None, false),
            (Some(100), Some(2000), None, true),
            (Some(100), Some(2000), Some(2000), true),
            (Some(100), Some(2000), Some(3000), true),
            (Some(100), Some(2000), Some(-1), true),
        ] {
            assert_eq!(output_rate(output, duration, first, stream), None);
        }
    }

    #[test]
    fn automatic_columns_fit_default_viewport_and_keep_numeric_columns_compact() {
        for available in [938., 1098., 1100., 1134., 1454., 2400.] {
            let widths = fitted_widths(available, &[None; super::REQUEST_COLUMNS]);
            assert!((widths.iter().sum::<f32>() - available).abs() < 0.1);
            assert_eq!(widths[0], 110.);
            // Latency and cost stay compact; the status column fits English text.
            assert!(
                widths[3] <= 228.,
                "compact Token column must not consume spare name space"
            );
            if available >= PREFERRED_WIDTHS.iter().sum() {
                assert!(widths[1] >= 168. && widths[2] >= 156.);
            }
            assert!(widths[4] <= 145. && widths[5] <= 148. && widths[6] <= 96.);
            assert!(
                widths
                    .iter()
                    .zip(super::DRAG_FLOORS)
                    .all(|(width, min)| *width >= min)
            );
        }
    }

    #[test]
    fn user_columns_are_preserved_while_remaining_columns_fit() {
        // Eight Studio V2 columns; model and Token widths are dragged here.
        let mut custom = [None; super::REQUEST_COLUMNS];
        custom[2] = Some(120.);
        custom[3] = Some(200.);
        for available in [1280., 1454.] {
            let widths = fitted_widths(available, &custom);
            assert_eq!(widths[0], 110.); // Time is always fixed.
            assert_eq!(widths[2], 120.);
            assert_eq!(widths[3], 200.);
            // If both names were manually sized, unused space is preferable
            // to stretching compact numeric columns beyond their auto caps.
            let minimum = PREFERRED_WIDTHS.iter().sum::<f32>()
                + (120. - PREFERRED_WIDTHS[2])
                + (200. - PREFERRED_WIDTHS[3]);
            assert!(widths.iter().sum::<f32>() <= available.max(minimum) + 0.1);
            assert!(widths[4] >= PREFERRED_WIDTHS[4]);
        }
    }

    #[test]
    fn narrow_windows_scroll_instead_of_hiding_or_crushing_columns() {
        let widths = fitted_widths(800., &[None; super::REQUEST_COLUMNS]);
        assert_eq!(widths, super::DRAG_FLOORS);
        assert!(widths.iter().sum::<f32>() > 800.);
    }

    #[test]
    fn outcome_labels_distinguish_local_rejection_from_upstream_failure() {
        assert_eq!(outcome_label(Outcome::Completed, None, false), "Completed");
        assert_eq!(
            outcome_label(Outcome::Failed, Some(ErrorKind::UpstreamHttp), false),
            "Upstream rejected"
        );
        assert_eq!(
            outcome_label(Outcome::Rejected, Some(ErrorKind::MissingKey), false),
            "Not sent"
        );
        assert_eq!(outcome_label(Outcome::Cancelled, None, false), "Cancelled");
    }
}

#[cfg(test)]
mod source_column_tests {
    use super::{amount, compact, exact, http_text, origin_text, percent, status_text};
    use crate::stats_dto::{Currency, Outcome, RecordOrigin};

    #[test]
    fn model_source_labels_match_the_builtin_and_custom_filter_partition() {
        // Capture provenance must not introduce a third model-source category.
        let zh = [
            RecordOrigin::Proxy,
            RecordOrigin::WorkbuddyBuiltin,
            RecordOrigin::WorkbuddyCustom,
        ]
        .map(|origin| origin_text(origin, true));
        let en = [
            RecordOrigin::Proxy,
            RecordOrigin::WorkbuddyBuiltin,
            RecordOrigin::WorkbuddyCustom,
        ]
        .map(|origin| origin_text(origin, false));
        assert_eq!(zh, ["自定义", "内置", "自定义"]);
        assert_eq!(en, ["Custom", "Built-in", "Custom"]);
        assert_ne!(zh[0], zh[1]);
        assert_ne!(zh[1], zh[2]);
        assert_eq!(zh[0], zh[2]);
        assert_eq!(en[0], en[2]);
    }

    #[test]
    fn an_absent_http_status_is_a_dash_and_never_zero_or_200() {
        // AC069: inventing a 0 or a default 200 would report a result that was
        // never observed.
        assert_eq!(http_text(None), "—");
        assert_ne!(http_text(None), "0");
        assert_ne!(http_text(None), "200");
        assert_eq!(http_text(Some(503)), "503");
    }

    #[test]
    fn an_imported_row_reads_as_recorded_and_not_as_an_http_result() {
        // The log says the message exists; it does not say a request succeeded.
        assert_eq!(status_text(true, Outcome::Completed, true), "已记录");
        assert_eq!(status_text(true, Outcome::Completed, false), "Recorded");
        assert_eq!(status_text(false, Outcome::Completed, true), "完成");
        assert_eq!(status_text(false, Outcome::Failed, false), "Failed");
        // A recorded row never borrows the completed wording.
        assert_ne!(
            status_text(true, Outcome::Completed, true),
            status_text(false, Outcome::Completed, true)
        );
    }

    #[test]
    fn missing_rates_and_amounts_are_dashes_rather_than_zeros() {
        assert_eq!(compact(None, true), "—");
        assert_eq!(exact(None), "—");
        assert_eq!(amount(None, Some(Currency::Usd)), "—");
        assert_eq!(percent(None), "—");
        // A measured zero is still a zero.
        assert_eq!(compact(Some(0), true), "0");
        assert_eq!(exact(Some(0)), "0");
        assert_eq!(percent(Some(0.0)), "0.0%");
    }
}
