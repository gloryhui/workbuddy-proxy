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
        BreakdownDto, ClearPreviewDto, CostStatus, Currency, DashboardDto, ERR_MAINTENANCE_FAILED,
        FilterOptionDto, GroupBy, HealthDto, MaintenanceDto, MaintenanceStatus, PriceRuleDto,
        ProviderSettingsDto, RequestCursor, RequestDetailDto, RequestRowDto, SettingsDto,
        SortMetric, UsageMode, WarningCode,
    },
    stats_dto::{HealthState, LossScope},
    usage_chart::{ChartMode, ChartPoint, SeriesKind, UsagePlot},
    usage_table::{self, GroupRows, UsageRows},
};
mod event_metrics;
mod import_controls;
mod layout;
mod mapping_controls;
mod overview;
mod pagination;
mod preferences;
mod source_controls;

use chrono::{Local, TimeZone as _};
use gpui_kit::component::{
    Disableable, ElementExt, Icon, IconName, Sizable, WindowExt,
    button::{Button, ButtonVariants},
    calendar::{CalendarEvent, CalendarState, Date},
    checkbox::Checkbox,
    input::{InputEvent, InputState},
    label::Label,
    popover::Popover,
    select::{SelectEvent, SelectItem, SelectState},
    table::{TableEvent, TableState},
};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    time::{Duration, Instant},
};

const BG: u32 = crate::theme::color::BG_PAGE;
const INK: u32 = crate::theme::color::TEXT_1;
const MUTED: u32 = crate::theme::color::TEXT_3;
const BORDER: u32 = crate::theme::color::BORDER;
const OK: u32 = crate::theme::color::SUCCESS;
const WARN: u32 = 0xb7791f;
const PANEL: u32 = 0xffffff;

/// Explicit logical-pixel metrics avoid rem-relative Small buttons shrinking
/// below the neighbouring filters on both Windows and macOS.
fn stats_button(id: impl Into<ElementId>) -> Button {
    crate::ui_components::action_button(id).px(px(10.))
}

/// Debounce applied after any filter change before a query is sent.
const DEBOUNCE: Duration = Duration::from_millis(250);

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tab {
    Requests,
    Providers,
    Models,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RangePreset {
    Today,
    Day,
    Week,
    Fortnight,
    Month,
    Quarter,
    Custom,
}

impl RangePreset {
    fn title(self, zh: bool) -> &'static str {
        match (self, zh) {
            (Self::Today, true) => "今天",
            (Self::Today, false) => "Today",
            (Self::Day, true) => "1天",
            (Self::Day, false) => "24 hours",
            (Self::Week, true) => "7天",
            (Self::Week, false) => "7 days",
            (Self::Fortnight, true) => "14天",
            (Self::Fortnight, false) => "14 days",
            (Self::Month, true) => "30天",
            (Self::Month, false) => "30 days",
            (Self::Quarter, true) => "90天",
            (Self::Quarter, false) => "90 days",
            (Self::Custom, true) => "自定义",
            (Self::Custom, false) => "Custom",
        }
    }

    fn bounds(self, now: i64) -> Option<(i64, i64)> {
        let days = match self {
            Self::Today => 0,
            Self::Day => return Some((now - 86_400_000, now)),
            Self::Week => 6,
            Self::Fortnight => 13,
            Self::Month => 29,
            Self::Quarter => 89,
            Self::Custom => return None,
        };
        Some((local_midnight_days_ago(days), now))
    }
}

/// Resolves only the applied selection. Draft edits never reach query builders.
fn resolve_custom_range(range: (i64, i64), follow_now: bool, now: i64) -> (i64, i64) {
    (range.0, if follow_now { now } else { range.1 })
}

fn validate_date_draft(
    from: &str,
    to: &str,
    follow_now: bool,
    now: i64,
) -> Result<(i64, i64), &'static str> {
    crate::stats_controller::validate_range(
        crate::usage_table::parse_local_time(from),
        if follow_now {
            Some(now)
        } else {
            crate::usage_table::parse_local_time(to)
        },
    )
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

