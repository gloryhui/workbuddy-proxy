//! Native statistics page: filters, quality warnings, overview cards, the
//! trend chart, the request log / provider / model tables and the P1 pricing and
//! storage sections.
//!
//! Everything is server-side: the page sends one `stats_dashboard` and one
//! `stats_requests` (or `stats_breakdown`) query per filter generation and
//! discards replies whose generation is stale. It never opens the database and
//! never blocks the shared settings backend.
use crate::{
    stats_backend::{JobKey, JobKind, StatsReply, StatsScheduler},
    stats_dto::{
        BreakdownDto, ClearPreviewDto, CostStatus, Currency, DashboardDto, FilterOptionDto,
        GroupBy, HealthDto, MaintenanceDto, MaintenanceStatus, PriceRuleDto, ProviderSettingsDto,
        RequestCursor, RequestDetailDto, RequestRowDto, SettingsDto, SortMetric, UsageMode,
        WarningCode,
    },
    stats_dto::{HealthState, LossScope},
    usage_chart::{ChartMode, ChartPoint, SeriesKind, UsagePlot},
    usage_table::{self, GroupRows, UsageRows},
};
use chrono::{Local, TimeZone as _};
use gpui_kit::component::{
    Disableable, IconName, Sizable, WindowExt,
    button::{Button, ButtonVariants},
    checkbox::Checkbox,
    dialog::{DialogContent, DialogFooter},
    input::{Input, InputState},
    label::Label,
    select::{Select, SelectEvent, SelectItem, SelectState},
    table::{DataTable, TableState},
    tag::Tag,
};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    time::{Duration, Instant},
};

const BG: u32 = 0xf4f7fb;
const INK: u32 = 0x19334d;
const MUTED: u32 = 0x6b7c90;
const BORDER: u32 = 0xe2e9f1;
const OK: u32 = 0x2f8f5b;
const WARN: u32 = 0xb7791f;
const PANEL: u32 = 0xffffff;

/// Debounce applied after any filter change before a query is sent.
const DEBOUNCE: Duration = Duration::from_millis(250);

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Requests,
    Providers,
    Models,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RangePreset {
    Today,
    Week,
    Month,
    Quarter,
    /// User-typed window in this machine's timezone.
    Custom,
}

impl RangePreset {
    fn code(self) -> &'static str {
        match self {
            RangePreset::Today => "today",
            RangePreset::Week => "7d",
            RangePreset::Month => "30d",
            RangePreset::Quarter => "90d",
            RangePreset::Custom => "custom",
        }
    }

    /// Parses the stored code; unknown values keep the previous preset.
    fn parse(code: &str) -> Option<Self> {
        match code {
            "today" => Some(RangePreset::Today),
            "7d" => Some(RangePreset::Week),
            "30d" => Some(RangePreset::Month),
            "90d" => Some(RangePreset::Quarter),
            "custom" => Some(RangePreset::Custom),
            _ => None,
        }
    }

    fn title(self, zh: bool) -> &'static str {
        match (self, zh) {
            (RangePreset::Today, true) => "今天",
            (RangePreset::Today, false) => "Today",
            (RangePreset::Week, true) => "近 7 天",
            (RangePreset::Week, false) => "Last 7 days",
            (RangePreset::Month, true) => "近 30 天",
            (RangePreset::Month, false) => "Last 30 days",
            (RangePreset::Quarter, true) => "近 90 天",
            (RangePreset::Quarter, false) => "Last 90 days",
            (RangePreset::Custom, true) => "自定义",
            (RangePreset::Custom, false) => "Custom",
        }
    }
}

#[derive(Clone)]
struct Choice {
    code: String,
    title: SharedString,
}

impl SelectItem for Choice {
    type Value = String;
    fn title(&self) -> SharedString {
        self.title.clone()
    }
    fn value(&self) -> &String {
        &self.code
    }
    fn matches(&self, query: &str) -> bool {
        let query = query.trim().to_lowercase();
        self.title.to_lowercase().contains(&query) || self.code.to_lowercase().contains(&query)
    }
}

type ChoiceState = SelectState<Vec<Choice>>;

fn choice(code: impl Into<String>, title: impl Into<SharedString>) -> Choice {
    Choice {
        code: code.into(),
        title: title.into(),
    }
}

fn now_ms() -> i64 {
    chrono::Local::now().timestamp_millis()
}

/// Local midnight `days` days before today, as UTC milliseconds.
fn local_midnight_days_ago(days: i64) -> i64 {
    let today = Local::now().date_naive();
    let date = today - chrono::Duration::days(days.max(0));
    let naive = date.and_hms_opt(0, 0, 0).expect("valid midnight");
    Local
        .from_local_datetime(&naive)
        .earliest()
        .map(|value| value.timestamp_millis())
        .unwrap_or_else(|| now_ms() - days * 86_400_000)
}

fn stats_card(id: &'static str) -> Stateful<Div> {
    // Unlike the model-management `card()`, statistics cards size to their
    // content; only the chart and the table get explicit heights.
    div()
        .id(id)
        .flex()
        .flex_col()
        .w_full()
        .min_w_0()
        .bg(rgb(0xffffff))
        .border_1()
        .border_color(rgb(BORDER))
        .rounded(px(12.))
        .p_4()
        .gap_3()
}

pub struct Statistics {
    zh: bool,
    visible: bool,
    backend: StatsScheduler,
    /// Sequence of the newest read intent per kind; a reply that does not match
    /// is stale and must not touch the screen.
    dashboard_seq: u64,
    /// Identity of the newest rows read; replies carrying an older identity are
    /// dropped instead of clearing a newer pending flag.
    rows_gate: crate::stats_controller::ReplyGate,
    groups_gate: crate::stats_controller::ReplyGate,
    /// Full health snapshot; the badge reflects collection health, not the fact
    /// that a query happened to succeed.
    health: Option<crate::stats_dto::HealthDto>,
    maintenance_poll: Instant,
    /// Client filter generation; replies with another generation are dropped.
    generation: u64,
    applied: u64,
    debounce_until: Option<Instant>,
    refresh_interval: u64,
    last_refresh: Instant,
    pending_dashboard: bool,
    pending_rows: bool,
    // Filter draft.
    range: RangePreset,
    provider: String,
    model: String,
    outcome: String,
    usage: String,
    include_tests: bool,
    range_select: Entity<ChoiceState>,
    provider_select: Entity<ChoiceState>,
    model_select: Entity<ChoiceState>,
    outcome_select: Entity<ChoiceState>,
    usage_select: Entity<ChoiceState>,
    refresh_select: Entity<ChoiceState>,
    currency_select: Entity<ChoiceState>,
    // Data.
    dashboard: Option<DashboardDto>,
    rows: Vec<RequestRowDto>,
    total_rows: i64,
    as_of_id: i64,
    groups: Vec<crate::stats_dto::BreakdownItemDto>,
    total_groups: i64,
    provider_options: Vec<FilterOptionDto>,
    configured_providers: Vec<(String, String)>,
    /// Current model search text (R07 wires the input to this).
    model_query: String,
    model_options: Vec<FilterOptionDto>,
    timezone: String,
    filter_key: String,
    tab: Tab,
    page_size: i64,
    /// The window captured when the query was (re)started. Pagination is bound
    /// to the exact filter it began with, so `now` is never recomputed while
    /// walking pages.
    frozen_range: Option<(i64, i64)>,
    /// Parsed custom window, kept in sync with the two inputs.
    custom_range: (i64, i64),
    custom_from: Entity<InputState>,
    custom_to: Entity<InputState>,
    /// Request-log pagination: the cursor of the current page, the one the core
    /// offered next, and the history needed to walk back (1 -> 2 -> 3 -> 2 -> 1).
    rows_cursors: crate::stats_controller::CursorStack<RequestCursor>,
    /// The grouping listing keeps its own cursor chain: sharing the request
    /// cursors made "next" on one tab move the other.
    group_cursor_stack: Vec<Option<crate::stats_dto::BreakdownCursor>>,
    group_current_cursor: Option<crate::stats_dto::BreakdownCursor>,
    group_next_cursor: Option<crate::stats_dto::BreakdownCursor>,
    group_sort: SortMetric,
    loading_dashboard: bool,
    loading_rows: bool,
    error: Option<String>,
    last_updated: Option<Instant>,
    currency: Currency,
    visible_series: BTreeSet<SeriesKind>,
    details: Option<RequestDetailDto>,
    detail_open: bool,
    // P1.
    settings: Option<SettingsDto>,
    provider_modes: BTreeMap<String, ProviderSettingsDto>,
    prices: Vec<PriceRuleDto>,
    price_open: bool,
    /// True while the dialog is being closed because a save was confirmed.
    price_closing: bool,
    /// The price being created or edited. It owns its provider and model, so a
    /// draft never touches the report filters.
    price_draft: crate::stats_controller::PriceDraft,
    maintenance: Option<MaintenanceDto>,
    clear_preview: Option<ClearPreviewDto>,
    clear_token: Option<String>,
    pricing_open: bool,
    storage_open: bool,
    draft_generation: u64,
    provider_search: Entity<InputState>,
    model_search: Entity<InputState>,
    price_search: Entity<InputState>,
    provider_query: String,
    price_query: String,
    provider_after: Option<String>,
    model_after: Option<String>,
    prices_cursors: crate::stats_controller::CursorStack<String>,
    prices_page_size: i64,
    prices_loading: bool,
    storage_page: usize,
    storage_page_size: usize,
    provider_next: Option<String>,
    model_next: Option<String>,
    provider_seq: u64,
    model_seq: u64,
    prices_seq: u64,
    detail_seq: u64,
    settings_seq: u64,
    date_error: Option<String>,
    // Controls owned by P1 dialogs.
    price_model: Entity<InputState>,
    price_input: Entity<InputState>,
    price_output: Entity<InputState>,
    price_read: Entity<InputState>,
    /// Disjoint pricing needs a cache write rate; without it the mode could
    /// never be saved.
    price_write: Entity<InputState>,
    price_label: Entity<InputState>,
    price_mode: Entity<ChoiceState>,
    price_currency: Entity<ChoiceState>,
    /// Provider of the price being edited, independent of the report filter.
    price_provider: Entity<ChoiceState>,
    usage_table: Entity<TableState<UsageRows>>,
    group_table: Entity<TableState<GroupRows>>,
    _timer: Task<()>,
}

