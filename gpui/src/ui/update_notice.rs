//! Feedback for the existing forced, public stable-release check.
use super::*;
use gpui_kit::base::{DeferredPopover, GlobalState, Popup};

gpui_kit::actions!(update_notice, [Close]);

pub(super) fn init(cx: &mut App) {
    cx.bind_keys([KeyBinding::new("escape", Close, Some("UpdateNotice"))]);
}

#[derive(Debug, PartialEq)]
struct UpdateInfo {
    available: bool,
    tag: String,
    current: String,
}

fn parse_result(text: &str) -> Result<UpdateInfo, ()> {
    let value: Value = serde_json::from_str(text).map_err(|_| ())?;
    let label = |name: &str| {
        value[name]
            .as_str()
            .filter(|s| !s.is_empty() && s.len() <= 32 && !s.chars().any(char::is_control))
            .map(str::to_owned)
            .ok_or(())
    };
    Ok(UpdateInfo {
        // The Core owns version ordering. Do not repeat its comparison here.
        available: value["available"].as_bool().ok_or(())?,
        tag: label("tag")?,
        current: label("current")?,
    })
}

enum Phase {
    Checking,
    Complete(UpdateInfo),
    Failed(String),
}

pub(super) struct State {
    open: bool,
    phase: Phase,
    focus: FocusHandle,
    previous_focus: Option<FocusHandle>,
    deferred: Option<DeferredPopover>,
    viewport: Option<Size<Pixels>>,
    focus_pending: bool,
}

impl State {
    pub(super) fn new(cx: &mut App) -> Self {
        Self {
            open: false,
            phase: Phase::Checking,
            focus: cx.focus_handle().tab_stop(false),
            previous_focus: None,
            deferred: None,
            viewport: None,
            focus_pending: false,
        }
    }
}

impl Proxy {
    pub(super) fn check_update_with_notice(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        if !self.update_notice.open {
            self.update_notice.previous_focus =
                window.focused(cx).or_else(|| Some(self.focus.clone()));
            self.update_notice.deferred = Some(GlobalState::register_deferred_popover(cx));
        }
        self.update_notice.open = true;
        self.update_notice.phase = Phase::Checking;
        self.update_notice.viewport = Some(window.viewport_size());
        self.update_notice.focus.focus(window, cx);
        self.send(
            "check_update",
            vec![json!({"command":"check_update","force":true})],
            cx,
        );
        if !self.busy {
            self.finish_update_notice(Err(self.status.clone()), cx);
        }
    }

    pub(super) fn finish_update_notice(
        &mut self,
        result: Result<String, String>,
        cx: &mut Context<Self>,
    ) {
        self.update_notice.phase = match result {
            Ok(text) => match parse_result(&text) {
                Ok(info) => Phase::Complete(info),
                Err(()) => {
                    // A malformed reply must not become a false "up to date".
                    // Keep the existing error area consistent with this view.
                    self.error = true;
                    self.status = self
                        .t("更新检查返回了无效结果", "Invalid update check response")
                        .into();
                    Phase::Failed(self.status.clone())
                }
            },
            Err(reason) => Phase::Failed(reason),
        };
        // Dismissal is independent of the original request: no cancellation,
        // no automatic reopening when a later reply arrives.
        if self.update_notice.open {
            cx.notify();
        }
    }

    fn close_update_notice(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.update_notice.open {
            return;
        }
        let restore_focus =
            self.update_notice.focus.contains_focused(window, cx) || window.focused(cx).is_none();
        self.update_notice.open = false;
        self.update_notice.viewport = None;
        self.update_notice.deferred = None;
        if let Some(focus) = self.update_notice.previous_focus.take()
            && restore_focus
        {
            focus.focus(window, cx);
        }
        cx.notify();
    }

