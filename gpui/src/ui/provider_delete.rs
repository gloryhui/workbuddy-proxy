//! Native delete confirmation. The existing control API owns removal and persistence.
use super::*;
use gpui_kit::component::{
    Disableable,
    button::ButtonCustomVariant,
    scroll::{ScrollableElement, ScrollableMask},
};

#[derive(Clone)]
pub(super) struct DeleteTarget {
    id: String,
    name: String,
    models: usize,
    dirty: bool,
}

pub(super) fn margin_top(window: &Window) -> Pixels {
    ((window.viewport_size().height - px(322.)) / 2. - px(18.)).max(px(16.))
}

fn body_limit(window: &Window) -> Pixels {
    // Header, footer and padding occupy 183px. Keep the actions in the window
    // even when an unusually long provider name needs its own scroll viewport.
    (window.viewport_size().height - margin_top(window) - px(199.)).max(px(139.))
}

fn text(value: impl Into<SharedString>, font: f32, line: f32, color: u32) -> Label {
    Label::new(value)
        .text_size(px(font))
        .line_height(px(line))
        .text_color(rgb(color))
        .whitespace_normal()
        .h_auto()
        .flex_none()
}

impl Proxy {
    pub(super) fn subscribe_provider_delete_bounds(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.observe_window_bounds(window, |this, _, cx| {
            if this.delete_open {
                cx.notify();
            }
        })
        .detach();
    }

    pub(super) fn open_provider_delete(&mut self, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        self.capture(cx);
        let id = self.selected();
        let Some(provider) = self.cfg.providers.iter().find(|provider| provider.id == id) else {
            return;
        };
        self.provider_delete_target = Some(DeleteTarget {
            id,
            name: if provider.name.trim().is_empty() {
                self.t("未命名供应商", "Unnamed provider").into()
            } else {
                provider.name.clone()
            },
            models: provider.models.len(),
            dirty: self
                .drafts
                .providers
                .get(&provider.id)
                .is_some_and(|draft| draft != &Draft::from_provider(provider)),
        });
        self.provider_delete_scroll
            .set_offset(point(px(0.), px(0.)));
        self.delete_open = true;
        cx.notify();
    }

    fn confirm_provider_delete(&mut self, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let Some(target) = self.provider_delete_target.take() else {
            return;
        };
        self.delete_open = false;
        self.delete_feedback.begin(&target.id);
        // The warning and the command identify the same provider, even if the
        // selected row changes while a background reply is being reconciled.
        self.send(
            "delete",
            vec![json!({"command":"delete","provider_id":target.id})],
            cx,
        );
        if !self.busy {
            self.delete_feedback.clear();
        }
    }

    pub(super) fn scroll_provider_delete(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let modifiers = event.keystroke.modifiers;
        if !self.delete_open
            || modifiers.platform
            || modifiers.control
            || modifiers.alt
            || modifiers.shift
        {
            return;
        }
        let offset = self.provider_delete_scroll.offset();
        let step = match event.keystroke.key.as_str() {
            "down" => px(-30.),
            "up" => px(30.),
            "pagedown" => -body_limit(window),
            "pageup" => body_limit(window),
            "home" => {
                self.provider_delete_scroll
                    .set_offset(point(px(0.), px(0.)));
                px(0.)
            }
            "end" => {
                self.provider_delete_scroll.scroll_to_item(2);
                px(0.)
            }
            _ => return,
        };
        if step != px(0.) {
            self.provider_delete_scroll
                .set_offset(point(offset.x, offset.y + step));
        }
        window.prevent_default();
        cx.stop_propagation();
        cx.notify();
    }

