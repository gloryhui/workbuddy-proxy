//! Studio picker: the original draft selection, presented by Kit's virtual list.
use super::*;
use gpui_kit::component::{
    Disableable, IndexPath,
    checkbox::Checkbox,
    list::{List, ListDelegate, ListItem, ListState},
    tag::Tag,
};

gpui_kit::actions!(model_picker, [Toggle, First, Last, PageUp, PageDown]);

pub(super) fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("space", Toggle, Some("ModelPicker")),
        KeyBinding::new("home", First, Some("ModelPicker")),
        KeyBinding::new("end", Last, Some("ModelPicker")),
        KeyBinding::new("pageup", PageUp, Some("ModelPicker")),
        KeyBinding::new("pagedown", PageDown, Some("ModelPicker")),
    ]);
}

pub(super) fn surface_height(window: &Window) -> Pixels {
    (window.viewport_size().height - px(64.)).min(px(672.))
}

pub(super) struct Candidates {
    owner: WeakEntity<Proxy>,
    indices: Vec<usize>,
    cursor: Option<IndexPath>,
    zh: bool,
}

impl Candidates {
    pub(super) fn new(owner: WeakEntity<Proxy>, zh: bool) -> Self {
        Self {
            owner,
            indices: Vec::new(),
            cursor: None,
            zh,
        }
    }
}

impl ListDelegate for Candidates {
    type Item = ListItem;

    fn items_count(&self, _: usize, _: &App) -> usize {
        self.indices.len()
    }