impl Statistics {
    fn t(&self, zh: &'static str, en: &'static str) -> &'static str {
        if self.zh { zh } else { en }
    }

    pub fn new(dir: PathBuf, zh: bool, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let backend = StatsScheduler::new(dir);
        let provider_search = cx.new(|cx| InputState::new(window, cx));
        let model_search = cx.new(|cx| InputState::new(window, cx));
        let price_search = cx.new(|cx| InputState::new(window, cx));
        let range_select = cx.new(|cx| {
            SelectState::new(
                [
                    RangePreset::Today,
                    RangePreset::Week,
                    RangePreset::Month,
                    RangePreset::Quarter,
                    RangePreset::Custom,
                ]
                .into_iter()
                .map(|preset| choice(preset.code(), preset.title(zh)))
                .collect::<Vec<_>>(),
                None,
                window,
                cx,
            )
        });
        range_select.update(cx, |state, cx| {
            state.set_selected_value(&RangePreset::Month.code().to_owned(), window, cx);
        });
        let custom_from = cx.new(|cx| InputState::new(window, cx).placeholder("YYYY-MM-DD HH:MM"));
        let custom_to = cx.new(|cx| InputState::new(window, cx).placeholder("YYYY-MM-DD HH:MM"));
        let provider_select = cx.new(|cx| {
            SelectState::new(
                vec![choice(
                    "",
                    if zh {
                        "全部供应商"
                    } else {
                        "All providers"
                    },
                )],
                None,
                window,
                cx,
            )
        });
        let model_select = cx.new(|cx| {
            SelectState::new(
                vec![choice("", if zh { "全部模型" } else { "All models" })],
                None,
                window,
                cx,
            )
        });
        let outcome_select = cx.new(|cx| {
            let mut items = vec![choice("", if zh { "全部状态" } else { "All states" })];
            for (code, cn, en) in [
                ("completed", "已完成", "Completed"),
                ("failed", "失败", "Failed"),
                ("cancelled", "已取消", "Cancelled"),
                ("interrupted", "被中断", "Interrupted"),
                ("rejected", "本地拒绝", "Rejected"),
                ("in_flight", "进行中", "In flight"),
            ] {
                items.push(choice(code, if zh { cn } else { en }));
            }
            SelectState::new(items, None, window, cx)
        });
        let usage_select = cx.new(|cx| {
            let mut items = vec![choice("", if zh { "全部用量" } else { "All usage" })];
            for (code, cn, en) in [
                ("complete", "完整", "Complete"),
                ("partial", "部分", "Partial"),
                ("missing", "缺失", "Missing"),
                ("invalid", "无效", "Invalid"),
                ("not_applicable", "不适用", "N/A"),
            ] {
                items.push(choice(code, if zh { cn } else { en }));
            }
            SelectState::new(items, None, window, cx)
        });
        let refresh_select = cx.new(|cx| {
            SelectState::new(
                vec![
                    choice(
                        "0",
                        if zh {
                            "自动刷新：关闭"
                        } else {
                            "Auto refresh: off"
                        },
                    ),
                    choice("10", if zh { "每 10 秒" } else { "Every 10s" }),
                    choice("30", if zh { "每 30 秒" } else { "Every 30s" }),
                    choice("60", if zh { "每 60 秒" } else { "Every 60s" }),
                ],
                None,
                window,
                cx,
            )
        });
        let currency_select = cx.new(|cx| {
            SelectState::new(
                vec![choice("USD", "USD"), choice("CNY", "CNY")],
                None,
                window,
                cx,
            )
        });
        // The dialog owns its provider select: reusing the report filter here
        // would change what the page reports on as a side effect of editing.
        let price_provider = cx.new(|cx| {
            SelectState::new(
                vec![choice(
                    "",
                    if zh {
                        "选择供应商"
                    } else {
                        "Select a provider"
                    },
                )],
                None,
                window,
                cx,
            )
        });
        let price_mode = cx.new(|cx| {
            SelectState::new(
                vec![
                    choice("flat", if zh { "统一输入价" } else { "Flat input" }),
                    choice(
                        "read_discount",
                        if zh {
                            "缓存读折扣"
                        } else {
                            "Cache read discount"
                        },
                    ),
                    choice(
                        "disjoint",
                        if zh {
                            "读/写分开"
                        } else {
                            "Read/write split"
                        },
                    ),
                ],
                None,
                window,
                cx,
            )
        });
        let price_currency = cx.new(|cx| {
            SelectState::new(
                vec![choice("USD", "USD"), choice("CNY", "CNY")],
                None,
                window,
                cx,
            )
        });
        let price_model = cx.new(|cx| InputState::new(window, cx).placeholder("model-id"));
        // Placeholders are unit examples; they read the same in both languages.
        let price_input = cx.new(|cx| InputState::new(window, cx).placeholder("0"));
        let price_output = cx.new(|cx| InputState::new(window, cx).placeholder("0"));
        let price_read = cx.new(|cx| InputState::new(window, cx).placeholder("0"));
        let price_write = cx.new(|cx| InputState::new(window, cx).placeholder("0"));
        let price_label = cx
            .new(|cx| InputState::new(window, cx).placeholder(if zh { "备注" } else { "Label" }));
        let owner = cx.entity().downgrade();
        let usage_table = cx.new(|cx| {
            TableState::new(
                UsageRows {
                    owner: owner.clone(),
                    rows: Vec::new(),
                    zh,
                    loading: false,
                },
                window,
                cx,
            )
            .row_selectable(false)
            .col_selectable(false)
            .sortable(false)
            .col_movable(false)
        });
        let group_table = cx.new(|cx| {
            TableState::new(
                GroupRows {
                    rows: Vec::new(),
                    zh,
                    group_by: GroupBy::Provider,
                    loading: false,
                },
                window,
                cx,
            )
            .row_selectable(false)
            .col_selectable(false)
            .sortable(false)
            .col_movable(false)
        });
        let timer = cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(250))
                    .await;
                if this
                    .update_in(cx, |this, window, cx| this.tick(window, cx))
                    .is_err()
                {
                    break;
                }
            }
        });
        let mut this = Self {
            zh,
            visible: false,
            backend,
            generation: 0,
            applied: 0,
            debounce_until: None,
            refresh_interval: 0,
            last_refresh: Instant::now() - Duration::from_secs(3600),
            pending_dashboard: false,
            pending_rows: false,
            range: RangePreset::Month,
            provider: String::new(),
            model: String::new(),
            outcome: String::new(),
            usage: String::new(),
            include_tests: false,
            range_select,
            provider_select,
            custom_from,
            custom_to,
            model_select,
            outcome_select,
            usage_select,
            refresh_select,
            currency_select,
            dashboard: None,
            rows: Vec::new(),
            total_rows: 0,
            as_of_id: 0,
            groups: Vec::new(),
            total_groups: 0,
            provider_options: Vec::new(),
            configured_providers: Vec::new(),
            model_query: String::new(),
            model_options: Vec::new(),
            timezone: String::new(),
            filter_key: String::new(),
            tab: Tab::Requests,
            dashboard_seq: 0,
            rows_gate: crate::stats_controller::ReplyGate::default(),
            groups_gate: crate::stats_controller::ReplyGate::default(),
            health: None,
            maintenance_poll: Instant::now(),
            page_size: 20,
            frozen_range: None,
            custom_range: (local_midnight_days_ago(6), now_ms()),
            rows_cursors: crate::stats_controller::CursorStack::default(),
            group_cursor_stack: Vec::new(),
            group_current_cursor: None,
            group_next_cursor: None,
            group_sort: SortMetric::TotalTokens,
            loading_dashboard: false,
            loading_rows: false,
            error: None,
            last_updated: None,
            currency: Currency::Usd,
            visible_series: BTreeSet::from([
                SeriesKind::Input,
                SeriesKind::Output,
                SeriesKind::CacheRead,
            ]),
            details: None,
            detail_open: false,
            settings: None,
            provider_modes: BTreeMap::new(),
            prices: Vec::new(),
            price_open: false,
            price_closing: false,
            price_draft: crate::stats_controller::PriceDraft::blank(String::new(), 0),
            maintenance: None,
            clear_preview: None,
            clear_token: None,
            pricing_open: false,
            storage_open: false,
            draft_generation: 0,
            provider_search,
            model_search,
            price_search,
            provider_query: String::new(),
            price_query: String::new(),
            provider_after: None,
            model_after: None,
            prices_cursors: Default::default(),
            prices_page_size: 20,
            prices_loading: false,
            storage_page: 0,
            storage_page_size: 20,
            provider_next: None,
            model_next: None,
            provider_seq: 0,
            model_seq: 0,
            prices_seq: 0,
            detail_seq: 0,
            settings_seq: 0,
            date_error: None,
            price_model,
            price_input,
            price_output,
            price_read,
            price_write,
            price_label,
            price_mode,
            price_currency,
            price_provider,
            usage_table,
            group_table,
            _timer: timer,
        };
        this.subscribe(window, cx);
        this.rebuild_choices(window, cx);
        this
    }

    fn subscribe(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        cx.subscribe_in(&self.refresh_select, window, |this, _, event, _, cx| {
            if let SelectEvent::Confirm(Some(code)) = event {
                this.refresh_interval = code.parse().unwrap_or(0);
                cx.notify();
            }
        })
        .detach();
        cx.subscribe_in(&self.currency_select, window, |this, _, event, _, cx| {
            if let SelectEvent::Confirm(Some(code)) = event {
                this.currency = Currency::parse(code).unwrap_or(Currency::Usd);
                cx.notify();
            }
        })
        .detach();
    }

    /// Called by the settings window when the statistics page becomes visible.
    pub fn set_configured_providers(&mut self, providers: Vec<(String, String)>) {
        if self.configured_providers != providers {
            self.configured_providers = providers;
            self.storage_page = 0;
        }
    }

    pub fn set_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        if self.visible == visible {
            return;
        }
        self.visible = visible;
        if !visible {
            self.backend.cancel_reads();
            self.rows_gate.invalidate();
            self.groups_gate.invalidate();
            self.dashboard_seq = 0;
            self.provider_seq = 0;
            self.model_seq = 0;
            self.prices_seq = 0;
            self.detail_seq = 0;
            self.settings_seq = 0;
            self.debounce_until = None;
            self.pending_dashboard = false;
            self.pending_rows = false;
            self.loading_dashboard = false;
            self.loading_rows = false;
        }
        if visible {
            // Entering the page always reloads: leaving it stops all queries.
            self.mark_dirty(cx);
            self.request_health(cx);
            self.request_settings(cx);
            self.provider_after = None;
            self.model_after = None;
            self.request_filters(cx);
            self.request_model_filters(cx);
            self.request_prices(cx);
            self.request_provider_modes(cx);
        }
    }

    pub fn set_language(&mut self, zh: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.zh == zh {
            return;
        }
        if self.price_open {
            self.collect_price_draft(cx);
        }
        self.zh = zh;
        self.rebuild_choices(window, cx);
        self.usage_table.update(cx, |table, cx| {
            table.delegate_mut().zh = zh;
            table.refresh(cx);
        });
        self.group_table.update(cx, |table, cx| {
            table.delegate_mut().zh = zh;
            table.refresh(cx);
        });
        cx.notify();
    }

    fn rebuild_choices(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let zh = self.zh;
        let ranges = [
            RangePreset::Today,
            RangePreset::Week,
            RangePreset::Month,
            RangePreset::Quarter,
            RangePreset::Custom,
        ]
        .into_iter()
        .map(|r| choice(r.code(), r.title(zh)))
        .collect();
        let outcomes = vec![
            choice("", self.t("全部状态", "All states")),
            choice("completed", self.t("已完成", "Completed")),
            choice("failed", self.t("失败", "Failed")),
            choice("cancelled", self.t("已取消", "Cancelled")),
            choice("interrupted", self.t("被中断", "Interrupted")),
            choice("rejected", self.t("本地拒绝", "Rejected")),
            choice("in_flight", self.t("进行中", "In flight")),
        ];
        let usage = vec![
            choice("", self.t("全部用量", "All usage")),
            choice("complete", self.t("完整", "Complete")),
            choice("partial", self.t("部分", "Partial")),
            choice("missing", self.t("缺失", "Missing")),
            choice("invalid", self.t("无效", "Invalid")),
            choice("not_applicable", self.t("不适用", "N/A")),
        ];
        let refresh = vec![
            choice("0", self.t("自动刷新：关闭", "Auto refresh: off")),
            choice("10", self.t("每10秒", "Every 10s")),
            choice("30", self.t("每30秒", "Every 30s")),
            choice("60", self.t("每60秒", "Every 60s")),
        ];
        let modes = vec![
            choice("flat", self.t("统一输入价", "Flat input")),
            choice("read_discount", self.t("缓存读折扣", "Cache read discount")),
            choice("disjoint", self.t("读/写分开", "Read/write split")),
        ];
        let mut providers = vec![choice("", self.t("全部供应商", "All providers"))];
        providers.extend(self.provider_options.iter().map(|p| {
            choice(
                p.key.clone(),
                if p.historical {
                    format!("{} ({})", p.label, self.t("历史配置", "historical"))
                } else if p.label.is_empty() {
                    p.key.clone()
                } else {
                    p.label.clone()
                },
            )
        }));
        if !self.provider.is_empty() && !providers.iter().any(|p| p.code == self.provider) {
            providers.push(choice(self.provider.clone(), self.provider.clone()));
        }
        let mut models = vec![choice("", self.t("全部模型", "All models"))];
        models.extend(
            self.model_options
                .iter()
                .map(|m| choice(m.key.clone(), m.key.clone())),
        );
        if !self.model.is_empty() && !models.iter().any(|p| p.code == self.model) {
            models.push(choice(self.model.clone(), self.model.clone()));
        }
        for (select, items) in [
            (&self.range_select, ranges),
            (&self.outcome_select, outcomes),
            (&self.usage_select, usage),
            (&self.refresh_select, refresh),
            (&self.price_mode, modes),
            (&self.provider_select, providers),
            (&self.model_select, models),
        ] {
            let selected = select
                .read(cx)
                .selected_value()
                .cloned()
                .or_else(|| items.first().map(|item: &Choice| item.code.clone()));
            select.update(cx, |state, cx| {
                state.set_items(items, window, cx);
                if let Some(selected) = selected {
                    state.set_selected_value(&selected, window, cx);
                }
            });
        }
        for (input, hint) in [
            (
                &self.provider_search,
                self.t("搜索供应商", "Search providers"),
            ),
            (&self.model_search, self.t("搜索模型", "Search models")),
            (&self.price_search, self.t("精确模型ID", "Exact model ID")),
        ] {
            input.update(cx, |state, cx| {
                state.set_placeholder(hint.to_owned(), window, cx)
            });
        }
        if self.currency_select.read(cx).selected_value().is_none() {
            self.currency_select.update(cx, |state, cx| {
                state.set_selected_value(&self.currency.as_str().to_owned(), window, cx)
            });
        }
        self.sync_price_providers(window, cx);
    }

    fn mark_dirty(&mut self, cx: &mut Context<Self>) {
        self.generation += 1;
        self.debounce_until = Some(Instant::now() + DEBOUNCE);
        self.reset_pagination();
        // Every reply already in flight is invalid *now*, not when the debounce
        // finally issues the next request: otherwise a stale page could land in
        // between and clear the pending flag of the newer one.
        self.rows_gate.invalidate();
        self.groups_gate.invalidate();
        // The dashboard belongs to the same filter, so its outstanding read is
        // stale the moment the filter changes: it must not land after the
        // debounce and show the previous window.
        self.dashboard_seq = 0;
        self.pending_dashboard = false;
        self.pending_rows = false;
        self.loading_dashboard = false;
        self.loading_rows = false;
        cx.notify();
    }

    /// Drops every page cursor and re-captures the time window, so the next
    /// query and its pages share one frozen filter.
    /// The grouping listing restarts whenever its grouping or sort changes: the
    /// cursors belong to the old ordering.
    fn reset_group_pagination(&mut self) {
        self.group_cursor_stack.clear();
        self.group_current_cursor = None;
        self.group_next_cursor = None;
    }

    fn reset_pagination(&mut self) {
        self.frozen_range = Some(self.compute_range());
        self.rows_cursors.reset();
        self.reset_group_pagination();
    }

    fn compute_range(&self) -> (i64, i64) {
        match self.range {
            RangePreset::Today => (local_midnight_days_ago(0), now_ms()),
            RangePreset::Week => (local_midnight_days_ago(6), now_ms()),
            RangePreset::Month => (local_midnight_days_ago(29), now_ms()),
            RangePreset::Quarter => (local_midnight_days_ago(89), now_ms()),
            // Values are validated on submit; an unparsable pair falls back to
            // today rather than silently querying an empty range.
            RangePreset::Custom => {
                let (from, to) = self.custom_range;
                (from, to)
            }
        }
    }

    fn filter(&self) -> Value {
        let (from_ms, to_ms) = self.frozen_range.unwrap_or_else(|| self.compute_range());
        let option = |value: &str| -> Value {
            if value.is_empty() {
                Value::Null
            } else {
                Value::String(value.to_owned())
            }
        };
        json!({
            "from_ms": from_ms,
            "to_ms": to_ms,
            // Empty asks the core for this machine's IANA zone.
            "timezone": "",
            "provider_id": option(&self.provider),
            "upstream_model": option(&self.model),
            "route_id": Value::Null,
            "outcome": option(&self.outcome),
            "usage_status": option(&self.usage),
            "include_tests": self.include_tests,
        })
    }

    // ---- requests ---------------------------------------------------------

    fn request_dashboard(&mut self, cx: &mut Context<Self>) {
        let tag = format!("dash-{}", self.generation);
        let request = crate::stats_backend::command(
            "stats_dashboard",
            &tag,
            json!({"filter": self.filter(), "granularity": "auto"}),
        );
        self.dashboard_seq = self.backend.submit_read(JobKey::Dashboard, tag, request);
        self.loading_dashboard = true;
        self.pending_dashboard = true;
        cx.notify();
    }

    fn request_rows(&mut self, cx: &mut Context<Self>) {
        let tag = format!("rows-{}", self.generation);
        let mut fields = json!({
            "filter": self.filter(),
            "page_size": self.page_size,
        });
        if let Some(cursor) = &self.rows_cursors.current
            && let Ok(value) = serde_json::to_value(cursor)
        {
            fields["cursor"] = value;
        }
        let request = crate::stats_backend::command("stats_requests", &tag, fields);
        // The identity is recorded before the request leaves, so a reply that
        // arrives after the filters change is recognisably stale.
        let cursor_key = self
            .rows_cursors
            .current
            .as_ref()
            .and_then(|cursor| serde_json::to_string(cursor).ok());
        let identity = self.rows_gate.issue(
            self.generation,
            crate::stats_controller::StatsTab::Rows,
            GroupBy::Model,
            SortMetric::TotalTokens,
            cursor_key,
        );
        let _ = identity;
        let seq = self.backend.submit_read(JobKey::Rows, tag, request);
        // The scheduler's own sequence is what replies carry, so the gate is
        // re-issued with it to keep one identity per outstanding read.
        self.rows_gate
            .replace_seq(crate::stats_controller::StatsTab::Rows, seq);
        self.loading_rows = true;
        self.pending_rows = true;
        cx.notify();
    }

    fn request_groups(&mut self, cx: &mut Context<Self>) {
        let tag = format!("rows-{}", self.generation);
        let group_by = match self.tab {
            Tab::Models => GroupBy::Model,
            _ => GroupBy::Provider,
        };
        let mut fields = json!({
            "filter": self.filter(),
            "group_by": group_by,
            "sort": self.group_sort,
            "limit": self.page_size,
        });
        // The grouping page must send its own cursor, or every "next" repeats
        // the first page.
        if let Some(cursor) = &self.group_current_cursor
            && let Ok(value) = serde_json::to_value(cursor)
        {
            fields["cursor"] = value;
        }
        let request = crate::stats_backend::command("stats_breakdown", &tag, fields);
        // The identity records the grouping, sort and cursor this page was
        // fetched with, so a reply for a different grouping cannot land.
        self.groups_gate.issue(
            self.generation,
            crate::stats_controller::StatsTab::Groups,
            group_by,
            self.group_sort,
            self.group_current_cursor
                .as_ref()
                .and_then(|cursor| serde_json::to_string(cursor).ok()),
        );
        let seq = self.backend.submit_read(JobKey::Groups, tag, request);
        self.groups_gate
            .replace_seq(crate::stats_controller::StatsTab::Groups, seq);
        self.loading_rows = true;
        self.pending_rows = true;
        cx.notify();
    }

    fn request_filters(&mut self, cx: &mut Context<Self>) {
        let tag = format!("filters-{}", self.generation);
        let request = crate::stats_backend::command(
            "stats_filters",
            &tag,
            json!({"kind": "provider", "query": self.provider_query, "after_key": self.provider_after, "limit": 100}),
        );
        self.provider_seq = self
            .backend
            .submit_read(JobKey::ProviderFilters, tag, request);
        let _ = cx;
    }

    /// Model options, scoped to the provider currently selected: changing the
    /// provider is a different resource and must not be collapsed away.
    fn request_model_filters(&mut self, cx: &mut Context<Self>) {
        let tag = format!("models-{}", self.generation);
        let request = crate::stats_backend::command(
            "stats_filters",
            &tag,
            json!({
                "kind": "model",
                "query": self.model_query.clone(), "after_key": self.model_after,
                "provider_id": if self.provider.is_empty() { Value::Null } else { Value::String(self.provider.clone()) },
                "limit": 100,
            }),
        );
        self.model_seq = self.backend.submit_read(
            JobKey::ModelFilters {
                provider_id: self.provider.clone(),
                query: self.model_query.clone(),
                cursor: self.model_after.clone().unwrap_or_default(),
            },
            tag,
            request,
        );
        let _ = cx;
    }

    fn request_health(&mut self, cx: &mut Context<Self>) {
        let tag = format!("health-{}", self.generation);
        let request = crate::stats_backend::command("stats_health", &tag, json!({}));
        self.backend.submit_read(JobKey::Health, tag, request);
        let _ = cx;
    }

    fn request_settings(&mut self, cx: &mut Context<Self>) {
        let tag = format!("settings-{}", self.generation);
        let request = crate::stats_backend::command("stats_settings_get", &tag, json!({}));
        self.settings_seq = self.backend.submit_read(JobKey::Settings, tag, request);
        let _ = cx;
    }

    fn request_prices(&mut self, cx: &mut Context<Self>) {
        let tag = format!("prices-{}", self.generation);
        self.prices_cursors.reset();
        self.request_price_page(tag, cx);
    }

    fn request_price_page(&mut self, tag: String, cx: &mut Context<Self>) {
        // Never leave the previous page's rows/actions under the new page
        // number, or reuse its next cursor after a failed read.
        self.prices.clear();
        self.prices_cursors.finish_page(None);
        let request = crate::stats_backend::command(
            "stats_prices_list",
            &tag,
            json!({"limit":self.prices_page_size,"after_rule_id":self.prices_cursors.current,
                   "provider_id": if self.provider.is_empty(){None}else{Some(&self.provider)},
                   "upstream_model":if self.price_query.is_empty(){None}else{Some(&self.price_query)}}),
        );
        self.prices_seq = self.backend.submit_read(JobKey::Prices, tag, request);
        self.prices_loading = true;
        cx.notify();
    }

    /// Mirrors the selectors into the fields the query builder reads.
    ///
    /// `Select` has no change callback, so the filters are synchronised on the
    /// tick instead of relying on render-time side effects; a real change marks
    /// the query dirty exactly once.
    fn sync_filter_state(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut dirty = false;
        let range = self
            .range_select
            .read(cx)
            .selected_value()
            .cloned()
            .unwrap_or_default();
        let preset = RangePreset::parse(&range).unwrap_or(self.range);
        let mut valid = true;
        if preset == RangePreset::Custom {
            match self.parsed_custom_range(cx) {
                Ok(custom) => {
                    self.date_error = None;
                    if custom != self.custom_range {
                        self.custom_range = custom;
                        dirty = true;
                    }
                }
                Err(code) => {
                    valid = false;
                    self.date_error = Some(
                        match code {
                            "reversed_date" => {
                                self.t("结束时间必须晚于开始时间", "End must be after start")
                            }
                            "range_too_long" => {
                                self.t("时间范围不能超过366天", "Range cannot exceed 366 days")
                            }
                            _ => self.t(
                                "日期格式无效：YYYY-MM-DD HH:MM",
                                "Invalid date: YYYY-MM-DD HH:MM",
                            ),
                        }
                        .to_owned(),
                    );
                    self.debounce_until = None;
                    self.rows_gate.invalidate();
                    self.groups_gate.invalidate();
                    self.dashboard_seq = 0;
                    self.pending_rows = false;
                    self.pending_dashboard = false;
                    self.loading_rows = false;
                    self.loading_dashboard = false;
                }
            }
        } else {
            self.date_error = None;
        }
        if preset != self.range && valid {
            self.range = preset;
            dirty = true;
        }
        let provider = self
            .provider_select
            .read(cx)
            .selected_value()
            .cloned()
            .unwrap_or_default();
        if provider != self.provider {
            self.provider = provider;
            self.model.clear();
            self.model_after = None;
            self.model_next = None;
            self.model_seq = 0;
            self.model_options.clear();
            self.model_select.update(cx, |state, cx| {
                state.set_items(
                    vec![choice(
                        "",
                        if self.zh {
                            "全部模型"
                        } else {
                            "All models"
                        },
                    )],
                    window,
                    cx,
                );
                state.set_selected_value(&String::new(), window, cx);
            });
            self.request_model_filters(cx);
            self.request_prices(cx);
            dirty = true;
        }
        let pq = self.provider_search.read(cx).value().trim().to_owned();
        if pq != self.provider_query {
            self.provider_query = pq;
            self.provider_after = None;
            self.request_filters(cx);
        }
        let mq = self.model_search.read(cx).value().trim().to_owned();
        if mq != self.model_query {
            self.model_query = mq;
            self.model_after = None;
            self.request_model_filters(cx);
        }
        let priceq = self.price_search.read(cx).value().trim().to_owned();
        if priceq != self.price_query {
            self.price_query = priceq;
            self.request_prices(cx);
        }
        for (select, slot) in [
            (&self.provider_select, 0u8),
            (&self.model_select, 1),
            (&self.outcome_select, 2),
            (&self.usage_select, 3),
        ] {
            let value = select
                .read(cx)
                .selected_value()
                .cloned()
                .unwrap_or_default();
            let target = match slot {
                0 => &mut self.provider,
                1 => &mut self.model,
                2 => &mut self.outcome,
                _ => &mut self.usage,
            };
            if *target != value {
                *target = value;
                dirty = true;
            }
        }
        if dirty && valid {
            self.mark_dirty(cx);
        }
        cx.notify();
    }

    /// Parses the two custom inputs; an unparsable end keeps the last good one
    /// so the query never silently widens or empties.
    fn parsed_custom_range(&self, cx: &App) -> Result<(i64, i64), &'static str> {
        crate::stats_controller::validate_range(
            crate::usage_table::parse_local_time(&self.custom_from.read(cx).value()),
            crate::usage_table::parse_local_time(&self.custom_to.read(cx).value()),
        )
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        if self.date_error.is_some() {
            return;
        }
        self.reset_pagination();
        self.request_dashboard(cx);
        match self.tab {
            Tab::Requests => self.request_rows(cx),
            _ => self.request_groups(cx),
        }
        self.last_refresh = Instant::now();
        self.request_health(cx);
    }

    pub fn tick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Hand pending intents to the worker. A full channel just leaves them
        // queued; nothing is dropped and write intents keep their order.
        let foreground = crate::stats_controller::may_refresh(
            self.visible,
            window.is_window_active(),
            window.has_active_dialog(cx),
            self.date_error.is_none(),
        );
        if !self.visible {
            self.backend.cancel_reads();
        }
        if self.visible || self.backend.has_pending_writes() {
            self.backend.pump_with_reads(self.visible);
        }
        if self.visible && !window.has_active_dialog(cx) {
            self.sync_filter_state(window, cx);
        }
        // Leaving the page cancels the debounce instead of firing a query the
        // user no longer sees.
        if let Some(deadline) = self.debounce_until {
            if !self.visible {
                self.debounce_until = None;
            } else if foreground && self.date_error.is_none() && Instant::now() >= deadline {
                self.debounce_until = None;
                self.applied = self.generation;
                self.refresh(cx);
            }
        }
        if foreground
            && self.refresh_interval > 0
            && !self.pending_dashboard
            && !self.pending_rows
            && self.last_refresh.elapsed() >= Duration::from_secs(self.refresh_interval)
        {
            self.refresh(cx);
        }
        // Provider modes are filled in one per tick so the list converges
        // without ever queueing a job per provider at once.
        if foreground && self.storage_open {
            self.request_provider_modes(cx);
        }
        // Maintenance progress is polled at most every two seconds and only
        // when no poll is already in flight.
        if self.visible
            && self.maintenance.is_some()
            && self.maintenance_poll.elapsed() >= Duration::from_secs(2)
            && !self.backend.is_busy(&JobKey::Maintenance)
        {
            self.maintenance_poll = Instant::now();
            self.poll_maintenance(cx);
        }
        while let Ok(reply) = self.backend.replies().try_recv() {
            self.backend.complete(&reply.key);
            self.handle_reply(reply, window, cx);
        }
    }

    fn handle_reply(&mut self, reply: StatsReply, window: &mut Window, cx: &mut Context<Self>) {
        let StatsReply {
            seq,
            kind,
            tag,
            result,
            ..
        } = reply;
        // Routing is the controller's decision, so the real page and its tests
        // agree on when a reply may touch anything.
        let reply_kind = match kind {
            JobKind::Dashboard => crate::stats_controller::ReplyKind::Dashboard,
            JobKind::Rows => crate::stats_controller::ReplyKind::Rows,
            JobKind::Groups => crate::stats_controller::ReplyKind::Groups,
            _ => crate::stats_controller::ReplyKind::Other,
        };
        let tab = match self.tab {
            Tab::Requests => crate::stats_controller::StatsTab::Rows,
            _ => crate::stats_controller::StatsTab::Groups,
        };
        if crate::stats_controller::route_reply(
            reply_kind,
            seq,
            self.dashboard_seq,
            &self.rows_gate,
            &self.groups_gate,
            tab,
        ) == crate::stats_controller::ReplyRoute::Ignore
        {
            // A late reply changes nothing at all — not the data, not the error,
            // and not the pending flag of the read that replaced it.
            return;
        }
        if kind.is_read() && !self.visible {
            return;
        }
        let expected = match kind {
            JobKind::Prices => Some(self.prices_seq),
            JobKind::Settings => Some(self.settings_seq),
            JobKind::Detail => Some(self.detail_seq),
            JobKind::Filters if tag.starts_with("filters-") => Some(self.provider_seq),
            JobKind::Filters => Some(self.model_seq),
            _ => None,
        };
        if expected.is_some_and(|expected| expected == 0 || expected != seq) {
            return;
        }
        if kind == JobKind::PriceSave && self.price_draft.tag.as_deref() != Some(&tag) {
            if result.is_ok() {
                self.request_prices(cx);
            }
            return;
        }
        let dashboard_reply = kind == JobKind::Dashboard;
        let rows_reply = matches!(kind, JobKind::Rows | JobKind::Groups);
        // Collection health is shown from the core's own snapshot, never
        // inferred from the fact that a query succeeded.
        if kind == JobKind::Health {
            if let Ok(health) =
                serde_json::from_value::<HealthDto>(result.clone().unwrap_or(Value::Null))
            {
                if self.timezone.is_empty() {
                    self.timezone = health.timezone.clone();
                }
                // Full snapshot: the badge and the warnings come from here, not
                // from the fact that this query succeeded.
                self.health = Some(health);
            }
            return;
        }
        let result = result.and_then(|value| {
            let valid = match kind {
                JobKind::Dashboard => {
                    serde_json::from_value::<DashboardDto>(value.clone()).map(|_| ())
                }
                JobKind::Rows => {
                    serde_json::from_value::<crate::stats_dto::RequestPageDto>(value.clone())
                        .map(|_| ())
                }
                JobKind::Groups => {
                    serde_json::from_value::<BreakdownDto>(value.clone()).map(|_| ())
                }
                JobKind::Filters => {
                    serde_json::from_value::<crate::stats_dto::FilterOptionsDto>(value.clone())
                        .map(|_| ())
                }
                JobKind::Prices => {
                    serde_json::from_value::<crate::stats_dto::PriceListDto>(value.clone())
                        .map(|_| ())
                }
                JobKind::Detail => {
                    serde_json::from_value::<RequestDetailDto>(value.clone()).map(|_| ())
                }
                JobKind::Settings | JobKind::SettingsUpdate => {
                    serde_json::from_value::<SettingsDto>(value.clone()).map(|_| ())
                }
                _ => Ok(()),
            };
            valid.map(|_| value).map_err(|_| {
                self.t("统计响应格式错误", "Invalid statistics response")
                    .to_owned()
            })
        });
        match result {
            Err(failure) => {
                if kind == JobKind::Prices {
                    self.prices_loading = false;
                }
                if dashboard_reply {
                    self.pending_dashboard = false;
                }
                if rows_reply {
                    self.pending_rows = false;
                }
                if kind == JobKind::PriceSave {
                    // Keep the dialog open with every typed value so the failure
                    // can be corrected in place. An unrelated error never
                    // touches the draft.
                    self.price_draft.apply_reply(&tag, Err(failure.clone()));
                }
                // An expired cursor is expected whenever the window, run or
                // maintenance revision moved under us; restart pagination
                // instead of showing a stale page.
                if rows_reply && failure.contains(crate::stats_dto::ERR_CURSOR_EXPIRED) {
                    self.reset_pagination();
                    self.error = Some(
                        self.t(
                            "分页已失效（数据窗口或维护已变化），已回到第一页",
                            "Pagination expired (window or maintenance changed); back to page 1",
                        )
                        .to_owned(),
                    );
                    self.loading_dashboard = false;
                    self.loading_rows = false;
                    // Only the listing that expired goes back to its first page.
                    match kind {
                        JobKind::Groups => self.request_groups(cx),
                        _ => self.request_rows(cx),
                    }
                    cx.notify();
                    return;
                }
                // Keep whatever data is on screen and report the failure.
                self.error = Some(failure);
                self.loading_dashboard = false;
                self.loading_rows = false;
            }
            Ok(value) => self.apply_reply(kind, &tag, value, window, cx),
        }
        cx.notify();
    }

    fn apply_reply(
        &mut self,
        kind: JobKind,
        tag: &str,
        value: Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if tag.starts_with("dash-") {
            self.pending_dashboard = false;
            self.loading_dashboard = false;
            if let Ok(dashboard) = serde_json::from_value::<DashboardDto>(value) {
                self.timezone = dashboard.timezone.clone();
                self.filter_key = dashboard.filter_key.clone();
                self.dashboard = Some(dashboard);
                self.error = None;
                self.last_updated = Some(Instant::now());
            }
            return;
        }
        if tag.starts_with("rows-") {
            self.pending_rows = false;
            self.loading_rows = false;
            match self.tab {
                Tab::Requests => {
                    if let Ok(page) =
                        serde_json::from_value::<crate::stats_dto::RequestPageDto>(value)
                    {
                        self.as_of_id = page.as_of_id;
                        self.total_rows = if page.total_rows >= 0 {
                            page.total_rows
                        } else {
                            self.total_rows
                        };
                        // Only the core decides whether another page exists.
                        self.rows_cursors.finish_page(page.next_cursor);
                        self.rows = page.items;
                        self.usage_table.update(cx, |table, cx| {
                            table.delegate_mut().rows = self.rows.clone();
                            table.delegate_mut().loading = false;
                            table.refresh(cx);
                        });
                        self.error = None;
                        self.last_updated = Some(Instant::now());
                    }
                }
                _ => {
                    if let Ok(page) = serde_json::from_value::<BreakdownDto>(value) {
                        self.total_groups = if page.total_groups >= 0 {
                            page.total_groups
                        } else {
                            self.total_groups
                        };
                        self.groups = page.items;
                        // The core's own cursor is what makes the next page
                        // reachable; without it "next" re-requested page one.
                        self.group_next_cursor = page.next_cursor;
                        self.group_table.update(cx, |table, cx| {
                            table.delegate_mut().rows = self.groups.clone();
                            table.delegate_mut().group_by = page.group_by;
                            table.delegate_mut().loading = false;
                            table.refresh(cx);
                        });
                        self.error = None;
                        self.last_updated = Some(Instant::now());
                    }
                }
            }
            return;
        }
        if tag.starts_with("filters-") {
            if let Ok(page) = serde_json::from_value::<crate::stats_dto::FilterOptionsDto>(value) {
                self.provider_next = page.next_key;
                if self.provider_after.is_none() {
                    self.provider_options.clear();
                }
                for item in page.items {
                    if !self.provider_options.iter().any(|old| old.key == item.key) {
                        self.provider_options.push(item);
                    }
                }
                self.sync_price_providers(window, cx);
                let mut items = vec![choice(
                    "",
                    if self.zh {
                        "全部供应商"
                    } else {
                        "All providers"
                    },
                )];
                for option in &self.provider_options {
                    let label = if option.historical && !option.label.is_empty() {
                        format!(
                            "{} ({})",
                            option.label,
                            if self.zh {
                                "历史配置"
                            } else {
                                "historical"
                            }
                        )
                    } else if option.label.is_empty() {
                        option.key.clone()
                    } else {
                        option.label.clone()
                    };
                    items.push(choice(option.key.clone(), label));
                }
                self.provider_select.update(cx, |state, cx| {
                    state.set_items(items, window, cx);
                    state.set_selected_value(&self.provider.clone(), window, cx);
                });
                self.request_provider_modes(cx);
            }
            return;
        }
        if tag.starts_with("models-") {
            if let Ok(page) = serde_json::from_value::<crate::stats_dto::FilterOptionsDto>(value) {
                self.model_next = page.next_key;
                if self.model_after.is_none() {
                    self.model_options.clear();
                }
                for item in page.items {
                    if !self.model_options.iter().any(|old| old.key == item.key) {
                        self.model_options.push(item);
                    }
                }
                let mut items = vec![choice(
                    "",
                    if self.zh {
                        "全部模型"
                    } else {
                        "All models"
                    },
                )];
                for option in &self.model_options {
                    items.push(choice(option.key.clone(), option.key.clone()));
                }
                self.model_select.update(cx, |state, cx| {
                    state.set_items(items, window, cx);
                    state.set_selected_value(&self.model.clone(), window, cx);
                });
            }
            return;
        }
        if tag.starts_with("settings-") || kind == JobKind::SettingsUpdate {
            if let Ok(settings) = serde_json::from_value::<SettingsDto>(value) {
                self.settings_seq = 0;
                self.settings = Some(settings);
            }
            return;
        }
        if tag.starts_with("providersettings-") {
            if let Ok(settings) = serde_json::from_value::<ProviderSettingsDto>(value) {
                self.provider_modes
                    .insert(settings.provider_id.clone(), settings);
            }
            return;
        }
        if kind == JobKind::PriceDelete {
            self.request_prices(cx);
            return;
        }
        if kind == JobKind::PriceSave {
            // Only the reply for the draft that is still open may close it; a
            // late reply for a replaced draft is ignored entirely.
            if self.price_draft.apply_reply(tag, Ok(())) {
                self.price_open = false;
                self.price_closing = true;
                // The dialog is closed here, not when the button was pressed:
                // a rejection must leave the form open with its values.
                window.close_dialog(cx);
                self.error = None;
                self.request_prices(cx);
                // The current price changed, so the visible statistics may now
                // price new requests differently.
                self.refresh(cx);
            }
            cx.notify();
            return;
        }
        if kind == JobKind::Prices {
            self.prices_loading = false;
            if let Ok(list) = serde_json::from_value::<crate::stats_dto::PriceListDto>(value) {
                self.prices_cursors.finish_page(list.next_rule_id);
                self.prices = list.items;
            }
            return;
        }
        if tag.starts_with("detail-") {
            if let Ok(detail) = serde_json::from_value::<RequestDetailDto>(value) {
                self.details = Some(detail);
            }
            return;
        }
        if tag.starts_with("maintenance-") {
            if let Ok(status) = serde_json::from_value::<MaintenanceDto>(value) {
                if status.status != MaintenanceStatus::Running {
                    self.refresh(cx);
                    self.request_prices(cx);
                }
                self.maintenance = Some(status);
            }
            return;
        }
        if tag.starts_with("clear-") {
            // The preview token is only valid in this process and for 60s.
            self.clear_token = value
                .get("token")
                .and_then(Value::as_str)
                .map(str::to_owned);
            if let Ok(preview) = serde_json::from_value::<ClearPreviewDto>(value) {
                self.clear_preview = Some(preview);
            }
        }
    }

    pub(crate) fn open_detail(&mut self, row: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(item) = self.rows.get(row) else {
            return;
        };
        let request_id = item.request_id.clone();
        let tag = format!("detail-{}", self.generation);
        let request = crate::stats_backend::command(
            "stats_request_detail",
            &tag,
            json!({"request_id": request_id}),
        );
        self.detail_seq =
            self.backend
                .submit_read(JobKey::Detail(request_id.clone()), tag, request);
        self.details = None;
        self.detail_open = true;
        self.open_detail_dialog(window, cx);
    }

    fn open_detail_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let owner = cx.entity().downgrade();
        let observer = cx.entity().clone();
        let body = cx.new(|cx| {
            cx.observe(&observer, |_, _, cx| cx.notify()).detach();
            DetailBody {
                owner: owner.clone(),
            }
        });
        window.open_dialog(cx, move |dialog, _window, _cx| {
            let close_owner = owner.clone();
            dialog
                .w(px(560.))
                .close_button(true)
                .overlay_closable(true)
                .child(body.clone())
                .on_close(move |_, _, cx| {
                    let _ = close_owner.update(cx, |this, cx| {
                        this.detail_seq = 0;
                        this.detail_open = false;
                        this.details = None;
                        cx.notify();
                    });
                })
                .on_ok(move |_, _, _| true)
        });
    }
}

