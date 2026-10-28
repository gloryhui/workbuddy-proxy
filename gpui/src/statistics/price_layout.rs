//! Studio pricing dialog. Existing states, draft validation and writes own behavior.
use super::*;
use gpui_kit::component::{
    input::Input,
    scroll::{ScrollableElement, ScrollableMask},
    tag::Tag,
};

const SURFACE: u32 = crate::theme::color::BG_TABLE_HEAD;

fn field(label: &'static str, control: impl IntoElement) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(6.))
        .flex_1()
        .min_w_0()
        .flex_shrink_0()
        .child(Label::new(label).text_size(px(12.)).text_color(rgb(MUTED)))
        .child(control)
}

fn input(state: &Entity<InputState>, disabled: bool, label: &'static str) -> Input {
    Styled::h(crate::ui_components::text_input(state, disabled), px(42.))
        .bg(rgb(SURFACE))
        .border_color(rgb(BORDER))
        .rounded(px(10.))
        .aria_label(label)
}

impl Statistics {
    pub(super) fn subscribe_price_focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        // The native input scrolls its text, not its enclosing form. Keep the
        // entire labelled row visible when Tab/Shift+Tab moves into it.
        for (handle, row) in [
            (self.price_provider.read(cx).focus_handle(cx), 0),
            (self.price_model.read(cx).focus_handle(cx), 1),
            (self.price_mode.read(cx).focus_handle(cx), 2),
            (self.price_currency.read(cx).focus_handle(cx), 2),
            (self.price_input.read(cx).focus_handle(cx), 3),
            (self.price_output.read(cx).focus_handle(cx), 3),
            (self.price_read.read(cx).focus_handle(cx), 4),
            (self.price_write.read(cx).focus_handle(cx), 4),
            (self.price_label.read(cx).focus_handle(cx), 5),
        ] {
            cx.on_focus(&handle, window, move |this, _, cx| {
                if this.price_open {
                    this.price_form_scroll.scroll_to_item(row);
                    cx.notify();
                }
            })
            .detach();
        }
    }

    pub(crate) fn open_price_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.price_open = true;
        self.price_form_scroll.set_offset(point(px(0.), px(0.)));
        self.sync_price_providers(window, cx);
        let owner = cx.entity().downgrade();
        let observer = cx.entity().clone();
        let body = cx.new(|cx| {
            cx.observe(&observer, |_, _, cx| cx.notify()).detach();
            PriceBody {
                owner: owner.clone(),
            }
        });
        window.open_dialog(cx, move |dialog, window, _| {
            let height = (window.viewport_size().height - px(64.)).min(px(625.));
            let close_owner = owner.clone();
            let save_owner = owner.clone();
            dialog
                .width(px(574.))
                .h(height)
                .p(px(26.))
                .pt(px(22.))
                .pb(px(10.))
                .rounded(px(18.))
                .border_color(rgb(BORDER))
                .bg(rgb(PANEL))
                .margin_top(((window.viewport_size().height - height) / 2.).max(px(16.)))
                .close_button(true)
                // Kit's child slot adds its own whole-body scrollbar. The content
                // slot lets the form scroll while header and actions remain fixed.
                .content({
                    let body = body.clone();
                    move |content, _, _| content.gap_0().child(body.clone())
                })
                .on_ok(move |_, _, cx| {
                    let _ = save_owner.update(cx, |this, cx| this.save_price(cx));
                    false
                })
                .on_close(move |_, window, cx| {
                    let _ = close_owner.update(cx, |this, cx| {
                        this.price_closing = false;
                        this.cancel_price(window, cx)
                    });
                })
        });
    }

    fn price_body(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let height = (window.viewport_size().height - px(64.)).min(px(625.));
        let body_height = height - px(34.); // Native surface padding and border.
        let header_height = if self.zh { 67. } else { 84. };
        let footer_height = if self.price_draft.error.is_some() {
            120.
        } else {
            90.
        };
        let form_height = (body_height - px(header_height + footer_height)).max(px(100.));
        let symbol = match self
            .price_currency
            .read(cx)
            .selected_value()
            .map(String::as_str)
        {
            Some("USD") => "$",
            _ => "¥",
        };
        let price = |state, label| {
            input(state, false, label)
                .prefix(Label::new(symbol).text_size(px(13.)).text_color(rgb(MUTED)))
        };
        let select_style = |select: gpui_kit::component::select::Select<Vec<Choice>>| {
            select
                .h(px(42.))
                .w_full()
                .bg(rgb(SURFACE))
                .border_color(rgb(BORDER))
                .rounded(px(10.))
        };
        let locked = self.price_draft.identity_locked();
        let provider_label = self.t("供应商", "Provider");
        let model_label = self.t("上游模型 ID", "Upstream model ID");
        let mode_label = self.t("计价方式", "Pricing mode");
        let currency_label = self.t("币种", "Currency");
        let rows = div()
            .id("price-form-rows")
            .flex()
            .flex_col()
            .gap(px(16.))
            .pt(px(20.))
            .pb(px(20.))
            .pr(px(12.))
            .child(field(
                provider_label,
                select_style(crate::ui_components::select(&self.price_provider, locked))
                    .accessibility_label(provider_label),
            ))
            .child(field(
                model_label,
                input(&self.price_model, locked, model_label),
            ))
            .child(
                div()
                    .flex()
                    .gap(px(16.))
                    .flex_shrink_0()
                    .child(field(
                        mode_label,
                        select_style(crate::ui_components::select(&self.price_mode, false))
                            .accessibility_label(mode_label),
                    ))
                    .child(field(
                        currency_label,
                        select_style(crate::ui_components::select(&self.price_currency, false))
                            .accessibility_label(currency_label),
                    )),
            )
            .child(
                div()
                    .flex()
                    .gap(px(14.))
                    .flex_shrink_0()
                    .child(field(
                        self.t("输入价格 / 百万", "Input / million"),
                        price(
                            &self.price_input,
                            self.t("输入价格 / 百万", "Input / million"),
                        ),
                    ))
                    .child(field(
                        self.t("输出价格 / 百万", "Output / million"),
                        price(
                            &self.price_output,
                            self.t("输出价格 / 百万", "Output / million"),
                        ),
                    )),
            )
            .child(
                div()
                    .flex()
                    .gap(px(14.))
                    .flex_shrink_0()
                    .child(field(
                        self.t("缓存读取 / 百万", "Cache read / million"),
                        price(
                            &self.price_read,
                            self.t("缓存读取 / 百万", "Cache read / million"),
                        ),
                    ))
                    .child(field(
                        self.t("缓存写入 / 百万", "Cache write / million"),
                        price(
                            &self.price_write,
                            self.t("缓存写入 / 百万", "Cache write / million"),
                        ),
                    )),
            )
            // The existing label is deliberately retained below the design's
            // fields; it remains reachable without stretching the native modal.
            .child(field(
                self.t("备注", "Label"),
                input(&self.price_label, false, self.t("备注", "Label")),
            ))
            .when(locked, |rows| {
                rows.child(
                    Label::new(self.t(
                        "编辑时供应商和上游模型 ID 不可更改",
                        "Provider and upstream model ID are fixed while editing",
                    ))
                    .text_size(px(11.))
                    .text_color(rgb(MUTED)),
                )
            });
        let form = div()
            .relative()
            .w_full()
            .h(form_height)
            .flex_none()
            .child(
                rows.h(form_height)
                    .overflow_y_scroll()
                    .track_scroll(&self.price_form_scroll),
            )
            .child(ScrollableMask::new(Axis::Vertical, &self.price_form_scroll))
            .vertical_scrollbar(&self.price_form_scroll);
        div().flex().flex_col().w_full().h(body_height).gap_0()
            .child(div().flex().flex_col().h(px(header_height)).flex_none().gap(px(8.))
                .child(div().flex().items_center().justify_between().gap(px(12.)).pr(px(24.))
                    .child(Label::new(if self.price_draft.is_editing() { self.t("编辑价格", "Edit price") }
                        else { self.t("新建价格规则", "New price rule") })
                        .text_size(px(21.)).font_weight(FontWeight::BOLD).text_color(rgb(INK)))
                    .child(Tag::custom(rgb(SURFACE).into(), rgb(0x4c566d).into(), rgb(SURFACE).into())
                        .small().h(px(27.)).rounded(px(9.)).px(px(14.)).text_size(px(11.))
                        .child(self.t("每百万 Token", "Per million tokens"))))
                .child(div().text_size(px(12.)).line_height(px(17.)).text_color(rgb(MUTED)).child(self.t(
                    "为指定供应商及上游模型设置估算价格，保存后仅影响新请求。",
                    "Set estimated prices for a provider and upstream model. Saving affects new requests only.")))
                .child(div().h(px(1.)).w_full().bg(rgb(BORDER))))
            .child(form)
            .child(div().flex().flex_col().flex_none().gap(px(10.)).h(px(footer_height))
                .border_t_1().border_color(rgb(BORDER)).pt(px(12.))
                .when_some(self.price_draft.error.clone(), |footer, error| footer.child(
                    div().text_size(px(12.)).line_height(px(16.)).text_color(rgb(0xc94f5e)).child(error)))
                .child(Label::new(self.t("不修改历史请求：已有调用仍保留原有价格快照", "Existing requests keep their original price snapshots"))
                    .text_size(px(11.)).text_color(rgb(MUTED)))
                .child(div().flex().items_center().justify_end().gap(px(9.))
                    .child(stats_button("price-cancel").outline().h(px(36.)).min_w(px(82.)).rounded(px(9.))
                        .label(self.t("取消", "Cancel"))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.price_closing = false;
                            this.cancel_price(window, cx);
                            window.close_dialog(cx);
                        })))
                    .child(stats_button("price-save").primary().h(px(36.)).min_w(px(98.)).rounded(px(9.))
                        .label(if self.price_draft.pending { self.t("保存中…", "Saving…") }
                            else if self.price_draft.is_editing() { self.t("保存修改", "Save changes") }
                            else { self.t("保存规则", "Save rule") })
                        .disabled(self.price_draft.pending)
                        .on_click(cx.listener(|this, _, _, cx| this.save_price(cx))))))
            .into_any_element()
    }
}

struct PriceBody {
    owner: WeakEntity<Statistics>,
}

impl Render for PriceBody {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.owner
            .update(cx, |this, cx| this.price_body(window, cx))
            .unwrap_or_else(|_| div().into_any_element())
    }
}
