//! Live import controls and reply dispatch. The display poll queries SQLite;
//! only the two explicit check buttons enqueue source work.
use super::*;

#[derive(Clone, serde::Deserialize)]
pub(super) struct Status {
    api_version: u32,
    root_found: bool,
    files_seen: i64,
    enabled: bool,
    effective_enabled: bool,
    settings_revision: i64,
    state: String,
    imported: i64,
    excluded_proxy: i64,
    pending: i64,
    conflict: i64,
    skipped: i64,
    deleted: i64,
    last_incremental_at_ms: Option<i64>,
    last_full_scan_at_ms: Option<i64>,
    last_error_code: Option<String>,
    scan_state: Option<String>,
    scan_mode: Option<String>,
    mapping_tasks_pending: i64,
    #[serde(default)]
    progress: Option<progress::Snapshot>,
}

pub(super) struct ImportUi {
    status: Option<Status>,
    write_seq: Option<u64>,
    toggle: bool,
    error: Option<String>,
    write_failed: bool,
    last_poll: Instant,
}

impl ImportUi {
    pub(super) fn new() -> Self {
        Self {
            status: None,
            write_seq: None,
            toggle: false,
            error: None,
            write_failed: false,
            last_poll: Instant::now(),
        }
    }
    fn running(&self) -> bool {
        self.status.as_ref().is_some_and(|status| {
            status.mapping_tasks_pending > 0
                || matches!(status.scan_state.as_deref(), Some("queued" | "running"))
                || matches!(
                    status.state.as_str(),
                    "discovering" | "backfilling" | "scanning"
                )
        })
    }
}

impl Statistics {
    pub(super) fn poll_import_status(&mut self, cx: &mut Context<Self>) {
        if self.visible
            && self.import_ui.last_poll.elapsed() >= Duration::from_secs(2)
            && !self.backend.is_busy(&JobKey::workbuddy_read("status"))
        {
            self.import_ui.last_poll = Instant::now();
            self.request_import_status(cx);
        }
    }

    fn apply_import_status(&mut self, result: &Result<Value, String>, _cx: &mut Context<Self>) {
        match result.clone().and_then(|value| {
            let status = serde_json::from_value::<Status>(value)
                .map_err(|_| "stats_unavailable".to_owned())?;
            if status.api_version != crate::stats_dto::API_VERSION
                || status.settings_revision <= 0
                || status
                    .progress
                    .as_ref()
                    .is_some_and(|progress| !progress.valid())
                || [
                    status.imported,
                    status.excluded_proxy,
                    status.pending,
                    status.conflict,
                    status.skipped,
                    status.deleted,
                    status.files_seen,
                ]
                .iter()
                .any(|value| *value < 0)
            {
                return Err("stats_unavailable".into());
            }
            Ok(status)
        }) {
            Ok(status) => {
                if self.import_ui.write_seq.is_none() {
                    self.import.apply(Ok(status.enabled));
                }
                self.import.set_counters(
                    crate::workbuddy_import::ImportCounters {
                        imported: status.imported,
                        excluded_proxy: status.excluded_proxy,
                        pending: status.pending,
                        conflict: status.conflict,
                        skipped: status.skipped,
                        deleted: status.deleted,
                        files_seen: status.files_seen,
                    },
                    crate::stats_dto::SourceScope::All,
                );
                if !self.import_ui.write_failed {
                    self.import_ui.error = status.last_error_code.clone();
                }
                self.import_ui.status = Some(status);
            }
            Err(error) => self.import_ui.error = Some(error),
        }
    }

    pub(super) fn handle_import_reply(
        &mut self,
        kind: JobKind,
        seq: u64,
        tag: &str,
        result: &Result<Value, String>,
        cx: &mut Context<Self>,
    ) -> bool {
        if kind == JobKind::WorkbuddyRead && tag.starts_with("import-") {
            if seq != self.import_seq || !self.visible {
                return true;
            }
            self.apply_import_status(result, cx);
            cx.notify();
            return true;
        }
        if kind != JobKind::WorkbuddyWrite {
            return false;
        }
        if self.import_ui.write_seq != Some(seq) {
            return true;
        }
        self.import_ui.write_seq = None;
        if self.import_ui.toggle {
            let response = result.clone().and_then(|value| {
                value
                    .get("enabled")
                    .and_then(Value::as_bool)
                    .ok_or("stats_unavailable".into())
            });
            self.import_ui.error = response.as_ref().err().cloned();
            self.import_ui.write_failed = response.is_err();
            self.import.apply(response);
        } else {
            self.import_ui.error = result.as_ref().err().cloned();
            self.import_ui.write_failed = result.is_err();
        }
        self.request_import_status(cx);
        self.request_settings(cx);
        if result.is_ok() {
            self.mark_dirty(cx);
        }
        cx.notify();
        true
    }

