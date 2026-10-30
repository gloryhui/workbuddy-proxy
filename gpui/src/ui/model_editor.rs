//! Studio model editor. Capability validation and draft writes stay in their owners.
use super::*;
use crate::capabilities::{EFFORTS, ModelCapabilities};
use gpui_kit::component::{
    Disableable,
    button::ButtonCustomVariant,
    input::Input,
    popover::Popover,
    scroll::{ScrollableElement, ScrollableMask},
    tag::Tag,
};

const SOFT: u32 = crate::theme::color::BG_TABLE_HEAD;
const BORDER: u32 = crate::theme::color::BORDER;

fn accent_tint() -> u32 {
    if crate::theme::current_skin() == crate::theme::Skin::StudioIndigo {
        0xeeedff
    } else {
        crate::theme::palette().tint
    }
}

fn accent_line() -> u32 {
    if crate::theme::current_skin() == crate::theme::Skin::StudioIndigo {
        0xd5d1ff
    } else {
        crate::theme::palette().border
    }
}

// The styled Kit Checkbox derives its indicator from rems and fixes its mark
// asset. Compose its same native Base engine to retain focus, keyboard, checked
// and disabled semantics with Figma's explicit 16/12px parts instead.
fn model_checkbox(
    id: impl Into<ElementId>,
    label: &'static str,
    checked: bool,
    disabled: bool,
    read_only: bool,
    on_change: impl Fn(&bool, &mut Window, &mut App) + 'static,
) -> gpui_kit::base::Checkbox {
    let primary = rgb(crate::theme::palette().primary);
    let dim = disabled && !read_only;
    gpui_kit::base::Checkbox::new(id)
        .checked(checked)
        .disabled(disabled)
        .accessibility_label(label)
        .flex()
        .items_center()
        .gap(px(10.))
        .h(px(20.))
        .rounded(px(5.))
        .focus_visible(move |style| {
            style.shadow(vec![
                BoxShadow::new(px(0.), px(0.), primary.into())
                    .blur_radius(px(0.))
                    .spread_radius(px(1.)),
            ])
        })
        .child(
            gpui_kit::base::CheckboxIndicator::new()
                .checked(checked)
                .disabled(disabled)
                .flex()
                .items_center()
                .justify_center()
                .flex_none()
                .size(px(16.))
                .rounded(px(5.))
                .border_1()
                .border_color(if checked {
                    primary
                } else {
                    rgb(crate::theme::color::BORDER_INPUT)
                })
                .bg(if checked {
                    primary
                } else {
                    rgb(crate::theme::color::WHITE)
                })
                .when(dim, |part| part.opacity(0.5))
                .when(checked, |part| {
                    part.child(
                        svg()
                            .path("studio/model-check.svg")
                            .size(px(12.))
                            .text_color(rgb(0xffffff)),
                    )
                }),
        )
        .child(
            Label::new(label)
                .text_size(px(13.))
                .line_height(px(19.))
                .text_color(rgb(if dim {
                    MUTED
                } else if checked {
                    INK
                } else {
                    crate::theme::color::TEXT_2
                })),
        )
        .on_change(move |state, _, window, cx| {
            window.prevent_default();
            on_change(
                &(state == gpui_kit::base::CheckboxState::Checked),
                window,
                cx,
            );
        })
}

pub(super) fn surface_height(window: &Window) -> Pixels {
    (window.viewport_size().height - px(64.)).min(px(800.))
}

fn caption(text: &'static str) -> Label {
    Label::new(text)
        .text_size(px(12.))
        .line_height(px(17.))
        .text_color(rgb(MUTED))
}

fn title(text: &'static str) -> Label {
    Label::new(text)
        .text_size(px(15.))
        .line_height(px(22.))
        .font_weight(FontWeight::BOLD)
        .text_color(rgb(INK))
}

fn field(label: &'static str, control: impl IntoElement) -> Div {
    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_w_0()
        .flex_shrink_0()
        .gap(px(4.))
        .child(caption(label))
        .child(control)
}

fn input(state: &Entity<InputState>, disabled: bool, label: &'static str) -> Input {
    Styled::h(crate::ui_components::text_input(state, disabled), px(41.))
        .bg(rgb(if disabled {
            SOFT
        } else {
            crate::theme::color::BG_BAR
        }))
        .border_color(rgb(BORDER))
        .rounded(px(10.))
        .aria_label(label)
}

