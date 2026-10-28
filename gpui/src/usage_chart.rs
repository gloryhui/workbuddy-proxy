//! Native multi-series usage chart.
//!
//! One coordinate system, one hover state and a single tooltip for every visible
//! series. Values stay `Option<i64>`: unknown intervals are crossed with a
//! labelled dashed guide instead of falling to zero. Quality uses a compact
//! warning and the shared native tooltip, leaving the plot background white.
use crate::{
    stats_dto::{Currency, SeriesPointDto, SeriesQuality},
    usage_table::{compact, exact},
};
use chrono::TimeZone as _;
use gpui_kit::base::plot::{
    Curve, Plot, PlotElement, PlotLabel, TooltipState, label::Text as PlotText, shape::Line,
};
use gpui_kit::component::plot::tooltip::{CrossLine, Dot, Tooltip};
use gpui_kit::*;
use std::collections::BTreeSet;
mod continuity;

/// Order matters: it drives the legend and the tooltip rows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SeriesKind {
    Input,
    Output,
    CacheRead,
    CacheWrite,
}

impl SeriesKind {
    pub const ALL: [SeriesKind; 4] = [
        SeriesKind::Input,
        SeriesKind::Output,
        SeriesKind::CacheRead,
        SeriesKind::CacheWrite,
    ];