    fn import_write(&mut self, enabled: Option<bool>, full: bool, cx: &mut Context<Self>) {
        if self.import_ui.write_seq.is_some() {
            return;
        }
        let Some(status) = self.import_ui.status.as_ref() else {
            return;
        };
        let fields = if let Some(enabled) = enabled {
            let requested = if enabled {
                self.import.request_enable()
            } else {
                self.import.request_disable()
            };
            if requested.is_none() {
                return;
            }
            json!({"enabled":enabled,"expected_revision":status.settings_revision})
        } else {
            json!({"mode":if full {"full"} else {"incremental"}})
        };
        self.import_ui.write_failed = false;
        self.import_ui.error = None;
        self.import_ui.toggle = enabled.is_some();
        let command = if enabled.is_some() {
            "stats_workbuddy_settings_update"
        } else {
            "stats_workbuddy_scan"
        };
        let tag = format!("wb-write-{}", self.generation);
        let request = crate::stats_backend::command(command, &tag, fields);
        match self
            .backend
            .submit_write(JobKind::WorkbuddyWrite, tag, request)
        {
            Ok(seq) => self.import_ui.write_seq = Some(seq),
            Err(error) => {
                if enabled.is_some() {
                    self.import.apply(Err(error.clone()));
                }
                self.import_ui.error = Some(error);
                self.import_ui.write_failed = true;
            }
        }
        cx.notify();
    }

    fn import_status_line(&self) -> String {
        let status = self.import_ui.status.as_ref();
        let label = match status.map(|status| status.state.as_str()) {
            Some("disabled") => self.t(
                "已暂停导入，已有统计保留",
                "Import paused; statistics preserved",
            ),
            Some("paused") => self.t("全局采集已关闭", "Global collection is disabled"),
            Some("idle") => self.t("增量检查就绪", "Incremental check ready"),
            Some("discovering" | "backfilling" | "scanning") => self.t("处理中", "Processing"),
            Some(_) => self.t("需要检查", "Needs attention"),
            None => self.t("正在读取状态…", "Loading status…"),
        };
        let time = |at: Option<i64>| {
            at.and_then(|at| Local.timestamp_millis_opt(at).single())
                .map(|time| time.format("%m-%d %H:%M:%S").to_string())
                .unwrap_or_else(|| "—".into())
        };
        format!(
            "{} · {} {} · {} {}",
            label,
            self.t("最近增量", "Last incremental"),
            time(status.and_then(|status| status.last_incremental_at_ms)),
            self.t("最近完整核对", "Last full check"),
            time(status.and_then(|status| status.last_full_scan_at_ms))
        )
    }

