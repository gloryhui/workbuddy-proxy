//! Presentation of the existing Core autostart action, after its actual reply.
use super::*;
use gpui_kit::base::{DeferredPopover, GlobalState, Popup};

gpui_kit::actions!(autostart_notice, [Close]);

pub(super) fn init(cx: &mut App) {
    cx.bind_keys([KeyBinding::new("escape", Close, Some("AutostartNotice"))]);
}

#[derive(Clone, Copy)]
enum Phase {
    Pending,
    Complete,
    Failed,
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
            phase: Phase::Complete,
            focus: cx.focus_handle().tab_stop(false),
            previous_focus: None,
            deferred: None,
            viewport: None,
            focus_pending: false,
        }
    }
}

impl Proxy {
    pub(super) fn autostart_control(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut button = crate::ui_components::system_button_base(
            "autostart",
            if self.cfg.autostart {
                "studio/autostart-on.svg"
            } else {
                "studio/autostart.svg"
            },
            self.t("开机启动", "Launch at login"),
            self.busy,
        )
        .selected(self.cfg.autostart)
        .toggled(self.cfg.autostart)
        .when(self.cfg.autostart && !self.busy, |button| {
            button
                .bg(rgb(crate::theme::toolbar_selection()))
                .text_color(rgb(crate::theme::palette().primary))
        })
        .on_click(cx.listener(|this, _, window, cx| this.toggle_autostart(window, cx)));
        let owner = cx.entity().downgrade();
        // Kit's string tooltip retains its text for the current hover. Use the
        // native GPUI host and the existing styled Tooltip view, observing the
        // owner only for this visible view's lifetime, so an in-place result
        // updates it without another mouse move or an additional polling timer.
        button.interactivity().tooltip(move |_, cx| {
            let owner = owner.clone();
            cx.new(|cx| {
                if let Some(proxy) = owner.upgrade() {
                    cx.observe(&proxy, |_, _, cx| cx.notify()).detach();
                }
                gpui_kit::component::tooltip::Tooltip::element(move |_, cx| {
                    let text = owner
                        .upgrade()
                        .map(|proxy| proxy.read(cx).autostart_tooltip_text())
                        .unwrap_or_else(|| "Launch at login".into());
                    Label::new(text)
                })
            })
            .into()
        });
        button.into_any_element()
    }

    fn autostart_tooltip_text(&self) -> SharedString {
        if matches!(self.autostart_notice.phase, Phase::Pending) && self.busy {
            self.t("正在设置开机启动…", "Changing launch at login…")
        } else if self.cfg.autostart {
            self.t(
                "开机启动：已开启（点击关闭）",
                "Launch at login: on (click to disable)",
            )
        } else {
            self.t(
                "开机启动：已关闭（点击开启）",
                "Launch at login: off (click to enable)",
            )
        }
        .into()
    }

    pub(super) fn toggle_autostart(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        if !self.autostart_notice.open {
            self.autostart_notice.previous_focus =
                window.focused(cx).or_else(|| Some(self.focus.clone()));
            self.autostart_notice.deferred = Some(GlobalState::register_deferred_popover(cx));
        }
        self.autostart_notice.open = true;
        self.autostart_notice.phase = Phase::Pending;
        self.autostart_notice.viewport = Some(window.viewport_size());
        self.autostart_notice.focus.focus(window, cx);
        // Keep the original action and confirmed config; no optimistic toggle.
        let enabled = !self.cfg.autostart;
        self.send(
            "autostart",
            vec![json!({"command":"autostart","enabled":enabled})],
            cx,
        );
        if !self.busy {
            self.finish_autostart_notice(false, cx);
        }
    }

    pub(super) fn finish_autostart_notice(&mut self, success: bool, cx: &mut Context<Self>) {
        // Called after the original config and snapshot handling. Closing while
        // pending does not cancel the action or reopen this informational layer.
        self.autostart_notice.phase = if success {
            Phase::Complete
        } else {
            Phase::Failed
        };
        if self.autostart_notice.open {
            cx.notify();
        }
    }

    fn close_autostart_notice(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.autostart_notice.open {
            return;
        }
        let restore_focus = self.autostart_notice.focus.contains_focused(window, cx)
            || window.focused(cx).is_none();
        self.autostart_notice.open = false;
        self.autostart_notice.viewport = None;
        self.autostart_notice.deferred = None;
        if let Some(focus) = self.autostart_notice.previous_focus.take()
            && restore_focus
        {
            focus.focus(window, cx);
        }
        cx.notify();
    }

