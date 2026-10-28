//! Presentation of checkpoint snapshots; no source I/O or scan commands.
use super::*;
use gpui_kit::component::progress::Progress;

#[derive(Clone, serde::Deserialize)]
pub(super) struct Snapshot {
    files_total: i64,
    files_remaining: i64,
    bytes_total: i64,
    bytes_committed: i64,
    bytes_remaining: i64,
    total_known: bool,
}

impl Snapshot {
    pub(super) fn valid(&self) -> bool {
        [
            self.files_total,
            self.files_remaining,
            self.bytes_total,
            self.bytes_committed,
            self.bytes_remaining,
        ]
        .iter()
        .all(|v| *v >= 0)
            && self.files_remaining <= self.files_total
            && self.bytes_committed.checked_add(self.bytes_remaining) == Some(self.bytes_total)
    }

    pub(super) fn committed_text(&self) -> String {
        data_size(self.bytes_committed)
    }

    fn percent(&self) -> f32 {
        if self.bytes_total == 0 {
            return 0.;
        }
        let percent = self.bytes_committed as f64 / self.bytes_total as f64 * 100.;
        // Do not round an unfinished checkpoint up to 100%.
        let cap = if self.files_remaining > 0 { 99.9 } else { 100. };
        ((percent * 10.).floor() / 10.).min(cap) as f32
    }
}

fn data_size(bytes: i64) -> String {
    for (unit, size) in [
        ("GiB", 1024_i64.pow(3)),
        ("MiB", 1024_i64.pow(2)),
        ("KiB", 1024),
    ] {
        if bytes >= size {
            return format!("{:.1} {unit}", bytes as f64 / size as f64);
        }
    }
    format!("{bytes} B")
}

impl Statistics {
    pub(in crate::statistics::import_controls) fn import_progress(
        &self,
        compact: bool,
    ) -> AnyElement {
        let status = self.import_ui.status.as_ref();
        let snapshot = status.and_then(|status| status.progress.as_ref());
        let enabled = status.is_some_and(|status| status.effective_enabled);
        let error = self.import_ui.error.is_some();
        let full = status.is_some_and(|status| {
            status.scan_mode.as_deref() == Some("full")
                && matches!(status.scan_state.as_deref(), Some("queued" | "running"))
        });
        let known = snapshot.is_some_and(|snapshot| snapshot.total_known) && !full;
        let title = if error {
            self.t(
                "导入需重试 · 保留已提交进度",
                "Import needs retry · committed progress retained",
            )
            .to_owned()
        } else if !enabled {
            self.t(
                "导入已暂停 · 保留已提交进度",
                "Import paused · committed progress retained",
            )
            .to_owned()
        } else if full {
            self.t(
                "正在完整核对 · 统计展示已入库部分",
                "Full check in progress · statistics show committed rows",
            )
            .to_owned()
        } else if snapshot.is_none() {
            self.t(
                "进度暂不可用 · 剩余数量未知",
                "Progress unavailable · remaining amount unknown",
            )
            .to_owned()
        } else if !known {
            self.t(
                "正在发现文件 · 总量尚未确定",
                "Discovering files · total size not yet known",
            )
            .to_owned()
        } else if let Some(snapshot) = snapshot {
            if snapshot.files_remaining == 0 {
                self.t("已发现文件已读取追平", "Discovered files are caught up")
                    .to_owned()
            } else {
                format!(
                    "{} {:.1}%",
                    self.t("后台导入", "Background import"),
                    snapshot.percent()
                )
            }
        } else {
            self.t("正在读取进度", "Loading progress").to_owned()
        };
        let detail = snapshot
            .map(|snapshot| {
                format!(
                    "{} {} / {} · {} {} / {} · {} {}",
                    self.t("剩余文件", "Files remaining"),
                    snapshot.files_remaining,
                    snapshot.files_total,
                    self.t("剩余数据", "Data remaining"),
                    data_size(snapshot.bytes_remaining),
                    data_size(snapshot.bytes_total),
                    self.t("已入库", "Imported rows"),
                    status.map_or(0, |status| status.imported),
                )
            })
            .unwrap_or_else(|| {
                self.t("剩余数量尚未确定", "Remaining amount not yet known")
                    .to_owned()
            });
        let label = format!("{title}. {detail}");
        let bar = Progress::new("wb-import-progress")
            .small()
            .w_full()
            .value(if known {
                snapshot.map_or(0., Snapshot::percent)
            } else {
                0.
            })
            .loading(snapshot.is_some() && enabled && !error && !known)
            .when(compact, |bar| {
                bar.color(rgb(if enabled && !error {
                    crate::theme::color::CHART_CACHE_READ
                } else {
                    MUTED
                }))
            })
            .accessibility_label(label);
        if compact {
            return div().w_full().child(bar).into_any_element();
        }
        div().flex().flex_col().w_full().gap_1()
            .child(Label::new(title).text_size(px(crate::theme::font::LABEL)).text_color(rgb(MUTED)))
            .child(bar)
            .child(div().text_size(px(crate::theme::font::LABEL)).text_color(rgb(MUTED)).child(detail))
            .child(div().text_size(px(crate::theme::font::LABEL)).text_color(rgb(MUTED)).child(self.t(
                "按最近发现的文件快照计算；已去重副本与异常记录不计入未读积压。",
                "Based on the last discovered file snapshot; deduplicated copies and anomalies are separate from unread backlog.")))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::{Snapshot, data_size};

    fn snapshot() -> Snapshot {
        Snapshot {
            files_total: 2,
            files_remaining: 1,
            bytes_total: 1000,
            bytes_committed: 999,
            bytes_remaining: 1,
            total_known: true,
        }
    }

    #[test]
    fn validates_snapshot_and_never_rounds_unfinished_work_to_complete() {
        let mut value = snapshot();
        assert!(value.valid());
        assert_eq!(value.percent(), 99.9);
        value.bytes_remaining = 0;
        assert!(!value.valid());
        value.bytes_committed = 1000;
        assert!(value.valid());
        assert_eq!(value.percent(), 99.9);
        value.files_remaining = 0;
        assert_eq!(value.percent(), 100.);
        value.files_remaining = -1;
        assert!(!value.valid());
        value.files_remaining = 3;
        assert!(!value.valid());
    }

    #[test]
    fn empty_and_overflow_snapshots_are_bounded() {
        let mut value = snapshot();
        value.bytes_total = 0;
        value.bytes_committed = 0;
        value.bytes_remaining = 0;
        assert!(value.valid());
        assert_eq!(value.percent(), 0.);
        value.bytes_total = i64::MAX;
        value.bytes_committed = i64::MAX;
        value.bytes_remaining = 1;
        assert!(!value.valid());
        assert_eq!(data_size(1), "1 B");
        assert_eq!(data_size(1024), "1.0 KiB");
    }
}
