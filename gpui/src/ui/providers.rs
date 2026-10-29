//! Native provider summary, connection form and virtualized model directory.
use super::*;
use crate::theme::{color, size};
use gpui_kit::component::Disableable;

fn section(number: &'static str, title: &'static str) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(12.))
        .child(
            Label::new(number)
                .text_size(px(12.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(crate::theme::palette().primary)),
        )
        .child(
            Label::new(title)
                .text_size(px(17.))
                .line_height(px(25.))
                .font_weight(FontWeight::SEMIBOLD),
        )
}

fn field(label: &'static str, control: impl IntoElement) -> Div {
    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_w_0()
        .gap(px(4.))
        .child(field_label(label))
        .child(control)
}

fn model_count(count: usize, zh: bool) -> String {
    format!(
        "{count} {}",
        if zh {
            "个模型"
        } else if count == 1 {
            "model"
        } else {
            "models"
        }
    )
}

impl Proxy {
    fn key_visibility_button(&self, cx: &mut Context<Self>) -> Button {
        let masked = self.key.read(cx).presentation().is_masked();
        let label = if masked {
            self.t("显示 API Key", "Show API key")
        } else {
            self.t("隐藏 API Key", "Hide API key")
        };
        Button::new("api-key-visibility")
            .ghost()
            .with_size(px(24.))
            .size(px(24.))
            .p_0()
            .disabled(self.busy)
            .tab_stop(true)
            .accessibility_label(label)
            .tooltip(label)
            .child(
                Icon::default()
                    .path("studio/provider-key-eye.svg")
                    .with_size(px(17.))
                    .text_color(rgb(color::TEXT_3)),
            )
            .on_click(cx.listener(|this, _, window, cx| {
                this.key
                    .update(cx, |state, cx| state.toggle_masked(window, cx));
                cx.notify();
            }))
    }

