//! Studio primary KPIs and the retained seven-metric detail view. Every value
//! comes from the existing summary; inputs, cache counters, credits and currency
//! totals retain their separate units and unknown-value semantics.
use super::*;
use crate::stats_dto::SummaryDto;

#[derive(Clone)]
struct Card {
    id: MetricId,
    value: String,
    hint: String,
    exact: String,
}

struct PrimaryMetric {
    label: &'static str,
    value: String,
    hint: String,
    exact: String,
    icon: &'static str,
    font_size: f32,
    line_height: f32,
}
fn text(zh: bool, cn: &'static str, en: &'static str) -> &'static str {
    if zh { cn } else { en }
}
fn number(value: Option<i64>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "—".into())
}

fn title(id: MetricId, zh: bool) -> &'static str {
    if zh {
        return match id {
            MetricId::InputTokens => "输入 / 输出 Token",
            MetricId::CacheRead => "缓存读取 / 创建",
            _ => id.title(),
        };
    }
    match id {
        MetricId::UsageRecords => "Usage records",
        MetricId::ProxySuccessRate => "Proxy success rate",
        MetricId::InputTokens => "Input / output tokens",
        MetricId::OutputTokens => "Output tokens",
        MetricId::CacheRead => "Cache read / write",
        MetricId::CacheWrite => "Cache write",
        MetricId::RecordedCoverage => "Recorded usage completeness",
        MetricId::EstimatedCost => "Proxy estimated cost",
        MetricId::WorkbuddyCredit => "WorkBuddy credits",
    }
}

fn token_cards(summary: &SummaryDto, zh: bool) -> Vec<Card> {
    let mut cards = vec![Card {
        id: MetricId::InputTokens,
        value: format!(
            "{} / {}",
            usage_table::compact(summary.input_tokens, zh),
            usage_table::compact(summary.output_tokens, zh)
        ),
        exact: format!(
            "{} {}\n{} {}",
            text(zh, "输入", "Input"),
            number(summary.input_tokens),
            text(zh, MetricId::OutputTokens.title(), "Output tokens"),
            number(summary.output_tokens)
        ),
        hint: text(
            zh,
            "输入 / 输出，各自独立计数",
            "Input / output, counted separately",
        )
        .into(),
    }];
    cards.push(Card {
        id: MetricId::CacheRead,
        value: format!(
            "{} / {}",
            usage_table::compact(summary.cache_read_tokens, zh),
            usage_table::compact(summary.cache_write_tokens, zh)
        ),
        exact: format!(
            "{} {}\n{} {}\n{}",
            text(zh, MetricId::CacheRead.title(), "Cache read"),
            number(summary.cache_read_tokens),
            text(zh, MetricId::CacheWrite.title(), "Cache write"),
            number(summary.cache_write_tokens),
            text(
                zh,
                "缓存是输入的子集，不可重复相加",
                "Cache is a subset of input; never add twice"
            )
        ),
        hint: format!(
            "{} {}",
            text(zh, "命中率", "Hit rate"),
            usage_table::percent(summary.cache_hit_rate)
        ),
    });
    cards
}

fn cost_card(summary: &SummaryDto, zh: bool) -> Card {
    let cost = if summary.costs.is_empty() {
        text(zh, "未定价", "Unpriced").to_owned()
    } else {
        summary
            .costs
            .iter()
            .map(|cost| {
                format!(
                    "{} {}",
                    cost.currency.as_str(),
                    crate::stats_dto::format_amount_text(&cost.total_cost_nanos)
                )
            })
            .collect::<Vec<_>>()
            .join(" / ")
    };
    let exact_cost = summary
        .costs
        .iter()
        .map(|cost| {
            format!(
                "{} {}",
                cost.currency.as_str(),
                usage_table::format_credit_nanos(&cost.total_cost_nanos)
                    .unwrap_or_else(|| "—".into())
            )
        })
        .collect::<Vec<_>>()
        .join(" / ");
    Card {
        id: MetricId::EstimatedCost,
        value: cost,
        exact: exact_cost,
        hint: format!(
            "{} {} {}",
            text(zh, "导入费用未知", "Imported unpriced"),
            summary.imported_unpriced_count,
            text(zh, "条", "records")
        ),
    }
}

