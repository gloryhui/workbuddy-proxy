//! Studio collection/storage card; existing revisioned handlers stay authoritative.
use super::*;
use gpui_kit::component::{
    menu::{DropdownMenu, PopupMenuItem},
    scroll::{ScrollableElement, ScrollableMask},
    tag::Tag,
};

/// Local Studio status chips; legacy theme keys and global tag variants stay intact.
pub(super) fn status_badge(
    kind: crate::ui_components::TagKind,
    label: impl Into<SharedString>,
) -> Tag {
    use crate::ui_components::TagKind;
    let (surface, ink) = match kind {
        TagKind::Success => (crate::theme::color::SUCCESS_TINT, OK),
        TagKind::Warning => (0xfff7ed, WARN),
        TagKind::Danger => (0xfff0f0, 0xc94f5e),
        TagKind::Primary => (crate::theme::palette().tint, crate::theme::palette().dark),
        TagKind::Secondary => (crate::theme::color::BG_TABLE_HEAD, MUTED),
    };
    Tag::custom(rgb(surface).into(), rgb(ink).into(), rgb(surface).into())
        .small()
        .h(px(26.))
        .rounded(px(8.))
        .px(px(12.))
        .text_size(px(11.))
        .child(label.into().to_string())
}

impl Statistics {
    pub(super) fn maintenance_panel(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.clear_preview.is_none()
            && self.maintenance.is_none()
            && self.maintenance_error.is_none()
        {
            return None;
        }
        let progress = self
            .maintenance
            .clone()
            .filter(|task| task.status == MaintenanceStatus::Running);
        let maintenance_label = self.t("维护任务", "Maintenance");
        let rows_label = self.t("行已处理", "rows processed");
        let danger = div()
            .flex()
            .flex_col()
            .gap_2()
            .w_full()
            .child(div().h(px(1.)).w_full().bg(rgb(BORDER)).mt_2())
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        Label::new(self.t("危险操作", "Danger zone"))
                            .text_size(px(crate::theme::font::LABEL))
                            .text_color(rgb(MUTED)),
                    )
                    .child(
                        crate::ui_components::button(
                            "clear-preview-again",
                            crate::ui_components::ButtonKind::DangerOutline,
                            self.t("重新预览", "Preview again"),
                            false,
                            false,
                        )
                        .on_click(cx.listener(|this, _, _, cx| this.preview_clear(cx))),
                    ),
            )
            .when_some(self.maintenance_error.clone(), |this, message| {
                this.child(
                    Label::new(message)
                        .text_size(px(crate::theme::font::LABEL))
                        .text_color(rgb(0xa4424a)),
                )
            })
            .when_some(progress, |this, task| {
                this.child(
                    Label::new(format!(
                        "{} {} · {} {}",
                        maintenance_label, task.kind, task.affected_rows, rows_label
                    ))
                    .text_size(px(crate::theme::font::LABEL))
                    .text_color(rgb(MUTED)),
                )
            })
            .when_some(self.clear_preview.clone(), |this, preview| {
                let token = preview.confirmed;
                this.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            Label::new(format!(
                                "{} {}",
                                if token {
                                    self.t("将删除已结束的", "Deletes finished")
                                } else {
                                    self.t("预览：将删除", "Preview: would delete")
                                },
                                preview.row_count
                            ))
                            .text_size(px(crate::theme::font::LABEL))
                            .text_color(rgb(0xa4424a)),
                        )
                        .child(
                            Label::new(self.t(
                                "保留去重标记，不修改 WorkBuddy 源文件。",
                                "Deduplication markers remain; WorkBuddy source files are unchanged.",
                            ))
                            .text_size(px(crate::theme::font::LABEL))
                            .text_color(rgb(MUTED)),
                        )
                        .child(
                            stats_button("clear-confirm")
                                .danger()
                                .label(self.t("确认清除", "Confirm clear"))
                                .on_click(cx.listener(|this, _, _, cx| this.confirm_clear(cx))),
                        ),
                )
            });

        Some(
            stats_card("stats-maintenance")
                .anchor_scroll(Some(self.maintenance_anchor.clone()))
                .child(danger)
                .when_some(
                    self.maintenance
                        .clone()
                        .filter(|task| task.status != MaintenanceStatus::Running),
                    |panel, task| {
                        panel.child(
                            Label::new(format!(
                                "{} · {} {}",
                                if task.status == MaintenanceStatus::Completed {
                                    self.t("清理已完成", "Cleanup completed")
                                } else {
                                    self.t("清理失败", "Cleanup failed")
                                },
                                task.affected_rows,
                                self.t("行已处理", "rows processed")
                            ))
                            .text_size(px(12.))
                            .text_color(rgb(
                                if task.status == MaintenanceStatus::Completed {
                                    OK
                                } else {
                                    WARN
                                },
                            )),
                        )
                    },
                )
                .when(self.clear_preview.is_some(), |panel| {
                    panel.child(
                        stats_button("clear-cancel")
                            .ghost()
                            .label(self.t("取消预览", "Cancel preview"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.clear_preview = None;
                                this.clear_token = None;
                                cx.notify();
                            })),
                    )
                })
                .into_any_element(),
        )
    }
}

