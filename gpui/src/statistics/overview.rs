//! The nine actual metric cards, in U08 order. Each source uses this one
//! composition path; numeric tooltips retain exact values and distinct units.
use super::*;
use crate::stats_dto::SummaryDto;

struct Card {
    id: MetricId,
    value: String,
    hint: String,
    exact: String,
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
        return id.title();
    }
    match id {
        MetricId::UsageRecords => "Usage records",
        MetricId::ProxySuccessRate => "Proxy success rate",
        MetricId::InputTokens => "Input tokens",
        MetricId::OutputTokens => "Output tokens",
        MetricId::CacheRead => "Cache read",
        MetricId::CacheWrite => "Cache write",
        MetricId::RecordedCoverage => "Recorded usage completeness",
        MetricId::EstimatedCost => "Proxy estimated cost",
        MetricId::WorkbuddyCredit => "WorkBuddy credits",
    }
}

fn token_cards(summary: &SummaryDto, zh: bool) -> Vec<Card> {
    let mut cards = Vec::with_capacity(4);
    for (id, value) in [
        (MetricId::InputTokens, summary.input_tokens),
        (MetricId::OutputTokens, summary.output_tokens),
        (MetricId::CacheRead, summary.cache_read_tokens),
        (MetricId::CacheWrite, summary.cache_write_tokens),
    ] {
        cards.push(Card {
            id,
            value: usage_table::compact(value, zh),
            exact: number(value),
            hint: match id {
                MetricId::CacheRead => format!(
                    "{} {}",
                    text(zh, "命中率", "Hit rate"),
                    usage_table::percent(summary.cache_hit_rate)
                ),
                MetricId::CacheWrite => {
                    text(zh, "输入子集，不可相加", "Input subset; never add twice").into()
                }
                _ => text(
                    zh,
                    "只汇总用量完整的已记录条目",
                    "Complete recorded usage only",
                )
                .into(),
            },
        });
    }
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

fn columns(available: f32) -> usize {
    if available >= 1_000. {
        5
    } else if available >= 600. {
        3
    } else {
        2
    }
}

impl Statistics {
    fn metric_card(&self, index: usize, card: Card, width: f32) -> AnyElement {
        let title = title(card.id, self.zh);
        let tooltip = card.exact.clone();
        div()
            .id(("stats-metric", index))
            .role(Role::Group)
            .aria_label(format!("{title} {}", card.value))
            .flex()
            .flex_col()
            .flex_shrink_0()
            .w(px(width))
            .min_w(px(METRIC_MIN_WIDTH as f32))
            .gap_1()
            .p_3()
            .rounded(px(8.))
            .bg(rgb(BG))
            .tooltip(move |window, cx| {
                gpui_kit::component::tooltip::Tooltip::new(tooltip.clone()).build(window, cx)
            })
            .child(Label::new(title).text_size(px(11.)).text_color(rgb(MUTED)))
            .child(
                div()
                    .text_size(px(16.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(card.value),
            )
            .child(
                Label::new(card.hint)
                    .text_size(px(10.))
                    .text_color(rgb(MUTED)),
            )
            .into_any_element()
    }

    pub(super) fn overview(&self, _cx: &mut Context<Self>) -> AnyElement {
        let Some(dashboard) = self.dashboard.as_ref() else {
            return stats_card("stats-overview")
                .child(usage_table::strong(
                    self.t("正在读取统计…", "Reading statistics…"),
                ))
                .into_any_element();
        };
        let summary = &dashboard.summary;
        let main = usage_table::compact(summary.total_tokens, self.zh);
        let columns = columns(self.filters_width);
        let width = ((self.filters_width - (columns - 1) as f32 * METRIC_GAP as f32)
            / columns as f32)
            .floor()
            .max(METRIC_MIN_WIDTH as f32);
        let metrics = cards(summary, self.zh)
            .into_iter()
            .enumerate()
            .map(|(index, card)| self.metric_card(index, card, width));
        stats_card("stats-overview")
            .child(
                div()
                    .id("stats-total-tokens")
                    .role(Role::Group)
                    .aria_label(format!("{} {main}", self.t("Token 总用量", "Total tokens")))
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(stats_heading(self.t("Token 总用量", "Total tokens")))
                    .child(
                        div()
                            .text_size(px(28.))
                            .font_weight(FontWeight::BOLD)
                            .child(main),
                    )
                    .child(
                        Label::new(format!(
                            "{}/{} {}",
                            summary.usage_complete_count,
                            summary.usage_eligible_count,
                            self.t("条记录用量完整", "records with complete usage")
                        ))
                        .text_size(px(11.))
                        .text_color(rgb(MUTED)),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(METRIC_GAP as f32))
                    .children(metrics),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::{cards, columns};
    use crate::{statistics::MetricId, stats_dto::SummaryDto};
    #[test]
    fn actual_metric_composition_is_exactly_nine_unique_cards_with_honest_unknowns() {
        for zh in [true, false] {
            let values = cards(&SummaryDto::default(), zh);
            assert_eq!(values.len(), 9);
            let ids: Vec<_> = values.iter().map(|card| card.id).collect();
            assert_eq!(
                ids,
                vec![
                    MetricId::UsageRecords,
                    MetricId::ProxySuccessRate,
                    MetricId::InputTokens,
                    MetricId::OutputTokens,
                    MetricId::CacheRead,
                    MetricId::CacheWrite,
                    MetricId::RecordedCoverage,
                    MetricId::EstimatedCost,
                    MetricId::WorkbuddyCredit
                ]
            );
            assert_eq!(values[1].value, "—");
            assert_eq!(values[8].value, "—");
            let mut summary = SummaryDto {
                credit_nanos: Some("1140000000".into()),
                ..SummaryDto::default()
            };
            assert_eq!(cards(&summary, zh)[8].value, "1.14");
            summary.credit_nanos = Some("0".into());
            assert_eq!(cards(&summary, zh)[8].value, "0");
        }
    }
    #[test]
    fn actual_layout_obeys_the_three_metric_breakpoints() {
        for (width, expected) in [(1_000., 5), (999., 3), (600., 3), (599., 2)] {
            assert_eq!(columns(width), expected);
        }
    }
}
