use super::*;
use gpui_kit::component::popover::Popover;

impl Proxy {
    pub(super) fn skin_control(&self, cx: &mut Context<Self>) -> AnyElement {
        let owner = cx.entity().downgrade();
        let content_owner = owner.clone();
        Popover::new("skin-settings")
            .open(self.skin_open)
            .trigger(
                crate::ui_components::icon_button(
                    "skin-settings-trigger",
                    Icon::default().path("icons/skin-shirt.svg"),
                    self.t("皮肤设置", "Appearance"),
                    self.t("皮肤设置", "Appearance"),
                    false,
                )
                .text_color(rgb(0x5c6679)),
            )
            .on_open_change(move |open, _, cx| {
                let _ = owner.update(cx, |this, cx| {
                    this.skin_open = *open;
                    this.skin_error = false;
                    cx.notify();
                });
            })
            .content(move |_, _, cx| {
                content_owner
                    .update(cx, |this, cx| this.skin_panel(cx))
                    .unwrap_or_else(|_| div().into_any_element())
            })
            .into_any_element()
    }

    fn skin_panel(&self, cx: &mut Context<Self>) -> AnyElement {
        let selected = crate::theme::current_skin();
        div()
            .w(px(320.))
            .flex()
            .flex_col()
            .gap(px(12.))
            .p(px(8.))
            .child(
                Label::new(self.t("皮肤设置", "Appearance"))
                    .text_size(px(crate::theme::font::CARD_TITLE))
                    .font_weight(FontWeight::SEMIBOLD),
            )
            .child(
                Label::new(self.t(
                    "选择主题色，即时生效并自动保存",
                    "Choose a color; applied and saved immediately",
                ))
                .text_size(px(crate::theme::font::LABEL))
                .text_color(rgb(MUTED)),
            )
            .child(div().grid().grid_cols(2).gap(px(8.)).children(
                crate::theme::Skin::ALL.into_iter().map(|skin| {
                    let colors = skin.palette();
                    let label = skin.label(self.zh);
                    crate::ui_components::action_button(("skin-preset", skin as usize))
                        .w_full()
                        .h(px(56.))
                        .px(px(10.))
                        .bg(rgb(if selected == skin {
                            colors.tint
                        } else {
                            0xffffff
                        }))
                        .border_color(rgb(if selected == skin {
                            colors.primary
                        } else {
                            crate::theme::color::BORDER
                        }))
                        .accessibility_label(label)
                        .toggled(selected == skin)
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(8.))
                                .child(
                                    div()
                                        .size(px(20.))
                                        .flex_shrink_0()
                                        .rounded_full()
                                        .bg(rgb(colors.primary)),
                                )
                                .child(Label::new(label).text_size(px(crate::theme::font::BODY)))
                                .when(selected == skin, |row| {
                                    row.child(
                                        crate::ui_components::action_icon(IconName::Check)
                                            .text_color(rgb(colors.dark)),
                                    )
                                }),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Err(_error) = crate::theme::save_skin(&this.dir, skin) {
                                this.skin_error = true;
                            } else {
                                this.skin_error = false;
                                crate::theme::apply_skin(skin, cx);
                                this.statistics.update(cx, |_, cx| cx.notify());
                            }
                            cx.notify();
                        }))
                }),
            ))
            .when(self.skin_error, |panel| {
                panel.child(
                    Label::new(self.t(
                        "保存失败，请检查配置目录的写入权限",
                        "Could not save. Check configuration folder permissions.",
                    ))
                    .text_size(px(crate::theme::font::LABEL))
                    .text_color(rgb(crate::theme::color::DANGER)),
                )
            })
            .into_any_element()
    }
}
