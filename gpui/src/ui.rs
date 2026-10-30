mod autostart_notice;
mod language_menu;
mod model_editor;
mod model_navigation;
pub(crate) use model_navigation::bindings as model_navigation_bindings;
mod model_picker;
mod protocol_dialog;
mod provider_choices;
mod provider_delete;
mod provider_edit;
mod provider_feedback;
mod provider_new;
mod providers;
mod relay;
mod sidebar;
mod skin_controls;
mod sync_dialog;
mod update_notice;
mod window_restore;
use crate::{
    backend::{Backend, Job},
    config::{self, Config},
    state::{Draft, Drafts, ModelMove, Selection},
    statistics::Statistics,
};
use gpui_kit::component::{
    ActiveTheme, ElementExt, Icon, IconName, InteractiveElementExt as _, Selectable, Sizable,
    TitleBar, WindowExt,
    button::{Button, ButtonVariants},
    input::{InputEvent, InputState, TextareaState},
    label::Label,
    select::{SelectEvent, SelectItem},
    table::TableState,
};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    time::{Duration, Instant},
};
gpui_kit::actions!(
    proxy,
    [
        Save,
        RemoveModels,
        SelectModels,
        EditModel,
        ModelPrevious,
        ModelNext,
        ModelFirst,
        ModelLast,
        ExtendModelPrevious,
        ExtendModelNext,
        ExtendModelFirst,
        ExtendModelLast,
        ModelNextControl,
        ModelPreviousControl,
        Dismiss,
        DismissModelInput,
        Quit,
        ActivateProtocolTab,
        ActivateProviderDeleteControl,
        ActivateProviderProfileControl
    ]
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

const BG: u32 = crate::theme::color::BG_PAGE;
const INK: u32 = crate::theme::color::TEXT_1;
const MUTED: u32 = crate::theme::color::TEXT_3;
const PROJECT_URL: &str = "https://github.com/gloryhui/workbuddy-proxy";
const FEEDBACK_URL: &str = "https://github.com/gloryhui/workbuddy-proxy/issues/new";

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
type ChoiceState = provider_choices::MenuState;
fn protocol_choices(zh: bool) -> provider_choices::MenuChoices {
    let items = [
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
    .collect();
    provider_choices::MenuChoices::new(items, provider_choices::MenuKind::Protocol, zh)
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
fn preset_choices(zh: bool) -> provider_choices::MenuChoices {
    provider_choices::MenuChoices::new(
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
        provider_choices::MenuKind::Preset,
        zh,
    )
}
pub struct Proxy {
    dir: PathBuf,
    page: Page,
    providers_expanded: bool,
    statistics: Entity<Statistics>,
    cfg: Config,
    drafts: Drafts,
    provider_edits: provider_edit::ProviderEdits,
    /// Completion context is only known for successful creations in this window.
    /// Existing saved rows must not acquire an extra completion badge.
    saved_creations: BTreeMap<String, provider_new::CreationKind>,
    provider_profile_focus: FocusHandle,
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
    provider_search: Entity<InputState>,
    provider_scroll: ScrollHandle,
    provider_rail_scroll: ScrollHandle,
    provider_rail_layout: Option<sidebar::RailLayout>,
    edit_name: Entity<InputState>,
    edit_model: Entity<InputState>,
    pub(crate) model_options: crate::model_options::OptionsEditor,
    model_edit_error: Option<String>,
    model_editor_scroll: ScrollHandle,
    model_editor_layout: Option<(Pixels, bool)>,
    model_editor_focused_row: usize,
    model_editor_focus: [FocusHandle; 6],
    logs: Entity<TextareaState>,
    log_text: String,
    summary: String,
    proxy_address: String,
    proxy_counts: [u64; 3],
    models: Vec<Value>,
    effort: String,
    selection: Selection,
    table_focus: FocusHandle,
    modal_focus: FocusHandle,
    model_table: Entity<TableState<crate::model_table::Models>>,
    table_width: Pixels,
    dialog_open: bool,
    protocol_open: bool,
    protocol_error: Option<String>,
    protocol_scroll: ScrollHandle,
    protocol_tab_focus: [FocusHandle; 4],
    protocol_focused_control: usize,
    protocol_layout: Option<(Pixels, crate::upstream_config::UpstreamProtocol)>,
    protocol_select: Entity<ChoiceState>,
    protocol_options: crate::protocol_options::Editor,
    focus: FocusHandle,
    picker: Option<Vec<String>>,
    picked: BTreeSet<usize>,
    only_picked: bool,
    picker_list: Entity<gpui_kit::component::list::ListState<model_picker::Candidates>>,
    picker_focus: FocusHandle,
    editing: Option<usize>,
    new_model: bool,
    effort_select: Entity<ChoiceState>,
    preset_select: Entity<ChoiceState>,
    delete_open: bool,
    provider_delete_target: Option<provider_delete::DeleteTarget>,
    delete_feedback: provider_feedback::DeleteFeedback,
    provider_delete_scroll: ScrollHandle,
    provider_delete_focus: [FocusHandle; 2],
    sync_open: bool,
    unsync_open: bool,
    startup_checked: bool,
    startup_report: Option<crate::workbuddy_status::EndpointCheck>,
    startup_sync_open: bool,
    port_open: bool,
    port_error: bool,
    pending_port: Option<u16>,
    resetting_window: bool,
    skin_open: bool,
    skin_error: bool,
    skin_focus: FocusHandle,
    language_menu: language_menu::State,
    window_restore: window_restore::State,
    autostart_notice: autostart_notice::State,
    update_notice: update_notice::State,
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
                &self.provider_search,
                if zh {
                    "搜索供应商"
                } else {
                    "Search providers"
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
            s.set_items(
                provider_choices::MenuChoices::new(
                    choices(self.zh, false),
                    provider_choices::MenuKind::Effort,
                    self.zh,
                ),
                window,
                cx,
            );
            s.set_selected_value(&self.effort, window, cx);
        });
        self.preset_select
            .update(cx, |s, cx| s.set_items(preset_choices(self.zh), window, cx));
        self.protocol_select.update(cx, |s, cx| {
            let selected = s.selected_value().cloned();
            s.set_items(protocol_choices(self.zh), window, cx);
            if let Some(selected) = selected {
                s.set_selected_value(&selected, window, cx);
            }
        });
        self.sync_preset(window, cx);
    }
    fn app_controls(&self, cx: &mut Context<Self>) -> AnyElement {
        // Figma 103:442: all seven original actions, in their original order.
        // The shared native buttons retain focus, tooltips and disabled rules.
        let toolbar = div()
            .id("titlebar-actions")
            .w(px(crate::theme::size::SYSTEM_TOOLBAR_WIDTH))
            .flex()
            .flex_shrink_0()
            .h(px(crate::theme::size::TOP_ICON_ROW_HEIGHT))
            .items_center()
            .gap(px(crate::theme::size::SYSTEM_GAP))
            // Exclude actions from the TitleBar's native Windows drag hitbox
            // and prevent macOS mouse movement/double clicks from dragging or
            // zooming the window while the user operates a toolbar button.
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_double_click(|_, _, cx| cx.stop_propagation())
            .child(self.skin_control(cx))
            .child(self.autostart_control(cx))
            .child(
                crate::ui_components::system_button(
                    "check-update",
                    "studio/updates.svg",
                    self.t("检查更新", "Check updates"),
                    self.t("检查更新", "Check updates"),
                    self.busy,
                )
                .on_click(
                    cx.listener(|this, _, window, cx| this.check_update_with_notice(window, cx)),
                ),
            )
            .child(self.language_menu(cx))
            .child(
                crate::ui_components::system_button(
                    "restore-window-size",
                    "studio/restore.svg",
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
                crate::ui_components::system_button(
                    "project-homepage",
                    "studio/github.svg",
                    self.t("GitHub 项目主页", "GitHub project"),
                    self.t("GitHub 项目主页", "GitHub project"),
                    false,
                )
                .on_click(|_, _, cx| cx.open_url(PROJECT_URL)),
            )
            .child(
                crate::ui_components::system_button(
                    "submit-feedback",
                    "studio/feedback.svg",
                    self.t("提交反馈", "Submit feedback"),
                    self.t("提交反馈", "Submit feedback"),
                    false,
                )
                .on_click(|_, _, cx| cx.open_url(FEEDBACK_URL)),
            )
            .into_any_element();
        let toolbar = self.language_toolbar(toolbar, cx);
        let toolbar = self.restore_toolbar(toolbar, cx);
        let toolbar = self.autostart_toolbar(toolbar, cx);
        self.update_toolbar(toolbar, cx)
    }
    fn restore_window_size(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.resetting_window {
            return;
        }
        self.show_restore_notice(window, cx);
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
                        this.restore_notice_failed();
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
    fn effort_menu(&self, cx: &App) -> AnyElement {
        provider_choices::control(
            &self.effort_select,
            self.busy,
            self.t("思考强度", "Thinking effort"),
            Some(crate::theme::size::EFFORT_SELECT_WIDTH),
            cx,
        )
    }
    fn preset_menu(&self, cx: &App) -> AnyElement {
        provider_choices::control(
            &self.preset_select,
            self.busy,
            self.t("供应商预设", "Provider preset"),
            None,
            cx,
        )
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
        model_picker::init(cx);
        provider_choices::init(cx);
        language_menu::init(cx);
        window_restore::init(cx);
        autostart_notice::init(cx);
        update_notice::init(cx);
        let drafts = Drafts::load(&dir, &cfg);
        let provider_search = input(
            "",
            if zh {
                "搜索供应商"
            } else {
                "Search providers"
            },
            false,
            window,
            cx,
        );
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
        // Model names already reject control characters in Core validation.
        // Keep an IME cancellation key from becoming editable name content.
        edit_name.update(cx, |state, cx| {
            state.set_validator(|value, _| !value.chars().any(char::is_control), cx);
        });
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
        let effort_select = cx.new(|cx| {
            ChoiceState::new(
                provider_choices::MenuChoices::new(
                    choices(zh, false),
                    provider_choices::MenuKind::Effort,
                    zh,
                ),
                window,
                cx,
            )
        });
        let preset_select = cx.new(|cx| ChoiceState::new(preset_choices(zh), window, cx));
        let protocol_options = crate::protocol_options::Editor::new(window, cx);
        let protocol_select = cx.new(|cx| ChoiceState::new(protocol_choices(zh), window, cx));
        let logs = cx.new(|cx| TextareaState::new(window, cx));
        let focus = cx.focus_handle();
        let table_focus = cx.focus_handle().tab_stop(true);
        let modal_focus = cx.focus_handle();
        let owner = cx.entity().downgrade();
        let picker_list = cx.new(|cx| {
            gpui_kit::component::list::ListState::new(
                model_picker::Candidates::new(owner.clone(), zh),
                window,
                cx,
            )
        });
        let picker_focus = picker_list.read(cx).focus_handle(cx).tab_stop(true);
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
        // The application owns multi-selection and its single Tab entry. Kit's
        // private single-selection focus stays usable by pointer, but is not a
        // second Tab stop. FocusHandle clones share this registered setting.
        model_table.read(cx).focus_handle(cx).tab_stop(false);
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
            provider_edits: provider_edit::ProviderEdits::default(),
            saved_creations: BTreeMap::new(),
            provider_profile_focus: cx.focus_handle().tab_stop(false),
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
            provider_search,
            provider_scroll: ScrollHandle::new(),
            provider_rail_scroll: ScrollHandle::new(),
            provider_rail_layout: None,
            edit_name,
            edit_model,
            model_options,
            model_edit_error: None,
            model_editor_scroll: ScrollHandle::new(),
            model_editor_layout: None,
            model_editor_focused_row: 0,
            model_editor_focus: std::array::from_fn(|_| cx.focus_handle().tab_stop(false)),
            logs,
            log_text: String::new(),
            summary: String::new(),
            proxy_address: String::new(),
            proxy_counts: [0; 3],
            models: vec![],
            effort: "keep".into(),
            selection: Selection::default(),
            table_focus,
            modal_focus,
            model_table,
            table_width: px(0.),
            dialog_open: false,
            protocol_open: false,
            protocol_error: None,
            protocol_scroll: ScrollHandle::new(),
            protocol_tab_focus: std::array::from_fn(|_| cx.focus_handle().tab_stop(false)),
            protocol_focused_control: 2,
            protocol_layout: None,
            protocol_select,
            protocol_options,
            focus,
            picker: None,
            picked: BTreeSet::new(),
            only_picked: false,
            picker_list,
            picker_focus,
            editing: None,
            new_model: false,
            effort_select,
            preset_select,
            delete_open: false,
            provider_delete_target: None,
            delete_feedback: Default::default(),
            provider_delete_scroll: ScrollHandle::new(),
            provider_delete_focus: std::array::from_fn(|_| cx.focus_handle().tab_stop(false)),
            sync_open: false,
            unsync_open: false,
            startup_checked: false,
            startup_report: None,
            startup_sync_open: false,
            port_open: false,
            port_error: false,
            pending_port: None,
            resetting_window: false,
            skin_open: false,
            skin_error: false,
            skin_focus: cx.focus_handle().tab_stop(false),
            language_menu: language_menu::State::new(zh, window, cx),
            window_restore: window_restore::State::new(cx),
            autostart_notice: autostart_notice::State::new(cx),
            update_notice: update_notice::State::new(cx),
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
                this.capture(cx);
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
        this.subscribe_model_editor_focus(window, cx);
        this.subscribe_skin_focus(window, cx);
        this.subscribe_protocol_editor_focus(window, cx);
        this.subscribe_provider_delete_bounds(window, cx);
        this.subscribe_provider_rail_bounds(window, cx);
        this.subscribe_restore_notice_bounds(window, cx);
        this.subscribe_autostart_notice_bounds(window, cx);
        this.subscribe_update_notice_bounds(window, cx);
        this.load_form(window, cx);
        // Current-provider labels and save state follow editing events, rather
        // than waiting for an idle draft-persistence tick to redraw the UI.
        for state in [&this.name, &this.key, &this.url] {
            cx.subscribe(state, |this, _, event, cx| {
                if matches!(event, InputEvent::Change) {
                    this.capture(cx);
                    cx.notify();
                }
            })
            .detach();
        }
        // The shared heading follows the statistics view without duplicating
        // the report's state or changing its refresh cadence.
        cx.observe(&this.statistics, |_, _, cx| cx.notify())
            .detach();
        this.switch_page(Page::Statistics, window, cx);
        this.focus.focus(window, cx);
        cx.subscribe(&this.search, |_, _, event, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        })
        .detach();
        cx.subscribe(&this.provider_search, |_, _, event, cx| {
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
                    d.protocol = self.protocol_options.protocol();
                    d.protocol_settings = settings;
                }
                let protocol = self.protocol_options.protocol();
                self.protocol_select.update(cx, |s, cx| {
                    s.set_selected_value(&protocol.as_str().to_owned(), window, cx)
                });
                self.last_metadata = Instant::now() - Duration::from_secs(10);
                self.sync_preset(window, cx);
                self.protocol_error = None;
                self.protocol_open = false;
                self.error = false;
            }
            Err(error) => {
                self.protocol_error = Some(error);
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
            .map(|d| {
                vec![if let Some(pending) = self
                    .drafts
                    .pending
                    .as_ref()
                    .filter(|p| p.id == self.selected())
                {
                    crate::provider_create::request(pending, d)
                } else {
                    d.payload(&self.selected())
                }]
            })
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
            || self.port_open
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
        if self.pending_selected() {
            return self.save_steps(cx);
        }
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
        if id != self.selected() {
            self.delete_feedback.clear();
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
        if tag == "delete" {
            let selected = self.selected();
            let survivor = cfg
                .providers
                .iter()
                .find(|p| p.id == selected)
                .or_else(|| cfg.providers.first())
                .map(|p| p.id.as_str())
                .unwrap_or("");
            self.delete_feedback.committed(&self.cfg, &cfg, survivor);
        } else if matches!(
            tag,
            "save" | "test" | "install" | "add" | "duplicate" | "copy_prepare"
        ) {
            self.delete_feedback.clear();
        }
        self.delete_feedback.reconcile(&cfg);
        if self
            .drafts
            .pending
            .as_ref()
            .is_some_and(|p| p.created && !cfg.providers.iter().any(|saved| saved.id == p.id))
            && let Some(pending) = self.drafts.pending.take()
        {
            self.drafts.providers.remove(&pending.id);
            if self.drafts.selected == pending.id {
                self.drafts.selected = pending.return_to;
            }
        }
        let saved = self.selected();
        // update_config is reached only after the existing successful/committed
        // reply gate. Failed saves keep both the draft and its edit baseline.
        self.provider_edits.reconcile(
            &cfg,
            matches!(
                tag,
                "save" | "test" | "install" | "duplicate" | "copy_prepare"
            )
            .then_some(saved.as_str()),
        );
        self.saved_creations
            .retain(|id, _| cfg.providers.iter().any(|provider| provider.id == *id));
        let fresh = Drafts::from_config(&self.dir, &cfg);
        self.drafts.providers.retain(|id, _| {
            fresh.providers.contains_key(id)
                || self.drafts.pending.as_ref().is_some_and(|p| p.id == *id)
        });
        for (id, draft) in fresh.providers {
            if !self.drafts.providers.contains_key(&id)
                || ((tag == "save"
                    || tag == "test"
                    || tag == "install"
                    || tag == "duplicate"
                    || tag == "copy_prepare")
                    && id == saved)
            {
                self.drafts.providers.insert(id, draft);
            }
        }
        if tag == "add" || tag == "duplicate" {
            // A new draft must remain visible even when the old name filter
            // did not match it. Existing provider selection never clears it.
            self.provider_search
                .update(cx, |state, cx| state.set_value("", window, cx));
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
            "save" | "test" | "install" | "add" | "duplicate" | "delete" | "copy_prepare"
        ) {
            let selected = self.selection.rows.clone();
            let anchor = self.selection.anchor;
            let cursor = self.selection.cursor;
            self.load_form(window, cx);
            if matches!(tag, "save" | "test" | "install") {
                self.selection.rows = selected
                    .into_iter()
                    .filter(|i| *i < self.models.len())
                    .collect();
                self.selection.anchor = anchor.min(self.models.len().saturating_sub(1));
                self.selection.cursor = cursor.filter(|row| *row < self.models.len());
            }
        }
    }
    fn reconcile_failed_delete(
        &mut self,
        cfg: Config,
        snapshot: &Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !crate::config_refresh::matches_core(&self.cfg, &cfg, snapshot) {
            return;
        }
        self.capture(cx);
        let edits = &self.provider_edits;
        let reload =
            crate::config_refresh::refresh_drafts(&mut self.drafts, &self.cfg, &cfg, |id| {
                edits.contains(id)
            });
        self.provider_edits.reconcile(&cfg, None);
        self.saved_creations
            .retain(|id, _| cfg.providers.iter().any(|p| &p.id == id));
        self.delete_feedback.reconcile(&cfg);
        self.cfg = cfg;
        self.port.update(cx, |input, cx| {
            input.set_value(self.cfg.port.to_string(), window, cx)
        });
        if reload {
            self.load_form(window, cx);
        }
        // Keep the original error. A refreshed persisted view is not a success
        // receipt and must not send any save/delete/sync operation.
        cx.notify();
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
        while let Ok(mut reply) = self.backend.replies.try_recv() {
            let autostart_result = (reply.tag == "autostart").then(|| reply.result.is_ok());
            let update_result = (reply.tag == "check_update").then(|| reply.result.clone());
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
                        if reply.tag == "delete" {
                            self.delete_feedback.clear();
                        }
                        self.error = true;
                        self.status = crate::i18n::message(message, self.zh)
                            .lines()
                            .next()
                            .unwrap_or("")
                            .to_owned();
                    }
                }
                let creation_attempted = reply.creation.is_some();
                let creation_saved = reply
                    .creation
                    .map(|outcome| self.bind_created_draft(outcome))
                    .unwrap_or(false);
                if reply.result.is_ok()
                    || reply.synced
                    || creation_saved
                    || reply.saved_provider.is_some()
                {
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
                    if let Some(cfg) = reply.config.take() {
                        self.update_config(cfg, &reply.tag, window, cx);
                    }
                    if reply.tag == "copy_prepare" {
                        self.begin_provider_draft(true, window, cx);
                    }
                } else if creation_attempted && let Some(cfg) = reply.config.take() {
                    self.update_config(cfg, "creation_pending", window, cx);
                } else if reply.tag == "delete"
                    && let (Some(cfg), Some(snapshot)) =
                        (reply.config.take(), reply.snapshot.as_ref())
                {
                    self.reconcile_failed_delete(cfg, snapshot, window, cx);
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
            if let Some(success) = autostart_result {
                self.finish_autostart_notice(success, cx);
            }
            if let Some(result) = update_result {
                self.finish_update_notice(result, cx);
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
            && !self.port_open
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
        self.capture(cx);
        cx.notify();
    }
    fn select_models(&mut self, _: &SelectModels, _: &mut Window, cx: &mut Context<Self>) {
        self.selection.select_all(self.models.len());
        cx.notify();
    }
    fn edit_selected(&mut self, _: &EditModel, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(i) = self.selection.edit_row() {
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
        self.model_edit_error = None;
        self.model_editor_layout = None;
        self.model_editor_focused_row = 0;
        self.model_editor_scroll.set_offset(point(px(0.), px(0.)));
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
    pub(crate) fn model_id_for_options(&self, cx: &App) -> String {
        self.edit_model.read(cx).value().to_string()
    }
    fn confirm_edit(&mut self, cx: &mut Context<Self>) {
        if self.editing.is_none() {
            return;
        }
        let id = self.edit_model.read(cx).value().trim().to_owned();
        if id.is_empty() {
            self.model_edit_error = Some(self.t("请填写模型 ID", "Model ID is required").into());
            cx.notify();
            return;
        }
        let capabilities = match self.model_options.read(cx) {
            Ok(value) => value,
            Err(error) => {
                self.model_edit_error = Some(crate::i18n::message(&error, self.zh));
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
                self.model_edit_error =
                    Some(self.t("最多支持 256 个模型", "At most 256 models").into());
                cx.notify();
                return;
            }
            self.models.push(row);
        } else if let Some(i) = self.editing {
            self.models[i] = row;
        }
        self.editing = None;
        self.model_edit_error = None;
        self.error = false;
        self.capture(cx);
        self.status = self
            .t("模型已更新，保存后生效", "Models updated; save to apply")
            .into();
        cx.notify();
    }
    fn dismiss_model_input(
        &mut self,
        _: &DismissModelInput,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Resolve composition before closing the editor. The input action is
        // different from the dialog's Cancel action, so a plain Escape needs
        // an explicit route from inputs in this window-layer modal.
        let composing = [&self.edit_name, &self.edit_model]
            .into_iter()
            .chain(self.model_options.limits.iter())
            .any(|input| {
                input.update(cx, |state, cx| {
                    state.focus_handle(cx).is_focused(window)
                        && state
                            .marked_text_range(window, cx)
                            .is_some_and(|range| !range.is_empty())
                })
            });
        if composing {
            window.dispatch_action(Box::new(gpui_kit::component::input::Escape), cx);
        } else {
            self.dismiss(&Dismiss, window, cx);
        }
    }
    fn dismiss(&mut self, _: &Dismiss, window: &mut Window, cx: &mut Context<Self>) {
        self.picker = None;
        self.editing = None;
        self.model_edit_error = None;
        self.delete_open = false;
        self.provider_delete_target = None;
        self.sync_open = false;
        self.unsync_open = false;
        if self.startup_sync_open {
            self.startup_report = None;
        }
        self.startup_sync_open = false;
        if self.pending_port.take().is_some() || self.port_open {
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
        self.port_open = false;
        self.port_error = false;
        self.protocol_open = false;
        self.protocol_error = None;
        self.focus.focus(window, cx);
        cx.notify();
    }
    fn quit(&mut self, _: &Quit, _: &mut Window, cx: &mut Context<Self>) {
        self.capture(cx);
        let _ = self.drafts.persist(&self.dir);
        let _ = config::write_json(&self.dir.join("gpui-exit-host"), &true);
        cx.quit();
    }
    fn refresh_table(&self, window: &Window, cx: &mut Context<Self>) {
        let width = if self.table_width > px(0.) {
            self.table_width
        } else {
            // First frame, before the model table's own prepaint reports its
            // bounds: page padding + right rail + rail gap + card padding.
            let chrome = crate::theme::space::PAGE_X * 2.
                + relay::rail_width(window)
                + relay::COLUMN_GAP
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
            || self.port_open
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
            let is_provider_delete = self.delete_open;
            let is_protocol = self.protocol_open;
            let is_picker = self.picker.is_some();
            let is_model_editor = self.editing.is_some();
            let is_sync_confirmation = self.sync_open || self.unsync_open;
            let sync_zh = self.zh;
            let sync_remove = self.unsync_open;
            let width = if self.picker.is_some() || self.editing.is_some() {
                720.
            } else {
                480.
            };
            let focus = if self.protocol_open {
                // Focus a control rendered inside the protocol dialog; focusing
                // the window root would leave the modal focus trap inactive and
                // let Tab/Enter reach background controls (T09 X03).
                self.protocol_options.initial_focus(cx)
            } else if self.port_open {
                self.port.read(cx).focus_handle(cx)
            } else if self.picker.is_some() {
                self.search.read(cx).focus_handle(cx)
            } else if self.editing.is_some() {
                self.edit_model.read(cx).focus_handle(cx)
            } else {
                self.modal_focus.clone()
            };
            let first = std::cell::Cell::new(true);
            window.open_dialog(cx, move |dialog, window, cx| {
                if first.replace(false) {
                    focus.focus(window, cx);
                }
                let confirm_owner = owner.clone();
                let owner = owner.clone();
                let dialog = if is_provider_delete {
                    let content_body = body.clone();
                    dialog
                        .width(px(532.))
                        .min_h(px(322.))
                        .p(px(26.))
                        .pt(px(22.))
                        .pb(px(18.))
                        .bg(rgb(crate::theme::color::BG_BAR))
                        .rounded(px(18.))
                        .border_color(rgb(crate::theme::color::BORDER))
                        .margin_top(provider_delete::margin_top(window))
                        .content(move |content, _, _| content.gap_0().child(content_body.clone()))
                } else if is_protocol {
                    let height = protocol_dialog::surface_height(window);
                    let content_body = body.clone();
                    dialog
                        .width(px(crate::theme::size::DIALOG_PROTOCOL_WIDTH))
                        .h(height)
                        .p(px(25.))
                        .pt(px(20.))
                        .pb(px(14.))
                        .bg(rgb(crate::theme::color::BG_BAR))
                        .rounded(px(18.))
                        .border_color(rgb(crate::theme::color::BORDER))
                        .margin_top(((window.viewport_size().height - height) / 2.).max(px(16.)))
                        .content(move |content, _, _| content.gap_0().child(content_body.clone()))
                } else if is_sync_confirmation {
                    let content_body = body.clone();
                    dialog
                        .width(px(526.))
                        .min_h(px(318.))
                        .p(px(26.))
                        .pt(px(22.))
                        .pb(px(26.))
                        .bg(rgb(crate::theme::color::BG_BAR))
                        .rounded(px(18.))
                        .border_color(rgb(crate::theme::color::BORDER))
                        .margin_top(
                            ((window.viewport_size().height
                                - sync_dialog::estimated_height(sync_zh, sync_remove))
                                / 2.
                                - px(17.))
                            .max(px(16.)),
                        )
                        .content(move |content, _, _| content.gap_0().child(content_body.clone()))
                } else if is_model_editor {
                    let height = model_editor::surface_height(window);
                    let content_body = body.clone();
                    dialog
                        .width(px(728.))
                        .h(height)
                        .p(px(26.))
                        .pl(px(27.))
                        .pr(px(25.))
                        .pt(px(21.))
                        .pb(px(19.))
                        .bg(rgb(crate::theme::color::BG_BAR))
                        .rounded(px(18.))
                        .border_color(rgb(crate::theme::color::BORDER))
                        .margin_top(
                            ((window.viewport_size().height - height) / 2. - px(5.)).max(px(16.)),
                        )
                        .content(move |content, _, _| content.gap_0().child(content_body.clone()))
                } else if is_picker {
                    let height = model_picker::surface_height(window);
                    let content_body = body.clone();
                    dialog
                        .width(px(726.))
                        .h(height)
                        .p(px(25.))
                        .pt(px(20.))
                        .pb(px(15.))
                        .bg(rgb(0xffffff))
                        .rounded(px(18.))
                        .border_color(rgb(crate::theme::color::BORDER))
                        .margin_top(
                            ((window.viewport_size().height - height) / 2. - px(21.)).max(px(16.)),
                        )
                        .content(move |content, _, _| content.gap_0().child(content_body.clone()))
                } else {
                    dialog.w(px(width)).child(body.clone())
                };
                dialog
                    .close_button(true)
                    .overlay_closable(false)
                    .on_ok(move |_, window, cx| {
                        let _ = confirm_owner.update(cx, |this, cx| {
                            if this.port_open {
                                this.prepare_port(window, cx);
                            } else if this.protocol_open {
                                if !this
                                    .protocol_tab_focus
                                    .iter()
                                    .any(|focus| focus.contains_focused(window, cx))
                                {
                                    this.confirm_protocol(window, cx);
                                }
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
                crate::theme::color::SUCCESS,
                self.t("运行中", "Running"),
                crate::theme::color::SUCCESS,
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
                crate::theme::color::SUCCESS,
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

    fn dialog_body(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let content = if self.port_open {
            self.port_dialog(cx)
        } else if self.protocol_open {
            self.protocol_dialog(window, cx)
        } else if self.picker.is_some() {
            self.picker_dialog(window, cx)
        } else if self.editing.is_some() {
            self.model_editor_dialog(window, cx)
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
        } else if self.unsync_open || self.sync_open {
            self.sync_confirmation(cx)
        } else if self.delete_open {
            self.provider_delete_confirmation(window, cx)
        } else {
            return None;
        };
        Some(content)
    }
}
impl Render for Proxy {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.refresh_table(window, cx);
        let sidebar = self.sidebar(window, cx);
        let center = match self.page {
            Page::Providers => self.center(window, cx),
            Page::Statistics => self.statistics.clone().into_any_element(),
        };
        let body = if self.page == Page::Providers {
            let summary = (!self.selected().is_empty()).then(|| self.provider_summary(window, cx));
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_h_0()
                .gap(px(12.))
                .when_some(summary, |column, summary| column.child(summary))
                .child(
                    div()
                        .flex()
                        .flex_1()
                        .min_h_0()
                        .gap(px(relay::COLUMN_GAP))
                        .child(center)
                        .child(self.right(window, cx)),
                )
                .into_any_element()
        } else {
            center
        };
        self.manage_dialog(window, cx);
        let status_bar = self.status_bar();
        let title = match self.page {
            Page::Providers => self.t("连接与模型", "Connections & models"),
            Page::Statistics => self.statistics.read(cx).page_title(),
        };
        let subtitle = match self.page {
            Page::Providers => self.t(
                "管理上游协议、凭证和 WorkBuddy 可用模型",
                "Manage upstream protocols, credentials and WorkBuddy models",
            ),
            Page::Statistics => self.statistics.read(cx).page_subtitle(),
        };
        let heading = div()
            .flex()
            .flex_col()
            .flex_shrink_0()
            // Figma52/56/57: title27/39, subtitle13/19. Preserve the original
            // 97px heading flow so subtitle and report anchors stay in place.
            .gap(px(10.))
            .pb(px(29.))
            .child(
                div().flex().items_start().gap_4().child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_size(px(crate::theme::font::PAGE_TITLE))
                        .line_height(px(crate::theme::line_height::PAGE_TITLE))
                        .font_weight(FontWeight::BOLD)
                        .child(title),
                ),
            )
            .child(
                Label::new(subtitle)
                    .text_size(px(crate::theme::font::BODY))
                    .line_height(px(19.))
                    .text_color(rgb(crate::theme::color::TEXT_3)),
            );
        // Page headings keep their original anchors and total flow height. Actions are
        // positioned separately at the window root, below Kit's modal layers.
        let content = div()
            .relative()
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
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .flex_1()
                                .min_h_0()
                                .px(px(crate::theme::space::PAGE_X))
                                .pt(px(crate::theme::space::PAGE_TOP))
                                .pb(px(crate::theme::space::PAGE_BOTTOM))
                                .gap(px(crate::theme::space::SECTION_GAP))
                                .child(heading)
                                .child(body),
                        )
                        .child(status_bar),
                ),
            );
        div()
            .relative()
            .key_context("Proxy")
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::save))
            .on_action(cx.listener(Self::dismiss))
            .on_action(cx.listener(Self::quit))
            .size_full()
            .flex()
            .flex_col()
            .child(
                TitleBar::new()
                    .h(px(crate::theme::size::NATIVE_TITLEBAR))
                    .pl_0()
                    .bg(rgb(BG))
                    .border_b_0()
                    // Kit reserves macOS traffic lights on the left and adds
                    // native Windows min/max/close hit areas on the right.
                    .child(
                        div()
                            .w(px(crate::theme::size::SIDEBAR_WIDTH))
                            .flex_shrink_0()
                            .h_full()
                            .bg(rgb(crate::theme::color::SIDEBAR)),
                    )
                    .child(div().flex_1().min_w_0().h_full()),
            )
            .child(content.flex_1().min_h_0())
            .child(
                div()
                    .absolute()
                    .top(px(crate::theme::space::SYSTEM_TOOLBAR_TOP))
                    .right(px(crate::theme::space::PAGE_X))
                    .w(px(crate::theme::size::SYSTEM_TOOLBAR_WIDTH))
                    .h(px(crate::theme::size::TOP_ICON_ROW_HEIGHT))
                    .child(self.app_controls(cx)),
            )
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
            .update(cx, |this, cx| {
                this.dialog_body(window, cx).map(|body| {
                    div()
                        .id("proxy-modal-body")
                        .track_focus(&this.modal_focus)
                        .when(this.protocol_open || this.delete_open, |body| {
                            // The modal view lives in Kit's window layer, outside
                            // Proxy's dispatch tree. Its generic body focus needs
                            // a local dismissal route too, not only input focus.
                            body.key_context("ProxyModal")
                                .on_action(cx.listener(Proxy::dismiss))
                        })
                        .when(this.editing.is_some(), |body| {
                            body.key_context("ModelEditor")
                                .on_action(cx.listener(Proxy::dismiss_model_input))
                        })
                        .when(this.delete_open, |body| {
                            body.on_key_down(cx.listener(Proxy::scroll_provider_delete))
                        })
                        .child(body)
                        .into_any_element()
                })
            })
            .ok()
            .flatten()
            .unwrap_or_else(|| div().into_any_element())
    }
}