fn stats_heading(text: &'static str) -> Div {
    div()
        .text_size(px(13.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(rgb(INK))
        .child(text)
}

/// Card padding from DESIGN_SPEC §3 (16–20). `stats_card` applies it and the
/// filter card's width probe subtracts exactly the same value, so the KPI grid
/// is budgeted against the width it really gets.
const CARD_PAD: f32 = 16.;

fn stats_card(id: &'static str) -> Stateful<Div> {
    // Unlike the model-management `card()`, statistics cards size to their
    // content; only the chart and the table get explicit heights.
    crate::ui_components::content_card(id).p(px(CARD_PAD))
}

pub struct Statistics {
    preferences_dir: PathBuf,
    saved_preferences: preferences::Preferences,
    view: preferences::View,
    summary_tab: Tab,
    preferences_error: bool,
    zh: bool,
    visible: bool,
    backend: StatsScheduler,
    /// Sequence of the newest read intent per kind; a reply that does not match
    /// is stale and must not touch the screen.
    dashboard_seq: u64,
    /// The WorkBuddy import panel: switch, health line and counters.
    import: crate::workbuddy_import::ImportPanel,
    import_ui: import_controls::ImportUi,
    mapping_ui: mapping_controls::MappingUi,
    page_scroll: ScrollHandle,
    mapping_from: Entity<InputState>,
    mapping_to: Entity<InputState>,
    /// The filter generation and the one request it owes.
    coordinator: crate::stats_controller::FilterCoordinator,
    /// The source group (U03): always first in the bar, always exactly one.
    source: crate::source_filter::SourceFilter,
    source_focus: FocusHandle,
    event_metrics: event_metrics::EventMetrics,
    scope_reset_pending: bool,
    /// The advanced capture-method dropdown (U05).
    capture: crate::stats_controller::CaptureFilter,
    /// Sequence of the last import-status read.
    import_seq: u64,
    /// How many more times the page may poll a running scan.
    import_poll: crate::stats_controller::ScanPollBudget,
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
    record_kind: crate::stats_dto::RecordKind,
    kind_select: Entity<ChoiceState>,
    include_tests: bool,
    more_filters_open: bool,
    filters_width: f32,
    date_open: bool,
    refresh_open: bool,
    draft_preset: RangePreset,
    draft_follow_now: bool,
    custom_follow_now: bool,
    draft_text: (String, String),
    calendar: Entity<CalendarState>,
    provider_select: Entity<ChoiceState>,
    model_select: Entity<ChoiceState>,
    outcome_select: Entity<ChoiceState>,
    usage_select: Entity<ChoiceState>,
    capture_select: Entity<ChoiceState>,
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
    filter_key: String,
    tab: Tab,
    page_size: i64,
    page_size_select: Entity<ChoiceState>,
    page_input: Entity<InputState>,
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
    rows_cursors: pagination::Pager<RequestCursor>,
    /// The grouping listing keeps its own cursor chain: sharing the request
    /// cursors made "next" on one tab move the other.
    group_pages: pagination::Pager<crate::stats_dto::BreakdownCursor>,
    group_sort: SortMetric,
    loading_dashboard: bool,
    loading_rows: bool,
    error: Option<String>,
    last_updated: Option<String>,
    currency: Currency,
    visible_series: BTreeSet<SeriesKind>,
    details: Option<RequestDetailDto>,
    detail_open: bool,
    // P1.
    settings: Option<SettingsDto>,
    provider_modes: BTreeMap<String, ProviderSettingsDto>,
    prices: Vec<PriceRuleDto>,
    price_sync: Option<crate::stats_dto::PriceSyncDto>,
    price_sync_pending: bool,
    price_open: bool,
    /// True while the dialog is being closed because a save was confirmed.
    price_closing: bool,
    /// The price being created or edited. It owns its provider and model, so a
    /// draft never touches the report filters.
    price_draft: crate::stats_controller::PriceDraft,
    maintenance: Option<MaintenanceDto>,
    maintenance_error: Option<String>,
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
        let preferences = preferences::Preferences::load(&dir);
        let backend = StatsScheduler::new(dir.clone());
        let page_input = cx.new(|cx| InputState::new(window, cx).placeholder("1"));
        let page_size_select = cx.new(|cx| {
            SelectState::new(
                pagination::PAGE_SIZES
                    .into_iter()
                    .map(|size| {
                        choice(
                            size.to_string(),
                            if zh {
                                format!("{size} 条/页")
                            } else {
                                format!("{size} per page")
                            },
                        )
                    })
                    .collect::<Vec<_>>(),
                None,
                window,
                cx,
            )
        });
        let wake = backend.wake_receiver();
        let provider_search = cx.new(|cx| InputState::new(window, cx));
        let model_search = cx.new(|cx| InputState::new(window, cx));
        let price_search = cx.new(|cx| InputState::new(window, cx));
        let mapping_from =
            cx.new(|cx| InputState::new(window, cx).placeholder("YYYY-MM-DD HH:MM:SS.SSS"));
        let mapping_to =
            cx.new(|cx| InputState::new(window, cx).placeholder("YYYY-MM-DD HH:MM:SS.SSS"));
        let calendar = cx.new(|cx| CalendarState::new(window, cx));
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
        let capture_select =
            cx.new(|cx| SelectState::new(source_controls::capture_choices(zh), None, window, cx));
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
                    available_width: 1134.,
                    custom_widths: [None; usage_table::REQUEST_COLUMNS],
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
                let mut timer =
                    Box::pin(cx.background_executor().timer(Duration::from_millis(250)));
                let mut notified = Box::pin(wake.recv());
                std::future::poll_fn(|context| {
                    use std::future::Future;
                    if notified.as_mut().poll(context).is_ready()
                        || timer.as_mut().poll(context).is_ready()
                    {
                        std::task::Poll::Ready(())
                    } else {
                        std::task::Poll::Pending
                    }
                })
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
            preferences_dir: dir,
            saved_preferences: preferences.clone(),
            view: preferences::View::Details,
            summary_tab: Tab::Providers,
            preferences_error: false,
            zh,
            visible: false,
            backend,
            generation: 0,
            applied: 0,
            debounce_until: None,
            // Refresh the displayed report independently of background collection.
            refresh_interval: 5,
            last_refresh: Instant::now() - Duration::from_secs(3600),
            pending_dashboard: false,
            pending_rows: false,
            range: RangePreset::Week,
            provider: String::new(),
            model: String::new(),
            outcome: String::new(),
            usage: String::new(),
            record_kind: crate::stats_dto::RecordKind::All,
            kind_select: cx.new(|cx| SelectState::new(layout::kind_choices(zh), None, window, cx)),
            include_tests: false,
            more_filters_open: false,
            filters_width: 760.,
            date_open: false,
            refresh_open: false,
            draft_preset: RangePreset::Week,
            draft_follow_now: true,
            custom_follow_now: false,
            draft_text: (String::new(), String::new()),
            calendar,
            provider_select,
            custom_from,
            custom_to,
            model_select,
            outcome_select,
            usage_select,
            capture_select,
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
            filter_key: String::new(),
            tab: Tab::Requests,
            dashboard_seq: 0,
            import: crate::workbuddy_import::ImportPanel::new(),
            import_ui: import_controls::ImportUi::new(),
            mapping_ui: mapping_controls::MappingUi::default(),
            page_scroll: ScrollHandle::new(),
            mapping_from,
            mapping_to,
            coordinator: crate::stats_controller::FilterCoordinator::new(),
            source: crate::source_filter::SourceFilter::new(),
            source_focus: cx.focus_handle().tab_index(0).tab_stop(true),
            event_metrics: event_metrics::EventMetrics::new(),
            scope_reset_pending: false,
            capture: crate::stats_controller::CaptureFilter::new(),
            import_seq: 0,
            import_poll: crate::stats_controller::ScanPollBudget::new(),
            rows_gate: crate::stats_controller::ReplyGate::default(),
            groups_gate: crate::stats_controller::ReplyGate::default(),
            health: None,
            maintenance_poll: Instant::now(),
            page_size: 10,
            page_size_select,
            page_input,
            frozen_range: None,
            custom_range: (local_midnight_days_ago(0), now_ms()),
            rows_cursors: pagination::Pager::default(),
            group_pages: pagination::Pager::default(),
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
            price_sync: None,
            price_sync_pending: false,
            price_open: false,
            price_closing: false,
            price_draft: crate::stats_controller::PriceDraft::blank(String::new(), 0),
            maintenance: None,
            maintenance_error: None,
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
        this.restore_preferences(preferences, window, cx);
        this.subscribe(window, cx);
        this.rebuild_choices(window, cx);
        // The bar opens on its default scope: reconcile the capture dropdown
        // with it and open the first filter generation, which the first
        // dashboard request settles. Reconciling the default is a no-op by
        // construction, which is exactly the point: nothing is hidden yet.
        this.capture.reconcile(this.source_scope());
        this.coordinator.change();
        this
    }

    fn subscribe(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        cx.subscribe_in(&self.page_input, window, |this, _, event, _, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                this.jump_to_input(cx);
            }
        })
        .detach();

        cx.subscribe_in(&self.page_size_select, window, |this, _, event, _, cx| {
            if let SelectEvent::Confirm(Some(code)) = event
                && let Ok(size) = code.parse::<i64>()
                && pagination::PAGE_SIZES.contains(&size)
                && size != this.page_size
            {
                this.page_size = size;
                this.rows_gate.invalidate();
                this.groups_gate.invalidate();
                this.pending_rows = false;
                this.reset_pagination();
                this.request_listing(cx);
                this.persist_preferences();
            }
        })
        .detach();

        cx.on_focus(&self.source_focus, window, |this, _, cx| {
            this.source.set_focused(true);
            cx.notify();
        })
        .detach();
        cx.on_blur(&self.source_focus, window, |this, _, cx| {
            this.source.set_focused(false);
            cx.notify();
        })
        .detach();
        for input in [&self.mapping_from, &self.mapping_to] {
            cx.subscribe(input, |this, _, event, cx| {
                if matches!(event, InputEvent::Change) {
                    this.invalidate_mapping_draft(cx);
                }
            })
            .detach();
        }
        cx.subscribe(&self.usage_table, |_, table, event, cx| {
            if let TableEvent::ColumnWidthsChanged(widths) = event {
                table.update(cx, |table, cx| {
                    table.delegate_mut().resize(widths);
                    table.refresh(cx);
                    cx.notify();
                });
            }
        })
        .detach();
        cx.subscribe_in(&self.calendar, window, |this, _, event, window, cx| {
            let CalendarEvent::Selected(date) = event;
            this.draft_preset = RangePreset::Custom;
            this.date_error = None;
            if let Some(start) = date.start() {
                let time = this.custom_from.read(cx).value();
                let time = time.split_whitespace().nth(1).unwrap_or("00:00");
                this.custom_from.update(cx, |state, cx| {
                    state.set_value(format!("{start} {time}"), window, cx)
                });
            }
            if let Some(end) = date.end() {
                let time = this.custom_to.read(cx).value();
                let time = time.split_whitespace().nth(1).unwrap_or("23:59");
                this.custom_to.update(cx, |state, cx| {
                    state.set_value(format!("{end} {time}"), window, cx)
                });
                // A calendar end selection is explicit, so it must be honored.
                this.draft_follow_now = false;
            }
            this.draft_text = (
                this.custom_from.read(cx).value().to_string(),
                this.custom_to.read(cx).value().to_string(),
            );
            cx.notify();
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
            self.persist_preferences();

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
            self.request_import_status(cx);
            self.provider_after = None;
            self.model_after = None;
            if !self.scope_reset_pending {
                self.request_filters(cx);
                self.request_model_filters(cx);
            }
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
        self.capture_select.update(cx, |state, cx| {
            state.set_items(source_controls::capture_choices(self.zh), window, cx);
            state.set_selected_value(
                &self
                    .capture
                    .displayed(self.source_scope())
                    .as_str()
                    .to_owned(),
                window,
                cx,
            );
        });
        self.kind_select.update(cx, |state, cx| {
            state.set_items(layout::kind_choices(self.zh), window, cx);
            state.set_selected_value(&self.record_kind.as_str().to_owned(), window, cx);
        });
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
                .map(|m| choice(m.key.clone(), usage_table::model_label(&m.key, self.zh))),
        );
        if !self.model.is_empty() && !models.iter().any(|p| p.code == self.model) {
            models.push(choice(
                self.model.clone(),
                usage_table::model_label(&self.model, self.zh),
            ));
        }
        for (select, items) in [
            (&self.outcome_select, outcomes),
            (&self.usage_select, usage),
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
        self.group_pages.reset();
    }

    fn reset_pagination(&mut self) {
        self.frozen_range = Some(self.compute_range());
        self.rows_cursors.reset();
        self.reset_group_pagination();
    }

    fn compute_range(&self) -> (i64, i64) {
        self.range.bounds(now_ms()).unwrap_or_else(|| {
            resolve_custom_range(self.custom_range, self.custom_follow_now, now_ms())
        })
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
            "include_tests": self.capture.include_tests(),
            "source_scope": self.source_scope(),
            "capture_method": self.capture.effective(self.source_scope()).as_str(),
            "record_kind": self.record_kind,
        })
    }

    // ---- requests ---------------------------------------------------------

    fn request_dashboard(&mut self, cx: &mut Context<Self>) {
        // The tag carries the filter generation, so a reply is always judged
        // against the filters it was issued for.
        let tag = format!("dash-{}", self.coordinator.generation());
        let request = crate::stats_backend::command(
            "stats_dashboard",
            &tag,
            json!({"filter": self.filter(), "granularity": "auto"}),
        );
        // A dashboard request settles the one request a filter change owes; the
        // generation it was issued under is what a stale reply is judged by.
        self.coordinator.take_request();
        self.dashboard_seq = self.backend.submit_read(JobKey::Dashboard, tag, request);
        self.loading_dashboard = true;
        self.pending_dashboard = true;
        cx.notify();
    }

    /// Reads the import status. Reads are keyed by resource, so two refreshes in
    /// a row collapse into one request.
    fn request_import_status(&mut self, cx: &mut Context<Self>) {
        let tag = format!("import-{}", self.generation);
        let request = crate::stats_backend::command("stats_workbuddy_status", &tag, json!({}));
        self.import_seq = self
            .backend
            .submit_read(JobKey::workbuddy_read("status"), tag, request);
        self.import_poll = crate::stats_controller::ScanPollBudget::new();
        cx.notify();
    }

    fn request_rows(&mut self, cx: &mut Context<Self>) {
        let tag = format!("rows-{}", self.generation);
        let mut fields = json!({
            "filter": self.filter(),
            "page_size": self.page_size,
        });
        if let Some(cursor) = self.rows_cursors.cursor()
            && let Ok(value) = serde_json::to_value(cursor)
        {
            fields["cursor"] = value;
        }
        if let Some(page) = self.rows_cursors.seek_page() {
            fields["page"] = json!(page);
        }
        let request = crate::stats_backend::command("stats_requests", &tag, fields);
        // The identity is recorded before the request leaves, so a reply that
        // arrives after the filters change is recognisably stale.
        let cursor_key = self
            .rows_cursors
            .cursor()
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
        if let Some(cursor) = self.group_pages.cursor()
            && let Ok(value) = serde_json::to_value(cursor)
        {
            fields["cursor"] = value;
        }
        if let Some(page) = self.group_pages.seek_page() {
            fields["page"] = json!(page);
        }
        let request = crate::stats_backend::command("stats_breakdown", &tag, fields);
        // The identity records the grouping, sort and cursor this page was
        // fetched with, so a reply for a different grouping cannot land.
        self.groups_gate.issue(
            self.generation,
            crate::stats_controller::StatsTab::Groups,
            group_by,
            self.group_sort,
            self.group_pages
                .cursor()
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
            json!({"kind": "provider", "query": self.provider_query, "after_key": self.provider_after, "limit": 100,
                "source_scope":self.source_scope(),"capture_method":self.capture.effective(self.source_scope()).as_str()}),
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
                "source_scope":self.source_scope(),"capture_method":self.capture.effective(self.source_scope()).as_str(),
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
        if std::mem::take(&mut self.scope_reset_pending) {
            self.reset_source_dependents(window, cx);
            return;
        }
        let capture = self
            .capture_select
            .read(cx)
            .selected_value()
            .map(String::as_str)
            .unwrap_or("all");
        let capture = match capture {
            "proxy" => crate::stats_controller::CaptureChoice::Proxy,
            "workbuddy" => crate::stats_controller::CaptureChoice::WorkbuddyImport,
            _ => crate::stats_controller::CaptureChoice::All,
        };
        if self.capture.select(capture, self.source_scope()) {
            self.reset_source_dependents(window, cx);
            return;
        }
        let mut dirty = false;
        let kind = self
            .kind_select
            .read(cx)
            .selected_value()
            .and_then(|value| crate::stats_dto::RecordKind::parse(value))
            .unwrap_or_default();
        if kind != self.record_kind {
            self.record_kind = kind;
            dirty = true;
        }
        if self.date_open {
            let text = (
                self.custom_from.read(cx).value().to_string(),
                self.custom_to.read(cx).value().to_string(),
            );
            if text != self.draft_text {
                if text.1 != self.draft_text.1 {
                    self.draft_follow_now = false;
                }
                self.draft_preset = RangePreset::Custom;
                self.date_error = None;
                self.draft_text = text;
                let from =
                    chrono::NaiveDateTime::parse_from_str(&self.draft_text.0, "%Y-%m-%d %H:%M")
                        .ok();
                let to =
                    chrono::NaiveDateTime::parse_from_str(&self.draft_text.1, "%Y-%m-%d %H:%M")
                        .ok();
                if let (Some(from), Some(to)) = (from, to) {
                    let date = Date::Range(Some(from.date()), Some(to.date()));
                    self.calendar.update(cx, |calendar, cx| {
                        if calendar.date() != date {
                            calendar.set_date(date, window, cx);
                        }
                    });
                }
            }
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
            let started = self.event_metrics.begin();
            self.provider_query = pq;
            self.provider_after = None;
            self.provider_next = None;
            self.request_filters(cx);
            self.event_metrics
                .record("provider_input_apply", started, None);
        }
        let mq = self.model_search.read(cx).value().trim().to_owned();
        if mq != self.model_query {
            let started = self.event_metrics.begin();
            self.model_query = mq;
            self.model_after = None;
            self.model_next = None;
            self.request_model_filters(cx);
            self.event_metrics
                .record("model_input_apply", started, None);
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
        if dirty {
            self.mark_dirty(cx);
        }
        cx.notify();
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        self.refresh_report(true, cx);
    }

    fn refresh_report(&mut self, reset_page: bool, cx: &mut Context<Self>) {
        // Following the clock can eventually exceed the core's 366 day cap.
        // Keep the last valid data instead of sending an invalid range.
        let (from, to) = self.compute_range();
        if crate::stats_controller::validate_range(Some(from), Some(to)).is_err() {
            self.error = Some(
                self.t(
                    "时间范围不能超过366天，请重新选择",
                    "Range exceeds 366 days; select a new range",
                )
                .to_owned(),
            );
            self.last_refresh = Instant::now();
            return;
        }
        // Periodic reads must not return a reader on page two to page one.
        // Keep that page's frozen range and cursor until an explicit refresh
        // or filter change; the live first page still follows the clock.
        if reset_page
            || (self.rows_cursors.page_number() == 1 && self.group_pages.page_number() == 1)
        {
            self.reset_pagination();
        }
        self.request_dashboard(cx);
        // The import panel refreshes with the rest of the statistics; its read
        // is keyed by resource, so two refreshes in a row collapse into one.
        self.request_import_status(cx);
        match self.tab {
            Tab::Requests => self.request_rows(cx),
            _ => self.request_groups(cx),
        }
        self.last_refresh = Instant::now();
        self.request_health(cx);
    }

    pub fn tick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.visible && self.mapping_ui.open {
            self.close_mapping_state();
            window.close_dialog(cx);
        }
        self.expire_mapping_preview(cx);
        // Hand pending intents to the worker. A full channel just leaves them
        // queued; nothing is dropped and write intents keep their order.
        let foreground = crate::stats_controller::may_refresh(
            self.visible,
            window.is_window_active(),
            window.has_active_dialog(cx) || self.date_open,
            true,
        );
        if !self.visible {
            self.backend.cancel_reads();
        }
        if self.visible || self.backend.has_pending_writes() {
            self.backend.pump_with_reads(self.visible);
        }
        if self.visible && !window.has_active_dialog(cx) {
            self.sync_filter_state(window, cx);
            self.persist_preferences();
        }
        // Leaving the page cancels the debounce instead of firing a query the
        // user no longer sees.
        if let Some(deadline) = self.debounce_until {
            if !self.visible {
                self.debounce_until = None;
            } else if !window.has_active_dialog(cx) && !self.date_open && Instant::now() >= deadline
            {
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
            self.refresh_report(false, cx);
        }
        // Provider modes are filled in one per tick so the list converges
        // without ever queueing a job per provider at once.
        self.poll_import_status(cx);
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
        if self.handle_mapping_reply(kind, seq, &tag, &result, cx) {
            return;
        }
        if self.handle_import_reply(kind, seq, &tag, &result, cx) {
            return;
        }
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
                // Full snapshot: the badge and the warnings come from here, not
                // from the fact that this query succeeded.
                self.health = Some(health);
            }
            return;
        }
        let result = result.and_then(|value| {
            let requested_page = match kind {
                JobKind::Rows => self.rows_cursors.seek_page(),
                JobKind::Groups => self.group_pages.seek_page(),
                _ => None,
            };
            if requested_page.is_some_and(|page| value["page"].as_u64() != Some(page as u64)) {
                return Err(self
                    .t(
                        "页码跳转需要更新并重启后台核心",
                        "Update and restart the core to jump to a page",
                    )
                    .to_owned());
            }

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
                JobKind::PriceSync => serde_json::from_value::<crate::stats_dto::PriceSyncDto>(
                    value["catalog_sync"].clone(),
                )
                .map(|_| ()),
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
                if kind == JobKind::PriceSync {
                    self.price_sync_pending = false;
                }
                if kind == JobKind::Prices {
                    self.prices_loading = false;
                }
                if dashboard_reply {
                    self.pending_dashboard = false;
                }
                if rows_reply {
                    self.pending_rows = false;
                    self.rows_cursors.fail();
                    self.group_pages.fail();
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
                self.filter_key = dashboard.filter_key.clone();
                self.dashboard = Some(dashboard);
                self.error = None;
                self.last_updated = Some(chrono::Local::now().format("%H:%M").to_string());
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
                            // Row-only updates must not rebuild column metadata
                            // while the user is dragging a column divider.
                            cx.notify();
                        });
                        self.error = None;
                        self.last_updated = Some(chrono::Local::now().format("%H:%M").to_string());
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
                        self.group_pages.finish_page(page.next_cursor);
                        self.group_table.update(cx, |table, cx| {
                            table.delegate_mut().rows = self.groups.clone();
                            table.delegate_mut().group_by = page.group_by;
                            table.delegate_mut().loading = false;
                            table.refresh(cx);
                        });
                        self.error = None;
                        self.last_updated = Some(chrono::Local::now().format("%H:%M").to_string());
                    }
                }
            }
            let page = match self.tab {
                Tab::Requests => self.rows_cursors.page_number(),
                _ => self.group_pages.page_number(),
            };
            self.page_input.update(cx, |state, cx| {
                state.set_value(page.to_string(), window, cx)
            });
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
                if !self.provider.is_empty() && !items.iter().any(|item| item.code == self.provider)
                {
                    items.push(choice(self.provider.clone(), self.provider.clone()));
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
                    items.push(choice(
                        option.key.clone(),
                        usage_table::model_label(&option.key, self.zh),
                    ));
                }
                if !self.model.is_empty() && !items.iter().any(|item| item.code == self.model) {
                    items.push(choice(
                        self.model.clone(),
                        usage_table::model_label(&self.model, self.zh),
                    ));
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
        if kind == JobKind::PriceSync {
            self.price_sync_pending = false;
            self.price_sync = value
                .get("catalog_sync")
                .cloned()
                .and_then(|value| serde_json::from_value(value).ok());
            self.error = None;
            self.request_prices(cx);
            cx.notify();
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
                self.price_sync = list.catalog_sync;
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
                if status.status == MaintenanceStatus::Failed {
                    // A failed clear must not look like a successful one, and
                    // the raw SQLite/trigger text stays in the core log only.
                    let message = self
                        .t(
                            "清理失败，历史数据仍保留；请检查存储后重新预览并重试。",
                            "Cleanup failed; history is preserved. Check storage, preview again and retry.",
                        )
                        .to_owned();
                    self.maintenance_error = Some(message.clone());
                    self.error = Some(message);
                    self.maintenance = Some(status);
                    // Reads are intentionally unavailable while degraded; the
                    // health snapshot can still tell the user what happened.
                    self.request_health(cx);
                    cx.notify();
                    return;
                }
                if status.status == MaintenanceStatus::Completed {
                    self.maintenance_error = None;
                    self.error = None;
                }
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
                .w(px(680.))
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
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content = self
            .owner
            .update(cx, |this, cx| this.detail_content(cx))
            .unwrap_or_else(|_| div().into_any_element());
        div()
            .id("usage-detail-scroll")
            .max_h(px(f32::from(window.viewport_size().height) * 0.8 - 60.))
            .overflow_y_scroll()
            .child(content)
    }
}