    fn render_item(
        &mut self,
        ix: IndexPath,
        _: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Option<Self::Item> {
        let candidate = *self.indices.get(ix.row)?;
        let owner = self.owner.upgrade()?;
        let state = owner.read(cx);
        let id = state.picker.as_ref()?.get(candidate)?.clone();
        let existing = state
            .models
            .iter()
            .any(|model| model["model"].as_str() == Some(&id));
        let checked = existing || state.picked.contains(&candidate);
        let zh = state.zh;
        let target = self.owner.clone();
        let studio = crate::theme::current_skin() == crate::theme::Skin::StudioIndigo;
        let label = Label::new(id.clone())
            .text_size(px(13.))
            .text_ellipsis()
            .when(studio, |label| label.flex_1().min_w_0())
            .text_color(rgb(if existing { MUTED } else { INK }));
        let on_change = move |checked: bool, window: &mut Window, cx: &mut App| {
            cx.stop_propagation();
            let _ = target.update(cx, |this, cx| {
                this.set_picker_checked(candidate, checked);
                cx.notify();
                this.picker_list.update(cx, |list, cx| {
                    list.set_selected_index(Some(ix), window, cx);
                    list.scroll_to_selected_item(window, cx);
                });
                this.picker_focus.focus(window, cx);
            });
        };
        let checkbox = if studio {
            // Compose the same native engine as model_editor with the exact
            // provided 15px indicator and unmodified 12px design check asset.
            let fill = rgb(if existing {
                MUTED
            } else {
                crate::theme::palette().primary
            });
            let tooltip = id.clone();
            gpui_kit::base::Checkbox::new(("candidate-check", candidate))
                .checked(checked)
                .disabled(existing)
                .tab_stop(false)
                .accessibility_id(format!("candidate-{candidate}"))
                .accessibility_label(id.clone())
                .flex()
                .items_center()
                .gap(px(15.))
                .flex_1()
                .min_w_0()
                .child(
                    gpui_kit::base::CheckboxIndicator::new()
                        .checked(checked)
                        .disabled(existing)
                        .flex()
                        .items_center()
                        .justify_center()
                        .flex_none()
                        .size(px(15.))
                        .rounded(px(4.))
                        .border_1()
                        .border_color(if checked {
                            fill
                        } else {
                            rgb(crate::theme::color::BORDER_INPUT)
                        })
                        .bg(if checked { fill } else { rgb(0xffffff) })
                        .when(checked, |part| {
                            part.child(
                                svg()
                                    .path("studio/model-check.svg")
                                    .size(px(12.))
                                    .text_color(rgb(0xffffff)),
                            )
                        }),
                )
                .child(label.line_height(px(19.)).font_weight(if existing {
                    FontWeight::MEDIUM
                } else {
                    FontWeight::NORMAL
                }))
                .on_change(move |state, _, window, cx| {
                    window.prevent_default();
                    on_change(state == gpui_kit::base::CheckboxState::Checked, window, cx);
                })
                .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
                .tooltip(move |window, cx| {
                    crate::ui_components::full_value_tooltip(tooltip.clone(), window, cx)
                })
                .into_any_element()
        } else {
            Checkbox::new(("candidate-check", candidate))
                .with_size(px(15.))
                .checked(checked)
                .disabled(existing)
                .tab_stop(false)
                .accessibility_id(format!("candidate-{candidate}"))
                .accessibility_label(id.clone())
                .tooltip(id.clone())
                .flex_1()
                .min_w_0()
                .child(label)
                .on_change(move |checked, window, cx| on_change(*checked, window, cx))
                .into_any_element()
        };
        Some(
            ListItem::new(("candidate", candidate))
                .h(px(39.))
                .w_full()
                .px(px(14.))
                .border_b_1()
                .border_color(rgb(crate::theme::color::BORDER))
                .when(ix.row % 2 == 1 || existing, |row| {
                    row.bg(rgb(crate::theme::color::BG_TABLE_HEAD))
                })
                .accessibility_label(id.clone())
                .child(checkbox)
                .when(existing, |row| {
                    row.suffix(move |_, _| {
                        Tag::custom(
                            rgb(crate::theme::palette().tint).into(),
                            rgb(crate::theme::palette().dark).into(),
                            rgb(crate::theme::palette().tint).into(),
                        )
                        .small()
                        .h(px(27.))
                        .min_w(px(87.))
                        .rounded(px(9.))
                        .text_size(px(11.))
                        .when(studio, |tag| {
                            tag.bg(rgb(0xeeedff))
                                .border_color(rgb(0xeeedff))
                                .justify_center()
                                .font_weight(FontWeight::MEDIUM)
                                .line_height(px(16.))
                        })
                        .child(if zh { "已添加" } else { "Added" })
                    })
                }),
        )
    }

    fn set_selected_index(
        &mut self,
        index: Option<IndexPath>,
        _: &mut Window,
        _: &mut Context<ListState<Self>>,
    ) {
        self.cursor = index;
    }

    fn confirm(&mut self, _: bool, window: &mut Window, cx: &mut Context<ListState<Self>>) {
        let Some(candidate) = self.cursor.and_then(|ix| self.indices.get(ix.row)).copied() else {
            return;
        };
        let _ = self.owner.update(cx, |this, cx| {
            this.set_picker_checked(candidate, !this.picked.contains(&candidate));
            cx.notify();
            this.picker_focus.focus(window, cx);
        });
    }

    fn render_empty(
        &mut self,
        _: &mut Window,
        _: &mut Context<ListState<Self>>,
    ) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .justify_center()
            .size_full()
            .text_color(rgb(MUTED))
            .child(if self.zh {
                "没有匹配的模型"
            } else {
                "No matching models"
            })
    }
}

impl Proxy {
    fn filtered(&self, cx: &App) -> Vec<usize> {
        let query = self.search.read(cx).value().to_lowercase();
        let existing = self
            .models
            .iter()
            .filter_map(|model| model["model"].as_str())
            .collect::<BTreeSet<_>>();
        self.picker
            .as_ref()
            .map(|ids| {
                ids.iter()
                    .enumerate()
                    .filter(|(index, id)| {
                        id.to_lowercase().contains(&query)
                            && (!self.only_picked
                                || self.picked.contains(index)
                                || existing.contains(id.as_str()))
                    })
                    .map(|(index, _)| index)
                    .collect()
            })
            .unwrap_or_default()
    }

    fn set_picker_checked(&mut self, index: usize, checked: bool) {
        let Some(id) = self.picker.as_ref().and_then(|ids| ids.get(index)) else {
            return;
        };
        if self
            .models
            .iter()
            .any(|model| model["model"].as_str() == Some(id))
        {
            return;
        }
        if checked {
            self.picked.insert(index);
        } else {
            self.picked.remove(&index);
        }
    }