fn cards(summary: &SummaryDto, zh: bool) -> Vec<Card> {
    let mut cards = vec![
        Card {
            id: MetricId::UsageRecords,
            value: summary.request_count.to_string(),
            exact: summary.request_count.to_string(),
            hint: format!(
                "{} {} · {} {}",
                summary.proxy_record_count,
                text(zh, "代理", "proxy"),
                summary.imported_record_count,
                text(zh, "导入", "imported")
            ),
        },
        Card {
            id: MetricId::ProxySuccessRate,
            value: metric_text(
                MetricId::ProxySuccessRate,
                summary
                    .success_rate
                    .map(|rate| MetricValue::Known((rate * 1000.).round() as i64))
                    .unwrap_or(MetricValue::Unknown),
            ),
            exact: usage_table::percent(summary.success_rate),
            hint: if zh {
                MetricId::ProxySuccessRate.subtitle().into()
            } else {
                "Only proxy requests have status evidence".into()
            },
        },
    ];
    cards.extend(token_cards(summary, zh));
    cards.push(Card {
        id: MetricId::RecordedCoverage,
        value: usage_table::percent(summary.usage_coverage),
        exact: usage_table::percent(summary.usage_coverage),
        hint: text(
            zh,
            "只统计已记录条目的用量完整度",
            "Completeness of recorded usage only",
        )
        .into(),
    });
    cards.push(cost_card(summary, zh));
    let credit = summary
        .credit_nanos
        .as_deref()
        .and_then(usage_table::format_credit_nanos)
        .unwrap_or_else(|| "—".into());
    cards.push(Card {
        id: MetricId::WorkbuddyCredit,
        value: credit.clone(),
        exact: credit,
        hint: text(
            zh,
            "日志记录值，不代表账号余额",
            "Logged usage; not account balance",
        )
        .into(),
    });
    cards
}

/// Primary V2 layout: 454px hero plus three readable metrics at 1120px;
/// compact workspaces stack the hero above the three-card row.
fn kpi_widths(width: f32) -> (f32, [f32; 3], bool) {
    let width = width.max(0.);
    let wide = width >= 1000.;
    let hero = if wide { width * 454. / 1120. } else { width };
    let available = if wide {
        width - hero - 3. * METRIC_GAP as f32
    } else {
        (width - 2. * METRIC_GAP as f32).max(0.)
    };
    let metrics = if wide {
        let first = available * 204. / 618.;
        [first, first, available - 2. * first]
    } else {
        [available / 3.; 3]
    };
    (hero, metrics, wide)
}

/// One bar per existing time bucket, without converting unknown usage to zero.
fn spark_heights(samples: &[Option<i64>]) -> Vec<Option<f32>> {
    let samples = &samples[samples.len().saturating_sub(14)..];
    let peak = samples
        .iter()
        .flatten()
        .copied()
        .filter(|value| *value > 0)
        .max();
    samples
        .iter()
        .map(|value| match value {
            Some(value) if *value >= 0 => {
                Some(peak.map_or(0., |peak| (*value as f64 / peak as f64 * 76.) as f32))
            }
            _ => None,
        })
        .collect()
}

