//! SQLite-only status and bounded, actionable import notices.
use super::*;

#[derive(Clone, Copy)]
enum Notice {
    Error,
    Pending,
    Conflict,
    Missing,
    Disabled,
}

impl Statistics {
    pub(in crate::statistics) fn import_health_text(&self) -> String {
        let Some(status) = self.import_ui.status.as_ref() else {
            return self.t("WorkBuddy：正在读取", "WorkBuddy: loading").into();
        };
        let label = if self.import_ui.error.is_some() || status.pending > 0 || status.conflict > 0 {
            self.t("需处理", "Needs attention").into()
        } else if !status.enabled {
            self.t("未开启", "Off").into()
        } else if !status.effective_enabled {
            self.t("已暂停", "Paused").into()
        } else if self.import_ui.running() {
            self.t("导入中", "Importing").into()
        } else if let Some(at) = status.last_incremental_at_ms {
            let time = Local
                .timestamp_millis_opt(at)
                .single()
                .map(|date| date.format("%H:%M").to_string())
                .unwrap_or_else(|| "—".into());
            format!("{} {time}", self.t("已更新", "Updated"))
        } else {
            self.t("导入中", "Importing").into()
        };
        format!("WorkBuddy：{label}")
    }

    pub(in crate::statistics) fn import_notices(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let Some(status) = self.import_ui.status.as_ref() else {
            return Vec::new();
        };
        let mut notices = Vec::new();
        let missing = status.enabled && !status.root_found;
        if self.import_ui.error.is_some() && !missing {
            notices.push(Notice::Error);
        }
        if status.pending > 0 {
            notices.push(Notice::Pending);
        }
        if status.conflict > 0 {
            notices.push(Notice::Conflict);
        }
        if missing {
            notices.push(Notice::Missing);
        }
        if !status.enabled {
            notices.push(Notice::Disabled);
        }
        notices
            .into_iter()
            .take(2)
            .enumerate()
            .map(|(index, notice)| self.import_notice(index, notice, status, cx))
            .collect()
    }

    fn import_notice(
        &self,
        index: usize,
        notice: Notice,
        status: &Status,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (text, action, warning) = self.import_notice_text(notice, status);
        div()
            .id(("workbuddy-notice", index))
            .role(Role::Group)
            .aria_label(text.clone())
            .flex()
            .flex_wrap()
            .items_center()
            .gap_2()
            .p_2()
            .rounded_md()
            .bg(rgb(if warning { 0xfff8ec } else { PANEL }))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_size(px(crate::theme::font::LABEL))
                    .text_color(rgb(if warning { WARN } else { MUTED }))
                    .child(text),
            )
            .child(
                stats_button(("workbuddy-notice-action", index))
                    .ghost()
                    .label(action)
                    .disabled(
                        !matches!(notice, Notice::Pending | Notice::Disabled)
                            && (!status.effective_enabled || self.import_ui.running()),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        match notice {
                            Notice::Pending => this.open_mapping_dialog(window, cx),
                            Notice::Disabled => {
                                this.select_view(preferences::View::Settings, cx);
                                this.settings_section = 2;
                                let anchor = this.settings_anchors[2].clone();
                                window
                                    .on_next_frame(move |window, cx| anchor.scroll_to(window, cx));
                                this.request_import_status(cx);
                            }
                            _ => this.import_write(None, matches!(notice, Notice::Conflict), cx),
                        }
                        cx.notify();
                    })),
            )
            .into_any_element()
    }
    fn import_notice_text(&self, notice: Notice, status: &Status) -> (String, &'static str, bool) {
        match notice {
            Notice::Error => {
                let reason = crate::workbuddy_import::error_text(
                    self.import_ui.error.as_deref().unwrap_or(""),
                    self.zh,
                );
                let message = self.t(
                    "WorkBuddy 导入需要处理，已导入数据仍可查看。",
                    "WorkBuddy import needs attention; imported records remain available.",
                );
                (format!("{message} {reason}"), self.t("重试", "Retry"), true)
            }
            Notice::Pending => {
                let prefix = self.t("有", "Records awaiting source reread:");
                let message = self.t(
                    "条记录正在等待后台重读，或缺少模型字段，暂未计入总量。",
                    "(excluded from totals).",
                );
                (
                    format!("{prefix} {} {message}", status.pending),
                    self.t("查看说明", "Details"),
                    true,
                )
            }
            Notice::Conflict => {
                let message = self.t(
                    "条 WorkBuddy 记录存在内容冲突，未计入总量。请检查源文件后完整核对。",
                    "WorkBuddy records conflict and are excluded. Check source files and run a full check.",
                );
                (
                    format!("{} {message}", status.conflict),
                    self.t("完整核对", "Full check"),
                    true,
                )
            }
            Notice::Missing => (
                self.t(
                    "未找到本机 WorkBuddy 数据。已有代理统计可正常查看。",
                    "Local WorkBuddy data was not found. Proxy statistics remain available.",
                )
                .into(),
                self.t("重新检查", "Recheck"),
                false,
            ),
            Notice::Disabled => (
                self.t(
                    "尚未导入 WorkBuddy 内置及自定义模型用量。",
                    "WorkBuddy built-in and custom model usage has not been imported.",
                )
                .into(),
                self.t("设置导入", "Import settings"),
                false,
            ),
        }
    }
}