    pub(super) fn import_controls(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let status = self.import_ui.status.as_ref();
        let saving = self.import_ui.write_seq.is_some();
        let active = status.is_some_and(|status| status.effective_enabled);
        let running = self.import_ui.running();
        let attention = self.import_ui.error.is_some()
            || status.is_some_and(|status| {
                status.pending > 0
                    || status.conflict > 0
                    || (status.enabled && !status.root_found)
                    || !matches!(
                        status.state.as_str(),
                        "idle" | "disabled" | "paused" | "discovering" | "backfilling" | "scanning"
                    )
            });
        let badge = if status.is_none() {
            self.t("读取中…", "Loading…").to_owned()
        } else if attention {
            self.t("需处理", "Needs attention").to_owned()
        } else if !active {
            self.t("已暂停", "Paused").to_owned()
        } else if running {
            self.t("正在同步", "Syncing").to_owned()
        } else {
            self.t("同步就绪", "Sync ready").to_owned()
        };
        let count = |index| {
            if status.is_some() {
                self.import.counters().value(index).to_string()
            } else {
                "—".to_owned()
            }
        };
        let metadata = status
            .map(|status| {
                let time = [status.last_incremental_at_ms, status.last_full_scan_at_ms]
                    .into_iter()
                    .flatten()
                    .max()
                    .and_then(|at| Local.timestamp_millis_opt(at).single())
                    .map(|date| date.format("%H:%M").to_string())
                    .unwrap_or_else(|| "—".into());
                let bytes = status
                    .progress
                    .as_ref()
                    .map(|p| p.committed_text())
                    .unwrap_or_else(|| "—".into());
                format!(
                    "{} {} · {bytes} · {} {time}",
                    status.files_seen,
                    self.t("个文件已发现", "files discovered"),
                    self.t("最近同步", "Last sync")
                )
            })
            .unwrap_or_else(|| {
                self.t("正在读取本地用量状态…", "Loading local usage status…")
                    .to_owned()
            });
        stats_card("workbuddy-import-controls")
            .anchor_scroll(Some(self.settings_anchors[2].clone()))
            .p(px(20.)).gap(px(10.)).min_w_0().min_h(px(292.))
            .child(div().flex().items_center().justify_between().flex_wrap().gap(px(8.))
                .child(Label::new(self.t("WorkBuddy 本地用量", "WorkBuddy local usage"))
                    .text_size(px(17.)).font_weight(FontWeight::BOLD).text_color(rgb(INK)))
                .child(div().flex().items_center().gap(px(10.))
                    .child(Checkbox::new("wb-import-enabled").large().label(self.t("导入", "Import"))
                        .checked(self.import.is_enabled()).disabled(status.is_none() || saving)
                        .on_change(cx.listener(|this, value: &bool, _, cx| this.import_write(Some(*value), false, cx))))
                    .child(super::settings_capture::status_badge(if attention { crate::ui_components::TagKind::Warning }
                        else if active { crate::ui_components::TagKind::Success } else { crate::ui_components::TagKind::Secondary }, badge))))
            .child(Label::new(self.t("每 5 秒增量同步内置与自定义用量", "Built-in and custom usage sync every 5 seconds"))
                .text_size(px(12.)).text_color(rgb(MUTED)))
            .child(div().flex().items_center().justify_between().gap(px(12.))
                .child(div().flex().items_end().gap(px(16.))
                    .child(Label::new(count(0)).text_size(px(32.)).font_weight(FontWeight::BOLD).text_color(rgb(INK)))
                    .child(Label::new(self.t("已入库记录", "Imported rows")).text_size(px(12.)).text_color(rgb(MUTED))))
                .child(super::settings_capture::status_badge(crate::ui_components::TagKind::Secondary,
                    format!("{} {}", count(2), self.t("待处理", "pending")))))
            .child(self.import_progress(true))
            .child(Label::new(metadata).w_full().text_ellipsis().text_size(px(12.)).text_color(rgb(MUTED)))
            .child(div().w_full().h(px(1.)).bg(rgb(BORDER)))
            .child(div().flex().items_center().gap(px(12.)).children([
                (0, self.t("已导入", "Imported")), (1, self.t("重复已去重", "Deduplicated")),
                (3, self.t("冲突", "Conflict")), (2, self.t("待重读", "Awaiting reread")),
            ].into_iter().map(|(index, label)| div().flex().flex_col().gap(px(4.)).flex_1().min_w_0()
                .child(Label::new(count(index)).text_size(px(18.)).font_weight(FontWeight::BOLD).text_color(rgb(INK)))
                .child(Label::new(label).text_size(px(11.)).text_color(rgb(MUTED))))))
            .child(div().flex().items_center().justify_end().flex_wrap().gap(px(8.))
                .child(self.import_details_control(cx))
                .child(stats_button("wb-check-updates").outline().h(px(32.)).label(self.t("检查更新", "Check updates"))
                    .disabled(status.is_none() || saving || !active || running)
                    .on_click(cx.listener(|this, _, _, cx| this.import_write(None, false, cx))))
                .child(stats_button("clear-preview").h(px(32.)).rounded(px(9.))
                    .custom(gpui_kit::component::button::ButtonCustomVariant::new(cx)
                        .color(rgb(0xfff0f0).into()).foreground(rgb(0xc94f5e).into())
                        .hover(rgb(0xffe2e7).into()).active(rgb(0xffd3dc).into()))
                    .label(self.t("清除统计…", "Clear history…"))
                    .tooltip(self.t("按当前时间截止预览全局已结束统计，保留去重标记和源文件", "Preview globally finished statistics before the current cutoff; keep deduplication markers and source files"))
                    .on_click(cx.listener(|this, _, _, cx| this.preview_clear(cx))))
                .child(self.import_more_actions(cx)))
            .when(self.import.counters().conflict > 0, |card| card.child(Label::new(self.t(
                "冲突记录不计入统计，请检查源文件后完整核对。", "Conflicts are excluded; review the source and run a full check."))
                .text_size(px(12.)).text_color(rgb(WARN))))
            .when_some(self.import_ui.error.clone(), |card, error| card.child(
                Label::new(crate::workbuddy_import::error_text(&error, self.zh)).text_size(px(12.)).text_color(rgb(WARN))))
    }