/// Dialog body rendered from the owner's state.
struct DetailBody {
    owner: WeakEntity<Statistics>,
}

impl Render for DetailBody {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.owner
            .update(cx, |this, cx| this.detail_content(cx))
            .unwrap_or_else(|_| div().into_any_element())
    }
}

impl Statistics {
    fn detail_content(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let zh = self.zh;
        let title = if zh { "请求详情" } else { "Request detail" };
        let Some(detail) = &self.details else {
            return DialogContent::new()
                .w_full()
                .gap_3()
                .child(
                    div()
                        .text_lg()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(title),
                )
                .child(Label::new(if zh { "正在读取…" } else { "Loading…" }).text_color(rgb(MUTED)))
                .into_any_element();
        };
        let item = &detail.request;
        let row = |name: String, value: String| {
            div()
                .flex()
                .justify_between()
                .gap_3()
                .text_size(px(12.))
                .child(Label::new(name).text_color(rgb(MUTED)))
                .child(Label::new(value).text_color(rgb(INK)))
        };
        let cost_text = match item.cost.as_ref() {
            Some(cost)
                if matches!(
                    cost.status,
                    CostStatus::Priced | CostStatus::DefaultZero | CostStatus::NotApplicable
                ) =>
            {
                format!(
                    "{} {}",
                    cost.currency.map_or("", |currency| currency.as_str()),
                    usage_table::amount(cost.total_cost_nanos.as_deref(), cost.currency)
                )
            }
            Some(cost) => usage_table::cost_label(cost.status, zh),
            None => usage_table::cost_label(CostStatus::Unpriced, zh),
        };
        let mut rows = vec![
            row(
                if zh {
                    "请求标识".into()
                } else {
                    "Request ID".into()
                },
                item.request_id.clone(),
            ),
            row(
                if zh {
                    "时间".into()
                } else {
                    "Started".into()
                },
                usage_table::timestamp(item.started_at_ms, zh),
            ),
            row(
                if zh {
                    "供应商 / 路由".into()
                } else {
                    "Provider / route".into()
                },
                format!("{} · {}", item.provider_name, item.route_name),
            ),
            row(
                if zh {
                    "请求模型".into()
                } else {
                    "Requested model".into()
                },
                item.requested_model.clone(),
            ),
            row(
                if zh {
                    "上游模型".into()
                } else {
                    "Upstream model".into()
                },
                item.upstream_model.clone(),
            ),
            row(
                if zh {
                    "响应声明模型".into()
                } else {
                    "Response model".into()
                },
                item.response_model.clone().unwrap_or_else(|| "—".into()),
            ),
            row(
                if zh {
                    "状态".into()
                } else {
                    "Outcome".into()
                },
                format!(
                    "HTTP {} · {}",
                    item.http_status
                        .map_or("—".into(), |status| status.to_string()),
                    usage_table::outcome_label(item.outcome, item.error_kind, zh)
                ),
            ),
            row(
                if zh { "用量".into() } else { "Usage".into() },
                format!(
                    "{} · {}",
                    usage_table::usage_label(item.usage_status, zh).0,
                    item.usage_reason
                        .map_or_else(|| "—".into(), |reason| format!("{reason}"))
                ),
            ),
            row(
                if zh { "Token".into() } else { "Tokens".into() },
                format!(
                    "↓{} ↑{} R{} W{} Σ{}",
                    usage_table::exact(item.input_tokens),
                    usage_table::exact(item.output_tokens),
                    usage_table::exact(item.cache_read_tokens),
                    usage_table::exact(item.cache_write_tokens),
                    usage_table::exact(item.total_tokens)
                ),
            ),
            row(
                if zh {
                    "性能".into()
                } else {
                    "Latency".into()
                },
                format!(
                    "{} {} · {} {} · TTFB {}",
                    if zh { "总" } else { "total" },
                    usage_table::duration(item.duration_ms),
                    if zh { "首内容" } else { "first" },
                    usage_table::duration(item.first_content_ms),
                    usage_table::duration(item.ttfb_ms)
                ),
            ),
            row(
                if zh {
                    "估算费用".into()
                } else {
                    "Estimated cost".into()
                },
                cost_text,
            ),
        ];
        if let Some(snapshot) = &detail.price_snapshot {
            // The prices this request actually used, read from its own row: they
            // stay correct even after the configuration is edited or deleted.
            let rates = [
                snapshot.input_rate_nanos.as_deref(),
                snapshot.output_rate_nanos.as_deref(),
                snapshot.cache_read_rate_nanos.as_deref(),
                snapshot.cache_write_rate_nanos.as_deref(),
            ];
            let text = match snapshot.price_source {
                crate::stats_dto::PriceSource::DefaultZero => {
                    if zh {
                        "未配置价格，按 0 计算".to_owned()
                    } else {
                        "No price configured · counted as 0".to_owned()
                    }
                }
                _ => rates
                    .iter()
                    .enumerate()
                    .filter_map(|(index, rate)| {
                        rate.map(|value| {
                            let label = ["in", "out", "read", "write"][index];
                            format!("{label} {}", usage_table::amount(Some(value), None))
                        })
                    })
                    .collect::<Vec<_>>()
                    .join(" · "),
            };
            let source = snapshot
                .source_price_id
                .as_deref()
                .map(|id| {
                    format!(
                        " · {} {}@{}",
                        if zh { "来源" } else { "source" },
                        id,
                        snapshot.source_price_revision.unwrap_or(0)
                    )
                })
                .unwrap_or_default();
            rows.push(row(
                if zh {
                    "本次单价".into()
                } else {
                    "Unit price used".into()
                },
                format!("{text}{source}"),
            ));
        }
        rows.push(row(
            if zh {
                "记录时间".into()
            } else {
                "Recorded".into()
            },
            format!(
                "{} → {}",
                usage_table::timestamp(detail.created_at_ms, zh),
                usage_table::timestamp(detail.updated_at_ms, zh)
            ),
        ));
        DialogContent::new()
            .w_full()
            .gap_3()
            .child(
                div()
                    .text_lg()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(title),
            )
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(rgb(MUTED))
                    .child(if zh {
                        "只保存统计摘要：不包含聊天正文、工具参数、图片或密钥"
                    } else {
                        "Summary only: no chat text, tool arguments, images or keys are stored"
                    }),
            )
            .child(div().flex().flex_col().gap_2().children(rows))
            .child(
                DialogFooter::new().child(
                    Button::new("detail-close")
                        .label(if zh { "关闭" } else { "Close" })
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.detail_open = false;
                            window.close_dialog(cx);
                        })),
                ),
            )
            .into_any_element()
    }
}

