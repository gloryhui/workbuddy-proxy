//! Fixed page controls and independently scrolling report content.
use super::preferences::View;
use super::*;

pub(super) fn kind_choices(zh: bool) -> Vec<Choice> {
    [
        ("all", "全部类型", "All record types"),
        ("model", "模型用量", "Model usage"),
        ("tool", "工具收费", "Tool charges"),
        ("other_charge", "其他收费", "Other charges"),
    ]
    .into_iter()
    .map(|(code, cn, en)| choice(code, if zh { cn } else { en }))
    .collect()
}

impl Statistics {
    fn select_view(&mut self, view: View, cx: &mut Context<Self>) {
        if self.view == view {
            return;
        }
        self.view = view;
        self.tab = if view == View::Summary {
            self.summary_tab
        } else {
            Tab::Requests
        };
        self.rows_gate.invalidate();
        self.groups_gate.invalidate();
        self.pending_rows = false;
        self.loading_rows = false;
        self.persist_preferences();
        if view != View::Settings {
            self.refresh(cx);
        }
        cx.notify();
    }

    pub(super) fn view_tabs(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .flex()
            .items_center()
            .justify_between()
            .flex_wrap()
            .gap_2()
            .flex_shrink_0()
            .pb_2()
            .border_b_1()
            .border_color(rgb(BORDER))
            .child(
                div().flex().items_center().gap_2().children(
                    [
                        ("view-overview", View::Overview, self.t("概览", "Overview")),
                        (
                            "view-details",
                            View::Details,
                            self.t("用量明细", "Usage records"),
                        ),
                        (
                            "view-summary",
                            View::Summary,
                            self.t("汇总分析", "Analysis"),
                        ),
                    ]
                    .into_iter()
                    .map(|(id, view, label)| {
                        stats_button(id)
                            .when(self.view == view, |button| button.primary().outline())
                            .when(self.view != view, |button| button.ghost())
                            .label(label)
                            .on_click(cx.listener(move |this, _, _, cx| this.select_view(view, cx)))
                    }),
                ),
            )
            .child(
                stats_button("view-settings")
                    .when(self.view == View::Settings, |button| {
                        button.primary().outline()
                    })
                    .when(self.view != View::Settings, |button| button.ghost())
                    .label(self.t("统计设置", "Statistics settings"))
                    .on_click(cx.listener(|this, _, _, cx| this.select_view(View::Settings, cx))),
            )
            .into_any_element()
    }

    pub(super) fn compact_status(&self, cx: &mut Context<Self>) -> AnyElement {
        let (label, color) = match self.health.as_ref() {
            Some(health) if health.enabled && health.state == HealthState::Ready => {
                (self.t("采集正常", "Collecting"), OK)
            }
            Some(health) if !health.enabled => (self.t("采集已暂停", "Collection paused"), WARN),
            Some(_) => (self.t("采集需关注", "Collection needs attention"), WARN),
            None => (self.t("读取采集状态…", "Reading collection status…"), MUTED),
        };
        let warning_count = self
            .dashboard
            .as_ref()
            .map_or(0, |dashboard| dashboard.warnings.len());
        div()
            .flex()
            .items_center()
            .flex_wrap()
            .gap_2()
            .flex_shrink_0()
            .child(
                Label::new(format!("● {label}"))
                    .text_size(px(crate::theme::font::LABEL))
                    .text_color(rgb(color)),
            )
            .child(
                Label::new(self.import_health_text())
                    .text_size(px(crate::theme::font::LABEL))
                    .text_color(rgb(MUTED)),
            )
            .when(
                warning_count > 0 || self.import.counters().needs_attention(),
                |row| {
                    row.child(
                        stats_button("view-notices")
                            .ghost()
                            .label(self.t("查看数据提示", "View data notices"))
                            .on_click(
                                cx.listener(|this, _, _, cx| this.select_view(View::Settings, cx)),
                            ),
                    )
                },
            )
            .when(self.preferences_error, |row| {
                row.child(
                    Label::new(self.t(
                        "筛选条件保存失败，当前窗口仍可使用",
                        "Could not save filters; this window remains usable",
                    ))
                    .text_size(px(crate::theme::font::LABEL))
                    .text_color(rgb(WARN)),
                )
            })
            .into_any_element()
    }

    pub(super) fn report_totals(&self) -> AnyElement {
        let text = if let Some(dashboard) = self.dashboard.as_ref() {
            let summary = &dashboard.summary;
            let credit = summary
                .credit_nanos
                .as_deref()
                .and_then(usage_table::format_credit_nanos)
                .unwrap_or_else(|| "—".into());
            format!(
                "{} {} {}  ·  Token {}  ·  {} {}",
                self.t("当前范围", "Current range"),
                summary.request_count,
                self.t("条记录", "records"),
                usage_table::compact(summary.total_tokens, self.zh),
                self.t("WorkBuddy 积分", "WorkBuddy credits"),
                credit
            )
        } else {
            self.t("正在读取当前范围…", "Reading current range…")
                .to_owned()
        };
        div()
            .flex()
            .flex_shrink_0()
            .flex_wrap()
            .gap_2()
            .px_3()
            .py_2()
            .rounded_md()
            .bg(rgb(crate::theme::palette().tint))
            .child(
                Label::new(text)
                    .text_size(px(12.))
                    .text_color(rgb(crate::theme::palette().dark)),
            )
            .into_any_element()
    }

    pub(super) fn reset_filters(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.source = crate::source_filter::SourceFilter::new();
        self.capture = crate::stats_controller::CaptureFilter::new();
        self.range = RangePreset::Week;
        self.usage.clear();
        self.record_kind = crate::stats_dto::RecordKind::All;
        self.usage_select.update(cx, |state, cx| {
            state.set_selected_value(&String::new(), window, cx)
        });
        self.kind_select.update(cx, |state, cx| {
            state.set_selected_value(&"all".to_owned(), window, cx)
        });
        self.reset_source_dependents(window, cx);
    }

    pub(super) fn report_body(&self, cx: &mut Context<Self>) -> AnyElement {
        match self.view {
            View::Details | View::Summary => div()
                .flex()
                .flex_col()
                .flex_1()
                .min_h_0()
                .gap_2()
                .overflow_hidden()
                .child(self.report_totals())
                .child(self.details(cx))
                .into_any_element(),
            View::Overview => div()
                .id("statistics-overview-scroll")
                .track_scroll(&self.page_scroll)
                .flex()
                .flex_col()
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .gap_3()
                .child(self.overview(cx))
                .child(self.trends(cx))
                .into_any_element(),
            View::Settings => div()
                .id("statistics-settings-scroll")
                .flex()
                .flex_col()
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .gap_3()
                .child(self.health_bar(cx))
                .children(self.warnings())
                .children(self.import_notices(cx))
                .child(self.pricing_section(cx))
                .child(self.storage_section(cx))
                .into_any_element(),
        }
    }
}