    pub(super) fn provider_summary(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let selected = self.selected();
        let pending = self.drafts.pending.as_ref().filter(|p| p.id == selected);
        let editing = self.provider_edits.contains(&selected) || pending.is_some();
        let cancel_label = if let Some(pending) = pending {
            if pending.source_id.is_some() {
                self.t("取消复制", "Cancel copy")
            } else {
                self.t("取消新增", "Cancel new")
            }
        } else {
            self.t("取消修改", "Cancel changes")
        };
        let compact = window.viewport_size().width < px(1300.);
        let name = self.name.read(cx).value();
        let display = if name.is_empty() && pending.is_some() {
            self.t("新供应商", "New provider").to_owned()
        } else if name.is_empty() {
            self.cfg
                .providers
                .iter()
                .find(|provider| provider.id == selected)
                .map(|provider| provider.name.clone())
                .unwrap_or_default()
        } else {
            name.to_string()
        };
        let dirty = pending.is_some()
            || self
                .cfg
                .providers
                .iter()
                .find(|provider| provider.id == selected)
                .is_some_and(|provider| {
                    self.drafts
                        .providers
                        .get(&selected)
                        .is_some_and(|draft| draft != &Draft::from_provider(provider))
                });
        let creation = self.saved_creations.get(&selected);
        let saved_copy = matches!(creation, Some(provider_new::CreationKind::Copy));
        let deleted = self.delete_feedback.is_for(&selected);
        let (status, ink, background) = if self.busy {
            (
                self.t("处理中", "Working"),
                color::TEXT_3,
                color::BG_TABLE_HEAD,
            )
        } else if dirty {
            (
                self.t("草稿未保存", "Unsaved draft"),
                color::DRAFT_INK,
                color::DRAFT_TINT,
            )
        } else if editing {
            (
                self.t("编辑中", "Editing"),
                color::DRAFT_INK,
                color::DRAFT_TINT,
            )
        } else {
            (
                if compact && deleted {
                    self.t("已删除 1 个", "Deleted 1")
                } else if compact && saved_copy {
                    self.t("已保存副本", "Saved copy")
                } else {
                    self.t("已保存", "Saved")
                },
                color::SUCCESS,
                color::SUCCESS_TINT,
            )
        };
        let detail = if self.busy {
            self.t("处理中", "Working")
        } else if let Some(pending) = pending {
            if pending.source_id.is_some() {
                self.t("复制草稿 · 未保存", "Copied draft")
            } else {
                self.t("未保存草稿", "Unsaved draft")
            }
        } else if dirty {
            self.t("修改待保存", "Changes pending")
        } else if editing {
            self.t("正在编辑", "Editing profile")
        } else if deleted {
            self.t("已删除 1 个", "Deleted 1")
        } else if saved_copy {
            self.t("已保存副本", "Saved copy")
        } else {
            self.t("已保存", "Saved")
        };
        div()
            .flex()
            .items_center()
            .flex_shrink_0()
            .h(px(44.))
            .px(px(16.))
            .gap(px(12.))
            .bg(rgb(color::WHITE))
            .border_1()
            .border_color(rgb(color::BORDER))
            .rounded(px(12.))
            .child(
                Label::new(self.t("当前供应商", "Current provider"))
                    .w(px(if self.zh { 76. } else { 104. }))
                    .flex_shrink_0()
                    .text_size(px(12.))
                    .text_color(rgb(color::TEXT_3)),
            )
            .child(
                div()
                    .id("provider-current-name")
                    .aria_label(display.clone())
                    .w(px(132.))
                    .min_w_0()
                    .text_ellipsis()
                    .text_size(px(16.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(display),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .flex_shrink_0()
                    .w(px(83.))
                    .h(px(26.))
                    .px(px(10.))
                    .rounded(px(7.))
                    .bg(rgb(color::BG_TABLE_HEAD))
                    .child(
                        Label::new(model_count(self.models.len(), self.zh))
                            .text_size(px(11.))
                            .text_color(rgb(color::TEXT_3)),
                    ),
            )
            .when(
                !compact && (editing || dirty || self.busy || creation.is_some() || deleted),
                |row| {
                    row.child(
                        div()
                            .id("provider-summary-detail")
                            .aria_label(detail)
                            .flex()
                            .items_center()
                            .flex_none()
                            .w(px(if deleted && !editing && !dirty && !self.busy {
                                118.
                            } else {
                                142.
                            }))
                            .h(px(26.))
                            .px(px(12.))
                            .rounded(px(8.))
                            .bg(rgb(background))
                            .child(
                                Label::new(detail)
                                    .text_size(px(11.))
                                    .line_height(px(16.))
                                    .text_color(rgb(ink)),
                            ),
                    )
                },
            )
            .child(div().flex_1())
            .child(
                div()
                    .id("provider-summary-status")
                    .aria_label(status)
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap(px(5.))
                    .flex_shrink_0()
                    .w(px(136.))
                    .h(px(27.))
                    .px(px(12.))
                    .rounded(px(9.))
                    .bg(rgb(background))
                    .child(div().size(px(6.)).flex_none().rounded_full().bg(rgb(ink)))
                    .child(Label::new(status).text_size(px(11.)).text_color(rgb(ink))),
            )
            .child(
                div()
                    .flex_none()
                    .ml(px(if compact { 12. } else { 31. }))
                    .track_focus(&self.provider_profile_focus)
                    .key_context("ProviderProfileControl")
                    .on_action(cx.listener(
                        |this, _: &ActivateProviderProfileControl, window, cx| {
                            this.toggle_provider_edit(window, cx);
                            cx.stop_propagation();
                        },
                    ))
                    .child(
                        Button::new("provider-edit-profile")
                            .disabled(self.busy)
                            .with_size(px(32.))
                            .w(px(if editing && !self.zh { 144. } else { 121. }))
                            .h(px(32.))
                            .px(px(12.))
                            .outline()
                            .accessibility_label(if editing {
                                cancel_label
                            } else {
                                self.t("编辑供应商资料", "Edit provider profile")
                            })
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(6.))
                                    .child(
                                        Icon::default()
                                            .path("studio/profile-edit.svg")
                                            .with_size(px(16.))
                                            .text_color(rgb(crate::theme::palette().dark)),
                                    )
                                    .child(
                                        Label::new(if editing {
                                            cancel_label
                                        } else {
                                            self.t("编辑资料", "Edit profile")
                                        })
                                        .text_size(px(12.))
                                        .text_color(rgb(crate::theme::palette().dark)),
                                    ),
                            )
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.toggle_provider_edit(window, cx);
                            })),
                    ),
            )
            .into_any_element()
    }

    pub(super) fn center(&mut self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        if self.selected().is_empty() {
            return card("empty-provider")
                .flex_1()
                .items_center()
                .justify_center()
                .gap(px(16.))
                .child(img("brand.png").size(px(64.)))
                .child(
                    Label::new(self.t("添加你的第一个供应商", "Add your first provider"))
                        .text_size(px(18.))
                        .font_weight(FontWeight::SEMIBOLD),
                )
                .child(hint_line(self.t(
                    "配置接口地址、Key 和模型即可开始",
                    "Configure an API URL, key and models to get started",
                )))
                .child(
                    crate::ui_components::action_button("first-provider")
                        .primary()
                        .label(self.t("添加供应商", "Add provider"))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.begin_provider_draft(false, window, cx)
                        })),
                )
                .into_any_element();
        }
        div()
            .id("provider-editor-scroll")
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .gap(px(12.))
            .overflow_y_scroll()
            .track_scroll(&self.provider_scroll)
            .child(self.connection_card(cx))
            .child(self.model_directory(window.viewport_size().width < px(1320.), cx))
            .into_any_element()
    }

    fn connection_card(&self, cx: &mut Context<Self>) -> AnyElement {
        rail_card("provider-settings")
            .flex_shrink_0()
            .min_h(px(338.))
            .gap(px(8.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(section("01", self.t("连接配置", "Connection settings")))
                    .child(hint_line(self.t(
                        "账户凭证、协议与接口地址",
                        "Account credentials, protocol and endpoint",
                    ))),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .child(field(
                        self.t("供应商预设", "Provider preset"),
                        self.preset_menu(cx),
                    ))
                    .child(
                        div()
                            .flex()
                            .gap(px(12.))
                            .child(field(
                                self.t("供应商名称", "Provider name"),
                                crate::ui_components::text_input(&self.name, self.busy)
                                    .aria_label(self.t("供应商名称", "Provider name")),
                            ))
                            .child(field(
                                "API Key",
                                crate::ui_components::masked_input(
                                    &self.key,
                                    self.busy,
                                    self.key_visibility_button(cx),
                                )
                                .aria_label("API Key"),
                            )),
                    )
                    .child(
                        div()
                            .flex()
                            .gap(px(12.))
                            .child(field(
                                self.t("上游协议", "Upstream protocol"),
                                provider_choices::control(
                                    &self.protocol_select,
                                    self.busy,
                                    self.t("上游协议", "Upstream protocol"),
                                    None,
                                    cx,
                                ),
                            ))
                            .child(field(
                                self.t("API 地址", "API URL"),
                                crate::ui_components::text_input(&self.url, self.busy)
                                    .aria_label(self.t("API 地址", "API URL"))
                                    .font_family(cx.theme().mono_font_family.clone()),
                            )),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(12.))
                    .child(
                        Button::new("protocol-options")
                            .disabled(self.busy)
                            .ghost()
                            .with_size(px(28.))
                            .h(px(28.))
                            .p_0()
                            .label(self.t("协议选项", "Protocol options"))
                            .text_color(rgb(crate::theme::palette().dark))
                            .on_click(cx.listener(|this, _, window, cx| {
                                let draft = this
                                    .drafts
                                    .providers
                                    .get(&this.selected())
                                    .cloned()
                                    .unwrap_or_default();
                                this.protocol_options.load(
                                    draft.protocol,
                                    &draft.protocol_settings,
                                    window,
                                    cx,
                                );
                                this.protocol_error = None;
                                this.protocol_scroll.set_offset(point(px(0.), px(0.)));
                                this.protocol_focused_control = if draft.protocol
                                    == crate::upstream_config::UpstreamProtocol::AnthropicMessages
                                {
                                    1
                                } else {
                                    2
                                };
                                this.protocol_layout = None;
                                this.protocol_open = true;
                                this.error = false;
                                cx.notify();
                            })),
                    )
                    .child(
                        hint_line(self.url_hint())
                            .text_size(px(11.))
                            .flex_1()
                            .min_w_0()
                            .text_ellipsis(),
                    ),
            )
            .into_any_element()
    }

    fn provider_model_table(
        &self,
        keyboard_hint: &'static str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
                    .id("provider-model-table")
                    .key_context("ModelList")
                    .track_focus(&self.table_focus)
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h(px(if self.models.len() == 1 { 129. } else { 116. }))
                    .aria_label(keyboard_hint)
                    .on_action(cx.listener(Self::remove_models))
                    .on_action(cx.listener(Self::select_models))
                    .on_action(cx.listener(Self::edit_selected))
                    .on_prepaint({
                        let owner = cx.entity().downgrade();
                        move |bounds, _, cx| {
                            let width = (bounds.size.width - px(2.)).max(px(240.));
                            let _ = owner.update(cx, |this, cx| {
                                if this.table_width != width {
                                    this.table_width = width;
                                    cx.notify();
                                }
                            });
                        }
                    })
                    .child(
                        div().min_h_0()
                            .when(self.models.len() == 1, |frame| frame.h(px(63.)).flex_shrink_0())
                            .when(self.models.len() != 1, |frame| frame.flex_1())
                            .child(crate::ui_components::table(&self.model_table).with_size(px(29.))),
                    )
                    .when(self.models.len() == 1, |table| {
                        table.child(
                            div().id("single-model-helper").flex().items_center()
                                .flex_shrink_0().h(px(50.)).max_w(px(664.)).mx(px(8.)).mt(px(16.)).px(px(14.))
                                .rounded(px(10.)).bg(rgb(color::BG_NOTICE))
                                .child(Label::new(self.t(
                                    "这里只有 1 个模型。需要增加模型时，可直接获取上游列表或手动添加。",
                                    "There is 1 model here. Fetch the upstream list or add a model manually to include more.",
                                )).w_full().min_w_0().whitespace_normal().text_size(px(12.)).line_height(px(17.)).text_color(rgb(color::TEXT_2))),
                        )
                    })
        .into_any_element()
    }

    fn model_directory(&self, compact: bool, cx: &mut Context<Self>) -> AnyElement {
        let selected = self.selected();
        let total: usize = self
            .cfg
            .providers
            .iter()
            .map(|provider| {
                if provider.id == selected {
                    self.models.len()
                } else {
                    self.drafts
                        .providers
                        .get(&provider.id)
                        .map_or(provider.models.len(), |draft| draft.models.len())
                }
            })
            .sum::<usize>()
            + if self.pending_selected() && !self.cfg.providers.iter().any(|p| p.id == selected) {
                self.models.len()
            } else {
                0
            };
        let keyboard_hint = self.t(
            "Shift 连续多选 · Ctrl / ⌘ 多选 · 删除键移除 · 双击编辑",
            "Shift range · Ctrl / ⌘ toggle · Delete to remove · Double-click to edit",
        );
        let add_disabled = self.busy || total >= config::MAX_MODELS;
        let add_ink = if add_disabled {
            color::TEXT_3
        } else {
            color::WHITE
        };
        rail_card("model-directory")
            .flex_1()
            .flex_shrink_0()
            .min_h(px(if compact { 360. } else { 280. }))
            .p(px(16.))
            .gap(px(8.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(8.))
                    .flex_shrink_0()
                    .when(compact, |header| header.flex_col().items_stretch())
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(12.))
                            .child(section("02", self.t("模型目录", "Model directory")))
                            .child(
                                Label::new(model_count(self.models.len(), self.zh))
                                    .text_size(px(11.))
                                    .text_color(rgb(color::TEXT_3)),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_end()
                            .gap(px(8.))
                            .child(
                                crate::ui_components::button(
                                    "fetch",
                                    crate::ui_components::ButtonKind::Normal,
                                    self.t("获取模型", "Fetch models"),
                                    self.busy,
                                    false,
                                )
                                .on_click(cx.listener(
                                    |this, _, _, cx| {
                                        this.fetch_provider_models(cx);
                                    },
                                )),
                            )
                            .child(
                                crate::ui_components::action_button("model-add")
                                    .primary()
                                    .accessibility_label(self.t("添加模型", "Add model"))
                                    .tooltip(self.t("添加模型", "Add model"))
                                    .disabled(add_disabled)
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(px(4.))
                                            .child(
                                                Icon::default()
                                                    .path("studio/supplier-add.svg")
                                                    .with_size(px(16.))
                                                    .text_color(rgb(add_ink)),
                                            )
                                            .child(
                                                Label::new(self.t("添加", "Add"))
                                                    .text_size(px(13.))
                                                    .text_color(rgb(add_ink)),
                                            ),
                                    )
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.edit(this.models.len(), true, window, cx)
                                    })),
                            )
                            .child(
                                crate::ui_components::named_icon_button(
                                    "model-remove",
                                    IconName::Close,
                                    self.t("删除模型", "Remove models"),
                                    self.t("删除模型", "Remove models"),
                                    self.busy || self.selection.rows.is_empty(),
                                )
                                .on_click(cx.listener(
                                    |this, _, window, cx| {
                                        this.remove_models(&RemoveModels, window, cx)
                                    },
                                )),
                            ),
                    ),
            )
            .child(self.provider_model_table(keyboard_hint, cx))
            .when(compact, |card| {
                card.child(hint_line(keyboard_hint).text_size(px(11.)))
            })
            .child(
                div()
                    .id("provider-actions")
                    .flex()
                    .items_center()
                    .justify_between()
                    .flex_shrink_0()
                    .gap(px(8.))
                    .h(px(if compact { 84. } else { size::BUTTON_HEIGHT }))
                    .when(compact, |footer| footer.flex_col().items_stretch())
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(field_label(self.t("思考强度", "Thinking effort")))
                            .child(self.effort_menu(cx)),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_end()
                            .gap(px(8.))
                            .child(
                                crate::ui_components::button(
                                    "test",
                                    crate::ui_components::ButtonKind::Outline,
                                    self.t("测试连接", "Test connection"),
                                    self.busy || self.models.is_empty(),
                                    self.busy,
                                )
                                .on_click(cx.listener(|this, _, _, cx| this.test(cx))),
                            )
                            .child(
                                crate::ui_components::button(
                                    "save",
                                    crate::ui_components::ButtonKind::Primary,
                                    self.t("保存配置", "Save configuration"),
                                    self.busy,
                                    self.busy,
                                )
                                .on_click(
                                    cx.listener(|this, _, window, cx| this.save(&Save, window, cx)),
                                ),
                            ),
                    ),
            )
            .into_any_element()
    }
}
