//! Studio settings navigation and cost-pricing layout.
use super::*;
use gpui_kit::component::{
    menu::{DropdownMenu, PopupMenuItem},
    scroll::{ScrollableElement, ScrollableMask},
};

impl Statistics {
    pub(super) fn pricing_section(&self, cx: &mut Context<Self>) -> AnyElement {
        let studio = crate::theme::current_skin() == crate::theme::Skin::StudioIndigo;
        let sync_note = self.price_sync.as_ref().and_then(|sync| {
            sync.last_success_day.as_ref().map(|day| {
                format!(
                    "{} {day} · {} {} · {} {}",
                    self.t("价格更新于", "Prices updated"),
                    self.t("更新", "Updated"),
                    sync.updated_count,
                    self.t("未匹配", "Unmatched"),
                    sync.unmatched_count,
                )
            })
        });
        // DESIGN_SPEC §4.2 price table: model (+ rule id), source/mapping,
        // currency, input/output price per million, actions. Data semantics are
        // unchanged: the same DTO fields and the same save/delete callbacks.
        let header = div()
            .flex()
            .items_center()
            .h(px(36.))
            .px(px(12.))
            .gap(px(16.))
            .bg(rgb(crate::theme::color::BG_TABLE_HEAD))
            .when(!studio, |header| {
                header.border_b(px(1.)).border_color(rgb(BORDER))
            })
            .text_size(px(if studio {
                11.
            } else {
                crate::theme::font::LABEL
            }))
            .when(studio, |header| {
                header.line_height(px(16.)).font_weight(FontWeight::MEDIUM)
            })
            .text_color(rgb(MUTED))
            .child(
                div()
                    .flex_1()
                    .when(studio, |col| col.flex_grow(252.))
                    .min_w_0()
                    .child(self.t("模型 / ID", "Model / ID")),
            )
            .child(
                div()
                    .flex_1()
                    .when(studio, |col| col.flex_grow(300.))
                    .min_w_0()
                    .child(self.t("来源 / 上游映射", "Source / upstream mapping")),
            )
            .child(
                div()
                    .w(px(if studio { 93. } else { 80. }))
                    .child(self.t("币种", "Currency")),
            )
            .child(
                div()
                    .flex_1()
                    .when(studio, |col| col.flex_grow(243.))
                    .min_w_0()
                    .child(self.t("输入 / 输出单价", "Input / output price")),
            )
            .child(
                div()
                    .w(px(110.))
                    .flex()
                    .justify_end()
                    .child(self.t("操作", "Actions")),
            );
        let table = div()
            .flex()
            .flex_col()
            .w_full()
            .min_w_0()
            .when(!studio, |table| table.border_1().border_color(rgb(BORDER)))
            .rounded(px(8.))
            .overflow_hidden()
            .child(header);
        let row_height = if self.prices_loading || self.prices.is_empty() {
            48.
        } else {
            self.prices.len().min(3) as f32 * if studio { 48. } else { 44. }
        };
        let mut rows = div()
            .id("pricing-rule-rows")
            .w_full()
            .h(px(row_height))
            .flex_none()
            .min_h_0()
            .relative()
            .track_scroll(&self.price_rows_scroll)
            .overflow_y_scroll()
            .flex()
            .flex_col();
        if self.prices_loading {
            rows = rows.child(
                div().px(px(12.)).py(px(16.)).child(
                    Label::new(self.t("加载中…", "Loading…"))
                        .text_size(px(crate::theme::font::LABEL))
                        .text_color(rgb(MUTED)),
                ),
            );
        } else if self.prices.is_empty() {
            rows = rows.child(
                div().px(px(12.)).py(px(16.)).child(
                    Label::new(self.t(
                        "没有匹配的价格规则，可手动更新或新建。未匹配模型暂按默认币种记录0元。",
                        "No matching price rules. Update prices or add a rule; unmatched models default to zero.",
                    ))
                    .text_size(px(crate::theme::font::LABEL))
                    .text_color(rgb(MUTED)),
                ),
            );
        } else {
            for (index, rule) in self.prices.iter().enumerate() {
                let short_id: String = rule.rule_id.chars().take(12).collect();
                // The price DTO has no separate source column; the label is the
                // existing user-visible source/annotation. An empty label is the
                // manual rule, not a blank cell.
                let source_label = if rule.label.trim().is_empty() {
                    self.t("手工", "Manual").to_owned()
                } else {
                    rule.label.clone()
                };
                let provider_name = self
                    .configured_providers
                    .iter()
                    .find(|(id, _)| id == &rule.provider_id)
                    .map(|(_, name)| name.clone())
                    .filter(|name| !name.is_empty());
                let mapping_line = match provider_name {
                    Some(name) => format!("{name} / {}", rule.upstream_model),
                    None => rule.upstream_model.clone(),
                };
                rows = rows.child(
                    div()
                        .id(("price-row", index))
                        .tooltip({
                            let hint = format!(
                                "{}\n{}\n{}\n{}",
                                rule.upstream_model, rule.rule_id, mapping_line, rule.input_mode
                            );
                            move |window, cx| {
                                gpui_kit::component::tooltip::Tooltip::new(hint.clone())
                                    .build(window, cx)
                            }
                        })
                        .flex()
                        .items_center()
                        .gap(px(16.))
                        .px(px(12.))
                        .h(px(if studio { 48. } else { 44. }))
                        .when(studio && index % 2 == 1, |row| {
                            row.bg(rgb(crate::theme::color::BG_NOTICE))
                        })
                        .flex_shrink_0()
                        .py(px(6.))
                        .border_b(px(1.))
                        .border_color(rgb(if studio { 0xe7eaf2 } else { 0xeef2f7 }))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .flex_1()
                                .when(studio, |col| col.flex_grow(252.))
                                .min_w_0()
                                .gap(px(2.))
                                .child(
                                    Label::new(rule.upstream_model.clone())
                                        .w_full()
                                        .text_ellipsis()
                                        .text_size(px(13.))
                                        .when(studio, |model| {
                                            model
                                                .line_height(px(18.))
                                                .font_weight(FontWeight::MEDIUM)
                                        })
                                        .text_color(rgb(INK)),
                                )
                                .child(
                                    Label::new(short_id)
                                        .text_size(px(if studio {
                                            10.
                                        } else {
                                            crate::theme::font::LABEL
                                        }))
                                        .when(studio, |id| {
                                            crate::studio_fonts::latin(id, cx)
                                                .line_height(px(14.))
                                                .font_weight(FontWeight::NORMAL)
                                        })
                                        .text_color(rgb(MUTED)),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .flex_1()
                                .when(studio, |col| col.flex_grow(300.))
                                .min_w_0()
                                .gap(px(2.))
                                .child(
                                    Label::new(source_label)
                                        .w_full()
                                        .text_ellipsis()
                                        .text_size(px(crate::theme::font::LABEL))
                                        .when(studio, |source| {
                                            source
                                                .line_height(px(17.))
                                                .font_weight(FontWeight::MEDIUM)
                                        })
                                        .text_color(rgb(if studio {
                                            crate::theme::color::TEXT_2
                                        } else {
                                            INK
                                        })),
                                )
                                .child(
                                    Label::new(mapping_line)
                                        .w_full()
                                        .text_ellipsis()
                                        .text_size(px(if studio {
                                            11.
                                        } else {
                                            crate::theme::font::LABEL
                                        }))
                                        .when(studio, |mapping| {
                                            mapping
                                                .line_height(px(16.))
                                                .font_weight(FontWeight::NORMAL)
                                        })
                                        .text_color(rgb(MUTED)),
                                ),
                        )
                        .child(
                            div()
                                .w(px(if studio { 93. } else { 80. }))
                                .flex()
                                .items_center()
                                .child(if studio {
                                    super::settings_capture::status_badge(
                                        crate::ui_components::TagKind::Secondary,
                                        rule.currency.as_str(),
                                    )
                                    .w(px(53.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_color(rgb(crate::theme::color::TEXT_2))
                                    .into_any_element()
                                } else {
                                    crate::ui_components::tag(
                                        crate::ui_components::TagKind::Secondary,
                                        rule.currency.as_str(),
                                    )
                                    .into_any_element()
                                }),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .flex_1()
                                .when(studio, |col| col.flex_grow(243.))
                                .min_w_0()
                                .gap(px(2.))
                                .child(
                                    Label::new(format!(
                                        "{} / {}",
                                        usage_table::amount(
                                            Some(&rule.input_rate_nanos),
                                            Some(rule.currency)
                                        ),
                                        usage_table::amount(
                                            Some(&rule.output_rate_nanos),
                                            Some(rule.currency)
                                        ),
                                    ))
                                    .text_size(px(if studio { 13. } else { 12. }))
                                    .when(studio, |price| {
                                        crate::studio_fonts::latin(price, cx)
                                            .line_height(px(18.))
                                            .font_weight(FontWeight::SEMIBOLD)
                                    })
                                    .text_color(rgb(INK)),
                                )
                                .when(!studio, |column| {
                                    column.child(
                                        Label::new(format!("{}", rule.input_mode))
                                            .text_size(px(crate::theme::font::LABEL))
                                            .text_color(rgb(MUTED)),
                                    )
                                }),
                        )
                        .child(self.price_row_actions(index, rule, cx)),
                );
            }
        }
        // Match Kit's native table: consume vertical wheel events in capture
        // while the inner body can scroll, before an ancestor div also moves.
        let table = table.child(
            div()
                .id("pricing-rule-body")
                .w_full()
                .h(px(row_height))
                .flex_none()
                .relative()
                .child(rows)
                .when(!self.prices_loading && !self.prices.is_empty(), |body| {
                    body.child(ScrollableMask::new(Axis::Vertical, &self.price_rows_scroll))
                })
                .vertical_scrollbar(&self.price_rows_scroll),
        );
        let body = div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(
                div()
                    .flex()
                    .items_start()
                    .justify_between()
                    .flex_wrap()
                    .gap(px(12.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(5.))
                            .when(studio, |column| column.pl(px(4.)))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(14.))
                                    .child(
                                        Label::new(self.t("成本定价", "Cost pricing"))
                                            .text_size(px(17.))
                                            .when(studio, |title| {
                                                title.min_w(px(88.)).line_height(px(24.))
                                            })
                                            .font_weight(FontWeight::BOLD)
                                            .text_color(rgb(INK)),
                                    )
                                    .child(
                                        crate::ui_components::tag(
                                            crate::ui_components::TagKind::Secondary,
                                            if self.prices_loading {
                                                self.t("读取中…", "Loading…").to_owned()
                                            } else {
                                                format!(
                                                    "{} {}",
                                                    self.prices.len(),
                                                    self.t("条 / 本页", "on this page")
                                                )
                                            },
                                        )
                                        .when(
                                            studio,
                                            |tag| {
                                                tag.h(px(26.))
                                                    .min_w(px(86.))
                                                    .text_size(px(11.))
                                                    .line_height(px(16.))
                                                    .font_weight(FontWeight::MEDIUM)
                                                    .bg(rgb(crate::theme::color::BG_TABLE_HEAD))
                                                    .border_color(rgb(
                                                        crate::theme::color::BG_TABLE_HEAD,
                                                    ))
                                                    .text_color(rgb(MUTED))
                                            },
                                        ),
                                    )
                                    .when(studio, |row| row.child(self.pricing_actions(cx))),
                            )
                            .child(
                                Label::new(self.t(
                                    "每百万 Token · 不修改历史请求的价格快照",
                                    "Per million tokens · recorded price snapshots stay unchanged",
                                ))
                                .text_size(px(12.))
                                .when(studio, |subtitle| {
                                    subtitle
                                        .line_height(px(17.))
                                        .font_weight(FontWeight::NORMAL)
                                })
                                .text_color(rgb(MUTED)),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .flex_wrap()
                            .gap(px(10.))
                            .when(studio, |row| row.gap(px(11.)).pr(px(6.)))
                            .child(
                                crate::ui_components::text_input(&self.price_search, false)
                                    .w(px(if studio && self.filters_width >= 1000. {
                                        284.
                                    } else {
                                        240.
                                    }))
                                    .when(studio, |input| {
                                        input.h(px(36.)).text_size(px(11.)).line_height(px(16.))
                                    })
                                    .prefix(Icon::new(IconName::Search)),
                            )
                            .child(
                                stats_button("price-add-top")
                                    .primary()
                                    .h(px(36.))
                                    .when(studio && self.filters_width >= 1000., |button| {
                                        button.w(px(211.))
                                    })
                                    .icon(Icon::new(IconName::Plus).size(px(16.)))
                                    .label(self.t("新建规则", "New rule"))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.new_price(window, cx)
                                    })),
                            )
                            .when(!studio, |row| row.child(self.pricing_actions(cx))),
                    ),
            )
            .child(table)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(12.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .flex_1()
                            .min_w_0()
                            .when_some(sync_note.clone().filter(|_| studio), |left, note| {
                                let hint = note.clone();
                                left.child(
                                    div()
                                        .id("pricing-sync-note")
                                        .flex_1()
                                        .min_w_0()
                                        .tooltip(move |window, cx| {
                                            crate::ui_components::full_value_tooltip(
                                                hint.clone(),
                                                window,
                                                cx,
                                            )
                                        })
                                        .child(
                                            Label::new(note)
                                                .w_full()
                                                .text_ellipsis()
                                                .text_size(px(11.))
                                                .line_height(px(16.))
                                                .text_color(rgb(MUTED)),
                                        ),
                                )
                            })
                            .child(
                                Label::new(if self.zh {
                                    format!(
                                        "第 {} 页 · {} 条",
                                        self.prices_cursors.page_number(),
                                        self.prices.len()
                                    )
                                } else {
                                    format!(
                                        "Page {} · {} rows",
                                        self.prices_cursors.page_number(),
                                        self.prices.len()
                                    )
                                })
                                .text_size(px(crate::theme::font::LABEL))
                                .text_color(rgb(MUTED))
                                .flex_none()
                                .when(studio, |label| {
                                    label.text_size(px(11.)).line_height(px(16.))
                                }),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                stats_button("prices-prev")
                                    .when(studio, |button| button.h(px(24.)).text_size(px(11.)))
                                    .ghost()
                                    .label(self.t("上一页", "Previous"))
                                    .disabled(
                                        self.prices_loading || !self.prices_cursors.can_go_back(),
                                    )
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        if let Some(previous) = this.prices_cursors.back() {
                                            this.prices_cursors.begin_page(previous);
                                            this.request_price_page(
                                                format!("prices-{}", this.generation),
                                                cx,
                                            );
                                        }
                                    })),
                            )
                            .child(
                                stats_button("prices-page-size")
                                    .when(studio, |button| button.h(px(24.)).text_size(px(11.)))
                                    .ghost()
                                    .label(format!(
                                        "{} {}",
                                        self.prices_page_size,
                                        self.t("条/页", "per page")
                                    ))
                                    .disabled(self.prices_loading)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.prices_page_size = match this.prices_page_size {
                                            20 => 50,
                                            50 => 100,
                                            _ => 20,
                                        };
                                        this.request_prices(cx);
                                    })),
                            )
                            .child(
                                stats_button("prices-next")
                                    .when(studio, |button| button.h(px(24.)).text_size(px(11.)))
                                    .ghost()
                                    .label(self.t("下一页", "Next"))
                                    .disabled(
                                        self.prices_loading || !self.prices_cursors.can_go_next(),
                                    )
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        if let Some(next) = this.prices_cursors.forward() {
                                            this.prices_cursors.begin_page(next);
                                            this.request_price_page(
                                                format!("prices-{}", this.generation),
                                                cx,
                                            );
                                        }
                                    })),
                            ),
                    ),
            );
        stats_card("stats-pricing")
            .anchor_scroll(Some(self.settings_anchors[0].clone()))
            .child(body)
            .when_some(self.price_sync_error.clone(), |card, error| {
                card.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(4.))
                        .px(px(12.))
                        .py(px(8.))
                        .rounded(px(8.))
                        .bg(rgb(crate::theme::color::DANGER_BG))
                        .child(
                            Label::new(self.t(
                                "价格更新失败，已保留现有规则",
                                "Price update failed; existing rules retained",
                            ))
                            .text_size(px(12.))
                            .text_color(rgb(crate::theme::color::DANGER)),
                        )
                        .child(
                            Label::new(crate::i18n::message(&error, self.zh))
                                .text_size(px(12.))
                                .text_color(rgb(MUTED)),
                        ),
                )
            })
            .when_some(sync_note.filter(|_| !studio), |card, note| {
                card.child(Label::new(note).text_size(px(11.)).text_color(rgb(MUTED)))
            })
            .into_any_element()
    }
}

