//! Latest request has its own read identity; it never reuses a table page.
use super::*;

pub(super) struct RecentRead<T> {
    seq: u64,
    generation: u64,
    pub value: Option<T>,
    pub error: Option<String>,
    pub loading: bool,
    pub ready: bool,
}
impl<T> Default for RecentRead<T> {
    fn default() -> Self {
        Self {
            seq: 0,
            generation: 0,
            value: None,
            error: None,
            loading: false,
            ready: false,
        }
    }
}
impl<T> RecentRead<T> {
    pub fn invalidate(&mut self) {
        self.seq = 0;
        self.value = None;
        self.error = None;
        self.loading = false;
        self.ready = false;
    }
    pub fn issue(&mut self, seq: u64, generation: u64) {
        if self.generation != generation {
            self.value = None;
            self.ready = false;
        }
        self.seq = seq;
        self.generation = generation;
        self.error = None;
        self.loading = true;
    }
    pub fn apply(&mut self, seq: u64, generation: u64, result: Result<Option<T>, String>) -> bool {
        if seq == 0 || seq != self.seq || generation != self.generation {
            return false;
        }
        self.seq = 0;
        self.loading = false;
        self.ready = true;
        match result {
            Ok(value) => {
                self.value = value;
                self.error = None;
            }
            Err(error) => {
                self.value = None;
                self.error = Some(error);
            }
        }
        true
    }
}

fn latest_request(filter: Value, tag: &str) -> Value {
    crate::stats_backend::command(
        "stats_requests",
        tag,
        json!({"filter": filter, "page_size": super::pagination::PAGE_SIZES[0]}),
    )
}

impl Statistics {
    pub(super) fn request_recent(&mut self, cx: &mut Context<Self>) {
        if !self.visible || self.view != preferences::View::Overview {
            return;
        }
        let tag = format!("recent-{}", self.generation);
        // Intentionally no cursor, page or as_of_id from the paginated table.
        let request = latest_request(self.filter(), &tag);
        let seq = self.backend.submit_read(JobKey::Recent, tag, request);
        self.recent.issue(seq, self.generation);
        cx.notify();
    }

