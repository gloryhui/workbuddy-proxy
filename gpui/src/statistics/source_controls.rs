//! Actual source/capture controls; every transition resets the same page state
//! and sends the exact source contract to the existing query scheduler.
use super::*;
use crate::{
    source_filter::{OPTIONS, SelectionOutcome},
    stats_dto::SourceScope,
};

impl Statistics {
    fn select_source(&mut self, scope: SourceScope, window: &mut Window, cx: &mut Context<Self>) {
        let started = self.event_metrics.begin();
        if self.source.select(scope) == SelectionOutcome::Unchanged {
            self.event_metrics
                .record("source_repeat", started, Some(true));
            return;
        }
        self.capture.reconcile(scope);
        self.reset_source_dependents(window, cx);
        if started.is_some() {
            cx.on_next_frame(window, move |this, _, _| {
                this.event_metrics.record(
                    "source_next_frame",
                    started,
                    Some(this.source_scope() == scope),
                );
            });
        }
        self.event_metrics.record(
            "source_callback",
            started,
            Some(self.source_scope() == scope),
        );
    }

    pub(super) fn reset_source_dependents(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.mapping_ui.open {
            self.close_mapping_state();
            window.close_dialog(cx);
        }
        self.coordinator.change();
        self.provider_seq = 0;
        self.model_seq = 0;
        self.provider.clear();
        self.model.clear();
        self.outcome.clear();
        self.provider_query.clear();
        self.model_query.clear();
        self.provider_options.clear();
        self.model_options.clear();
        self.provider_after = None;
        self.provider_next = None;
        self.model_after = None;
        self.model_next = None;
        self.include_tests = self.capture.include_tests();
        for input in [&self.provider_search, &self.model_search] {
            input.update(cx, |state, cx| state.set_value("", window, cx));
        }
        for (select, label) in [
            (&self.provider_select, self.t("全部供应商", "All providers")),
            (&self.model_select, self.t("全部模型", "All models")),
        ] {
            select.update(cx, |state, cx| {
                state.set_items(vec![choice("", label)], window, cx);
                state.set_selected_value(&String::new(), window, cx);
            });
        }
        self.outcome_select.update(cx, |state, cx| {
            state.set_selected_value(&String::new(), window, cx)
        });
        self.capture_select.update(cx, |state, cx| {
            state.set_selected_value(
                &self
                    .capture
                    .displayed(self.source_scope())
                    .as_str()
                    .to_owned(),
                window,
                cx,
            )
        });
        self.mark_dirty(cx);
        self.request_filters(cx);
        self.request_model_filters(cx);
    }

    pub(super) fn capture_control(&self) -> AnyElement {
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(self.filter_label(self.t("采集方式", "Capture method")))
            .child(
                div()
                    .w(px(164.))
                    .h(px(crate::theme::size::SELECT_HEIGHT))
                    .child(
                        crate::ui_components::select(
                            &self.capture_select,
                            self.capture.is_locked(self.source_scope()),
                        )
                        .w_full()
                        .accessibility_label(self.t("采集方式", "Capture method")),
                    ),
            )
            .into_any_element()
    }