impl Statistics {
    fn detail_content(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let zh = self.zh;
        let title = if zh { "用量详情" } else { "Usage detail" };
        let Some(detail) = &self.details else {
            return crate::ui_components::dialog_body()
                .w_full()
                .gap_3()
                .child(
                    div()
                        .text_size(px(crate::theme::font::DIALOG_TITLE))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(title),
                )
                .child(Label::new(if zh { "正在读取…" } else { "Loading…" }).text_color(rgb(MUTED)))
                .into_any_element();
        };
        let item = &detail.request;
        let row = |name: String, value: String| {
            div()
                .id(SharedString::from(format!("detail-{name}")))
                .role(Role::Group)
                .aria_label(format!("{name}：{value}"))
                .flex()
                .justify_between()
                .gap_3()
                .text_size(px(crate::theme::font::BODY))
                .child(
                    Label::new(name)
                        .text_size(px(crate::theme::font::BODY))
                        .flex_shrink_0()
                        .text_color(rgb(MUTED)),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_right()
                        .text_color(rgb(INK))
                        .child(value),
                )
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
            Some(cost) => usage_table::cost_hint(cost.status, zh),
            None => usage_table::cost_label(CostStatus::Unpriced, zh),
        };
        let mut rows: Vec<_> = detail_metadata::fields(item, zh)
            .into_iter()
            .map(|(label, value)| row(label.into(), value))
            .collect();
        rows.extend(vec![
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
                format!(
                    "{} · {}",
                    item.provider_name,
                    usage_table::route_text(item, zh)
                ),
            ),
            row(
                if zh {
                    "请求模型".into()
                } else {
                    "Requested model".into()
                },
                usage_table::requested_text(item, zh),
            ),
            row(
                if zh {
                    "上游模型".into()
                } else {
                    "Upstream model".into()
                },
                usage_table::model_text(item, zh),
            ),
            row(
                if zh {
                    "上游协议".into()
                } else {
                    "Upstream protocol".into()
                },
                match item.upstream_protocol.as_deref() {
                    Some("openai_responses") => "OpenAI Responses",
                    Some("anthropic_messages") => "Anthropic Messages",
                    Some("gemini_generate_content") => "Gemini GenerateContent",
                    Some("openai_chat") => "OpenAI Chat Completions",
                    _ => "—",
                }
                .into(),
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
                if item.usage_status == crate::stats_dto::UsageStatus::NotApplicable {
                    if zh { "不适用" } else { "Not applicable" }.into()
                } else {
                    format!(
                        "↓{} ↑{} R{} W{} Σ{}",
                        usage_table::exact(item.input_tokens),
                        usage_table::exact(item.output_tokens),
                        usage_table::exact(item.cache_read_tokens),
                        usage_table::exact(item.cache_write_tokens),
                        usage_table::exact(item.total_tokens)
                    )
                },
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
                    "缓存命中率".into()
                } else {
                    "Cache hit rate".into()
                },
                usage_table::percent(usage_table::cache_rate(
                    item.cache_read_tokens,
                    item.input_tokens,
                )),
            ),
            row(
                "tok/s".into(),
                format!(
                    "{} · {}",
                    usage_table::rate_text(item),
                    usage_table::rate_hint(item.stream, zh)
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
        ]);
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
        crate::ui_components::dialog_body()
            .w_full()
            .gap_3()
            .child(
                div()
                    .text_size(px(crate::theme::font::DIALOG_TITLE))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(title),
            )
            .child(
                div()
                    .text_size(px(crate::theme::font::LABEL))
                    .text_color(rgb(MUTED))
                    .child(if zh {
                        "只保存统计摘要：不包含聊天正文、工具参数、图片或密钥"
                    } else {
                        "Summary only: no chat text, tool arguments, images or keys are stored"
                    }),
            )
            .child(div().flex().flex_col().gap_2().children(rows))
            .child(
                crate::ui_components::dialog_footer().child(
                    stats_button("detail-close")
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
    pub fn page_title(&self) -> &'static str {
        use preferences::View;
        match self.view {
            View::Overview => self.t("用量概览", "Usage overview"),
            View::Details => self.t("用量明细", "Usage records"),
            View::Summary => self.t("汇总分析", "Usage analysis"),
            View::Settings => self.t("统计设置", "Statistics settings"),
        }
    }

    fn header(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .flex()
            .items_end()
            .flex_wrap()
            .justify_between()
            .gap_3()
            .w_full()
            .mb(px(8.))
            .flex_shrink_0()
            .child(self.source_group(cx))
            .into_any_element()
    }

    fn write_date_draft(&mut self, range: (i64, i64), window: &mut Window, cx: &mut Context<Self>) {
        let format = |ms| {
            Local
                .timestamp_millis_opt(ms)
                .single()
                .expect("valid timestamp")
                .format("%Y-%m-%d %H:%M")
                .to_string()
        };
        self.draft_text = (format(range.0), format(range.1));
        self.custom_from.update(cx, |state, cx| {
            state.set_value(self.draft_text.0.clone(), window, cx)
        });
        self.custom_to.update(cx, |state, cx| {
            state.set_value(self.draft_text.1.clone(), window, cx)
        });
        self.calendar.update(cx, |calendar, cx| {
            calendar.set_date(
                Date::Range(
                    Local
                        .timestamp_millis_opt(range.0)
                        .single()
                        .map(|dt| dt.date_naive()),
                    Local
                        .timestamp_millis_opt(range.1)
                        .single()
                        .map(|dt| dt.date_naive()),
                ),
                window,
                cx,
            )
        });
    }

    fn open_date_draft(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.draft_preset = self.range;
        self.draft_follow_now = self.range != RangePreset::Custom || self.custom_follow_now;
        self.date_error = None;
        self.write_date_draft(self.compute_range(), window, cx);
    }

    fn apply_date_draft(&mut self, cx: &mut Context<Self>) {
        if !self.calendar.read(cx).date().is_complete()
            && self.draft_text
                == (
                    self.custom_from.read(cx).value().to_string(),
                    self.custom_to.read(cx).value().to_string(),
                )
        {
            self.date_error = Some(
                self.t(
                    "请在日历中选择结束日期",
                    "Select an end date in the calendar",
                )
                .to_owned(),
            );
            cx.notify();
            return;
        }
        match validate_date_draft(
            &self.custom_from.read(cx).value(),
            &self.custom_to.read(cx).value(),
            self.draft_follow_now,
            now_ms(),
        ) {
            Ok(range) => {
                self.custom_range = range;
                self.custom_follow_now = self.draft_follow_now;
                self.range = if self.draft_follow_now {
                    self.draft_preset
                } else {
                    RangePreset::Custom
                };
                self.date_error = None;
                self.date_open = false;
                self.mark_dirty(cx);
            }
            Err(code) => {
                self.date_error = Some(
                    match code {
                        "reversed_date" => {
                            self.t("结束时间必须晚于开始时间", "End must be after start")
                        }
                        "range_too_long" => {
                            self.t("时间范围不能超过366天", "Range cannot exceed 366 days")
                        }
                        _ => self.t(
                            "请输入完整日期时间：YYYY-MM-DD HH:MM",
                            "Enter a complete date/time: YYYY-MM-DD HH:MM",
                        ),
                    }
                    .to_owned(),
                );
                cx.notify();
            }
        }
    }

    fn date_panel(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .flex()
            .flex_col()
            .w(px(600.))
            .gap_3()
            .child(
                div().flex().gap_2().children(
                    [
                        RangePreset::Today,
                        RangePreset::Day,
                        RangePreset::Week,
                        RangePreset::Fortnight,
                        RangePreset::Month,
                        RangePreset::Quarter,
                    ]
                    .into_iter()
                    .enumerate()
                    .map(|(ix, preset)| {
                        stats_button(("date-preset", ix))
                            .outline()
                            .label(preset.title(self.zh))
                            .when(self.draft_preset == preset, |button| button.primary())
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.draft_preset = preset;
                                this.draft_follow_now = true;
                                this.date_error = None;
                                this.write_date_draft(
                                    preset.bounds(now_ms()).expect("relative preset"),
                                    window,
                                    cx,
                                );
                                cx.notify();
                            }))
                    }),
                ),
            )
            .child(
                div()
                    .flex()
                    .gap_4()
                    .border_t_1()
                    .border_color(rgb(BORDER))
                    .pt_3()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .w(px(290.))
                            .gap_3()
                            .child(self.filter_label(
                                self.t("支持日期与时间（本地时间）", "Date and time (local)"),
                            ))
                            .child(self.filter_label(self.t("开始时间", "Start")))
                            .child(
                                crate::ui_components::compact_input(&self.custom_from, false)
                                    .w_full(),
                            )
                            .child(self.filter_label(self.t("结束时间", "End")))
                            .child(
                                crate::ui_components::compact_input(
                                    &self.custom_to,
                                    self.draft_follow_now,
                                )
                                .w_full(),
                            )
                            .child(
                                Checkbox::new("date-follow-now")
                                    .large()
                                    .label(
                                        self.t("结束时间跟随当前时刻", "End follows current time"),
                                    )
                                    .checked(self.draft_follow_now)
                                    .on_change(cx.listener(|this, enabled: &bool, window, cx| {
                                        this.draft_follow_now = *enabled;
                                        // Turning off following freezes exactly the displayed draft.
                                        // Turning it on keeps a manually edited start fixed.
                                        if *enabled {
                                            let text =
                                                Local::now().format("%Y-%m-%d %H:%M").to_string();
                                            this.custom_to.update(cx, |state, cx| {
                                                state.set_value(text.clone(), window, cx)
                                            });
                                            this.draft_text.1 = text;
                                        }
                                        cx.notify();
                                    })),
                            )
                            .when_some(self.date_error.clone(), |this, error| {
                                this.child(
                                    Label::new(error)
                                        .text_size(px(crate::theme::font::LABEL))
                                        .text_color(rgb(0xa4424a)),
                                )
                            })
                            .child(
                                div()
                                    .flex()
                                    .gap_2()
                                    .mt_2()
                                    .child(
                                        stats_button("date-cancel")
                                            .outline()
                                            .label(self.t("取消", "Cancel"))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.date_open = false;
                                                this.date_error = None;
                                                cx.notify();
                                            })),
                                    )
                                    .child(
                                        stats_button("date-apply")
                                            .primary()
                                            .label(self.t("确定", "Apply"))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                // Capture text changes immediately, even before the polling tick.
                                                let text = (
                                                    this.custom_from.read(cx).value().to_string(),
                                                    this.custom_to.read(cx).value().to_string(),
                                                );
                                                if text != this.draft_text {
                                                    this.draft_preset = RangePreset::Custom;
                                                }
                                                this.apply_date_draft(cx);
                                            })),
                                    ),
                            ),
                    )
                    .child(crate::ui_components::calendar(&self.calendar, self.zh)),
            )
            .into_any_element()
    }

    fn date_control(&self, cx: &mut Context<Self>) -> AnyElement {
        let owner = cx.entity().downgrade();
        let content_owner = owner.clone();
        let tooltip = if self.range == RangePreset::Custom {
            let (from, to) = self.compute_range();
            format!(
                "{} — {}{}",
                Local
                    .timestamp_millis_opt(from)
                    .single()
                    .unwrap()
                    .format("%Y-%m-%d %H:%M"),
                Local
                    .timestamp_millis_opt(to)
                    .single()
                    .unwrap()
                    .format("%Y-%m-%d %H:%M"),
                if self.custom_follow_now {
                    self.t("（结束时间跟随当前时刻）", " (end follows now)")
                } else {
                    ""
                }
            )
        } else {
            self.range.title(self.zh).to_owned()
        };
        Popover::new("stats-date-range")
            .open(self.date_open)
            .trigger(
                stats_button("stats-date-trigger")
                    .h(px(crate::theme::size::SELECT_HEIGHT))
                    .w_full()
                    .outline()
                    .bg(rgb(0xffffff))
                    .border_color(rgb(0xe1e5eb))
                    .text_color(rgb(0x5c6679))
                    .rounded(px(8.))
                    .label(self.range.title(self.zh))
                    .tooltip(tooltip)
                    .accessibility_label(self.t("选择日期范围", "Choose date range")),
            )
            .on_open_change(move |open, window, cx| {
                let _ = owner.update(cx, |this, cx| {
                    if *open && !this.date_open {
                        this.open_date_draft(window, cx);
                    }
                    this.date_open = *open;
                    if !*open {
                        this.date_error = None;
                    }
                    cx.notify();
                });
            })
            .content(move |_, _, cx| {
                content_owner
                    .update(cx, |this, cx| this.date_panel(cx))
                    .unwrap_or_else(|_| div().into_any_element())
            })
            .into_any_element()
    }

    fn refresh_control(&self, cx: &mut Context<Self>) -> AnyElement {
        let owner = cx.entity().downgrade();
        let content_owner = owner.clone();
        let label = if self.refresh_interval == 0 {
            self.t("关闭", "Off").to_owned()
        } else {
            format!("{} {}", self.refresh_interval, self.t("秒", "s"))
        };
        Popover::new("stats-auto-refresh")
            .open(self.refresh_open)
            .trigger(
                stats_button("stats-auto-trigger")
                    .h(px(crate::theme::size::SELECT_HEIGHT))
                    .w_full()
                    .outline()
                    .bg(rgb(0xffffff))
                    .border_color(rgb(0xe1e5eb))
                    .text_color(rgb(0x5c6679))
                    .rounded(px(8.))
                    .child(
                        div().flex().items_center().gap(px(6.))
                            .child(img("design/header-refresh.png").size(px(16.)))
                            .child(Label::new(label)),
                    )
                    .tooltip(self.t(
                        "统计页面自动刷新间隔；仅刷新已采集的数据，不改变后台采集频率",
                        "Report refresh interval; refreshes collected data without changing background collection frequency",
                    ))
                    .accessibility_label(self.t("统计页面自动刷新间隔", "Report auto refresh interval")),
            )
            .on_open_change(move |open, _, cx| {
                let _ = owner.update(cx, |this, cx| {
                    this.refresh_open = *open;
                    cx.notify();
                });
            })
            .content(move |_, _, cx| {
                content_owner
                    .update(cx, |this, cx| {
                        div()
                            .flex()
                            .flex_col()
                            .w(px(110.))
                            .gap_1()
                            .children([0, 5, 10, 30, 60].into_iter().map(|seconds| {
                                let label = if seconds == 0 {
                                    this.t("关闭", "Off").to_owned()
                                } else {
                                    format!("{seconds}{}", this.t("秒", "s"))
                                };
                                stats_button(("refresh-interval", seconds as usize))
                                    .ghost()
                                    .w_full()
                                    .label(label)
                                    .when(this.refresh_interval == seconds, |button| {
                                        button.primary()
                                    })
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.refresh_interval = seconds;
                                        this.last_refresh = Instant::now();
                                        this.refresh_open = false;
                                        cx.notify();
                                    }))
                            }))
                            .into_any_element()
                    })
                    .unwrap_or_else(|_| div().into_any_element())
            })
            .into_any_element()
    }

    fn filter_search(&self, provider: bool, cx: &mut Context<Self>) -> AnyElement {
        let (input, has_next, label, more_label) = if provider {
            (
                &self.provider_search,
                self.provider_next.is_some(),
                self.t(
                    "搜索供应商（含历史记录）",
                    "Search providers, including history",
                ),
                self.t("加载下一批供应商选项", "Load next provider options"),
            )
        } else {
            (
                &self.model_search,
                self.model_next.is_some(),
                self.t("搜索模型", "Search models"),
                self.t("加载下一批模型选项", "Load next model options"),
            )
        };
        div()
            .flex()
            .flex_col()
            .w(px(230.))
            .gap_1()
            .child(self.filter_label(label))
            .child(
                div()
                    .flex()
                    .items_center()
                    .h(px(crate::theme::size::INPUT_HEIGHT))
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(crate::ui_components::compact_input(input, false).w_full()),
                    )
                    .when(has_next, |row| {
                        row.child(
                            stats_button(if provider {
                                "provider-search-more"
                            } else {
                                "model-search-more"
                            })
                            .size(px(28.))
                            .outline()
                            .child(crate::ui_components::action_icon(
                                Icon::new(IconName::ChevronDown).size(px(16.)),
                            ))
                            .tooltip(more_label)
                            .accessibility_label(more_label)
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    if provider {
                                        this.provider_after = this.provider_next.clone();
                                        this.provider_next = None;
                                        this.request_filters(cx);
                                    } else {
                                        this.model_after = this.model_next.clone();
                                        this.model_next = None;
                                        this.request_model_filters(cx);
                                    }
                                },
                            )),
                        )
                    }),
            )
            .into_any_element()
    }

    /// The source scope the bar is currently showing.
    fn source_scope(&self) -> crate::stats_dto::SourceScope {
        self.source.selected()
    }

    fn filters(&self, cx: &mut Context<Self>) -> AnyElement {
        let tests_available = self.capture.include_tests_available(self.source_scope());
        // The source switch belongs in the heading. The remaining controls
        // wrap naturally; no empty client/source column or enclosing card.
        let slots = crate::source_filter::SLOTS
            .iter()
            .copied()
            .filter(|slot| {
                !matches!(
                    slot,
                    crate::source_filter::FilterSlot::Source
                        | crate::source_filter::FilterSlot::Status
                )
            })
            .collect::<Vec<_>>();
        div()
            .id("stats-filters")
            .flex()
            .flex_col()
            .flex_shrink_0()
            .w_full()
            .on_prepaint({
                let owner = cx.entity().downgrade();
                move |bounds, _, cx| {
                    // KPI/chart cards still have their original inner padding.
                    let available = f32::from(bounds.size.width) - CARD_PAD * 2. - 2.;
                    let _ = owner.update(cx, |this, cx| {
                        if (this.filters_width - available).abs() > 0.5 {
                            this.filters_width = available;
                            cx.notify();
                        }
                    });
                }
            })
            .gap_2()
            .child(self.filter_row(0, &slots, cx))
            .when(self.more_filters_open, |this| {
                this.child(
                    div()
                        .flex()
                        .flex_wrap()
                        .items_end()
                        .gap_3()
                        .pt_3()
                        .border_t_1()
                        .border_color(rgb(BORDER))
                        .child(self.capture_control())
                        .child(div().w(px(150.)).child(
                            crate::ui_components::select(&self.outcome_select, false).w_full(),
                        ))
                        .child(self.filter_search(true, cx))
                        .child(self.filter_search(false, cx))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap_1()
                                .child(
                                    self.filter_label(self.t("用量完整性", "Usage completeness")),
                                )
                                .child(
                                    div()
                                        .w(px(140.))
                                        .h(px(crate::theme::size::SELECT_HEIGHT))
                                        .child(
                                            crate::ui_components::select(&self.usage_select, false)
                                                .w_full()
                                                .accessibility_label(
                                                    self.t("用量完整性", "Usage completeness"),
                                                ),
                                        ),
                                ),
                        )
                        .child(
                            div().flex().items_center().h(px(28.)).child(
                                Checkbox::new("include-tests")
                                    .large()
                                    .label(self.t("包含测试连接", "Include connection tests"))
                                    .checked(self.capture.include_tests())
                                    .disabled(!tests_available)
                                    .on_change(cx.listener(|this, enabled: &bool, _, cx| {
                                        this.capture
                                            .set_include_tests(*enabled, this.source_scope());
                                        this.include_tests = this.capture.include_tests();
                                        this.mark_dirty(cx);
                                    })),
                            ),
                        ),
                )
            })
            .when_some(self.error.clone(), |this, error| {
                this.child(
                    Label::new(crate::i18n::message(&error, self.zh))
                        .text_size(px(crate::theme::font::LABEL))
                        .text_color(rgb(0xa4424a)),
                )
            })
            .into_any_element()
    }

    fn filter_label(&self, text: &'static str) -> Label {
        Label::new(text)
            .text_size(px(crate::theme::font::LABEL))
            .text_color(rgb(MUTED))
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
                    .text_size(px(crate::theme::font::LABEL))
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

    /// Both trend canvases stay visible, including when a currency is unpriced.
    fn trends(&self, cx: &mut Context<Self>) -> AnyElement {
        let series = self
            .dashboard
            .as_ref()
            .map(|d| d.series.as_slice())
            .unwrap_or_default();
        let (points, cost_points) = ChartPoint::split_series(series, self.currency);
        // The bucket width comes from the series itself, so the cost card's
        // subtitle describes the range that is actually plotted.
        let bucket_ms = points
            .first()
            .map(|point| (point.end_ms - point.start_ms).max(0))
            .unwrap_or(0);
        let incomplete = points.iter().any(|point| {
            point.collection_warning || point.quality == crate::stats_dto::SeriesQuality::Unknown
        });
        let token_chart = UsagePlot {
            id: ElementId::Name("usage-token-chart".into()),
            points: points.clone(),
            visible: self.visible_series.clone(),
            zh: self.zh,
            mode: ChartMode::Tokens,
            currency: self.currency,
        };
        let token_continuity = token_chart.continuity_note();
        let token_summary = UsagePlot {
            id: ElementId::Name("usage-token-chart-summary".into()),
            points: points.clone(),
            visible: self.visible_series.clone(),
            zh: self.zh,
            mode: ChartMode::Tokens,
            currency: self.currency,
        }
        .summary();
        let has_cost = cost_points.iter().any(|point| point.cost_nanos.is_some());
        let show_cost = series.iter().any(|point| !point.costs.is_empty());
        let cost_chart = UsagePlot {
            id: ElementId::Name("usage-cost-chart".into()),
            // Keep the actual time buckets on an empty currency axis. Their
            // costs remain None, so no zero/free-cost line is fabricated.
            points: if cost_points.is_empty() {
                points.clone()
            } else {
                cost_points
            },
            visible: BTreeSet::new(),
            zh: self.zh,
            mode: ChartMode::Cost,
            currency: self.currency,
        };
        let cost_continuity = cost_chart.continuity_note();
        let legend = div()
            .flex()
            .flex_wrap()
            .items_center()
            .justify_center()
            .h(px(18.))
            .gap_3()
            .children(SeriesKind::ALL.into_iter().map(|kind| {
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
                            .text_size(px(crate::theme::font::LABEL))
                            .text_color(rgb(INK)),
                    )
                    .into_any_element()
            }));
        // DESIGN_SPEC §4.1.4: two cards side by side — usage trend and
        // estimated cost — each with its own header. Both plots, the legend,
        // the warning, the gaps and the currency select keep their semantics.
        let bucket_text = if bucket_ms <= 0 {
            self.t("按区间汇总", "Per interval").to_owned()
        } else if bucket_ms % 86_400_000 == 0 {
            if self.zh {
                format!("按 {} 天汇总", bucket_ms / 86_400_000)
            } else {
                format!("Per {}-day bucket", bucket_ms / 86_400_000)
            }
        } else if bucket_ms % 3_600_000 == 0 {
            if self.zh {
                format!("按 {} 小时汇总", bucket_ms / 3_600_000)
            } else {
                format!("Per {}-hour bucket", bucket_ms / 3_600_000)
            }
        } else if self.zh {
            format!("按 {} 分钟汇总", (bucket_ms / 60_000).max(1))
        } else {
            format!("Per {}-minute bucket", (bucket_ms / 60_000).max(1))
        };
        let token_card = stats_card("stats-trends")
            .flex_1()
            .min_w_0()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .h(px(crate::theme::size::SELECT_HEIGHT))
                    .gap_2()
                    .child(stats_heading(self.t("使用趋势", "Usage trend")))
                    .when(incomplete, |this| {
                        this.child(
                            Label::new(self.t("部分时段记录不全", "Some intervals are incomplete"))
                                .text_size(px(crate::theme::font::SMALL))
                                .text_color(rgb(WARN)),
                        )
                    }),
            )
            .child(
                div()
                    .text_size(px(crate::theme::font::LABEL))
                    .text_color(rgb(crate::theme::color::TEXT_3))
                    .child(self.t(
                        "缓存读/写是输入的子集，图例可独立开关，不可相加",
                        "Cache read/write are input subsets; toggle them, never sum them",
                    )),
            )
            .when_some(token_continuity, |card, note| {
                card.child(
                    Label::new(note)
                        .text_size(px(crate::theme::font::SMALL))
                        .line_height(px(18.))
                        .text_color(rgb(MUTED)),
                )
            })
            .child(
                div()
                    .id("token-chart-host")
                    .w_full()
                    .h(px(250.))
                    .min_h_0()
                    .aria_label(token_summary.clone())
                    .child(token_chart),
            )
            .child(legend);
        let cost_card = stats_card("stats-cost-trend")
            .flex_1()
            .min_w_0()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .h(px(crate::theme::size::SELECT_HEIGHT))
                    .gap_2()
                    .child(stats_heading(self.t("估算费用", "Estimated cost")))
                    .child(
                        div()
                            .w(px(96.))
                            .h(px(crate::theme::size::SELECT_HEIGHT))
                            .child(
                                crate::ui_components::select(&self.currency_select, false)
                                    .w_full()
                                    .accessibility_label(self.t("费用币种", "Cost currency")),
                            ),
                    ),
            )
            .child(
                div()
                    .text_size(px(crate::theme::font::LABEL))
                    .text_color(rgb(crate::theme::color::TEXT_3))
                    .child(bucket_text),
            )
            .when_some(cost_continuity, |card, note| {
                card.child(
                    Label::new(note)
                        .text_size(px(crate::theme::font::SMALL))
                        .line_height(px(18.))
                        .text_color(rgb(MUTED)),
                )
            })
            .child(
                div()
                    .id("cost-chart-host")
                    .relative()
                    .w_full()
                    .h(px(250.))
                    .min_h_0()
                    .aria_label(self.t(
                        "费用趋势图，按币种单独显示",
                        "Cost trend chart, one currency at a time",
                    ))
                    .child(cost_chart)
                    .when(!has_cost, |plot| {
                        plot.child(
                            div()
                                .absolute()
                                .inset_0()
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(
                                    Label::new(self.t(
                                        "当前币种暂无费用数据",
                                        "No cost data for this currency",
                                    ))
                                    .text_size(px(12.))
                                    .text_color(rgb(crate::theme::color::TEXT_3)),
                                ),
                        )
                    }),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .h(px(18.))
                    .gap_1()
                    .child(div().w(px(10.)).h(px(3.)).rounded(px(2.)).bg(rgb(0xc2185b)))
                    .child(
                        Label::new(self.t("估算费用", "Estimated cost"))
                            .text_size(px(crate::theme::font::LABEL))
                            .text_color(rgb(INK)),
                    ),
            );
        let pair = div().flex().w_full().gap(px(12.));
        if self.filters_width >= CHART_SIDE_BY_SIDE_WIDTH {
            pair.flex_row()
                .items_stretch()
                .child(token_card)
                .when(show_cost, |pair| pair.child(cost_card))
                .into_any_element()
        } else {
            pair.flex_col()
                .child(token_card)
                .when(show_cost, |pair| pair.child(cost_card))
                .into_any_element()
        }
    }

    /// Tabs, table and pagination.
    fn details(&self, cx: &mut Context<Self>) -> AnyElement {
        let tab_button = |id: &'static str, tab: Tab, label: String| {
            let active = self.tab == tab;
            stats_button(id)
                .label(label)
                .when(active, |button| button.primary().outline())
                .when(!active, |button| button.ghost())
                .on_click(cx.listener(move |this, _, window, cx| {
                    if this.tab == tab {
                        return;
                    }
                    this.tab = tab;
                    this.summary_tab = tab;
                    this.persist_preferences();
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
        let sort_button = |id: &'static str, sort: SortMetric, label: String| {
            let active = self.group_sort == sort;
            stats_button(id)
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
            // DESIGN_SPEC §4.1.5: the request log is a 56px-row table with a
            // 32px header. Aggregate rows use a readable 36px height.
            Tab::Requests => crate::ui_components::log_table(&self.usage_table).into_any_element(),
            _ => crate::ui_components::table(&self.group_table)
                .with_size(px(36.))
                .into_any_element(),
        };
        stats_card("stats-details")
            .flex_1()
            .min_h_0()
            .overflow_hidden()
            .gap_2()
            .when(self.tab != Tab::Requests, |panel| {
                panel.child(
                    div()
                        .flex()
                        .flex_shrink_0()
                        .flex_wrap()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
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
                                                    .text_size(px(crate::theme::font::LABEL))
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
                        ),
                )
            })
            .child(
                div()
                    .id("stats-table-host")
                    .w_full()
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .on_prepaint({
                        let table = self.usage_table.downgrade();
                        move |bounds, _, cx| {
                            let available = f32::from(bounds.size.width) - 2.;
                            let _ = table.update(cx, |table, cx| {
                                if (table.delegate().available_width - available).abs() > 0.5 {
                                    table.delegate_mut().available_width = available;
                                    table.refresh(cx);
                                    cx.notify();
                                }
                            });
                        }
                    })
                    .child(table),
            )
            .child(self.details_pagination(cx))
            .into_any_element()
    }
    fn details_pagination(&self, cx: &mut Context<Self>) -> AnyElement {
        let (page, total, previous, next) = match self.tab {
            Tab::Requests => (
                self.rows_cursors.page_number(),
                self.total_rows,
                self.rows_cursors.can_go_back(),
                self.rows_cursors.can_go_next(),
            ),
            _ => (
                self.group_pages.page_number(),
                self.total_groups,
                self.group_pages.can_go_back(),
                self.group_pages.can_go_next(),
            ),
        };
        let pages = pagination::pages(total, self.page_size);
        let count_prefix =
            if self.tab != Tab::Requests && total > crate::stats_dto::MAX_COUNTED_GROUPS {
                "≥"
            } else {
                ""
            };
        let busy = self.loading_rows;
        div()
            .flex()
            .items_center()
            .gap_2()
            .flex_wrap()
            .flex_shrink_0()
            .child(
                Label::new(format!(
                    "{} {}/{}{} · {}{} {}",
                    self.t("第", "Page"),
                    page,
                    count_prefix,
                    pages,
                    count_prefix,
                    total.max(0),
                    self.t("条", "rows")
                ))
                .text_size(px(crate::theme::font::LABEL))
                .text_color(rgb(MUTED)),
            )
            .child(
                crate::ui_components::pager_button(
                    "page-prev",
                    self.t("上一页", "Previous"),
                    busy || !previous,
                )
                .on_click(
                    cx.listener(move |this, _, _, cx| {
                        this.navigate_page(page.saturating_sub(1), cx)
                    }),
                ),
            )
            .child(
                crate::ui_components::pager_button(
                    "page-next",
                    self.t("下一页", "Next"),
                    busy || !next,
                )
                .on_click(cx.listener(move |this, _, _, cx| this.navigate_page(page + 1, cx))),
            )
            .child(
                div()
                    .w(px(125.))
                    .child(crate::ui_components::select(&self.page_size_select, busy)),
            )
            .child(Label::new(self.t("跳至", "Go to")).text_size(px(crate::theme::font::LABEL)))
            .child(div().w(px(70.)).child(crate::ui_components::text_input(
                &self.page_input,
                busy || total == 0,
            )))
            .child(
                stats_button("page-go")
                    .ghost()
                    .label(self.t("跳转", "Go"))
                    .disabled(busy || total == 0)
                    .on_click(cx.listener(|this, _, _, cx| this.jump_to_input(cx))),
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
            .id("statistics-page")
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .overflow_hidden()
            .pl_0()
            .pr_0()
            .py_0()
            .gap_3()
            .bg(rgb(BG))
            .child(self.header(cx))
            .child(self.view_tabs(cx))
            .child(self.filters(cx))
            .when(self.view != preferences::View::Settings, |page| {
                page.child(self.compact_status(cx))
            })
            .child(self.report_body(cx))
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
            // The quality banner below already explains this condition. Keep
            // a fallback here only before that dashboard has been loaded.
            LossScope::UnknownAfterUncleanExit
                if self.dashboard.as_ref().is_none_or(|dashboard| {
                    !dashboard
                        .warnings
                        .iter()
                        .any(|w| w.code == WarningCode::UncleanExit)
                }) =>
            {
                alerts.push(
                    self.t(
                        "上次运行未正常退出",
                        "the previous run did not stop cleanly",
                    )
                    .to_owned(),
                )
            }
            LossScope::UnknownAfterUncleanExit => {}
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
            let alert = if code == ERR_MAINTENANCE_FAILED {
                self.t(
                    "统计维护失败；现有记录已保留",
                    "Statistics maintenance failed; existing records are preserved",
                )
                .to_owned()
            } else {
                self.t("统计数据暂不可用", "Statistics temporarily unavailable")
                    .to_owned()
            };
            alerts.push(alert);
        }
        let attention = matches!(health.state, HealthState::Degraded | HealthState::Migrating)
            || health.write_failures_current_run > 0
            || health.loss_scope == LossScope::LossObserved;
        let _ = cx;
        let state = if self.error.is_some() {
            self.t("数据更新失败", "Update failed")
        } else if attention {
            self.t("需关注", "Needs attention")
        } else {
            self.t("正常", "OK")
        };
        let mut summary = vec![
            label.to_owned(),
            state.to_owned(),
            self.import_health_text(),
        ];
        summary.extend(facts);
        summary.push(match &self.last_updated {
            Some(at) => format!("{} {at}", self.t("更新于", "Updated")),
            None => self.t("尚未刷新", "Not refreshed").to_owned(),
        });
        div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_2()
            .child(div().size(px(6.)).rounded_full().bg(rgb(color)))
            .child(
                Label::new(summary.join(" · "))
                    .text_size(px(12.))
                    .text_color(rgb(if self.error.is_some() || attention {
                        WARN
                    } else {
                        0x8a93a5
                    })),
            )
            .children(alerts.into_iter().map(|alert| {
                Label::new(alert)
                    .text_size(px(crate::theme::font::LABEL))
                    .text_color(rgb(WARN))
            }))
            .into_any_element()
    }
}