/// Filter row and the page header.
impl Statistics {
    fn header(&self, _cx: &mut Context<Self>) -> AnyElement {
        let healthy = self.error.is_none();
        div()
            .flex()
            .items_center()
            .justify_between()
            .w_full()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .text_size(px(18.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(self.t("使用统计", "Usage statistics")),
                    )
                    .child(
                        Label::new(self.t(
                            "统计本代理已记录的模型请求",
                            "Requests recorded by this proxy",
                        ))
                        .text_size(px(11.))
                        .text_color(rgb(MUTED)),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(Tag::secondary().small().child(match &self.timezone {
                        name if name.is_empty() => self.t("本机时区", "Local timezone").to_owned(),
                        name => name.clone(),
                    }))
                    .child(Tag::secondary().small().child(match self.last_updated {
                        Some(at) => format!(
                            "{} {:.1}s",
                            self.t("刷新于", "updated"),
                            at.elapsed().as_secs_f32()
                        ),
                        None => self.t("尚未刷新", "not refreshed").to_owned(),
                    }))
                    .child(if healthy {
                        Tag::success().small().child(self.t("正常", "OK"))
                    } else {
                        Tag::danger()
                            .small()
                            .child(self.t("数据更新失败", "Update failed"))
                    }),
            )
            .into_any_element()
    }

    fn filters(&self, cx: &mut Context<Self>) -> AnyElement {
        stats_card("stats-filters")
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_end()
                    .gap_3()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(self.filter_label(self.t("时间范围", "Range")))
                            .child(Select::new(&self.range_select).w(px(140.)).small()),
                    )
                    .when(
                        self.range_select
                            .read(cx)
                            .selected_value()
                            .is_some_and(|v| v == "custom"),
                        |this| {
                            // A custom window is typed in this machine's timezone;
                            // both ends must parse before the query is re-run.
                            this.child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .child(self.filter_label(self.t("起始", "From")))
                                    .child(Input::new(&self.custom_from).w(px(160.))),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .child(self.filter_label(self.t("结束", "To")))
                                    .child(Input::new(&self.custom_to).w(px(160.))),
                            )
                        },
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(self.filter_label(self.t("供应商", "Provider")))
                            .child(Select::new(&self.provider_select).w(px(180.)).small()),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(self.filter_label(self.t("模型", "Model")))
                            .child(Select::new(&self.model_select).w(px(200.)).small()),
                    )
                    .child(Input::new(&self.provider_search).w(px(150.)))
                    .child(
                        Button::new("provider-search-more")
                            .small()
                            .ghost()
                            .label(self.t("更多供应商", "More providers"))
                            .disabled(self.provider_next.is_none())
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.provider_after = this.provider_next.clone();
                                this.request_filters(cx);
                            })),
                    )
                    .child(Input::new(&self.model_search).w(px(150.)))
                    .child(
                        Button::new("model-search-more")
                            .small()
                            .ghost()
                            .label(self.t("更多模型", "More models"))
                            .disabled(self.model_next.is_none())
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.model_after = this.model_next.clone();
                                this.request_model_filters(cx);
                            })),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(self.filter_label(self.t("状态", "State")))
                            .child(Select::new(&self.outcome_select).w(px(140.)).small()),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(self.filter_label(self.t("用量完整性", "Usage")))
                            .child(Select::new(&self.usage_select).w(px(140.)).small()),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(self.filter_label(self.t("自动刷新", "Auto refresh")))
                            .child(Select::new(&self.refresh_select).w(px(150.)).small()),
                    )
                    .child(
                        Checkbox::new("include-tests")
                            .label(self.t("包含测试连接", "Include connection tests"))
                            .checked(self.include_tests)
                            .small()
                            .on_change(cx.listener(|this, enabled: &bool, _, cx| {
                                this.include_tests = *enabled;
                                this.mark_dirty(cx);
                            })),
                    )
                    .child(
                        Button::new("stats-refresh")
                            .small()
                            .primary()
                            .icon(IconName::RefreshCw)
                            .label(self.t("刷新", "Refresh"))
                            .disabled(self.loading_dashboard || self.loading_rows)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.generation += 1;
                                this.debounce_until = None;
                                this.applied = this.generation;
                                this.refresh(cx);
                            })),
                    ),
            )
            .when_some(self.date_error.clone(), |this, error| {
                this.child(
                    Label::new(error)
                        .text_color(rgb(0xa4424a))
                        .text_size(px(11.)),
                )
            })
            .when_some(self.error.clone(), |this, error| {
                this.child(
                    Label::new(crate::i18n::message(&error, self.zh))
                        .text_size(px(11.))
                        .text_color(rgb(0xa4424a)),
                )
            })
            .into_any_element()
    }

    fn filter_label(&self, text: &'static str) -> Label {
        Label::new(text).text_size(px(11.)).text_color(rgb(MUTED))
    }

    /// Quality banner. Every entry comes from real counters; an absent warning
    /// is never rendered as "fine".
    fn warnings(&self) -> Option<AnyElement> {
        let dashboard = self.dashboard.as_ref()?;
        if dashboard.warnings.is_empty() {
            return None;
        }
        let rows: Vec<AnyElement> = dashboard
            .warnings
            .iter()
            .map(|warning| {
                let text = match warning.code {
                    WarningCode::UsageMissing => format!(
                        "{} {}",
                        if self.zh {
                            "未报告用量的请求："
                        } else {
                            "Requests without reported usage:"
                        },
                        warning.request_count
                    ),
                    WarningCode::UsagePartial => format!(
                        "{} {}",
                        if self.zh {
                            "部分用量："
                        } else {
                            "Partial usage:"
                        },
                        warning.request_count
                    ),
                    WarningCode::UsageInvalid => format!(
                        "{} {}",
                        if self.zh {
                            "无效用量（已排除）："
                        } else {
                            "Invalid usage (excluded):"
                        },
                        warning.request_count
                    ),
                    WarningCode::CollectionDisabled => {
                        if self.zh {
                            "采集当前已暂停，新请求不会写入统计".to_owned()
                        } else {
                            "Collection is paused; new requests are not recorded".to_owned()
                        }
                    }
                    WarningCode::CollectorLoss => format!(
                        "{} {}",
                        if self.zh {
                            "本次运行丢弃的统计事件："
                        } else {
                            "Dropped statistics events this run:"
                        },
                        warning.request_count
                    ),
                    WarningCode::UncleanExit => {
                        if self.zh {
                            "上次退出异常，中断区间的记录可能不全".to_owned()
                        } else {
                            "Previous exit was unclean; some intervals may be incomplete".to_owned()
                        }
                    }
                    WarningCode::RetentionLimited => {
                        if self.zh {
                            "保留策略已删除更早的明细，总览只覆盖尚存记录".to_owned()
                        } else {
                            "Retention removed older rows; the overview covers surviving records"
                                .to_owned()
                        }
                    }
                    WarningCode::ProviderObserveMode => format!(
                        "{} {}",
                        if self.zh {
                            "供应商仍为被动采集，可能没有 Token："
                        } else {
                            "Providers still in observe mode (tokens may be missing):"
                        },
                        warning.request_count
                    ),
                };
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_size(px(11.))
                    .child(div().w(px(6.)).h(px(6.)).rounded(px(3.)).bg(rgb(0xd9822b)))
                    .child(Label::new(text).text_color(rgb(0x8a5a1f)))
                    .into_any_element()
            })
            .collect();
        Some(
            stats_card("stats-warnings")
                .bg(rgb(0xfff8ec))
                .border_color(rgb(0xf0dcc0))
                .gap_2()
                .children(rows)
                .into_any_element(),
        )
    }

    /// Overview: one primary confirmed-token card plus secondary metrics.
    fn overview(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(dashboard) = self.dashboard.clone() else {
            return stats_card("stats-overview")
                .child(usage_table::strong(
                    self.t("正在读取统计…", "Reading statistics…"),
                ))
                .into_any_element();
        };
        let summary = &dashboard.summary;
        let zh = self.zh;
        let main_value = match summary.total_tokens {
            Some(value) => usage_table::compact(Some(value), zh),
            None => "—".into(),
        };
        let main_hint = match (summary.usage_complete_count, summary.usage_eligible_count) {
            (0, _) => self.t("暂无完整用量", "No complete usage yet").to_owned(),
            (complete, eligible) => format!(
                "{}/{} {}",
                complete,
                eligible,
                self.t("个请求用量完整", "requests with complete usage")
            ),
        };
        let costs = if summary.costs.is_empty() {
            self.t("未定价", "Unpriced").to_owned()
        } else {
            summary
                .costs
                .iter()
                .map(|cost| {
                    format!(
                        "{} {}",
                        cost.currency.as_str(),
                        crate::stats_dto::format_amount(
                            cost.total_cost_nanos.parse::<i64>().unwrap_or(0)
                        )
                    )
                })
                .collect::<Vec<_>>()
                .join(" / ")
        };
        let secondary: Vec<(&'static str, String, String)> = vec![
            (
                self.t("请求数", "Requests"),
                summary.request_count.to_string(),
                format!(
                    "{} {}",
                    summary.in_flight_count,
                    self.t("进行中", "in flight")
                ),
            ),
            (
                self.t("成功率", "Success rate"),
                usage_table::percent(summary.success_rate),
                format!(
                    "{} {} · {} {}",
                    summary.completed_count,
                    self.t("成功", "ok"),
                    summary.failed_count,
                    self.t("失败", "failed")
                ),
            ),
            (
                self.t("输入 Token", "Input tokens"),
                usage_table::compact(summary.input_tokens, zh),
                format!(
                    "{} {}",
                    summary.cache_read_known_count,
                    self.t("已知缓存", "with cache data")
                ),
            ),
            (
                self.t("输出 Token", "Output tokens"),
                usage_table::compact(summary.output_tokens, zh),
                self.t("不含思考子集重复计算", "reasoning stays a subset")
                    .into(),
            ),
            (
                self.t("缓存读取", "Cache read"),
                usage_table::compact(summary.cache_read_tokens, zh),
                format!(
                    "{} {}",
                    self.t("命中率", "hit rate"),
                    usage_table::percent(summary.cache_hit_rate)
                ),
            ),
            (
                self.t("缓存创建", "Cache write"),
                usage_table::compact(summary.cache_write_tokens, zh),
                self.t("输入子集，不可相加", "input subset, never summed")
                    .into(),
            ),
            (
                self.t("完整覆盖率", "Usage coverage"),
                usage_table::percent(summary.usage_coverage),
                format!(
                    "{} {}/{}",
                    self.t("用量完整", "complete"),
                    summary.usage_complete_count,
                    summary.usage_eligible_count
                ),
            ),
            (
                self.t("估算费用", "Estimated cost"),
                costs,
                format!(
                    "{} {} · {} {}",
                    summary.unpriced_request_count,
                    self.t("未定价", "unpriced"),
                    summary.cost_incomplete_request_count,
                    self.t("用量不全", "incomplete")
                ),
            ),
        ];
        let width = cx.entity_id();
        let _ = width;
        stats_card("stats-overview")
            .child(
                div()
                    .flex()
                    .items_end()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                Label::new(self.t("已确认 Token", "Confirmed tokens"))
                                    .text_size(px(11.))
                                    .text_color(rgb(MUTED)),
                            )
                            .child(
                                div()
                                    .text_size(px(28.))
                                    .font_weight(FontWeight::BOLD)
                                    .child(main_value),
                            )
                            .child(
                                Label::new(main_hint)
                                    .text_size(px(11.))
                                    .text_color(rgb(MUTED)),
                            ),
                    )
                    .child(
                        Label::new(self.t(
                            "只统计已结束且用量完整的请求",
                            "Counts finished requests with complete usage only",
                        ))
                        .text_size(px(10.))
                        .text_color(rgb(MUTED)),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_3()
                    .children(secondary.into_iter().map(|(title, value, hint)| {
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .min_w(px(148.))
                            .flex_1()
                            .p_3()
                            .rounded(px(8.))
                            .bg(rgb(BG))
                            .child(Label::new(title).text_size(px(11.)).text_color(rgb(MUTED)))
                            .child(
                                div()
                                    .text_size(px(16.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(value),
                            )
                            .child(Label::new(hint).text_size(px(10.)).text_color(rgb(MUTED)))
                    })),
            )
            .into_any_element()
    }

    /// Token chart, optional cost chart and the legend.
    fn trends(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(dashboard) = self.dashboard.clone() else {
            return stats_card("stats-trends")
                .child(usage_table::strong(self.t("暂无趋势数据", "No trend data")))
                .into_any_element();
        };
        let points = ChartPoint::from_series(&dashboard.series, self.currency);
        let token_chart = UsagePlot {
            id: ElementId::Name("usage-token-chart".into()),
            points: points.clone(),
            visible: self.visible_series.clone(),
            zh: self.zh,
            mode: ChartMode::Tokens,
            currency: self.currency,
        };
        let token_summary = UsagePlot {
            id: ElementId::Name("usage-token-chart-summary".into()),
            points: points.clone(),
            visible: self.visible_series.clone(),
            zh: self.zh,
            mode: ChartMode::Tokens,
            currency: self.currency,
        }
        .summary();
        let has_cost = points.iter().any(|point| point.cost_nanos.is_some());
        let cost_chart = UsagePlot {
            id: ElementId::Name("usage-cost-chart".into()),
            points: points.clone(),
            visible: BTreeSet::new(),
            zh: self.zh,
            mode: ChartMode::Cost,
            currency: self.currency,
        };
        let legend = div().flex().flex_wrap().items_center().gap_3().children(
            SeriesKind::ALL.into_iter().map(|kind| {
                let active = self.visible_series.contains(&kind);
                div()
                    .id(("legend", kind as usize))
                    .flex()
                    .items_center()
                    .gap_1()
                    .cursor_pointer()
                    .opacity(if active { 1. } else { 0.4 })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if !this.visible_series.remove(&kind) {
                            this.visible_series.insert(kind);
                        }
                        cx.notify();
                    }))
                    .child(
                        div()
                            .w(px(10.))
                            .h(px(3.))
                            .rounded(px(2.))
                            .bg(rgb(kind.color())),
                    )
                    .child(
                        Label::new(kind.label(self.zh))
                            .text_size(px(11.))
                            .text_color(rgb(INK)),
                    )
                    .into_any_element()
            }),
        );
        stats_card("stats-trends")
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(13.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(self.t("趋势", "Trend")),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(legend)
                            .child(Select::new(&self.currency_select).w(px(96.)).small()),
                    ),
            )
            .child(
                div()
                    .text_size(px(10.))
                    .text_color(rgb(MUTED))
                    .child(self.t(
                        "缓存读/写是输入的子集，图例可独立开关，不可相加",
                        "Cache read/write are input subsets; toggle them, never sum them",
                    )),
            )
            .child(
                div()
                    .id("token-chart-host")
                    .w_full()
                    .h(px(280.))
                    .min_h_0()
                    .aria_label(token_summary.clone())
                    .child(token_chart),
            )
            .when(has_cost, |this| {
                this.child(
                    div()
                        .text_size(px(11.))
                        .text_color(rgb(MUTED))
                        .child(self.t("估算费用（按币种）", "Estimated cost (per currency)")),
                )
                .child(
                    div()
                        .id("cost-chart-host")
                        .w_full()
                        .h(px(140.))
                        .min_h_0()
                        .aria_label(self.t(
                            "费用趋势图，按币种单独显示",
                            "Cost trend chart, one currency at a time",
                        ))
                        .child(cost_chart),
                )
            })
            .into_any_element()
    }

    /// Tabs, table and pagination.
    fn details(&self, cx: &mut Context<Self>) -> AnyElement {
        let tab_button = |id: &'static str, tab: Tab, label: String| {
            let active = self.tab == tab;
            Button::new(id)
                .small()
                .when(!active, |button| button.ghost())
                .when(active, |button| button.primary().outline())
                .label(label)
                .on_click(cx.listener(move |this, _, window, cx| {
                    if this.tab == tab {
                        return;
                    }
                    this.tab = tab;
                    this.reset_pagination();
                    // Leaving a tab invalidates its outstanding read at once:
                    // the reply that is already in flight must not land in the
                    // tab the user just opened.
                    this.rows_gate.invalidate();
                    this.groups_gate.invalidate();
                    match tab {
                        Tab::Requests => this.request_rows(cx),
                        _ => this.request_groups(cx),
                    }
                    let _ = window;
                    cx.notify();
                }))
        };
        let page = match self.tab {
            Tab::Requests => {
                if self.total_rows > 0 {
                    let pages = (self.total_rows + self.page_size - 1) / self.page_size;
                    format!(
                        "{} {}/{}",
                        self.t("第", "page"),
                        self.rows_cursors.page_number() as i64,
                        pages
                    )
                } else {
                    String::new()
                }
            }
            _ => format!("{} {}", self.total_groups, self.t("个分组", "groups")),
        };
        let sort_button = |id: &'static str, sort: SortMetric, label: String| {
            let active = self.group_sort == sort;
            Button::new(id)
                .small()
                .when(!active, |button| button.ghost())
                .when(active, |button| button.primary().outline())
                .label(label)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.group_sort = sort;
                    this.reset_pagination();
                    if this.tab != Tab::Requests {
                        this.request_groups(cx);
                    }
                }))
        };
        let table: AnyElement = match self.tab {
            Tab::Requests => DataTable::new(&self.usage_table)
                .small()
                .header_row_height(px(32.))
                .scrollbar_visible(true, true)
                .into_any_element(),
            _ => DataTable::new(&self.group_table)
                .small()
                .header_row_height(px(32.))
                .scrollbar_visible(true, true)
                .into_any_element(),
        };
        stats_card("stats-details")
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(tab_button(
                                "tab-requests",
                                Tab::Requests,
                                self.t("请求日志", "Request log").to_owned(),
                            ))
                            .child(tab_button(
                                "tab-providers",
                                Tab::Providers,
                                self.t("供应商统计", "Providers").to_owned(),
                            ))
                            .child(tab_button(
                                "tab-models",
                                Tab::Models,
                                self.t("模型统计", "Models").to_owned(),
                            ))
                            .when(self.tab != Tab::Requests, |this| {
                                this.child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap_2()
                                        .child(
                                            Label::new(self.t("排序", "Sort"))
                                                .text_size(px(11.))
                                                .text_color(rgb(MUTED)),
                                        )
                                        .child(sort_button(
                                            "sort-tokens",
                                            SortMetric::TotalTokens,
                                            self.t("总 Token", "Total tokens").to_owned(),
                                        ))
                                        .child(sort_button(
                                            "sort-requests",
                                            SortMetric::RequestCount,
                                            self.t("请求数", "Requests").to_owned(),
                                        )),
                                )
                            }),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(Label::new(page).text_size(px(11.)).text_color(rgb(MUTED)))
                            // The grouping listing owns its own cursor chain, so
                            // walking providers/models never moves the request
                            // log and vice versa.
                            .when(self.tab != Tab::Requests, |this| {
                                let group_by = match self.tab {
                                    Tab::Models => GroupBy::Model,
                                    _ => GroupBy::Provider,
                                };
                                this.child(
                                    Button::new("groups-prev")
                                        .small()
                                        .ghost()
                                        .label(self.t("上一页", "Previous"))
                                        .disabled(
                                            self.loading_rows || self.group_cursor_stack.is_empty(),
                                        )
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            if let Some(previous) = this.group_cursor_stack.pop() {
                                                this.group_next_cursor =
                                                    this.group_current_cursor.clone();
                                                this.group_current_cursor = previous;
                                                this.request_groups(cx);
                                            }
                                        })),
                                )
                                .child(
                                    Button::new("groups-next")
                                        .small()
                                        .ghost()
                                        .label(self.t("下一页", "Next"))
                                        .disabled(
                                            self.loading_rows || self.group_next_cursor.is_none(),
                                        )
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            let Some(next) = this.group_next_cursor.clone() else {
                                                return;
                                            };
                                            this.group_cursor_stack
                                                .push(this.group_current_cursor.clone());
                                            this.group_current_cursor = Some(next);
                                            this.group_next_cursor = None;
                                            let _ = group_by;
                                            this.request_groups(cx);
                                        })),
                                )
                            })
                            .when(self.tab != Tab::Requests, |this| {
                                this.child(
                                    Button::new("group-page-size")
                                        .small()
                                        .ghost()
                                        .label(format!(
                                            "{} {}",
                                            self.page_size,
                                            self.t("条/页", "per page")
                                        ))
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.page_size = match this.page_size {
                                                20 => 50,
                                                50 => 100,
                                                _ => 20,
                                            };
                                            this.reset_group_pagination();
                                            this.groups_gate.invalidate();
                                            this.request_groups(cx);
                                        })),
                                )
                            })
                            .when(self.tab == Tab::Requests, |this| {
                                this.child(
                                    Button::new("page-prev")
                                        .small()
                                        .ghost()
                                        .label(self.t("上一页", "Previous"))
                                        .disabled(
                                            self.loading_rows || !self.rows_cursors.can_go_back(),
                                        )
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            // The stack holds the start cursor of each
                                            // page, so popping restores exactly the
                                            // previous page (1 -> 2 -> 3 -> 2 -> 1).
                                            if let Some(previous) = this.rows_cursors.back() {
                                                this.rows_cursors.begin_page(previous);
                                                this.request_rows(cx);
                                            }
                                        })),
                                )
                                .child(
                                    Button::new("page-size")
                                        .small()
                                        .ghost()
                                        .label(format!(
                                            "{} {}",
                                            self.page_size,
                                            self.t("条/页", "per page")
                                        ))
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.page_size = match this.page_size {
                                                20 => 50,
                                                50 => 100,
                                                _ => 20,
                                            };
                                            this.reset_pagination();
                                            this.request_rows(cx);
                                            cx.notify();
                                        })),
                                )
                                .child(
                                    Button::new("page-next")
                                        .small()
                                        .ghost()
                                        .label(self.t("下一页", "Next"))
                                        .disabled(
                                            self.loading_rows || !self.rows_cursors.can_go_next(),
                                        )
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            if let Some(cursor) = this.rows_cursors.forward() {
                                                this.rows_cursors.begin_page(cursor);
                                                this.request_rows(cx);
                                            }
                                        })),
                                )
                            }),
                    ),
            )
            .child(
                div()
                    .id("stats-table-host")
                    .w_full()
                    .h(px(400.))
                    .min_h_0()
                    .overflow_hidden()
                    .child(table),
            )
            .into_any_element()
    }
}