    pub(super) fn subscribe_autostart_notice_bounds(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.observe_window_bounds(window, |this, window, cx| {
            if !this.autostart_notice.open {
                return;
            }
            let viewport = window.viewport_size();
            if this.autostart_notice.viewport == Some(viewport) {
                return;
            }
            this.autostart_notice.viewport = Some(viewport);
            if this.autostart_notice.focus_pending {
                return;
            }
            this.autostart_notice.focus_pending = true;
            cx.on_next_frame(window, |this, window, cx| {
                this.autostart_notice.focus_pending = false;
                if this.autostart_notice.open {
                    this.autostart_notice.focus.focus(window, cx);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    pub(super) fn autostart_toolbar(
        &self,
        toolbar: AnyElement,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        // Figma104:1600: full-toolbar right anchor, window y24+93=117.
        Popup::new("autostart-popup", toolbar)
            .anchor(Anchor::TopRight)
            .offset(px(73.))
            .w(px(crate::theme::size::SYSTEM_TOOLBAR_WIDTH))
            .h(px(crate::theme::size::TOP_ICON_ROW_HEIGHT))
            .when(self.autostart_notice.open, |popup| {
                popup.content(self.autostart_notice(cx))
            })
            .into_any_element()
    }

    fn autostart_notice(&self, cx: &mut Context<Self>) -> AnyElement {
        let (title, detail) = match self.autostart_notice.phase {
            Phase::Pending => (
                self.t("正在设置开机启动…", "Changing launch at login…"),
                self.t("请等待设置结果。", "Waiting for the result."),
            ),
            Phase::Complete if self.cfg.autostart => (
                self.t("开机启动已开启", "Launch at login is on"),
                self.t(
                    "登录系统后仅启动托盘和代理，不弹出设置界面。",
                    "Starts the tray and proxy at login, without opening settings.",
                ),
            ),
            Phase::Complete => (
                self.t("开机启动已关闭", "Launch at login is off"),
                self.t(
                    "登录系统后不再自动启动托盘和代理。",
                    "The tray and proxy will not start automatically at login.",
                ),
            ),
            Phase::Failed => (
                self.t("开机启动设置失败", "Could not change launch at login"),
                if self.cfg.autostart {
                    self.t(
                        "仍为开启状态。再次点击电源按钮重试。",
                        "Still on. Click the power button to retry.",
                    )
                } else {
                    self.t(
                        "仍为关闭状态。再次点击电源按钮重试。",
                        "Still off. Click the power button to retry.",
                    )
                },
            ),
        };
        div()
            .id("autostart-notice")
            .role(Role::Dialog)
            .aria_label(title)
            .key_context("AutostartNotice")
            .track_focus(&self.autostart_notice.focus)
            .tab_group()
            .w(px(345.))
            .h(px(88.))
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
                this.close_autostart_notice(window, cx);
                cx.stop_propagation();
            }))
            .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
            .on_mouse_down_out(
                cx.listener(|this, _, window, cx| this.close_autostart_notice(window, cx)),
            )
            .child(
                div()
                    .h(px(39.))
                    .flex_none()
                    .flex()
                    .items_start()
                    .justify_between()
                    .pl(px(14.))
                    .pr(px(12.))
                    .child(
                        Label::new(title)
                            .w(px(272.))
                            .h(px(20.))
                            .mt(px(12.))
                            .text_size(px(14.))
                            .line_height(px(20.))
                            .font_weight(FontWeight::BOLD)
                            .text_color(rgb(
                                if matches!(self.autostart_notice.phase, Phase::Failed) {
                                    crate::theme::color::DANGER
                                } else {
                                    INK
                                },
                            )),
                    )
                    .child(
                        crate::ui_components::action_button("close-autostart-notice")
                            .ghost()
                            .w(px(27.))
                            .h(px(27.))
                            .p_0()
                            .border_0()
                            .mt(px(7.))
                            .flex_none()
                            .rounded(px(7.))
                            .bg(rgb(crate::theme::color::BG_TABLE_HEAD))
                            .accessibility_label(
                                self.t("关闭开机启动提示", "Close launch at login notice"),
                            )
                            .child(
                                Label::new("×")
                                    .text_size(px(16.))
                                    .line_height(px(22.))
                                    .text_color(rgb(MUTED)),
                            )
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.close_autostart_notice(window, cx)
                            })),
                    ),
            )
            .child(
                Label::new(detail)
                    .w(px(312.))
                    .h(px(32.))
                    .ml(px(14.))
                    .mt(px(4.))
                    .text_size(px(11.))
                    .line_height(px(15.))
                    .whitespace_normal()
                    .text_color(rgb(MUTED)),
            )
            .into_any_element()
    }
}