    fn picker_room(&self) -> usize {
        let other = self
            .cfg
            .providers
            .iter()
            .filter(|provider| provider.id != self.selected())
            .map(|provider| provider.models.len())
            .sum::<usize>();
        config::MAX_MODELS.saturating_sub(other + self.models.len())
    }

    pub(super) fn apply_picker(&mut self, cx: &mut Context<Self>) {
        let Some(ids) = &self.picker else {
            return;
        };
        let missing = self
            .picked
            .iter()
            .filter_map(|index| ids.get(*index))
            .filter(|id| {
                !self
                    .models
                    .iter()
                    .any(|model| model["model"].as_str() == Some(id))
            })
            .collect::<Vec<_>>();
        // Match the disabled confirmation button, including keyboard Enter.
        if missing.is_empty() {
            return;
        }
        if missing.len() > self.picker_room() {
            self.error = true;
            self.status = self.t("最多支持 256 个模型", "At most 256 models").into();
            cx.notify();
            return;
        }
        for id in missing {
            self.models.push(json!({"model":id,"name":id}));
        }
        self.picker = None;
        self.capture(cx);
        self.status = self
            .t("模型已添加，保存后生效", "Models added; save to apply")
            .into();
        cx.notify();
    }

    fn move_picker(
        &mut self,
        edge: isize,
        page: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let count = self.picker_list.read(cx).delegate().indices.len();
        if count == 0 {
            return;
        }
        let current = self
            .picker_list
            .read(cx)
            .selected_index()
            .map_or(0, |ix| ix.row);
        let step = if page {
            ((f32::from(surface_height(window)) - 320.) / 39.)
                .floor()
                .max(1.) as usize
        } else {
            count
        };
        let target = if edge < 0 {
            current.saturating_sub(step)
        } else {
            current.saturating_add(step).min(count - 1)
        };
        self.picker_list.update(cx, |list, cx| {
            list.set_selected_index(Some(IndexPath::new(target)), window, cx);
            list.scroll_to_selected_item(window, cx);
        });
    }

    fn picker_toggle(&mut self, _: &Toggle, window: &mut Window, cx: &mut Context<Self>) {
        let candidate = {
            let list = self.picker_list.read(cx);
            list.selected_index()
                .and_then(|ix| list.delegate().indices.get(ix.row))
                .copied()
        };
        if let Some(index) = candidate {
            self.set_picker_checked(index, !self.picked.contains(&index));
            cx.notify();
            self.picker_list
                .update(cx, |list, cx| list.scroll_to_selected_item(window, cx));
        }
    }