impl Render for Statistics {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.render_page(cx)
    }
}

impl Statistics {
    pub fn render_page(&mut self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .id("statistics-scroll")
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .overflow_y_scroll()
            .p_4()
            .gap_4()
            .bg(rgb(BG))
            .child(self.header(cx))
            .child(self.health_bar(cx))
            .child(self.filters(cx))
            .children(self.warnings())
            .child(self.overview(cx))
            .child(self.trends(cx))
            .child(self.details(cx))
            .child(self.pricing_section(cx))
            .child(self.storage_section(cx))
            .into_any_element()
    }
}

impl Statistics {
    /// Collection health as reported by the core.
    ///
    /// A successful query only proves the control API answered; the badge,
    /// database size and loss scope come from the core's own snapshot so a
    /// paused, degraded or lossy collection is never hidden behind a working
    /// page.
    fn health_bar(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(health) = self.health.as_ref() else {
            return div().into_any_element();
        };
        let (label, color) = match health.state {
            HealthState::Ready if health.enabled => (self.t("采集中", "Collecting"), OK),
            HealthState::Ready => (self.t("已暂停", "Paused"), MUTED),
            HealthState::Degraded => (self.t("降级运行", "Degraded"), WARN),
            HealthState::Migrating => (self.t("迁移中", "Migrating"), WARN),
            HealthState::Disabled => (self.t("已停用", "Disabled"), MUTED),
        };
        let mut facts = vec![
            format!("{} {}", self.t("记录", "rows"), health.record_count),
            format!(
                "{} {:.1} MiB",
                self.t("数据库", "database"),
                health.db_bytes as f64 / (1024.0 * 1024.0)
            ),
        ];
        if health.queue_depth > 0 || health.pending_requests > 0 {
            facts.push(format!(
                "{} {}/{}",
                self.t("队列", "queue"),
                health.queue_depth,
                health.queue_capacity
            ));
        }
        if let Some(earliest) = health.earliest_record_at_ms {
            facts.push(format!(
                "{} {}",
                self.t("最早记录", "earliest"),
                crate::usage_table::timestamp(earliest, self.zh)
            ));
        }
        let mut alerts: Vec<String> = Vec::new();
        if !health.enabled {
            alerts.push(self.t("采集已关闭", "collection is off").to_owned());
        }
        match health.loss_scope {
            LossScope::LossObserved => alerts.push(
                self.t("本次运行有丢弃", "requests were dropped in this run")
                    .to_owned(),
            ),
            LossScope::UnknownAfterUncleanExit => alerts.push(
                self.t(
                    "上次运行未正常退出",
                    "the previous run did not stop cleanly",
                )
                .to_owned(),
            ),
            LossScope::NoLossObservedInCurrentRun => {}
        }
        if health.write_failures_current_run > 0 {
            alerts.push(format!(
                "{} {}",
                self.t("写入失败", "write failures"),
                health.write_failures_current_run
            ));
        }
        if let Some(code) = health.error_code.as_ref() {
            alerts.push(code.clone());
        }
        let _ = cx;
        div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_3()
            .px_3()
            .py_2()
            .rounded_md()
            .bg(rgb(PANEL))
            .child(
                div()
                    .px_2()
                    .py_1()
                    .rounded_sm()
                    .bg(rgb(color))
                    .child(Label::new(label).text_size(px(11.)).text_color(rgb(BG))),
            )
            .children(
                facts
                    .into_iter()
                    .map(|fact| Label::new(fact).text_size(px(11.)).text_color(rgb(MUTED))),
            )
            .children(
                alerts
                    .into_iter()
                    .map(|alert| Label::new(alert).text_size(px(11.)).text_color(rgb(WARN))),
            )
            .into_any_element()
    }
}

