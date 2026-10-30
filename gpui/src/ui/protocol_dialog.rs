//! Studio protocol shell; parsing and protocol validation stay in the typed editor.
use super::*;
use crate::upstream_config::UpstreamProtocol;
use gpui_kit::component::{button::ButtonCustomVariant, tag::Tag};

pub(super) fn surface_height(window: &Window) -> Pixels {
    (window.viewport_size().height - px(64.)).min(px(724.))
}

const PROTOCOLS: [(UpstreamProtocol, &str); 4] = [
    (UpstreamProtocol::OpenaiChat, "Chat"),
    (UpstreamProtocol::OpenaiResponses, "Responses"),
    (UpstreamProtocol::AnthropicMessages, "Anthropic"),
    (UpstreamProtocol::GeminiGenerateContent, "Gemini"),
];

impl Proxy {
    pub(super) fn subscribe_protocol_editor_focus(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        for (handle, control) in self.protocol_options.focus_targets(cx) {
            cx.on_focus(&handle, window, move |this, _, cx| {
                if this.protocol_open {
                    this.protocol_focused_control = control;
                    this.protocol_scroll
                        .scroll_to_item(this.protocol_options.focus_row(control));
                    cx.notify();
                }
            })
            .detach();
        }
        cx.observe_window_bounds(window, |this, _, cx| {
            if this.protocol_open {
                cx.notify();
            }
        })
        .detach();
    }

    fn select_protocol_tab(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        // Kit's Button intentionally prevents mouse-down focus. Give the tab
        // its own native focus scope so Enter cannot remain on an old input
        // and reach the dialog's Apply action after a pointer switch.
        self.protocol_tab_focus[index].focus(window, cx);
        let protocol = PROTOCOLS[index].0;
        if self.protocol_options.protocol() != protocol {
            self.protocol_options.switch_to(protocol, window, cx);
            self.protocol_error = None;
            self.protocol_focused_control = if protocol == UpstreamProtocol::AnthropicMessages {
                1
            } else {
                2
            };
            self.protocol_scroll.set_offset(point(px(0.), px(0.)));
            self.protocol_layout = None;
        }
        cx.notify();
    }