#[derive(Clone, Copy)]
enum Flag {
    Tools,
    Images,
    Reasoning,
    OnlyReasoning,
    DisableThinking,
    CustomProtocol,
}
impl Flag {
    fn get(self, value: &ModelCapabilities) -> bool {
        match self {
            Self::Tools => value.supports_tool_call,
            Self::Images => value.supports_images,
            Self::Reasoning => value.supports_reasoning,
            Self::OnlyReasoning => value.only_reasoning,
            Self::DisableThinking => value.reasoning.can_disable_thinking,
            Self::CustomProtocol => value.use_custom_protocol,
        }
    }
    fn set(self, value: &mut ModelCapabilities, enabled: bool) {
        match self {
            Self::Tools => value.supports_tool_call = enabled,
            Self::Images => value.supports_images = enabled,
            Self::Reasoning => {
                value.supports_reasoning = enabled;
                if !enabled {
                    value.only_reasoning = false;
                }
            }
            Self::OnlyReasoning => {
                value.only_reasoning = enabled;
                if enabled {
                    value.reasoning.can_disable_thinking = false;
                }
            }
            Self::DisableThinking => value.reasoning.can_disable_thinking = enabled,
            Self::CustomProtocol => value.use_custom_protocol = enabled,
        }
    }
}

impl Proxy {
    pub(super) fn subscribe_model_editor_focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        // One non-tab-stop focus scope per row also covers native checkboxes and
        // toggle buttons, without adding extra stops or replacing Kit's focus.
        for (row, handle) in self.model_editor_focus.iter().enumerate() {
            cx.on_focus_in(handle, window, move |this, _, cx| {
                if this.editing.is_some() {
                    this.model_editor_focused_row = row;
                    this.model_editor_scroll.scroll_to_item(row);
                    cx.notify();
                }
            })
            .detach();
        }
        // Track entity-owned input/select handles directly as well. They keep
        // keyboard focus even if a resize clips their enclosing dispatch node.
        for (handle, row) in [
            (self.edit_name.read(cx).focus_handle(cx), 0),
            (self.edit_model.read(cx).focus_handle(cx), 0),
            (
                self.model_options.default_effort.read(cx).focus_handle(cx),
                3,
            ),
            (self.model_options.limits[0].read(cx).focus_handle(cx), 4),
            (self.model_options.limits[1].read(cx).focus_handle(cx), 4),
            (self.model_options.limits[2].read(cx).focus_handle(cx), 5),
            (self.model_options.limits[3].read(cx).focus_handle(cx), 5),
        ] {
            cx.on_focus(&handle, window, move |this, _, cx| {
                if this.editing.is_some() {
                    this.model_editor_focused_row = row;
                    this.model_editor_scroll.scroll_to_item(row);
                    cx.notify();
                }
            })
            .detach();
        }
        cx.observe_window_bounds(window, |this, _, cx| {
            if this.editing.is_some() {
                cx.notify();
            }
        })
        .detach();
    }

    fn model_source_note(&self) -> &'static str {
        if !self.model_options.automatic {
            return self.t(
                "使用手动设置，自动识别不会覆盖",
                "Manual settings take precedence over detected capabilities",
            );
        }
        match self.model_options.source.as_str() {
            "upstream" => self.t(
                "已自动填充 · 供应商模型信息",
                "Auto-filled · Provider model metadata",
            ),
            "models.dev:provider" => self.t(
                "已自动填充 · models.dev 供应商目录",
                "Auto-filled · models.dev provider catalog",
            ),
            "models.dev:canonical" => self.t(
                "已自动填充 · models.dev 原厂模型信息",
                "Auto-filled · models.dev canonical model",
            ),
            "builtin" => self.t(
                "使用内置能力预设，自动尝试补齐",
                "Built-in defaults; checking for additional metadata",
            ),
            _ => self.t(
                "暂未找到可信能力信息，可手动设置",
                "No confirmed metadata yet; manual settings are available",
            ),
        }
    }

    fn model_notice(&self, cx: &mut Context<Self>) -> Div {
        let automatic = self.model_options.automatic;
        let unknown_image = automatic && !self.model_options.image_known;
        div()
            .flex()
            .flex_col()
            .gap(px(3.))
            .h(px(if unknown_image { 101. } else { 77. }))
            .px(px(16.))
            .pt(px(13.))
            .pb(px(14.))
            .mr(px(2.))
            .rounded(px(12.))
            .bg(rgb(if automatic { accent_tint() } else { SOFT }))
            .child(
                div()
                    .flex()
                    .items_center()
                    .h(px(27.))
                    .gap(px(13.))
                    .flex_none()
                    .child(
                        Tag::custom(
                            rgb(crate::theme::color::BG_BAR).into(),
                            rgb(crate::theme::palette().dark).into(),
                            rgb(crate::theme::color::BG_BAR).into(),
                        )
                        .small()
                        .h(px(27.))
                        .min_w(px(80.))
                        .rounded(px(9.))
                        .text_size(px(11.))
                        .child(if automatic {
                            "● AUTO"
                        } else {
                            "MANUAL"
                        }),
                    )
                    .child(
                        model_checkbox(
                            "model-capabilities-auto",
                            self.t(
                                "自动识别模型能力（取消后自定义）",
                                "Detect capabilities automatically",
                            ),
                            automatic,
                            false,
                            false,
                            cx.listener(|this, enabled: &bool, window, cx| {
                                if *enabled {
                                    let model = this.model_id_for_options(cx);
                                    this.model_options.load(
                                        &Value::Null,
                                        &model,
                                        this.zh,
                                        window,
                                        cx,
                                    );
                                } else {
                                    this.model_options.automatic = false;
                                }
                                cx.notify();
                            }),
                        )
                        .text_size(px(13.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .tooltip({
                            let text = self.t(
                                "使用内置模型规则与 models.dev 元数据，取消勾选后手动覆盖",
                                "Use built-in rules and models.dev metadata; uncheck to customize",
                            ).to_owned();
                            move |window, cx| {
                                crate::ui_components::full_value_tooltip(text.clone(), window, cx)
                            }
                        }),
                    ),
            )
            .child(caption(self.model_source_note()).h(px(20.)).flex_none())
            .when(unknown_image, |notice| {
                notice.child(
                    caption(self.t(
                        "图片能力尚未确认，未自动开启",
                        "Image capability is unknown and has not been enabled automatically",
                    ))
                    .h(px(20.))
                    .flex_none()
                    .text_color(rgb(crate::theme::color::WARNING)),
                )
            })
    }

    fn model_capability_help(&self) -> AnyElement {
        let explanation = self.t(
            "图片输入用于附件和读图工具；其他媒体格式按上游接口透传。自定义协议同步完整 Chat Completions 地址。",
            "Image input enables attachments and image tools. Other media follows the upstream format. Custom protocol uses the full Chat Completions URL.");
        Popover::new("model-capability-help")
            .trigger(
                crate::ui_components::action_button("model-capability-help-button")
                    .ghost()
                    .h(px(24.))
                    .px(px(8.))
                    .label(self.t("能力说明", "About capabilities")),
            )
            .content(move |_, _, _| {
                div()
                    .w(px(460.))
                    .p(px(16.))
                    .child(
                        Label::new(explanation)
                            .text_size(px(13.))
                            .line_height(px(20.))
                            .whitespace_normal()
                            .h_auto(),
                    )
                    .into_any_element()
            })
            .into_any_element()
    }

    fn model_flags(&self, cx: &mut Context<Self>) -> Div {
        let flags = [
            (Flag::Tools, "工具调用", "Tool calls"),
            (Flag::Images, "图片输入", "Image input"),
            (Flag::Reasoning, "思考模式", "Reasoning"),
            (Flag::OnlyReasoning, "仅思考模式", "Reasoning only"),
            (Flag::DisableThinking, "允许关闭思考", "Allow thinking off"),
            (Flag::CustomProtocol, "自定义协议", "Custom protocol"),
        ];
        let rows = flags.chunks(3).enumerate().map(|(row, flags)| {
            div()
                .flex()
                .items_center()
                .gap_0()
                .h(px(20.))
                .flex_none()
                .mt(px(if row == 0 { 12. } else { 17. }))
                .children(flags.iter().enumerate().map(|(column, &(flag, cn, en))| {
                    let unavailable = self.model_options.automatic
                        || (matches!(flag, Flag::CustomProtocol) && self.native_protocol())
                        || (matches!(flag, Flag::OnlyReasoning | Flag::DisableThinking)
                            && !self.model_options.value.supports_reasoning)
                        || (matches!(flag, Flag::DisableThinking)
                            && self.model_options.value.only_reasoning);
                    model_checkbox(
                        ("model-capability", row * 3 + column),
                        self.t(cn, en),
                        flag.get(&self.model_options.value)
                            && !(matches!(flag, Flag::CustomProtocol) && self.native_protocol()),
                        unavailable,
                        self.model_options.automatic,
                        cx.listener(move |this, enabled: &bool, _, cx| {
                            flag.set(&mut this.model_options.value, *enabled);
                            cx.notify();
                        }),
                    )
                    .when(column < 2, |cell| {
                        cell.flex_none()
                            .w(px(if column == 0 { 198. } else { 200. }))
                    })
                    .when(column == 2, |cell| cell.flex_1())
                    .min_w_0()
                    .text_size(px(13.))
                    .h(px(20.))
                }))
        });
        div()
            .flex()
            .flex_col()
            .pb(px(27.))
            .border_b_1()
            .border_color(rgb(BORDER))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_0()
                    .h(px(24.))
                    .flex_none()
                    .child(
                        title(self.t("模型能力", "Capabilities"))
                            .w(px(154.))
                            .flex_none(),
                    )
                    .child(
                        Label::new(self.t(
                            "WorkBuddy 能力声明，不修改转发的消息",
                            "Capabilities only; messages are unchanged",
                        ))
                        .text_size(px(11.))
                        .flex_1()
                        .min_w_0()
                        .text_ellipsis()
                        .text_color(rgb(MUTED)),
                    )
                    .child(self.model_capability_help()),
            )
            .children(rows)
    }

    fn model_efforts(&self, cx: &mut Context<Self>) -> Div {
        let disabled = self.model_options.automatic || !self.model_options.value.supports_reasoning;
        let toggles = EFFORTS
            .iter()
            .zip([
                ("低", "Low"),
                ("中", "Medium"),
                ("高", "High"),
                ("超高", "Extra high"),
                ("极致", "Max"),
            ])
            .enumerate()
            .map(|(index, (effort, (cn, en)))| {
                let effort = (*effort).to_owned();
                let active = self
                    .model_options
                    .value
                    .reasoning
                    .supported_efforts
                    .contains(&effort);
                crate::ui_components::action_button(("model-supported-effort", index))
                    .h(px(36.))
                    .flex_1()
                    .min_w_0()
                    .rounded(px(9.))
                    .text_size(px(13.))
                    .justify_start()
                    .custom(
                        ButtonCustomVariant::new(cx)
                            .color(rgb(if active { accent_tint() } else { SOFT }).into())
                            .foreground(
                                rgb(if active {
                                    crate::theme::palette().dark
                                } else {
                                    MUTED
                                })
                                .into(),
                            )
                            .hover(rgb(accent_tint()).into()),
                    )
                    .px(px(11.))
                    .bg(rgb(if active { accent_tint() } else { SOFT }))
                    .text_color(rgb(if active {
                        crate::theme::palette().dark
                    } else {
                        MUTED
                    }))
                    .border_1()
                    .border_color(rgb(if active { accent_line() } else { SOFT }))
                    .accessibility_label(self.t(cn, en))
                    .child(
                        Label::new(if active {
                            format!("✓  {}", self.t(cn, en))
                        } else {
                            self.t(cn, en).to_owned()
                        })
                        .w_full()
                        .text_size(px(13.))
                        .line_height(px(19.))
                        .text_color(rgb(if active {
                            crate::theme::palette().dark
                        } else {
                            MUTED
                        }))
                        .font_weight(if active {
                            FontWeight::BOLD
                        } else {
                            FontWeight::MEDIUM
                        }),
                    )
                    .toggled(active)
                    .disabled(disabled)
                    .when(disabled && !self.model_options.automatic, |button| {
                        button.opacity(0.5)
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let efforts = &mut this.model_options.value.reasoning.supported_efforts;
                        efforts.retain(|s| s != &effort);
                        if !active {
                            efforts.push(effort.clone());
                        }
                        efforts.sort_by_key(|s| EFFORTS.iter().position(|v| *v == s).unwrap_or(5));
                        cx.notify();
                    }))
            });
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .h(px(30.))
                    .flex_none()
                    .gap(px(16.))
                    .child(title(
                        self.t("支持的思考档位", "Supported thinking efforts"),
                    ))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                Label::new(self.t("默认思考强度", "Default effort"))
                                    .text_size(px(11.))
                                    .text_color(rgb(MUTED)),
                            )
                            .child(
                                crate::ui_components::select(
                                    &self.model_options.default_effort,
                                    disabled,
                                )
                                .w(px(136.))
                                .h(px(30.))
                                .rounded(px(9.))
                                .accessibility_label(
                                    self.t("默认思考强度", "Default thinking effort"),
                                ),
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .h(px(36.))
                    .flex_none()
                    .children(toggles),
            )
    }

    pub(super) fn model_editor_dialog(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let height = surface_height(window) - px(42.);
        let footer_height = if self.model_edit_error.is_some() {
            114.
        } else {
            60.
        };
        let form_height = (height - px(62. + footer_height)).max(px(100.));
        let automatic = self.model_options.automatic;
        let unknown_image = automatic && !self.model_options.image_known;
        let layout_key = (form_height, unknown_image);
        if self.model_editor_layout != Some(layout_key) {
            self.model_editor_layout = Some(layout_key);
            // Validation, resizing and arriving metadata can move an already
            // focused row without a focus event. A clipped input can retain
            // keyboard focus while absent from the rendered focus tree, so use
            // the last native focus-in group instead of querying that tree.
            // ScrollHandle consumes its active-item request before updating the
            // viewport bounds in prepaint. Wait for this layout to finish, then
            // request one additional paint with the new bounds. This is a
            // one-shot geometry correction, never a recurring redraw.
            cx.on_next_frame(window, |this, _, cx| {
                if this.editing.is_some() {
                    this.model_editor_scroll
                        .scroll_to_item(this.model_editor_focused_row);
                    cx.notify();
                }
            });
        }
        let needs_gutter =
            surface_height(window) < px(800.) || self.model_edit_error.is_some() || unknown_image;
        let pair = |start: usize, labels: [(&'static str, &'static str); 2]| {
            div().flex().gap(px(11.)).pr(px(1.)).flex_none().children(
                labels.into_iter().enumerate().map(|(offset, (cn, en))| {
                    let label = self.t(cn, en);
                    field(
                        label,
                        input(&self.model_options.limits[start + offset], automatic, label),
                    )
                }),
            )
        };
        let rows = div()
            .id("model-editor-rows")
            // Leave the native overlay thumb room beside text and field borders.
            .when(needs_gutter, |rows| rows.pr(px(12.)))
            .flex()
            .flex_col()
            .w_full()
            .pb(px(10.))
            .child(
                div()
                    .flex()
                    .gap(px(13.))
                    .pr(px(1.))
                    .pt(px(20.))
                    .flex_none()
                    .track_focus(&self.model_editor_focus[0])
                    .child(
                        field(
                            self.t("显示名称", "Display name"),
                            input(&self.edit_name, false, self.t("显示名称", "Display name")),
                        )
                        .flex_none()
                        .w(px(329.)),
                    )
                    .child(field(
                        self.t("上游模型 ID", "Upstream model ID"),
                        input(
                            &self.edit_model,
                            false,
                            self.t("上游模型 ID", "Upstream model ID"),
                        ),
                    )),
            )
            .child(
                div()
                    .pt(px(22.))
                    .flex_none()
                    .track_focus(&self.model_editor_focus[1])
                    .child(self.model_notice(cx)),
            )
            .child(
                div()
                    .pt(px(26.))
                    .flex_none()
                    .track_focus(&self.model_editor_focus[2])
                    .child(self.model_flags(cx)),
            )
            .child(
                div()
                    .pt(px(16.))
                    .flex_none()
                    .track_focus(&self.model_editor_focus[3])
                    .child(self.model_efforts(cx)),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .pt(px(31.))
                    .gap(px(10.))
                    .flex_none()
                    .track_focus(&self.model_editor_focus[4])
                    .child(title(
                        self.t("Token 与上下文限制", "Token and context limits"),
                    ))
                    .child(pair(
                        0,
                        [
                            ("最大输入 tokens", "Max input tokens"),
                            ("最大输出 tokens", "Max output tokens"),
                        ],
                    )),
            )
            .child(
                div()
                    .pt(px(19.))
                    .flex_none()
                    .track_focus(&self.model_editor_focus[5])
                    .child(pair(
                        2,
                        [
                            ("默认上下文长度", "Default context length"),
                            (
                                "可选上下文长度（逗号分隔）",
                                "Context choices (comma separated)",
                            ),
                        ],
                    )),
            );
        let form = div()
            .relative()
            .w_full()
            .h(form_height)
            .flex_none()
            .child(
                rows.h(form_height)
                    .overflow_y_scroll()
                    .track_scroll(&self.model_editor_scroll),
            )
            .child(ScrollableMask::new(
                Axis::Vertical,
                &self.model_editor_scroll,
            ))
            .vertical_scrollbar(&self.model_editor_scroll);
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
                    .h(px(62.))
                    .flex_none()
                    .gap(px(4.))
                    .border_b_1()
                    .border_color(rgb(BORDER))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .h(px(30.))
                            .pr(px(13.))
                            .flex_none()
                            .child(
                                Label::new(if self.new_model {
                                    self.t("添加模型", "Add model")
                                } else {
                                    self.t("编辑模型", "Edit model")
                                })
                                .text_size(px(21.))
                                .font_weight(FontWeight::BOLD)
                                .text_color(rgb(INK)),
                            )
                            .child(
                                Tag::custom(
                                    rgb(if automatic {
                                        crate::theme::color::SUCCESS_TINT
                                    } else {
                                        crate::theme::palette().tint
                                    })
                                    .into(),
                                    rgb(if automatic {
                                        crate::theme::color::SUCCESS
                                    } else {
                                        crate::theme::palette().dark
                                    })
                                    .into(),
                                    rgb(if automatic {
                                        crate::theme::color::SUCCESS_TINT
                                    } else {
                                        crate::theme::palette().tint
                                    })
                                    .into(),
                                )
                                .small()
                                .h(px(27.))
                                .min_w(px(104.))
                                .rounded(px(9.))
                                .text_size(px(11.))
                                .child(if automatic {
                                    self.t("自动识别", "Automatic")
                                } else {
                                    self.t("手动覆盖", "Manual override")
                                }),
                            ),
                    )
                    .child(caption(self.t(
                        "按模型真实能力生成 WorkBuddy 选择器，确认后仍需保存供应商",
                        "Use real capabilities in WorkBuddy; confirm, then save this provider.",
                    ))),
            )
            .child(form)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .h(px(footer_height))
                    .flex_none()
                    .gap(px(10.))
                    .border_t_1()
                    .border_color(rgb(BORDER))
                    .pt(px(if self.model_edit_error.is_some() {
                        12.
                    } else {
                        21.
                    }))
                    .when_some(self.model_edit_error.clone(), |footer, error| {
                        footer.child(
                            Label::new(error)
                                .text_size(px(12.))
                                .line_height(px(17.))
                                .whitespace_normal()
                                .h_auto()
                                .flex_none()
                                .text_color(rgb(crate::theme::color::DANGER)),
                        )
                    })
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_end()
                            .gap(px(14.))
                            .flex_none()
                            .child(
                                crate::ui_components::action_button("edit-cancel")
                                    .outline()
                                    .h(px(36.))
                                    .min_w(px(90.))
                                    .rounded(px(9.))
                                    .label(self.t("取消", "Cancel"))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.dismiss(&Dismiss, window, cx)
                                    })),
                            )
                            .child(
                                crate::ui_components::action_button("edit-confirm")
                                    .primary()
                                    .h(px(36.))
                                    .min_w(px(110.))
                                    .rounded(px(9.))
                                    .label(self.t("确认修改", "Confirm changes"))
                                    .on_click(cx.listener(|this, _, _, cx| this.confirm_edit(cx))),
                            ),
                    ),
            )
            .into_any_element()
    }
}