    fn import_details_control(&self, cx: &mut Context<Self>) -> AnyElement {
        let owner = cx.entity().downgrade();
        Popover::new("wb-import-details-popover")
            .trigger(stats_button("wb-import-details").ghost().h(px(32.)).label(self.t("详情", "Details")))
            .content(move |_, window, cx| owner.update(cx, |this, _| {
                let counters = crate::workbuddy_import::ImportCounters::ALL.iter().enumerate().map(|(index, (zh, en))|
                    Label::new(format!("{} {}", this.t(zh,en), if this.import_ui.status.is_some() {
                        this.import.counters().value(index).to_string() } else { "—".into() }))
                        .text_size(px(12.)).text_color(rgb(INK)));
                div().id("wb-import-details-panel").w(px((f32::from(window.viewport_size().width)-64.).min(560.)))
                    .max_h(px((f32::from(window.viewport_size().height)-160.).max(220.)))
                    .overflow_y_scroll().flex().flex_col().gap(px(12.))
                    .child(Label::new(this.t("导入状态与去重", "Import status & deduplication"))
                        .text_size(px(16.)).font_weight(FontWeight::BOLD).text_color(rgb(INK)))
                    .child(Label::new(this.t("同次调用去重并优先代理明细；页面刷新关闭不影响后台同步。", "Exact duplicates prefer proxy details; page refresh does not control background sync."))
                        .text_size(px(12.)).text_color(rgb(MUTED)))
                    .child(Label::new(this.import_status_line()).text_size(px(12.)).text_color(rgb(MUTED)))
                    .child(this.import_progress(false))
                    .child(div().flex().flex_wrap().gap(px(16.)).children(counters))
                    .into_any_element()
            }).unwrap_or_else(|_| div().into_any_element()))
            .into_any_element()
    }

    fn import_more_actions(&self, cx: &mut Context<Self>) -> AnyElement {
        use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
        let owner = cx.entity().downgrade();
        let disabled = self.import_ui.status.is_none()
            || self.import_ui.write_seq.is_some()
            || self.import_ui.running()
            || !self
                .import_ui
                .status
                .as_ref()
                .is_some_and(|s| s.effective_enabled);
        let zh = self.zh;
        stats_button("wb-import-more")
            .ghost()
            .h(px(28.))
            .w(px(28.))
            .p_0()
            .label("⋯")
            .accessibility_label(self.t("本地用量操作", "Local usage actions"))
            .dropdown_menu_with_anchor(Anchor::TopRight, move |mut menu, _, _| {
                for (full, label) in [
                    (true, if zh { "完整核对" } else { "Full check" }),
                    (
                        false,
                        if zh {
                            "自动分类与去重说明"
                        } else {
                            "Classification and deduplication"
                        },
                    ),
                ] {
                    let owner = owner.clone();
                    menu = menu.item(
                        PopupMenuItem::element(move |_, _| Label::new(label).text_size(px(13.)))
                            .disabled(full && disabled)
                            .on_click(move |_, window, cx| {
                                let _ = owner.update(cx, |this, cx| {
                                    if full {
                                        this.import_write(None, true, cx)
                                    } else {
                                        this.open_mapping_dialog(window, cx)
                                    }
                                });
                            }),
                    );
                }
                menu.min_w(px(240.))
            })
            .into_any_element()
    }
}

mod notices;
mod progress;