    pub(super) fn subscribe_update_notice_bounds(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.observe_window_bounds(window, |this, window, cx| {
            if !this.update_notice.open {
                return;
            }
            let viewport = window.viewport_size();
            if this.update_notice.viewport == Some(viewport) {
                return;
            }
            this.update_notice.viewport = Some(viewport);
            if this.update_notice.focus_pending {
                return;
            }
            this.update_notice.focus_pending = true;
            cx.on_next_frame(window, |this, window, cx| {
                this.update_notice.focus_pending = false;
                if this.update_notice.open {
                    this.update_notice.focus.focus(window, cx);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    pub(super) fn update_toolbar(&self, toolbar: AnyElement, cx: &mut Context<Self>) -> AnyElement {
        Popup::new("check-update-popup", toolbar)
            .anchor(Anchor::TopRight)
            .offset(px(73.))
            .w(px(crate::theme::size::SYSTEM_TOOLBAR_WIDTH))
            .h(px(crate::theme::size::TOP_ICON_ROW_HEIGHT))
            .when(self.update_notice.open, |popup| {
                popup.content(self.update_notice(cx))
            })
            .into_any_element()
    }

    fn update_notice(&self, cx: &mut Context<Self>) -> AnyElement {
        let (message, footer) = match &self.update_notice.phase {
            Phase::Checking => (
                self.t("◌  正在检查更新…", "◌  Checking for updates…")
                    .to_owned(),
                self.t("来源：GitHub Releases", "Source: GitHub Releases")
                    .to_owned(),
            ),
            Phase::Complete(info) => (
                if info.available {
                    format!("{} {}", self.t("发现新版本", "Update available"), info.tag)
                } else {
                    self.t("当前已是最新版本", "Already up to date").to_owned()
                },
                format!(
                    "{} {}",
                    self.t("当前版本：", "Current version:"),
                    info.current
                ),
            ),
            Phase::Failed(reason) => (
                crate::i18n::message(reason, self.zh)
                    .lines()
                    .next()
                    .unwrap_or("")
                    .to_owned(),
                self.t(
                    "点击顶部刷新按钮重试。",
                    "Click the update button to retry.",
                )
                .to_owned(),
            ),
        };
        div()
            .id("check-update-notice")
            .role(Role::Dialog)
            .aria_label(self.t("检查软件更新", "Check software updates"))
            .key_context("UpdateNotice")
            .track_focus(&self.update_notice.focus)
            .tab_group()
            .w(px(299.))
            .h(px(113.))
            .flex()
            .flex_col()
            .gap_0()
            .border_1()
            .border_color(rgb(crate::theme::color::BORDER))
            .rounded(px(12.))
            .bg(rgb(crate::theme::color::BG_BAR))
            .shadow(vec![BoxShadow {
                color: rgba(0x121c3d1f).into(),
                offset: point(px(0.), px(8.)),
                blur_radius: px(22.),
                spread_radius: px(0.),
                inset: false,
            }])
            .on_action(cx.listener(|this, _: &Close, window, cx| {
                this.close_update_notice(window, cx);
                cx.stop_propagation();
            }))
            .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
            .on_mouse_down_out(
                cx.listener(|this, _, window, cx| this.close_update_notice(window, cx)),
            )
            .child(
                div()
                    .h(px(38.))
                    .flex_none()
                    .flex()
                    .items_start()
                    .justify_between()
                    .pl(px(13.))
                    .pr(px(6.))
                    .child(
                        Label::new(self.t("检查软件更新", "Check software updates"))
                            .w(px(238.))
                            .h(px(20.))
                            .mt(px(11.))
                            .text_size(px(14.))
                            .line_height(px(20.))
                            .font_weight(FontWeight::BOLD)
                            .text_color(rgb(INK)),
                    )
                    .child(
                        crate::ui_components::action_button("close-check-update")
                            .ghost()
                            .w(px(27.))
                            .h(px(27.))
                            .p_0()
                            .border_0()
                            .mt(px(6.))
                            .flex_none()
                            .rounded(px(7.))
                            .bg(rgb(crate::theme::color::BG_TABLE_HEAD))
                            .accessibility_label(self.t("关闭更新提示", "Close update notice"))
                            .child(
                                Label::new("×")
                                    .text_size(px(16.))
                                    .line_height(px(22.))
                                    .text_color(rgb(MUTED)),
                            )
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.close_update_notice(window, cx)
                            })),
                    ),
            )
            .child(
                div()
                    .id("check-update-result")
                    .aria_label(message.clone())
                    .w(px(254.))
                    .h(px(20.))
                    .ml(px(13.))
                    .mt(px(5.))
                    .overflow_hidden()
                    .child(
                        Label::new(message)
                            .w_full()
                            .text_size(px(12.))
                            .line_height(px(17.))
                            .font_weight(FontWeight::MEDIUM)
                            .truncate()
                            .text_color(rgb(
                                if matches!(self.update_notice.phase, Phase::Failed(_)) {
                                    crate::theme::color::DANGER
                                } else {
                                    crate::theme::color::TEXT_2
                                },
                            )),
                    ),
            )
            .child(
                Label::new(footer)
                    .w(px(265.))
                    .h(px(20.))
                    .ml(px(13.))
                    .mt(px(11.))
                    .text_size(px(11.))
                    .line_height(px(15.))
                    .text_color(rgb(MUTED)),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::{UpdateInfo, parse_result};

    #[test]
    fn respects_the_core_result_without_reordering_versions() {
        assert_eq!(
            parse_result(
                r#"{"available":true,"tag":"v0.6.0","current":"0.5.7","cached":false,"url":"https://github.com/gloryhui/workbuddy-proxy/releases/tag/v0.6.0"}"#
            ),
            Ok(UpdateInfo {
                available: true,
                tag: "v0.6.0".into(),
                current: "0.5.7".into()
            })
        );
        assert_eq!(
            parse_result(r#"{"available":false,"tag":"v0.5.0","current":"0.5.7"}"#),
            Ok(UpdateInfo {
                available: false,
                tag: "v0.5.0".into(),
                current: "0.5.7".into()
            })
        );
    }

    #[test]
    fn incomplete_or_wrongly_typed_replies_cannot_claim_up_to_date() {
        for text in [
            "{}",
            "null",
            "[]",
            "not json",
            r#"{"available":"false","tag":"v0.5.7","current":"0.5.7"}"#,
            r#"{"available":false,"tag":"","current":"0.5.7"}"#,
            r#"{"available":false,"tag":"v0.5.7"}"#,
            r#"{"available":false,"tag":"v0.5.7\n","current":"0.5.7"}"#,
        ] {
            assert!(parse_result(text).is_err(), "{text}");
        }
    }
}
