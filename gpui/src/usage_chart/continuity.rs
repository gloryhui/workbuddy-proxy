//! Display-only continuity: unknown data never becomes a measured value.
use super::{ChartMode, SeriesKind, UsagePlot};

#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Segments {
    pub runs: Vec<Vec<usize>>,
    pub bridges: Vec<(usize, usize)>,
}

/// Solid runs contain only adjacent measured buckets. A guide may cross an
/// interior gap, but never extrapolates into an unknown leading/trailing range.
pub(super) fn segments<T>(values: &[Option<T>]) -> Segments {
    let mut result = Segments::default();
    let mut previous = None;
    for (index, value) in values.iter().enumerate() {
        if value.is_none() {
            continue;
        }
        if let Some(last) = previous {
            if last + 1 == index {
                result.runs.last_mut().unwrap().push(index);
            } else {
                result.bridges.push((last, index));
                result.runs.push(vec![index]);
            }
        } else {
            result.runs.push(vec![index]);
        }
        previous = Some(index);
    }
    result
}

impl UsagePlot {
    fn displayed_values(&self) -> Vec<(&'static str, Vec<Option<i64>>)> {
        match self.mode {
            ChartMode::Tokens => SeriesKind::ALL
                .into_iter()
                .filter(|kind| self.visible.contains(kind))
                .map(|kind| {
                    (
                        kind.label(self.zh),
                        self.points.iter().map(|p| p.value(kind)).collect(),
                    )
                })
                .collect(),
            ChartMode::Cost => vec![(
                self.currency.as_str(),
                self.points.iter().map(|p| p.cost_nanos).collect(),
            )],
        }
    }

    /// Visible beside the chart; count each series independently so an isolated
    /// cache/currency sample is not mistaken for a continuous series.
    pub fn continuity_note(&self) -> Option<String> {
        let mut singles = Vec::new();
        let mut has_gap = false;
        for (name, values) in self.displayed_values() {
            if values.iter().filter(|v| v.is_some()).count() == 1 {
                singles.push(name);
            }
            has_gap |= !segments(&values).bridges.is_empty();
        }
        let mut messages = Vec::new();
        if !singles.is_empty() {
            messages.push(if self.zh {
                format!("{} 仅 1 个数据点，暂无法形成折线", singles.join("、"))
            } else {
                format!(
                    "{}: only 1 data point; no line can be drawn",
                    singles.join(", ")
                )
            });
        }
        if has_gap {
            messages.push(if self.zh {
                "虚线表示跨越缺失区间，仅连接已知点，不代表期间实际用量或费用".into()
            } else {
                "Dashed guides cross missing intervals; they do not represent measured usage or cost".into()
            });
        }
        (!messages.is_empty()).then(|| messages.join(if self.zh { "；" } else { "; " }))
    }

    pub(super) fn gap_hints(&self, index: usize) -> Vec<(String, String)> {
        self.displayed_values()
            .into_iter()
            .filter_map(|(name, values)| {
                let (left, right) = segments(&values)
                    .bridges
                    .into_iter()
                    .find(|(left, right)| *left < index && index < *right)?;
                let from = crate::usage_table::timestamp(self.points[left + 1].start_ms, self.zh);
                let to = crate::usage_table::timestamp(self.points[right - 1].end_ms, self.zh);
                Some(if self.zh {
                    (
                        format!("{name}缺失区间"),
                        format!("{from} → {to}；虚线仅作连接，缺失值未知"),
                    )
                } else {
                    (
                        format!("{name} missing interval"),
                        format!("{from} → {to}; guide only, values unknown"),
                    )
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::segments;

    #[test]
    fn interior_gaps_are_guides_and_unknown_edges_are_never_extrapolated() {
        let values = [None, Some(0), Some(10), None, None, Some(30), None];
        let before = values;
        let plan = segments(&values);
        assert_eq!(plan.runs, vec![vec![1, 2], vec![5]]);
        assert_eq!(plan.bridges, vec![(2, 5)]);
        assert_eq!(values, before, "rendering must not fill unknown buckets");
        assert!(segments::<i64>(&[]).runs.is_empty());
        assert!(segments::<i64>(&[None, None]).bridges.is_empty());
        assert_eq!(segments(&[None, Some(0), None]).runs, vec![vec![1]]);
        assert!(segments(&[Some(0), Some(0), Some(10)]).bridges.is_empty());
    }

    #[test]
    fn separate_gaps_keep_separate_known_endpoints() {
        let plan = segments(&[Some(10), None, Some(20), None, Some(30)]);
        assert_eq!(plan.bridges, vec![(0, 2), (2, 4)]);
        assert_eq!(plan.runs, vec![vec![0], vec![2], vec![4]]);
    }
}