    pub(super) fn picker_dialog(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let studio = crate::theme::current_skin() == crate::theme::Skin::StudioIndigo;
        let filtered = self.filtered(cx);
        let count = filtered.len();
        let total = self.picker.as_ref().map_or(0, Vec::len);
        let chosen = self.picked.len();
        let existing = self
            .models
            .iter()
            .filter_map(|model| model["model"].as_str())
            .collect::<BTreeSet<_>>();
        let checked = self.picker.as_ref().map_or(0, |ids| {
            ids.iter()
                .enumerate()
                .filter(|(index, id)| existing.contains(id.as_str()) || self.picked.contains(index))
                .count()
        });
        self.picker_list.update(cx, |list, cx| {
            if list.delegate().indices != filtered {
                let previous = list
                    .selected_index()
                    .and_then(|ix| list.delegate().indices.get(ix.row))
                    .copied();
                let selected = previous
                    .and_then(|index| filtered.iter().position(|candidate| *candidate == index))
                    .or(if filtered.is_empty() { None } else { Some(0) });
                list.delegate_mut().indices = filtered;
                list.set_selected_index(selected.map(IndexPath::new), window, cx);
                list.scroll_to_selected_item(window, cx);
            }
            list.delegate_mut().zh = self.zh;
            cx.notify();
        });
        let over_limit = chosen > self.picker_room();
        let footer_height = if over_limit { 105. } else { 81. };
        let height = surface_height(window) - px(37.);
        let list_height = (height - px(203. + footer_height)).max(px(100.));
        let action = |id, label, min_width, active: bool| {
            crate::ui_components::action_button(id)
                .h(px(35.))
                .min_w(px(min_width))
                .rounded(px(9.))
                .when(studio, |button| {
                    button.accessibility_label(label).child(
                        div()
                            .text_size(px(12.))
                            .line_height(px(17.))
                            .font_weight(FontWeight::MEDIUM)
                            .child(label),
                    )
                })
                .when(!studio, |button| button.label(label))
                .when(studio && !active, |button| {
                    button
                        .bg(rgb(crate::theme::color::BG_TABLE_HEAD))
                        .border_1()
                        .border_color(rgb(crate::theme::color::BORDER))
                })
                .when(active, |button| button.primary().outline())
                .when(!active, |button| {
                    button.custom(
                        gpui_kit::component::button::ButtonCustomVariant::new(cx)
                            .color(rgb(crate::theme::color::BG_TABLE_HEAD).into())
                            .foreground(rgb(crate::theme::color::TEXT_2).into())
                            .hover(rgb(crate::theme::palette().tint).into()),
                    )
                })
        };
        div()
            .flex()
            .flex_col()
            .w_full()
            .h(height)
            .gap_0()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .justify_between()
                    .h(px(123.))
                    .flex_none()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(12.))
                            .h(px(32.))
                            .when(studio, |header| header.items_start())
                            .pr(px(if studio { 14. } else { 16. }))
                            .child(
                                Label::new(self.t("选择模型", "Select models"))
                                    .text_size(px(21.))
                                    .when(studio, |title| {
                                        title.line_height(px(30.)).relative().top(px(-1.))
                                    })
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(rgb(INK)),
                            )
                            .child(
                                Tag::custom(
                                    rgb(crate::theme::palette().tint).into(),
                                    rgb(crate::theme::palette().dark).into(),
                                    rgb(crate::theme::palette().tint).into(),
                                )
                                .small()
                                .h(px(27.))
                                .min_w(px(120.))
                                .rounded(px(9.))
                                .text_size(px(11.))
                                .when(studio, |tag| {
                                    tag.bg(rgb(0xeeedff))
                                        .border_color(rgb(0xeeedff))
                                        .justify_center()
                                        .line_height(px(16.))
                                        .font_weight(FontWeight::MEDIUM)
                                        .mt(px(1.))
                                })
                                .child(format!(
                                    "{} {checked}/{total}",
                                    self.t("候选已选", "Checked")
                                )),
                            ),
                    )
                    .child(
                        Label::new(format!(
                            "{} · {} {} · {} {} · {} {count}",
                            self.t(
                                "从上游读取模型，勾选后加入当前供应商",
                                "Choose upstream models to add to this provider"
                            ),
                            self.t("已有", "Added"),
                            self.models.len(),
                            self.t("待添加", "To add"),
                            chosen,
                            self.t("匹配", "Matching")
                        ))
                        .text_size(px(12.))
                        .text_color(rgb(MUTED)),
                    )
                    .child(
                        Styled::h(
                            crate::ui_components::text_input(&self.search, false),
                            px(41.),
                        )
                        .bg(rgb(crate::theme::color::BG_TABLE_HEAD))
                        .border_color(rgb(crate::theme::color::BORDER))
                        .rounded(px(10.))
                        .when(studio, |input| {
                            input
                                .text_color(rgb(crate::theme::color::TEXT_2))
                                .line_height(px(19.))
                                .font_weight(FontWeight::NORMAL)
                        })
                        .aria_label(self.t("搜索模型 ID", "Search model IDs"))
                        .prefix(if studio {
                            Label::new("⌕")
                                .text_size(px(13.))
                                .line_height(px(19.))
                                .text_color(rgb(MUTED))
                                .into_any_element()
                        } else {
                            Icon::new(IconName::Search)
                                .with_size(px(14.))
                                .text_color(rgb(MUTED))
                                .into_any_element()
                        }),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .h(px(35.))
                    .mt(px(12.))
                    .flex_none()
                    .child(
                        action(
                            "picker-select-all",
                            self.t("全选当前筛选", "Select filtered"),
                            140.,
                            false,
                        )
                        .disabled(count == 0)
                        .on_click(cx.listener(|this, _, _, cx| {
                            for index in this.filtered(cx) {
                                this.set_picker_checked(index, true);
                            }
                            cx.notify();
                        })),
                    )
                    .child(
                        action(
                            "picker-invert",
                            self.t("反选当前筛选", "Invert filtered"),
                            140.,
                            false,
                        )
                        .disabled(count == 0)
                        .on_click(cx.listener(|this, _, _, cx| {
                            for index in this.filtered(cx) {
                                this.set_picker_checked(index, !this.picked.contains(&index));
                            }
                            cx.notify();
                        })),
                    )
                    .child(
                        action(
                            "picker-clear",
                            self.t("清空选择", "Clear selection"),
                            99.,
                            false,
                        )
                        .disabled(chosen == 0)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.picked.clear();
                            cx.notify();
                        })),
                    )
                    .child(
                        action(
                            "picker-only",
                            self.t("仅看已勾选", "Selected only"),
                            121.,
                            self.only_picked,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.only_picked = !this.only_picked;
                            cx.notify();
                        })),
                    ),
            )
            .child(
                div()
                    .id("model-picker-list")
                    .key_context("ModelPicker")
                    .h(list_height)
                    .w_full()
                    .mt(px(12.))
                    .flex_none()
                    .border_1()
                    .border_color(rgb(crate::theme::color::BORDER))
                    .rounded(px(10.))
                    .overflow_hidden()
                    .capture_action(cx.listener(
                        |this, _: &gpui_kit::component::dialog::Confirm, _, cx| {
                            this.apply_picker(cx)
                        },
                    ))
                    .on_action(cx.listener(Self::picker_toggle))
                    .on_action(cx.listener(|this, _: &First, window, cx| {
                        this.move_picker(-1, false, window, cx)
                    }))
                    .on_action(cx.listener(|this, _: &Last, window, cx| {
                        this.move_picker(1, false, window, cx)
                    }))
                    .on_action(cx.listener(|this, _: &PageUp, window, cx| {
                        this.move_picker(-1, true, window, cx)
                    }))
                    .on_action(cx.listener(|this, _: &PageDown, window, cx| {
                        this.move_picker(1, true, window, cx)
                    }))
                    .child(List::new(&self.picker_list).p_0().size_full()),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .h(px(footer_height))
                    .mt(px(21.))
                    .flex_none()
                    .gap(px(7.))
                    .border_t_1()
                    .border_color(rgb(crate::theme::color::BORDER))
                    .pt(px(20.))
                    .when(over_limit, |footer| {
                        footer.child(
                            Label::new(self.t(
                                "最多支持 256 个模型，请减少本次选择。",
                                "At most 256 models; reduce this selection.",
                            ))
                            .text_size(px(12.))
                            .line_height(px(17.))
                            .flex_none()
                            .text_color(rgb(0xc94f5e)),
                        )
                    })
                    .child(
                        Label::new(self.t(
                            "已添加的模型会保留，确认后仍需要保存当前供应商",
                            "Existing models stay; save this provider after confirming.",
                        ))
                        .text_size(px(12.))
                        .line_height(px(17.))
                        .flex_none()
                        .text_color(rgb(MUTED)),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_end()
                            .gap(px(8.))
                            .child(
                                crate::ui_components::action_button("picker-cancel")
                                    .outline()
                                    .h(px(36.))
                                    .min_w(px(85.))
                                    .rounded(px(9.))
                                    .label(self.t("取消", "Cancel"))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.dismiss(&Dismiss, window, cx)
                                    })),
                            )
                            .child(
                                crate::ui_components::action_button("picker-confirm")
                                    .primary()
                                    .h(px(36.))
                                    .min_w(px(101.))
                                    .rounded(px(9.))
                                    .label(format!(
                                        "{} ({chosen})",
                                        self.t("添加选中模型", "Add selected")
                                    ))
                                    .disabled(chosen == 0)
                                    .on_click(cx.listener(|this, _, _, cx| this.apply_picker(cx))),
                            ),
                    ),
            )
            .into_any_element()
    }
}
