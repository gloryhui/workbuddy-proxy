//! Table delegates for the statistics page: the request log plus the provider
//! and model summaries. All rows are already server-aggregated; the table only
//! renders the current page and never computes statistics locally.
use crate::{
    statistics::Statistics,
    stats_dto::{
        BreakdownItemDto, CostStatus, Currency, Outcome, RequestRowDto, Source, UsageStatus,
    },
};
use chrono::{Local, TimeZone as _};
use gpui_kit::component::{
    ActiveTheme, Sizable,
    label::Label,
    table::{Column, TableDelegate, TableState},
    tag::Tag,
};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

pub fn muted(text: impl Into<SharedString>) -> Label {
    Label::new(text)
        .text_size(px(11.))
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
    };
    if zh { key.0 } else { key.1 }.to_owned()
}

pub fn usage_label(status: UsageStatus, zh: bool) -> (String, Tag) {
    let (text, tag) = match status {
        UsageStatus::Complete => (if zh { "完整" } else { "Complete" }, Tag::success()),
        UsageStatus::Partial => (if zh { "部分" } else { "Partial" }, Tag::warning()),
        UsageStatus::Missing => (if zh { "缺失" } else { "Missing" }, Tag::secondary()),
        UsageStatus::Invalid => (if zh { "无效" } else { "Invalid" }, Tag::danger()),
        UsageStatus::NotApplicable => (if zh { "不适用" } else { "N/A" }, Tag::secondary()),
    };
    (text.to_owned(), tag.small())
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
            "拆分不支持"
        } else {
            "Split unsupported"
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

/// Single-line cell with an ellipsis and an optional hover tooltip.
fn cell(
    primary: String,
    hint: Option<String>,
    color: u32,
    id: (&'static str, usize),
) -> AnyElement {
    let text = Label::new(primary)
        .text_size(px(12.))
        .text_color(rgb(color))
        .w_full()
        .text_ellipsis();
    div()
        .id(id)
        .w_full()
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

pub struct UsageRows {
    pub owner: WeakEntity<Statistics>,
    pub rows: Vec<RequestRowDto>,
    pub zh: bool,
    pub loading: bool,
}

impl UsageRows {
    fn columns(&self) -> [(&'static str, &'static str, f32); 7] {
        if self.zh {
            [
                ("time", "时间", 110.),
                ("model", "供应商 / 模型", 240.),
                ("request", "请求标识", 130.),
                ("tokens", "Token（↓输入 ↑输出 R缓存读 Σ总）", 260.),
                ("cost", "估算费用", 130.),
                ("latency", "性能", 110.),
                ("status", "状态", 140.),
            ]
        } else {
            [
                ("time", "Time", 110.),
                ("model", "Provider / Model", 240.),
                ("request", "Request", 130.),
                ("tokens", "Tokens (↓in ↑out Rread Σtotal)", 260.),
                ("cost", "Est. cost", 130.),
                ("latency", "Latency", 110.),
                ("status", "Status", 140.),
            ]
        }
    }
}

impl TableDelegate for UsageRows {
    fn columns_count(&self, _: &App) -> usize {
        7
    }

    fn rows_count(&self, _: &App) -> usize {
        self.rows.len()
    }

    fn column(&self, index: usize, _: &App) -> Column {
        let (id, title, width) = self.columns()[index];
        Column::new(id, title)
            .width(px(width))
            .min_width(px(width))
            .resizable(false)
            .selectable(false)
    }

    fn render_tr(
        &mut self,
        row: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> Stateful<Div> {
        let owner = self.owner.clone();
        div()
            .id(("usage-row", row))
            .accessibility_id(format!("usage-row-{row}"))
            .bg(cx.theme().background)
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
        _: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let zh = self.zh;
        let item = &self.rows[row];
        match col {
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
                0x19334d,
                ("usage-time", row),
            ),
            1 => cell(
                item.upstream_model.clone(),
                Some(format!(
                    "{}\n{}\n{}{}",
                    item.provider_name,
                    item.route_name,
                    if zh { "请求模型 " } else { "requested " },
                    item.requested_model
                )),
                0x19334d,
                ("usage-model", row),
            ),
            2 => cell(
                if item.request_id.len() > 12 {
                    format!("{}…", &item.request_id[..12])
                } else {
                    item.request_id.clone()
                },
                Some(format!(
                    "{}\nHTTP {}\n{}",
                    item.request_id,
                    item.http_status
                        .map_or("—".into(), |status| status.to_string()),
                    if zh {
                        "点击查看详情"
                    } else {
                        "Click for details"
                    }
                )),
                0x6b7c90,
                ("usage-request", row),
            ),
            3 => cell(
                format!(
                    "↓{} ↑{} R{} Σ{}",
                    compact(item.input_tokens, zh),
                    compact(item.output_tokens, zh),
                    compact(item.cache_read_tokens, zh),
                    compact(item.total_tokens, zh)
                ),
                Some(format!(
                    "{} {}\n{} {}\n{} {}\n{} {}",
                    if zh { "输入" } else { "Input" },
                    exact(item.input_tokens),
                    if zh { "输出" } else { "Output" },
                    exact(item.output_tokens),
                    if zh { "缓存读" } else { "Cache read" },
                    exact(item.cache_read_tokens),
                    if zh { "总 Token" } else { "Total" },
                    exact(item.total_tokens)
                )),
                0x19334d,
                ("usage-tokens", row),
            ),
            4 => {
                let status = item
                    .cost
                    .as_ref()
                    .map_or(CostStatus::Unpriced, |cost| cost.status);
                let label = cost_label(status, zh);
                if label.is_empty()
                    || matches!(status, CostStatus::DefaultZero | CostStatus::NotApplicable)
                {
                    let text = amount(
                        item.cost
                            .as_ref()
                            .and_then(|cost| cost.total_cost_nanos.as_deref()),
                        item.cost.as_ref().and_then(|cost| cost.currency),
                    );
                    cell(
                        text.clone(),
                        Some(format!(
                            "{}\n{}",
                            text,
                            if zh {
                                "估算值，不是实际账单"
                            } else {
                                "Estimate, not a real invoice"
                            }
                        )),
                        0x19334d,
                        ("usage-cost", row),
                    )
                } else {
                    cell(label.clone(), Some(label), 0x8a98a7, ("usage-cost", row))
                }
            }
            5 => cell(
                duration(item.duration_ms),
                Some(format!(
                    "{} {}\n{} {}\n{} {}",
                    if zh { "总耗时" } else { "Duration" },
                    duration(item.duration_ms),
                    if zh { "首内容" } else { "First content" },
                    duration(item.first_content_ms),
                    "TTFB",
                    duration(item.ttfb_ms)
                )),
                0x19334d,
                ("usage-latency", row),
            ),
            _ => {
                let (text, _) = usage_label(item.usage_status, zh);
                let outcome = outcome_label(item.outcome, item.error_kind, zh);
                let http = item
                    .http_status
                    .map_or("—".to_owned(), |status| status.to_string());
                cell(
                    format!("{http} · {outcome}"),
                    Some(format!(
                        "HTTP {http}\n{outcome}\n{} {text}",
                        if zh { "用量" } else { "Usage" }
                    )),
                    match item.outcome {
                        Outcome::Completed => 0x2f7d4f,
                        Outcome::InFlight => 0x8a98a7,
                        _ => 0xa4424a,
                    },
                    ("usage-status", row),
                )
            }
        }
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
                220.,
            ),
            1 => ("requests", if zh { "请求" } else { "Requests" }, 150.),
            2 => (
                "tokens",
                if zh {
                    "输入 / 输出"
                } else {
                    "Input / Output"
                },
                190.,
            ),
            3 => ("cache", if zh { "缓存命中" } else { "Cache hit" }, 150.),
            4 => ("cost", if zh { "估算费用" } else { "Est. cost" }, 170.),
            _ => (
                "coverage",
                if zh { "用量覆盖" } else { "Usage coverage" },
                150.,
            ),
        };
        Column::new(id, title)
            .width(px(width))
            .min_width(px(width))
            .resizable(false)
            .selectable(false)
    }

    fn render_tr(
        &mut self,
        row: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> Stateful<Div> {
        div().id(("group-row", row)).bg(cx.theme().background)
    }

    fn render_td(
        &mut self,
        row: usize,
        col: usize,
        _: &mut Window,
        _: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let zh = self.zh;
        let item = &self.rows[row];
        let summary = &item.summary;
        match col {
            0 => cell(
                item.label.clone(),
                Some(format!("{}\n{}", item.key, item.provider_count)),
                0x19334d,
                ("group-key", row),
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
                0x19334d,
                ("group-requests", row),
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
                0x19334d,
                ("group-tokens", row),
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
                0x19334d,
                ("group-cache", row),
            ),
            4 => {
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
                    text,
                    Some(format!(
                        "{} {}\n{} {}",
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
                    0x19334d,
                    ("group-cost", row),
                )
            }
            _ => cell(
                percent(summary.usage_coverage),
                Some(format!(
                    "{}/{}",
                    summary.usage_complete_count, summary.usage_eligible_count
                )),
                0x19334d,
                ("group-coverage", row),
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
    use super::{amount, compact, duration, outcome_label, percent};
    use crate::stats_dto::{Currency, ErrorKind, Outcome};

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
