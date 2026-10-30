//! Studio usage plot and billing summary, projected from existing dashboard data.
use super::*;
use crate::stats_dto::SummaryDto;

fn selected_cost(summary: &SummaryDto, currency: Currency) -> String {
    let amount = summary.costs.iter().find(|cost| cost.currency == currency);
    usage_table::amount(
        amount.map(|cost| cost.total_cost_nanos.as_str()),
        Some(currency),
    )
}

impl Statistics {
    pub(super) fn trends(&self, cx: &mut Context<Self>) -> AnyElement {
        let series = self
            .dashboard
            .as_ref()
            .map(|data| data.series.as_slice())
            .unwrap_or_default();
        let (points, _) = ChartPoint::split_series(series, self.currency);
        let incomplete = points.iter().any(|point| {
            point.collection_warning || point.quality == crate::stats_dto::SeriesQuality::Unknown
        });
        let chart = UsagePlot {
            id: "usage-token-chart".into(),
            points,
            visible: self.visible_series.clone(),
            zh: self.zh,
            mode: ChartMode::Tokens,
            currency: self.currency,
        };
        let continuity = chart.continuity_note();
        let summary = chart.summary();
        let legend = div()
            .flex()
            .items_center()
            .flex_wrap()
            .gap(px(6.))
            .children(SeriesKind::ALL.into_iter().map(|kind| {
                let active = self.visible_series.contains(&kind);
                stats_button(("legend", kind as usize))
                    .ghost()
                    .with_size(px(26.))
                    .h(px(26.))
                    .px(px(3.))
                    .toggled(active)
                    .accessibility_label(kind.label(self.zh))
                    .opacity(if active { 1. } else { 0.4 })
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .child(
                                div()
                                    .w(px(12.))
                                    .h(px(3.))
                                    .rounded(px(2.))
                                    .bg(rgb(kind.color())),
                            )
                            .child(
                                Label::new(kind.label(self.zh))
                                    .text_size(px(11.))
                                    .text_color(rgb(MUTED)),
                            ),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if !this.visible_series.remove(&kind) {
                            this.visible_series.insert(kind);
                        }
                        cx.notify();
                    }))
            }));
        let cache_note = self.t(
            "缓存读/写是输入的子集，图例可独立开关，不可相加",
            "Cache read/write are input subsets; toggle them, never sum them",
        );
        let usage = stats_card("stats-trends")
            .flex_grow(1.69)
            .flex_basis(px(0.))
            .flex_shrink_0()
            .when(self.filters_width < CHART_SIDE_BY_SIDE_WIDTH, |card| {
                card.flex_basis(Length::Auto)
            })
            .min_w_0()
            .p(px(20.))
            .gap(px(6.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .flex_wrap()
                    .gap(px(8.))
                    .child(
                        Label::new(self.t("使用趋势", "Usage trend"))
                            .text_size(px(16.))
                            .font_weight(FontWeight::BOLD),
                    )
                    .child(legend)
                    .child(
                        div()
                            .px(px(12.))
                            .h(px(26.))
                            .rounded(px(13.))
                            .bg(rgb(crate::theme::color::BG_TABLE_HEAD))
                            .child(
                                Label::new(self.range.title(self.zh))
                                    .text_size(px(12.))
                                    .text_color(rgb(crate::theme::color::TEXT_2)),
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .flex_wrap()
                    .gap(px(6.))
                    .child(
                        div()
                            .id("usage-cache-note")
                            .tooltip(move |window, cx| {
                                gpui_kit::component::tooltip::Tooltip::new(cache_note)
                                    .build(window, cx)
                            })
                            .child(
                                Label::new(self.t("Token 使用量", "Token usage"))
                                    .text_size(px(12.))
                                    .text_color(rgb(MUTED)),
                            ),
                    )
                    .when(incomplete, |row| {
                        row.child(
                            Label::new(self.t("部分时段记录不全", "Some intervals are incomplete"))
                                .text_size(px(11.))
                                .text_color(rgb(WARN)),
                        )
                    }),
            )
            .when_some(continuity, |card, note| {
                card.child(
                    Label::new(note)
                        .text_size(px(11.))
                        .line_height(px(16.))
                        .text_color(rgb(MUTED)),
                )
            })
            .child(
                div()
                    .id("token-chart-host")
                    .w_full()
                    .h(px(145.))
                    .min_h_0()
                    .aria_label(summary)
                    .child(chart),
            );
        div()
            .w_full()
            .flex()
            .items_stretch()
            .gap(px(16.))
            .when(self.filters_width < CHART_SIDE_BY_SIDE_WIDTH, |row| {
                row.flex_col()
            })
            .child(usage)
            .child(self.billing_card(cx))
            .into_any_element()
    }

    fn billing_card(&self, cx: &mut Context<Self>) -> AnyElement {
        let summary = self.dashboard.as_ref().map(|data| &data.summary);
        let amount = summary
            .map(|data| selected_cost(data, self.currency))
            .unwrap_or_else(|| "—".into());
        let credit = summary
            .and_then(|data| data.credit_nanos.as_deref())
            .and_then(usage_table::format_credit_nanos)
            .unwrap_or_else(|| "—".into());
        let unpriced = summary.map(|data| data.imported_unpriced_count);
        stats_card("stats-billing")
            .flex_1()
            .flex_basis(px(0.))
            .flex_shrink_0()
            .when(self.filters_width < CHART_SIDE_BY_SIDE_WIDTH, |card| {
                card.flex_basis(Length::Auto)
            })
            .min_w_0()
            .min_h(px(246.))
            .p(px(20.))
            .gap(px(8.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(8.))
                    .child(
                        Label::new(self.t("费用与计费", "Costs & billing"))
                            .text_size(px(16.))
                            .font_weight(FontWeight::BOLD),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .child(self.cost_history_control(cx))
                            .child(
                                div().w(px(78.)).child(
                                    crate::ui_components::select(&self.currency_select, false)
                                        .small()
                                        .h(px(30.))
                                        .rounded(px(15.))
                                        .bg(rgb(crate::theme::color::BG_TABLE_HEAD))
                                        .accessibility_label(self.t("费用币种", "Cost currency")),
                                ),
                            ),
                    ),
            )
            .child(
                crate::studio_fonts::latin(Label::new(amount), cx)
                    .text_size(px(31.))
                    .line_height(px(44.))
                    .font_weight(FontWeight::BOLD)
                    .text_color(rgb(INK)),
            )
            .child(
                Label::new(self.t("代理估算费用", "Proxy estimated cost"))
                    .text_size(px(12.))
                    .text_color(rgb(MUTED)),
            )
            .child(div().h(px(1.)).mt(px(8.)).mb(px(4.)).bg(rgb(BORDER)))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(8.))
                    .child(
                        Label::new(self.t("WorkBuddy 积分", "WorkBuddy credits"))
                            .text_size(px(12.))
                            .text_color(rgb(crate::theme::color::TEXT_2)),
                    )
                    .child(
                        Label::new(credit)
                            .text_size(px(18.))
                            .font_weight(FontWeight::SEMIBOLD),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(8.))
                    .child(
                        Label::new(self.t("导入用量费用未定价", "Imported usage is unpriced"))
                            .text_size(px(12.))
                            .text_color(rgb(MUTED)),
                    )
                    .child(
                        div()
                            .px(px(12.))
                            .py(px(5.))
                            .rounded(px(15.))
                            .bg(rgb(0xfff3e4))
                            .child(
                                Label::new(
                                    unpriced
                                        .map(|count| {
                                            format!("{count} {}", self.t("条导入", "imported"))
                                        })
                                        .unwrap_or_else(|| "—".into()),
                                )
                                .text_size(px(11.))
                                .text_color(rgb(0xb57a35)),
                            ),
                    ),
            )
            .when_some(
                summary.filter(|data| data.default_zero_request_count > 0),
                |card, summary| {
                    card.child(
                        Label::new(format!(
                            "{} {}",
                            summary.default_zero_request_count,
                            self.t("条使用默认零价", "records use the default zero price")
                        ))
                        .text_size(px(11.))
                        .text_color(rgb(MUTED)),
                    )
                },
            )
            .into_any_element()
    }

    fn cost_history_control(&self, cx: &mut Context<Self>) -> AnyElement {
        let owner = cx.entity().downgrade();
        let label = self.t("查看费用趋势", "View cost trend");
        Popover::new("stats-cost-history")
            .trigger(
                stats_button("stats-cost-history-trigger")
                    .ghost()
                    .with_size(px(28.))
                    .size(px(28.))
                    .p_0()
                    .accessibility_label(label)
                    .tooltip(label)
                    .child(
                        Icon::default()
                            .path("studio/nav-chart.svg")
                            .with_size(px(18.))
                            .text_color(rgb(MUTED)),
                    ),
            )
            .content(move |_, window, cx| {
                owner
                    .update(cx, |this, cx| this.cost_history_panel(window, cx))
                    .unwrap_or_else(|_| div().into_any_element())
            })
            .into_any_element()
    }

    fn cost_history_panel(&self, window: &Window, _cx: &mut Context<Self>) -> AnyElement {
        let series = self
            .dashboard
            .as_ref()
            .map(|data| data.series.as_slice())
            .unwrap_or_default();
        let (token_points, mut points) = ChartPoint::split_series(series, self.currency);
        if points.is_empty() {
            points = token_points;
        }
        let has_cost = points.iter().any(|point| point.cost_nanos.is_some());
        let chart = UsagePlot {
            id: "usage-cost-chart".into(),
            points,
            visible: BTreeSet::new(),
            zh: self.zh,
            mode: ChartMode::Cost,
            currency: self.currency,
        };
        let continuity = chart.continuity_note();
        div()
            .id("stats-cost-history-panel")
            .w(px((f32::from(window.viewport_size().width) - 48.).min(620.)))
            .max_h(px(
                (f32::from(window.viewport_size().height) - 160.).max(220.)
            ))
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap(px(8.))
            .p(px(12.))
            .child(
                Label::new(format!(
                    "{} · {}",
                    self.t("估算费用趋势", "Estimated cost trend"),
                    self.currency.as_str()
                ))
                .text_size(px(16.))
                .font_weight(FontWeight::SEMIBOLD),
            )
            .when_some(continuity, |card, note| {
                card.child(Label::new(note).text_size(px(12.)).text_color(rgb(MUTED)))
            })
            .child(
                div()
                    .id("cost-chart-host")
                    .relative()
                    .w_full()
                    .h(px(200.))
                    .min_h(px(200.))
                    .aria_label(self.t(
                        "费用趋势图，按币种单独显示",
                        "Cost trend chart, one currency at a time",
                    ))
                    .child(chart)
                    .when(!has_cost, |plot| {
                        plot.child(
                            div()
                                .absolute()
                                .inset_0()
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(
                                    Label::new(self.t(
                                        "当前币种暂无费用数据",
                                        "No cost data for this currency",
                                    ))
                                    .text_size(px(12.))
                                    .text_color(rgb(MUTED)),
                                ),
                        )
                    }),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::selected_cost;
    use crate::stats_dto::{Currency, SummaryDto};
    #[test]
    fn billing_keeps_missing_currency_distinct_from_a_recorded_zero() {
        let mut summary = SummaryDto::default();
        assert_eq!(selected_cost(&summary, Currency::Usd), "—");
        summary.costs.push(crate::stats_dto::CostTotalDto {
            currency: Currency::Cny,
            total_cost_nanos: "0".into(),
            priced_request_count: 1,
        });
        assert_eq!(selected_cost(&summary, Currency::Usd), "—");
        assert_eq!(selected_cost(&summary, Currency::Cny), "¥0");
        summary.costs.push(crate::stats_dto::CostTotalDto {
            currency: Currency::Usd,
            total_cost_nanos: "1250000000".into(),
            priced_request_count: 1,
        });
        assert_eq!(selected_cost(&summary, Currency::Usd), "$1.2500");
        assert_eq!(selected_cost(&summary, Currency::Cny), "¥0");
    }
}
