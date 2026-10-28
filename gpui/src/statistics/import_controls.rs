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

    fn import_actions(&self, cx: &mut Context<Self>) -> AnyElement {
        let saving = self.import_ui.write_seq.is_some();
        let disabled = self.import_ui.status.is_none() || saving;
        let running = self.import_ui.running();
        let active = self
            .import_ui
            .status
            .as_ref()
            .is_some_and(|status| status.effective_enabled);
        let buttons = [false, true].into_iter().map(|full| {
            let label = if running {
                self.t("处理中", "Processing")
            } else if full {
                self.t("完整核对", "Full check")
            } else {
                self.t("检查更新", "Check updates")
            };
            let tip = if full {
                self.t(
                    "重新检查全部本地记录，不会恢复已清除条目",
                    "Recheck local records without restoring cleared entries",
                )
            } else {
                self.t("检查本地新增记录", "Check newly saved local records")
            };
            stats_button(("wb-check", full as usize))
                .h(px(28.))
                .outline()
                .label(label)
                .tooltip(tip)
                .disabled(disabled || !active || running)
                .on_click(cx.listener(move |this, _, _, cx| this.import_write(None, full, cx)))
        });
        let toggle = Checkbox::new("wb-import-enabled")
            .large()
            .label(self.t("导入本机 WorkBuddy 用量", "Import local WorkBuddy usage"))
            .checked(self.import.is_enabled())
            .disabled(disabled)
            .on_change(
                cx.listener(|this, value: &bool, _, cx| this.import_write(Some(*value), false, cx)),
            );
        div()
            .flex()
            .flex_wrap()
            .items_center()
            .justify_between()
            .gap_2()
            .child(toggle)
            .child(div().flex().gap_2().children(buttons))
            .into_any_element()
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

    pub(super) fn import_controls(&self, cx: &mut Context<Self>) -> AnyElement {
        let note=self.t("每5秒后台增量同步内置与自定义模型；同次调用去重并优先本机代理明细。页面刷新关闭不影响同步。",
            "Built-in and custom records sync every 5 seconds. Exact duplicates prefer native proxy details. Page refresh does not control sync.");
        let counters = crate::workbuddy_import::ImportCounters::ALL
            .iter()
            .enumerate()
            .map(|(index, (zh, en))| {
                Label::new(format!(
                    "{} {}",
                    self.t(zh, en),
                    self.import.counters().value(index)
                ))
                .text_size(px(crate::theme::font::LABEL))
            });
        div()
            .id("workbuddy-import-controls")
            .anchor_scroll(Some(self.settings_anchors[2].clone()))
            .flex()
            .flex_col()
            .w_full()
            .gap_2()
            .pt_3()
            .border_t_1()
            .border_color(rgb(BORDER))
            .child(stats_heading(
                self.t("WorkBuddy 本地用量", "WorkBuddy local usage"),
            ))
            .child(self.import_actions(cx))
            .child(Label::new(note).text_size(px(crate::theme::font::LABEL)).text_color(rgb(MUTED)))
            .child(
                Label::new(self.import_status_line())
                    .text_size(px(crate::theme::font::LABEL))
                    .text_color(rgb(MUTED)),
            )
            .child(self.import_progress())
            .child(div().flex().flex_wrap().gap_3().children(counters))
            .child(stats_button("wb-import-diagnostics").ghost()
                .label(self.t("自动分类与去重说明","Classification and deduplication"))
                .on_click(cx.listener(|this,_,window,cx|this.open_mapping_dialog(window,cx))))
            .when(self.import.counters().conflict>0,|this|this.child(Label::new(self.t("同一记录的内容发生变化，冲突记录不计入统计，请检查源文件后完整核对。","Conflicting record content is excluded and requires a full check after reviewing source files.")).text_size(px(crate::theme::font::LABEL)).text_color(rgb(WARN))))
            .when_some(self.import_ui.error.clone(), |this, error| {
                this.child(
                    Label::new(crate::workbuddy_import::error_text(&error, self.zh))
                        .text_size(px(crate::theme::font::LABEL))
                        .text_color(rgb(WARN)),
                )
            })
            .into_any_element()
    }
}

mod notices;
mod progress;
