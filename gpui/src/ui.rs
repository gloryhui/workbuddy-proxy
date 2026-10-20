use crate::{
    backend::{Backend, Job},
    config::{self, Config},
    state::{Draft, Drafts, Selection},
    statistics::Statistics,
};
use gpui_kit::component::{
    ActiveTheme, ElementExt, Icon, IconName, InteractiveElementExt as _, Selectable, Sizable,
    TitleBar, WindowExt,
    button::{Button, ButtonVariants},
    input::{InputEvent, InputState, Textarea, TextareaState},
    label::Label,
    list::ListItem,
    menu::{DropdownMenu, PopupMenuItem},
    select::{SearchableVec, SelectEvent, SelectItem, SelectState},
    table::TableState,
};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    path::PathBuf,
    time::{Duration, Instant},
};
gpui_kit::actions!(
    proxy,
    [Save, RemoveModels, SelectModels, EditModel, Dismiss, Quit]
);

/// Top-level pages of the settings window.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Providers,
    Statistics,
}

/// Trusted relay health shown by the T03 window skeleton (difference D13).
///
/// The value is derived only from the real `poll` reply that the window already
/// consumes, so the status bar can never claim a healthy relay on its own. A
/// missing or failed reply stays visibly unknown instead of reading as normal.
#[derive(Clone, Copy, PartialEq, Eq)]
enum CoreState {
    /// No poll reply yet, or the reply carried no relay address.
    Unknown,
    /// The most recent poll returned a snapshot, so the core answered.
    Responding,
    /// The most recent poll did not return a snapshot, so the core did not answer.
    Unreachable,
}

const BG: u32 = 0xf7f8f9;
const INK: u32 = 0x19334d;
const MUTED: u32 = 0x6b7c90;
// T03 skeleton geometry comes from the T01 tokens in `theme`: 224px sidebar,
// 36px title toolbar, 30px status bar, 34px provider rows. Only the brand mark
// and the provider panel's title row have no token and stay local. Page bodies
// keep their own values until T04–T08 migrate them.
const BRAND_MARK_SIZE: f32 = 34.;
const BRAND_MARK_RADIUS: f32 = 10.;
const PROJECT_URL: &str = "https://github.com/gloryhui/workbuddy-proxy";
const FEEDBACK_URL: &str = "https://github.com/gloryhui/workbuddy-proxy/issues/new";

/// Sidebar navigation label. DESIGN_SPEC §2 defines the selected item as
/// primary-dark and 600 over a primary-tint row; unselected items stay regular.
fn navigation_title(text: &'static str, active: bool) -> Label {
    Label::new(text)
        .text_size(px(15.))
        .line_height(px(20.))
        .font_weight(if active {
            FontWeight::SEMIBOLD
        } else {
            FontWeight::NORMAL
        })
        .text_color(rgb(if active {
            crate::theme::color::PRIMARY_DARK
        } else {
            crate::theme::color::TEXT_1
        }))
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
        self.title.to_lowercase().contains(&query)
            || self.code.to_lowercase().contains(&query)
            || crate::presets::all().iter().any(|p| {
                p.id == self.code
                    && (p.name.to_lowercase().contains(&query)
                        || p.url.to_lowercase().contains(&query))
            })
    }
}
type ChoiceState = SelectState<Vec<Choice>>;
fn protocol_choices() -> Vec<Choice> {
    [
        ("openai_chat", "OpenAI Chat Completions"),
        ("openai_responses", "OpenAI Responses"),
        ("anthropic_messages", "Anthropic Messages"),
        ("gemini_generate_content", "Gemini GenerateContent"),
    ]
    .into_iter()
    .map(|(code, title)| Choice {
        code: code.into(),
        title: title.into(),
    })
    .collect()
}
fn choices(zh: bool, languages: bool) -> Vec<Choice> {
    let entries: &[(&str, &str, &str)] = if languages {
        &[
            ("auto", "跟随系统", "System default"),
            ("zh", "简体中文", "简体中文"),
            ("en", "English", "English"),
        ]
    } else {
        &[
            ("keep", "保留各模型设置", "Keep per-model settings"),
            ("default", "跟随 WorkBuddy", "Follow WorkBuddy"),
            ("none", "关闭思考", "Thinking off"),
            ("low", "低", "Low"),
            ("high", "高", "High"),
            ("max", "最高", "Max"),
        ]
    };
    entries
        .iter()
        .map(|(code, cn, en)| Choice {
            code: (*code).into(),
            title: (if zh { *cn } else { *en }).into(),
        })
        .collect()
}
fn preset_choices(zh: bool) -> SearchableVec<Choice> {
    SearchableVec::new(
        std::iter::once(Choice {
            code: "custom".into(),
            title: (if zh {
                "自定义地址"
            } else {
                "Custom endpoint"
            })
            .into(),
        })
        .chain(crate::presets::all().iter().map(|p| Choice {
            code: p.id.clone(),
            title: p.title(zh).to_owned().into(),
        }))
        .collect::<Vec<_>>(),
    )
}
pub struct Proxy {
    dir: PathBuf,
    page: Page,
    providers_expanded: bool,
    statistics: Entity<Statistics>,
    cfg: Config,
    drafts: Drafts,
    backend: Backend,
    zh: bool,
    busy: bool,
    polling: bool,
    /// Last real core health observed by `tick`'s poll loop; the window skeleton
    /// reports this instead of a hardcoded "running" label (difference D13).
    core_state: CoreState,
    metadata_polling: bool,
    metadata_context: Value,
    last_metadata: Instant,
    status: String,
    error: bool,
    name: Entity<InputState>,
    key: Entity<InputState>,
    url: Entity<InputState>,
    port: Entity<InputState>,
    search: Entity<InputState>,
    edit_name: Entity<InputState>,
    edit_model: Entity<InputState>,
    pub(crate) model_options: crate::model_options::OptionsEditor,
    logs: Entity<TextareaState>,
    log_text: String,
    summary: String,
    proxy_address: String,
    proxy_counts: [u64; 3],
    models: Vec<Value>,
    effort: String,
    selection: Selection,
    table_focus: FocusHandle,
    model_table: Entity<TableState<crate::model_table::Models>>,
    table_width: Pixels,
    dialog_open: bool,
    protocol_open: bool,
    protocol_select: Entity<ChoiceState>,
    protocol_options: crate::protocol_options::Editor,
    focus: FocusHandle,
    picker: Option<Vec<String>>,
    picked: BTreeSet<usize>,
    only_picked: bool,
    editing: Option<usize>,
    new_model: bool,
    effort_select: Entity<ChoiceState>,
    preset_select: Entity<SelectState<SearchableVec<Choice>>>,
    delete_open: bool,
    sync_open: bool,
    unsync_open: bool,
    startup_checked: bool,
    startup_report: Option<crate::workbuddy_status::EndpointCheck>,
    startup_sync_open: bool,
    pending_port: Option<u16>,
    resetting_window: bool,
    wake: String,
    last_poll: Instant,
    last_dirty: String,
    _timer: Task<()>,
}
fn input(
    value: &str,
    hint: &str,
    masked: bool,
    window: &mut Window,
    cx: &mut App,
) -> Entity<InputState> {
    cx.new(|cx| {
        let mut state = InputState::new(window, cx)
            .placeholder(hint.to_owned())
            .masked(masked);
        state.set_value(value.to_owned(), window, cx);
        state
    })
}
/// Provider-page field label (DESIGN_SPEC §1.2): 12px muted, with no incidental
/// margin so the gap to its control comes from the layout tokens.
fn field_label(s: impl Into<SharedString>) -> Label {
    Label::new(s)
        .text_size(px(crate::theme::font::LABEL))
        .line_height(px(crate::theme::line_height::LABEL))
        .text_color(rgb(crate::theme::color::TEXT_3))
}
/// 13px dialog body line (addresses and other single values).
fn body_line(text: impl Into<SharedString>) -> Label {
    Label::new(text)
        .text_size(px(crate::theme::font::BODY))
        .line_height(px(crate::theme::line_height::BODY))
        .text_color(rgb(crate::theme::color::TEXT_1))
}
/// 12px muted dialog body hint.
fn body_hint(text: impl Into<SharedString>) -> Label {
    Label::new(text)
        .text_size(px(crate::theme::font::LABEL))
        .line_height(px(crate::theme::line_height::LABEL))
        .text_color(rgb(crate::theme::color::TEXT_3))
}
/// Muted 11px helper line (URL hint, model-list keyboard hint).
fn hint_line(s: impl Into<SharedString>) -> Label {
    Label::new(s)
        .text_size(px(crate::theme::font::SMALL))
        .text_color(rgb(crate::theme::color::TEXT_3))
}
/// Layout surfaces are not interactive controls. GroupBox adds nested intrinsic-
/// height wrappers, so flexible table/log panels use GPUI layout containers.
fn card(id: &'static str) -> Stateful<Div> {
    crate::ui_components::panel_card(id)
}
/// Right-rail card: content-sized like `content_card`, but with the design's
/// larger card padding (DESIGN_SPEC §3 "Card：内边距 16–20").
fn rail_card(id: &'static str) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .flex_col()
        .min_h_0()
        .w_full()
        .bg(rgb(crate::theme::color::WHITE))
        .border_1()
        .border_color(rgb(crate::theme::color::BORDER))
        .rounded(px(crate::theme::radius::CARD))
        .p(px(crate::theme::space::CARD_LG))
        .gap(px(crate::theme::space::CARD_GAP))
}

