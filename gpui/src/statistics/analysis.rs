//! Studio analysis cards project the existing filtered dashboard snapshot.
//! Cache remains an input subset; unknown counters never become zero fills.
use super::*;
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};

/// Render a composition only when both disjoint parts agree with the total.
fn token_split(total: Option<i64>, input: Option<i64>, output: Option<i64>) -> Option<(f32, f32)> {
    let (total, input, output) = (total?, input?, output?);
    if total <= 0 || input < 0 || output < 0 || input.checked_add(output)? != total {
        return None;
    }
    Some((input as f32 / total as f32, output as f32 / total as f32))
}

fn valid_ratio(value: Option<f64>) -> Option<f64> {
    value.filter(|value| value.is_finite() && (0.0..=1.0).contains(value))
}

fn heading(value: &'static str) -> Label {
    Label::new(value)
        .text_size(px(16.))
        .font_weight(FontWeight::BOLD)
        .text_color(rgb(INK))
        .when(
            crate::theme::current_skin() == crate::theme::Skin::StudioIndigo,
            |label| label.h(px(23.)).line_height(px(23.)),
        )
}
fn caption(value: impl Into<SharedString>) -> Label {
    Label::new(value)
        .text_size(px(12.))
        .text_color(rgb(MUTED))
        .when(
            crate::theme::current_skin() == crate::theme::Skin::StudioIndigo,
            |label| label.line_height(px(17.)),
        )
}