    fn protocol_tabs(&self, window: &Window, cx: &mut Context<Self>) -> Div {
        let active = self.protocol_options.protocol();
        let studio = crate::theme::current_skin() == crate::theme::Skin::StudioIndigo;
        div()
            .flex()
            .items_center()
            .w_full()
            .h(px(40.))
            .flex_none()
            .p(px(4.))
            .gap(px(if studio { 0. } else { 4. }))
            .bg(rgb(crate::theme::color::BG_TABLE_HEAD))
            .rounded(px(10.))
            .children(
                PROTOCOLS
                    .into_iter()
                    .enumerate()
                    .map(|(index, (protocol, title))| {
                        let selected = active == protocol;
                        let scope_focused = self.protocol_tab_focus[index].is_focused(window);
                        let button = crate::ui_components::action_button(("protocol-tab", index))
                            .w_full()
                            .h(px(32.))
                            .rounded(px(8.))
                            .text_size(px(12.))
                            .when(studio, |button| {
                                button.px(px(14.)).accessibility_label(title).child(
                                    div()
                                        .w_full()
                                        .text_size(px(12.))
                                        .line_height(px(17.))
                                        .font_weight(if selected {
                                            FontWeight::BOLD
                                        } else {
                                            FontWeight::MEDIUM
                                        })
                                        .child(title),
                                )
                            })
                            .when(!studio, |button| button.label(title))
                            .toggled(selected)
                            .selected(studio && selected)
                            .border_1()
                            .border_color(rgb(if scope_focused {
                                crate::theme::palette().primary
                            } else if selected && studio {
                                0xd5d1ff
                            } else if selected {
                                crate::theme::palette().border
                            } else {
                                crate::theme::color::BG_TABLE_HEAD
                            }))
                            .custom(
                                ButtonCustomVariant::new(cx)
                                    .color(
                                        rgb(if selected {
                                            crate::theme::color::BG_BAR
                                        } else {
                                            crate::theme::color::BG_TABLE_HEAD
                                        })
                                        .into(),
                                    )
                                    .foreground(
                                        rgb(if selected {
                                            crate::theme::palette().dark
                                        } else {
                                            MUTED
                                        })
                                        .into(),
                                    )
                                    .hover(rgb(crate::theme::palette().tint).into())
                                    // Native selected styling reads active; toggled only exposes AX pressed.
                                    .active(if studio && selected {
                                        rgb(crate::theme::color::BG_BAR).into()
                                    } else {
                                        cx.theme().transparent
                                    }),
                            )
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.select_protocol_tab(index, window, cx);
                            }));
                        div()
                            .when(studio, |tab| {
                                tab.w(px([123., 124., 124., 137.][index]))
                                    .flex_none()
                                    .ml(px([0., 15., 16., 16.][index]))
                            })
                            .when(!studio, |tab| tab.flex_1())
                            .min_w_0()
                            .h(px(32.))
                            .track_focus(&self.protocol_tab_focus[index])
                            .key_context("ProtocolTab")
                            .on_action(cx.listener(
                                move |this, _: &ActivateProtocolTab, window, cx| {
                                    this.select_protocol_tab(index, window, cx);
                                    cx.stop_propagation();
                                },
                            ))
                            .child(button)
                    }),
            )
    }

    pub(super) fn protocol_dialog(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let studio = crate::theme::current_skin() == crate::theme::Skin::StudioIndigo;
        let height = surface_height(window) - px(36.);
        let footer_height = if self.protocol_error.is_some() {
            98.
        } else {
            44.
        };
        let form_height = (height - px(106. + footer_height)).max(px(100.));
        let protocol = self.protocol_options.protocol();
        let geometry = (form_height, protocol);
        if self.protocol_layout != Some(geometry) {
            self.protocol_layout = Some(geometry);
            // GPUI consumes scroll_to_item before updating viewport bounds.
            // Correct once after geometry settles; there is no timer/redraw loop.
            cx.on_next_frame(window, |this, _, cx| {
                if this.protocol_open {
                    this.protocol_scroll.scroll_to_item(
                        this.protocol_options
                            .focus_row(this.protocol_focused_control),
                    );
                    cx.notify();
                }
            });
        }
        let tag = match protocol {
            UpstreamProtocol::OpenaiChat => "CHAT",
            UpstreamProtocol::OpenaiResponses => "RESPONSES",
            UpstreamProtocol::AnthropicMessages => "MESSAGES",
            UpstreamProtocol::GeminiGenerateContent => "GEMINI",
        };
        let form = self.protocol_options.render(
            self.zh,
            cx.theme().mono_font_family.clone(),
            &self.protocol_scroll,
            form_height,
            surface_height(window) < px(724.) || self.protocol_error.is_some(),
            cx,
        );
        div().flex().flex_col().w_full().h(height).gap_0()
            .child(div().flex().flex_col().h(px(66.)).flex_none().gap(px(4.))
                .child(div().flex().items_center().justify_between().h(px(29.)).flex_none().pr(px(if studio { 7. } else { 9. })).gap(px(12.))
                    .child(Label::new(self.t("上游协议选项", "Upstream protocol options"))
                        .text_size(px(20.)).line_height(px(29.)).font_weight(FontWeight::BOLD).text_color(rgb(INK))
                        .when(studio, |title| title.relative().top(px(-1.))))
                    .child(Tag::custom(rgb(if studio { 0xeeedff } else { crate::theme::palette().tint }).into(), rgb(crate::theme::palette().dark).into(),
                        rgb(if studio { 0xeeedff } else { crate::theme::palette().tint }).into()).small().h(px(27.)).w(px(126.))
                        .rounded(px(8.)).text_size(px(11.))
                        .when(studio, |tag| crate::studio_fonts::latin(tag, cx)
                            .font_weight(FontWeight::MEDIUM).line_height(px(16.)).px(px(11.)).py_0())
                        .child(tag)))
                .child(Label::new(self.t("配置不同上游协议的思考参数，不改变 WorkBuddy 本地入口。",
                    "Configure upstream thinking parameters; the local WorkBuddy entry is unchanged."))
                    .text_size(px(12.)).line_height(px(17.)).text_color(rgb(MUTED))))
            .child(self.protocol_tabs(window, cx))
            .child(form)
            .child(div().flex().flex_col().h(px(footer_height)).flex_none()
                .border_t_1().border_color(rgb(crate::theme::color::BORDER)).pt(px(6.))
                .when_some(self.protocol_error.clone(), |footer, error| {
                    footer.child(div().h(px(48.)).flex_none().child(Label::new(error)
                        .text_size(px(12.)).line_height(px(17.)).whitespace_normal().h_auto()
                        .text_color(rgb(crate::theme::color::DANGER))))
                })
                .child(div().flex().items_center().justify_between().gap(px(12.)).h(px(38.)).flex_none()
                    .child(Label::new(self.t("修改保留在草稿中，保存供应商后生效",
                        "Apply changes to the draft, then save this provider."))
                        .flex_1().min_w_0().whitespace_normal().h_auto()
                        .text_size(px(11.)).line_height(px(16.)).text_color(rgb(MUTED)))
                    .child(crate::ui_components::action_button("protocol-apply").primary()
                        .h(px(38.)).min_w(px(123.)).rounded(px(10.))
                        .label(self.t("应用到草稿", "Apply to draft"))
                        .on_click(cx.listener(|this, _, window, cx| this.confirm_protocol(window, cx))))))
            .into_any_element()
    }
}
