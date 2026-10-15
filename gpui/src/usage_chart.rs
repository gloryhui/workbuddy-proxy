//! Native multi-series usage chart.
//!
//! One coordinate system, one hover state and a single tooltip for every visible
//! series. Values stay `Option<i64>`: a bucket without confirmed usage breaks
//! the line instead of falling to zero, and unknown/degraded buckets get a
//! shaded background so "no data" never looks like "no traffic".
use crate::{
    stats_dto::{Currency, SeriesPointDto, SeriesQuality},
    usage_table::{compact, exact},
};
use chrono::TimeZone as _;
use gpui_kit::base::plot::{
    Curve, Plot, PlotElement, PlotLabel, TooltipState, label::Text as PlotText, shape::Line,
};
use gpui_kit::*;
use std::collections::BTreeSet;

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

    /// Input blue, output green, cache read purple, cache write orange.
    pub fn color(self) -> u32 {
        match self {
            SeriesKind::Input => 0x2f6fd0,
            SeriesKind::Output => 0x2f9e63,
            SeriesKind::CacheRead => 0x7b5cd6,
            SeriesKind::CacheWrite => 0xd9822b,
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
        max.filter(|value| *value > 0.)
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

    /// Human readable summary for screen readers and empty states.
    pub fn summary(&self) -> String {
        let known = self
            .points
            .iter()
            .filter(|point| point.complete_count > 0)
            .count();
        if self.zh {
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
        let grid = rgb(0xe6edf4);
        let muted = rgb(0x8a98a7);

        // Unknown or lossy buckets are shaded so "no data" is visible.
        for (index, bucket) in self.points.iter().enumerate() {
            if bucket.collection_warning || bucket.quality == SeriesQuality::Unknown {
                let left = Self::x_for(&self.points, &rect, index);
                let half = width / (self.points.len().max(2) as f32 * 2.);
                window.paint_quad(quad(
                    Bounds {
                        origin: point(rect.origin.x + px(left - half), rect.origin.y),
                        size: size(px((half * 2.).max(1.)), px(height)),
                    },
                    px(0.),
                    rgba(0xf1f4f8),
                    px(0.),
                    transparent_black(),
                    BorderStyle::default(),
                ));
            }
        }

        // Horizontal grid plus value labels.
        let mut labels = Vec::new();
        // The axis snaps to a readable 1/2/5 × 10^n maximum.
        let ymax = self.series_max().map(nice_max);
        for step in 0..=4 {
            let ratio = step as f32 / 4.;
            let y = rect.origin.y + px(height * (1. - ratio));
            window.paint_quad(quad(
                Bounds {
                    origin: point(rect.origin.x, y),
                    size: size(px(width), px(1.)),
                },
                px(0.),
                if step == 0 { muted } else { grid },
                px(0.),
                transparent_black(),
                BorderStyle::default(),
            ));
            let value = ymax.map_or(0., |max| max * ratio as f64);
            let text = match self.mode {
                ChartMode::Tokens => compact(Some(value.round() as i64), self.zh),
                ChartMode::Cost => crate::stats_dto::format_amount(value.round() as i64),
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
            let stride = (count as f32 / 7.).ceil().max(1.) as usize;
            for index in (0..count).step_by(stride) {
                let x = Self::x_for(&self.points, &rect, index);
                labels.push(
                    PlotText::new(
                        bucket_label(&self.points[index]),
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
            // Each contiguous known run becomes its own line, so missing
            // buckets break the series instead of being bridged.
            let mut run: Vec<usize> = Vec::new();
            let mut runs: Vec<Vec<usize>> = Vec::new();
            for index in 0..count {
                if entry.ys[index].is_some() {
                    run.push(index);
                } else if !run.is_empty() {
                    runs.push(std::mem::take(&mut run));
                }
            }
            if !run.is_empty() {
                runs.push(run);
            }
            let stroke = match self.mode {
                ChartMode::Tokens => entry.kind.color(),
                ChartMode::Cost => 0xc2185b,
            };
            for run in runs {
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
        let rect = Self::plot_rect(&bounds);
        let index = self.nearest(
            position.x.as_f32() - rect.origin.x.as_f32() + rect.origin.x.as_f32(),
            &rect,
        )?;
        let x = Self::x_for(&self.points, &rect, index);
        Some(TooltipState::new(
            index,
            point(rect.origin.x + px(x), rect.origin.y),
            vec![],
        ))
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
        let _ = bounds;
        let flip = cursor.x.as_f32() > 0.6 * 400.;
        let left = if flip {
            cursor.x - px(268.)
        } else {
            cursor.x + px(14.)
        };
        let mut rows: Vec<AnyElement> = Vec::new();
        rows.push(
            div()
                .text_size(px(11.))
                .text_color(rgb(0x6b7c90))
                .child(format!(
                    "{} → {}",
                    crate::usage_table::timestamp(bucket.start_ms, self.zh),
                    crate::usage_table::timestamp(bucket.end_ms, self.zh)
                ))
                .into_any_element(),
        );
        rows.push(
            div()
                .text_size(px(11.))
                .text_color(rgb(0x19334d))
                .child(if self.zh {
                    format!(
                        "{} 个请求 · {} 已结束 · {} 完整用量",
                        bucket.request_count, bucket.finished_count, bucket.complete_count
                    )
                } else {
                    format!(
                        "{} requests · {} finished · {} complete",
                        bucket.request_count, bucket.finished_count, bucket.complete_count
                    )
                })
                .into_any_element(),
        );
        match self.mode {
            ChartMode::Tokens => {
                for kind in SeriesKind::ALL {
                    if !self.visible.contains(&kind) {
                        continue;
                    }
                    rows.push(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .text_size(px(11.))
                            .child(
                                div()
                                    .w(px(8.))
                                    .h(px(8.))
                                    .rounded(px(4.))
                                    .bg(rgb(kind.color())),
                            )
                            .child(div().text_color(rgb(0x19334d)).child(format!(
                                "{} {}",
                                kind.label(self.zh),
                                exact(bucket.value(kind))
                            )))
                            .into_any_element(),
                    );
                }
            }
            ChartMode::Cost => {
                rows.push(
                    div()
                        .text_size(px(11.))
                        .text_color(rgb(0x19334d))
                        .child(format!(
                            "{} {}",
                            self.currency.as_str(),
                            bucket
                                .cost_nanos
                                .map_or_else(|| "—".into(), crate::stats_dto::format_amount)
                        ))
                        .into_any_element(),
                );
            }
        }
        rows.push(
            div()
                .text_size(px(10.))
                .text_color(rgb(0x8a98a7))
                .child(match bucket.quality {
                    SeriesQuality::Known => {
                        if self.zh {
                            "覆盖率完整"
                        } else {
                            "complete coverage"
                        }
                    }
                    SeriesQuality::Partial => {
                        if self.zh {
                            "部分覆盖"
                        } else {
                            "partial coverage"
                        }
                    }
                    SeriesQuality::Empty => {
                        if self.zh {
                            "确认无请求"
                        } else {
                            "confirmed empty"
                        }
                    }
                    SeriesQuality::Unknown => {
                        if self.zh {
                            "记录可能不全"
                        } else {
                            "records may be incomplete"
                        }
                    }
                })
                .into_any_element(),
        );
        Some(
            div()
                .absolute()
                .left(left)
                .top(px(8.))
                .w(px(254.))
                .p_2()
                .flex()
                .flex_col()
                .gap_1()
                .bg(rgb(0xffffff))
                .border_1()
                .border_color(rgb(0xdbe5ee))
                .rounded(px(8.))
                .shadow_md()
                .children(rows)
                .into_any_element(),
        )
    }
}

impl IntoElement for UsagePlot {
    type Element = PlotElement<Self>;

    fn into_element(self) -> Self::Element {
        PlotElement::new(self)
    }
}

fn bucket_label(bucket: &ChartPoint) -> String {
    match chrono::Local.timestamp_millis_opt(bucket.start_ms).single() {
        Some(value) => {
            let span = bucket.end_ms - bucket.start_ms;
            if span <= 36 * 3_600_000 {
                value.format("%H:%M").to_string()
            } else {
                value.format("%m-%d").to_string()
            }
        }
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    // Explicit imports only: a glob import would bring GPUI's `#[gpui::test]`
    // attribute into scope and shadow the built-in `#[test]` macro.
    use super::{ChartPoint, SeriesQuality, nice_max};
    use crate::stats_dto::SeriesPointDto;

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
}