/// P1 sections live in the same file but stay collapsed by default.
impl Statistics {
    fn section_toggle(
        &self,
        id: &'static str,
        title: &'static str,
        subtitle: Option<&'static str>,
        open: bool,
        cx: &mut Context<Self>,
        which: u8,
    ) -> AnyElement {
        // DESIGN_SPEC §4.2: one collapsed row per section, chevron + title and
        // an optional one-line hint. The hint is a label, not part of the
        // clickable toggle, so it never changes the collapse target.
        div()
            .flex()
            .items_center()
            .gap_2()
            .child(
                stats_button(id)
                    .ghost()
                    .child(crate::ui_components::action_icon(
                        Icon::new(if open {
                            IconName::ChevronDown
                        } else {
                            IconName::ChevronRight
                        })
                        .size(px(16.)),
                    ))
                    .label(title)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        match which {
                            0 => this.pricing_open = !this.pricing_open,
                            _ => this.storage_open = !this.storage_open,
                        }
                        cx.notify();
                    })),
            )
            .when_some(subtitle, |this, subtitle| {
                this.child(
                    Label::new(subtitle)
                        .text_size(px(crate::theme::font::LABEL))
                        .text_color(rgb(MUTED)),
                )
            })
            .into_any_element()
    }

    fn pricing_section(&self, cx: &mut Context<Self>) -> AnyElement {
        let open = self.pricing_open;
        let sync_note = self.price_sync.as_ref().and_then(|sync| {
            sync.last_success_day.as_ref().map(|day| {
                format!(
                    "{} {day} · {} {} · {} {}",
                    self.t("价格更新于", "Prices updated"),
                    self.t("更新", "Updated"),
                    sync.updated_count,
                    self.t("未匹配", "Unmatched"),
                    sync.unmatched_count,
                )
            })
        });
        // DESIGN_SPEC §4.2 price table: model (+ rule id), source/mapping,
        // currency, input/output price per million, actions. Data semantics are
        // unchanged: the same DTO fields and the same save/delete callbacks.
        let header = div()
            .flex()
            .items_center()
            .h(px(32.))
            .px(px(12.))
            .gap(px(16.))
            .bg(rgb(0xf7f9fc))
            .border_b(px(1.))
            .border_color(rgb(BORDER))
            .text_size(px(crate::theme::font::LABEL))
            .text_color(rgb(MUTED))
            .child(div().flex_1().min_w_0().child(self.t("模型", "Model")))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(self.t("来源 / 映射", "Source / mapping")),
            )
            .child(div().w(px(80.)).child(self.t("币种", "Currency")))
            .child(div().flex_1().min_w_0().child(self.t(
                "价格 / 百万 Token（输入 / 输出）",
                "Price / million tokens (input / output)",
            )))
            .child(
                div()
                    .w(px(110.))
                    .flex()
                    .justify_end()
                    .child(self.t("操作", "Actions")),
            );
        let mut table = div()
            .flex()
            .flex_col()
            .w_full()
            .min_w_0()
            .border_1()
            .border_color(rgb(BORDER))
            .rounded(px(8.))
            .overflow_hidden()
            .child(header);
        if self.prices_loading {
            table = table.child(
                div().px(px(12.)).py(px(16.)).child(
                    Label::new(self.t("加载中…", "Loading…"))
                        .text_size(px(crate::theme::font::LABEL))
                        .text_color(rgb(MUTED)),
                ),
            );
        } else if self.prices.is_empty() {
            table = table.child(
                div().px(px(12.)).py(px(16.)).child(
                    Label::new(self.t(
                        "没有匹配的价格规则，可手动更新或新建。未匹配模型暂按默认币种记录0元。",
                        "No matching price rules. Update prices or add a rule; unmatched models default to zero.",
                    ))
                    .text_size(px(crate::theme::font::LABEL))
                    .text_color(rgb(MUTED)),
                ),
            );
        } else {
            for (index, rule) in self.prices.iter().enumerate() {
                let short_id: String = rule.rule_id.chars().take(12).collect();
                // The price DTO has no separate source column; the label is the
                // existing user-visible source/annotation. An empty label is the
                // manual rule, not a blank cell.
                let source_label = if rule.label.trim().is_empty() {
                    self.t("手工", "Manual").to_owned()
                } else {
                    rule.label.clone()
                };
                let provider_name = self
                    .configured_providers
                    .iter()
                    .find(|(id, _)| id == &rule.provider_id)
                    .map(|(_, name)| name.clone())
                    .filter(|name| !name.is_empty());
                let mapping_line = match provider_name {
                    Some(name) => format!("{name} / {}", rule.upstream_model),
                    None => rule.upstream_model.clone(),
                };
                table = table.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(16.))
                        .px(px(12.))
                        .py(px(10.))
                        .border_b(px(1.))
                        .border_color(rgb(0xeef2f7))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .flex_1()
                                .min_w_0()
                                .gap(px(2.))
                                .child(
                                    Label::new(rule.upstream_model.clone())
                                        .text_size(px(12.))
                                        .text_color(rgb(INK)),
                                )
                                .child(
                                    Label::new(short_id)
                                        .text_size(px(crate::theme::font::LABEL))
                                        .text_color(rgb(MUTED)),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .flex_1()
                                .min_w_0()
                                .gap(px(2.))
                                .child(
                                    Label::new(source_label)
                                        .text_size(px(crate::theme::font::LABEL))
                                        .text_color(rgb(INK)),
                                )
                                .child(
                                    Label::new(mapping_line)
                                        .text_size(px(crate::theme::font::LABEL))
                                        .text_color(rgb(MUTED)),
                                ),
                        )
                        .child(div().w(px(80.)).flex().items_center().child(
                            crate::ui_components::tag(
                                crate::ui_components::TagKind::Secondary,
                                rule.currency.as_str(),
                            ),
                        ))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .flex_1()
                                .min_w_0()
                                .gap(px(2.))
                                .child(
                                    Label::new(format!(
                                        "{} / {}",
                                        usage_table::amount(
                                            Some(&rule.input_rate_nanos),
                                            Some(rule.currency)
                                        ),
                                        usage_table::amount(
                                            Some(&rule.output_rate_nanos),
                                            Some(rule.currency)
                                        ),
                                    ))
                                    .text_size(px(12.))
                                    .text_color(rgb(INK)),
                                )
                                .child(
                                    Label::new(format!("{}", rule.input_mode))
                                        .text_size(px(crate::theme::font::LABEL))
                                        .text_color(rgb(MUTED)),
                                ),
                        )
                        .child(
                            div()
                                .w(px(110.))
                                .flex()
                                .justify_end()
                                .gap_1()
                                .child(
                                    stats_button(("price-edit", index))
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
                                    stats_button(("price-delete", index))
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
        let body = div()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .gap_2()
                            .child(
                                crate::ui_components::text_input(&self.price_search, false)
                                    .w(px(240.))
                                    .prefix(Icon::new(IconName::Search)),
                            )
                            .child(
                                stats_button("price-add-top")
                                    .primary()
                                    .child(crate::ui_components::action_icon(Icon::new(IconName::Plus).size(px(16.))))
                                    .label(self.t("新建价格规则", "New price rule"))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.new_price(window, cx)
                                    })),
                            )
                            .child(
                                stats_button("price-reload")
                                    .outline()
                                    .child(crate::ui_components::action_icon(Icon::new(IconName::RefreshCw).size(px(16.))))
                                    .label(self.t("刷新价格", "Reload prices"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.request_prices(cx)
                                    })),
                            )
                            .child(
                                stats_button("price-sync-models-dev")
                                    .outline()
                                    .label(if self.price_sync_pending {
                                        self.t("正在更新价格…", "Updating prices…")
                                    } else {
                                        self.t("手动更新价格", "Update prices now")
                                    })
                                    .disabled(self.price_sync_pending)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        let tag = format!("price-sync-{}", this.generation);
                                        let request = crate::stats_backend::command(
                                            "stats_prices_update",
                                            &tag,
                                            json!({}),
                                        );
                                        match this
                                            .backend
                                            .submit_write(JobKind::PriceSync, tag, request)
                                        {
                                            Ok(_) => this.price_sync_pending = true,
                                            Err(error) => this.error = Some(error),
                                        }
                                        cx.notify();
                                    })),
                            ),
                    )
                    .when_some(sync_note, |this, note| {
                        this.child(
                            Label::new(note)
                                .text_size(px(crate::theme::font::LABEL))
                                .text_color(rgb(MUTED)),
                        )
                    }),
            )
            .child(
                Label::new(self.t(
                    "每日从 models.dev 更新；同模型价格直接覆盖手工调整，历史调用金额保持不变。",
                    "Daily models.dev updates overwrite matching manual prices; recorded costs stay unchanged.",
                ))
                .text_size(px(crate::theme::font::LABEL))
                .text_color(rgb(MUTED)),
            )
            .child(table)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        Label::new(if self.zh {
                            format!(
                                "第 {} 页 · {} 条",
                                self.prices_cursors.page_number(),
                                self.prices.len()
                            )
                        } else {
                            format!(
                                "Page {} · {} rows",
                                self.prices_cursors.page_number(),
                                self.prices.len()
                            )
                        })
                        .text_size(px(crate::theme::font::LABEL))
                        .text_color(rgb(MUTED)),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                stats_button("prices-prev")
                                    .ghost()
                                    .label(self.t("上一页", "Previous"))
                                    .disabled(
                                        self.prices_loading || !self.prices_cursors.can_go_back(),
                                    )
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        if let Some(previous) = this.prices_cursors.back() {
                                            this.prices_cursors.begin_page(previous);
                                            this.request_price_page(
                                                format!("prices-{}", this.generation),
                                                cx,
                                            );
                                        }
                                    })),
                            )
                            .child(
                                stats_button("prices-page-size")
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
                                stats_button("prices-next")
                                    .ghost()
                                    .label(self.t("下一页", "Next"))
                                    .disabled(
                                        self.prices_loading || !self.prices_cursors.can_go_next(),
                                    )
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        if let Some(next) = this.prices_cursors.forward() {
                                            this.prices_cursors.begin_page(next);
                                            this.request_price_page(
                                                format!("prices-{}", this.generation),
                                                cx,
                                            );
                                        }
                                    })),
                            ),
                    ),
            );
        // Repricing history with the current price was removed: every request
        // keeps the snapshot it was admitted with.
        stats_card("stats-pricing")
            .child(self.section_toggle(
                "pricing-toggle",
                self.t("成本定价", "Cost pricing"),
                Some(self.t(
                    "按当前供应商筛选；输入精确模型 ID 搜索价格",
                    "Prices follow the provider filter; search by exact model ID",
                )),
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
        let wide = self.filters_width >= STORAGE_SIDE_BY_SIDE_WIDTH;

        // DESIGN_SPEC §4.2: left column keeps the three settings, right column
        // the provider capture-mode table. Narrow windows stack the columns;
        // no control is hidden in either arrangement.
        let settings_column = div()
            .flex()
            .flex_col()
            .gap_4()
            .flex_1()
            .min_w_0()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .w(px(88.))
                            .child(self.filter_label(self.t("缺省默认币种", "Default currency"))),
                    )
                    .children([Currency::Cny, Currency::Usd].into_iter().map(|currency| {
                        stats_button((
                            "stats-default-currency",
                            if currency == Currency::Cny {
                                0usize
                            } else {
                                1usize
                            },
                        ))
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
                    }))
                    .child(
                        Label::new(self.t("仅新请求", "new requests only"))
                            .text_size(px(crate::theme::font::LABEL))
                            .text_color(rgb(MUTED)),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap_3()
                    .child(
                        div()
                            .w(px(88.))
                            .pt(px(2.))
                            .child(self.filter_label(self.t("摘要采集", "Summary collection"))),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                Checkbox::new("collect-enabled")
                                    .label(self.t("启用摘要采集", "Enable summary collection"))
                                    .checked(enabled)
                                    .large()
                                    .on_change(cx.listener(move |this, value: &bool, _, cx| {
                                        this.update_settings(*value, retention, revision, cx);
                                    })),
                            )
                            .child(
                                Label::new(self.t(
                                    "暂停只影响新请求，已接纳的请求仍会结算",
                                    "Pausing only stops new requests; admitted ones still settle",
                                ))
                                .text_size(px(crate::theme::font::LABEL))
                                .text_color(rgb(MUTED)),
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap_3()
                    .child(
                        div()
                            .w(px(88.))
                            .pt(px(4.))
                            .child(self.filter_label(self.t("保留天数", "Retention days"))),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(
                                div().flex().items_center().gap_2().children(
                                    [
                                        (30i64, self.t("30 天", "30 days")),
                                        (90, self.t("90 天", "90 days")),
                                        (180, self.t("180 天", "180 days")),
                                        (365, self.t("365 天", "365 days")),
                                        (0, self.t("永久", "Forever")),
                                    ]
                                    .into_iter()
                                    .map(|(days, label)| {
                                        let active = retention == days;
                                        let id = if days == 0 {
                                            "retention-forever".to_owned()
                                        } else {
                                            format!("retention-{days}")
                                        };
                                        let button = stats_button(id).label(label).on_click(
                                            cx.listener(move |this, _, _, cx| {
                                                this.update_settings(enabled, days, revision, cx)
                                            }),
                                        );
                                        if active {
                                            button.primary().outline()
                                        } else {
                                            button.ghost()
                                        }
                                    }),
                                ),
                            )
                            .child(
                                Label::new(format!(
                                    "{} {} · {}",
                                    self.t("当前", "now"),
                                    if retention == 0 {
                                        self.t("永久", "forever").to_owned()
                                    } else {
                                        format!("{} {}", retention, self.t("天", "days"))
                                    },
                                    self.t("(0 = 永久)", "(0 = forever)")
                                ))
                                .text_size(px(crate::theme::font::LABEL))
                                .text_color(rgb(MUTED)),
                            ),
                    ),
            );

        let mode_rows = self
            .configured_providers
            .iter()
            .enumerate()
            .skip(self.storage_page * self.storage_page_size)
            .take(self.storage_page_size)
            .map(|(index, (key, name))| {
                let id = key.clone();
                let mode_settings = self.provider_modes.get(&id).cloned();
                let mode = mode_settings
                    .as_ref()
                    .map_or(UsageMode::default(), |settings| settings.usage_mode);
                let mode_revision = mode_settings
                    .as_ref()
                    .map_or(0, |settings| settings.revision);
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
                    .px(px(12.))
                    .py(px(10.))
                    .border_b(px(1.))
                    .border_color(rgb(0xeef2f7))
                    .child(Label::new(label).text_size(px(12.)).text_color(rgb(INK)))
                    .child(
                        Checkbox::new(("provider-mode", index))
                            .label(self.t(
                                "请求上游返回用量（request）",
                                "Request upstream usage (request mode)",
                            ))
                            .checked(mode == UsageMode::Request)
                            .disabled(
                                mode_settings.is_none()
                                    || self.backend.is_busy(&JobKey::ProviderMode(id.clone())),
                            )
                            .large()
                            .on_change(cx.listener(move |this, value: &bool, _, cx| {
                                this.set_provider_mode(id.clone(), *value, mode_revision, cx);
                            })),
                    )
                    .into_any_element()
            })
            .collect::<Vec<_>>();
        let modes_table = div()
            .flex()
            .flex_col()
            .w_full()
            .min_w_0()
            .border_1()
            .border_color(rgb(BORDER))
            .rounded(px(8.))
            .overflow_hidden()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .h(px(32.))
                    .px(px(12.))
                    .bg(rgb(0xf7f9fc))
                    .border_b(px(1.))
                    .border_color(rgb(BORDER))
                    .text_size(px(crate::theme::font::LABEL))
                    .text_color(rgb(MUTED))
                    .child(self.t("供应商", "Provider"))
                    .child(self.t("用量采集方式", "Capture mode")),
            )
            .children(if mode_rows.is_empty() {
                vec![
                    div()
                        .px(px(12.))
                        .py(px(16.))
                        .child(
                            Label::new(self.t("暂无供应商", "No providers"))
                                .text_size(px(crate::theme::font::LABEL))
                                .text_color(rgb(MUTED)),
                        )
                        .into_any_element(),
                ]
            } else {
                mode_rows
            });

        let modes_column = div()
            .flex()
            .flex_col()
            .gap_3()
            .flex_1()
            .min_w_0()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(stats_heading(
                        self.t("供应商采集模式", "Provider capture modes"),
                    ))
                    .child(
                        stats_button("provider-modes-refresh")
                            .ghost()
                            .child(crate::ui_components::action_icon(
                                Icon::new(IconName::RefreshCw).size(px(16.)),
                            ))
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
            .child(modes_table)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
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
                        .text_size(px(crate::theme::font::LABEL))
                        .text_color(rgb(MUTED)),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                stats_button("storage-prev")
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
                                stats_button("storage-page-size")
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
                                stats_button("storage-next")
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
                    ),
            );

        let columns = div()
            .flex()
            .gap(px(24.))
            .when(wide, |this| this.flex_row())
            .when(!wide, |this| this.flex_col())
            .child(settings_column)
            .child(modes_column);

        let danger = div()
            .flex()
            .flex_col()
            .gap_2()
            .w_full()
            .child(div().h(px(1.)).w_full().bg(rgb(BORDER)).mt_2())
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        Label::new(self.t("危险操作", "Danger zone"))
                            .text_size(px(crate::theme::font::LABEL))
                            .text_color(rgb(MUTED)),
                    )
                    .child(
                        crate::ui_components::button(
                            "clear-preview",
                            crate::ui_components::ButtonKind::DangerOutline,
                            self.t("清除历史统计…", "Clear history…"),
                            false,
                            false,
                        )
                        .on_click(cx.listener(|this, _, _, cx| this.preview_clear(cx))),
                    ),
            )
            .when_some(self.maintenance_error.clone(), |this, message| {
                this.child(
                    Label::new(message)
                        .text_size(px(crate::theme::font::LABEL))
                        .text_color(rgb(0xa4424a)),
                )
            })
            .when_some(progress, |this, task| {
                this.child(
                    Label::new(format!(
                        "{} {} · {} {}",
                        maintenance_label, task.kind, task.affected_rows, rows_label
                    ))
                    .text_size(px(crate::theme::font::LABEL))
                    .text_color(rgb(MUTED)),
                )
            })
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
                            .text_size(px(crate::theme::font::LABEL))
                            .text_color(rgb(0xa4424a)),
                        )
                        .child(
                            Label::new(self.t(
                                "保留去重标记，不修改 WorkBuddy 源文件。",
                                "Deduplication markers remain; WorkBuddy source files are unchanged.",
                            ))
                            .text_size(px(crate::theme::font::LABEL))
                            .text_color(rgb(MUTED)),
                        )
                        .child(
                            stats_button("clear-confirm")
                                .danger()
                                .label(self.t("确认清除", "Confirm clear"))
                                .on_click(cx.listener(|this, _, _, cx| this.confirm_clear(cx))),
                        ),
                )
            });

        let body = div()
            .flex()
            .flex_col()
            .gap_4()
            .child(columns)
            .child(self.import_controls(cx))
            .child(danger);
        stats_card("stats-storage")
            .child(self.section_toggle(
                "storage-toggle",
                self.t("采集与存储", "Collection & storage"),
                None,
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
        // DESIGN_SPEC §4.3: each price field carries the selected currency's
        // symbol. The symbol follows the currency control, never a stale draft.
        let symbol = match self
            .price_currency
            .read(cx)
            .selected_value()
            .map(String::as_str)
        {
            Some("USD") => "$",
            _ => "¥",
        };
        crate::ui_components::dialog_body()
            .w_full()
            .gap_3()
            .child(
                div()
                    .text_size(px(crate::theme::font::DIALOG_TITLE))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(if self.price_draft.is_editing() {
                        self.t("编辑价格", "Edit price")
                    } else {
                        self.t("新建价格规则", "New price rule")
                    }),
            )
            .child(
                div()
                    .text_size(px(crate::theme::font::LABEL))
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
                        crate::ui_components::select(
                            &self.price_provider,
                            self.price_draft.identity_locked(),
                        )
                        .w_full(),
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
                    .child(crate::ui_components::text_input(
                        &self.price_model,
                        self.price_draft.identity_locked(),
                    )),
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
                            .flex_grow(3.)
                            .child(self.filter_label(self.t("计价方式", "Pricing mode")))
                            .child(crate::ui_components::select(&self.price_mode, false).w_full()),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .flex_grow(2.)
                            .child(self.filter_label(self.t("币种", "Currency")))
                            .child(
                                crate::ui_components::select(&self.price_currency, false).w_full(),
                            ),
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
                            .child(
                                crate::ui_components::text_input(&self.price_input, false).prefix(
                                    Label::new(symbol).text_size(px(12.)).text_color(rgb(MUTED)),
                                ),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .flex_1()
                            .child(self.filter_label(self.t("输出价 / 百万", "Output / million")))
                            .child(
                                crate::ui_components::text_input(&self.price_output, false).prefix(
                                    Label::new(symbol).text_size(px(12.)).text_color(rgb(MUTED)),
                                ),
                            ),
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
                            .child(
                                crate::ui_components::text_input(&self.price_read, false).prefix(
                                    Label::new(symbol).text_size(px(12.)).text_color(rgb(MUTED)),
                                ),
                            ),
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
                            .child(
                                crate::ui_components::text_input(&self.price_write, false).prefix(
                                    Label::new(symbol).text_size(px(12.)).text_color(rgb(MUTED)),
                                ),
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(self.filter_label(self.t("备注", "Label")))
                    .child(crate::ui_components::text_input(&self.price_label, false)),
            )
            .child(
                Label::new(self.t(
                    "保存后只影响新请求；每个请求保留自己的单价快照",
                    "Saving only affects new requests; each request keeps its own price snapshot",
                ))
                .text_size(px(crate::theme::font::LABEL))
                .text_color(rgb(MUTED)),
            )
            .when_some(self.price_draft.error.clone(), |this, error| {
                this.child(
                    Label::new(error)
                        .text_size(px(crate::theme::font::LABEL))
                        .text_color(rgb(0xa4424a)),
                )
            })
            .child(
                crate::ui_components::dialog_footer().child(
                    stats_button("price-save")
                        .primary()
                        .child(crate::ui_components::action_icon(
                            Icon::new(IconName::Check).size(px(16.)),
                        ))
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

/// What an unknown value looks like. It is a dash, never a zero: a zero would
/// claim the value was measured and happened to be nothing.
pub const UNKNOWN_TEXT: &str = "—";

/// The nine metrics, in the order U08 fixes them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MetricId {
    /// Records in range. The subtitle splits them by origin.
    UsageRecords,
    /// Success rate, which only the proxy can justify.
    ProxySuccessRate,
    InputTokens,
    OutputTokens,
    CacheRead,
    CacheWrite,
    /// How complete the recorded usage is.
    RecordedCoverage,
    /// Estimated cost, per currency.
    EstimatedCost,
    /// WorkBuddy credit, summed on its own.
    WorkbuddyCredit,
}

impl MetricId {
    /// The heading.
    pub fn title(self) -> &'static str {
        match self {
            MetricId::UsageRecords => "用量记录",
            MetricId::ProxySuccessRate => "代理成功率",
            MetricId::InputTokens => "输入 Token",
            MetricId::OutputTokens => "输出 Token",
            MetricId::CacheRead => "缓存读取",
            MetricId::CacheWrite => "缓存创建",
            MetricId::RecordedCoverage => "已记录用量完整率",
            MetricId::EstimatedCost => "代理估算费用",
            MetricId::WorkbuddyCredit => "WorkBuddy 点数",
        }
    }

    /// The subtitle, which says what the number does and does not mean.
    pub fn subtitle(self) -> &'static str {
        match self {
            MetricId::UsageRecords => "代理 X · 导入 Y",
            MetricId::ProxySuccessRate => "仅代理请求有状态依据",
            MetricId::RecordedCoverage => "只统计已记录条目的用量完整度",
            MetricId::EstimatedCost => "导入费用未知 N 条",
            MetricId::WorkbuddyCredit => "日志记录值，不代表账号余额",
            MetricId::InputTokens
            | MetricId::OutputTokens
            | MetricId::CacheRead
            | MetricId::CacheWrite => "只汇总用量完整的已记录条目",
        }
    }
}

/// One metric's value.
///
/// `Unknown` is a first-class answer: it is what a pure-import range says about
/// the proxy's success rate, and what an unpriced import says about cost.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MetricValue {
    /// A measured number.
    Known(i64),
    /// No measurement exists.
    Unknown,
}

/// The compact text a metric shows.
pub fn metric_text(metric: MetricId, value: MetricValue) -> String {
    match value {
        MetricValue::Unknown => UNKNOWN_TEXT.to_owned(),
        MetricValue::Known(number) => match metric {
            // A rate is shown with one decimal; the caller supplies it as
            // per-mille so no floating point is involved.
            MetricId::ProxySuccessRate | MetricId::RecordedCoverage => {
                format!("{}.{}%", number / 10, number % 10)
            }
            MetricId::WorkbuddyCredit => {
                crate::usage_table::format_credit_nanos(&number.to_string())
                    .unwrap_or_else(|| "—".into())
            }
            // Everything else is a count or an amount.
            _ => compact_number(number),
        },
    }
}

/// A compact rendering of a large number, for a narrow card.
pub fn compact_number(value: i64) -> String {
    let negative = value < 0;
    let magnitude = value.unsigned_abs();
    let text = if magnitude >= 1_000_000_000 {
        format!("{:.1}B", magnitude as f64 / 1_000_000_000.0)
    } else if magnitude >= 1_000_000 {
        format!("{:.1}M", magnitude as f64 / 1_000_000.0)
    } else if magnitude >= 10_000 {
        format!("{:.1}K", magnitude as f64 / 1_000.0)
    } else {
        magnitude.to_string()
    };
    if negative { format!("-{text}") } else { text }
}

/// The smallest width a single metric may occupy.
/// Content width at which the two chart cards sit side by side (DESIGN_SPEC
/// §4.1.4). Below it they stack, which keeps 1100×700 readable.
pub const CHART_SIDE_BY_SIDE_WIDTH: f32 = 840.;
/// Content width at which the storage settings and the provider capture-mode
/// table stay side by side (DESIGN_SPEC §4.2). Below it they stack so the
/// 1100×700 window still reaches every control.
pub const STORAGE_SIDE_BY_SIDE_WIDTH: f32 = 980.;
pub const METRIC_MIN_WIDTH: i32 = 160;
/// The gap between metrics.
pub const METRIC_GAP: i32 = 12;

#[cfg(test)]
mod date_range_tests {
    use super::{
        RangePreset, local_midnight_days_ago, now_ms, resolve_custom_range, validate_date_draft,
    };

    #[test]
    fn fixed_and_following_ranges_resolve_without_mutating_applied_snapshot() {
        let applied = (100, 200);
        assert_eq!(resolve_custom_range(applied, false, 300), (100, 200));
        assert_eq!(resolve_custom_range(applied, true, 300), (100, 300));
        assert_eq!(applied, (100, 200));
    }

    #[test]
    fn date_draft_validates_both_fields_but_following_uses_the_clock() {
        let now = crate::usage_table::parse_local_time("2026-10-16 12:00").unwrap();
        let from = crate::usage_table::parse_local_time("2026-10-15 00:00").unwrap();
        assert_eq!(
            validate_date_draft("2026-10-15 00:00", "2026-10-16 12:00", false, now),
            Ok((from, now))
        );
        assert_eq!(
            validate_date_draft("2026-10-15 00:00", "", true, now),
            Ok((from, now))
        );
        assert!(validate_date_draft("invalid", "2026-10-16 12:00", false, now).is_err());
        assert!(validate_date_draft("2026-10-15 00:00", "", false, now).is_err());
        assert_eq!(
            validate_date_draft("2026-10-16 12:01", "", true, now),
            Err("reversed_date")
        );
        assert_eq!(
            validate_date_draft("2025-01-01 00:00", "", true, now),
            Err("range_too_long")
        );
    }

    #[test]
    fn one_day_means_24_hours_and_custom_has_no_relative_bounds() {
        let now = now_ms();
        assert_eq!(RangePreset::Day.bounds(now), Some((now - 86_400_000, now)));
        assert_eq!(RangePreset::Custom.bounds(now), None);
        let (from, to) = RangePreset::Fortnight.bounds(now).unwrap();
        assert_eq!(from, local_midnight_days_ago(13));
        assert_eq!(to, now);
    }
}

#[cfg(test)]
mod metric_text_tests {
    use super::{
        METRIC_GAP, METRIC_MIN_WIDTH, MetricId, MetricValue, UNKNOWN_TEXT, compact_number,
        metric_text,
    };

    #[test]
    fn an_unknown_metric_reads_as_a_dash_and_never_as_a_zero() {
        // AC068: "unknown" and "measured zero" are different answers.
        assert_eq!(UNKNOWN_TEXT, "—");
        for metric in [
            MetricId::UsageRecords,
            MetricId::ProxySuccessRate,
            MetricId::InputTokens,
            MetricId::OutputTokens,
            MetricId::CacheRead,
            MetricId::CacheWrite,
            MetricId::RecordedCoverage,
            MetricId::EstimatedCost,
            MetricId::WorkbuddyCredit,
        ] {
            assert_eq!(
                metric_text(metric, MetricValue::Unknown),
                "—",
                "{} must show a dash when unknown",
                metric.title()
            );
        }
    }

    #[test]
    fn a_pure_import_range_has_no_success_rate() {
        // AC068: the proxy's success rate needs proxy status; with none, the
        // metric is unknown rather than 0%.
        let rate = match None::<f64> {
            Some(value) => MetricValue::Known((value * 1000.0).round() as i64),
            None => MetricValue::Unknown,
        };
        assert_eq!(metric_text(MetricId::ProxySuccessRate, rate), "—");
        // A real rate still renders with one decimal.
        assert_eq!(
            metric_text(MetricId::ProxySuccessRate, MetricValue::Known(997)),
            "99.7%"
        );
    }

    #[test]
    fn a_measured_zero_is_still_a_zero() {
        // The counterpart of the rule above: a known zero is shown as 0.
        assert_eq!(
            metric_text(MetricId::WorkbuddyCredit, MetricValue::Known(0)),
            "0"
        );
        assert_eq!(
            metric_text(MetricId::UsageRecords, MetricValue::Known(0)),
            "0"
        );
    }

    #[test]
    fn large_numbers_use_a_compact_form() {
        assert_eq!(compact_number(999), "999");
        assert_eq!(compact_number(9_999), "9999");
        assert_eq!(compact_number(12_345), "12.3K");
        assert_eq!(compact_number(2_500_000), "2.5M");
        assert_eq!(compact_number(-12_345), "-12.3K");
    }

    #[test]
    fn the_metric_row_geometry_matches_the_spec() {
        assert_eq!(METRIC_MIN_WIDTH, 160);
        assert_eq!(METRIC_GAP, 12);
    }
}

mod detail_metadata;