impl Statistics {
    pub(super) fn analysis_table_header(&self, cx: &mut Context<Self>) -> AnyElement {
        let tabs = div()
            .flex()
            .items_center()
            .gap(px(4.))
            .pl(px(4.))
            .pr(px(6.))
            .py(px(4.))
            .w(px(314.))
            .h(px(38.))
            .flex_none()
            .rounded(px(9.))
            .bg(rgb(crate::theme::color::BG_TABLE_HEAD))
            .children(
                [
                    (
                        "tab-providers",
                        Tab::Providers,
                        self.t("供应商统计", "Providers"),
                    ),
                    ("tab-models", Tab::Models, self.t("模型统计", "Models")),
                ]
                .into_iter()
                .map(|(id, tab, label)| {
                    let active = self.tab == tab;
                    stats_button(id)
                        .with_size(px(30.))
                        .h(px(30.))
                        .w(px(if tab == Tab::Providers { 145. } else { 155. }))
                        .rounded(px(7.))
                        .custom(
                            gpui_kit::component::button::ButtonCustomVariant::new(cx)
                                .color(
                                    rgb(if active {
                                        PANEL
                                    } else {
                                        crate::theme::color::BG_TABLE_HEAD
                                    })
                                    .into(),
                                )
                                .foreground(rgb(if active { INK } else { MUTED }).into())
                                .hover(rgb(crate::theme::palette().tint).into())
                                .active(rgb(crate::theme::palette().tint).into()),
                        )
                        .accessibility_label(label)
                        .child(
                            Label::new(label)
                                .text_size(px(12.))
                                .line_height(px(17.))
                                .text_color(rgb(if active { INK } else { MUTED }))
                                .font_weight(if active {
                                    FontWeight::BOLD
                                } else {
                                    FontWeight::MEDIUM
                                }),
                        )
                        .toggled(active)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if this.tab == tab {
                                return;
                            }
                            this.tab = tab;
                            this.summary_tab = tab;
                            this.persist_preferences();
                            this.reset_pagination();
                            this.rows_gate.invalidate();
                            this.groups_gate.invalidate();
                            this.request_groups(cx);
                            cx.notify();
                        }))
                }),
            );
        // The caption belongs to the left title column, independently of the
        // 38px selector. Let both columns wrap only when the viewport needs it.
        div().flex().items_start().justify_between().flex_wrap()
            .min_h(px(52.)).pl(px(5.)).pr(px(13.)).gap(px(14.)).flex_shrink_0()
            .child(div().flex().flex_col().flex_1().min_w(px(220.)).gap(px(5.))
                .child(div().flex().items_start().h(px(27.)).gap(px(14.))
                    .child(Label::new(if self.tab == Tab::Providers {
                        self.t("供应商表现", "Provider performance")
                    } else { self.t("模型表现", "Model performance") })
                        .min_w(px(118.)).h(px(20.)).line_height(px(24.)).flex_shrink_0()
                        .text_size(px(17.)).font_weight(FontWeight::BOLD).text_color(rgb(INK)))
                    .child(div().pt(px(2.)).child(self.range_count_summary("analysis-range-summary", "analysis-range-summary-trigger",
                        format!("{}{} {}", if self.total_groups > crate::stats_dto::MAX_COUNTED_GROUPS { "≥" } else { "" },
                            self.total_groups.max(0), self.t("项", "groups")), cx))))
                .child(caption(if self.tab == Tab::Providers {
                    self.t("相同名称代表不同的上游记录组，保留各组统计", "Identical names may represent different upstream groups; each keeps its own totals")
                } else {
                    self.t("按模型汇总，费用与积分分别统计", "Grouped by model; costs and credits remain separate")
                }).w_full().h(px(20.)).line_height(px(17.)).text_ellipsis()))
            .child(tabs)
            .into_any_element()
    }

    pub(super) fn analysis_sort_control(&self, cx: &mut Context<Self>) -> AnyElement {
        let owner = cx.entity().downgrade();
        let selected = self.group_sort;
        let zh = self.zh;
        stats_button("analysis-sort")
            .ghost()
            .with_size(px(28.))
            .h(px(28.))
            .label(match (selected, zh) {
                (SortMetric::TotalTokens, true) => "按总 Token 排序 ⌄",
                (SortMetric::TotalTokens, false) => "By total tokens ⌄",
                (SortMetric::RequestCount, true) => "按请求数排序 ⌄",
                (SortMetric::RequestCount, false) => "By request count ⌄",
            })
            .dropdown_menu_with_anchor(Anchor::TopLeft, move |mut menu, _, _| {
                for (sort, title) in [
                    (
                        SortMetric::TotalTokens,
                        if zh { "总 Token" } else { "Total tokens" },
                    ),
                    (
                        SortMetric::RequestCount,
                        if zh { "请求数" } else { "Request count" },
                    ),
                ] {
                    let owner = owner.clone();
                    menu = menu.item(
                        PopupMenuItem::element(move |_, _| {
                            div()
                                .min_h(px(32.))
                                .flex()
                                .items_center()
                                .child(Label::new(title).text_size(px(13.)))
                        })
                        .checked(sort == selected)
                        .on_click(move |_, _, cx| {
                            let _ = owner.update(cx, |this, cx| {
                                this.group_sort = sort;
                                this.reset_pagination();
                                this.request_groups(cx);
                            });
                        }),
                    );
                }
                menu.min_w(px(180.))
            })
            .into_any_element()
    }

    pub(super) fn analysis_cards(&self, cx: &App) -> AnyElement {
        let studio = crate::theme::current_skin() == crate::theme::Skin::StudioIndigo;
        let summary = self.dashboard.as_ref().map(|dashboard| &dashboard.summary);
        let input = summary.and_then(|summary| summary.input_tokens);
        let output = summary.and_then(|summary| summary.output_tokens);
        let total = summary.and_then(|summary| summary.total_tokens);
        let split = token_split(total, input, output);
        let cache = valid_ratio(summary.and_then(|summary| summary.cache_hit_rate));
        let coverage = valid_ratio(summary.and_then(|summary| summary.usage_coverage));
        let compact = self.filters_width < 900.;
        let token_hint = format!("{} {}\n{} {}\n{} {}\n{}",
            self.t("总 Token", "Total tokens"), usage_table::exact(total),
            self.t("输入", "Input"), usage_table::exact(input),
            self.t("输出", "Output"), usage_table::exact(output),
            self.t("仅绘制与总量一致的已记录分项；缓存是输入的子集，不重复相加",
                "Only recorded parts agreeing with the total are drawn; cache is an input subset, never added twice"));
        let tokens = stats_card("analysis-token-composition")
            .flex_1()
            .flex_basis(px(0.))
            .flex_shrink_0()
            .min_w_0()
            .min_h(px(201.))
            .when(studio && !compact, |card| card.flex_grow(542.))
            .when(compact, |card| card.flex_basis(Length::Auto))
            .p(px(20.))
            .gap(px(12.))
            .when(studio, |card| card.pt(px(16.)).pr(px(24.)).gap_0())
            .child(heading(self.t("Token 构成", "Token composition")))
            .child(
                caption(self.t("输入与输出分开计算", "Input and output counted separately"))
                    .when(studio, |label| label.mt(px(7.)).h(px(20.))),
            )
            .child(
                div()
                    .id("analysis-token-total")
                    .flex()
                    .items_end()
                    .flex_wrap()
                    .gap(px(16.))
                    .when(studio, |row| row.mt(px(7.)).items_start())
                    .tooltip(move |window, cx| {
                        gpui_kit::component::tooltip::Tooltip::new(token_hint.clone())
                            .build(window, cx)
                    })
                    .child(
                        crate::studio_fonts::latin(
                            Label::new(usage_table::compact(total, self.zh)),
                            cx,
                        )
                        .text_size(px(32.))
                        .line_height(px(45.))
                        .font_weight(FontWeight::BOLD)
                        .text_color(rgb(INK))
                        .when(studio, |value| value.min_w(px(157.))),
                    )
                    .child(
                        caption(self.t("当前范围已记录用量", "Recorded usage in this range"))
                            .when(studio, |label| label.mt(px(17.)).h(px(20.))),
                    ),
            )
            .child(
                div()
                    .w_full()
                    .h(px(12.))
                    .when(studio, |track| track.mt(px(13.)))
                    .rounded(px(6.))
                    .overflow_hidden()
                    .bg(rgb(crate::theme::color::BG_TABLE_HEAD))
                    .flex()
                    .when_some(split, |track, (input, output)| {
                        track
                            .child(
                                div()
                                    .w(relative(input))
                                    .h_full()
                                    .bg(rgb(crate::theme::color::CHART_INPUT)),
                            )
                            .child(
                                div()
                                    .w(relative(output))
                                    .h_full()
                                    .bg(rgb(crate::theme::color::CHART_CACHE_READ)),
                            )
                    }),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .flex_wrap()
                    .gap(px(24.))
                    .when(studio, |legend| legend.mt(px(16.)).min_h(px(20.)).gap_0())
                    .children(
                        [
                            (
                                self.t("输入 Token", "Input tokens"),
                                input,
                                crate::theme::color::CHART_INPUT,
                            ),
                            (
                                self.t("输出 Token", "Output tokens"),
                                output,
                                crate::theme::color::CHART_CACHE_READ,
                            ),
                        ]
                        .into_iter()
                        .enumerate()
                        .map(|(index, (label, value, color))| {
                            div()
                                .flex()
                                .items_center()
                                .gap(px(8.))
                                .when(studio && index == 0, |item| item.min_w(px(202.)))
                                .child(div().size(px(8.)).rounded(px(3.)).bg(rgb(color)))
                                .child(
                                    caption(format!(
                                        "{label}  {}",
                                        usage_table::compact(value, self.zh)
                                    ))
                                    .when(studio, |label| {
                                        label
                                            .h(px(20.))
                                            .text_color(rgb(crate::theme::color::TEXT_2))
                                    }),
                                )
                        }),
                    ),
            )
            .when(split.is_none(), |card| {
                card.child(
                    caption(
                        if total == Some(0) && input == Some(0) && output == Some(0) {
                            self.t("已记录总量为 0", "Recorded total is zero")
                        } else {
                            self.t(
                                "分项缺失或与总量不一致，比例暂不可用",
                                "Missing or inconsistent parts; proportions unavailable",
                            )
                        },
                    )
                    .when(studio, |notice| {
                        notice.mt(px(12.)).w_full().whitespace_normal()
                    }),
                )
            });
        let cache_hint = summary
            .map(|summary| {
                format!(
                    "{} {} / {}\n{} {} / {}\n{} {} / {}",
                    self.t(
                        "缓存读取 Token / 对应输入 Token",
                        "Cache read tokens / corresponding input tokens"
                    ),
                    usage_table::exact(summary.cache_read_tokens),
                    usage_table::exact(summary.cache_read_denominator_input_tokens),
                    self.t("已知缓存读记录", "Records with known cache reads"),
                    summary.cache_read_known_count,
                    summary.request_count,
                    self.t(
                        "完整用量记录 / 可计量记录",
                        "Complete / eligible usage records"
                    ),
                    summary.usage_complete_count,
                    summary.usage_eligible_count
                )
            })
            .unwrap_or_else(|| {
                self.t("尚未读取当前范围", "Current range not loaded")
                    .to_owned()
            });
        let cache_card = stats_card("analysis-cache-coverage")
            .flex_1()
            .flex_basis(px(0.))
            .flex_shrink_0()
            .min_w_0()
            .min_h(px(201.))
            .when(studio && !compact, |card| card.flex_grow(562.))
            .when(compact, |card| card.flex_basis(Length::Auto))
            .p(px(20.))
            .gap(px(16.))
            .when(studio, |card| {
                card.pt(px(16.)).pr(px(22.)).pb(px(27.)).gap_0()
            })
            .tooltip(move |window, cx| {
                gpui_kit::component::tooltip::Tooltip::new(cache_hint.clone()).build(window, cx)
            })
            .child(heading(self.t("缓存与覆盖", "Cache & coverage")))
            .child(
                caption(self.t("缓存读取命中率", "Cache read hit rate"))
                    .when(studio, |label| label.mt(px(20.)).h(px(20.))),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(24.))
                    .when(studio, |row| row.mt(px(1.)).pr(px(6.)).h(px(38.)))
                    .child(
                        crate::studio_fonts::latin(Label::new(usage_table::percent(cache)), cx)
                            .text_size(px(27.))
                            .line_height(px(38.))
                            .font_weight(FontWeight::BOLD)
                            .text_color(rgb(INK))
                            .when(studio, |value| value.min_w(px(168.)).flex_none()),
                    )
                    .child(
                        div()
                            .flex_1()
                            .h(px(8.))
                            .rounded(px(5.))
                            .overflow_hidden()
                            .bg(rgb(crate::theme::color::BG_TABLE_HEAD))
                            .when_some(cache, |track, value| {
                                track.child(
                                    div()
                                        .w(relative(value as f32))
                                        .h_full()
                                        .rounded(px(5.))
                                        .bg(rgb(crate::theme::color::CHART_CACHE_READ)),
                                )
                            }),
                    ),
            )
            .child(
                div()
                    .w_full()
                    .h(px(1.))
                    .bg(rgb(BORDER))
                    .when(studio, |line| line.mt(px(10.))),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(12.))
                    .when(studio, |row| row.mt(px(17.)).min_h(px(26.)))
                    .child(
                        caption(self.t("已记录用量完整性", "Recorded usage completeness"))
                            .when(studio, |label| label.h(px(20.))),
                    )
                    .child(
                        crate::studio_fonts::latin(Label::new(usage_table::percent(coverage)), cx)
                            .text_size(px(18.))
                            .line_height(px(26.))
                            .font_weight(FontWeight::BOLD)
                            .text_color(rgb(if coverage.is_some() { OK } else { MUTED })),
                    ),
            );
        div()
            .flex()
            .w_full()
            .gap(px(if studio { 16. } else { 18. }))
            .flex_shrink_0()
            .when(compact, |row| row.flex_col())
            .child(tokens)
            .child(cache_card)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::{token_split, valid_ratio};

    #[test]
    fn composition_requires_known_disjoint_parts_and_matching_positive_total() {
        assert_eq!(token_split(Some(100), Some(80), Some(20)), Some((0.8, 0.2)));
        assert_eq!(token_split(Some(100), Some(100), Some(0)), Some((1., 0.)));
        for (total, input, output) in [
            (None, Some(80), Some(20)),
            (Some(100), None, Some(20)),
            (Some(100), Some(80), None),
            (Some(0), Some(0), Some(0)),
            (Some(100), Some(80), Some(10)),
            (Some(100), Some(-1), Some(101)),
            (Some(i64::MAX), Some(i64::MAX), Some(1)),
        ] {
            assert_eq!(token_split(total, input, output), None);
        }
    }

    #[test]
    fn ratios_keep_known_zero_separate_from_unknown_or_invalid() {
        assert_eq!(valid_ratio(Some(0.)), Some(0.));
        assert_eq!(valid_ratio(Some(1.)), Some(1.));
        for value in [
            None,
            Some(-0.1),
            Some(1.1),
            Some(f64::NAN),
            Some(f64::INFINITY),
        ] {
            assert_eq!(valid_ratio(value), None);
        }
    }
}