impl Statistics {
    fn primary_metric(&self, metric: PrimaryMetric, width: f32, cx: &App) -> AnyElement {
        let PrimaryMetric {
            label,
            value,
            hint,
            exact,
            icon,
            font_size,
            line_height,
        } = metric;
        div()
            .id(icon)
            .w(px(width))
            .min_w(px(METRIC_MIN_WIDTH as f32))
            .h(px(168.))
            .flex_shrink_0()
            .flex()
            .flex_col()
            .p(px(16.))
            .rounded(px(16.))
            .border_1()
            .border_color(rgb(BORDER))
            .bg(rgb(crate::theme::color::WHITE))
            .role(Role::Group)
            .aria_label(format!("{label} {value}"))
            .tooltip(move |window, cx| {
                gpui_kit::component::tooltip::Tooltip::new(exact.clone()).build(window, cx)
            })
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(9.))
                    .child(
                        Icon::default()
                            .path(icon)
                            .with_size(px(18.))
                            .text_color(rgb(crate::theme::palette().primary)),
                    )
                    .child(
                        Label::new(label)
                            .text_size(px(12.))
                            .line_height(px(17.))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(rgb(MUTED)),
                    ),
            )
            .child(
                crate::studio_fonts::latin(Label::new(value), cx)
                    .mt(px(20.))
                    .text_size(px(font_size))
                    .line_height(px(line_height))
                    .font_weight(FontWeight::BOLD)
                    .text_color(rgb(INK)),
            )
            .child(div().flex_1())
            .child(
                Label::new(hint)
                    .mb(px(7.))
                    .text_size(px(12.))
                    .line_height(px(17.))
                    .font_weight(FontWeight::NORMAL)
                    .text_color(rgb(MUTED)),
            )
            .into_any_element()
    }

    fn metric_details(&self, summary: &SummaryDto, cx: &App) -> AnyElement {
        let snapshot = cards(summary, self.zh);
        let zh = self.zh;
        let label = self.t("查看详细指标", "View detailed metrics");
        Popover::new("stats-metric-details")
            .trigger(
                Button::new("stats-metric-details-trigger")
                    .custom(
                        gpui_kit::component::button::ButtonCustomVariant::new(cx)
                            .color(rgb(crate::theme::color::TERMINAL).into())
                            .foreground(rgb(crate::theme::color::TERMINAL_TEXT).into())
                            .hover(rgb(crate::theme::color::SIDEBAR_HOVER).into())
                            .active(rgb(crate::theme::color::SIDEBAR_ACTIVE).into()),
                    )
                    .with_size(px(28.))
                    .size(px(28.))
                    .p_0()
                    .accessibility_label(label)
                    .child(
                        Label::new("⋯")
                            .text_size(px(22.))
                            .text_color(rgb(crate::theme::color::TERMINAL_TEXT)),
                    ),
            )
            .content(move |_, window, _| {
                div()
                    .id("stats-metric-details-content")
                    .w(px(460.))
                    .max_h(px(
                        (f32::from(window.viewport_size().height) - 180.).max(180.)
                    ))
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .p(px(12.))
                    .children(snapshot.iter().map(|card| {
                        div()
                            .flex()
                            .flex_col()
                            .flex_shrink_0()
                            .gap(px(4.))
                            .border_b_1()
                            .border_color(rgb(BORDER))
                            .pb(px(10.))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .gap(px(12.))
                                    .child(
                                        Label::new(title(card.id, zh))
                                            .text_size(px(12.))
                                            .text_color(rgb(MUTED)),
                                    )
                                    .child(
                                        Label::new(card.value.clone())
                                            .text_size(px(16.))
                                            .font_weight(FontWeight::SEMIBOLD),
                                    ),
                            )
                            .child(
                                Label::new(card.hint.clone())
                                    .text_size(px(11.))
                                    .text_color(rgb(MUTED)),
                            )
                            .child(
                                Label::new(card.exact.clone())
                                    .text_size(px(11.))
                                    .text_color(rgb(MUTED)),
                            )
                    }))
            })
            .into_any_element()
    }

    pub(super) fn overview(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(dashboard) = self.dashboard.as_ref() else {
            return stats_card("stats-overview")
                .child(usage_table::strong(
                    self.t("正在读取统计…", "Reading statistics…"),
                ))
                .into_any_element();
        };
        let summary = &dashboard.summary;
        // filters_width is the existing chart's padded content width; the V2
        // cards themselves fill its whole parent without another outer card.
        let (hero_width, metric_widths, wide) = kpi_widths(self.filters_width + CARD_PAD * 2. + 2.);
        let compact_total = usage_table::compact(summary.total_tokens, self.zh);
        let (amount, unit) = ["万", "亿", "k", "M", "B"]
            .into_iter()
            .find_map(|unit| {
                compact_total
                    .strip_suffix(unit)
                    .map(|amount| (amount, unit))
            })
            .unwrap_or((&compact_total, ""));
        let total_exact = format!(
            "{} {}",
            self.t("Token 总用量", "Total tokens"),
            number(summary.total_tokens)
        );
        let summary_cards = cards(summary, self.zh);
        let mut total = div()
            .id("stats-token-hero")
            .relative()
            .w(px(hero_width))
            .h(px(168.))
            .flex_shrink_0()
            .flex()
            .flex_col()
            .px(px(25.))
            .pt(px(21.))
            .pb(px(25.))
            .rounded(px(16.))
            .bg(rgb(crate::theme::color::TERMINAL))
            .role(Role::Group)
            .aria_label(total_exact.clone())
            .tooltip(move |window, cx| {
                gpui_kit::component::tooltip::Tooltip::new(total_exact.clone()).build(window, cx)
            })
            .child(
                div().flex().items_center().justify_between().child(
                    crate::studio_fonts::latin(Label::new("TOKEN VOLUME"), cx)
                        .text_size(px(11.))
                        .line_height(px(16.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(crate::theme::color::TERMINAL_TEXT)),
                ),
            )
            .child(
                div()
                    .flex()
                    .items_start()
                    .mt(px(14.))
                    .gap(px(14.))
                    .child(
                        crate::studio_fonts::latin(Label::new(amount.to_string()), cx)
                            .text_size(px(47.))
                            .line_height(px(67.))
                            .font_weight(FontWeight::BOLD)
                            .text_color(rgb(crate::theme::color::WHITE)),
                    )
                    .child(
                        Label::new(unit)
                            .mt(px(21.))
                            .text_size(px(21.))
                            .line_height(px(30.))
                            .font_weight(FontWeight::BOLD)
                            .text_color(rgb(crate::theme::color::WHITE)),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .mt(px(-1.))
                    .gap(px(7.))
                    .child(
                        div()
                            .w(px(145.))
                            .h(px(2.))
                            .bg(rgb(crate::theme::sidebar_accent())),
                    )
                    .child(
                        Label::new(self.t(
                            "当前筛选范围内的 Token 用量",
                            "Token usage in the current filter range",
                        ))
                        .ml(px(1.))
                        .text_size(px(12.))
                        .line_height(px(17.))
                        .text_color(rgb(crate::theme::color::TERMINAL_TEXT)),
                    ),
            );
        total = total.child(
            div()
                .absolute()
                .right(px(16.))
                .top(px(12.))
                .child(self.metric_details(summary, cx)),
        );
        let samples = dashboard
            .series
            .iter()
            .rev()
            .take(14)
            .rev()
            .map(|point| {
                if point.quality == crate::stats_dto::SeriesQuality::Unknown {
                    None
                } else {
                    point.total_tokens
                }
            })
            .collect::<Vec<_>>();
        let heights = spark_heights(&samples);
        if hero_width >= 400. && heights.iter().flatten().any(|height| *height > 0.) {
            total = total.child(
                div()
                    .id("stats-token-sparkline")
                    .absolute()
                    .right(px(20.))
                    .top(px(35.))
                    .h(px(76.))
                    .w(px(190.))
                    .flex()
                    .items_end()
                    .gap(px(6.))
                    .role(Role::Group)
                    .aria_label(self.t(
                        "最近14个时间桶的Token，未知留空",
                        "Tokens in the latest 14 time buckets; unknowns stay blank",
                    ))
                    .children(heights.into_iter().map(|height| {
                        div()
                            .w(px(8.))
                            .h(px(height.unwrap_or(0.)))
                            .rounded(px(3.))
                            .bg(rgb(crate::theme::sidebar_accent()))
                    })),
            );
        }
        let metrics = div().flex().gap(px(METRIC_GAP as f32)).children([
            self.primary_metric(
                PrimaryMetric {
                    label: self.t("用量记录", "Usage records"),
                    value: summary_cards[0].value.clone(),
                    hint: summary_cards[0].hint.clone(),
                    exact: summary_cards[0].exact.clone(),
                    icon: "studio/kpi-grid.svg",
                    font_size: 34.,
                    line_height: 49.,
                },
                metric_widths[0],
                cx,
            ),
            self.primary_metric(
                PrimaryMetric {
                    label: self.t("代理成功率", "Proxy success rate"),
                    value: summary_cards[1].value.clone(),
                    hint: self.t("仅统计代理请求", "Proxy requests only").into(),
                    exact: summary_cards[1].exact.clone(),
                    icon: "studio/kpi-shield.svg",
                    font_size: 27.,
                    line_height: 39.,
                },
                metric_widths[1],
                cx,
            ),
            self.primary_metric(
                PrimaryMetric {
                    label: self.t("缓存命中率", "Cache hit rate"),
                    value: usage_table::percent(summary.cache_hit_rate),
                    hint: format!(
                        "{} {}",
                        self.t("缓存读取", "Cache read"),
                        usage_table::compact(summary.cache_read_tokens, self.zh)
                    ),
                    exact: summary_cards[3].exact.clone(),
                    icon: "studio/kpi-activity.svg",
                    font_size: 34.,
                    line_height: 49.,
                },
                metric_widths[2],
                cx,
            ),
        ]);
        div()
            .id("stats-overview")
            .w_full()
            .flex()
            .gap(px(METRIC_GAP as f32))
            .when(!wide, |row| row.flex_col())
            .child(total)
            .child(metrics)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::{cards, kpi_widths, spark_heights};
    use crate::{statistics::MetricId, stats_dto::SummaryDto};
    #[test]
    fn actual_metric_composition_preserves_separate_counters_in_seven_cards() {
        for zh in [true, false] {
            let values = cards(&SummaryDto::default(), zh);
            assert_eq!(values.len(), 7);
            let ids: Vec<_> = values.iter().map(|card| card.id).collect();
            assert_eq!(
                ids,
                vec![
                    MetricId::UsageRecords,
                    MetricId::ProxySuccessRate,
                    MetricId::InputTokens,
                    MetricId::CacheRead,
                    MetricId::RecordedCoverage,
                    MetricId::EstimatedCost,
                    MetricId::WorkbuddyCredit
                ]
            );
            assert_eq!(values[1].value, "—");
            assert_eq!(values[2].value, "— / —");
            assert_eq!(values[3].value, "— / —");
            assert_eq!(values[6].value, "—");
            let mut summary = SummaryDto {
                credit_nanos: Some("1140000000".into()),
                input_tokens: Some(1200),
                output_tokens: Some(0),
                cache_read_tokens: Some(600),
                cache_write_tokens: Some(20),
                ..SummaryDto::default()
            };
            let populated = cards(&summary, zh);
            assert!(populated[2].value.ends_with(" / 0"));
            assert!(populated[2].exact.contains("1200"));
            assert_eq!(populated[3].value, "600 / 20");
            assert_eq!(populated[6].value, "1.14");
            summary.credit_nanos = Some("0".into());
            assert_eq!(cards(&summary, zh)[6].value, "0");
        }
    }
    #[test]
    fn actual_kpi_layout_fits_both_supported_workspace_widths() {
        for width in [780., 1000., 1120., 1280.] {
            let (hero, metrics, wide) = kpi_widths(width);
            assert!(
                metrics
                    .iter()
                    .all(|metric| *metric >= super::super::METRIC_MIN_WIDTH as f32),
                "readable metric width at {width}"
            );
            let row = if wide {
                hero + metrics.iter().sum::<f32>() + 48.
            } else {
                metrics.iter().sum::<f32>() + 32.
            };
            assert!((row - width).abs() < 0.01);
            assert!(hero <= width);
        }
        assert_eq!(kpi_widths(1120.), (454., [204., 204., 210.], true));
        assert!(!kpi_widths(780.).2);
        assert!(kpi_widths(1120.).2);
    }
    #[test]
    fn sparkline_preserves_unknowns_and_only_uses_the_latest_real_buckets() {
        assert_eq!(
            spark_heights(&[Some(0), None, Some(10), Some(20)]),
            vec![Some(0.), None, Some(38.), Some(76.)]
        );
        assert_eq!(
            spark_heights(&[None, Some(-1), Some(0)]),
            vec![None, None, Some(0.)]
        );
        let mut samples = vec![Some(999); 10];
        samples.extend([Some(20); 14]);
        assert_eq!(spark_heights(&samples), vec![Some(76.); 14]);
        assert!(spark_heights(&[]).is_empty());
    }
}