    pub(super) fn recent_card(&self, cx: &mut Context<Self>) -> AnyElement {
        let row = self.recent.value.as_ref();
        let wide = self.filters_width >= CHART_SIDE_BY_SIDE_WIDTH;
        stats_card("stats-recent-request")
            .flex_shrink_0()
            .min_h(px(91.))
            .mt(px(1.))
            .rounded(px(14.))
            .px(px(20.))
            .py(px(12.))
            .gap(px(11.))
            .child(
                div()
                    .h(px(21.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(12.))
                    .child(
                        div()
                            .ml(px(-4.))
                            .flex()
                            .items_center()
                            .gap(px(9.))
                            .child(
                                Icon::default()
                                    .path("studio/recent-clock.svg")
                                    .with_size(px(18.))
                                    .mt(px(5.))
                                    .text_color(rgb(crate::theme::palette().primary)),
                            )
                            .child(
                                Label::new(self.t("最近请求", "Latest request"))
                                    .text_size(px(15.))
                                    .line_height(px(21.))
                                    .font_weight(FontWeight::BOLD),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .when(self.recent.loading, |head| {
                                head.child(
                                    Label::new(self.t("读取中…", "Loading…"))
                                        .text_size(px(11.))
                                        .text_ellipsis()
                                        .text_color(rgb(MUTED)),
                                )
                            })
                            .child(
                                stats_button("stats-recent-detail")
                                    .ghost()
                                    .with_size(px(28.))
                                    .h(px(28.))
                                    .label(self.t("查看详情", "View details"))
                                    .disabled(row.is_none())
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        if let Some(row) = this.recent.value.as_ref() {
                                            this.open_detail_id(row.request_id.clone(), window, cx);
                                        }
                                    })),
                            ),
                    ),
            )
            .when_some(row, |card, row| {
                let status = usage_table::status_text(row.recorded, row.outcome, self.zh);
                let status = row
                    .http_status
                    .map(|http| format!("{http} · {status}"))
                    .unwrap_or_else(|| status.to_owned());
                let ink = if !row.recorded && row.outcome == crate::stats_dto::Outcome::Completed {
                    OK
                } else {
                    crate::theme::color::TEXT_2
                };
                card.child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .flex_wrap()
                        .gap_x(px(if wide { 0. } else { 14. }))
                        .gap_y(px(10.))
                        .child(
                            crate::studio_fonts::latin(
                                Label::new(usage_table::timestamp(row.started_at_ms, self.zh)),
                                cx,
                            )
                            // Figma row labels start 8px below the 28px tag.
                            // A centered line receives half its top margin.
                            .mt(px(5.))
                            .when(wide, |label| label.w(px(150.)))
                            .text_size(px(12.))
                            .line_height(px(17.))
                            .font_weight(FontWeight::NORMAL)
                            .text_color(rgb(MUTED)),
                        )
                        .child(
                            Label::new(if row.provider_name.is_empty() {
                                row.provider_id.clone()
                            } else {
                                row.provider_name.clone()
                            })
                            .mt(px(7.))
                            .text_size(px(13.))
                            .line_height(px(19.))
                            .font_weight(FontWeight::MEDIUM)
                            .w(px(150.))
                            .text_ellipsis(),
                        )
                        .child(
                            div()
                                .w(px(196.))
                                .h(px(28.))
                                .when(wide, |tag| tag.mr(px(64.)))
                                .px(px(12.))
                                .py(px(5.))
                                .rounded(px(14.))
                                .bg(rgb(crate::theme::color::BG_TABLE_HEAD))
                                .child(
                                    Label::new(usage_table::model_text(row, self.zh))
                                        .w_full()
                                        .text_ellipsis()
                                        .text_size(px(11.))
                                        .line_height(px(16.))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(rgb(crate::theme::color::TEXT_2)),
                                ),
                        )
                        .child(
                            Label::new(format!(
                                "{} Token",
                                usage_table::compact(row.total_tokens, self.zh)
                            ))
                            .mt(px(5.))
                            .when(wide, |label| label.flex_1())
                            .text_size(px(12.))
                            .line_height(px(17.))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(rgb(crate::theme::color::TEXT_2)),
                        )
                        .child(
                            Label::new(status)
                                .mt(px(3.))
                                .when(wide, |label| label.w(px(171.)))
                                .text_size(px(12.))
                                .line_height(px(17.))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(rgb(ink)),
                        ),
                )
            })
            .when(row.is_none(), |card| {
                card.child(
                    Label::new(if self.recent.error.is_some() {
                        self.t("最近请求读取失败", "Could not load the latest request")
                    } else if self.recent.loading || !self.recent.ready {
                        self.t("正在读取最近请求…", "Reading the latest request…")
                    } else {
                        self.t(
                            "当前筛选范围暂无请求",
                            "No requests in the current filter range",
                        )
                    })
                    .text_size(px(12.))
                    .text_color(rgb(if self.recent.error.is_some() {
                        crate::theme::color::DANGER
                    } else {
                        MUTED
                    })),
                )
            })
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::{RecentRead, latest_request};
    #[test]
    fn latest_read_rejects_old_sequences_generations_and_hidden_views() {
        let mut read = RecentRead::<u32>::default();
        assert!(!read.ready);
        read.issue(1, 10);
        read.issue(2, 11);
        assert!(!read.apply(1, 10, Ok(Some(99))));
        assert!(!read.apply(2, 10, Ok(Some(99))));
        assert!(read.loading);
        assert!(read.value.is_none());
        assert!(!read.ready);
        assert!(read.apply(2, 11, Ok(Some(42))));
        assert_eq!(read.value, Some(42));
        assert!(read.ready);
        read.issue(3, 11);
        read.invalidate();
        assert!(!read.ready);
        assert!(!read.apply(3, 11, Ok(Some(99))));
        assert!(read.value.is_none());
    }
    #[test]
    fn empty_and_failed_latest_reads_do_not_relabel_an_old_row_as_latest() {
        let mut read = RecentRead::<u32>::default();
        read.issue(1, 1);
        assert!(read.apply(1, 1, Ok(Some(42))));
        read.issue(2, 1);
        assert!(read.apply(2, 1, Ok(None)));
        assert!(read.value.is_none());
        read.issue(3, 1);
        assert!(read.apply(3, 1, Ok(Some(99))));
        read.issue(4, 1);
        assert!(read.apply(4, 1, Err("offline".into())));
        assert!(read.value.is_none());
        assert_eq!(read.error.as_deref(), Some("offline"));
        assert!(!read.loading);
    }
    #[test]
    fn latest_request_keeps_the_filter_but_never_carries_a_listing_cursor() {
        let filter = serde_json::json!({"source_scope":"builtin", "from_ms":10, "to_ms":20});
        let request = latest_request(filter.clone(), "recent-7");
        assert_eq!(request["command"], "stats_requests");
        assert_eq!(request["filter"], filter);
        assert_eq!(request["page_size"], 10);
        assert!(
            super::super::pagination::PAGE_SIZES.contains(&request["page_size"].as_i64().unwrap())
        );
        for field in ["cursor", "page", "as_of_id"] {
            assert!(request.get(field).is_none());
        }
    }
}
