//! Informational feedback for the existing immediate native window action.
use super::*;
use gpui_kit::base::{DeferredPopover, GlobalState, Popup};

gpui_kit::actions!(window_restore, [Close]);

pub(super) fn init(cx: &mut App) {
    cx.bind_keys([KeyBinding::new(
        "escape",
        Close,
        Some("WindowRestoreNotice"),
    )]);
}

pub(super) struct State {
    open: bool,
    failed: bool,
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
            failed: false,
            focus: cx.focus_handle().tab_stop(false),
            previous_focus: None,
            deferred: None,
            viewport: None,
            focus_pending: false,
        }
    }
}

impl Proxy {
    pub(super) fn show_restore_notice(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.window_restore.open {
            self.window_restore.previous_focus =
                window.focused(cx).or_else(|| Some(self.focus.clone()));
            self.window_restore.deferred = Some(GlobalState::register_deferred_popover(cx));
        }
        self.window_restore.open = true;
        self.window_restore.failed = false;
        self.window_restore.viewport = Some(window.viewport_size());
        self.window_restore.focus.focus(window, cx);
        cx.notify();
    }

    pub(super) fn restore_notice_failed(&mut self) {
        self.window_restore.failed = true;
    }

    fn close_restore_notice(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.window_restore.open {
            return;
        }
        let restore_focus =
            self.window_restore.focus.contains_focused(window, cx) || window.focused(cx).is_none();
        self.window_restore.open = false;
        self.window_restore.viewport = None;
        self.window_restore.deferred = None;
        if let Some(focus) = self.window_restore.previous_focus.take()
            && restore_focus
        {
            focus.focus(window, cx);
        }
        cx.notify();
    }

    pub(super) fn subscribe_restore_notice_bounds(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.observe_window_bounds(window, |this, window, cx| {
            if !this.window_restore.open {
                return;
            }
            let viewport = window.viewport_size();
            if this.window_restore.viewport == Some(viewport) {
                return;
            }
            this.window_restore.viewport = Some(viewport);
            if this.window_restore.focus_pending {
                return;
            }
            this.window_restore.focus_pending = true;
            // Native live-resize may clear focus. Restore it after the updated
            // popup has been laid out; coalesce events, without an idle timer.
            cx.on_next_frame(window, |this, window, cx| {
                this.window_restore.focus_pending = false;
                if this.window_restore.open {
                    this.window_restore.focus.focus(window, cx);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    pub(super) fn restore_toolbar(
        &self,
        toolbar: AnyElement,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        // Figma104:2258 uses full-toolbar right alignment, y24+93=117.
        Popup::new("window-restore-popup", toolbar)
            .anchor(Anchor::TopRight)
            .offset(px(73.))
            .w(px(crate::theme::size::SYSTEM_TOOLBAR_WIDTH))
            .h(px(crate::theme::size::TOP_ICON_ROW_HEIGHT))
            .when(self.window_restore.open, |popup| {
                popup.content(self.restore_notice(cx))
            })
            .into_any_element()
    }

    fn restore_notice(&self, cx: &mut Context<Self>) -> AnyElement {
        let target = crate::default_window_size();
        let width: f32 = target.width.into();
        let height: f32 = target.height.into();
        let caption = format!(
            "{width:.0} × {height:.0} {}",
            self.t("逻辑像素", "logical pixels")
        );
        let detail = if self.window_restore.failed {
            self.t(
                "请退出全屏或最大化后重试。",
                "Leave fullscreen or maximize, then retry.",
            )
        } else if self.resetting_window {
            self.t("正在恢复窗口…", "Restoring the window…")
        } else {
            // This is the default target, not an unverified current-size claim.
            self.t("可以继续调整窗口大小。", "You can resize the window again.")
        };
        div()
            .id("window-restore-notice")
            .role(Role::Dialog)
            .aria_label(self.t("恢复默认窗口大小", "Restore default window size"))
            .key_context("WindowRestoreNotice")
            .track_focus(&self.window_restore.focus)
            .tab_group()
            .w(px(325.))
            .h(px(103.))
            .flex()
            .flex_col()
            .gap_0()
            .border_1()
            .border_color(rgb(crate::theme::color::BORDER))
            .rounded(px(12.))
            .bg(rgb(crate::theme::color::BG_BAR))
            .shadow(vec![BoxShadow {
                color: rgba(0x121c3d21).into(),
                offset: point(px(0.), px(9.)),
                blur_radius: px(22.),
                spread_radius: px(0.),
                inset: false,
            }])
            .on_action(cx.listener(|this, _: &Close, window, cx| {
                this.close_restore_notice(window, cx);
                cx.stop_propagation();
            }))
            .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
            .on_mouse_down_out(
                cx.listener(|this, _, window, cx| this.close_restore_notice(window, cx)),
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
                        Label::new(self.t("恢复默认窗口大小", "Restore default window size"))
                            .w(px(250.))
                            .h(px(20.))
                            .mt(px(11.))
                            .text_size(px(14.))
                            .line_height(px(20.))
                            .font_weight(FontWeight::BOLD)
                            .text_color(rgb(INK)),
                    )
                    .child(
                        crate::ui_components::action_button("close-window-restore")
                            .ghost()
                            .w(px(27.))
                            .h(px(27.))
                            .p_0()
                            .border_0()
                            .mt(px(6.))
                            .flex_none()
                            .rounded(px(7.))
                            .bg(rgb(crate::theme::color::BG_TABLE_HEAD))
                            .accessibility_label(
                                self.t("关闭窗口尺寸提示", "Close window size notice"),
                            )
                            .child(
                                Label::new("×")
                                    .text_size(px(16.))
                                    .line_height(px(22.))
                                    .text_color(rgb(MUTED)),
                            )
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.close_restore_notice(window, cx)
                            })),
                    ),
            )
            .child(
                Label::new(caption)
                    .w(px(274.))
                    .h(px(20.))
                    .ml(px(13.))
                    .mt(px(5.))
                    .text_size(px(13.))
                    .line_height(px(18.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(rgb(crate::theme::palette().dark)),
            )
            .child(
                Label::new(detail)
                    .w(px(280.))
                    .h(px(20.))
                    .ml(px(13.))
                    .mt(px(8.))
                    .text_size(px(11.))
                    .line_height(px(15.))
                    .text_color(rgb(if self.window_restore.failed {
                        crate::theme::color::DANGER
                    } else {
                        MUTED
                    })),
            )
            .into_any_element()
    }
}
