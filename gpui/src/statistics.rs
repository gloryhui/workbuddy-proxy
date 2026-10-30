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
mod analysis;
mod detail_layout;
mod event_metrics;
mod import_controls;
mod layout;
mod mapping_controls;
mod overview;
mod pagination;
mod preferences;
mod price_layout;
mod recent;
mod settings_capture;
mod settings_layout;
mod source_controls;
mod trends;

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

/// Build labels from the same page-size contract at creation and locale changes.
fn page_size_choices(zh: bool) -> Vec<Choice> {
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
        .collect()
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
    last_report_view: preferences::View,
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
    analysis_scroll: ScrollHandle,
    settings_scroll: ScrollHandle,
    provider_modes_scroll: ScrollHandle,
    maintenance_anchor: ScrollAnchor,
    price_rows_scroll: ScrollHandle,
    price_form_scroll: ScrollHandle,
    settings_anchors: [ScrollAnchor; 3],
    settings_section: usize,
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
    recent: recent::RecentRead<RequestRowDto>,
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
    detail_request_id: Option<String>,
    detail_error: Option<String>,
    detail_scroll: ScrollHandle,
    detail_metadata_scroll: ScrollHandle,
    // P1.
    settings: Option<SettingsDto>,
    provider_modes: BTreeMap<String, ProviderSettingsDto>,
    prices: Vec<PriceRuleDto>,
    price_sync: Option<crate::stats_dto::PriceSyncDto>,
    price_sync_pending: bool,
    price_sync_error: Option<String>,
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
        let settings_scroll = ScrollHandle::new();
        let settings_anchors =
            std::array::from_fn(|_| ScrollAnchor::for_handle(settings_scroll.clone()));
        let backend = StatsScheduler::new(dir.clone());
        let page_input = cx.new(|cx| InputState::new(window, cx).placeholder("1"));
        let page_size_select =
            cx.new(|cx| SelectState::new(page_size_choices(zh), None, window, cx));
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
            last_report_view: preferences::View::Details,
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
            recent: recent::RecentRead::default(),
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
            analysis_scroll: ScrollHandle::new(),
            maintenance_anchor: ScrollAnchor::for_handle(settings_scroll.clone()),
            provider_modes_scroll: ScrollHandle::new(),
            settings_scroll,
            price_rows_scroll: ScrollHandle::new(),
            price_form_scroll: ScrollHandle::new(),
            settings_anchors,
            settings_section: 0,
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
            detail_request_id: None,
            detail_error: None,
            detail_scroll: ScrollHandle::new(),
            detail_metadata_scroll: ScrollHandle::new(),
            settings: None,
            provider_modes: BTreeMap::new(),
            prices: Vec::new(),
            price_sync: None,
            price_sync_pending: false,
            price_sync_error: None,
            price_open: false,
            price_closing: false,
            price_draft: crate::stats_controller::PriceDraft::blank(String::new(), 0),
            maintenance: None,
            maintenance_error: None,
            clear_preview: None,
            clear_token: None,
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
        self.subscribe_price_focus(window, cx);
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
            self.recent.invalidate();
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
            (&self.page_size_select, page_size_choices(self.zh)),
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
            (&self.price_label, self.t("备注", "Label")),
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
        self.recent.invalidate();
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
        self.price_rows_scroll.set_offset(point(px(0.), px(0.)));
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
        if self.view == preferences::View::Overview {
            self.request_recent(cx);
        } else {
            match self.tab {
                Tab::Requests => self.request_rows(cx),
                _ => self.request_groups(cx),
            }
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
        if foreground && self.view == preferences::View::Settings {
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
        if kind == JobKind::Recent {
            if self.visible && self.view == preferences::View::Overview {
                let parsed = result.and_then(|value| {
                    serde_json::from_value::<crate::stats_dto::RequestPageDto>(value)
                        .map(|page| page.items.into_iter().next())
                        .map_err(|_| "Invalid statistics response".to_string())
                });
                if self.recent.apply(seq, self.generation, parsed) {
                    cx.notify();
                }
            }
            return;
        }
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
                    self.price_sync_error = Some(failure.clone());
                }
                if kind == JobKind::Prices {
                    self.prices_loading = false;
                }
                if kind == JobKind::Detail {
                    self.detail_error = Some(failure.clone());
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
            let previous_page = match self.tab {
                Tab::Requests => self.rows_cursors.page_number(),
                _ => self.group_pages.page_number(),
            };
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
            // A same-page refresh must not replace an unsubmitted edit. A
            // committed navigation still normalizes the input even when a
            // mouse-activated pager button leaves focus in the text field.
            if page != previous_page
                || !self.page_input.read(cx).focus_handle(cx).is_focused(window)
            {
                self.page_input.update(cx, |state, cx| {
                    state.set_value(page.to_string(), window, cx)
                });
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
            self.price_sync_error = None;
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
                if self.detail_request_id.as_deref() == Some(detail.request.request_id.as_str()) {
                    self.detail_error = None;
                    self.details = Some(detail);
                } else {
                    self.detail_error = Some(
                        self.t(
                            "请求标识不匹配，请重试读取",
                            "Request ID mismatch; retry reading the record",
                        )
                        .to_owned(),
                    );
                }
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
                let anchor = self.maintenance_anchor.clone();
                window.on_next_frame(move |window, cx| anchor.scroll_to(window, cx));
            }
        }
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
            View::Settings => self.t("成本与采集", "Costs & collection"),
        }
    }

    pub fn page_subtitle(&self) -> &'static str {
        if self.view == preferences::View::Details {
            self.t(
                "逐条查看用量、缓存命中、费用与响应性能",
                "Inspect usage, cache hits, costs and response performance for each request",
            )
        } else if self.view == preferences::View::Summary {
            self.t(
                "按供应商和模型汇总请求，比较使用规模与效率",
                "Group requests by provider and model to compare usage and efficiency",
            )
        } else if self.view == preferences::View::Settings {
            self.t(
                "管理估算价格、存储策略和 WorkBuddy 本地用量",
                "Manage estimated pricing, storage and local WorkBuddy usage",
            )
        } else {
            self.t(
                "掌握模型请求、Token 消耗与缓存表现",
                "Explore model requests, token usage and cache performance",
            )
        }
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
        let control = Popover::new("stats-date-range")
            .open(self.date_open)
            .trigger(
                stats_button("stats-date-trigger")
                    .h(px(42.))
                    .w_full()
                    .outline()
                    .bg(rgb(0xffffff))
                    .border_color(rgb(BORDER))
                    .text_color(rgb(crate::theme::color::TEXT_2))
                    .rounded(px(10.))
                    .child(
                        div()
                            .w_full()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(6.))
                            .child(
                                Label::new(self.range.title(self.zh))
                                    .text_size(px(12.))
                                    .line_height(px(17.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(rgb(crate::theme::color::TEXT_2)),
                            )
                            .child(source_controls::caret_spacer()),
                    )
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
            .into_any_element();
        source_controls::caret_surface(control)
    }

    fn refresh_control(&self, cx: &mut Context<Self>) -> AnyElement {
        let owner = cx.entity().downgrade();
        let content_owner = owner.clone();
        let label = if self.refresh_interval == 0 {
            self.t("自动刷新 · 关闭", "Auto refresh · Off").to_owned()
        } else {
            format!(
                "{} · {} {}",
                self.t("自动刷新", "Auto refresh"),
                self.refresh_interval,
                self.t("秒", "s")
            )
        };
        let control = Popover::new("stats-auto-refresh")
            .open(self.refresh_open)
            .trigger(
                stats_button("stats-auto-trigger")
                    .h(px(42.))
                    .w_full()
                    .outline()
                    .bg(rgb(0xffffff))
                    .border_color(rgb(BORDER))
                    .text_color(rgb(crate::theme::color::TEXT_2))
                    .rounded(px(10.))
                    .child(
                        div().w_full().flex().items_center().justify_between().gap(px(6.))
                            .child(Label::new(label).text_size(px(12.)).line_height(px(17.)).font_weight(FontWeight::MEDIUM).text_color(rgb(crate::theme::color::TEXT_2)))
                            .child(source_controls::caret_spacer()),
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
            .into_any_element();
        source_controls::caret_surface(control)
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
        let slots = crate::source_filter::SLOTS;
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
            .child(self.filter_row(0, slots, cx))
            .when_some(self.error.clone(), |this, error| {
                this.child(
                    Label::new(crate::i18n::message(&error, self.zh))
                        .text_size(px(crate::theme::font::LABEL))
                        .text_color(rgb(0xa4424a)),
                )
            })
            .into_any_element()
    }

    pub(super) fn more_filter_panel(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let tests_available = self.capture.include_tests_available(self.source_scope());
        div()
            .flex()
            .flex_wrap()
            .items_end()
            .gap_3()
            .pt_3()
            .border_t_1()
            .border_color(rgb(BORDER))
            .child(
                div()
                    .w_full()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        Label::new(self.t("更多筛选", "More filters"))
                            .text_size(px(15.))
                            .font_weight(FontWeight::SEMIBOLD),
                    )
                    .child(
                        stats_button("stats-more-close")
                            .ghost()
                            .label(self.t("完成", "Done"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.more_filters_open = false;
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(self.filter_label(self.t("记录类型", "Record type")))
                    .child(div().w(px(164.)).child(
                        self.filter_select(&self.kind_select, self.t("记录类型", "Record type")),
                    )),
            )
            .child(self.capture_control())
            .child(
                div()
                    .w(px(150.))
                    .child(crate::ui_components::select(&self.outcome_select, false).w_full()),
            )
            .child(self.filter_search(true, cx))
            .child(self.filter_search(false, cx))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(self.filter_label(self.t("用量完整性", "Usage completeness")))
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
            )
            .id("stats-more-filter-content")
            .w(px((f32::from(window.viewport_size().width) - 48.).min(740.)))
            .max_h(px(
                (f32::from(window.viewport_size().height) - 160.).max(160.)
            ))
            .overflow_y_scroll()
            .p(px(8.))
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

    /// Tabs, table and pagination.
    fn details(&self, cx: &mut Context<Self>) -> AnyElement {
        let table: AnyElement = match self.tab {
            // Studio V2 uses a separate eight-column request log. Aggregate
            // rows retain their native table and existing sort handlers.
            Tab::Requests => crate::ui_components::log_table(&self.usage_table)
                .with_size(px(52.))
                // Kit adds an outer one-pixel header divider.
                .header_row_height(px(38.))
                .into_any_element(),
            _ => crate::ui_components::table(&self.group_table)
                .with_size(px(43.))
                .header_row_height(px(37.))
                .into_any_element(),
        };
        stats_card("stats-details")
            .flex_1()
            .when(self.view == preferences::View::Details, |panel| {
                panel.max_h(px(540.))
            })
            .min_h_0()
            .overflow_hidden()
            .gap_2()
            .when(self.tab == Tab::Requests, |panel| {
                panel
                    .child(
                        div()
                            .flex()
                            .flex_shrink_0()
                            .flex_wrap()
                            .items_center()
                            .justify_between()
                            .gap(px(8.))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(14.))
                                    .child(
                                        Label::new(self.t("请求记录", "Request records"))
                                            .text_size(px(17.))
                                            .line_height(px(24.))
                                            .font_weight(FontWeight::BOLD)
                                            .text_color(rgb(INK)),
                                    )
                                    .child(self.request_count_summary(cx)),
                            )
                            // The core supports only newest-first request ordering.
                            // Show its actual order without a non-functional menu.
                            .child(
                                Label::new(self.t("时间 · 最新优先", "Time · newest first"))
                                    .text_size(px(12.))
                                    .text_color(rgb(MUTED)),
                            ),
                    )
                    .child(
                        Label::new(self.t(
                            "代理请求和 WorkBuddy 导入统一展示，未知字段保留原始状态",
                            "Proxy requests and WorkBuddy imports; unknown fields remain unknown",
                        ))
                        .w_full()
                        .mb(px(10.))
                        .text_ellipsis()
                        .text_size(px(12.))
                        .line_height(px(17.))
                        .text_color(rgb(MUTED)),
                    )
            })
            .when(self.tab != Tab::Requests, |panel| {
                // Figma57:8: the native border plus these insets place the
                // title at (20,16) and the table at (15,80) within the card.
                panel
                    .pl(px(14.))
                    .pr(px(16.))
                    .pt(px(15.))
                    .gap(px(12.))
                    .child(self.analysis_table_header(cx))
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
    /// Use Kit's shared header/body scroll handle so right-hand columns are
    /// reachable even when the OS hides overlay scrollbars or has no trackpad.
    fn table_column_navigation(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .flex()
            .items_center()
            .justify_end()
            .gap(px(6.))
            .flex_shrink_0()
            .child(
                stats_button("table-first-column")
                    .ghost()
                    .when(self.view == preferences::View::Details, |button| {
                        button.h(px(32.))
                    })
                    .label(self.t("首列", "First"))
                    .tooltip(self.t("返回首列", "First column"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        match this.tab {
                            Tab::Requests => this
                                .usage_table
                                .update(cx, |table, cx| table.scroll_to_col(0, cx)),
                            _ => this
                                .group_table
                                .update(cx, |table, cx| table.scroll_to_col(0, cx)),
                        }
                    })),
            )
            .child(
                stats_button("table-last-column")
                    .ghost()
                    .when(self.view == preferences::View::Details, |button| {
                        button.h(px(32.))
                    })
                    .label(self.t("右侧列", "Rightmost"))
                    .tooltip(self.t("查看右侧列", "Rightmost columns"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        match this.tab {
                            Tab::Requests => this.usage_table.update(cx, |table, cx| {
                                table.scroll_to_col(usage_table::REQUEST_COLUMNS - 1, cx)
                            }),
                            _ => this
                                .group_table
                                .update(cx, |table, cx| table.scroll_to_col(5, cx)),
                        }
                    })),
            )
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
        let compact = self.view == preferences::View::Details;
        div()
            .flex()
            .items_center()
            .gap_2()
            .flex_wrap()
            .flex_shrink_0()
            .when(self.tab != Tab::Requests, |footer| {
                footer.child(self.analysis_sort_control(cx))
            })
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
                .when(compact, |button| button.h(px(32.)))
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
                .when(compact, |button| button.h(px(32.)))
                .on_click(cx.listener(move |this, _, _, cx| this.navigate_page(page + 1, cx))),
            )
            .child(
                div().w(px(125.)).child(
                    crate::ui_components::select(&self.page_size_select, busy)
                        .when(compact, |select| select.h(px(32.))),
                ),
            )
            .child(Label::new(self.t("跳至", "Go to")).text_size(px(crate::theme::font::LABEL)))
            .child(
                div().w(px(70.)).child(
                    crate::ui_components::text_input(&self.page_input, busy || total == 0)
                        .when(compact, |input| input.h(px(32.))),
                ),
            )
            .child(
                stats_button("page-go")
                    .ghost()
                    .when(compact, |button| button.h(px(32.)))
                    .label(self.t("跳转", "Go"))
                    .disabled(busy || total == 0)
                    .on_click(cx.listener(|this, _, _, cx| this.jump_to_input(cx))),
            )
            .child(self.table_column_navigation(cx))
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
            .when(self.view != preferences::View::Settings, |page| {
                // Figma52/56/57: tabs47px, controls start8px later, then19px
                // before the request panel. Wrapping controls may still grow.
                page.gap_0()
                    .child(
                        div()
                            .min_h(px(47.))
                            .pt(px(2.))
                            .flex_shrink_0()
                            .child(self.view_tabs(cx)),
                    )
                    .child(
                        div()
                            .pt(px(8.))
                            .pb(px(19.))
                            .flex_shrink_0()
                            .child(self.filters(cx)),
                    )
            })
            .bg(rgb(BG))
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

impl Statistics {
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
pub const METRIC_MIN_WIDTH: i32 = 180;
/// The gap between metrics.
pub const METRIC_GAP: i32 = 16;

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
        assert_eq!(METRIC_MIN_WIDTH, 180);
        assert_eq!(METRIC_GAP, 16);
    }
}

mod detail_metadata;