/// P1 sections live in the same file but stay collapsed by default.
impl Statistics {
    fn section_toggle(
        &self,
        id: &'static str,
        title: &'static str,
        open: bool,
        cx: &mut Context<Self>,
        which: u8,
    ) -> AnyElement {
        Button::new(id)
            .small()
            .ghost()
            .icon(if open {
                IconName::ChevronDown
            } else {
                IconName::ChevronRight
            })
            .label(title)
            .on_click(cx.listener(move |this, _, _, cx| {
                match which {
                    0 => this.pricing_open = !this.pricing_open,
                    _ => this.storage_open = !this.storage_open,
                }
                cx.notify();
            }))
            .into_any_element()
    }

    fn pricing_section(&self, cx: &mut Context<Self>) -> AnyElement {
        let open = self.pricing_open;
        let mut body = div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                Label::new(self.t(
                    "按当前供应商筛选；输入精确模型ID搜索价格",
                    "Prices follow provider filter; search by exact model ID",
                ))
                .text_size(px(11.)),
            )
            .child(Input::new(&self.price_search).w(px(240.)));
        body = body.child(
            div()
                .flex()
                .gap_2()
                .child(
                    Button::new("price-add-top")
                        .small()
                        .primary()
                        .label(self.t("新建价格规则", "New price rule"))
                        .on_click(cx.listener(|this, _, window, cx| this.new_price(window, cx))),
                )
                .child(
                    Button::new("price-reload")
                        .small()
                        .ghost()
                        .label(self.t("刷新价格", "Reload prices"))
                        .on_click(cx.listener(|this, _, _, cx| this.request_prices(cx))),
                ),
        );
        if self.prices_loading {
            body = body.child(Label::new(self.t("加载中…", "Loading…")).text_size(px(11.)));
        } else if self.prices.is_empty() {
            body = body.child(
                Label::new(self.t(
                    "还没有价格规则。缺少模型价格时按默认币种记录0元，历史快照不受以后改价影响。",
                    "No price rules yet. Missing model prices record zero in the default currency.",
                ))
                .text_size(px(11.))
                .text_color(rgb(MUTED)),
            );
        } else {
            for (index, rule) in self.prices.iter().enumerate() {
                body = body.child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap_3()
                        .text_size(px(11.))
                        .child(
                            Label::new(format!(
                                "{} / {} · {} · {}",
                                rule.provider_id,
                                rule.upstream_model,
                                rule.label,
                                rule.currency.as_str()
                            ))
                            .text_color(rgb(INK)),
                        )
                        .child(
                            Label::new(format!(
                                "{} / {} · {}",
                                usage_table::amount(
                                    Some(&rule.input_rate_nanos),
                                    Some(rule.currency)
                                ),
                                usage_table::amount(
                                    Some(&rule.output_rate_nanos),
                                    Some(rule.currency)
                                ),
                                rule.input_mode
                            ))
                            .text_color(rgb(MUTED)),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_1()
                                .child(
                                    Button::new(("price-edit", index))
                                        .small()
                                        .ghost()
                                        .label(self.t("编辑", "Edit"))
                                        .on_click(cx.listener({
                                            let rule = rule.clone();
                                            move |this, _, window, cx| {
                                                this.edit_price(&rule, window, cx);
                                            }
                                        })),
                                )
                                .child(
                                    Button::new(("price-delete", index))
                                        .small()
                                        .ghost()
                                        .label(self.t("删除", "Delete"))
                                        .on_click(cx.listener({
                                            let (price_id, revision) =
                                                (rule.rule_id.clone(), rule.revision);
                                            move |this, _, _, cx| {
                                                this.delete_price(price_id.clone(), revision, cx);
                                            }
                                        })),
                                ),
                        ),
                );
            }
        }
        body = body.child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    Label::new(format!(
                        "{} {} · {} {}",
                        self.t("第", "Page"),
                        self.prices_cursors.page_number(),
                        self.prices.len(),
                        self.t("条", "rows")
                    ))
                    .text_size(px(11.)),
                )
                .child(
                    Button::new("prices-prev")
                        .small()
                        .ghost()
                        .label(self.t("上一页", "Previous"))
                        .disabled(self.prices_loading || !self.prices_cursors.can_go_back())
                        .on_click(cx.listener(|this, _, _, cx| {
                            if let Some(previous) = this.prices_cursors.back() {
                                this.prices_cursors.begin_page(previous);
                                this.request_price_page(format!("prices-{}", this.generation), cx);
                            }
                        })),
                )
                .child(
                    Button::new("prices-page-size")
                        .small()
                        .ghost()
                        .label(format!(
                            "{} {}",
                            self.prices_page_size,
                            self.t("条/页", "per page")
                        ))
                        .disabled(self.prices_loading)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.prices_page_size = match this.prices_page_size {
                                20 => 50,
                                50 => 100,
                                _ => 20,
                            };
                            this.request_prices(cx);
                        })),
                )
                .child(
                    Button::new("prices-next")
                        .small()
                        .ghost()
                        .label(self.t("下一页", "Next"))
                        .disabled(self.prices_loading || !self.prices_cursors.can_go_next())
                        .on_click(cx.listener(|this, _, _, cx| {
                            if let Some(next) = this.prices_cursors.forward() {
                                this.prices_cursors.begin_page(next);
                                this.request_price_page(format!("prices-{}", this.generation), cx);
                            }
                        })),
                ),
        );
        // Repricing history with the current price was removed: every request
        // keeps the snapshot it was admitted with.
        stats_card("stats-pricing")
            .child(self.section_toggle(
                "pricing-toggle",
                self.t("成本定价", "Cost pricing"),
                open,
                cx,
                0,
            ))
            .when(open, |this| this.child(body))
            .into_any_element()
    }

    fn storage_section(&self, cx: &mut Context<Self>) -> AnyElement {
        let open = self.storage_open;
        let settings = self.settings.clone();
        let (enabled, retention, revision) = settings.as_ref().map_or((true, 180, 1), |settings| {
            (settings.enabled, settings.retention_days, settings.revision)
        });
        let progress = self
            .maintenance
            .clone()
            .filter(|task| task.status == MaintenanceStatus::Running);
        let maintenance_label = self.t("维护任务", "Maintenance");
        let rows_label = self.t("行已处理", "rows processed");
        let body = div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        Label::new(self.t(
                            "缺价默认币种（仅新请求）",
                            "Default currency for new unpriced requests",
                        ))
                        .text_size(px(11.)),
                    )
                    .children([Currency::Cny, Currency::Usd].into_iter().map(|currency| {
                        Button::new((
                            "stats-default-currency",
                            if currency == Currency::Cny {
                                0usize
                            } else {
                                1usize
                            },
                        ))
                        .small()
                        .outline()
                        .label(currency.as_str())
                        .disabled(
                            settings
                                .as_ref()
                                .is_none_or(|s| s.default_currency == currency),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.save_default_currency(currency, cx)
                        }))
                    })),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(
                        Checkbox::new("collect-enabled")
                            .label(self.t("启用摘要采集", "Enable summary collection"))
                            .checked(enabled)
                            .small()
                            .on_change(cx.listener(move |this, value: &bool, _, cx| {
                                this.update_settings(*value, retention, revision, cx);
                            })),
                    )
                    .child(
                        Label::new(self.t(
                            "暂停只影响新请求，已接纳的请求仍会结算",
                            "Pausing only stops new requests; admitted ones still settle",
                        ))
                        .text_size(px(11.))
                        .text_color(rgb(MUTED)),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        Label::new(self.t("保留天数", "Retention days"))
                            .text_size(px(11.))
                            .text_color(rgb(MUTED)),
                    )
                    .child(
                        Button::new("retention-30")
                            .small()
                            .ghost()
                            .label("30")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.update_settings(enabled, 30, revision, cx)
                            })),
                    )
                    .child(
                        Button::new("retention-90")
                            .small()
                            .ghost()
                            .label("90")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.update_settings(enabled, 90, revision, cx)
                            })),
                    )
                    .child(
                        Button::new("retention-180")
                            .small()
                            .ghost()
                            .label("180")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.update_settings(enabled, 180, revision, cx)
                            })),
                    )
                    .child(
                        Button::new("retention-365")
                            .small()
                            .ghost()
                            .label("365")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.update_settings(enabled, 365, revision, cx)
                            })),
                    )
                    .child(
                        Button::new("retention-forever")
                            .small()
                            .ghost()
                            .label(self.t("永久", "Forever"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.update_settings(enabled, 0, revision, cx)
                            })),
                    )
                    .child(
                        Label::new(format!(
                            "{} {} · {} {}",
                            self.t("当前", "now"),
                            retention,
                            self.t("天", "days"),
                            self.t("(0=永久)", "(0=forever)")
                        ))
                        .text_size(px(11.))
                        .text_color(rgb(MUTED)),
                    ),
            )
            .child(
                div().flex().items_center().gap_2().child(
                    Button::new("provider-modes-refresh")
                        .small()
                        .ghost()
                        .icon(IconName::RefreshCw)
                        .label(self.t("刷新供应商采集模式", "Reload provider modes"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            for (id, _) in this
                                .configured_providers
                                .iter()
                                .skip(this.storage_page * this.storage_page_size)
                                .take(this.storage_page_size)
                            {
                                this.provider_modes.remove(id);
                            }
                            this.request_provider_modes(cx);
                            cx.notify();
                        })),
                ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        Label::new(format!(
                            "{} {}/{}",
                            self.t("第", "Page"),
                            self.storage_page + 1,
                            self.configured_providers
                                .len()
                                .div_ceil(self.storage_page_size)
                                .max(1)
                        ))
                        .text_size(px(11.)),
                    )
                    .child(
                        Button::new("storage-prev")
                            .small()
                            .ghost()
                            .label(self.t("上一页", "Previous"))
                            .disabled(self.storage_page == 0)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.storage_page = this.storage_page.saturating_sub(1);
                                this.request_provider_modes(cx);
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("storage-page-size")
                            .small()
                            .ghost()
                            .label(format!(
                                "{} {}",
                                self.storage_page_size,
                                self.t("条/页", "per page")
                            ))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.storage_page_size = match this.storage_page_size {
                                    20 => 50,
                                    50 => 100,
                                    _ => 20,
                                };
                                this.storage_page = 0;
                                this.request_provider_modes(cx);
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("storage-next")
                            .small()
                            .ghost()
                            .label(self.t("下一页", "Next"))
                            .disabled(
                                (self.storage_page + 1) * self.storage_page_size
                                    >= self.configured_providers.len(),
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.storage_page += 1;
                                this.request_provider_modes(cx);
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div().flex().flex_col().gap_1().children(
                    self.configured_providers
                        .iter()
                        .enumerate()
                        .skip(self.storage_page * self.storage_page_size)
                        .take(self.storage_page_size)
                        .map(|(index, (key, name))| {
                            let id = key.clone();
                            let settings = self.provider_modes.get(&id).cloned();
                            let mode = settings
                                .as_ref()
                                .map_or(UsageMode::Observe, |settings| settings.usage_mode);
                            let revision =
                                settings.as_ref().map_or(0, |settings| settings.revision);
                            let label = if name.is_empty() {
                                key.clone()
                            } else {
                                name.clone()
                            };
                            div()
                                .flex()
                                .items_center()
                                .justify_between()
                                .gap_3()
                                .child(Label::new(label).text_size(px(11.)).text_color(rgb(INK)))
                                .child(
                                    Checkbox::new(("provider-mode", index))
                                        .label(self.t(
                                            "请求上游返回用量（request）",
                                            "Request upstream usage (request mode)",
                                        ))
                                        .checked(mode == UsageMode::Request)
                                        .disabled(
                                            settings.is_none()
                                                || self
                                                    .backend
                                                    .is_busy(&JobKey::ProviderMode(id.clone())),
                                        )
                                        .small()
                                        .on_change(cx.listener(
                                            move |this, value: &bool, _, cx| {
                                                this.set_provider_mode(
                                                    id.clone(),
                                                    *value,
                                                    revision,
                                                    cx,
                                                );
                                            },
                                        )),
                                )
                                .into_any_element()
                        }),
                ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        Button::new("clear-preview")
                            .small()
                            .danger()
                            .outline()
                            .label(self.t("清除历史统计…", "Clear history…"))
                            .on_click(cx.listener(|this, _, _, cx| this.preview_clear(cx))),
                    )
                    .when_some(progress, |this, task| {
                        this.child(
                            Label::new(format!(
                                "{} {} · {} {}",
                                maintenance_label, task.kind, task.affected_rows, rows_label
                            ))
                            .text_size(px(11.))
                            .text_color(rgb(MUTED)),
                        )
                    }),
            )
            .when_some(self.clear_preview.clone(), |this, preview| {
                let token = preview.confirmed;
                this.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            Label::new(format!(
                                "{} {}",
                                if token {
                                    self.t("将删除已结束的", "Deletes finished")
                                } else {
                                    self.t("预览：将删除", "Preview: would delete")
                                },
                                preview.row_count
                            ))
                            .text_size(px(11.))
                            .text_color(rgb(0xa4424a)),
                        )
                        .child(
                            Button::new("clear-confirm")
                                .small()
                                .danger()
                                .label(self.t("确认清除", "Confirm clear"))
                                .on_click(cx.listener(|this, _, _, cx| this.confirm_clear(cx))),
                        ),
                )
            });
        stats_card("stats-storage")
            .child(self.section_toggle(
                "storage-toggle",
                self.t("采集与存储", "Collection & storage"),
                open,
                cx,
                1,
            ))
            .when(open, |this| this.child(body))
            .into_any_element()
    }
}

