use super::*;
use gpui_kit::component::popover::Popover;

impl Proxy {
    pub(super) fn subscribe_skin_focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        cx.on_focus_out(&self.skin_focus, window, |this, _, window, cx| {
            if this.skin_open && !this.skin_focus.contains_focused(window, cx) {
                // This is a non-modal popover: Tab may leave it, but a surface
                // with no keyboard focus must not linger and lose Escape.
                // Keep the newly focused control; Kit owns normal dismissal
                // and trigger restoration while focus remains inside.
                this.skin_open = false;
                this.skin_error = false;
                cx.notify();
            }
        })
        .detach();
    }

    pub(super) fn skin_control(&self, cx: &mut Context<Self>) -> AnyElement {
        let owner = cx.entity().downgrade();
        let content_owner = owner.clone();
        let studio = crate::theme::current_skin() == crate::theme::Skin::StudioIndigo;
        Popover::new("skin-settings")
            .open(self.skin_open)
            .track_focus(&self.skin_focus)
            .when(studio, |popover| {
                // The native positioner measures the complete wrapper, keeping
                // the visible surface 37px from the window edge at both sizes.
                popover.appearance(false).offset(px(19.)).w(px(379.))
            })
            .trigger(crate::ui_components::system_button(
                "skin-settings-trigger",
                "studio/skin.svg",
                self.t("皮肤设置", "Appearance"),
                self.t("皮肤设置", "Appearance"),
                false,
            ))
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
        if crate::theme::current_skin() == crate::theme::Skin::StudioIndigo {
            return self.studio_skin_panel(cx);
        }
        let selected = crate::theme::current_skin();
        div()
            .track_focus(&self.skin_focus)
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
                        .when(skin == crate::theme::Skin::StudioIndigo, |button| {
                            button.col_span(2)
                        })
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

    fn studio_skin_panel(&self, cx: &mut Context<Self>) -> AnyElement {
        use crate::theme::Skin;
        use crate::theme::color;
        let selected = crate::theme::current_skin();
        let grid = div()
            .grid()
            .grid_cols(2)
            .gap_x(px(6.))
            .gap_y(px(10.))
            .w(px(314.))
            .mt(px(25.))
            .children(Skin::ALL.into_iter().map(|skin| {
                let active = selected == skin;
                let label = skin.label(self.zh);
                let swatch = match skin {
                    Skin::Monochrome => Some("studio/skin-mono.svg"),
                    Skin::Blue => Some("studio/skin-blue.svg"),
                    Skin::Violet => Some("studio/skin-violet.svg"),
                    Skin::Rose => Some("studio/skin-rose.svg"),
                    Skin::Amber => Some("studio/skin-amber.svg"),
                    Skin::Teal => Some("studio/skin-teal.svg"),
                    Skin::StudioIndigo => None,
                };
                crate::ui_components::action_button(("skin-preset", skin as usize))
                    .relative()
                    .w_full()
                    .h(px(66.))
                    .p_0()
                    .rounded(px(11.))
                    .custom(
                        gpui_kit::component::button::ButtonCustomVariant::new(cx)
                            .color(
                                rgb(if active {
                                    color::ACCENT_TINT
                                } else {
                                    color::WHITE
                                })
                                .into(),
                            )
                            .foreground(
                                rgb(if active {
                                    crate::theme::palette().dark
                                } else {
                                    color::TEXT_2
                                })
                                .into(),
                            )
                            .hover(rgb(color::BG_NOTICE).into())
                            .active(rgb(color::ACCENT_TINT).into()),
                    )
                    .border_1()
                    .border_color(rgb(if active {
                        color::ACCENT_LINE
                    } else {
                        color::BORDER
                    }))
                    .when(skin == Skin::StudioIndigo, |button| button.col_span(2))
                    .accessibility_label(label)
                    .toggled(active)
                    .child(
                        div()
                            .absolute()
                            .left(px(16.))
                            .top(px(20.))
                            .size(px(22.))
                            .when_some(swatch, |slot, path| {
                                slot.child(
                                    svg()
                                        .path(path)
                                        .size(px(22.))
                                        .text_color(rgb(skin.palette().primary)),
                                )
                            })
                            .when(swatch.is_none(), |slot| {
                                slot.rounded_full().bg(rgb(skin.palette().primary))
                            }),
                    )
                    .child(
                        Label::new(label)
                            .absolute()
                            .left(px(48.))
                            .top(px(18.))
                            .text_size(px(13.))
                            .line_height(px(19.))
                            .font_weight(if active {
                                FontWeight::BOLD
                            } else {
                                FontWeight::MEDIUM
                            })
                            .text_color(rgb(if active {
                                crate::theme::palette().dark
                            } else {
                                color::TEXT_2
                            })),
                    )
                    .when(active, |button| {
                        button.child(
                            Label::new("✓")
                                .absolute()
                                .right(px(4.))
                                .top(px(19.))
                                .w(px(17.))
                                .text_size(px(13.))
                                .line_height(px(19.))
                                .font_weight(FontWeight::BOLD)
                                .text_color(rgb(crate::theme::palette().dark)),
                        )
                    })
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
            }));
        div()
            .track_focus(&self.skin_focus)
            .w(px(350.))
            .flex()
            .flex_col()
            .px(px(16.))
            .pt(px(18.))
            .pb(px(12.))
            .border_1()
            .border_color(rgb(color::BORDER))
            .rounded(px(16.))
            .bg(rgb(color::WHITE))
            .shadow(vec![BoxShadow {
                color: rgba(color::POPOVER_SHADOW_RGBA).into(),
                offset: point(px(0.), px(10.)),
                blur_radius: px(30.),
                spread_radius: px(0.),
                inset: false,
            }])
            .child(
                Label::new(self.t("外观与主题", "Appearance & theme"))
                    .ml(px(2.))
                    .text_size(px(17.))
                    .line_height(px(24.))
                    .font_weight(FontWeight::BOLD)
                    .text_color(rgb(INK)),
            )
            .child(
                Label::new(self.t(
                    "切换主题色并自动保存；无需重启代理",
                    "Themes save automatically; no proxy restart needed",
                ))
                .ml(px(2.))
                .mt(px(6.))
                .text_size(px(12.))
                .line_height(px(17.))
                .font_weight(FontWeight::NORMAL)
                .text_color(rgb(MUTED)),
            )
            .child(grid)
            .child(
                Label::new(self.t(
                    "Studio V2 使用统一主题变量，其他皮肤沿用 GPUI 现有配置。",
                    "Studio V2 uses shared theme tokens; other skins keep their GPUI settings.",
                ))
                .ml(px(2.))
                .mt(px(18.))
                .text_size(px(11.))
                .line_height(px(16.))
                .font_weight(FontWeight::NORMAL)
                .text_color(rgb(MUTED)),
            )
            .when(self.skin_error, |panel| {
                panel.child(
                    Label::new(self.t(
                        "保存失败，请检查配置目录的写入权限",
                        "Could not save. Check configuration folder permissions.",
                    ))
                    .text_size(px(12.))
                    .text_color(rgb(crate::theme::color::DANGER)),
                )
            })
            .into_any_element()
    }
}
