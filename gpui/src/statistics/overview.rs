//! Seven metric cards alongside the two-row total-token hero. Input and output
//! share one card without combining their counters. Each source uses this one
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

/// How many metric cards fit in the grid. The design caps the row at four; the
/// card's own minimum width decides when fewer fit, so the row is always filled
/// instead of wrapping early.
fn columns(grid_width: f32) -> usize {
    let unit = METRIC_MIN_WIDTH as f32 + METRIC_GAP as f32;
    (((grid_width + METRIC_GAP as f32) / unit).floor() as usize).clamp(1, 4)
}

/// The hero column width fixed by DESIGN_SPEC §4.1.3.
const HERO_WIDTH: f32 = 260.;
/// The design's metric row height.
const METRIC_HEIGHT: f32 = 124.;
/// Below this the hero moves above the grid instead of beside it.
const HERO_BESIDE_WIDTH: f32 = 720.;

impl Statistics {
    fn metric_card(&self, index: usize, card: Card, width: f32) -> AnyElement {
        let title = title(card.id, self.zh);
        let tooltip = card.exact.clone();
        // The design emphasises the cache-read number: it is the value the
        // reader is meant to compare against the input tokens.
        let value_color = if card.id == MetricId::CacheRead {
            crate::theme::color::PRIMARY_DARK
        } else {
            crate::theme::color::TEXT_1
        };
        div()
            .id(("stats-metric", index))
            .role(Role::Group)
            .aria_label(format!("{title} {}", card.value))
            .flex()
            .flex_col()
            .justify_between()
            .flex_shrink_0()
            .w(px(width))
            .min_w(px(METRIC_MIN_WIDTH as f32))
            .h(px(METRIC_HEIGHT))
            .gap_1()
            .p_4()
            .bg(rgb(crate::theme::color::WHITE))
            .border_1()
            .border_color(rgb(crate::theme::color::BORDER))
            .rounded(px(crate::theme::radius::CARD))
            .tooltip(move |window, cx| {
                gpui_kit::component::tooltip::Tooltip::new(tooltip.clone()).build(window, cx)
            })
            .child(
                Label::new(title)
                    .text_size(px(crate::theme::font::LABEL))
                    .text_color(rgb(crate::theme::color::TEXT_3)),
            )
            .child(
                div()
                    // Two currencies or a long amount would overflow a 22px
                    // card, so long values step down to the card-title size.
                    .text_size(px(if card.value.chars().count() > 14 {
                        crate::theme::font::CARD_TITLE
                    } else {
                        crate::theme::font::KPI
                    }))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(value_color))
                    .child(card.value),
            )
            .child(
                Label::new(card.hint)
                    .text_size(px(crate::theme::font::SMALL))
                    .text_color(rgb(crate::theme::color::TEXT_3)),
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
        let beside = self.filters_width >= HERO_BESIDE_WIDTH;
        // When the hero sits beside the grid it owns a fixed column; the metric
        // cards share what is left. Either way the numbers come from the
        // measured content width, never from a hard-coded guess.
        let grid_width = if beside {
            self.filters_width - HERO_WIDTH - METRIC_GAP as f32
        } else {
            self.filters_width
        };
        let columns = columns(grid_width);
        // One pixel of slack keeps floating point rounding from wrapping a row
        // that the budget says fits.
        let width = (((grid_width - (columns - 1) as f32 * METRIC_GAP as f32) / columns as f32)
            .floor()
            - 1.)
            .max(METRIC_MIN_WIDTH as f32);
        let metrics = cards(summary, self.zh)
            .into_iter()
            .enumerate()
            .map(|(index, card)| self.metric_card(index, card, width))
            .collect::<Vec<_>>();
        let hero = div()
            .id("stats-total-tokens")
            .role(Role::Group)
            .aria_label(format!("{} {main}", self.t("Token 总用量", "Total tokens")))
            .flex()
            .flex_col()
            .justify_between()
            .gap_2()
            .p_4()
            .rounded(px(crate::theme::radius::CARD))
            .bg(rgb(crate::theme::color::HERO_BG))
            .when(beside, |card| {
                card.w(px(HERO_WIDTH))
                    .h(px(METRIC_HEIGHT * 2. + METRIC_GAP as f32))
            })
            .when(!beside, |card| card.w_full())
            .child(
                Label::new(self.t("Token 总用量", "Total tokens"))
                    .text_size(px(crate::theme::font::CARD_TITLE))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(crate::theme::color::PRIMARY_DARK)),
            )
            .child(
                div()
                    .text_size(px(crate::theme::font::HERO))
                    .line_height(px(crate::theme::line_height::HERO))
                    .font_weight(FontWeight::BOLD)
                    .text_color(rgb(crate::theme::color::PRIMARY_DARK))
                    .child(main),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        Label::new(format!(
                            "{}/{} {}",
                            summary.usage_complete_count,
                            summary.usage_eligible_count,
                            self.t("个请求用量完整", "requests with complete usage")
                        ))
                        .text_size(px(crate::theme::font::LABEL))
                        .text_color(rgb(crate::theme::color::TEXT_2)),
                    )
                    .child(
                        Label::new(self.t(
                            "只统计已结束且用量完整的请求",
                            "Finished requests with complete usage only",
                        ))
                        .text_size(px(crate::theme::font::LABEL))
                        .text_color(rgb(crate::theme::color::TEXT_3)),
                    ),
            );
        stats_card("stats-overview")
            .gap_3()
            .when(beside, |card| {
                card.flex_row().items_start().gap(px(METRIC_GAP as f32))
            })
            .child(hero)
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(METRIC_GAP as f32))
                    .when(beside, |grid| grid.flex_1().min_w_0())
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
    fn actual_layout_obeys_the_metric_breakpoints() {
        // The grid area decides the row: four cards from 160px each plus gaps,
        // then three, two and one as the page narrows.
        for (grid, expected) in [
            (1_200., 4),
            (676., 4),
            (675., 3),
            (504., 3),
            (503., 2),
            (332., 2),
            (331., 1),
            (0., 1),
        ] {
            assert_eq!(columns(grid), expected, "grid {grid}");
        }
    }
}