impl Statistics {
    /// Opens the native price form.
    pub(crate) fn open_price_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.price_open = true;
        // The dialog's own provider list must be current before it is shown.
        self.sync_price_providers(window, cx);
        let owner = cx.entity().downgrade();
        let observer = cx.entity().clone();
        let body = cx.new(|cx| {
            cx.observe(&observer, |_, _, cx| cx.notify()).detach();
            PriceBody {
                owner: owner.clone(),
            }
        });
        window.open_dialog(cx, move |dialog, _, _| {
            let close_owner = owner.clone();
            let save_owner = owner.clone();
            dialog
                .w(px(520.))
                .close_button(true)
                .child(body.clone())
                .on_ok(move |_, window, cx| {
                    // Enter submits exactly like the Save button; the dialog
                    // closes only when the core confirms this draft.
                    let _ = save_owner.update(cx, |this, cx| this.save_price(cx));
                    let _ = &window;
                    false
                })
                .on_close(move |_, window, cx| {
                    let _ = close_owner.update(cx, |this, cx| {
                        // X and Escape cancel the draft; a programmatic close
                        // (a confirmed save) only clears it.
                        this.price_closing = false;
                        this.cancel_price(window, cx)
                    });
                })
        });
    }

    fn price_body(&self, cx: &mut Context<Self>) -> AnyElement {
        DialogContent::new()
            .w_full()
            .gap_3()
            .child(div().text_lg().font_weight(FontWeight::SEMIBOLD).child(
                if self.price_draft.is_editing() {
                    self.t("编辑价格", "Edit price")
                } else {
                    self.t("新建价格规则", "New price rule")
                },
            ))
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(rgb(MUTED))
                    .child(self.t(
                        "价格是每百万 Token 的估算单价，不是实际账单",
                        "Prices are per-million estimates, not a real invoice",
                    )),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(self.filter_label(self.t("供应商", "Provider")))
                    .child(
                        Select::new(&self.price_provider)
                            .w_full()
                            .small()
                            .disabled(self.price_draft.identity_locked()),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(self.filter_label(if self.price_draft.identity_locked() {
                        self.t(
                            "上游模型 ID（编辑时不可更改）",
                            "Upstream model ID (fixed while editing)",
                        )
                    } else {
                        self.t("上游模型 ID", "Upstream model ID")
                    }))
                    .child(
                        Input::new(&self.price_model).disabled(self.price_draft.identity_locked()),
                    ),
            )
            .child(
                div()
                    .flex()
                    .gap_3()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .flex_1()
                            .child(self.filter_label(self.t("计价方式", "Pricing mode")))
                            .child(Select::new(&self.price_mode).w_full().small()),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .w(px(120.))
                            .child(self.filter_label(self.t("币种", "Currency")))
                            .child(Select::new(&self.price_currency).w_full().small()),
                    ),
            )
            .child(
                div()
                    .flex()
                    .gap_3()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .flex_1()
                            .child(self.filter_label(self.t("输入价 / 百万", "Input / million")))
                            .child(Input::new(&self.price_input)),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .flex_1()
                            .child(self.filter_label(self.t("输出价 / 百万", "Output / million")))
                            .child(Input::new(&self.price_output)),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .flex_1()
                            .child(
                                self.filter_label(
                                    self.t("缓存读价 / 百万", "Cache read / million"),
                                ),
                            )
                            .child(Input::new(&self.price_read)),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .flex_1()
                            .child(
                                self.filter_label(
                                    self.t("缓存写价 / 百万", "Cache write / million"),
                                ),
                            )
                            .child(Input::new(&self.price_write)),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(self.filter_label(self.t("备注", "Label")))
                    .child(Input::new(&self.price_label)),
            )
            .child(
                Label::new(self.t(
                    "保存后只影响新请求；每个请求保留自己的单价快照",
                    "Saving only affects new requests; each request keeps its own price snapshot",
                ))
                .text_size(px(11.))
                .text_color(rgb(MUTED)),
            )
            .when_some(self.price_draft.error.clone(), |this, error| {
                this.child(
                    Label::new(error)
                        .text_size(px(11.))
                        .text_color(rgb(0xa4424a)),
                )
            })
            .child(
                DialogFooter::new().child(
                    Button::new("price-save")
                        .primary()
                        .label(if self.price_draft.pending {
                            self.t("保存中…", "Saving…")
                        } else {
                            self.t("保存", "Save")
                        })
                        // A second submit while the first is in flight would
                        // race two revisions against the same row.
                        .disabled(self.price_draft.pending)
                        .on_click(cx.listener(|this, _, _, cx| this.save_price(cx))),
                ),
            )
            .into_any_element()
    }
    fn save_price(&mut self, cx: &mut Context<Self>) {
        if self.price_draft.pending {
            return;
        }
        // The inputs first flow into the draft; the payload is built from the
        // draft alone, so no field can silently fall back to a filter value.
        self.collect_price_draft(cx);
        if !self.price_draft.is_complete() {
            self.price_draft.error = Some(
                self.t("请选择供应商并填写模型", "Choose a provider and a model")
                    .to_owned(),
            );
            cx.notify();
            return;
        }
        let tag = format!("pricesave-{}", self.price_draft.generation);
        let payload = match self.price_draft.submit(tag.clone()) {
            Ok(payload) => payload,
            Err(detail) => {
                // The draft already carries the message and the typed values.
                self.error = Some(detail);
                cx.notify();
                return;
            }
        };
        let request = crate::stats_backend::command("stats_price_save", &tag, payload);
        // The dialog stays open until the core confirms the save, so a failure
        // keeps the typed values in place for correction.
        if let Err(detail) = self
            .backend
            .submit_write(JobKind::PriceSave, tag.clone(), request)
        {
            // The write never left, so the draft must not stay "saving": the
            // user has to be able to retry without reopening the form.
            self.price_draft.pending = false;
            self.price_draft.tag = None;
            let prefix = self
                .t("统计服务忙，请稍后重试", "Statistics are busy; try again")
                .to_owned();
            let message = format!("{prefix} ({detail})");
            self.price_draft.error = Some(message.clone());
            self.error = Some(message);
            cx.notify();
            return;
        }
        cx.notify();
    }

    fn save_default_currency(&mut self, currency: Currency, cx: &mut Context<Self>) {
        let Some(settings) = &self.settings else {
            return;
        };
        let tag = format!("settings-currency-{}", settings.revision);
        let request = crate::stats_backend::command(
            "stats_settings_update",
            &tag,
            json!({
            "expected_revision":settings.revision,"default_currency":currency.as_str(),
            "enabled":settings.enabled,"retention_days":settings.retention_days}),
        );
        if let Err(error) = self
            .backend
            .submit_write(JobKind::SettingsUpdate, tag, request)
        {
            self.error = Some(error);
        }
        cx.notify();
    }

    fn update_settings(
        &mut self,
        enabled: bool,
        retention: i64,
        revision: i64,
        cx: &mut Context<Self>,
    ) {
        let tag = format!("settings-{}", self.generation);
        let request = crate::stats_backend::command(
            "stats_settings_update",
            &tag,
            json!({
                "enabled": enabled,
                "retention_days": retention,
                "expected_revision": revision,
            }),
        );
        if let Err(detail) = self
            .backend
            .submit_write(JobKind::SettingsUpdate, tag, request)
        {
            self.error = Some(
                self.t(
                    "设置未能提交，请稍后重试",
                    "The setting was not sent; try again",
                )
                .to_owned()
                    + " "
                    + &detail,
            );
        }
        cx.notify();
    }

    /// Opens the dialog pre-filled with an existing price row.
    fn edit_price(
        &mut self,
        rule: &crate::stats_dto::PriceRuleDto,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.draft_generation += 1;
        // A fresh draft that owns every field. The report filters are untouched:
        // editing a price must not change what the page is reporting on.
        self.price_draft =
            crate::stats_controller::PriceDraft::editing(rule, self.draft_generation);
        self.sync_price_draft(window, cx, true);
        self.open_price_dialog(window, cx);
    }

    /// Starts a new price owned by `provider`, without touching the filters.
    fn new_price(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.draft_generation += 1;
        self.price_draft = crate::stats_controller::PriceDraft::blank(
            self.provider.clone(),
            self.draft_generation,
        );
        self.price_draft.currency = self
            .settings
            .as_ref()
            .map_or(Currency::Cny, |s| s.default_currency);
        self.sync_price_draft(window, cx, true);
        self.open_price_dialog(window, cx);
    }

    /// Closes the dialog. The draft is discarded and nothing else changes.
    fn cancel_price(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.price_open = false;
        self.price_draft = crate::stats_controller::PriceDraft::blank(String::new(), 0);
        self.price_draft.error = None;
        self.sync_price_draft(window, cx, false);
        cx.notify();
    }

    /// Keeps the dialog's provider select in step with the loaded suppliers.
    /// The provider of the draft being edited is always present, even when it
    /// only survives in historical rows and is not in the first page of options.
    fn sync_price_providers(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut keys: Vec<String> = self
            .configured_providers
            .iter()
            .map(|(key, _)| key.clone())
            .collect();
        for option in &self.provider_options {
            if !keys.contains(&option.key) {
                keys.push(option.key.clone());
            }
        }
        let draft_provider = self.price_draft.provider_id.trim().to_owned();
        if !draft_provider.is_empty() && !keys.iter().any(|key| key == &draft_provider) {
            keys.push(draft_provider.clone());
        }
        let mut items = vec![choice(
            "",
            if self.zh {
                "选择供应商"
            } else {
                "Select a provider"
            },
        )];
        for key in keys {
            let label = self
                .provider_options
                .iter()
                .find(|option| option.key == key)
                .map(|option| option.label.clone())
                .or_else(|| {
                    self.configured_providers
                        .iter()
                        .find(|(id, _)| id == &key)
                        .map(|(_, name)| name.clone())
                })
                .unwrap_or_else(|| key.clone());
            items.push(choice(key, label));
        }
        let wanted = if draft_provider.is_empty() {
            None
        } else {
            Some(draft_provider)
        };
        self.price_provider.update(cx, |state, cx| {
            state.set_items(items, window, cx);
            if let Some(wanted) = wanted {
                state.set_selected_value(&wanted, window, cx);
            }
        });
    }

    /// Copies the draft into the inputs (and selects). `fill` also resets every
    /// field, so reopening a price cannot inherit the previous draft's values.
    fn sync_price_draft(&mut self, window: &mut Window, cx: &mut Context<Self>, fill: bool) {
        if !fill {
            return;
        }
        let draft = self.price_draft.clone();
        let fields = [
            (&self.price_input, draft.input.clone()),
            (&self.price_output, draft.output.clone()),
            (&self.price_read, draft.cache_read.clone()),
            (&self.price_write, draft.cache_write.clone()),
        ];
        for (field, value) in fields {
            field.update(cx, |state, cx| state.set_value(value, window, cx));
        }
        self.price_model.update(cx, |state, cx| {
            state.set_value(draft.upstream_model.clone(), window, cx)
        });
        self.price_label.update(cx, |state, cx| {
            state.set_value(draft.label.clone(), window, cx)
        });
        self.price_mode.update(cx, |state, cx| {
            state.set_selected_value(&draft.input_mode.as_str().to_owned(), window, cx);
        });
        self.price_currency.update(cx, |state, cx| {
            state.set_selected_value(&draft.currency.as_str().to_owned(), window, cx);
        });
        let provider = draft.provider_id.clone();
        self.price_provider.update(cx, |state, cx| {
            state.set_selected_value(&provider, window, cx);
        });
    }

    /// Reads the inputs back into the draft. Every visible field maps to the
    /// draft; nothing reads or writes the report filters.
    fn collect_price_draft(&mut self, cx: &mut Context<Self>) {
        let text = |field: &Entity<InputState>, cx: &Context<Self>| {
            field.read(cx).value().trim().to_owned()
        };
        let input = text(&self.price_input, cx);
        let output = text(&self.price_output, cx);
        let cache_read = text(&self.price_read, cx);
        let cache_write = text(&self.price_write, cx);
        let model = text(&self.price_model, cx);
        let label = text(&self.price_label, cx);
        let mode = self.price_mode.read(cx).selected_value().cloned();
        let currency = self.price_currency.read(cx).selected_value().cloned();
        let provider = self.price_provider.read(cx).selected_value().cloned();
        let draft = &mut self.price_draft;
        draft.input = input;
        draft.output = output;
        draft.cache_read = cache_read;
        draft.cache_write = cache_write;
        draft.upstream_model = model;
        draft.label = label;
        if let Some(mode) = mode.as_deref().and_then(crate::stats_dto::InputMode::parse) {
            draft.input_mode = mode;
        }
        if let Some(currency) = currency
            .as_deref()
            .and_then(crate::stats_dto::Currency::parse)
        {
            draft.currency = currency;
        }
        if let Some(provider) = provider
            && !provider.is_empty()
        {
            draft.provider_id = provider;
        }
    }

    /// Deleting the current price only changes what *new* requests will use.
    fn delete_price(&mut self, price_id: String, revision: i64, cx: &mut Context<Self>) {
        let tag = format!("pricedelete-{}", self.generation);
        let request = crate::stats_backend::command(
            "stats_price_delete",
            &tag,
            json!({"rule_id": price_id, "expected_revision": revision}),
        );
        match self
            .backend
            .submit_write(JobKind::PriceDelete, tag, request)
        {
            Ok(_) => {
                self.error = None;
                self.request_prices(cx);
            }
            Err(detail) => self.error = Some(detail),
        }
        cx.notify();
    }

    fn preview_clear(&mut self, cx: &mut Context<Self>) {
        let tag = format!("clear-{}", self.generation);
        let request = crate::stats_backend::command(
            "stats_clear_preview",
            &tag,
            json!({"before_ms": now_ms()}),
        );
        self.backend.submit_read(JobKey::ClearPreview, tag, request);
        cx.notify();
    }

    fn confirm_clear(&mut self, cx: &mut Context<Self>) {
        if self.clear_preview.is_none() {
            return;
        }
        // The preview carries a short-lived token; the core refuses to delete
        // without it, so a stale or missing preview cannot wipe history.
        let Some(token) = self.clear_token.take() else {
            self.preview_clear(cx);
            return;
        };
        let tag = format!("maintenance-{}", self.generation);
        let request =
            crate::stats_backend::command("stats_clear_start", &tag, json!({"token": token}));
        if let Err(detail) = self.backend.submit_write(JobKind::ClearStart, tag, request) {
            self.error = Some(detail);
            cx.notify();
            return;
        }
        self.clear_preview = None;
        cx.notify();
    }

    /// Polls a running maintenance task; called from `tick` while visible.
    pub fn poll_maintenance(&mut self, cx: &mut Context<Self>) {
        let Some(task) = self.maintenance.clone() else {
            return;
        };
        if task.status != MaintenanceStatus::Running {
            return;
        }
        let tag = format!("maintenance-{}", self.generation);
        let request = crate::stats_backend::command(
            "stats_maintenance_status",
            &tag,
            json!({"task_id": task.task_id}),
        );
        self.backend.submit_read(JobKey::Maintenance, tag, request);
        let _ = cx;
    }

    /// Fetches the usage mode of one still-unknown provider.
    ///
    /// Requesting them all at once used to queue one job per provider and then
    /// silently keep only the last; instead this walks the list one provider per
    /// tick, so nothing is lost and the bounded queue is never flooded.
    fn request_provider_modes(&mut self, cx: &mut Context<Self>) {
        let Some((id, _)) = self
            .configured_providers
            .iter()
            .skip(self.storage_page * self.storage_page_size)
            .take(self.storage_page_size)
            .find(|(id, _)| {
                !self.provider_modes.contains_key(id)
                    && !self.backend.is_busy(&JobKey::ProviderMode(id.clone()))
            })
            .cloned()
        else {
            return;
        };
        let tag = format!("providersettings-{id}");
        let request = crate::stats_backend::command(
            "stats_provider_settings_get",
            &tag,
            json!({"provider_id":id}),
        );
        self.backend
            .submit_read(JobKey::ProviderMode(id), tag, request);
        let _ = cx;
    }

    /// Switches one provider between observe and request mode.
    fn set_provider_mode(
        &mut self,
        provider_id: String,
        request_mode: bool,
        revision: i64,
        cx: &mut Context<Self>,
    ) {
        let tag = format!("providersettings-{provider_id}");
        let request = crate::stats_backend::command(
            "stats_provider_settings_update",
            &tag,
            json!({
                "provider_id": provider_id,
                "usage_mode": if request_mode { "request" } else { "observe" },
                "expected_revision": revision,
            }),
        );
        if let Err(detail) = self
            .backend
            .submit_write(JobKind::ProviderMode, tag, request)
        {
            self.error = Some(
                self.t(
                    "供应商设置未能提交，请稍后重试",
                    "The provider setting was not sent; try again",
                )
                .to_owned()
                    + " "
                    + &detail,
            );
        }
        cx.notify();
    }
}

/// Dialog body for the price form.
struct PriceBody {
    owner: WeakEntity<Statistics>,
}

impl Render for PriceBody {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.owner
            .update(cx, |this, cx| this.price_body(cx))
            .unwrap_or_else(|_| div().into_any_element())
    }
}