impl Proxy {
    fn set_language(&mut self, code: &str, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy || self.cfg.language == code {
            return;
        }
        self.capture(cx);
        self.zh = code == "zh"
            || (code == "auto" && sys_locale::get_locale().is_some_and(|s| s.starts_with("zh")));
        let zh = self.zh;
        gpui_kit::component::set_locale(if zh { "zh-CN" } else { "en" });
        self.statistics
            .update(cx, |page, cx| page.set_language(zh, window, cx));
        let zh = self.zh;
        for (state, hint) in [
            (
                &self.name,
                if zh {
                    "填写供应商名称"
                } else {
                    "Enter provider name"
                },
            ),
            (
                &self.key,
                if zh {
                    "填写 API Key"
                } else {
                    "Enter API Key"
                },
            ),
            (
                &self.url,
                if zh {
                    "填写供应商提供的 API 基础地址"
                } else {
                    "Enter your provider's API base URL"
                },
            ),
            (
                &self.search,
                if zh {
                    "搜索模型 ID"
                } else {
                    "Search model IDs"
                },
            ),
            (
                &self.edit_name,
                if zh {
                    "默认使用模型 ID"
                } else {
                    "Defaults to model ID"
                },
            ),
            (
                &self.edit_model,
                if zh {
                    "填写模型 ID"
                } else {
                    "Enter model ID"
                },
            ),
        ] {
            state.update(cx, |s, cx| s.set_placeholder(hint.to_owned(), window, cx));
        }
        self.refresh_choices(window, cx);
        self.send(
            "language",
            vec![json!({"command":"language", "language":code})],
            cx,
        );
    }
    fn refresh_choices(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.effort_select.update(cx, |s, cx| {
            s.set_items(choices(self.zh, false), window, cx);
            s.set_selected_value(&self.effort, window, cx);
        });
        self.preset_select
            .update(cx, |s, cx| s.set_items(preset_choices(self.zh), window, cx));
        self.sync_preset(window, cx);
    }
    fn language_menu(&self, cx: &mut Context<Self>) -> AnyElement {
        let items = choices(self.zh, true);
        let selected = self.cfg.language.clone();
        let owner = cx.entity().downgrade();
        crate::ui_components::design_header_button(
            "language",
            "design/header-globe.png",
            self.t("切换界面语言", "Change interface language"),
            self.t("界面语言", "Interface language"),
            self.busy,
        )
        .dropdown_menu_with_anchor(Anchor::TopRight, move |mut menu, _, _| {
            for item in &items {
                let code = item.code.clone();
                let owner = owner.clone();
                menu = menu.item(
                    PopupMenuItem::new(item.title.clone())
                        .checked(code == selected)
                        .on_click(move |_, window, cx| {
                            let _ =
                                owner.update(cx, |this, cx| this.set_language(&code, window, cx));
                        }),
                );
            }
            menu.min_w(px(180.))
        })
        .into_any_element()
    }
    fn app_controls(&self, cx: &mut Context<Self>) -> AnyElement {
        // Match the original six glyphs and 34px center-to-center spacing.
        // Their real actions, tooltips and disabled rules remain unchanged.
        div()
            .id("titlebar-actions")
            .flex()
            .flex_shrink_0()
            .h(px(crate::theme::size::TOP_ICON_ROW_HEIGHT))
            .items_center()
            .gap(px(2.))
            // Exclude actions from the TitleBar's native Windows drag hitbox
            // and prevent macOS mouse movement/double clicks from dragging or
            // zooming the window while the user operates a toolbar button.
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_double_click(|_, _, cx| cx.stop_propagation())
            .child(
                crate::ui_components::design_header_button(
                    "autostart",
                    "design/header-power.png",
                    if self.cfg.autostart {
                        self.t(
                            "开机启动：已开启（点击关闭）",
                            "Launch at login: on (click to disable)",
                        )
                    } else {
                        self.t(
                            "开机启动：已关闭（点击开启）",
                            "Launch at login: off (click to enable)",
                        )
                    },
                    self.t("开机启动", "Launch at login"),
                    self.busy,
                )
                .selected(self.cfg.autostart)
                .toggled(self.cfg.autostart)
                .on_click(cx.listener(|this, _, _, cx| {
                    let enabled = !this.cfg.autostart;
                    this.send(
                        "autostart",
                        vec![json!({"command":"autostart","enabled":enabled})],
                        cx,
                    );
                })),
            )
            .child(
                crate::ui_components::design_header_button(
                    "check-update",
                    "design/header-refresh.png",
                    self.t("检查更新", "Check updates"),
                    self.t("检查更新", "Check updates"),
                    self.busy,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.send(
                        "check_update",
                        vec![json!({"command":"check_update","force":true})],
                        cx,
                    )
                })),
            )
            .child(self.language_menu(cx))
            .child(
                crate::ui_components::design_header_button(
                    "restore-window-size",
                    "design/header-fullscreen.png",
                    self.t(
                        "恢复默认窗口尺寸（1440 × 900）",
                        "Restore default window size (1440 × 900)",
                    ),
                    self.t("恢复默认窗口尺寸", "Restore default window size"),
                    self.resetting_window,
                )
                .on_click(cx.listener(|this, _, window, cx| this.restore_window_size(window, cx))),
            )
            .child(
                crate::ui_components::design_header_button(
                    "project-homepage",
                    "design/header-github.png",
                    self.t("GitHub 项目主页", "GitHub project"),
                    self.t("GitHub 项目主页", "GitHub project"),
                    false,
                )
                .on_click(|_, _, cx| cx.open_url(PROJECT_URL)),
            )
            .child(
                crate::ui_components::design_header_button(
                    "submit-feedback",
                    "design/header-user.png",
                    self.t("提交反馈", "Submit feedback"),
                    self.t("提交反馈", "Submit feedback"),
                    false,
                )
                .on_click(|_, _, cx| cx.open_url(FEEDBACK_URL)),
            )
            .into_any_element()
    }
    fn restore_window_size(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.resetting_window {
            return;
        }
        if !window.is_fullscreen() && !window.is_maximized() {
            window.resize(crate::default_window_size());
            return;
        }
        self.resetting_window = true;
        if window.is_fullscreen() {
            window.toggle_fullscreen();
        }
        cx.notify();
        // Fullscreen/zoom transitions are asynchronous on macOS. Wait for a
        // normal window before resizing, and issue unzoom at most once.
        let started = Instant::now();
        cx.spawn_in(window, async move |this, cx| {
            let mut unzoom_requested = false;
            loop {
                cx.background_executor().timer(Duration::from_millis(100)).await;
                let done = this.update_in(cx, |this, window, cx| {
                    if started.elapsed() > Duration::from_secs(5) {
                        this.resetting_window = false;
                        this.error = true;
                        this.status = this.t("窗口状态切换未完成，请退出全屏或最大化后重试", "Window transition did not finish; leave fullscreen or maximize mode and try again").into();
                        cx.notify();
                        return true;
                    }
                    if window.is_fullscreen() { return false; }
                    if window.is_maximized() {
                        if !unzoom_requested { window.zoom_window(); unzoom_requested = true; }
                        return false;
                    }
                    window.resize(crate::default_window_size());
                    this.resetting_window = false;
                    cx.notify();
                    true
                });
                if done.unwrap_or(true) { break; }
            }
        }).detach();
    }
    fn effort_menu(&self) -> AnyElement {
        crate::ui_components::select(&self.effort_select, self.busy)
            .id("effort")
            .w(px(crate::theme::size::EFFORT_SELECT_WIDTH))
            .menu_width(px(240.))
            .accessibility_label(self.t("思考强度", "Thinking effort"))
            .into_any_element()
    }
    fn preset_menu(&self) -> AnyElement {
        crate::ui_components::select(&self.preset_select, self.busy)
            .id("provider-preset")
            .w_full()
            .menu_width(px(420.))
            .search_placeholder(self.t(
                "搜索供应商、地区或套餐",
                "Search providers, regions or plans",
            ))
            .accessibility_label(self.t("供应商预设", "Provider preset"))
            .into_any_element()
    }
    fn sync_preset(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let draft = self
            .drafts
            .providers
            .get(&self.selected())
            .cloned()
            .unwrap_or_default();
        let id = crate::presets::matching_url(
            &self.url.read(cx).value(),
            draft.protocol,
            &draft.protocol_settings,
        )
        .to_owned();
        self.preset_select
            .update(cx, |s, cx| s.set_selected_value(&id, window, cx));
    }
    fn apply_preset(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        if let Some(preset) = crate::presets::all().iter().find(|p| p.id == id) {
            let base = preset.title(self.zh);
            let selected = self.selected();
            let mut name = base.to_owned();
            let mut number = 2;
            while self
                .cfg
                .providers
                .iter()
                .any(|p| p.id != selected && p.name == name)
            {
                name = format!("{base} {number}");
                number += 1;
            }
            self.name.update(cx, |s, cx| s.set_value(name, window, cx));
            self.capture(cx);
            if let Some(draft) = self.drafts.providers.get_mut(&selected) {
                preset.apply_connection(draft);
                let url = draft.upstream.clone();
                self.url.update(cx, |s, cx| s.set_value(url, window, cx));
            }
            self.protocol_select.update(cx, |s, cx| {
                s.set_selected_value(&preset.protocol.as_str().to_owned(), window, cx)
            });
            self.last_metadata = Instant::now() - Duration::from_secs(10);
        }
        cx.notify();
    }
    pub(crate) fn t(&self, zh: &'static str, en: &'static str) -> &'static str {
        if self.zh { zh } else { en }
    }
    pub fn new(dir: PathBuf, cfg: Config, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let zh = cfg.language == "zh"
            || (cfg.language == "auto"
                && sys_locale::get_locale().is_some_and(|s| s.starts_with("zh")));
        gpui_kit::component::set_locale(if zh { "zh-CN" } else { "en" });
        let drafts = Drafts::load(&dir, &cfg);
        let backend = Backend::new(dir.clone());
        let statistics = cx.new(|cx| Statistics::new(dir.clone(), zh, window, cx));
        let name = input(
            "",
            if zh {
                "填写供应商名称"
            } else {
                "Enter provider name"
            },
            false,
            window,
            cx,
        );
        let key = input(
            "",
            if zh {
                "填写 API Key"
            } else {
                "Enter API Key"
            },
            true,
            window,
            cx,
        );
        let url = input(
            "",
            if zh {
                "填写供应商提供的 API 基础地址"
            } else {
                "Enter your provider's API base URL"
            },
            false,
            window,
            cx,
        );
        let port = input(&cfg.port.to_string(), "", false, window, cx);
        let search = input(
            "",
            if zh {
                "搜索模型 ID"
            } else {
                "Search model IDs"
            },
            false,
            window,
            cx,
        );
        let edit_name = input(
            "",
            if zh {
                "默认使用模型 ID"
            } else {
                "Defaults to model ID"
            },
            false,
            window,
            cx,
        );
        let edit_model = input(
            "",
            if zh {
                "填写模型 ID"
            } else {
                "Enter model ID"
            },
            false,
            window,
            cx,
        );
        let model_options = crate::model_options::OptionsEditor::new(zh, window, cx);
        let effort_select = cx.new(|cx| SelectState::new(choices(zh, false), None, window, cx));
        let preset_select =
            cx.new(|cx| SelectState::new(preset_choices(zh), None, window, cx).searchable(true));
        let protocol_options = crate::protocol_options::Editor::new(window, cx);
        let protocol_select = cx.new(|cx| SelectState::new(protocol_choices(), None, window, cx));
        let logs = cx.new(|cx| TextareaState::new(window, cx));
        let focus = cx.focus_handle();
        let table_focus = cx.focus_handle();
        let owner = cx.entity().downgrade();
        let model_table = cx.new(|cx| {
            TableState::new(
                crate::model_table::Models {
                    owner,
                    rows: vec![],
                    selection: Selection::default(),
                    zh,
                    busy: false,
                    column_width: px(230.),
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
                let result = this.update_in(cx, |this, window, cx| this.tick(window, cx));
                if result.is_err() {
                    break;
                }
            }
        });
        let mut this = Self {
            dir,
            page: Page::Providers,
            providers_expanded: true,
            statistics,
            cfg,
            drafts,
            backend,
            zh,
            busy: false,
            polling: false,
            core_state: CoreState::Unknown,
            metadata_polling: false,
            metadata_context: Value::Null,
            last_metadata: Instant::now() - Duration::from_secs(10),
            status: if zh { "就绪" } else { "Ready" }.into(),
            error: false,
            name,
            key,
            url,
            port,
            search,
            edit_name,
            edit_model,
            model_options,
            logs,
            log_text: String::new(),
            summary: String::new(),
            proxy_address: String::new(),
            proxy_counts: [0; 3],
            models: vec![],
            effort: "keep".into(),
            selection: Selection::default(),
            table_focus,
            model_table,
            table_width: px(0.),
            dialog_open: false,
            protocol_open: false,
            protocol_select,
            protocol_options,
            focus,
            picker: None,
            picked: BTreeSet::new(),
            only_picked: false,
            editing: None,
            new_model: false,
            effort_select,
            preset_select,
            delete_open: false,
            sync_open: false,
            unsync_open: false,
            startup_checked: false,
            startup_report: None,
            startup_sync_open: false,
            pending_port: None,
            resetting_window: false,
            wake: String::new(),
            last_poll: Instant::now() - Duration::from_secs(10),
            last_dirty: String::new(),
            _timer: timer,
        };
        cx.subscribe_in(
            &this.protocol_select,
            window,
            |this, _, event, window, cx| {
                if let SelectEvent::Confirm(Some(code)) = event {
                    let Ok(protocol) =
                        serde_json::from_value(serde_json::Value::String(code.clone()))
                    else {
                        return;
                    };
                    let id = this.selected();
                    if let Some(draft) = this.drafts.providers.get_mut(&id)
                        && draft.protocol != protocol
                    {
                        let azure = draft.protocol_settings.is_azure()
                            && matches!(
                                protocol,
                                crate::upstream_config::UpstreamProtocol::OpenaiChat
                                    | crate::upstream_config::UpstreamProtocol::OpenaiResponses
                            );
                        draft.protocol = protocol;
                        draft.protocol_settings = Default::default();
                        if azure {
                            draft.protocol_settings.authentication =
                                crate::upstream_config::Authentication::AzureApiKey;
                        }
                    }
                    this.sync_preset(window, cx);
                    this.last_metadata = Instant::now() - Duration::from_secs(10);
                    cx.notify();
                }
            },
        )
        .detach();
        cx.subscribe(&this.effort_select, |this, _, event, cx| {
            if let SelectEvent::Confirm(Some(code)) = event {
                this.effort = code.clone();
                cx.notify();
            }
        })
        .detach();
        cx.subscribe_in(&this.preset_select, window, |this, _, event, window, cx| {
            if let SelectEvent::Confirm(Some(id)) = event {
                this.apply_preset(id, window, cx);
            }
        })
        .detach();
        cx.subscribe_in(&this.url, window, |this, _, event, window, cx| {
            if matches!(event, InputEvent::Change) {
                this.sync_preset(window, cx);
            }
        })
        .detach();
        cx.subscribe_in(
            &this.model_options.default_effort,
            window,
            |this, _, event, _, cx| {
                if let SelectEvent::Confirm(Some(effort)) = event {
                    this.model_options.value.reasoning.default_effort = effort.clone();
                    cx.notify();
                }
            },
        )
        .detach();
        cx.subscribe_in(&this.edit_model, window, |this, _, event, window, cx| {
            if matches!(event, InputEvent::Change)
                && this.editing.is_some()
                && this.model_options.automatic
            {
                let model = this.model_id_for_options(cx);
                this.model_options
                    .load(&Value::Null, &model, this.zh, window, cx);
                cx.notify();
            }
        })
        .detach();
        this.load_form(window, cx);
        this.switch_page(Page::Statistics, window, cx);
        this.focus.focus(window, cx);
        cx.subscribe(&this.search, |_, _, event, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        })
        .detach();
        let weak = cx.entity().downgrade();
        window.on_window_should_close(cx, move |_, cx| {
            let _ = weak.update(cx, |this, cx| {
                this.capture(cx);
                let _ = this.drafts.persist(&this.dir);
            });
            true
        });
        let _ = config::write_json(
            &this.dir.join("gpui.json"),
            &json!({"pid":std::process::id(),"version":env!("CARGO_PKG_VERSION")}),
        );
        this
    }
    pub(crate) fn native_protocol(&self) -> bool {
        self.drafts
            .providers
            .get(&self.selected())
            .is_some_and(|d| d.protocol.is_native())
    }
    fn metadata_request(&self, cx: &App) -> Value {
        json!({"command":"model_metadata","key":self.key.read(cx).value().trim(),"provider_id":self.selected(),"model":self.model_id_for_options(cx),"upstream":self.url.read(cx).value().trim(),"protocol":self.protocol_payload()["protocol"],"protocol_settings":self.protocol_payload()["protocol_settings"]})
    }
    fn protocol_payload(&self) -> Value {
        let draft = self
            .drafts
            .providers
            .get(&self.selected())
            .cloned()
            .unwrap_or_default();
        json!({"protocol":draft.protocol,"protocol_settings":draft.protocol_settings})
    }
    fn confirm_protocol(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.protocol_options.read(cx) {
            Ok(settings) => {
                let id = self.selected();
                if let Some(d) = self.drafts.providers.get_mut(&id) {
                    d.protocol_settings = settings;
                }
                self.sync_preset(window, cx);
                self.protocol_open = false;
                self.error = false;
            }
            Err(error) => {
                self.status = error;
                self.error = true;
            }
        }
        cx.notify();
    }
    fn url_hint(&self) -> &'static str {
        use crate::upstream_config::UpstreamProtocol as P;
        if self
            .drafts
            .providers
            .get(&self.selected())
            .is_some_and(|d| d.protocol_settings.is_azure())
        {
            return self.t("填写 Azure 资源地址，如 https://your-resource.openai.azure.com；模型 ID 填部署名称，认证使用 API Key。", "Enter your Azure resource URL, e.g. https://your-resource.openai.azure.com; use deployment names as model IDs. API Key authentication.");
        }
        match self
            .drafts
            .providers
            .get(&self.selected())
            .map(|d| d.protocol)
            .unwrap_or_default()
        {
            P::OpenaiChat => "https://host/gateway/v1 → /chat/completions",
            P::OpenaiResponses => "https://host/gateway/v1 → /responses",
            P::AnthropicMessages => "https://api.anthropic.com/v1 → /messages",
            P::GeminiGenerateContent => {
                "https://generativelanguage.googleapis.com/v1beta → /models/{model}:generateContent"
            }
        }
    }
    fn selected(&self) -> String {
        self.drafts.selected.clone()
    }
    fn capture(&mut self, cx: &App) {
        if self.drafts.selected.is_empty() {
            return;
        }
        self.drafts.providers.insert(
            self.selected(),
            Draft {
                name: self.name.read(cx).value().to_string(),
                key: self.key.read(cx).value().to_string(),
                upstream: self.url.read(cx).value().to_string(),
                protocol: self
                    .drafts
                    .providers
                    .get(&self.selected())
                    .map(|d| d.protocol)
                    .unwrap_or_default(),
                protocol_settings: self
                    .drafts
                    .providers
                    .get(&self.selected())
                    .map(|d| d.protocol_settings.clone())
                    .unwrap_or_default(),
                effort: self.effort.clone(),
                models: self.models.clone(),
            },
        );
    }
    fn load_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.drafts.providers.contains_key(&self.drafts.selected) {
            self.drafts.selected = self
                .cfg
                .providers
                .first()
                .map(|p| p.id.clone())
                .unwrap_or_default();
        }
        let draft = self
            .drafts
            .providers
            .get(&self.selected())
            .cloned()
            .unwrap_or_default();
        for (state, value) in [
            (&self.name, draft.name),
            (&self.key, draft.key),
            (&self.url, draft.upstream),
        ] {
            state.update(cx, |s, cx| s.set_value(value, window, cx));
        }
        self.protocol_select.update(cx, |s, cx| {
            s.set_selected_value(&draft.protocol.as_str().to_owned(), window, cx)
        });
        self.models = draft.models;
        self.effort = if draft.effort.is_empty() {
            "keep".into()
        } else {
            draft.effort
        };
        self.effort_select
            .update(cx, |s, cx| s.set_selected_value(&self.effort, window, cx));
        self.sync_preset(window, cx);
        self.selection = Selection::default();
        cx.notify();
    }
    fn send(&mut self, tag: &str, steps: Vec<Value>, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        match self.backend.jobs.try_send(Job {
            tag: tag.into(),
            steps,
        }) {
            Ok(()) => {
                self.busy = true;
                self.status = self.t("正在处理…", "Working…").into();
                self.error = false;
            }
            Err(_) => {
                self.status = self
                    .t("后台忙，请稍后再试", "Core is busy; try again")
                    .into();
                self.error = true;
            }
        }
        cx.notify();
    }
    fn save_steps(&mut self, cx: &App) -> Vec<Value> {
        self.capture(cx);
        self.drafts
            .providers
            .get(&self.selected())
            .map(|d| vec![d.payload(&self.selected())])
            .unwrap_or_default()
    }
    fn save(&mut self, _: &Save, _: &mut Window, cx: &mut Context<Self>) {
        if self.picker.is_some()
            || self.editing.is_some()
            || self.delete_open
            || self.protocol_open
            || self.sync_open
            || self.unsync_open
            || self.startup_sync_open
            || self.pending_port.is_some()
        {
            return;
        }
        let steps = self.save_steps(cx);
        if !steps.is_empty() {
            self.send("save", steps, cx);
        }
    }
    fn test(&mut self, cx: &mut Context<Self>) {
        let mut steps = self.save_steps(cx);
        let mut test = json!({"command":"test","provider_id":self.selected()});
        if let Some(index) = self.selection.rows.first() {
            test["model_index"] = json!(index);
        }
        steps.push(test);
        self.send("test", steps, cx);
    }
    fn dirty_steps(&mut self, cx: &App) -> Vec<Value> {
        self.capture(cx);
        let fresh = Drafts::from_config(&self.dir, &self.cfg);
        if self.drafts.providers.get(&self.selected()) == fresh.providers.get(&self.selected()) {
            vec![]
        } else {
            self.save_steps(cx)
        }
    }
    fn sync(&mut self, restart: bool, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let mut steps = if self.selected().is_empty() {
            vec![]
        } else {
            self.dirty_steps(cx)
        };
        steps.push(json!({"command":"install"}));
        self.sync_open = false;
        self.send(
            if restart {
                "install_restart"
            } else {
                "install"
            },
            steps,
            cx,
        );
        if restart && self.busy {
            self.status = self
                .t(
                    "正在同步并重启 WorkBuddy…",
                    "Syncing and restarting WorkBuddy…",
                )
                .into();
        }
    }
    fn unsync(&mut self, restart: bool, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        self.unsync_open = false;
        self.startup_report = None;
        // Removing saved WorkBuddy entries must not save provider drafts.
        self.send(
            if restart {
                "restore_restart"
            } else {
                "restore"
            },
            vec![json!({"command":"restore"})],
            cx,
        );
    }

    fn confirm_port(&mut self, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let Some(port) = self.pending_port.take() else {
            return;
        };
        self.startup_report = None;
        self.send(
            "port_restart",
            vec![json!({"command":"set_port","port":port})],
            cx,
        );
    }

    fn confirm_startup_sync(&mut self, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        self.startup_sync_open = false;
        self.startup_report = None;
        // Startup repair uses only saved configuration. Never save or discard
        // restored supplier drafts merely because an endpoint needs resyncing.
        self.send(
            "startup_install_restart",
            vec![json!({"command":"install"})],
            cx,
        );
    }

    fn switch_provider(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        self.capture(cx);
        self.drafts.selected = id;
        self.load_form(window, cx);
    }
    fn update_config(
        &mut self,
        cfg: Config,
        tag: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let saved = self.selected();
        let fresh = Drafts::from_config(&self.dir, &cfg);
        self.drafts
            .providers
            .retain(|id, _| fresh.providers.contains_key(id));
        for (id, draft) in fresh.providers {
            if !self.drafts.providers.contains_key(&id)
                || ((tag == "save" || tag == "test" || tag == "install" || tag == "duplicate")
                    && id == saved)
            {
                self.drafts.providers.insert(id, draft);
            }
        }
        if tag == "add" || tag == "duplicate" {
            self.drafts.selected = cfg
                .providers
                .last()
                .map(|p| p.id.clone())
                .unwrap_or_default();
        }
        self.cfg = cfg;
        self.port.update(cx, |s, cx| {
            s.set_value(self.cfg.port.to_string(), window, cx)
        });
        if matches!(
            tag,
            "save" | "test" | "install" | "add" | "duplicate" | "delete"
        ) {
            let selected = self.selection.rows.clone();
            let anchor = self.selection.anchor;
            self.load_form(window, cx);
            if matches!(tag, "save" | "test" | "install") {
                self.selection.rows = selected
                    .into_iter()
                    .filter(|i| *i < self.models.len())
                    .collect();
                self.selection.anchor = anchor;
            }
        }
    }
    fn snapshot(&mut self, v: Value, window: &mut Window, cx: &mut Context<Self>) {
        self.proxy_address = v["url"].as_str().unwrap_or("").to_owned();
        self.proxy_counts =
            ["providers", "models", "requests"].map(|key| v[key].as_u64().unwrap_or(0));
        if let Some(enabled) = v["autostart"].as_bool() {
            self.cfg.autostart = enabled;
        }
        self.summary = format!(
            "{}\n{}  {}   ·   {}  {}   ·   {}  {}",
            v["url"].as_str().unwrap_or(""),
            self.t("供应商", "Providers"),
            v["providers"],
            self.t("模型", "Models"),
            v["models"],
            self.t("请求", "Requests"),
            v["requests"]
        );
        let text = v["logs"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|row| {
                let ms = row["time"].as_u64()?;
                let message = row["message"].as_str()?;
                let time = chrono::DateTime::from_timestamp_millis(ms as i64)?
                    .with_timezone(&chrono::Local)
                    .format("%H:%M:%S")
                    .to_string();
                Some(format!(
                    "{time}：{}",
                    message
                        .lines()
                        .map(|s| crate::i18n::message(s, self.zh))
                        .collect::<Vec<_>>()
                        .join(" · ")
                ))
            })
            .collect::<Vec<_>>()
            .join("\n");
        let text = if text.is_empty() {
            self.t("暂无日志", "No log entries").to_owned()
        } else {
            text
        };
        if text != self.log_text {
            self.log_text = text;
            self.logs.update(cx, |s, cx| {
                s.set_value(self.log_text.clone(), window, cx);
                // Textarea clamps this to the actual bottom after laying out the new text.
                s.set_scroll_offset(point(px(0.), px(-f32::MAX)), cx);
            });
            cx.notify();
        }
    }
    fn tick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.dir.join("gpui-stop.json").exists() || !self.dir.join("instance.json").exists() {
            self.capture(cx);
            let _ = self.drafts.persist(&self.dir);
            cx.quit();
            return;
        }
        if self.page == Page::Statistics {
            self.statistics
                .update(cx, |page, cx| page.poll_maintenance(cx));
        }
        let wake = std::fs::read_to_string(self.dir.join("gpui-wake.json")).unwrap_or_default();
        if wake != self.wake {
            self.wake = wake;
            self.switch_page(Page::Statistics, window, cx);
            window.activate_window();
            cx.activate(true);
        }
        while let Ok(reply) = self.backend.replies.try_recv() {
            if reply.tag == "workbuddy_check" {
                match reply.result.and_then(|text| {
                    serde_json::from_str::<crate::workbuddy_status::EndpointCheck>(&text)
                        .map_err(|_| "Invalid WorkBuddy endpoint check response".to_owned())
                }) {
                    Ok(report) if report.mismatch_count > 0 => {
                        self.startup_report = Some(report);
                    }
                    Ok(_) => {}
                    Err(error) => {
                        self.error = true;
                        self.status = format!(
                            "{}{}",
                            self.t(
                                "WorkBuddy 地址检查失败：",
                                "WorkBuddy endpoint check failed: "
                            ),
                            crate::i18n::message(&error, self.zh)
                        );
                    }
                }
                cx.notify();
                continue;
            }
            if reply.tag == "metadata" {
                self.metadata_polling = false;
                if self.editing.is_some()
                    && self.model_options.automatic
                    && let Ok(message) = &reply.result
                    && let Ok(value) = serde_json::from_str::<Value>(message)
                    && self.metadata_context == self.metadata_request(cx)
                    && value["model"].as_str() == Some(self.model_id_for_options(cx).as_str())
                    && value["upstream"].as_str() == Some(self.url.read(cx).value().trim())
                    && value["protocol"] == self.protocol_payload()["protocol"]
                    && value["protocol_settings"] == self.protocol_payload()["protocol_settings"]
                    && self
                        .model_options
                        .apply_automatic(&value["metadata"], self.zh, window, cx)
                {
                    cx.notify();
                }
                continue;
            }
            if reply.tag == "poll" {
                self.polling = false;
                // D13: the skeleton reads the relay state from this very poll
                // reply, so a core that stops answering can never keep showing
                // as healthy. A poll only carries a snapshot when the core
                // answered with valid JSON.
                self.core_state = if reply.snapshot.is_some() {
                    CoreState::Responding
                } else {
                    CoreState::Unreachable
                };
            } else {
                self.busy = false;
                match &reply.result {
                    Ok(message) => {
                        self.error = false;
                        self.status = if reply.tag == "fetch" {
                            self.t("请选择需要添加的模型", "Select models to add")
                                .into()
                        } else if reply.tag == "check_update" {
                            serde_json::from_str::<Value>(message)
                                .ok()
                                .filter(|v| v["available"] == true)
                                .map(|v| {
                                    format!(
                                        "{} {}",
                                        self.t("发现新版本", "Update available"),
                                        v["tag"].as_str().unwrap_or("")
                                    )
                                })
                                .unwrap_or_else(|| {
                                    self.t("当前已是最新版本", "Already up to date").into()
                                })
                        } else if reply.tag == "port" {
                            serde_json::from_str::<Value>(message)
                                .ok()
                                .map(|v| {
                                    format!(
                                        "{} {}",
                                        if v["workbuddy_restarted"] == true {
                                            self.t(
                                                "端口已更新并已重启 WorkBuddy：",
                                                "Port updated and WorkBuddy restarted:",
                                            )
                                        } else {
                                            self.t("本地端口已更新：", "Local port updated:")
                                        },
                                        v["port"]
                                    )
                                })
                                .unwrap_or_else(|| message.clone())
                        } else {
                            crate::i18n::message(message, self.zh)
                                .lines()
                                .next()
                                .unwrap_or("")
                                .to_owned()
                        };
                    }
                    Err(message) => {
                        self.error = true;
                        self.status = crate::i18n::message(message, self.zh)
                            .lines()
                            .next()
                            .unwrap_or("")
                            .to_owned();
                    }
                }
                if reply.result.is_ok() || reply.synced {
                    if reply.tag == "fetch" {
                        self.picker = reply
                            .result
                            .as_ref()
                            .ok()
                            .and_then(|s| serde_json::from_str(s).ok());
                        self.picked.clear();
                        self.only_picked = false;
                        self.search.update(cx, |s, cx| s.set_value("", window, cx));
                        self.search.read(cx).focus_handle(cx).focus(window, cx);
                    }
                    if let Some(cfg) = reply.config {
                        self.update_config(cfg, &reply.tag, window, cx);
                    }
                }
                if reply.synced || (reply.tag == "port" && reply.result.is_ok()) {
                    self.startup_report = None;
                    self.startup_sync_open = false;
                }
                let _ = self.drafts.persist(&self.dir);
                cx.notify();
            }
            if let Some(snapshot) = reply.snapshot {
                self.snapshot(snapshot, window, cx);
            }
        }
        // One bounded read per settings launch, off the UI thread. Defer the
        // prompt while another editor or action is active.
        if !self.startup_checked
            && !self.busy
            && !self.dialog_open
            && self
                .backend
                .jobs
                .try_send(Job {
                    tag: "workbuddy_check".into(),
                    steps: vec![json!({"command":"workbuddy_check"})],
                })
                .is_ok()
        {
            self.startup_checked = true;
        }
        if self.startup_report.is_some()
            && !self.busy
            && !self.dialog_open
            && !self.sync_open
            && !self.unsync_open
            && self.pending_port.is_none()
            && !self.protocol_open
            && self.editing.is_none()
            && self.picker.is_none()
            && !self.delete_open
        {
            self.startup_sync_open = true;
        }
        if !self.busy
            && !self.polling
            && self.last_poll.elapsed() > Duration::from_secs(2)
            && self
                .backend
                .jobs
                .try_send(Job {
                    tag: "poll".into(),
                    steps: vec![json!({"command":"snapshot"})],
                })
                .is_ok()
        {
            self.polling = true;
            self.last_poll = Instant::now();
        }
        if self.editing.is_some()
            && self.model_options.automatic
            && !self.metadata_polling
            && self.last_metadata.elapsed() > Duration::from_secs(1)
        {
            let model = self.model_id_for_options(cx);
            let request = self.metadata_request(cx);
            if !model.is_empty()
                && self
                    .backend
                    .jobs
                    .try_send(Job {
                        tag: "metadata".into(),
                        steps: vec![request.clone()],
                    })
                    .is_ok()
            {
                self.metadata_context = request;
                self.metadata_polling = true;
                self.last_metadata = Instant::now();
            }
        }
        // Persist changed drafts only. Idle ticks never redraw a static window.
        self.capture(cx);
        let digest = serde_json::to_string(&self.drafts.providers).unwrap_or_default();
        if digest != self.last_dirty {
            self.last_dirty = digest;
            let _ = self.drafts.persist(&self.dir);
        }
    }
    fn remove_models(&mut self, _: &RemoveModels, _: &mut Window, cx: &mut Context<Self>) {
        if self.busy || self.editing.is_some() || self.picker.is_some() {
            return;
        }
        self.selection.remove(&mut self.models);
        cx.notify();
    }
    fn select_models(&mut self, _: &SelectModels, _: &mut Window, cx: &mut Context<Self>) {
        self.selection.rows = (0..self.models.len()).collect();
        cx.notify();
    }
    fn edit_selected(&mut self, _: &EditModel, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(i) = self.selection.rows.first().copied() {
            self.edit(i, false, window, cx);
        }
    }
    pub(crate) fn edit_model_row(&mut self, i: usize, window: &mut Window, cx: &mut Context<Self>) {
        if i < self.models.len() && self.editing.is_none() && self.picker.is_none() {
            self.edit(i, false, window, cx);
        }
    }
    fn edit(&mut self, i: usize, new: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        self.error = false;
        self.editing = Some(i);
        self.new_model = new;
        let row = self.models.get(i).cloned().unwrap_or(json!({}));
        self.edit_name.update(cx, |s, cx| {
            s.set_value(row["name"].as_str().unwrap_or("").to_owned(), window, cx)
        });
        self.edit_model.update(cx, |s, cx| {
            s.set_value(row["model"].as_str().unwrap_or("").to_owned(), window, cx)
        });
        self.model_options.load(
            &row,
            row["model"].as_str().unwrap_or(""),
            self.zh,
            window,
            cx,
        );
        self.edit_model.read(cx).focus_handle(cx).focus(window, cx);
        cx.notify();
    }
    pub(crate) fn is_chinese(&self) -> bool {
        self.zh
    }
    pub(crate) fn model_id_for_options(&self, cx: &App) -> String {
        self.edit_model.read(cx).value().to_string()
    }
    fn confirm_edit(&mut self, cx: &mut Context<Self>) {
        if self.editing.is_none() {
            return;
        }
        let id = self.edit_model.read(cx).value().trim().to_owned();
        if id.is_empty() {
            self.status = self.t("请填写模型 ID", "Model ID is required").into();
            self.error = true;
            cx.notify();
            return;
        }
        let capabilities = match self.model_options.read(cx) {
            Ok(value) => value,
            Err(error) => {
                self.status = crate::i18n::message(&error, self.zh);
                self.error = true;
                cx.notify();
                return;
            }
        };
        let mut row = self
            .editing
            .and_then(|i| self.models.get(i).cloned())
            .unwrap_or(json!({}));
        row["model"] = json!(id);
        row["capabilities"] = json!(capabilities);
        row["name"] = json!(self.edit_name.read(cx).value().trim().to_owned());
        if self.new_model {
            if self
                .cfg
                .providers
                .iter()
                .filter(|p| p.id != self.selected())
                .map(|p| p.models.len())
                .sum::<usize>()
                + self.models.len()
                >= 256
            {
                self.status = self.t("最多支持 256 个模型", "At most 256 models").into();
                self.error = true;
                return;
            }
            self.models.push(row);
        } else if let Some(i) = self.editing {
            self.models[i] = row;
        }
        self.editing = None;
        self.error = false;
        self.status = self
            .t("模型已更新，保存后生效", "Models updated; save to apply")
            .into();
        cx.notify();
    }
    fn dismiss(&mut self, _: &Dismiss, window: &mut Window, cx: &mut Context<Self>) {
        self.picker = None;
        self.editing = None;
        self.delete_open = false;
        self.sync_open = false;
        self.unsync_open = false;
        if self.startup_sync_open {
            self.startup_report = None;
        }
        self.startup_sync_open = false;
        if self.pending_port.take().is_some() {
            let current = self
                .proxy_address
                .parse::<reqwest::Url>()
                .ok()
                .and_then(|u| u.port_or_known_default())
                .unwrap_or(self.cfg.port);
            self.port.update(cx, |state, cx| {
                state.set_value(current.to_string(), window, cx)
            });
        }
        self.protocol_open = false;
        self.focus.focus(window, cx);
        cx.notify();
    }
    fn quit(&mut self, _: &Quit, _: &mut Window, cx: &mut Context<Self>) {
        self.capture(cx);
        let _ = self.drafts.persist(&self.dir);
        let _ = config::write_json(&self.dir.join("gpui-exit-host"), &true);
        cx.quit();
    }
    fn filtered(&self, cx: &App) -> Vec<usize> {
        let query = self.search.read(cx).value().to_lowercase();
        self.picker
            .as_ref()
            .map(|ids| {
                ids.iter()
                    .enumerate()
                    .filter(|(i, id)| {
                        id.to_lowercase().contains(&query)
                            && (!self.only_picked
                                || self.picked.contains(i)
                                || self.models.iter().any(|m| m["model"].as_str() == Some(id)))
                    })
                    .map(|(i, _)| i)
                    .collect()
            })
            .unwrap_or_default()
    }
    fn apply_picker(&mut self, cx: &mut Context<Self>) {
        let Some(ids) = &self.picker else {
            return;
        };
        let missing = self
            .picked
            .iter()
            .filter_map(|i| ids.get(*i))
            .filter(|id| !self.models.iter().any(|m| m["model"].as_str() == Some(id)))
            .collect::<Vec<_>>();
        let other = self
            .cfg
            .providers
            .iter()
            .filter(|p| p.id != self.selected())
            .map(|p| p.models.len())
            .sum::<usize>();
        if self.models.len() + other + missing.len() > 256 {
            self.error = true;
            self.status = self.t("最多支持 256 个模型", "At most 256 models").into();
            return;
        }
        for id in missing {
            self.models.push(json!({"model":id,"name":id}));
        }
        self.picker = None;
        self.status = self
            .t("模型已添加，保存后生效", "Models added; save to apply")
            .into();
        cx.notify();
    }

    fn refresh_table(&self, window: &Window, cx: &mut Context<Self>) {
        let width = if self.table_width > px(0.) {
            self.table_width
        } else {
            // First frame, before the model table's own prepaint reports its
            // bounds: page padding + right rail + rail gap + card padding.
            let chrome = crate::theme::space::PAGE_X * 2.
                + crate::theme::size::RIGHT_RAIL_WIDTH
                + crate::theme::space::CARD_GAP
                + crate::theme::space::CARD_SM * 2.
                + 2.;
            window.viewport_size().width - px(chrome + crate::theme::size::SIDEBAR_WIDTH)
        };
        let column_width = ((width - crate::model_table::EDIT_COLUMN_WIDTH) / 2.).max(px(120.));
        let data = self.model_table.read(cx).delegate();
        if data.rows == self.models
            && data.selection == self.selection
            && data.zh == self.zh
            && data.busy == self.busy
            && data.column_width == column_width
        {
            return;
        }
        self.model_table.update(cx, |table, cx| {
            let data = table.delegate_mut();
            data.rows = self.models.clone();
            data.selection = self.selection.clone();
            data.busy = self.busy;
            let header_changed = data.zh != self.zh || data.column_width != column_width;
            data.zh = self.zh;
            data.column_width = column_width;
            if header_changed {
                table.refresh(cx);
            }
            cx.notify();
        });
    }
    fn manage_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let active = self.picker.is_some()
            || self.editing.is_some()
            || self.delete_open
            || self.sync_open
            || self.unsync_open
            || self.startup_sync_open
            || self.pending_port.is_some()
            || self.protocol_open;
        if active && !self.dialog_open {
            self.dialog_open = true;
            let owner = cx.entity().downgrade();
            let observer = cx.entity().clone();
            let body = cx.new(|cx| {
                cx.observe(&observer, |_, _, cx| cx.notify()).detach();
                ModalBody {
                    owner: owner.clone(),
                }
            });
            let width = if self.protocol_open {
                crate::theme::size::DIALOG_PROTOCOL_WIDTH
            } else if self.picker.is_some() || self.editing.is_some() {
                720.
            } else {
                480.
            };
            let focus = if self.protocol_open {
                // Focus a control rendered inside the protocol dialog; focusing
                // the window root would leave the modal focus trap inactive and
                // let Tab/Enter reach background controls (T09 X03).
                self.protocol_options.initial_focus(cx)
            } else if self.picker.is_some() {
                self.search.read(cx).focus_handle(cx)
            } else if self.editing.is_some() {
                self.edit_model.read(cx).focus_handle(cx)
            } else {
                self.focus.clone()
            };
            let first = std::cell::Cell::new(true);
            window.open_dialog(cx, move |dialog, window, cx| {
                if first.replace(false) {
                    focus.focus(window, cx);
                }
                let confirm_owner = owner.clone();
                let owner = owner.clone();
                dialog
                    .w(px(width))
                    .close_button(true)
                    .overlay_closable(false)
                    .child(body.clone())
                    .on_ok(move |_, window, cx| {
                        let _ = confirm_owner.update(cx, |this, cx| {
                            if this.protocol_open {
                                this.confirm_protocol(window, cx);
                            } else if this.editing.is_some() {
                                this.confirm_edit(cx);
                            } else if this.picker.is_some() {
                                this.apply_picker(cx);
                            }
                        });
                        // Owner closes only after a valid submission; never use the
                        // Kit default close which could discard editor contents.
                        false
                    })
                    .on_close(move |_, window, cx| {
                        let _ = owner.update(cx, |this, cx| {
                            this.dialog_open = false;
                            this.dismiss(&Dismiss, window, cx);
                        });
                    })
            });
        } else if !active && self.dialog_open {
            self.dialog_open = false;
            window.close_dialog(cx);
            self.table_focus.focus(window, cx);
        }
    }
    pub(crate) fn click_model(
        &mut self,
        i: usize,
        event: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.busy || i >= self.models.len() {
            return;
        }
        let modifiers = event.modifiers();
        self.selection
            .click(i, modifiers.shift, modifiers.control || modifiers.platform);
        self.table_focus.focus(window, cx);
        if event.click_count() == 2 && !modifiers.shift && !modifiers.control && !modifiers.platform
        {
            self.edit(i, false, window, cx);
        }
        cx.notify();
    }
    pub(crate) fn switch_page(&mut self, page: Page, window: &mut Window, cx: &mut Context<Self>) {
        if self.page == page {
            return;
        }
        self.page = page;
        let providers = self
            .cfg
            .providers
            .iter()
            .filter(|p| !p.is_draft())
            .map(|p| (p.id.clone(), p.name.clone()))
            .collect();
        self.statistics.update(cx, |statistics, cx| {
            statistics.set_configured_providers(providers);
            statistics.set_visible(page == Page::Statistics, cx)
        });
        if page == Page::Providers {
            self.table_focus.focus(window, cx);
        }
        cx.notify();
    }

    /// Resolved relay state for the window skeleton.
    ///
    /// A responding core only counts as running when the snapshot also carried a
    /// relay address, so an empty poll result stays visibly unknown instead of
    /// being rounded up to healthy.
    fn core_state(&self) -> CoreState {
        match self.core_state {
            CoreState::Responding if self.proxy_address.is_empty() => CoreState::Unknown,
            state => state,
        }
    }

    /// Trusted relay indicator for the provider page's service sub-card.
    ///
    /// T03 replaced the hardcoded "running" tag with this trusted source; T04
    /// renders it as the design's dot plus label instead of a tag pill. The
    /// state resolution and wording are unchanged, and the dot colour comes
    /// from the same theme tokens.
    fn core_indicator(&self) -> (u32, &'static str, u32) {
        match self.core_state() {
            CoreState::Responding => (
                crate::theme::color::PRIMARY_DARK,
                self.t("运行中", "Running"),
                crate::theme::color::PRIMARY_DARK,
            ),
            CoreState::Unknown => (
                crate::theme::color::TEXT_3,
                self.t("状态未知", "Unknown"),
                crate::theme::color::TEXT_3,
            ),
            CoreState::Unreachable => (
                crate::theme::color::DANGER,
                self.t("未响应", "Not responding"),
                crate::theme::color::DANGER,
            ),
        }
    }
    /// Fixed 30px status bar across the bottom of the window (DESIGN_SPEC §2).
    ///
    /// It is a sibling of the scrolling page area, so it stays put while any
    /// page scrolls, and both its dot colour and its text come from the real
    /// core state (D13): a core that has not answered yet, or that stopped
    /// answering, reads as unknown or unreachable and never as running.
    fn status_bar(&self) -> AnyElement {
        let (dot, text, text_color) = match self.core_state() {
            CoreState::Responding => (
                crate::theme::color::PRIMARY_DARK,
                self.t("本地代理运行中", "Local relay running"),
                crate::theme::color::TEXT_3,
            ),
            CoreState::Unknown => (
                crate::theme::color::TEXT_3,
                self.t(
                    "代理状态未知，等待核心响应",
                    "Relay status unknown; waiting for the core",
                ),
                crate::theme::color::TEXT_3,
            ),
            CoreState::Unreachable => (
                crate::theme::color::DANGER,
                self.t(
                    "无法连接核心，代理状态未知",
                    "Core not responding; relay status unknown",
                ),
                crate::theme::color::DANGER,
            ),
        };
        div()
            .flex()
            .items_center()
            .justify_between()
            .flex_shrink_0()
            .w_full()
            .h(px(crate::theme::size::STATUS_BAR_HEIGHT))
            .px(px(crate::theme::space::PAGE_X))
            .bg(rgb(crate::theme::color::BG_BAR))
            .border_t_1()
            .border_color(rgb(crate::theme::color::BORDER))
            .child(
                div()
                    .id("status-relay")
                    .flex()
                    .items_center()
                    .gap_2()
                    .aria_label(text)
                    .child(
                        div()
                            .flex_shrink_0()
                            .size(px(crate::theme::size::STATUS_DOT))
                            .rounded_full()
                            .bg(rgb(dot)),
                    )
                    .child(
                        div()
                            .text_size(px(crate::theme::font::LABEL))
                            .text_color(rgb(text_color))
                            .child(text),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        Icon::default()
                            .path("icons/info.svg")
                            .with_size(px(13.))
                            .text_color(rgb(crate::theme::color::TEXT_3)),
                    )
                    .child(
                        div()
                            .text_size(px(crate::theme::font::LABEL))
                            .text_color(rgb(crate::theme::color::TEXT_3))
                            .child(self.t(
                                "关闭窗口后，代理仍在托盘运行",
                                "Close this window to keep the relay running in the tray",
                            )),
                    ),
            )
            .into_any_element()
    }

    /// Fixed 224px window sidebar (DESIGN_SPEC §2, T03).
    ///
    /// Three fixed blocks — brand, navigation and the provider panel — with the
    /// provider list as the only scrolling region, so add/copy/delete stay
    /// reachable however long the list grows. Every callback, disabled rule,
    /// tooltip and accessibility name is the one the pre-T03 sidebar used; the
    /// rows only changed their presentation.
    fn sidebar(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let selected = self.selected();
        let rows = self
            .cfg
            .providers
            .iter()
            .map(|p| {
                let id = p.id.clone();
                let active = self.page == Page::Providers && id == selected;
                let draft = self.drafts.providers.get(&id);
                let name = draft
                    .map(|d| d.name.clone())
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| p.name.clone());
                let count = draft.map_or(p.models.len(), |d| d.models.len());
                div()
                    .id(SharedString::from(format!("provider-{id}")))
                    .flex()
                    .items_center()
                    .gap_2()
                    .flex_shrink_0()
                    .h(px(crate::theme::size::PROVIDER_ROW_HEIGHT))
                    .px_2()
                    .rounded(px(crate::theme::radius::CONTROL))
                    .when(active, |this| {
                        this.bg(rgb(crate::theme::color::PRIMARY_TINT))
                    })
                    .when(!active, |this| this.bg(rgb(crate::theme::color::BG_PAGE)))
                    // The pre-T03 `SidebarMenuItem` was disabled as a whole while
                    // busy: no click handler ran and the row read as muted. Keep
                    // both halves so the row cannot act or look active.
                    .when(self.busy, |this| this.opacity(0.55))
                    .when(!self.busy, |this| {
                        this.hover(|this| this.bg(rgb(crate::theme::color::PRIMARY_TINT)))
                    })
                    .role(Role::TreeItem)
                    .aria_label(name.clone())
                    .aria_selected(active)
                    .child(
                        div()
                            .flex_shrink_0()
                            .size(px(crate::theme::size::PROVIDER_DOT))
                            .rounded_full()
                            .bg(rgb(crate::theme::color::PRIMARY)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_ellipsis()
                            .text_size(px(crate::theme::font::LABEL))
                            .font_weight(if active {
                                FontWeight::SEMIBOLD
                            } else {
                                FontWeight::NORMAL
                            })
                            .child(name),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_size(px(crate::theme::font::SMALL))
                            .text_color(rgb(crate::theme::color::TEXT_3))
                            .child(count.to_string()),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        // Keep the baseline "whole row disabled while busy"
                        // behaviour: returning here leaves the page, the
                        // selected provider and the drafts untouched. Without
                        // this guard `switch_page` would still run before
                        // `switch_provider` silently returned.
                        if this.busy {
                            return;
                        }
                        this.switch_page(Page::Providers, window, cx);
                        this.switch_provider(id.clone(), window, cx)
                    }))
                    .into_any_element()
            })
            .collect::<Vec<_>>();
        let statistics_active = self.page == Page::Statistics;
        let providers_active = self.page == Page::Providers;

        // Brand block. The mark moved here from the old window header; the
        // version is still read from the build.
        let subtitle = self.t(
            "轻量转发 · 多模型管理",
            "Lightweight relay · Multiple models",
        );
        let subtitle_text = format!("{subtitle} · v{}", env!("CARGO_PKG_VERSION"));
        let version_range = subtitle.len()..subtitle_text.len();
        let brand = div()
            .flex()
            .items_center()
            .gap_2()
            .flex_shrink_0()
            .child(
                div()
                    .flex_shrink_0()
                    .size(px(BRAND_MARK_SIZE))
                    .rounded(px(BRAND_MARK_RADIUS))
                    .bg(rgb(crate::theme::color::PRIMARY))
                    .overflow_hidden()
                    .child(img("brand.png").size(px(BRAND_MARK_SIZE))),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .text_size(px(crate::theme::font::CARD_TITLE))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("WorkBuddy Proxy"),
                    )
                    .child(
                        // DESIGN_SPEC §2 wraps this subtitle: the design itself
                        // breaks after "轻量转发 · 多模型管理 ·" and shows the
                        // whole build version on a second line inside the 224px
                        // sidebar. Ellipsising it hid the version, so the text
                        // wraps instead of being truncated.
                        div()
                            .text_size(px(crate::theme::font::SMALL))
                            .text_color(rgb(crate::theme::color::TEXT_3))
                            .child(StyledText::new(subtitle_text).with_highlights([(
                                version_range,
                                HighlightStyle {
                                    color: Some(rgb(crate::theme::color::TEXT_3).into()),
                                    ..Default::default()
                                },
                            )])),
                    ),
            );
        // Provider panel: fixed title row, scrollable list, fixed tool row.
        let panel = div()
            .flex()
            .flex_col()
            .min_h_0()
            .w_full()
            .overflow_hidden()
            .rounded(px(crate::theme::radius::CARD))
            .when(self.providers_expanded, |panel| {
                panel
                    .flex_1()
                    .bg(rgb(crate::theme::color::WHITE))
                    .border_1()
                    .border_color(rgb(crate::theme::color::BORDER))
            })
            .child(
                Button::new("nav-providers")
                    .ghost()
                    .w_full()
                    .h(px(crate::theme::size::NAV_ROW_HEIGHT))
                    .flex_shrink_0()
                    .when(providers_active, |button| {
                        button.bg(rgb(crate::theme::color::PRIMARY_TINT))
                    })
                    .accessibility_label(self.t("供应商管理", "Providers"))
                    .tooltip(if self.providers_expanded {
                        self.t("折叠供应商列表", "Collapse providers")
                    } else {
                        self.t("展开供应商列表", "Expand providers")
                    })
                    .child(
                        div()
                            .flex()
                            .flex_1()
                            .items_center()
                            .justify_between()
                            .child(navigation_title(
                                self.t("供应商管理", "Providers"),
                                providers_active,
                            ))
                            .child(
                                Icon::new(if self.providers_expanded {
                                    IconName::ChevronDown
                                } else {
                                    IconName::ChevronRight
                                })
                                .size_3(),
                            ),
                    )
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.providers_expanded = !this.providers_expanded;
                        this.switch_page(Page::Providers, window, cx);
                        cx.notify();
                    })),
            )
            .when(self.providers_expanded, |this| {
                this.child(
                    div()
                        .id("provider-list")
                        .flex()
                        .flex_col()
                        .gap_1()
                        .flex_1()
                        .min_h_0()
                        .p_2()
                        .overflow_y_scroll()
                        .children(rows),
                )
                // Measured with the real button helper: zh 52/52/52, en
                // 51/57/69 (the widest label is the English "Delete"), each
                // including the 16px icon, its gap and 8px side padding. The
                // row needs ~193px, so SIDEBAR_WIDTH grew from 224 to 256: the
                // three actions stay on one line, inside the rail, and every
                // id, callback, disabled state, tooltip and a11y label is
                // unchanged.
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .gap_2()
                        .w_full()
                        .min_h(px(crate::theme::size::FOOTER_HEIGHT))
                        .px_2()
                        .py_2()
                        .flex_shrink_0()
                        .border_t_1()
                        .border_color(rgb(crate::theme::color::BORDER))
                        .child(
                            crate::ui_components::button(
                                "provider-add",
                                crate::ui_components::ButtonKind::Ghost,
                                self.t("添加", "Add"),
                                self.busy,
                                false,
                            )
                            .icon(IconName::Plus)
                            .flex_shrink_0()
                            .px(px(crate::theme::size::BUTTON_X_TIGHT))
                            .tooltip(self.t("新增供应商", "Add provider"))
                            .accessibility_label(self.t("新增供应商", "Add provider"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.capture(cx);
                                this.send("add", vec![json!({"command":"add"})], cx);
                            })),
                        )
                        .child(
                            crate::ui_components::button(
                                "provider-copy",
                                crate::ui_components::ButtonKind::Ghost,
                                self.t("复制", "Copy"),
                                self.busy || selected.is_empty(),
                                false,
                            )
                            .icon(IconName::Copy)
                            .flex_shrink_0()
                            .px(px(crate::theme::size::BUTTON_X_TIGHT))
                            .tooltip(self.t("复制供应商", "Duplicate provider"))
                            .accessibility_label(self.t("复制供应商", "Duplicate provider"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                let mut steps = this.dirty_steps(cx);
                                steps.push(
                                    json!({"command":"duplicate","provider_id":this.selected()}),
                                );
                                this.send("duplicate", steps, cx);
                            })),
                        )
                        .child(
                            crate::ui_components::button(
                                "provider-delete",
                                crate::ui_components::ButtonKind::DangerOutline,
                                self.t("删除", "Delete"),
                                self.busy || selected.is_empty(),
                                false,
                            )
                            .icon(IconName::Delete)
                            .flex_shrink_0()
                            .px(px(crate::theme::size::BUTTON_X_TIGHT))
                            .tooltip(self.t("删除供应商", "Delete provider"))
                            .accessibility_label(self.t("删除供应商", "Delete provider"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.delete_open = true;
                                cx.notify();
                            })),
                        ),
                )
            });
        div()
            .flex()
            .flex_col()
            .gap(px(crate::theme::space::SECTION_GAP))
            .flex_shrink_0()
            .w(px(crate::theme::size::SIDEBAR_WIDTH))
            .h_full()
            .px(px(crate::theme::space::SIDEBAR_X))
            .py(px(crate::theme::space::SIDEBAR_Y))
            .bg(rgb(crate::theme::color::BG_BAR))
            .border_r_1()
            .border_color(rgb(crate::theme::color::BORDER))
            .child(brand)
            .child(
                Button::new("nav-statistics")
                    .ghost()
                    .w_full()
                    .h(px(crate::theme::size::NAV_ROW_HEIGHT))
                    .flex_shrink_0()
                    .justify_start()
                    .when(statistics_active, |button| {
                        button.bg(rgb(crate::theme::color::PRIMARY_TINT))
                    })
                    .accessibility_label(self.t("数据统计", "Statistics"))
                    .child(div().flex().flex_1().items_center().child(navigation_title(
                        self.t("数据统计", "Statistics"),
                        statistics_active,
                    )))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.switch_page(Page::Statistics, window, cx)
                    })),
            )
            .child(panel)
            .into_any_element()
    }
    fn center(&mut self, cx: &mut Context<Self>) -> AnyElement {
        if self.selected().is_empty() {
            return card("empty-provider")
                .flex_1()
                .items_center()
                .justify_center()
                .gap_4()
                .child(img("brand.png").size(px(64.)))
                .child(
                    div()
                        .text_lg()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(self.t("添加你的第一个供应商", "Add your first provider")),
                )
                .child(div().text_color(rgb(MUTED)).child(self.t(
                    "配置接口地址、Key 和模型即可开始",
                    "Configure an API URL, key and models to get started",
                )))
                .child(
                    Button::new("first-provider")
                        .primary()
                        .label(self.t("添加供应商", "Add provider"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.send("add", vec![json!({"command":"add"})], cx)
                        })),
                )
                .into_any_element();
        }
        let mono = cx.theme().mono_font_family.clone();
        // Connection settings (DESIGN_SPEC §4.3): preset, name/API key, protocol
        // and options, URL with its hint, the model list, thinking effort and the
        // two actions. Every callback and draft rule below is the pre-existing
        // one; only the visual structure changed.
        card("provider-settings")
            .flex_1()
            .min_w_0()
            .min_h_0()
            .p(px(crate::theme::space::CARD_SM))
            .gap(px(crate::theme::space::CARD_GAP))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(crate::theme::font::CARD_TITLE))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(self.t("连接设置", "Connection settings")),
                    )
                    .child(hint_line(self.t("修改后保存", "Save changes to apply"))),
            )
            .child(
                div()
                    .id("provider-form")
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .gap(px(crate::theme::space::CARD_GAP))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(crate::theme::space::SMALL))
                            .child(field_label(self.t("供应商预设", "Provider preset")))
                            .child(self.preset_menu()),
                    )
                    .child(
                        div()
                            .flex()
                            .gap(px(crate::theme::space::CARD_GAP))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .gap(px(crate::theme::space::SMALL))
                                    .child(field_label(self.t("供应商名称", "Provider name")))
                                    .child(crate::ui_components::text_input(&self.name, self.busy)),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .gap(px(crate::theme::space::SMALL))
                                    .child(field_label("API Key"))
                                    .child(crate::ui_components::masked_input(&self.key, self.busy)),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(crate::theme::space::SMALL))
                            .child(field_label(self.t("上游协议", "Upstream protocol")))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(crate::theme::space::CARD_GAP))
                                    .child(
                                        div().flex_1().min_w_0().child(
                                            crate::ui_components::select(
                                                &self.protocol_select,
                                                self.busy,
                                            )
                                            .w_full()
                                            .accessibility_label(
                                                self.t("上游协议", "Upstream protocol"),
                                            ),
                                        ),
                                    )
                                    .child(
                                        crate::ui_components::button(
                                            "protocol-options",
                                            crate::ui_components::ButtonKind::Normal,
                                            self.t("协议选项", "Protocol options"),
                                            self.busy,
                                            false,
                                        )
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            let draft = this
                                                .drafts
                                                .providers
                                                .get(&this.selected())
                                                .cloned()
                                                .unwrap_or_default();
                                            this.protocol_options.load(
                                                draft.protocol,
                                                &draft.protocol_settings,
                                                window,
                                                cx,
                                            );
                                            this.protocol_open = true;
                                            this.error = false;
                                            cx.notify();
                                        })),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(crate::theme::space::SMALL))
                            .child(field_label(self.t("API 地址", "API URL")))
                            .child(
                                crate::ui_components::text_input(&self.url, self.busy)
                                    .font_family(mono.clone()),
                            )
                            .child(hint_line(self.url_hint())),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .pt(px(crate::theme::space::TIGHT))
                            .child(
                                div()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_size(px(crate::theme::font::BODY))
                                    .child(self.t("模型列表", "Models")),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(crate::theme::space::ROW_GAP))
                                    .child(
                                        crate::ui_components::button(
                                            "fetch",
                                            crate::ui_components::ButtonKind::Outline,
                                            self.t("获取模型", "Fetch models"),
                                            self.busy,
                                            false,
                                        )
                                        .icon(IconName::RefreshCw)
                                        .on_click(cx.listener(|this, _, _, cx|{
                                            this.send("fetch",vec![json!({"command":"fetch","provider_id":this.selected(),"upstream":this.url.read(cx).value().to_string(),"key":this.key.read(cx).value().to_string(),"protocol":this.protocol_payload()["protocol"],"protocol_settings":this.protocol_payload()["protocol_settings"]})],cx);
                                        })),
                                    )
                                    .child(
                                        crate::ui_components::named_icon_button(
                                            "model-add",
                                            IconName::Plus,
                                            self.t("添加模型", "Add model"),
                                            self.t("添加模型", "Add model"),
                                            self.busy,
                                        )
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.edit(this.models.len(), true, window, cx)
                                        })),
                                    )
                                    .child(
                                        crate::ui_components::named_icon_button(
                                            "model-remove",
                                            IconName::Close,
                                            self.t("删除模型", "Remove models"),
                                            self.t("删除模型", "Remove models"),
                                            self.busy || self.selection.rows.is_empty(),
                                        )
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.remove_models(&RemoveModels, window, cx)
                                        })),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .key_context("ModelList")
                            .track_focus(&self.table_focus)
                            .flex_1()
                            .min_h(px(140.))
                            .on_action(cx.listener(Self::remove_models))
                            .on_action(cx.listener(Self::select_models))
                            .on_action(cx.listener(Self::edit_selected))
                            .on_prepaint({
                                let owner = cx.entity().downgrade();
                                move |bounds, _, cx| {
                                    let width = (bounds.size.width - px(2.)).max(px(240.));
                                    let _ = owner.update(cx, |this, cx| {
                                        if this.table_width != width {
                                            this.table_width = width;
                                            cx.notify();
                                        }
                                    });
                                }
                            })
                            .child(crate::ui_components::table(&self.model_table)),
                    )
                    .child(hint_line(self.t(
                        "Shift 连续多选 · Ctrl / ⌘ 多选 · 删除键移除 · 双击编辑",
                        "Shift range · Ctrl / ⌘ toggle · Delete to remove · Double-click to edit",
                    ))),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .gap(px(crate::theme::space::CARD_GAP))
                    .child(field_label(self.t("思考强度", "Thinking effort")))
                    .child(self.effort_menu())
                    .child(div().flex_1())
                    .child(
                        crate::ui_components::button(
                            "test",
                            crate::ui_components::ButtonKind::Outline,
                            self.t("测试连接", "Test connection"),
                            self.busy || self.models.is_empty(),
                            self.busy,
                        )
                        .on_click(cx.listener(|this, _, _, cx| this.test(cx))),
                    )
                    .child(
                        crate::ui_components::button(
                            "save",
                            crate::ui_components::ButtonKind::Primary,
                            self.t("保存", "Save"),
                            self.busy,
                            self.busy,
                        )
                        .icon(IconName::Check)
                        .on_click(cx.listener(|this, _, window, cx| this.save(&Save, window, cx))),
                    ),
            )
            .into_any_element()
    }
    /// Right rail (DESIGN_SPEC §4.3): the global-settings card above the log
    /// card, at the design's 300px base width. The rail itself does not scroll;
    /// the log area inside its card does.
    fn right(&mut self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .w(px(crate::theme::size::RIGHT_RAIL_WIDTH))
            .flex_shrink_0()
            .flex()
            .flex_col()
            .gap(px(crate::theme::space::SECTION_GAP))
            .child(self.global_settings_card(cx))
            .child(self.log_card(cx))
            .into_any_element()
    }

    /// Global settings card: the WorkBuddy sync actions, the local-relay
    /// sub-card (status, address, port confirmation entry, live counters) and
    /// the platform note. All callbacks are the pre-existing ones.
    fn global_settings_card(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let (dot, text, text_color) = self.core_indicator();
        let mono = cx.theme().mono_font_family.clone();
        rail_card("global-settings")
            .flex_shrink_0()
            .child(
                div()
                    .text_size(px(crate::theme::font::CARD_TITLE))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(self.t("全局设置", "Global settings")),
            )
            .when(self.error && self.editing.is_none(), |this| {
                this.child(
                    Label::new(self.status.clone())
                        .text_size(px(crate::theme::font::SMALL))
                        .text_color(rgb(crate::theme::color::DANGER)),
                )
            })
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(crate::theme::space::ROW_GAP))
                    .child(
                        crate::ui_components::button(
                            "sync",
                            crate::ui_components::ButtonKind::Primary,
                            self.t("同步 WorkBuddy", "Sync WorkBuddy"),
                            self.busy,
                            false,
                        )
                        .icon(IconName::RefreshCw)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.sync_open = true;
                            cx.notify();
                        })),
                    )
                    .child(
                        crate::ui_components::button(
                            "restore",
                            crate::ui_components::ButtonKind::Normal,
                            self.t("取消同步", "Unsync"),
                            self.busy,
                            false,
                        )
                        .icon(IconName::Delete)
                        .tooltip(self.t(
                            "移除代理同步到 WorkBuddy 的模型",
                            "Remove proxy models synced to WorkBuddy",
                        ))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.unsync_open = true;
                            cx.notify();
                        })),
                    ),
            )
            .child(
                Label::new(self.t(
                    "新增或修改模型后，旧会话若提示缺少地址或 Key，请完全退出并重开 WorkBuddy，再继续原会话。",
                    "After model changes, fully quit and reopen WorkBuddy if an existing conversation reports a missing URL or key; then continue that conversation.",
                ))
                .text_size(px(crate::theme::font::SMALL))
                .line_height(px(16.))
                .text_color(rgb(crate::theme::color::TEXT_3)),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(crate::theme::space::CARD_GAP))
                    .p(px(crate::theme::space::CARD_SM))
                    .rounded(px(crate::theme::radius::TABLE))
                    .bg(rgb(crate::theme::color::BG_PAGE))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                Label::new(self.t("本地代理", "Local relay"))
                                    .text_size(px(crate::theme::font::BODY)),
                            )
                            .child(
                                // D13: this entry used to print a hardcoded
                                // "running" state. It reports the same trusted
                                // core state as the status bar, rendered as the
                                // design's dot plus label (T04).
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(crate::theme::space::SMALL))
                                    .child(
                                        div()
                                            .w(px(crate::theme::size::STATUS_DOT))
                                            .h(px(crate::theme::size::STATUS_DOT))
                                            .rounded_full()
                                            .bg(rgb(dot)),
                                    )
                                    .child(
                                        Label::new(text)
                                            .text_size(px(crate::theme::font::SMALL))
                                            .text_color(rgb(text_color)),
                                    ),
                            ),
                    )
                    .child(
                        Label::new(self.proxy_address.clone())
                            .font_family(mono.clone())
                            .text_size(px(crate::theme::font::BODY))
                            .text_color(rgb(crate::theme::color::PRIMARY_DARK)),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(crate::theme::space::ROW_GAP))
                            .child(
                                div()
                                    .w(px(60.))
                                    .text_size(px(crate::theme::font::LABEL))
                                    .text_color(rgb(crate::theme::color::TEXT_3))
                                    .child(self.t("本地端口", "Local port")),
                            )
                            .child(
                                div().w(px(110.)).child(
                                    crate::ui_components::text_input(&self.port, self.busy)
                                        .aria_label(self.t("本地端口", "Local port")),
                                ),
                            )
                            .child(
                                crate::ui_components::named_icon_button(
                                    "port-apply",
                                    IconName::Check,
                                    self.t("应用端口", "Apply port"),
                                    self.t("应用端口", "Apply port"),
                                    self.busy,
                                )
                                .on_click(cx.listener(|this, _, _, cx| {
                                    match this
                                        .port
                                        .read(cx)
                                        .value()
                                        .parse::<u16>()
                                        .ok()
                                        .filter(|p| *p > 0)
                                    {
                                        Some(port) => {
                                            let current = this.proxy_address.parse::<reqwest::Url>().ok().and_then(|u| u.port_or_known_default()).unwrap_or(this.cfg.port);
                                            if port != current {
                                                this.pending_port = Some(port);
                                                cx.notify();
                                            }
                                        }
                                        None => {
                                            this.error = true;
                                            this.status = this
                                                .t(
                                                    "请输入 1–65535 的端口号",
                                                    "Enter a port from 1 to 65535",
                                                )
                                                .into();
                                            cx.notify();
                                        }
                                    }
                                })),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .gap(px(crate::theme::space::CARD_GAP))
                            .children(
                                [
                                    self.t("供应商", "Providers"),
                                    self.t("模型", "Models"),
                                    self.t("请求", "Requests"),
                                ]
                                .into_iter()
                                .zip(self.proxy_counts)
                                .map(|(name, count)| {
                                    div()
                                        .flex_1()
                                        .flex()
                                        .flex_col()
                                        .gap(px(crate::theme::space::TIGHT))
                                        .child(
                                            Label::new(count.to_string())
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .text_size(px(crate::theme::font::CARD_TITLE)),
                                        )
                                        .child(
                                            Label::new(name)
                                                .text_size(px(crate::theme::font::SMALL))
                                                .text_color(rgb(crate::theme::color::TEXT_3)),
                                        )
                                }),
                            ),
                    ),
            )
            .into_any_element()
    }

    /// Log card with its own scrolling text area; the clear and copy entries
    /// keep their original callbacks and payload.
    fn log_card(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let mono = cx.theme().mono_font_family.clone();
        rail_card("log-panel")
            .flex_1()
            .min_h_0()
            .child(
                div()
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(crate::theme::font::CARD_TITLE))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(self.t("日志", "Logs")),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(crate::theme::space::SMALL))
                            .child(
                                crate::ui_components::named_icon_button(
                                    "clear-logs",
                                    IconName::Delete,
                                    self.t("清空日志", "Clear logs"),
                                    self.t("清空日志", "Clear logs"),
                                    self.busy,
                                )
                                .on_click(cx.listener(
                                    |this, _, _, cx| {
                                        this.send(
                                            "clear_logs",
                                            vec![json!({"command":"clear_logs"})],
                                            cx,
                                        )
                                    },
                                )),
                            )
                            .child(
                                crate::ui_components::named_icon_button(
                                    "copy-logs",
                                    IconName::Copy,
                                    self.t("复制日志", "Copy logs"),
                                    self.t("复制日志", "Copy logs"),
                                    false,
                                )
                                .on_click(cx.listener(
                                    |this, _, _, cx| {
                                        cx.write_to_clipboard(ClipboardItem::new_string(format!(
                                            "{}\n\n{}",
                                            this.summary, this.log_text
                                        )));
                                        this.status = this.t("日志已复制", "Logs copied").into();
                                        this.error = false;
                                        cx.notify();
                                    },
                                )),
                            ),
                    ),
            )
            .child(
                div().flex_1().min_h_0().child(
                    Textarea::new(&self.logs)
                        .readonly(true)
                        .bordered(false)
                        .font_family(mono)
                        .text_size(px(crate::theme::font::SMALL))
                        .line_height(px(16.))
                        .h_full(),
                ),
            )
            .into_any_element()
    }
    fn dialog_body(&mut self, window: &Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let content = if self.protocol_open {
            crate::ui_components::dialog_body_sized(window.viewport_size().height, 210., 360., 570.)
                .child(crate::ui_components::dialog_title(
                    self.t("上游协议选项", "Upstream protocol options"),
                ))
                .child(
                    self.protocol_options
                        .render(self.zh, cx.theme().mono_font_family.clone()),
                )
                .when(self.error, |this| {
                    this.child(
                        Label::new(self.status.clone())
                            .text_size(px(crate::theme::font::LABEL))
                            .text_color(rgb(crate::theme::color::DANGER)),
                    )
                })
                .child(
                    crate::ui_components::dialog_footer().child(
                        crate::ui_components::button(
                            "protocol-apply",
                            crate::ui_components::ButtonKind::Primary,
                            self.t("应用到草稿", "Apply to draft"),
                            false,
                            false,
                        )
                        .on_click(
                            cx.listener(|this, _, window, cx| this.confirm_protocol(window, cx)),
                        ),
                    ),
                )
                .into_any_element()
        } else if self.picker.is_some() {
            self.picker_dialog(cx)
        } else if self.editing.is_some() {
            crate::ui_components::dialog_body_sized(window.viewport_size().height, 210., 360., 570.)
                .child(crate::ui_components::dialog_title(if self.new_model {
                    self.t("添加模型", "Add model")
                } else {
                    self.t("编辑模型", "Edit model")
                }))
                .child(
                    crate::ui_components::scrollable_form("model-editor-fields")
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(crate::theme::space::SMALL))
                                .child(field_label(self.t("显示名", "Display name")))
                                .child(crate::ui_components::text_input(&self.edit_name, false)),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(crate::theme::space::SMALL))
                                .child(field_label(self.t("模型 ID", "Model ID")))
                                .child(crate::ui_components::text_input(&self.edit_model, false)),
                        )
                        .child(crate::model_options::render(self, cx)),
                )
                .when(self.error, |this| {
                    this.child(
                        Label::new(self.status.clone())
                            .text_size(px(crate::theme::font::LABEL))
                            .text_color(rgb(crate::theme::color::DANGER)),
                    )
                })
                .child(
                    crate::ui_components::dialog_footer()
                        .child(
                            crate::ui_components::button(
                                "edit-cancel",
                                crate::ui_components::ButtonKind::Normal,
                                self.t("取消", "Cancel"),
                                false,
                                false,
                            )
                            .on_click(
                                cx.listener(|this, _, window, cx| {
                                    this.dismiss(&Dismiss, window, cx)
                                }),
                            ),
                        )
                        .child(
                            crate::ui_components::button(
                                "edit-confirm",
                                crate::ui_components::ButtonKind::Primary,
                                self.t("确认", "Confirm"),
                                false,
                                false,
                            )
                            .on_click(cx.listener(|this, _, _, cx| this.confirm_edit(cx))),
                        ),
                )
                .into_any_element()
        } else if let Some(port) = self.pending_port {
            crate::ui_components::dialog_body()
                .w_full()
                .gap(px(crate::theme::space::CARD_GAP))
                .child(crate::ui_components::dialog_title(self.t(
                    "修改端口并重启 WorkBuddy？",
                    "Change port and restart WorkBuddy?",
                )))
                .child(body_line(format!("{} → 127.0.0.1:{port}", self.proxy_address)))
                .child(body_hint(self.t(
                    "更改端口后必须重启 WorkBuddy，让现有会话读取新的连接地址。重启会中断正在执行的任务，请先保存工作。取消不会应用端口变更。",
                    "WorkBuddy must restart to let existing conversations read the new address. This interrupts running tasks; save your work first. Cancel leaves the port unchanged.",
                )))
                .when(!cfg!(target_os = "macos"), |this| {
                    this.child(body_hint(self.t(
                        "此平台暂不支持自动重启，无法执行此端口变更。",
                        "Automatic restart is unavailable on this platform; the port cannot be changed here.",
                    )))
                })
                .child(
                    crate::ui_components::dialog_footer()
                        .child(
                            crate::ui_components::button(
                                "port-restart-confirm",
                                crate::ui_components::ButtonKind::Primary,
                                self.t("同意并重启", "Agree and restart"),
                                self.busy || !cfg!(target_os = "macos"),
                                false,
                            )
                            .on_click(cx.listener(|this, _, _, cx| this.confirm_port(cx))),
                        )
                        .child(
                            crate::ui_components::button(
                                "port-restart-cancel",
                                crate::ui_components::ButtonKind::Normal,
                                self.t("取消", "Cancel"),
                                false,
                                false,
                            )
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.dismiss(&Dismiss, window, cx)
                            })),
                        ),
                )
                .into_any_element()
        } else if self.startup_sync_open {
            let report = self.startup_report.as_ref()?;
            crate::ui_components::dialog_body()
                .w_full()
                .gap(px(crate::theme::space::CARD_GAP))
                .child(crate::ui_components::dialog_title(self.t(
                    "WorkBuddy 连接地址不一致",
                    "WorkBuddy endpoint mismatch",
                )))
                .child(body_line(format!(
                    "{} {}",
                    self.t("当前代理地址：", "Current proxy:"),
                    report.expected_address
                )))
                .child(body_hint(format!(
                    "{} {} / {} · {}",
                    self.t("地址不一致的模型：", "Mismatched models:"),
                    report.mismatch_count,
                    report.checked_count,
                    report.actual_addresses.join(", ")
                )))
                .child(body_hint(self.t(
                    "是否按已保存的供应商配置同步并重启 WorkBuddy？重启会中断正在执行的任务，请先保存工作。取消将保留现有配置。",
                    "Sync saved provider settings and restart WorkBuddy? Restart interrupts running tasks; save your work first. Cancel keeps the existing configuration.",
                )))
                .when(!cfg!(target_os = "macos"), |this| {
                    this.child(body_hint(self.t(
                        "此平台暂不支持自动重启，请手动同步后重启 WorkBuddy。",
                        "Automatic restart is unavailable on this platform; sync and restart WorkBuddy manually.",
                    )))
                })
                .child(
                    crate::ui_components::dialog_footer()
                        .child(
                            crate::ui_components::button(
                                "startup-sync-restart",
                                crate::ui_components::ButtonKind::Primary,
                                self.t("同步并重启 WorkBuddy", "Sync and restart WorkBuddy"),
                                self.busy || !cfg!(target_os = "macos"),
                                false,
                            )
                            .on_click(
                                cx.listener(|this, _, _, cx| this.confirm_startup_sync(cx)),
                            ),
                        )
                        .child(
                            crate::ui_components::button(
                                "startup-sync-cancel",
                                crate::ui_components::ButtonKind::Normal,
                                self.t("取消", "Cancel"),
                                false,
                                false,
                            )
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.dismiss(&Dismiss, window, cx)
                            })),
                        ),
                )
                .into_any_element()
        } else if self.unsync_open {
            crate::ui_components::dialog_body()
                .w_full()
                .gap(px(crate::theme::space::CARD_GAP))
                .child(crate::ui_components::dialog_title(self.t(
                    "取消同步 WorkBuddy",
                    "Unsync WorkBuddy",
                )))
                .child(body_hint(self.t(
                    "将移除代理同步的模型，并恢复已备份的原有条目。重启 WorkBuddy 可让现有会话读取修改，但会中断正在执行的任务，请先保存工作。",
                    "Remove proxy-managed models and restore backed-up original entries. Restart WorkBuddy to let existing conversations read the change; this interrupts running tasks. Save your work first.",
                )))
                .when(!cfg!(target_os = "macos"), |this| {
                    this.child(body_hint(self.t(
                        "此平台暂不支持自动重启；仅移除后请手动重开 WorkBuddy。",
                        "Automatic restart is unavailable on this platform; remove the models, then reopen WorkBuddy manually.",
                    )))
                })
                .child(
                    crate::ui_components::dialog_footer()
                        .child(
                            crate::ui_components::button(
                                "unsync-restart",
                                crate::ui_components::ButtonKind::Primary,
                                self.t("移除并重启", "Remove and restart"),
                                self.busy || !cfg!(target_os = "macos"),
                                false,
                            )
                            .on_click(cx.listener(|this, _, _, cx| this.unsync(true, cx))),
                        )
                        .child(
                            crate::ui_components::button(
                                "unsync-only",
                                crate::ui_components::ButtonKind::Normal,
                                self.t("仅移除", "Remove only"),
                                self.busy,
                                false,
                            )
                            .on_click(cx.listener(|this, _, _, cx| this.unsync(false, cx))),
                        )
                        .child(
                            crate::ui_components::button(
                                "unsync-cancel",
                                crate::ui_components::ButtonKind::Normal,
                                self.t("取消", "Cancel"),
                                false,
                                false,
                            )
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.dismiss(&Dismiss, window, cx)
                            })),
                        ),
                )
                .into_any_element()
        } else if self.sync_open {
            crate::ui_components::dialog_body()
                .w_full()
                .gap(px(crate::theme::space::CARD_GAP))
                .child(crate::ui_components::dialog_title(
                    self.t("同步 WorkBuddy", "Sync WorkBuddy"),
                ))
                .child(body_hint(self.t(
                    "同步会保存当前供应商的修改，并更新所有已保存供应商的模型配置。重启可让旧会话读取新配置，但会中断 WorkBuddy 正在执行的任务，请先保存工作。",
                    "Sync saves changes to the current provider and updates models for all saved providers. Restart lets existing conversations load the new configuration, but interrupts running WorkBuddy tasks. Save your work first.",
                )))
                .when(!cfg!(target_os = "macos"), |this| {
                    this.child(body_hint(self.t(
                        "此平台暂不支持自动重启；仅同步后请手动重开 WorkBuddy。",
                        "Automatic restart is not supported on this platform yet. Sync, then reopen WorkBuddy manually.",
                    )))
                })
                .child(
                    crate::ui_components::dialog_footer()
                        .child(
                            crate::ui_components::button(
                                "sync-restart",
                                crate::ui_components::ButtonKind::Primary,
                                self.t("同步并重启", "Sync and restart"),
                                self.busy || !cfg!(target_os = "macos"),
                                false,
                            )
                            .on_click(cx.listener(|this, _, _, cx| this.sync(true, cx))),
                        )
                        .child(
                            crate::ui_components::button(
                                "sync-only",
                                crate::ui_components::ButtonKind::Normal,
                                self.t("仅同步", "Sync only"),
                                self.busy,
                                false,
                            )
                            .on_click(cx.listener(|this, _, _, cx| this.sync(false, cx))),
                        )
                        .child(
                            crate::ui_components::button(
                                "sync-cancel",
                                crate::ui_components::ButtonKind::Normal,
                                self.t("取消", "Cancel"),
                                false,
                                false,
                            )
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.dismiss(&Dismiss, window, cx)
                            })),
                        ),
                )
                .into_any_element()
        } else if self.delete_open {
            crate::ui_components::dialog_body()
                .w_full()
                .gap(px(crate::theme::space::CARD_GAP))
                .child(crate::ui_components::dialog_title(
                    self.t("删除供应商？", "Delete provider?"),
                ))
                .child(body_hint(self.t(
                    "将移除此供应商及其保存的 Key 和模型。",
                    "This removes the provider and its saved key and models.",
                )))
                .child(
                    crate::ui_components::dialog_footer()
                        .child(
                            crate::ui_components::button(
                                "delete-cancel",
                                crate::ui_components::ButtonKind::Normal,
                                self.t("取消", "Cancel"),
                                false,
                                false,
                            )
                            .on_click(
                                cx.listener(|this, _, window, cx| {
                                    this.dismiss(&Dismiss, window, cx)
                                }),
                            ),
                        )
                        .child(
                            crate::ui_components::button(
                                "delete-confirm",
                                crate::ui_components::ButtonKind::Danger,
                                self.t("删除", "Delete"),
                                false,
                                false,
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.delete_open = false;
                                this.send(
                                    "delete",
                                    vec![json!({"command":"delete","provider_id":this.selected()})],
                                    cx,
                                );
                            })),
                        ),
                )
                .into_any_element()
        } else {
            return None;
        };
        Some(content)
    }

    fn picker_dialog(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let filtered = self.filtered(cx);
        let count = filtered.len();
        let chosen = self.picked.len();
        let total = self.picker.as_ref().map_or(0, Vec::len);
        // "已选" is how many models this provider already has; "总数" is every
        // discovered candidate and "待添加" is the current pick. The numbers in
        // the design are sample data and are never copied.
        let added = self.models.len();
        let picker_button = |id: &'static str, label: &'static str, active: bool| {
            crate::ui_components::button(
                id,
                if active {
                    crate::ui_components::ButtonKind::Outline
                } else {
                    crate::ui_components::ButtonKind::Normal
                },
                label,
                false,
                false,
            )
        };
        crate::ui_components::dialog_body()
            .w_full()
            .h(px(550.))
            .gap(px(crate::theme::space::CARD_GAP))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(crate::ui_components::dialog_title(
                        self.t("选择模型", "Select models"),
                    ))
                    .child(hint_line(format!(
                        "{} {} / {}  ·  {} {}",
                        self.t("已选", "Added"),
                        added,
                        total,
                        chosen,
                        self.t("待添加", "to add")
                    ))),
            )
            .child(crate::ui_components::text_input(&self.search, false))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(crate::theme::space::ROW_GAP))
                    .child(
                        picker_button(
                            "picker-select-all",
                            self.t("全选当前筛选", "Select filtered"),
                            false,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            for i in this.filtered(cx) {
                                if !this.models.iter().any(|m| {
                                    m["model"].as_str()
                                        == this
                                            .picker
                                            .as_ref()
                                            .and_then(|ids| ids.get(i))
                                            .map(String::as_str)
                                }) {
                                    this.picked.insert(i);
                                }
                            }
                            cx.notify();
                        })),
                    )
                    .child(
                        picker_button(
                            "picker-invert",
                            self.t("反选当前筛选", "Invert filtered"),
                            false,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            for i in this.filtered(cx) {
                                if !this.models.iter().any(|m| {
                                    m["model"].as_str()
                                        == this
                                            .picker
                                            .as_ref()
                                            .and_then(|ids| ids.get(i))
                                            .map(String::as_str)
                                }) && !this.picked.remove(&i)
                                {
                                    this.picked.insert(i);
                                }
                            }
                            cx.notify();
                        })),
                    )
                    .child(
                        picker_button("picker-clear", self.t("清空选择", "Clear selection"), false)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.picked.clear();
                                cx.notify();
                            })),
                    )
                    .child(
                        picker_button(
                            "picker-only",
                            self.t("仅看已勾选", "Selected only"),
                            self.only_picked,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.only_picked = !this.only_picked;
                            cx.notify();
                        })),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .border_1()
                    .border_color(rgb(crate::theme::color::BORDER))
                    .rounded(px(crate::theme::radius::TABLE))
                    .overflow_hidden()
                    .child(
                        uniform_list(
                            "discovered-models",
                            count,
                            cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
                                range
                                    .map(|index| {
                                        let i = filtered[index];
                                        let id = this.picker.as_ref().unwrap()[i].clone();
                                        let existing = this
                                            .models
                                            .iter()
                                            .any(|m| m["model"].as_str() == Some(&id));
                                        let checked = existing || this.picked.contains(&i);
                                        let zh = this.zh;
                                        ListItem::new(("candidate", i))
                                            .w_full()
                                            .h(px(crate::theme::size::LIST_ROW_HEIGHT))
                                            // Models already in this provider keep the
                                            // design's tinted row and trailing note.
                                            .when(existing, |row| {
                                                row.bg(rgb(crate::theme::color::BG_TABLE_HEAD))
                                            })
                                            .accessibility_label(id.clone())
                                            .child(
                                                crate::ui_components::checkbox(
                                                    ("candidate-check", i),
                                                    id,
                                                    checked,
                                                    existing,
                                                )
                                                .accessibility_id(format!("candidate-{i}"))
                                                .w_full()
                                                .on_change(cx.listener(
                                                    move |this, checked: &bool, _, cx| {
                                                        if *checked {
                                                            this.picked.insert(i);
                                                        } else {
                                                            this.picked.remove(&i);
                                                        }
                                                        cx.notify();
                                                    },
                                                )),
                                            )
                                            .suffix(move |_, _| {
                                                Label::new(if existing {
                                                    if zh { "已添加" } else { "Added" }
                                                } else {
                                                    ""
                                                })
                                                .text_size(px(crate::theme::font::SMALL))
                                                .text_color(rgb(crate::theme::color::TEXT_3))
                                            })
                                            .into_any_element()
                                    })
                                    .collect()
                            }),
                        )
                        .size_full(),
                    ),
            )
            .child(hint_line(self.t(
                "已有模型保留，确认添加后仍需保存。",
                "Existing models are kept. Save after adding your selection.",
            )))
            .child(
                crate::ui_components::dialog_footer()
                    .child(
                        crate::ui_components::button(
                            "picker-cancel",
                            crate::ui_components::ButtonKind::Normal,
                            self.t("取消", "Cancel"),
                            false,
                            false,
                        )
                        .on_click(
                            cx.listener(|this, _, window, cx| this.dismiss(&Dismiss, window, cx)),
                        ),
                    )
                    .child(
                        crate::ui_components::button(
                            "picker-confirm",
                            crate::ui_components::ButtonKind::Primary,
                            format!("{} ({chosen})", self.t("确认添加", "Add selected")),
                            chosen == 0,
                            false,
                        )
                        .on_click(cx.listener(|this, _, _, cx| this.apply_picker(cx))),
                    ),
            )
            .into_any_element()
    }
}
impl Render for Proxy {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.refresh_table(window, cx);
        let sidebar = self.sidebar(cx);
        let center = match self.page {
            Page::Providers => self.center(cx),
            Page::Statistics => self.statistics.clone().into_any_element(),
        };
        let right = self.right(cx);
        self.manage_dialog(window, cx);
        let status_bar = self.status_bar();
        // Window skeleton (DESIGN_SPEC §2): the fixed 224px sidebar sits beside
        // the main column (36px top icon row over the page body), and the 30px
        // status bar spans the full window width underneath both. Only the page
        // bodies scroll; the sidebar tool row and the status bar stay put.
        let content = div()
            .relative()
            .key_context("Proxy")
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::save))
            .on_action(cx.listener(Self::dismiss))
            .on_action(cx.listener(Self::quit))
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(BG))
            .text_color(rgb(INK))
            .text_size(px(13.))
            .child(
                div().flex().flex_1().min_h_0().child(sidebar).child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_w_0()
                        .min_h_0()
                        .px(px(crate::theme::space::PAGE_X))
                        .pt(px(crate::theme::space::PAGE_TOP))
                        .pb(px(crate::theme::space::PAGE_BOTTOM))
                        .gap(px(crate::theme::space::SECTION_GAP))
                        .child(
                            div()
                                .flex()
                                .gap_3()
                                .flex_1()
                                .min_h_0()
                                .child(center)
                                .when(self.page == Page::Providers, |this| this.child(right)),
                        ),
                ),
            )
            .child(status_bar);
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                TitleBar::new()
                    .h(px(crate::theme::size::TOP_ICON_ROW_HEIGHT))
                    .bg(rgb(BG))
                    .border_b_0()
                    // Kit reserves macOS traffic lights on the left and adds
                    // native Windows min/max/close hit areas on the right.
                    .child(div().flex_1().min_w_0().h_full())
                    .child(
                        div()
                            .h_full()
                            .pr(px(if cfg!(target_os = "macos") {
                                crate::theme::space::PAGE_X
                            } else {
                                8.
                            }))
                            .child(self.app_controls(cx)),
                    ),
            )
            .child(content.flex_1().min_h_0())
    }
}
impl Drop for Proxy {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(self.dir.join("gpui.json"));
    }
}

struct ModalBody {
    owner: WeakEntity<Proxy>,
}
impl Render for ModalBody {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.owner
            .update(cx, |this, cx| this.dialog_body(window, cx))
            .ok()
            .flatten()
            .unwrap_or_else(|| div().into_any_element())
    }
}
