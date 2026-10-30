//! Studio V2 runtime rail. Core commands, counters and log ownership stay in Proxy.
use super::*;
use crate::theme::color;
use gpui_kit::component::{Disableable, button::ButtonCustomVariant};

pub(super) const COLUMN_GAP: f32 = 17.;

/// Shared by the rendered rail and the model table's initial width estimate.
pub(super) fn rail_width(window: &Window) -> f32 {
    if window.viewport_size().width < px(1280.) {
        300.
    } else {
        348.
    }
}

fn vector(path: &'static str, size: f32, ink: u32) -> Icon {
    Icon::default()
        .path(path)
        .with_size(px(size))
        .text_color(rgb(ink))
}

fn log_action(
    id: &'static str,
    icon: IconName,
    label: &'static str,
    disabled: bool,
    cx: &App,
) -> Button {
    Button::new(id)
        .custom(
            ButtonCustomVariant::new(cx)
                .color(rgb(color::TERMINAL).into())
                .foreground(rgb(color::SIDEBAR_MUTED).into())
                .hover(rgb(color::SIDEBAR_HOVER).into())
                .active(rgb(color::SIDEBAR_ACTIVE).into()),
        )
        .with_size(px(27.))
        .size(px(27.))
        .p_0()
        .rounded(px(7.))
        .disabled(disabled)
        .when(disabled, |button| button.opacity(0.45))
        .accessibility_label(label)
        .tooltip(label)
        .child(
            Icon::new(icon)
                .with_size(px(15.))
                .text_color(rgb(color::SIDEBAR_MUTED)),
        )
}

impl Proxy {
    pub(super) fn right(&mut self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        div()
            .w(px(rail_width(window)))
            .flex_shrink_0()
            .flex()
            .flex_col()
            .gap(px(17.))
            .child(self.runtime_card(cx))
            .child(self.log_card(cx))
            .into_any_element()
    }