    pub(super) fn provider_delete_confirmation(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(target) = self.provider_delete_target.as_ref() else {
            return div().into_any_element();
        };
        let danger = crate::theme::color::DESTRUCTIVE;
        let tint = crate::theme::color::DESTRUCTIVE_TINT;
        let description = if self.zh {
            format!(
                "保存的 API Key 和该供应商的 {} 个模型将一并移除。",
                target.models
            )
        } else {
            format!(
                "The saved API key and {} saved {} will also be removed.",
                target.models,
                if target.models == 1 {
                    "model"
                } else {
                    "models"
                }
            )
        };
        let rows = div().id("provider-delete-content").flex().flex_col().flex_none().w_full()
            .max_h(body_limit(window)).overflow_y_scroll().track_scroll(&self.provider_delete_scroll)
            .child(text(format!("{}  {}", self.t("即将删除", "Deleting"), target.name), 14., 20., INK)
                .font_weight(FontWeight::BOLD).pr(px(12.)))
            .child(div().flex().flex_col().flex_none().mt(px(11.)).gap(px(4.)).pr(px(12.))
                .child(text(description, 12., 17., crate::theme::color::TEXT_2))
                .when(target.dirty, |description| description.child(text(self.t(
                    "当前未保存的修改也会丢弃。", "Unsaved changes for this provider will also be discarded."),
                    12., 17., crate::theme::color::TEXT_2))))
            .child(div().flex().flex_col().flex_none().min_h(px(76.)).mt(px(15.)).px(px(14.)).py(px(13.))
                .gap(px(10.)).rounded(px(11.)).bg(rgb(tint))
                .child(text(self.t("此操作不能直接撤销。", "This cannot be undone directly."), 12., 17., danger)
                    .font_weight(FontWeight::BOLD))
                .child(text(self.t("删除后，需要再次同步 WorkBuddy，才能清理客户端中的对应模型。",
                    "Sync WorkBuddy again after deletion to remove its corresponding client models."),
                    12., 17., crate::theme::color::TEXT_2)));
        let content = div()
            .relative()
            .w_full()
            .flex_none()
            .child(rows)
            .child(ScrollableMask::new(
                Axis::Vertical,
                &self.provider_delete_scroll,
            ))
            .vertical_scrollbar(&self.provider_delete_scroll);
        let actions = [
            ("delete-cancel", self.t("取消", "Cancel"), false),
            (
                "delete-confirm",
                self.t("删除供应商", "Delete provider"),
                true,
            ),
        ]
        .into_iter()
        .enumerate()
        .map(|(index, (id, label, destructive))| {
            let button = crate::ui_components::action_button(id)
                .h(px(38.))
                .min_w(px(if destructive { 110. } else { 89. }))
                .rounded(px(10.))
                .label(label)
                .disabled(destructive && self.busy)
                .when(!destructive, |button| {
                    button.outline().bg(rgb(crate::theme::color::BG_TABLE_HEAD))
                })
                .when(destructive, |button| {
                    button
                        .custom(
                            ButtonCustomVariant::new(cx)
                                .color(rgb(danger).into())
                                .foreground(rgb(crate::theme::color::WHITE).into())
                                .hover(rgb(crate::theme::color::DESTRUCTIVE_HOVER).into())
                                .active(rgb(crate::theme::color::DESTRUCTIVE_HOVER).into()),
                        )
                        // Kit custom variants tint the resting fill to 20%.
                        // Keep the destructive action solid without painting over
                        // the native disabled state.
                        .when(!self.busy, |button| button.bg(rgb(danger)))
                })
                .on_click(cx.listener(move |this, _, window, cx| {
                    if destructive {
                        this.confirm_provider_delete(cx);
                    } else {
                        this.dismiss(&Dismiss, window, cx);
                    }
                }));
            div()
                .track_focus(&self.provider_delete_focus[index])
                .key_context("ProviderDeleteControl")
                .on_action(cx.listener(
                    move |this, _: &ActivateProviderDeleteControl, window, cx| {
                        if destructive {
                            this.confirm_provider_delete(cx);
                        } else {
                            this.dismiss(&Dismiss, window, cx);
                        }
                        cx.stop_propagation();
                    },
                ))
                .child(button)
        });
        div()
            .flex()
            .flex_col()
            .w_full()
            .gap_0()
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap(px(12.))
                    .h(px(46.))
                    .flex_none()
                    .pr(px(20.))
                    .child(
                        div()
                            .size(px(46.))
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(13.))
                            .bg(rgb(tint))
                            // Figma 78:1312 is a literal text node, not a substitute SVG.
                            .child(
                                Label::new("!")
                                    .text_size(px(23.))
                                    .line_height(px(33.))
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(rgb(danger)),
                            ),
                    )
                    .child(
                        text(self.t("删除供应商？", "Delete provider?"), 20., 28., INK)
                            .mt(px(2.))
                            .font_weight(FontWeight::BOLD),
                    ),
            )
            .child(div().mt(px(24.)).flex_none().child(content))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_end()
                    .flex_none()
                    .gap(px(8.))
                    .mt(px(15.))
                    .pt(px(17.))
                    .border_t_1()
                    .border_color(rgb(crate::theme::color::BORDER))
                    .children(actions),
            )
            .into_any_element()
    }
}