    fn source_label(&self, scope: SourceScope) -> &'static str {
        match scope {
            SourceScope::All => self.t("全部来源", "All sources"),
            SourceScope::Builtin => self.t("内置模型", "Built-in models"),
            SourceScope::Supplier => self.t("自定义模型", "Custom models"),
        }
    }

    fn source_key(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let current = OPTIONS
            .iter()
            .position(|option| option.scope == self.source_scope())
            .unwrap_or(0);
        let next = match key {
            "left" => (current + 2) % 3,
            "right" => (current + 1) % 3,
            "home" => 0,
            "end" => 2,
            "space" | " " | "enter" => current,
            _ => return false,
        };
        self.select_source(OPTIONS[next].scope, window, cx);
        true
    }

    fn source_button(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let option = OPTIONS[index];
        let selected = self.source_scope() == option.scope;
        let label = self.source_label(option.scope);
        div()
            .id(("stats-source", index))
            .flex()
            .items_center()
            .justify_center()
            .flex_shrink_0()
            .w(px(36.))
            .h(px(32.))
            .cursor_pointer()
            .bg(rgb(if selected { 0x356fc0 } else { 0xf7f8fa }))
            .text_color(rgb(if selected { 0xffffff } else { option.color }))
            .when(!selected, |button| {
                button.hover(|style| style.bg(rgb(0xedf2f8)))
            })
            .role(Role::RadioButton)
            .aria_label(label)
            .aria_toggled(if selected {
                Toggled::True
            } else {
                Toggled::False
            })
            .tooltip(move |window, cx| {
                gpui_kit::component::tooltip::Tooltip::new(label).build(window, cx)
            })
            .child(Icon::default().path(option.asset).size(px(16.)))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.source_focus.focus(window, cx);
                this.select_source(option.scope, window, cx);
            }))
            .into_any_element()
    }

    pub(super) fn source_group(&self, cx: &mut Context<Self>) -> AnyElement {
        let focused = self.source.is_focused();
        let label = self.source_label(self.source_scope());
        let group = div()
            .id("stats-source-group")
            .flex()
            .flex_shrink_0()
            .w(px(108.))
            .h(px(32.))
            .rounded(px(6.))
            .overflow_hidden()
            .role(Role::RadioGroup)
            .aria_label(self.t("来源", "Source"))
            .track_focus(&self.source_focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if this.source_key(&event.keystroke.key, window, cx) {
                    cx.stop_propagation();
                }
            }))
            .children((0..OPTIONS.len()).map(|index| self.source_button(index, cx)));
        div()
            .relative()
            .flex_shrink_0()
            .w(px(108.))
            .h(px(32.))
            .opacity(if self.source.is_disabled() { 0.5 } else { 1.0 })
            .child(group)
            .children([36., 72.].into_iter().map(|left| {
                div()
                    .absolute()
                    .left(px(left))
                    .top_0()
                    .h(px(32.))
                    .border_l_1()
                    .border_color(rgb(0xdfe3e8))
            }))
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .rounded(px(6.))
                    .border_1()
                    .border_color(rgb(0xdfe3e8))
                    .when(focused, |border| {
                        border.border_2().border_color(rgb(0x356fc0))
                    }),
            )
            .when(focused, |group| {
                group.child(
                    deferred(
                        div()
                            .absolute()
                            .top(px(36.))
                            .left_0()
                            .child(cx.new(|_| gpui_kit::component::tooltip::Tooltip::new(label))),
                    )
                    .with_priority(2),
                )
            })
            .into_any_element()
    }

    pub(super) fn filter_row(
        &self,
        index: usize,
        slots: &[crate::source_filter::FilterSlot],
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .id(("stats-filter-row", index))
            .flex()
            .items_center()
            .w_full()
            .gap(px(crate::source_filter::GAP as f32))
            .children(slots.iter().map(|slot| self.filter_control(*slot, cx)))
            .into_any_element()
    }

    fn filter_control(
        &self,
        slot: crate::source_filter::FilterSlot,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        use crate::source_filter::FilterSlot;
        let control = match slot {
            FilterSlot::Source => self.source_group(cx),
            FilterSlot::Provider => self.filter_select(
                &self.provider_select,
                self.t("供应商筛选", "Provider filter"),
            ),
            FilterSlot::Model => {
                self.filter_select(&self.model_select, self.t("模型筛选", "Model filter"))
            }
            FilterSlot::Status => {
                self.filter_select(&self.outcome_select, self.t("请求状态", "Request state"))
            }
            FilterSlot::Refresh => self.refresh_control(cx),
            FilterSlot::DateRange => self.date_control(cx),
            FilterSlot::RefreshButton => self.filter_refresh(cx),
            FilterSlot::More => self.filter_more(cx),
        };
        div()
            .w(px(slot.width() as f32))
            .h(px(crate::theme::size::SELECT_HEIGHT))
            .flex()
            .flex_col()
            .justify_center()
            .flex_shrink_0()
            .child(control)
            .into_any_element()
    }

    fn filter_select(&self, state: &Entity<ChoiceState>, label: &'static str) -> AnyElement {
        crate::ui_components::select(state, false)
            .w_full()
            .accessibility_label(label)
            .into_any_element()
    }

    fn filter_refresh(&self, cx: &mut Context<Self>) -> AnyElement {
        Button::new("stats-refresh")
            .small()
            .size(px(32.))
            .outline()
            .icon(IconName::RefreshCw)
            .tooltip(self.t("刷新统计", "Refresh statistics"))
            .accessibility_label(self.t("刷新统计", "Refresh statistics"))
            .on_click(cx.listener(|this, _, _, cx| {
                if this.loading_dashboard || this.loading_rows {
                    return;
                }
                this.generation += 1;
                this.debounce_until = None;
                this.applied = this.generation;
                this.refresh(cx);
            }))
            .into_any_element()
    }

    fn filter_more(&self, cx: &mut Context<Self>) -> AnyElement {
        let active = self.more_filters_open
            || !self.usage.is_empty()
            || self.capture.is_locked(self.source_scope())
            || self.capture.effective(self.source_scope())
                != crate::stats_controller::CaptureChoice::All
            || self.include_tests
            || !self.provider_query.is_empty()
            || !self.model_query.is_empty()
            || self.import.counters().needs_attention();
        Button::new("stats-more-filters")
            .small()
            .size(px(32.))
            .ghost()
            .icon(if self.more_filters_open {
                IconName::ChevronUp
            } else {
                IconName::ChevronDown
            })
            .when(active, |button| button.text_color(rgb(0x356fc0)))
            .tooltip(if self.more_filters_open {
                self.t("收起更多筛选", "Hide more filters")
            } else {
                self.t("展开更多筛选", "More filters")
            })
            .accessibility_label(self.t("更多筛选", "More filters"))
            .on_click(cx.listener(|this, _, _, cx| {
                this.more_filters_open = !this.more_filters_open;
                cx.notify();
            }))
            .into_any_element()
    }
}

pub(super) fn capture_choices(zh: bool) -> Vec<Choice> {
    vec![
        choice("all", if zh { "全部" } else { "All" }),
        choice("proxy", if zh { "代理采集" } else { "Proxy" }),
        choice(
            "workbuddy",
            if zh {
                "WorkBuddy 导入"
            } else {
                "WorkBuddy import"
            },
        ),
    ]
}
