use crate::{
    backend::{Backend, Job},
    config::{self, Config},
    state::{Draft, Drafts, Selection},
};
use gpui_kit::component::{
    Disableable, ElementExt, IconName, Sizable, TitleBar, WindowExt,
    button::{Button, ButtonVariants},
    checkbox::Checkbox,
    dialog::{DialogContent, DialogFooter},
    input::{Input, InputEvent, InputState, Textarea, TextareaState},
    label::Label,
    list::ListItem,
    select::{Select, SelectEvent, SelectItem, SelectState},
    sidebar::{Sidebar, SidebarHeader, SidebarMenu, SidebarMenuItem},
    table::{DataTable, TableState},
    tag::Tag,
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

const BG: u32 = 0xf4f7fb;
const INK: u32 = 0x19334d;
const MUTED: u32 = 0x6b7c90;
const BORDER: u32 = 0xe2e9f1;
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
}
type ChoiceState = SelectState<Vec<Choice>>;
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
pub struct Proxy {
    dir: PathBuf,
    cfg: Config,
    drafts: Drafts,
    backend: Backend,
    zh: bool,
    busy: bool,
    polling: bool,
    status: String,
    error: bool,
    name: Entity<InputState>,
    key: Entity<InputState>,
    url: Entity<InputState>,
    port: Entity<InputState>,
    search: Entity<InputState>,
    edit_name: Entity<InputState>,
    edit_model: Entity<InputState>,
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
    focus: FocusHandle,
    picker: Option<Vec<String>>,
    picked: BTreeSet<usize>,
    only_picked: bool,
    editing: Option<usize>,
    new_model: bool,
    language: Entity<ChoiceState>,
    effort_select: Entity<ChoiceState>,
    delete_open: bool,
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
fn label(s: impl Into<SharedString>) -> Label {
    Label::new(s)
        .text_size(px(12.))
        .text_color(rgb(MUTED))
        .mb_2()
}
// Layout surfaces are not interactive controls. GroupBox adds nested intrinsic-
// height wrappers, so flexible table/log panels use GPUI layout containers.
fn card(id: &'static str) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .flex_col()
        .min_h_0()
        .h_full()
        .bg(rgb(0xffffff))
        .border_1()
        .border_color(rgb(BORDER))
        .rounded(px(12.))
        .p_4()
        .gap_3()
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
                    "填写供应商提供的 OpenAI 兼容接口地址"
                } else {
                    "Enter your provider's OpenAI-compatible API URL"
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
        self.refresh_choices(code, window, cx);
        self.send(
            "language",
            vec![json!({"command":"language", "language":code})],
            cx,
        );
    }
    fn refresh_choices(&mut self, code: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.language.update(cx, |s, cx| {
            s.set_items(choices(self.zh, true), window, cx);
            s.set_selected_value(&code.to_owned(), window, cx);
        });
        self.effort_select.update(cx, |s, cx| {
            s.set_items(choices(self.zh, false), window, cx);
            s.set_selected_value(&self.effort, window, cx);
        });
    }
    fn language_menu(&self) -> AnyElement {
        Select::new(&self.language)
            .id("language")
            .small()
            .w(px(144.))
            .menu_width(px(180.))
            .accessibility_label(self.t("界面语言", "Interface language"))
            .disabled(self.busy)
            .into_any_element()
    }
    fn effort_menu(&self) -> AnyElement {
        Select::new(&self.effort_select)
            .id("effort")
            .small()
            .w(px(220.))
            .menu_width(px(240.))
            .accessibility_label(self.t("思考强度", "Thinking effort"))
            .disabled(self.busy)
            .into_any_element()
    }
    fn t(&self, zh: &'static str, en: &'static str) -> &'static str {
        if self.zh { zh } else { en }
    }
    pub fn new(dir: PathBuf, cfg: Config, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let zh = cfg.language == "zh"
            || (cfg.language == "auto"
                && sys_locale::get_locale().is_some_and(|s| s.starts_with("zh")));
        let drafts = Drafts::load(&dir, &cfg);
        let backend = Backend::new(dir.clone());
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
                "填写供应商提供的 OpenAI 兼容接口地址"
            } else {
                "Enter your provider's OpenAI-compatible API URL"
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
        let language = cx.new(|cx| SelectState::new(choices(zh, true), None, window, cx));
        language.update(cx, |s, cx| s.set_selected_value(&cfg.language, window, cx));
        let effort_select = cx.new(|cx| SelectState::new(choices(zh, false), None, window, cx));
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
            cfg,
            drafts,
            backend,
            zh,
            busy: false,
            polling: false,
            status: if zh { "就绪" } else { "Ready" }.into(),
            error: false,
            name,
            key,
            url,
            port,
            search,
            edit_name,
            edit_model,
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
            focus,
            picker: None,
            picked: BTreeSet::new(),
            only_picked: false,
            editing: None,
            new_model: false,
            language,
            effort_select,
            delete_open: false,
            wake: String::new(),
            last_poll: Instant::now() - Duration::from_secs(10),
            last_dirty: String::new(),
            _timer: timer,
        };
        cx.subscribe_in(&this.language, window, |this, _, event, window, cx| {
            if let SelectEvent::Confirm(Some(code)) = event {
                this.set_language(code, window, cx);
            }
        })
        .detach();
        cx.subscribe(&this.effort_select, |this, _, event, cx| {
            if let SelectEvent::Confirm(Some(code)) = event {
                this.effort = code.clone();
                cx.notify();
            }
        })
        .detach();
        this.load_form(window, cx);
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
            &json!({"pid":std::process::id(),"preview":1}),
        );
        this
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
        self.models = draft.models;
        self.effort = if draft.effort.is_empty() {
            "keep".into()
        } else {
            draft.effort
        };
        self.effort_select
            .update(cx, |s, cx| s.set_selected_value(&self.effort, window, cx));
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
        if self.picker.is_some() || self.editing.is_some() || self.delete_open {
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
    fn sync(&mut self, cx: &mut Context<Self>) {
        let mut steps = if self.selected().is_empty() {
            vec![]
        } else {
            self.dirty_steps(cx)
        };
        steps.push(json!({"command":"install"}));
        self.send("install", steps, cx);
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
                let offset = s.scroll_offset();
                s.set_value(self.log_text.clone(), window, cx);
                s.set_scroll_offset(offset, cx);
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
        let wake = std::fs::read_to_string(self.dir.join("gpui-wake.json")).unwrap_or_default();
        if wake != self.wake {
            self.wake = wake;
            window.activate_window();
            cx.activate(true);
        }
        while let Ok(reply) = self.backend.replies.try_recv() {
            if reply.tag == "poll" {
                self.polling = false;
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
                                        self.t("本地端口已更新：", "Local port updated:"),
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
                if reply.result.is_ok() {
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
                let _ = self.drafts.persist(&self.dir);
                cx.notify();
            }
            if let Some(snapshot) = reply.snapshot {
                self.snapshot(snapshot, window, cx);
            }
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
        self.edit_model.read(cx).focus_handle(cx).focus(window, cx);
        cx.notify();
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
        let mut row = self
            .editing
            .and_then(|i| self.models.get(i).cloned())
            .unwrap_or(json!({}));
        row["model"] = json!(id);
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
            window.viewport_size().width - px(616.)
        };
        let column_width = (width / 2.).max(px(120.));
        let data = self.model_table.read(cx).delegate();
        if data.rows == self.models
            && data.selection == self.selection
            && data.zh == self.zh
            && data.column_width == column_width
        {
            return;
        }
        self.model_table.update(cx, |table, cx| {
            let data = table.delegate_mut();
            data.rows = self.models.clone();
            data.selection = self.selection.clone();
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
        let active = self.picker.is_some() || self.editing.is_some() || self.delete_open;
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
            let width = if self.picker.is_some() { 720. } else { 480. };
            let focus = if self.picker.is_some() {
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
                    .on_ok(move |_, _, cx| {
                        let _ = confirm_owner.update(cx, |this, cx| {
                            if this.editing.is_some() {
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
    fn sidebar(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let selected = self.selected();
        let rows = self
            .cfg
            .providers
            .iter()
            .map(|p| {
                let id = p.id.clone();
                let active = id == selected;
                let draft = self.drafts.providers.get(&id);
                let name = draft
                    .map(|d| d.name.clone())
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| p.name.clone());
                let count = draft.map_or(p.models.len(), |d| d.models.len());
                SidebarMenuItem::new(name.clone())
                    .accessibility_label(name)
                    .active(active)
                    .font_weight(if active {
                        FontWeight::SEMIBOLD
                    } else {
                        FontWeight::NORMAL
                    })
                    .disable(self.busy)
                    .suffix(move |_, _| Tag::secondary().small().child(count.to_string()))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.switch_provider(id.clone(), window, cx)
                    }))
            })
            .collect::<Vec<_>>();
        Sidebar::new("providers")
            .collapsible(false)
            .w(px(196.))
            .border_1()
            .border_color(rgb(0xcbdbe6))
            .rounded(px(12.))
            .header(
                SidebarHeader::new().child(
                    Label::new(self.t("供应商", "Providers")).font_weight(FontWeight::SEMIBOLD),
                ),
            )
            .child(SidebarMenu::new().children(rows))
            .footer(
                div()
                    .w_full()
                    .p_2()
                    .border_t_1()
                    .border_color(rgb(BORDER))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap_2()
                            .child(
                                Button::new("provider-add")
                                    .small()
                                    .ghost()
                                    .icon(IconName::Plus)
                                    .tooltip(self.t("新增供应商", "Add provider"))
                                    .accessibility_label(self.t("新增供应商", "Add provider"))
                                    .disabled(self.busy)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.capture(cx);
                                        this.send("add", vec![json!({"command":"add"})], cx);
                                    })),
                            )
                            .child(
                                Button::new("provider-copy")
                                    .small()
                                    .ghost()
                                    .icon(IconName::Copy)
                                    .tooltip(self.t("复制供应商", "Duplicate provider"))
                                    .accessibility_label(self.t("复制供应商", "Duplicate provider"))
                                    .disabled(self.busy || selected.is_empty())
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        let mut steps = this.dirty_steps(cx);
                                        steps.push(
                                    json!({"command":"duplicate","provider_id":this.selected()}),
                                );
                                        this.send("duplicate", steps, cx);
                                    })),
                            )
                            .child(
                                Button::new("provider-delete")
                                    .small()
                                    .ghost()
                                    .icon(IconName::Delete)
                                    .tooltip(self.t("删除供应商", "Delete provider"))
                                    .accessibility_label(self.t("删除供应商", "Delete provider"))
                                    .disabled(self.busy || selected.is_empty())
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.delete_open = true;
                                        cx.notify();
                                    })),
                            ),
                    ),
            )
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
        card("provider-settings").flex_1().min_w_0().gap_4().child(div().flex().items_center().justify_between().child(div().text_size(px(15.)).font_weight(FontWeight::SEMIBOLD).child(self.t("连接设置","Connection settings"))).child(div().text_size(px(11.)).text_color(rgb(MUTED)).child(self.t("修改后保存","Save changes to apply"))))
        .child(div().flex().gap_3().child(div().w_2_5().child(label(self.t("供应商名称","Provider name"))).child(Input::new(&self.name).disabled(self.busy))).child(div().flex_1().child(label("API Key")).child(Input::new(&self.key).mask_toggle().disabled(self.busy))))
        .child(div().child(label(self.t("API 地址","API URL"))).child(Input::new(&self.url).disabled(self.busy)))
        .child(div().flex().items_center().justify_between().pt_1().child(div().font_weight(FontWeight::SEMIBOLD).child(self.t("模型列表","Models"))).child(div().flex().gap_2().child(Button::new("fetch").small().primary().outline().icon(IconName::RefreshCw).label(self.t("获取模型","Fetch models")).disabled(self.busy).on_click(cx.listener(|this,_,_,cx|{this.send("fetch",vec![json!({"command":"fetch","provider_id":this.selected(),"upstream":this.url.read(cx).value().to_string(),"key":this.key.read(cx).value().to_string()})],cx);})))
        .child(Button::new("model-add").small().ghost().icon(IconName::Plus).tooltip(self.t("添加模型","Add model")).accessibility_label(self.t("添加模型","Add model")).disabled(self.busy).on_click(cx.listener(|this,_,window,cx|this.edit(this.models.len(),true,window,cx))))
        .child(Button::new("model-remove").small().ghost().icon(IconName::Minus).tooltip(self.t("删除模型","Remove models")).accessibility_label(self.t("删除模型","Remove models")).disabled(self.busy||self.selection.rows.is_empty()).on_click(cx.listener(|this,_,window,cx|this.remove_models(&RemoveModels,window,cx))))))
        .child(div().key_context("ModelList").track_focus(&self.table_focus).flex_1().min_h(px(140.)).on_action(cx.listener(Self::remove_models)).on_action(cx.listener(Self::select_models)).on_action(cx.listener(Self::edit_selected)).on_prepaint({
            let owner = cx.entity().downgrade();
            move |bounds, _, cx| {
                let width = (bounds.size.width - px(2.)).max(px(240.));
                let _ = owner.update(cx, |this, cx| {
                    if this.table_width != width { this.table_width = width; cx.notify(); }
                });
            }
        }).child(DataTable::new(&self.model_table).small().header_row_height(px(32.)).scrollbar_visible(true,true)))
        .child(div().text_size(px(11.)).text_color(rgb(MUTED)).child(self.t("Shift 连续多选 · Ctrl / ⌘ 多选 · 删除键移除 · 双击编辑","Shift range · Ctrl / ⌘ toggle · Delete to remove · Double-click to edit")))
        .child(div().flex().items_center().gap_3().child(div().text_size(px(12.)).text_color(rgb(MUTED)).child(self.t("思考强度","Thinking effort"))).child(self.effort_menu()))
        .child(div().flex().gap_2().child(Button::new("save").small().primary().icon(IconName::Check).label(self.t("保存","Save")).disabled(self.busy).on_click(cx.listener(|this,_,window,cx|this.save(&Save,window,cx))))
        .child(Button::new("test").small().primary().outline().label(self.t("测试连接","Test connection")).disabled(self.busy||self.models.is_empty()).on_click(cx.listener(|this,_,_,cx|this.test(cx))))).into_any_element()
    }
    fn right(&mut self, cx: &mut Context<Self>) -> AnyElement {
        card("global-settings")
            .w(px(322.))
            .flex_shrink_0()
            .gap_3()
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_size(px(15.))
                    .child(self.t("全局设置与日志", "Settings & logs")),
            )
            .when(self.error && self.editing.is_none(), |this| {
                this.child(
                    Label::new(self.status.clone())
                        .text_size(px(11.))
                        .text_color(rgb(0xca4960)),
                )
            })
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(
                        Button::new("sync")
                            .small()
                            .primary()
                            .icon(IconName::RefreshCw)
                            .label(self.t("同步 WorkBuddy", "Sync WorkBuddy"))
                            .disabled(self.busy)
                            .on_click(cx.listener(|this, _, _, cx| this.sync(cx))),
                    )
                    .child(
                        Button::new("restore")
                            .small()
                            .ghost()
                            .icon(IconName::Delete)
                            .label(self.t("取消同步", "Unsync"))
                            .tooltip(self.t(
                                "移除代理同步到 WorkBuddy 的模型",
                                "Remove proxy models synced to WorkBuddy",
                            ))
                            .disabled(self.busy || self.cfg.installed.is_empty())
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.send("restore", vec![json!({"command":"restore"})], cx)
                            })),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .p_3()
                    .rounded(px(9.))
                    .bg(rgb(0xf0f7fa))
                    .border_1()
                    .border_color(rgb(BORDER))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                Label::new(self.t("本地代理", "Local relay"))
                                    .text_size(px(12.))
                                    .font_weight(FontWeight::SEMIBOLD),
                            )
                            .child(Tag::secondary().small().child(self.t("运行中", "Running"))),
                    )
                    .child(
                        Label::new(self.proxy_address.clone())
                            .text_size(px(12.))
                            .text_color(rgb(0x007e99)),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .w(px(60.))
                                    .text_size(px(12.))
                                    .text_color(rgb(MUTED))
                                    .child(self.t("本地端口", "Local port")),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .child(Input::new(&self.port).small().disabled(self.busy)),
                            )
                            .child(
                                Button::new("port-apply")
                                    .small()
                                    .ghost()
                                    .icon(IconName::Check)
                                    .tooltip(self.t("应用端口", "Apply port"))
                                    .accessibility_label(self.t("应用端口", "Apply port"))
                                    .disabled(self.busy)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        match this
                                            .port
                                            .read(cx)
                                            .value()
                                            .parse::<u16>()
                                            .ok()
                                            .filter(|p| *p > 0)
                                        {
                                            Some(port) => this.send(
                                                "port",
                                                vec![json!({"command":"set_port","port":port})],
                                                cx,
                                            ),
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
                        div().flex().gap_3().children(
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
                                    .gap_1()
                                    .child(
                                        Label::new(count.to_string())
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_size(px(15.)),
                                    )
                                    .child(
                                        Label::new(name).text_color(rgb(MUTED)).text_size(px(10.)),
                                    )
                            }),
                        ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(self.t("日志", "Logs")),
                    )
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(
                                Button::new("clear-logs")
                                    .small()
                                    .ghost()
                                    .icon(IconName::Delete)
                                    .tooltip(self.t("清空日志", "Clear logs"))
                                    .accessibility_label(self.t("清空日志", "Clear logs"))
                                    .disabled(self.busy)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.send(
                                            "clear_logs",
                                            vec![json!({"command":"clear_logs"})],
                                            cx,
                                        )
                                    })),
                            )
                            .child(
                                Button::new("copy-logs")
                                    .small()
                                    .ghost()
                                    .icon(IconName::Copy)
                                    .tooltip(self.t("复制日志", "Copy logs"))
                                    .accessibility_label(self.t("复制日志", "Copy logs"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        cx.write_to_clipboard(ClipboardItem::new_string(format!(
                                            "{}\n\n{}",
                                            this.summary, this.log_text
                                        )));
                                        this.status = this.t("日志已复制", "Logs copied").into();
                                        this.error = false;
                                        cx.notify();
                                    })),
                            ),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .bg(rgb(0xf8fafc))
                    .rounded(px(9.))
                    .text_size(px(11.))
                    .child(
                        Textarea::new(&self.logs)
                            .readonly(true)
                            .text_size(px(11.))
                            .line_height(px(16.))
                            .h_full(),
                    ),
            )
            .into_any_element()
    }
    fn dialog_body(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let content = if self.picker.is_some() {
            self.picker_dialog(cx)
        } else if self.editing.is_some() {
            DialogContent::new()
                .w_full()
                .gap_4()
                .child(
                    div()
                        .text_lg()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(if self.new_model {
                            self.t("添加模型", "Add model")
                        } else {
                            self.t("编辑模型", "Edit model")
                        }),
                )
                .child(
                    div()
                        .child(label(self.t("显示名", "Display name")))
                        .child(Input::new(&self.edit_name)),
                )
                .child(
                    div()
                        .child(label(self.t("模型 ID", "Model ID")))
                        .child(Input::new(&self.edit_model)),
                )
                .when(self.error, |this| {
                    this.child(
                        Label::new(self.status.clone())
                            .text_color(rgb(0xc03c45))
                            .text_size(px(12.)),
                    )
                })
                .child(
                    DialogFooter::new()
                        .child(
                            Button::new("edit-cancel")
                                .label(self.t("取消", "Cancel"))
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.dismiss(&Dismiss, window, cx)
                                })),
                        )
                        .child(
                            Button::new("edit-confirm")
                                .primary()
                                .label(self.t("确认", "Confirm"))
                                .on_click(cx.listener(|this, _, _, cx| this.confirm_edit(cx))),
                        ),
                )
                .into_any_element()
        } else if self.delete_open {
            DialogContent::new().w_full().gap_4().child(div().text_lg().font_weight(FontWeight::SEMIBOLD).child(self.t("删除供应商？","Delete provider?"))).child(div().text_color(rgb(MUTED)).child(self.t("将移除此供应商及其保存的 Key 和模型。","This removes the provider and its saved key and models.")))
            .child(DialogFooter::new().child(Button::new("delete-cancel").label(self.t("取消","Cancel")).on_click(cx.listener(|this,_,window,cx|this.dismiss(&Dismiss,window,cx)))).child(Button::new("delete-confirm").danger().label(self.t("删除","Delete")).on_click(cx.listener(|this,_,_,cx|{this.delete_open=false;this.send("delete",vec![json!({"command":"delete","provider_id":this.selected()})],cx);})))) .into_any_element()
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
        DialogContent::new()
            .w_full()
            .h(px(550.))
            .gap_3()
            .child(
                div()
                    .flex()
                    .justify_between()
                    .items_center()
                    .child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(self.t("选择模型", "Select models")),
                    )
                    .child(
                        div()
                            .text_color(rgb(MUTED))
                            .text_size(px(11.))
                            .child(format!(
                                "{count} / {total}  ·  {chosen} {}",
                                self.t("待添加", "to add")
                            )),
                    ),
            )
            .child(Input::new(&self.search))
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(
                        Button::new("picker-select-all")
                            .small()
                            .label(self.t("全选当前筛选", "Select filtered"))
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
                        Button::new("picker-invert")
                            .small()
                            .label(self.t("反选当前筛选", "Invert filtered"))
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
                        Button::new("picker-clear")
                            .small()
                            .label(self.t("清空选择", "Clear selection"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.picked.clear();
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("picker-only")
                            .small()
                            .label(self.t("仅看已勾选", "Selected only"))
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
                    .border_color(rgb(BORDER))
                    .rounded(px(8.))
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
                                            .h(px(36.))
                                            .accessibility_label(id.clone())
                                            .child(
                                                Checkbox::new(("candidate-check", i))
                                                    .accessibility_id(format!("candidate-{i}"))
                                                    .label(id)
                                                    .checked(checked)
                                                    .disabled(existing)
                                                    .small()
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
                                                .text_size(px(11.))
                                            })
                                            .into_any_element()
                                    })
                                    .collect()
                            }),
                        )
                        .size_full(),
                    ),
            )
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(rgb(MUTED))
                    .child(self.t(
                        "已有模型保留，确认添加后仍需保存。",
                        "Existing models are kept. Save after adding your selection.",
                    )),
            )
            .child(
                DialogFooter::new()
                    .child(
                        Button::new("picker-cancel")
                            .label(self.t("取消", "Cancel"))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.dismiss(&Dismiss, window, cx)
                            })),
                    )
                    .child(
                        Button::new("picker-confirm")
                            .primary()
                            .label(format!("{} ({chosen})", self.t("确认添加", "Add selected")))
                            .disabled(chosen == 0)
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
        let center = self.center(cx);
        let right = self.right(cx);
        self.manage_dialog(window, cx);
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
            .px_3()
            .pt_2()
            .pb_1()
            .gap_1()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .h(px(48.))
                    .mb_2()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .child(img("brand.png").size(px(36.)))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .child(
                                        div()
                                            .text_size(px(22.))
                                            .font_weight(FontWeight::BOLD)
                                            .child("WorkBuddy Proxy"),
                                    )
                                    .child(div().text_size(px(11.)).text_color(rgb(MUTED)).child(
                                        self.t(
                                            "轻量转发 · 多模型管理",
                                            "Lightweight relay · Multiple models",
                                        ),
                                    )),
                            )
                            .child(Tag::info().small().child("GPUI Kit Preview 04")),
                    )
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(
                                Button::new("check-update")
                                    .small()
                                    .ghost()
                                    .icon(IconName::RefreshCw)
                                    .label(self.t("检查更新", "Check updates"))
                                    .disabled(self.busy)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.send(
                                            "check_update",
                                            vec![json!({"command":"check_update","force":true})],
                                            cx,
                                        )
                                    })),
                            )
                            .child(self.language_menu()),
                    ),
            )
            .child(
                div()
                    .flex()
                    .gap_3()
                    .flex_1()
                    .min_h_0()
                    .child(sidebar)
                    .child(center)
                    .child(right),
            )
            .child(
                div().flex().justify_end().h(px(16.)).items_center().child(
                    Label::new(self.t(
                        "关闭窗口后，代理仍在托盘运行",
                        "Close this window to keep the relay running in the tray",
                    ))
                    .text_size(px(10.))
                    .text_color(rgb(MUTED)),
                ),
            );
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                TitleBar::new()
                    .child(Label::new("WorkBuddy Proxy · GPUI Preview").text_size(px(12.))),
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
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.owner
            .update(cx, |this, cx| this.dialog_body(cx))
            .ok()
            .flatten()
            .unwrap_or_else(|| div().into_any_element())
    }
}