impl Statistics {
    pub(super) fn settings_navigation(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .flex()
            .items_center()
            .when(
                crate::theme::current_skin() == crate::theme::Skin::StudioIndigo,
                |tabs| tabs.h(px(53.)).pt(px(7.)).pb(px(6.)),
            )
            .gap(px(9.))
            .flex_shrink_0()
            .children(
                [
                    ("settings-pricing", self.t("成本定价", "Cost pricing"), 117.),
                    (
                        "settings-capture",
                        self.t("采集与存储", "Collection & storage"),
                        if self.zh { 142. } else { 170. },
                    ),
                    ("settings-import", self.t("本地用量", "Local usage"), 117.),
                ]
                .into_iter()
                .enumerate()
                .map(|(index, (id, label, width))| {
                    let active = self.settings_section == index;
                    stats_button(id)
                        .h(px(40.))
                        .w(px(width))
                        .p_0()
                        .custom(
                            gpui_kit::component::button::ButtonCustomVariant::new(cx)
                                .color(rgb(if active { PANEL } else { BG }).into())
                                .foreground(rgb(if active { INK } else { MUTED }).into())
                                .hover(rgb(crate::theme::color::BG_TABLE_HEAD).into())
                                .active(rgb(crate::theme::palette().tint).into()),
                        )
                        .toggled(active)
                        .accessibility_label(label)
                        .child(
                            div()
                                .relative()
                                .size_full()
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(
                                    Label::new(label)
                                        .text_size(px(13.))
                                        .text_color(rgb(if active { INK } else { MUTED }))
                                        .font_weight(if active {
                                            FontWeight::BOLD
                                        } else {
                                            FontWeight::MEDIUM
                                        }),
                                )
                                .when(active, |tab| {
                                    tab.child(
                                        div()
                                            .absolute()
                                            .bottom_0()
                                            .left(px(14.))
                                            .right(px(14.))
                                            .h(px(3.))
                                            .rounded(px(2.))
                                            .bg(rgb(crate::theme::palette().primary)),
                                    )
                                }),
                        )
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.settings_section = index;
                            let anchor = this.settings_anchors[index].clone();
                            // Allow an expanded section to lay out before locating it.
                            window.on_next_frame(move |window, cx| anchor.scroll_to(window, cx));
                            cx.notify();
                        }))
                }),
            )
            .into_any_element()
    }

    fn pricing_actions(&self, cx: &mut Context<Self>) -> AnyElement {
        let owner = cx.entity().downgrade();
        let pending = self.price_sync_pending;
        let zh = self.zh;
        stats_button("price-actions").ghost().h(px(36.)).w(px(32.)).label("⋯")
            .accessibility_label(self.t("价格操作", "Price actions"))
            .tooltip(self.t(
                "每日从models.dev更新会覆盖匹配的手工价格；历史金额保持不变。价格沿用当前供应商筛选。",
                "Daily models.dev updates overwrite matching manual prices; recorded costs stay unchanged. Prices follow the provider filter."))
            .dropdown_menu_with_anchor(Anchor::TopRight, move |mut menu, _, _| {
                for (sync, title) in [
                    (false, if zh { "刷新价格" } else { "Reload prices" }),
                    (true, if pending { if zh { "正在更新价格…" } else { "Updating prices…" } }
                        else if zh { "手动更新价格" } else { "Update prices now" }),
                ] {
                    let owner = owner.clone();
                    menu = menu.item(PopupMenuItem::element(move |_, _| {
                        div().min_h(px(32.)).flex().items_center().child(Label::new(title).text_size(px(13.)))
                    }).disabled(sync && pending).on_click(move |_, _, cx| {
                        let _ = owner.update(cx, |this, cx| {
                            if !sync { this.request_prices(cx); return; }
                            if this.price_sync_pending { return; }
                            let tag = format!("price-sync-{}", this.generation);
                            let request = crate::stats_backend::command("stats_prices_update", &tag, json!({}));
                            match this.backend.submit_write(JobKind::PriceSync, tag, request) {
                                Ok(_) => this.price_sync_pending = true,
                                Err(error) => {
                                    this.price_sync_error = Some(error.clone());
                                    this.error = Some(error);
                                },
                            }
                            cx.notify();
                        });
                    }));
                }
                menu.min_w(px(200.))
            }).into_any_element()
    }

    fn price_row_actions(
        &self,
        index: usize,
        rule: &PriceRuleDto,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let owner = cx.entity().downgrade();
        let rule_id = rule.rule_id.clone();
        let revision = rule.revision;
        let delete = self.t("删除", "Delete");
        div()
            .w(px(110.))
            .flex()
            .items_center()
            .justify_end()
            .gap(px(4.))
            .child(
                stats_button(("price-edit", index))
                    .ghost()
                    .h(px(28.))
                    .label(self.t("编辑", "Edit"))
                    .on_click(cx.listener({
                        let rule = rule.clone();
                        move |this, _, window, cx| this.edit_price(&rule, window, cx)
                    })),
            )
            .child(
                stats_button(("price-row-actions", index))
                    .ghost()
                    .h(px(28.))
                    .w(px(28.))
                    .p_0()
                    .label("⋯")
                    .accessibility_label(self.t("价格规则操作", "Price rule actions"))
                    .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, _, _| {
                        let owner = owner.clone();
                        let rule_id = rule_id.clone();
                        menu.min_w(px(140.)).item(
                            PopupMenuItem::element(move |_, _| {
                                div().min_h(px(32.)).flex().items_center().child(
                                    Label::new(delete)
                                        .text_size(px(13.))
                                        .text_color(rgb(crate::theme::color::DANGER)),
                                )
                            })
                            .on_click(move |_, _, cx| {
                                let _ = owner.update(cx, |this, cx| {
                                    this.delete_price(rule_id.clone(), revision, cx)
                                });
                            }),
                        )
                    }),
            )
            .into_any_element()
    }
}