impl Statistics {
    pub(super) fn settings_capture_import(&self, cx: &mut Context<Self>) -> AnyElement {
        let compact = self.filters_width < STORAGE_SIDE_BY_SIDE_WIDTH;
        div()
            .flex()
            .w_full()
            .gap(px(18.))
            .flex_shrink_0()
            .when(compact, |row| row.flex_col())
            .child(
                self.capture_card(cx)
                    .flex_grow(0.95)
                    .flex_basis(px(0.))
                    .flex_shrink_0()
                    .when(compact, |card| card.flex_basis(Length::Auto)),
            )
            .child(
                self.import_controls(cx)
                    .flex_1()
                    .flex_basis(px(0.))
                    .flex_shrink_0()
                    .when(compact, |card| card.flex_basis(Length::Auto)),
            )
            .into_any_element()
    }

    fn capture_card(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let studio = crate::theme::current_skin() == crate::theme::Skin::StudioIndigo;
        let settings = self.settings.as_ref();
        let (enabled, retention, revision) =
            settings.map_or((false, 0, 0), |s| (s.enabled, s.retention_days, s.revision));
        let mut mode_rows = div()
            .id("capture-provider-rows")
            .flex()
            .flex_col()
            .gap(px(6.));
        let page = self
            .configured_providers
            .iter()
            .enumerate()
            .skip(self.storage_page * self.storage_page_size)
            .take(self.storage_page_size)
            .collect::<Vec<_>>();
        for pair in page.chunks(2) {
            mode_rows = mode_rows.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .h(px(32.))
                    .flex_shrink_0()
                    .children(pair.iter().map(|(index, (id, name))| {
                        let value = self.provider_modes.get(id);
                        let label = if name.is_empty() {
                            id.clone()
                        } else {
                            name.clone()
                        };
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .flex_1()
                            .min_w_0()
                            .child(
                                Label::new(label.clone())
                                    .text_size(px(12.))
                                    .text_color(rgb(INK))
                                    .flex_1()
                                    .min_w_0()
                                    .text_ellipsis(),
                            )
                            .child(self.capture_mode_button(
                                *index,
                                id.clone(),
                                label,
                                value.cloned(),
                                cx,
                            ))
                    })),
            );
        }
        let mode_count = page.len().div_ceil(2);
        let height =
            mode_count.clamp(1, 2) as f32 * 32. + mode_count.saturating_sub(1).min(1) as f32 * 6.;
        let mode_area = div()
            .id("capture-provider-body")
            .w_full()
            .h(px(height))
            .relative()
            .flex_none()
            .child(
                mode_rows
                    .when(mode_count > 2, |rows| rows.pr(px(14.)))
                    .h(px(height))
                    .flex_none()
                    .track_scroll(&self.provider_modes_scroll)
                    .overflow_y_scroll()
                    .when(page.is_empty(), |rows| {
                        rows.child(
                            Label::new(self.t("暂无供应商", "No providers"))
                                .text_size(px(12.))
                                .text_color(rgb(MUTED)),
                        )
                    }),
            )
            .when(mode_count > 2, |body| {
                body.child(ScrollableMask::new(
                    Axis::Vertical,
                    &self.provider_modes_scroll,
                ))
            })
            .vertical_scrollbar(&self.provider_modes_scroll);
        stats_card("stats-capture")
            .anchor_scroll(Some(self.settings_anchors[1].clone()))
            .p(px(20.))
            .gap(px(14.))
            .min_w_0()
            .min_h(px(292.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(8.))
                    .child(
                        Label::new(self.t("采集与存储", "Collection & storage"))
                            .text_size(px(17.))
                            .font_weight(FontWeight::BOLD)
                            .text_color(rgb(INK)),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(status_badge(
                                if settings.is_some() && enabled {
                                    crate::ui_components::TagKind::Success
                                } else {
                                    crate::ui_components::TagKind::Secondary
                                },
                                if settings.is_none() {
                                    self.t("读取中…", "Loading…")
                                } else if enabled {
                                    self.t("已启用", "Enabled")
                                } else {
                                    self.t("已暂停", "Paused")
                                },
                            ))
                            .child(self.capture_actions(cx)),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        Label::new(self.t("默认货币", "Default currency"))
                            .w(px(if studio && self.zh { 150. } else { 128. }))
                            .flex_shrink_0()
                            .text_size(px(12.))
                            .text_color(rgb(MUTED)),
                    )
                    .child(self.default_currency_control(cx)),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        Label::new(self.t("摘要采集", "Summary collection"))
                            .w(px(if studio && self.zh { 150. } else { 128. }))
                            .flex_shrink_0()
                            .text_size(px(12.))
                            .text_color(rgb(MUTED)),
                    )
                    .child(
                        Checkbox::new("collect-enabled")
                            .large()
                            .checked(enabled)
                            .disabled(settings.is_none())
                            .label(self.t("启用摘要采集", "Enable summary collection"))
                            .on_change(cx.listener(move |this, value: &bool, _, cx| {
                                this.update_settings(*value, retention, revision, cx)
                            })),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap(px(8.))
                    .child(
                        Label::new(self.t("数据保留", "Data retention"))
                            .w(px(if studio && self.zh { 150. } else { 128. }))
                            .flex_shrink_0()
                            .text_size(px(12.))
                            .text_color(rgb(MUTED))
                            .pt(px(8.)),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(6.))
                            .flex_1()
                            .min_w_0()
                            .child(
                                div().flex().flex_wrap().gap(px(6.)).children(
                                    [
                                        (30i64, self.t("30 天", "30 days")),
                                        (90, self.t("90 天", "90 days")),
                                        (180, self.t("180 天", "180 days")),
                                        (365, self.t("365 天", "365 days")),
                                        (0, self.t("永久", "Forever")),
                                    ]
                                    .into_iter()
                                    .map(|(days, label)| {
                                        stats_button(if days == 0 {
                                            "retention-forever".to_owned()
                                        } else {
                                            format!("retention-{days}")
                                        })
                                        .h(px(34.))
                                        .when(studio, |button| button.flex_1())
                                        .px(px(8.))
                                        .label(label)
                                        .disabled(settings.is_none())
                                        .when(settings.is_some() && retention == days, |button| {
                                            button.primary().outline()
                                        })
                                        .when(settings.is_none() || retention != days, |button| {
                                            button.ghost()
                                        })
                                        .when(studio, |button| {
                                            button
                                                .text_size(px(11.))
                                                .bg(rgb(if retention == days {
                                                    crate::theme::color::ACCENT_TINT
                                                } else {
                                                    0xf3f5fa
                                                }))
                                                .border_color(rgb(if retention == days {
                                                    crate::theme::color::ACCENT_LINE
                                                } else {
                                                    0xf3f5fa
                                                }))
                                        })
                                        .on_click(
                                            cx.listener(move |this, _, _, cx| {
                                                this.update_settings(enabled, days, revision, cx)
                                            }),
                                        )
                                    }),
                                ),
                            )
                            .when(
                                settings.is_some() && ![0, 30, 90, 180, 365].contains(&retention),
                                |column| {
                                    column.child(
                                        Label::new(format!(
                                            "{} {retention} {}",
                                            self.t("当前", "Current"),
                                            self.t("天", "days")
                                        ))
                                        .text_size(px(11.))
                                        .text_color(rgb(MUTED)),
                                    )
                                },
                            ),
                    ),
            )
            .child(div().w_full().h(px(1.)).bg(rgb(BORDER)))
            .child(
                Label::new(self.t("供应商用量采集方式", "Provider usage capture mode"))
                    .text_size(px(12.))
                    .text_color(rgb(MUTED)),
            )
            .child(mode_area)
            .when(
                self.configured_providers.len() > self.storage_page_size,
                |card| card.child(self.capture_pagination(cx)),
            )
    }

    fn default_currency_control(&self, cx: &mut Context<Self>) -> AnyElement {
        let selected = self.settings.as_ref().map(|s| s.default_currency);
        let owner = cx.entity().downgrade();
        stats_button("stats-default-currency")
            .outline()
            .h(px(38.))
            .w(px(125.))
            .label(format!(
                "{}  ⌄",
                selected.map_or("—", |value| value.as_str())
            ))
            .disabled(selected.is_none())
            .tooltip(self.t(
                "仅影响新请求，历史价格快照保持不变",
                "New requests only; recorded price snapshots stay unchanged",
            ))
            .dropdown_menu_with_anchor(Anchor::TopLeft, move |mut menu, _, _| {
                for value in [Currency::Cny, Currency::Usd] {
                    let owner = owner.clone();
                    menu = menu.item(
                        PopupMenuItem::element(move |_, _| {
                            Label::new(value.as_str()).text_size(px(13.))
                        })
                        .checked(selected == Some(value))
                        .disabled(selected == Some(value))
                        .on_click(move |_, _, cx| {
                            let _ =
                                owner.update(cx, |this, cx| this.save_default_currency(value, cx));
                        }),
                    );
                }
                menu.min_w(px(125.))
            })
            .into_any_element()
    }

    fn capture_mode_button(
        &self,
        index: usize,
        id: String,
        name: String,
        settings: Option<ProviderSettingsDto>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let value = settings.as_ref().map(|s| s.usage_mode);
        let revision = settings.as_ref().map_or(0, |s| s.revision);
        let owner = cx.entity().downgrade();
        let request = value == Some(UsageMode::Request);
        stats_button(("provider-mode", index))
            .h(px(28.))
            .px(px(10.))
            .custom(
                gpui_kit::component::button::ButtonCustomVariant::new(cx)
                    .color(
                        rgb(if request {
                            crate::theme::color::SUCCESS_TINT
                        } else {
                            crate::theme::color::BG_TABLE_HEAD
                        })
                        .into(),
                    )
                    .foreground(rgb(if request { OK } else { MUTED }).into())
                    .hover(rgb(crate::theme::palette().tint).into()),
            )
            .label(value.map_or_else(|| "—".to_owned(), |mode| mode.as_str().to_uppercase()))
            .accessibility_label(format!(
                "{name} · {}",
                self.t("用量采集方式", "Usage capture mode")
            ))
            .disabled(value.is_none() || self.backend.is_busy(&JobKey::ProviderMode(id.clone())))
            .dropdown_menu_with_anchor(Anchor::TopRight, move |mut menu, _, _| {
                for (mode, title) in [
                    (UsageMode::Request, "REQUEST"),
                    (UsageMode::Observe, "OBSERVE"),
                ] {
                    let owner = owner.clone();
                    let id = id.clone();
                    menu = menu.item(
                        PopupMenuItem::element(move |_, _| Label::new(title).text_size(px(13.)))
                            .checked(value == Some(mode))
                            .disabled(value == Some(mode))
                            .on_click(move |_, _, cx| {
                                let _ = owner.update(cx, |this, cx| {
                                    this.set_provider_mode(
                                        id.clone(),
                                        mode == UsageMode::Request,
                                        revision,
                                        cx,
                                    )
                                });
                            }),
                    );
                }
                menu.min_w(px(140.))
            })
            .into_any_element()
    }

    fn capture_actions(&self, cx: &mut Context<Self>) -> AnyElement {
        let owner = cx.entity().downgrade();
        let selected = self.storage_page_size;
        let zh = self.zh;
        stats_button("capture-actions").ghost().h(px(28.)).w(px(28.)).p_0().label("⋯")
            .accessibility_label(self.t("采集与存储操作", "Collection and storage actions"))
            .tooltip(self.t("暂停只影响新请求，已接纳请求仍结算；币种变更不改历史。刷新与页大小见菜单。", "Pause affects new requests; admitted requests still settle. Currency changes preserve history. Reload and page sizes are in this menu."))
            .dropdown_menu_with_anchor(Anchor::TopRight, move |mut menu, _, _| {
                for size in [0usize, 20, 50, 100] {
                    let owner = owner.clone();
                    let title = if size == 0 { if zh { "刷新供应商采集模式".into() } else { "Reload provider modes".into() } }
                        else if zh { format!("{size} 条/页") } else { format!("{size} per page") };
                    menu = menu.item(PopupMenuItem::element(move |_, _| Label::new(title.clone()).text_size(px(13.)))
                        .checked(size > 0 && selected == size).on_click(move |_, _, cx| {
                            let _ = owner.update(cx, |this, cx| {
                                if size == 0 {
                                    for (id, _) in this.configured_providers.iter().skip(this.storage_page * this.storage_page_size).take(this.storage_page_size) {
                                        this.provider_modes.remove(id);
                                    }
                                } else { this.storage_page_size = size; this.storage_page = 0; }
                                this.provider_modes_scroll.set_offset(point(px(0.), px(0.)));
                                this.request_provider_modes(cx); cx.notify();
                            });
                        }));
                }
                menu.min_w(px(200.))
            }).into_any_element()
    }

    fn capture_pagination(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .flex()
            .items_center()
            .flex_wrap()
            .justify_between()
            .gap(px(6.))
            .child(
                Label::new(format!(
                    "{} {}/{}",
                    self.t("第", "Page"),
                    self.storage_page + 1,
                    self.configured_providers
                        .len()
                        .div_ceil(self.storage_page_size)
                        .max(1)
                ))
                .text_size(px(11.))
                .text_color(rgb(MUTED)),
            )
            .child(
                div()
                    .flex()
                    .gap(px(4.))
                    .child(
                        stats_button("storage-prev")
                            .ghost()
                            .h(px(24.))
                            .label(self.t("上一页", "Previous"))
                            .disabled(self.storage_page == 0)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.storage_page = this.storage_page.saturating_sub(1);
                                this.provider_modes_scroll.set_offset(point(px(0.), px(0.)));
                                this.request_provider_modes(cx);
                                cx.notify();
                            })),
                    )
                    .child(
                        stats_button("storage-next")
                            .ghost()
                            .h(px(24.))
                            .label(self.t("下一页", "Next"))
                            .disabled(
                                (self.storage_page + 1) * self.storage_page_size
                                    >= self.configured_providers.len(),
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.storage_page += 1;
                                this.provider_modes_scroll.set_offset(point(px(0.), px(0.)));
                                this.request_provider_modes(cx);
                                cx.notify();
                            })),
                    ),
            )
            .into_any_element()
    }
}
