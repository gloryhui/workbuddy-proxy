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
    pub(super) fn select_view(&mut self, view: View, cx: &mut Context<Self>) {
        if self.view == view {
            return;
        }
        if self.view != View::Settings {
            self.last_report_view = self.view;
        }
        self.recent.invalidate();
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

    pub fn is_settings_view(&self) -> bool {
        self.view == View::Settings
    }

    pub fn open_workspace(&mut self, settings: bool, cx: &mut Context<Self>) {
        if settings {
            self.select_view(View::Settings, cx);
        } else if self.view == View::Settings {
            self.select_view(self.last_report_view, cx);
        }
    }

    pub(super) fn view_tabs(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .flex()
            .items_center()
            .justify_between()
            .flex_wrap()
            .gap(px(12.))
            .flex_shrink_0()
            .child(
                div().flex().items_center().gap(px(20.)).children(
                    [
                        (
                            "view-overview",
                            View::Overview,
                            self.t("概览", "Overview"),
                            87.,
                        ),
                        (
                            "view-details",
                            View::Details,
                            self.t("用量明细", "Usage records"),
                            if self.zh { 113. } else { 130. },
                        ),
                        (
                            "view-summary",
                            View::Summary,
                            self.t("汇总分析", "Analysis"),
                            113.,
                        ),
                    ]
                    .into_iter()
                    .map(|(id, view, label, width)| {
                        let active = self.view == view;
                        let ink = if active { INK } else { MUTED };
                        Button::new(id)
                            .custom(
                                gpui_kit::component::button::ButtonCustomVariant::new(cx)
                                    .color(
                                        rgb(if active {
                                            crate::theme::color::WHITE
                                        } else {
                                            BG
                                        })
                                        .into(),
                                    )
                                    .foreground(rgb(ink).into())
                                    .hover(rgb(crate::theme::color::BG_TABLE_HEAD).into())
                                    .active(rgb(crate::theme::palette().tint).into()),
                            )
                            .with_size(px(42.))
                            .w(px(width))
                            .h(px(42.))
                            .p_0()
                            .rounded(px(10.))
                            .toggled(active)
                            .accessibility_label(label)
                            .child(
                                div()
                                    .relative()
                                    .size_full()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(
                                        Label::new(label)
                                            .text_size(px(14.))
                                            .text_color(rgb(ink))
                                            .font_weight(if active {
                                                FontWeight::BOLD
                                            } else {
                                                FontWeight::MEDIUM
                                            }),
                                    )
                                    .when(active, |tab| {
                                        tab.child(
                                            div()
                                                .absolute()
                                                .bottom_0()
                                                .left(px(12.))
                                                .right(px(12.))
                                                .h(px(3.))
                                                .rounded(px(2.))
                                                .bg(rgb(crate::theme::palette().primary)),
                                        )
                                    }),
                            )
                            .on_click(cx.listener(move |this, _, _, cx| this.select_view(view, cx)))
                    }),
                ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .flex_wrap()
                    .gap(px(8.))
                    .child(self.compact_status(cx))
                    .child(self.filter_more(cx))
                    .child(
                        stats_button("reset-stats-filters")
                            .ghost()
                            .label(self.t("清除筛选", "Clear filters"))
                            .on_click(
                                cx.listener(|this, _, window, cx| this.reset_filters(window, cx)),
                            ),
                    ),
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

    /// Keep the real range totals available without inserting a second bar
    /// between the design's filters and request panel. The native popover
    /// reads the same live dashboard as the analysis view's existing summary.
    pub(super) fn request_count_summary(&self, cx: &mut Context<Self>) -> AnyElement {
        self.range_count_summary(
            "request-range-summary",
            "request-range-summary-trigger",
            format!("{} {}", self.total_rows.max(0), self.t("条", "rows")),
            cx,
        )
    }

    pub(super) fn range_count_summary(
        &self,
        popover_id: &'static str,
        trigger_id: &'static str,
        label: String,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let owner = cx.entity().downgrade();
        Popover::new(popover_id)
            .trigger(
                stats_button(trigger_id)
                    .custom(
                        gpui_kit::component::button::ButtonCustomVariant::new(cx)
                            .color(rgb(crate::theme::toolbar_selection()).into())
                            .foreground(rgb(crate::theme::palette().dark).into())
                            .hover(rgb(crate::theme::palette().tint).into())
                            .active(rgb(crate::theme::palette().tint).into()),
                    )
                    .bg(rgb(crate::theme::toolbar_selection()))
                    .with_size(px(25.))
                    .h(px(25.))
                    .min_w(px(64.))
                    .rounded(px(8.))
                    .text_size(px(11.))
                    .child(
                        Label::new(label)
                            .text_size(px(11.))
                            .line_height(px(16.))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(rgb(crate::theme::palette().dark)),
                    )
                    .tooltip(self.t("查看当前范围汇总", "View current range totals"))
                    .accessibility_label(self.t("查看当前范围汇总", "View current range totals")),
            )
            .content(move |_, _, cx| {
                owner
                    .update(cx, |this, _| this.report_totals())
                    .unwrap_or_else(|_| div().into_any_element())
            })
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
            View::Details => div()
                .flex()
                .flex_col()
                .flex_1()
                .min_h_0()
                .gap_2()
                .overflow_hidden()
                .child(self.details(cx))
                .into_any_element(),
            View::Summary => div()
                .flex()
                .flex_col()
                .flex_1()
                .min_h_0()
                .gap_2()
                .overflow_hidden()
                .child(
                    div()
                        .id("statistics-analysis-scroll")
                        .track_scroll(&self.analysis_scroll)
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .gap(px(18.))
                        .child(
                            div()
                                .w_full()
                                .h(px(321.))
                                .flex_none()
                                .flex()
                                .flex_col()
                                .min_h_0()
                                .child(self.details(cx)),
                        )
                        .child(self.analysis_cards(cx)),
                )
                .into_any_element(),
            View::Overview => div()
                .id("statistics-overview-scroll")
                .track_scroll(&self.page_scroll)
                .flex()
                .flex_col()
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .gap(px(17.))
                .child(self.overview(cx))
                .child(self.trends(cx))
                .child(self.recent_card(cx))
                .into_any_element(),
            View::Settings => div()
                .flex()
                .flex_col()
                .flex_1()
                .min_h_0()
                .gap(px(12.))
                .child(self.settings_navigation(cx))
                .child(
                    div()
                        .id("statistics-settings-scroll")
                        .track_scroll(&self.settings_scroll)
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .gap(px(18.))
                        .on_prepaint({
                            let owner = cx.entity().downgrade();
                            move |bounds, _, cx| {
                                let available = f32::from(bounds.size.width) - CARD_PAD * 2. - 2.;
                                let _ = owner.update(cx, |this, cx| {
                                    if (this.filters_width - available).abs() > 0.5 {
                                        this.filters_width = available;
                                        cx.notify();
                                    }
                                });
                            }
                        })
                        .children(self.warnings())
                        .child(self.pricing_section(cx))
                        .child(self.settings_capture_import(cx))
                        // Keep diagnostics and their original actions in the
                        // scrollable settings page without displacing its
                        // primary pricing and collection panels.
                        .child(self.health_bar(cx))
                        .children(self.import_notices(cx))
                        .children(self.maintenance_panel(cx)),
                )
                .into_any_element(),
        }
    }
}