    pub fn label(self, zh: bool) -> &'static str {
        match self {
            SeriesKind::Input => {
                if zh {
                    "输入"
                } else {
                    "Input"
                }
            }
            SeriesKind::Output => {
                if zh {
                    "输出"
                } else {
                    "Output"
                }
            }
            SeriesKind::CacheRead => {
                if zh {
                    "缓存读"
                } else {
                    "Cache read"
                }
            }
            SeriesKind::CacheWrite => {
                if zh {
                    "缓存写"
                } else {
                    "Cache write"
                }
            }
        }
    }

    /// V2 input/cache accents; retain the additional output and write series.
    pub fn color(self) -> u32 {
        match self {
            SeriesKind::Input => crate::theme::color::CHART_INPUT,
            SeriesKind::Output => crate::theme::color::CHART_OUTPUT,
            SeriesKind::CacheRead => crate::theme::color::CHART_CACHE_READ,
            SeriesKind::CacheWrite => crate::theme::color::CHART_CACHE_WRITE,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChartMode {
    Tokens,
    Cost,
}

/// One plotted bucket, derived from a dashboard series point.
#[derive(Clone, Debug)]
pub struct ChartPoint {
    pub start_ms: i64,
    pub end_ms: i64,
    pub center_ms: i64,
    pub request_count: i64,
    pub finished_count: i64,
    pub complete_count: i64,
    pub input: Option<i64>,
    pub output: Option<i64>,
    pub cache_read: Option<i64>,
    pub cache_write: Option<i64>,
    pub cost_nanos: Option<i64>,
    pub quality: SeriesQuality,
    pub collection_warning: bool,
}

impl ChartPoint {
    /// Token visibility is independent of whether the selected currency has
    /// priced proxy calls. Return both plots together to keep that boundary.
    pub fn split_series(points: &[SeriesPointDto], currency: Currency) -> (Vec<Self>, Vec<Self>) {
        (
            Self::from_series(points, currency),
            Self::currency_series(points, currency),
        )
    }

    /// The points of one currency series, or nothing at all.
    ///
    /// A range with no priced proxy call has no currency line to draw: returning
    /// an empty series hides it, where a series of zeros would claim the calls
    /// were free.
    pub fn currency_series(points: &[SeriesPointDto], currency: Currency) -> Vec<Self> {
        let has_proxy_cost = points
            .iter()
            .any(|point| point.costs.iter().any(|cost| cost.currency == currency));
        if !should_draw_currency_line(has_proxy_cost) {
            return Vec::new();
        }
        Self::from_series(points, currency)
    }

    pub fn from_series(points: &[SeriesPointDto], currency: Currency) -> Vec<Self> {
        points
            .iter()
            .map(|point| Self {
                start_ms: point.start_ms,
                end_ms: point.end_ms,
                center_ms: point.start_ms + (point.end_ms - point.start_ms) / 2,
                request_count: point.request_count,
                finished_count: point.finished_count,
                complete_count: point.usage_complete_count,
                input: point.input_tokens,
                output: point.output_tokens,
                cache_read: point.cache_read_tokens,
                cache_write: point.cache_write_tokens,
                cost_nanos: point
                    .costs
                    .iter()
                    .find(|cost| cost.currency == currency)
                    .and_then(|cost| cost.total_cost_nanos.parse::<i64>().ok()),
                quality: point.quality,
                collection_warning: point.collection_warning,
            })
            .collect()
    }

    fn value(&self, kind: SeriesKind) -> Option<i64> {
        match kind {
            SeriesKind::Input => self.input,
            SeriesKind::Output => self.output,
            SeriesKind::CacheRead => self.cache_read,
            SeriesKind::CacheWrite => self.cache_write,
        }
    }
}

/// Rounds a maximum up to a readable 1/2/5 × 10^n value.
fn nice_max(value: f64) -> f64 {
    if !value.is_finite() || value <= 0. {
        return 1.;
    }
    let exponent = value.log10().floor();
    let base = 10f64.powf(exponent);
    let scaled = value / base;
    let step = if scaled <= 1. {
        1.
    } else if scaled <= 2. {
        2.
    } else if scaled <= 5. {
        5.
    } else {
        10.
    };
    step * base
}

/// Keep zero-cost plots readable, with distinct numeric ticks. Axis labels use
/// enough decimal places for the chosen step rather than the money-cell cutoff.
fn cost_axis_max(value: f64) -> f64 {
    nice_max(if value == 0. {
        20_000_000.
    } else {
        value.max(4.)
    })
}

fn cost_axis_label(nanos: f64, maximum: f64) -> String {
    if nanos == 0. {
        return "0".into();
    }
    let step = maximum / 4. / 1_000_000_000.;
    let precision = (-step.log10().floor()).clamp(4., 9.) as usize;
    format!("{:.*}", precision, nanos / 1_000_000_000.)
}

/// One series projected into plot pixels.
struct ScreenSeries {
    kind: SeriesKind,
    ys: Vec<Option<f32>>,
}

const PAD_LEFT: f32 = 64.;
const PAD_RIGHT: f32 = 16.;
const PAD_TOP: f32 = 12.;
const PAD_BOTTOM: f32 = 28.;

pub struct UsagePlot {
    pub id: ElementId,
    pub points: Vec<ChartPoint>,
    pub visible: BTreeSet<SeriesKind>,
    pub zh: bool,
    pub mode: ChartMode,
    pub currency: Currency,
}

impl UsagePlot {
    fn plot_rect(bounds: &Bounds<Pixels>) -> Bounds<Pixels> {
        let width = (bounds.size.width.as_f32() - PAD_LEFT - PAD_RIGHT).max(1.);
        let height = (bounds.size.height.as_f32() - PAD_TOP - PAD_BOTTOM).max(1.);
        Bounds {
            origin: point(
                bounds.origin.x + px(PAD_LEFT),
                bounds.origin.y + px(PAD_TOP),
            ),
            size: size(px(width), px(height)),
        }
    }

    fn series_max(&self) -> Option<f64> {
        let mut max: Option<f64> = None;
        for bucket in &self.points {
            let value = match self.mode {
                ChartMode::Tokens => SeriesKind::ALL
                    .iter()
                    .filter(|kind| self.visible.contains(kind))
                    .filter_map(|kind| bucket.value(*kind))
                    .max(),
                ChartMode::Cost => bucket.cost_nanos,
            };
            if let Some(value) = value {
                let value = value as f64;
                max = Some(max.map_or(value, |current: f64| current.max(value)));
            }
        }
        max
    }

    fn axis_max(&self) -> Option<f64> {
        self.series_max().map(|value| match self.mode {
            ChartMode::Tokens => nice_max(value),
            ChartMode::Cost => cost_axis_max(value),
        })
    }

    /// Maps a bucket to the plot width by its real UTC centre time, so a 23 or
    /// 25 hour DST day keeps its true share of the axis.
    fn x_for(points: &[ChartPoint], rect: &Bounds<Pixels>, index: usize) -> f32 {
        let width = rect.size.width.as_f32();
        if points.len() <= 1 {
            return width / 2.;
        }
        let first = points[0].center_ms as f64;
        let last = points[points.len() - 1].center_ms as f64;
        if last <= first {
            return width / 2.;
        }
        let centre = points[index].center_ms as f64;
        (((centre - first) / (last - first)) as f32 * width).clamp(0., width)
    }

    fn nearest(&self, cursor_x: f32, rect: &Bounds<Pixels>) -> Option<usize> {
        if self.points.is_empty() {
            return None;
        }
        let count = self.points.len();
        let mut best = 0usize;
        let mut best_distance = f32::MAX;
        for index in 0..count {
            let distance = (Self::x_for(&self.points, rect, index) - cursor_x).abs();
            if distance < best_distance {
                best_distance = distance;
                best = index;
            }
        }
        Some(best)
    }

    // Plot receives cursor positions relative to its own bounds, including
    // the label gutters. Subtract only the local left gutter, never screen origin.
    fn hover_index(&self, position: Point<Pixels>, bounds: &Bounds<Pixels>) -> Option<usize> {
        let x = position.x.as_f32();
        let y = position.y.as_f32();
        if x < PAD_LEFT
            || x > bounds.size.width.as_f32() - PAD_RIGHT
            || y < PAD_TOP
            || y > bounds.size.height.as_f32() - PAD_BOTTOM
        {
            return None;
        }
        self.nearest(x - PAD_LEFT, &Self::plot_rect(bounds))
    }

    /// Human readable summary for screen readers and empty states.
    pub fn summary(&self) -> String {
        let known = self
            .points
            .iter()
            .filter(|point| point.complete_count > 0)
            .count();
        let summary = if self.zh {
            format!(
                "{} 个时间桶，其中 {} 个有完整用量",
                self.points.len(),
                known
            )
        } else {
            format!(
                "{} buckets, {} with complete usage",
                self.points.len(),
                known
            )
        };
        match self.continuity_note() {
            Some(note) => format!("{summary}; {note}"),
            None => summary,
        }
    }
}

impl Plot for UsagePlot {
    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn paint(&mut self, bounds: Bounds<Pixels>, window: &mut Window, cx: &mut App) {
        let rect = Self::plot_rect(&bounds);
        let width = rect.size.width.as_f32();
        let height = rect.size.height.as_f32();
        let grid = rgb(0xeff2f6);
        let muted = rgb(0x8591a2);

        // Horizontal grid plus value labels.
        let mut labels = Vec::new();
        // The axis snaps to a readable 1/2/5 × 10^n maximum.
        let ymax = self.axis_max();
        for step in 0..=4 {
            let ratio = step as f32 / 4.;
            let y = rect.origin.y + px(height * (1. - ratio));
            window.paint_quad(quad(
                Bounds {
                    origin: point(rect.origin.x, y),
                    size: size(px(width), px(1.)),
                },
                px(0.),
                if step == 0 { rgb(0xdce3eb) } else { grid },
                px(0.),
                transparent_black(),
                BorderStyle::default(),
            ));
            if ymax.is_none() && step > 0 {
                continue;
            }
            let value = ymax.map_or(0., |max| max * ratio as f64);
            let text = match self.mode {
                ChartMode::Tokens => compact(Some(value.round() as i64), self.zh),
                ChartMode::Cost => cost_axis_label(value, ymax.unwrap_or(1.)),
            };
            labels.push(
                PlotText::new(
                    text,
                    point(px(-8.), y - rect.origin.y - px(5.)),
                    muted.into(),
                )
                .align(gpui::TextAlign::Right),
            );
        }

        // X axis labels: at most eight, never overlapping by construction.
        let count = self.points.len();
        if count > 0 {
            let show_date =
                self.points.last().unwrap().end_ms - self.points[0].start_ms > 36 * 3_600_000;
            for index in tick_indices(count, width) {
                let x = Self::x_for(&self.points, &rect, index);
                labels.push(
                    PlotText::new(
                        bucket_label(&self.points[index], show_date),
                        point(px(x), px(height + 6.)),
                        muted.into(),
                    )
                    .align(gpui::TextAlign::Center),
                );
            }
        }
        PlotLabel::new(labels).paint(
            &Bounds {
                origin: rect.origin,
                size: rect.size,
            },
            window,
            cx,
        );

        if ymax.is_none() {
            return;
        }
        let ymax = ymax.unwrap_or(1.);

        // Pre-compute every projected point so the line closures own their
        // data instead of borrowing `self` (they must be `'static`).
        let xs: Vec<Option<f32>> = (0..count)
            .map(|index| Some(Self::x_for(&self.points, &rect, index)))
            .collect();
        let tokens: Vec<ScreenSeries> = SeriesKind::ALL
            .into_iter()
            .filter(|kind| self.visible.contains(kind))
            .map(|kind| ScreenSeries {
                kind,
                ys: self
                    .points
                    .iter()
                    .map(|point| {
                        point
                            .value(kind)
                            .map(|value| height * (1. - (value as f64 / ymax) as f32))
                    })
                    .collect(),
            })
            .collect();
        let cost = ScreenSeries {
            kind: SeriesKind::Input,
            ys: self
                .points
                .iter()
                .map(|point| {
                    point
                        .cost_nanos
                        .map(|value| height * (1. - (value as f64 / ymax) as f32))
                })
                .collect(),
        };
        let series: Vec<&ScreenSeries> = match self.mode {
            ChartMode::Tokens => tokens.iter().collect(),
            ChartMode::Cost => vec![&cost],
        };
        for entry in series {
            let segments = continuity::segments(&entry.ys);
            let stroke = match self.mode {
                ChartMode::Tokens => entry.kind.color(),
                ChartMode::Cost => 0xc2185b,
            };
            // Guides use only the two measured endpoints. Missing buckets stay
            // None in the series and tooltip; no interpolated values are made.
            for (left, right) in segments.bridges {
                let mut path = PathBuilder::stroke(px(2.)).dash_array(&[px(6.), px(4.)]);
                let endpoint = |index: usize| {
                    point(
                        rect.origin.x + px(xs[index].unwrap()),
                        rect.origin.y + px(entry.ys[index].unwrap()),
                    )
                };
                path.move_to(endpoint(left));
                path.line_to(endpoint(right));
                if let Ok(path) = path.build() {
                    window.paint_path(path, rgb(stroke).opacity(0.65));
                }
            }
            for run in segments.runs {
                let single = run.len() == 1;
                let xs = xs.clone();
                let ys = entry.ys.clone();
                let mut line = Line::new()
                    .data(run)
                    .x(move |index: &usize| xs.get(*index).copied().flatten())
                    .y(move |index: &usize| ys.get(*index).copied().flatten())
                    .stroke(rgb(stroke))
                    .stroke_width(px(2.))
                    .curve(Curve::Linear);
                if single {
                    line = line.dot().dot_fill(rgb(stroke)).dot_size(px(6.));
                }
                line.paint(
                    &Bounds {
                        origin: rect.origin,
                        size: rect.size,
                    },
                    window,
                );
            }
        }
    }

    fn tooltip_state(
        &self,
        position: Point<Pixels>,
        bounds: Bounds<Pixels>,
        _: &App,
    ) -> Option<TooltipState> {
        let index = self.hover_index(position, &bounds)?;
        let rect = Self::plot_rect(&bounds);
        let x = PAD_LEFT + Self::x_for(&self.points, &rect, index);
        Some(TooltipState::new(index, point(px(x), px(PAD_TOP)), vec![]))
    }

    fn tooltip(
        &self,
        state: &TooltipState,
        cursor: Point<Pixels>,
        bounds: Bounds<Pixels>,
        _: &mut Window,
        _: &mut App,
    ) -> Option<AnyElement> {
        let bucket = self.points.get(state.index)?;
        let rect = Self::plot_rect(&bounds);
        let mut tooltip = Tooltip::new(cursor, bounds.size)
            .cross_line(CrossLine::new(state.cross_line).span(PAD_TOP, rect.size.height.as_f32()))
            .title(format!(
                "{} → {}",
                crate::usage_table::timestamp(bucket.start_ms, self.zh),
                crate::usage_table::timestamp(bucket.end_ms, self.zh)
            ))
            .plain_row(
                if self.zh {
                    "请求 / 已结束 / 完整用量"
                } else {
                    "Requests / finished / complete"
                },
                format!(
                    "{} / {} / {}",
                    bucket.request_count, bucket.finished_count, bucket.complete_count
                ),
            );
        let mut dots = Vec::new();
        let ymax = self.axis_max();
        let mut add_dot = |value: Option<i64>, color: u32| {
            if let (Some(value), Some(ymax)) = (value, ymax) {
                let y = PAD_TOP + rect.size.height.as_f32() * (1. - (value as f64 / ymax) as f32);
                dots.push(
                    Dot::new(point(state.cross_line.x, px(y)))
                        .size(5.)
                        .halo(10.)
                        .stroke(rgb(0xffffff).into())
                        .fill(rgb(color).into()),
                );
            }
        };
        match self.mode {
            ChartMode::Tokens => {
                for kind in SeriesKind::ALL {
                    if self.visible.contains(&kind) {
                        tooltip = tooltip.row(
                            rgb(kind.color()),
                            kind.label(self.zh),
                            exact(bucket.value(kind)),
                        );
                        add_dot(bucket.value(kind), kind.color());
                    }
                }
            }
            ChartMode::Cost => {
                tooltip = tooltip.row(
                    rgb(0xc2185b),
                    self.currency.as_str(),
                    bucket
                        .cost_nanos
                        .map_or_else(|| "—".into(), crate::stats_dto::format_amount),
                );
                add_dot(bucket.cost_nanos, 0xc2185b);
            }
        }
        let quality = match bucket.quality {
            SeriesQuality::Known => {
                if self.zh {
                    "完整"
                } else {
                    "Complete"
                }
            }
            SeriesQuality::Partial => {
                if self.zh {
                    "部分覆盖"
                } else {
                    "Partial"
                }
            }
            SeriesQuality::Empty => {
                if self.zh {
                    "确认无请求"
                } else {
                    "Confirmed empty"
                }
            }
            SeriesQuality::Unknown => {
                if self.zh {
                    "记录可能不全"
                } else {
                    "May be incomplete"
                }
            }
        };
        tooltip = tooltip.plain_row(if self.zh { "数据覆盖" } else { "Coverage" }, quality);
        for (label, explanation) in self.gap_hints(state.index) {
            tooltip = tooltip.plain_row(label, explanation);
        }
        if bucket.collection_warning {
            tooltip = tooltip.plain_row(
                if self.zh {
                    "采集提示"
                } else {
                    "Collection"
                },
                if self.zh {
                    "该时段记录可能不全"
                } else {
                    "Records may be incomplete"
                },
            );
        }
        Some(tooltip.dots(dots).into_any_element())
    }
}

impl IntoElement for UsagePlot {
    type Element = PlotElement<Self>;

    fn into_element(self) -> Self::Element {
        PlotElement::new(self)
    }
}

/// Include both endpoints and space labels according to actual plot width.
fn tick_indices(count: usize, width: f32) -> Vec<usize> {
    if count == 0 {
        return Vec::new();
    }
    if count == 1 {
        return vec![0];
    }
    let ticks = ((width / 90.).floor() as usize).clamp(2, 8).min(count);
    (0..ticks).map(|i| i * (count - 1) / (ticks - 1)).collect()
}

fn bucket_label(bucket: &ChartPoint, show_date: bool) -> String {
    match chrono::Local.timestamp_millis_opt(bucket.start_ms).single() {
        Some(value) => {
            if !show_date {
                value.format("%H:%M").to_string()
            } else {
                value.format("%m/%d").to_string()
            }
        }
        None => String::new(),
    }
}

/// True when a currency series should be drawn at all.
///
/// A range that only contains imported records has no price for them, so there
/// is nothing to plot. Drawing a flat zero line would say "these calls were
/// free", which is exactly the claim the design forbids — the line is hidden
/// instead, and the cost card says how many imports are unpriced.
pub fn should_draw_currency_line(has_proxy_cost: bool) -> bool {
    has_proxy_cost
}

#[cfg(test)]
mod tests {
    // Explicit imports only: a glob import would bring GPUI's `#[gpui::test]`
    // attribute into scope and shadow the built-in `#[test]` macro.
    use super::{
        ChartMode, ChartPoint, SeriesQuality, UsagePlot, bucket_label, cost_axis_label,
        cost_axis_max, nice_max, tick_indices,
    };
    use crate::stats_dto::SeriesPointDto;
    use gpui_kit::{Bounds, ElementId, point as position, px, size};
    use std::collections::BTreeSet;

    #[test]
    fn nice_max_lands_on_readable_steps() {
        assert_eq!(nice_max(0.4), 0.5);
        assert_eq!(nice_max(1.0), 1.0);
        assert_eq!(nice_max(11.0), 20.0);
        assert_eq!(nice_max(230.0), 500.0);
        assert_eq!(nice_max(1100.0), 2000.0);
        assert_eq!(nice_max(0.0), 1.0);
        assert_eq!(nice_max(f64::NAN), 1.0);
    }

    #[test]
    fn currency_ticks_remain_distinct_for_zero_and_sub_cent_costs() {
        assert_eq!(cost_axis_max(0.), 20_000_000.);
        for maximum in [0., 1., 1_000., 9_000_000., 3_000_000_000.] {
            let top = cost_axis_max(maximum);
            let ticks = (0..=4)
                .map(|i| cost_axis_label(top * i as f64 / 4., top))
                .collect::<std::collections::BTreeSet<_>>();
            assert_eq!(ticks.len(), 5, "distinct ticks for {maximum} nanos");
        }
        assert_eq!(cost_axis_label(20_000_000., 20_000_000.), "0.0200");
        assert_eq!(cost_axis_label(0., 20_000_000.), "0");
    }

    fn point(
        start_ms: i64,
        complete: i64,
        input: Option<i64>,
        quality: SeriesQuality,
        warning: bool,
    ) -> SeriesPointDto {
        SeriesPointDto {
            start_ms,
            end_ms: start_ms + 3_600_000,
            request_count: 1,
            finished_count: 1,
            usage_complete_count: complete,
            input_tokens: input,
            output_tokens: input.map(|value| value / 10),
            total_tokens: input.map(|value| value + value / 10),
            cache_read_tokens: None,
            cache_write_tokens: None,
            cache_read_known_count: 0,
            cache_write_known_count: 0,
            cache_read_denominator_input_tokens: None,
            costs: Vec::new(),
            quality,
            collection_warning: warning,
        }
    }

    #[test]
    fn unpriced_imports_keep_token_plot_without_fabricating_currency_plot() {
        let mut imported = point(0, 1, Some(100), SeriesQuality::Known, true);
        imported.finished_count = 0;
        for currency in [
            crate::stats_dto::Currency::Usd,
            crate::stats_dto::Currency::Cny,
        ] {
            let (tokens, costs) = ChartPoint::split_series(&[imported.clone()], currency);
            assert_eq!(tokens.len(), 1);
            assert_eq!(tokens[0].input, Some(100));
            assert_eq!(tokens[0].output, Some(10));
            assert_eq!(tokens[0].finished_count, 0);
            assert!(tokens[0].collection_warning);
            assert!(costs.is_empty());
        }
    }

    #[test]
    fn unknown_buckets_never_become_zero_values() {
        let known = ChartPoint {
            start_ms: 0,
            end_ms: 3_600_000,
            center_ms: 1_800_000,
            request_count: 1,
            finished_count: 1,
            complete_count: 1,
            input: Some(100),
            output: Some(10),
            cache_read: None,
            cache_write: None,
            cost_nanos: None,
            quality: SeriesQuality::Known,
            collection_warning: false,
        };
        let unknown = ChartPoint {
            input: None,
            output: None,
            complete_count: 0,
            quality: SeriesQuality::Unknown,
            collection_warning: true,
            start_ms: 3_600_000,
            end_ms: 7_200_000,
            center_ms: 5_400_000,
            ..known.clone()
        };
        assert_eq!(known.input, Some(100));
        assert_eq!(unknown.input, None, "unknown usage must stay unknown");
        assert!(unknown.collection_warning);
        assert_eq!(known.center_ms, 1_800_000);
    }

    #[test]
    fn series_conversion_keeps_missing_values_missing() {
        let points = [
            point(0, 1, Some(100), SeriesQuality::Known, false),
            point(3_600_000, 0, None, SeriesQuality::Unknown, true),
        ];
        let chart = ChartPoint::from_series(&points, crate::stats_dto::Currency::Usd);
        assert_eq!(chart.len(), 2);
        assert_eq!(chart[0].input, Some(100));
        assert_eq!(chart[1].input, None);
        assert!(chart[1].collection_warning);
        assert_eq!(chart[0].center_ms, 1_800_000);
    }

    fn continuity_plot(values: &[Option<i64>]) -> UsagePlot {
        let points = values
            .iter()
            .enumerate()
            .map(|(index, value)| {
                point(
                    index as i64 * 3_600_000,
                    i64::from(value.is_some()),
                    *value,
                    if value.is_some() {
                        SeriesQuality::Known
                    } else {
                        SeriesQuality::Unknown
                    },
                    value.is_none(),
                )
            })
            .collect::<Vec<_>>();
        UsagePlot {
            id: "continuity-test".into(),
            points: ChartPoint::from_series(&points, crate::stats_dto::Currency::Usd),
            visible: BTreeSet::from([super::SeriesKind::Input]),
            zh: true,
            mode: ChartMode::Tokens,
            currency: crate::stats_dto::Currency::Usd,
        }
    }

    #[test]
    fn tooltip_explains_gaps_without_interpolating_or_changing_totals() {
        let plot = continuity_plot(&[Some(100), None, None, Some(400)]);
        let note = plot.continuity_note().unwrap();
        assert!(note.contains("虚线") && note.contains("不代表"));
        assert!(plot.gap_hints(0).is_empty() && plot.gap_hints(3).is_empty());
        for index in [1, 2] {
            let hints = plot.gap_hints(index);
            assert_eq!(hints.len(), 1);
            assert_eq!(hints[0].0, "输入缺失区间");
            assert!(hints[0].1.contains("缺失值未知"));
            assert_eq!(plot.points[index].input, None);
        }
        assert_eq!(plot.points.iter().filter_map(|p| p.input).sum::<i64>(), 500);
        assert!(plot.summary().contains("虚线"));
    }

    #[test]
    fn single_zero_sample_has_a_hint_but_empty_or_continuous_series_do_not() {
        let plot = continuity_plot(&[None, Some(0), None]);
        assert!(plot.continuity_note().unwrap().contains("仅 1 个数据点"));
        assert!(plot.gap_hints(0).is_empty() && plot.gap_hints(2).is_empty());
        assert!(continuity_plot(&[None, None]).continuity_note().is_none());
        assert!(
            continuity_plot(&[Some(0), Some(0)])
                .continuity_note()
                .is_none()
        );
        let mut english = plot;
        english.zh = false;
        assert!(
            english
                .continuity_note()
                .unwrap()
                .contains("only 1 data point")
        );
    }

    #[test]
    fn cache_and_currency_gaps_use_their_own_values_and_visible_series() {
        let mut plot = continuity_plot(&[Some(100), Some(200), Some(300)]);
        plot.points[0].cache_read = Some(0);
        plot.points[2].cache_read = Some(30);
        plot.visible.insert(super::SeriesKind::CacheRead);
        assert!(plot.continuity_note().unwrap().contains("虚线"));
        assert_eq!(plot.gap_hints(1)[0].0, "缓存读缺失区间");
        plot.visible.remove(&super::SeriesKind::CacheRead);
        assert!(plot.gap_hints(1).is_empty());
        assert!(plot.continuity_note().is_none());
        plot.mode = ChartMode::Cost;
        plot.currency = crate::stats_dto::Currency::Cny;
        assert!(
            plot.continuity_note().is_none(),
            "Token data is not currency data"
        );
        plot.points[1].cost_nanos = Some(0);
        assert!(plot.continuity_note().unwrap().starts_with("CNY 仅 1"));
        assert!(plot.gap_hints(0).is_empty());
        plot.points[1].cost_nanos = None;
        plot.points[0].cost_nanos = Some(100);
        plot.points[2].cost_nanos = Some(300);
        assert_eq!(plot.gap_hints(1)[0].0, "CNY缺失区间");
        assert_eq!(plot.points[1].cost_nanos, None);
    }

    #[test]
    fn ticks_include_both_endpoints_and_adapt_to_width() {
        assert!(tick_indices(0, 900.).is_empty());
        assert_eq!(tick_indices(1, 900.), vec![0]);
        assert_eq!(tick_indices(30, 200.), vec![0, 29]);
        let ticks = tick_indices(30, 900.);
        assert_eq!(ticks.len(), 8);
        assert_eq!((ticks[0], *ticks.last().unwrap()), (0, 29));
        assert!(ticks.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn daily_axes_show_dates_instead_of_repeated_midnight_labels() {
        let points = [point(
            1_790_000_000_000,
            1,
            Some(100),
            SeriesQuality::Known,
            false,
        )];
        let chart = ChartPoint::from_series(&points, crate::stats_dto::Currency::Usd);
        assert!(bucket_label(&chart[0], true).contains('/'));
        assert!(bucket_label(&chart[0], false).contains(':'));
    }

    #[test]
    fn hover_uses_local_plot_coordinates_after_scrolling() {
        let points = (0..3)
            .map(|i| point(i * 3_600_000, 1, Some(100), SeriesQuality::Known, false))
            .collect::<Vec<_>>();
        let plot = UsagePlot {
            id: ElementId::Name("hover-test".into()),
            points: ChartPoint::from_series(&points, crate::stats_dto::Currency::Usd),
            visible: BTreeSet::new(),
            zh: true,
            mode: ChartMode::Cost,
            currency: crate::stats_dto::Currency::Usd,
        };
        for origin in [(0., 0.), (200., -500.)] {
            let bounds = Bounds::new(
                position(px(origin.0), px(origin.1)),
                size(px(480.), px(240.)),
            );
            for (x, index) in [(64., 0), (264., 1), (464., 2)] {
                assert_eq!(
                    plot.hover_index(position(px(x), px(80.)), &bounds),
                    Some(index)
                );
            }
            assert_eq!(plot.hover_index(position(px(20.), px(80.)), &bounds), None);
            assert_eq!(
                plot.hover_index(position(px(200.), px(235.)), &bounds),
                None
            );
        }
    }
}

#[cfg(test)]
mod currency_line_tests {
    use super::should_draw_currency_line;

    #[test]
    fn an_import_only_range_hides_the_currency_line_instead_of_drawing_a_zero() {
        // AC068: no fake zero line that would imply the calls were free.
        assert!(!should_draw_currency_line(false));
        assert!(should_draw_currency_line(true));
    }
}