    fn runtime_card(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let (dot, text, ink) = self.core_indicator();
        let address = self
            .proxy_address
            .strip_suffix("/v1")
            .unwrap_or(&self.proxy_address)
            .trim_start_matches("http://")
            .trim_start_matches("https://")
            .trim_end_matches('/')
            .to_string();
        rail_card("runtime-center").p(px(18.)).gap_0().flex_shrink_0()
            .child(div().flex().items_center().gap(px(10.)).h(px(27.))
                .child(vector("studio/runtime-activity.svg", 19., crate::theme::palette().primary))
                .child(Label::new(self.t("运行中心", "Runtime")).text_size(px(16.)).font_weight(FontWeight::BOLD))
                .child(div().flex_1())
                .child(div().flex().items_center().gap(px(5.)).px(px(9.)).h(px(27.)).rounded(px(9.))
                    .bg(rgb(if self.core_state() == CoreState::Responding {color::SUCCESS_TINT} else {color::BG_PAGE}))
                    .child(div().size(px(6.)).rounded_full().bg(rgb(dot)))
                    .child(Label::new(text).text_size(px(11.)).text_color(rgb(ink)))))
            .when(self.error && self.editing.is_none(), |card| card.child(
                Label::new(self.status.clone()).mt(px(8.)).text_size(px(12.)).text_color(rgb(color::DANGER))))
            .child(div().mt(px(17.)).flex().items_center().justify_between().gap(px(8.))
                .child(Label::new(address).text_size(px(18.)).font_weight(FontWeight::SEMIBOLD))
                .child(crate::ui_components::action_button("copy-relay-address").ghost().size(px(27.)).p_0()
                    .accessibility_label(self.t("复制代理地址", "Copy relay address"))
                    .tooltip(self.t("复制代理地址", "Copy relay address"))
                    .child(vector("studio/runtime-copy.svg", 16., color::TEXT_3))
                    .on_click(cx.listener(|this, _, _, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(this.proxy_address.clone()));
                        this.status = this.t("代理地址已复制", "Relay address copied").into();
                        this.error = false;
                        cx.notify();
                    }))))
            .child(div().flex().items_center().justify_between().mt(px(2.)).gap(px(4.))
                .child(Label::new(self.t("本地 OpenAI 兼容接口 · /v1", "Local OpenAI-compatible · /v1")).text_size(px(11.)).text_color(rgb(color::TEXT_3)))
                .child(crate::ui_components::action_button("port-settings").ghost().size(px(24.)).p_0().disabled(self.busy)
                    .accessibility_label(self.t("端口设置", "Port settings"))
                    .tooltip(self.t("端口设置", "Port settings"))
                    .child(vector("studio/nav-settings.svg", 18., color::TEXT_3))
                    .on_click(cx.listener(|this, _, _, cx| {this.port_open = true; this.port_error = false; cx.notify();}))))
            .child(div().mt(px(9.)).h(px(1.)).bg(rgb(color::BORDER)))
            .child(div().mt(px(13.)).mb(px(20.)).flex().children(
                [self.t("供应商", "Providers"), self.t("模型", "Models"), self.t("请求", "Requests")]
                    .into_iter().zip(self.proxy_counts).map(|(label, count)| {
                        div().flex_1().flex().flex_col().gap(px(3.))
                            .child(Label::new(count.to_string()).text_size(px(25.)).font_weight(FontWeight::BOLD).line_height(px(33.)))
                            .child(Label::new(label).text_size(px(11.)).text_color(rgb(color::TEXT_3)))
                    })))
            .child(div().flex().items_center().gap(px(9.))
                .child(crate::ui_components::button("sync", crate::ui_components::ButtonKind::Primary, self.t("同步 WorkBuddy", "Sync WorkBuddy"), self.busy, false)
                    .flex_1().px(px(8.))
                    .tooltip(self.t("模型变更后可能需要完全退出并重开 WorkBuddy；同步前会提示确认。", "After model changes, WorkBuddy may need to quit and reopen; confirmation appears before syncing."))
                    .on_click(cx.listener(|this, _, _, cx| {this.sync_open = true; cx.notify();})))
                .child(crate::ui_components::button("restore", crate::ui_components::ButtonKind::Normal, self.t("取消同步", "Unsync"), self.busy, false)
                    .px(px(12.))
                    .tooltip(self.t("移除代理同步到 WorkBuddy 的模型", "Remove proxy models synced to WorkBuddy"))
                    .on_click(cx.listener(|this, _, _, cx| {this.unsync_open = true; cx.notify();}))))
            .into_any_element()
    }

    fn log_card(&mut self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .id("log-panel")
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .w_full()
            .bg(rgb(color::TERMINAL))
            .rounded(px(16.))
            .p(px(18.))
            .gap(px(12.))
            .child(
                div()
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(vector("studio/log-terminal.svg", 19., color::SIDEBAR_TEXT))
                    .child(
                        Label::new(self.t("活动日志", "Activity logs"))
                            .text_size(px(16.))
                            .font_weight(FontWeight::BOLD)
                            .text_color(rgb(color::SIDEBAR_TEXT)),
                    )
                    .child(div().flex_1())
                    .child(
                        log_action(
                            "clear-logs",
                            IconName::Delete,
                            self.t("清空日志", "Clear logs"),
                            self.busy,
                            cx,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.send("clear_logs", vec![json!({"command":"clear_logs"})], cx)
                        })),
                    )
                    .child(
                        log_action(
                            "copy-logs",
                            IconName::Copy,
                            self.t("复制日志", "Copy logs"),
                            false,
                            cx,
                        )
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
            )
            .child(
                div()
                    .flex_shrink_0()
                    .h(px(1.))
                    .bg(rgb(color::TERMINAL_LINE)),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .child(crate::ui_components::terminal_logs(
                        &self.logs,
                        cx.theme().mono_font_family.clone(),
                        self.t("活动日志", "Activity logs"),
                    )),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .h(px(1.))
                    .bg(rgb(color::TERMINAL_LINE)),
            )
            .child(
                Label::new(self.t(
                    "最近 100 条 · 新活动自动滚动",
                    "Latest 100 · new activity scrolls to bottom",
                ))
                .text_size(px(10.))
                .text_color(rgb(color::SIDEBAR_MUTED)),
            )
            .into_any_element()
    }

    /// Keep the existing port validation and confirmation contract. Opening this
    /// editor never writes configuration or restarts either application.
    pub(super) fn prepare_port(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        match self
            .port
            .read(cx)
            .value()
            .parse::<u16>()
            .ok()
            .filter(|p| *p > 0)
        {
            Some(port) => {
                let current = self
                    .proxy_address
                    .parse::<reqwest::Url>()
                    .ok()
                    .and_then(|u| u.port_or_known_default())
                    .unwrap_or(self.cfg.port);
                self.port_open = false;
                self.modal_focus.focus(window, cx);
                self.port_error = false;
                if port != current {
                    self.pending_port = Some(port);
                }
                cx.notify();
            }
            None => {
                self.port_error = true;
                cx.notify();
            }
        }
    }

    pub(super) fn port_dialog(&mut self, cx: &mut Context<Self>) -> AnyElement {
        crate::ui_components::dialog_body()
            .w_full()
            .gap(px(12.))
            .child(crate::ui_components::dialog_title(
                self.t("端口设置", "Port settings"),
            ))
            .child(body_hint(self.t(
                "应用后将先确认端口变更和 WorkBuddy 重启。",
                "Applying opens confirmation for the port change and WorkBuddy restart.",
            )))
            .child(
                crate::ui_components::text_input(&self.port, self.busy)
                    .aria_label(self.t("本地端口", "Local port")),
            )
            .when(self.port_error, |body| {
                body.child(
                    Label::new(self.t("请输入 1–65535 的端口号", "Enter a port from 1 to 65535"))
                        .text_size(px(12.))
                        .text_color(rgb(color::DANGER)),
                )
            })
            .child(
                crate::ui_components::dialog_footer()
                    .child(
                        crate::ui_components::button(
                            "port-apply",
                            crate::ui_components::ButtonKind::Primary,
                            self.t("应用端口", "Apply port"),
                            self.busy,
                            false,
                        )
                        .on_click(cx.listener(|this, _, window, cx| this.prepare_port(window, cx))),
                    )
                    .child(
                        crate::ui_components::button(
                            "port-edit-cancel",
                            crate::ui_components::ButtonKind::Normal,
                            self.t("取消", "Cancel"),
                            false,
                            false,
                        )
                        .on_click(
                            cx.listener(|this, _, window, cx| this.dismiss(&Dismiss, window, cx)),
                        ),
                    ),
            )
            .into_any_element()
    }
}
