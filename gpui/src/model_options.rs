//! Per-model WorkBuddy capability editor, independent from request overrides.
use crate::theme::{color, font, line_height, space};
use crate::{
    capabilities::{ContextWindow, EFFORTS, ModelCapabilities},
    ui::Proxy,
};
use gpui_kit::component::{
    input::InputState,
    label::Label,
    select::{SelectItem, SelectState},
};
use gpui_kit::*;
use serde_json::Value;

#[derive(Clone)]
pub struct EffortChoice {
    code: String,
    title: SharedString,
}
impl SelectItem for EffortChoice {
    type Value = String;
    fn title(&self) -> SharedString {
        self.title.clone()
    }
    fn value(&self) -> &String {
        &self.code
    }
}
fn choices(zh: bool) -> Vec<EffortChoice> {
    EFFORTS
        .iter()
        .zip([
            ("低", "Low"),
            ("中", "Medium"),
            ("高", "High"),
            ("超高", "Extra high"),
            ("极致", "Max"),
        ])
        .map(|(code, (cn, en))| EffortChoice {
            code: (*code).into(),
            title: (if zh { cn } else { en }).into(),
        })
        .collect()
}
pub struct OptionsEditor {
    pub automatic: bool,
    pub source: String,
    pub image_known: bool,
    pub value: ModelCapabilities,
    pub default_effort: Entity<SelectState<Vec<EffortChoice>>>,
    limits: [Entity<InputState>; 4],
}
impl OptionsEditor {
    pub fn new(zh: bool, window: &mut Window, cx: &mut App) -> Self {
        Self {
            automatic: true,
            source: "unknown".into(),
            image_known: false,
            value: ModelCapabilities::default(),
            default_effort: cx.new(|cx| SelectState::new(choices(zh), None, window, cx)),
            limits: std::array::from_fn(|_| cx.new(|cx| InputState::new(window, cx))),
        }
    }
    pub fn load(&mut self, row: &Value, model: &str, zh: bool, window: &mut Window, cx: &mut App) {
        let stored = row
            .get("capabilities")
            .filter(|v| !v.is_null())
            .and_then(|v| serde_json::from_value(v.clone()).ok());
        self.automatic = stored.is_none();
        self.value = stored.unwrap_or_else(|| ModelCapabilities::for_model(model));
        self.source = if !self.automatic {
            "manual"
        } else if self.value != ModelCapabilities::default() {
            "builtin"
        } else {
            "unknown"
        }
        .into();
        self.image_known = self.source != "unknown";
        self.refresh(zh, window, cx);
    }
    pub fn apply_automatic(
        &mut self,
        metadata: &Value,
        zh: bool,
        window: &mut Window,
        cx: &mut App,
    ) -> bool {
        let Ok(value) =
            serde_json::from_value::<ModelCapabilities>(metadata["capabilities"].clone())
        else {
            return false;
        };
        if !self.automatic || value.validate().is_err() {
            return false;
        }
        let source = metadata["sources"]
            .as_array()
            .and_then(|s| s.last())
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        let known = metadata["image_known"].as_bool().unwrap_or(false);
        if self.value == value && self.source == source && self.image_known == known {
            return false;
        }
        self.value = value;
        self.source = source.to_owned();
        self.image_known = known;
        self.refresh(zh, window, cx);
        true
    }
    pub fn refresh(&mut self, zh: bool, window: &mut Window, cx: &mut App) {
        let window_values = self.value.context_window.as_ref();
        let values = [
            self.value
                .max_input_tokens
                .map(|n| n.to_string())
                .unwrap_or_default(),
            self.value
                .max_output_tokens
                .map(|n| n.to_string())
                .unwrap_or_default(),
            window_values
                .map(|w| w.default_length.to_string())
                .unwrap_or_default(),
            window_values
                .map(|w| {
                    w.supported_lengths
                        .iter()
                        .map(u32::to_string)
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default(),
        ];
        for (i, (state, value)) in self.limits.iter().zip(values).enumerate() {
            state.update(cx, |s, cx| {
                s.set_placeholder(
                    (if i == 3 {
                        if zh {
                            "例如 128000, 300000, 1000000"
                        } else {
                            "e.g. 128000, 300000, 1000000"
                        }
                    } else if zh {
                        "留空使用默认值"
                    } else {
                        "Provider default"
                    })
                    .to_owned(),
                    window,
                    cx,
                );
                s.set_value(value, window, cx);
            });
        }
        self.default_effort.update(cx, |s, cx| {
            s.set_items(choices(zh), window, cx);
            s.set_selected_value(&self.value.reasoning.default_effort, window, cx);
        });
    }
    pub fn read(&self, cx: &App) -> Result<Option<ModelCapabilities>, String> {
        if self.automatic {
            return Ok(None);
        }
        let mut value = self.value.clone();
        value.max_input_tokens = number(&self.limits[0].read(cx).value())?;
        value.max_output_tokens = number(&self.limits[1].read(cx).value())?;
        let default = number(&self.limits[2].read(cx).value())?;
        let raw = self.limits[3].read(cx).value();
        let lengths = raw
            .split([',', '，'])
            .filter(|s| !s.trim().is_empty())
            .map(|s| number(s).map(|n| n.unwrap()))
            .collect::<Result<Vec<_>, _>>()?;
        value.context_window = match default {
            Some(default_length) => Some(ContextWindow {
                default_length,
                supported_lengths: if lengths.is_empty() {
                    vec![default_length]
                } else {
                    lengths
                },
            }),
            None if lengths.is_empty() => None,
            None => return Err("请填写默认上下文长度 / Enter the default context length".into()),
        };
        value.validate()?;
        Ok(Some(value))
    }
}
fn number(text: &str) -> Result<Option<u32>, String> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    text.parse::<u32>()
        .ok()
        .filter(|n| *n > 0)
        .map(Some)
        .ok_or_else(|| {
            "Token 数须为正整数，留空使用默认值 / Token counts must be positive integers or empty"
                .into()
        })
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

/// 12px muted label/explanation used by the model editor.
fn caption(text: impl Into<SharedString>) -> Label {
    Label::new(text)
        .text_size(px(font::LABEL))
        .line_height(px(line_height::LABEL))
        .text_color(rgb(color::TEXT_3))
}
/// A labelled field column (label above control, shared 12px gap).
fn field(label: Label, control: impl IntoElement) -> Div {
    div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .gap(px(space::SMALL))
        .child(label)
        .child(control)
}
pub fn render(owner: &Proxy, cx: &mut Context<Proxy>) -> AnyElement {
    let editor = &owner.model_options;
    let disabled = editor.automatic;
    let flags = [
        (Flag::Tools, "工具调用", "Tool calls"),
        (Flag::Images, "图片输入", "Image input"),
        (Flag::Reasoning, "思考模式", "Reasoning"),
        (Flag::OnlyReasoning, "仅思考模式", "Reasoning only"),
        (Flag::DisableThinking, "允许关闭思考", "Allow thinking off"),
        (Flag::CustomProtocol, "自定义协议", "Custom protocol"),
    ];
    let checks = div().flex().flex_wrap().gap(px(space::CARD_GAP)).children(
        flags.into_iter().enumerate().map(|(i, (flag, cn, en))| {
            let unavailable = disabled
                || (matches!(flag, Flag::CustomProtocol) && owner.native_protocol())
                || (matches!(flag, Flag::OnlyReasoning | Flag::DisableThinking)
                    && !editor.value.supports_reasoning)
                || (matches!(flag, Flag::DisableThinking) && editor.value.only_reasoning);
            crate::ui_components::checkbox(
                ("model-capability", i),
                owner.t(cn, en),
                flag.get(&editor.value)
                    && !(matches!(flag, Flag::CustomProtocol) && owner.native_protocol()),
                unavailable,
            )
            .on_change(cx.listener(move |this, enabled: &bool, _, cx| {
                flag.set(&mut this.model_options.value, *enabled);
                cx.notify();
            }))
        }),
    );
    let reasoning_disabled = disabled || !editor.value.supports_reasoning;
    let effort_checks = div().flex().flex_wrap().gap(px(space::CARD_GAP)).children(
        EFFORTS
            .iter()
            .zip([
                ("低", "Low"),
                ("中", "Medium"),
                ("高", "High"),
                ("超高", "Extra high"),
                ("极致", "Max"),
            ])
            .enumerate()
            .map(|(i, (effort, (cn, en)))| {
                let effort = (*effort).to_owned();
                crate::ui_components::checkbox(
                    ("model-supported-effort", i),
                    owner.t(cn, en),
                    editor.value.reasoning.supported_efforts.contains(&effort),
                    reasoning_disabled,
                )
                .on_change(cx.listener(move |this, enabled: &bool, _, cx| {
                    let efforts = &mut this.model_options.value.reasoning.supported_efforts;
                    efforts.retain(|s| s != &effort);
                    if *enabled {
                        efforts.push(effort.clone());
                    }
                    efforts.sort_by_key(|s| EFFORTS.iter().position(|v| *v == s).unwrap_or(5));
                    cx.notify();
                }))
            }),
    );
    let limits = div().flex().gap(px(space::CARD_GAP)).children(
        [
            (0, "最大输入 tokens", "Max input tokens"),
            (1, "最大输出 tokens", "Max output tokens"),
        ]
        .into_iter()
        .map(|(i, cn, en)| {
            field(
                caption(owner.t(cn, en)),
                crate::ui_components::text_input(&editor.limits[i], disabled),
            )
        }),
    );
    let context = div().flex().gap(px(space::CARD_GAP)).children(
        [
            (2, "默认上下文长度", "Default context length"),
            (
                3,
                "可选上下文长度（逗号分隔）",
                "Context choices (comma separated)",
            ),
        ]
        .into_iter()
        .map(|(i, cn, en)| {
            field(
                caption(owner.t(cn, en)),
                crate::ui_components::text_input(&editor.limits[i], disabled),
            )
        }),
    );
    div()
        .flex()
        .flex_col()
        .gap(px(space::CARD_GAP))
        .text_size(px(font::BODY))
        .child(
            div()
                .font_weight(FontWeight::SEMIBOLD)
                .text_size(px(font::BODY))
                .child(owner.t("WorkBuddy 模型能力", "WorkBuddy model capabilities")),
        )
        .child(
            crate::ui_components::checkbox(
                "model-capabilities-auto",
                owner.t(
                    "自动识别模型能力（取消后自定义）",
                    "Detect capabilities automatically (uncheck to customize)",
                ),
                editor.automatic,
                false,
            )
            .on_change(cx.listener(|this, enabled: &bool, window, cx| {
                if *enabled {
                    let model = this.model_id_for_options(cx);
                    let zh = this.is_chinese();
                    this.model_options.load(&Value::Null, &model, zh, window, cx);
                } else {
                    this.model_options.automatic = false;
                }
                cx.notify();
            })),
        )
        .child(caption(if editor.automatic {
            match editor.source.as_str() {
                "upstream" => {
                    owner.t("已自动填充 · 供应商模型信息", "Auto-filled · Provider model metadata")
                }
                "models.dev:provider" => owner.t(
                    "已自动填充 · models.dev 供应商目录",
                    "Auto-filled · models.dev provider catalog",
                ),
                "models.dev:canonical" => owner.t(
                    "已自动填充 · models.dev 原厂模型信息",
                    "Auto-filled · models.dev canonical model",
                ),
                "builtin" => owner.t(
                    "使用内置能力预设，自动尝试补齐",
                    "Built-in defaults; checking for additional metadata",
                ),
                _ => owner.t(
                    "暂未找到可信能力信息，可手动设置",
                    "No confirmed metadata yet; manual settings are available",
                ),
            }
        } else {
            owner.t(
                "使用手动设置，自动识别不会覆盖",
                "Manual settings take precedence over detected capabilities",
            )
        }))
        .children((editor.automatic && !editor.image_known).then(|| {
            caption(owner.t(
                "图片能力尚未确认，未自动开启",
                "Image capability is unknown and has not been enabled automatically",
            ))
        }))
        .child(checks)
        .child(caption(owner.t(
            "图片输入用于附件和读图工具；其他媒体格式按上游接口透传。自定义协议同步完整 Chat Completions 地址。",
            "Image input enables attachments and image tools. Other media follows the upstream format. Custom protocol uses the full Chat Completions URL.",
        )))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(space::SMALL))
                .child(caption(owner.t("默认思考强度", "Default thinking effort")))
                .child(
                    crate::ui_components::select(&editor.default_effort, reasoning_disabled)
                        .w_full(),
                ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(space::SMALL))
                .child(caption(owner.t("支持的思考强度", "Supported thinking efforts")))
                .child(effort_checks),
        )
        .child(limits)
        .child(context)
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::number;
    #[test]
    fn token_limits_cannot_silently_fall_back_on_invalid_input() {
        assert_eq!(number(" ").unwrap(), None);
        assert_eq!(number("1000000").unwrap(), Some(1_000_000));
        for input in ["0", "-1", "1.5", "4294967296", "many"] {
            assert!(number(input).is_err());
        }
    }
}
