//! Typed native provider controls; all edits remain drafts until Save.
use crate::theme::{color, font, line_height, radius, size, space};
use crate::upstream_config::{ProtocolSettings, ReasoningMode, UpstreamProtocol};
use gpui_kit::{
    component::{
        input::InputState,
        label::Label,
        select::{SelectItem, SelectState},
    },
    *,
};
use std::collections::BTreeMap;
const EFFORTS: [&str; 7] = ["none", "minimal", "low", "medium", "high", "xhigh", "max"];
#[derive(Clone)]
pub struct Mode {
    code: String,
    title: SharedString,
}
impl SelectItem for Mode {
    type Value = String;
    fn title(&self) -> SharedString {
        self.title.clone()
    }
    fn value(&self) -> &String {
        &self.code
    }
}
fn modes(protocol: UpstreamProtocol) -> Vec<Mode> {
    let entries: &[(&str, &str)] = match protocol {
        UpstreamProtocol::AnthropicMessages => &[
            ("none", "未配置 / Unconfigured"),
            ("anthropic_budget", "固定预算 / Token budget"),
            ("anthropic_adaptive", "自适应 / Adaptive"),
        ],
        UpstreamProtocol::GeminiGenerateContent => &[
            ("none", "未配置 / Unconfigured"),
            ("gemini_budget", "固定预算 / Token budget"),
            ("gemini_level", "思考档位 / Thinking level"),
        ],
        _ => &[("none", "原生 effort / Native effort")],
    };
    entries
        .iter()
        .map(|(code, title)| Mode {
            code: (*code).into(),
            title: (*title).into(),
        })
        .collect()
}
pub struct Editor {
    protocol: UpstreamProtocol,
    limit: Entity<InputState>,
    authentication: Entity<SelectState<Vec<Mode>>>,
    mode: Entity<SelectState<Vec<Mode>>>,
    mapping: [Entity<InputState>; 7],
}
impl Editor {
    pub fn new(window: &mut Window, cx: &mut App) -> Self {
        Self {
            protocol: UpstreamProtocol::default(),
            authentication: cx.new(|cx| {
                SelectState::new(
                    vec![
                        Mode {
                            code: "protocol_default".into(),
                            title: "协议默认 / Protocol default".into(),
                        },
                        Mode {
                            code: "azure_api_key".into(),
                            title: "Azure API Key".into(),
                        },
                    ],
                    None,
                    window,
                    cx,
                )
            }),
            limit: cx.new(|cx| InputState::new(window, cx)),
            mode: cx
                .new(|cx| SelectState::new(modes(UpstreamProtocol::default()), None, window, cx)),
            mapping: std::array::from_fn(|_| cx.new(|cx| InputState::new(window, cx))),
        }
    }
    /// First control that exists inside the dialog for the loaded protocol.
    ///
    /// The modal focus trap only engages while focus is inside the dialog, so
    /// opening the protocol editor must focus a rendered control rather than
    /// the window root. Otherwise Tab can leave the modal and Enter can reach
    /// background controls (the X03 focus-escape bug).
    pub fn initial_focus(&self, cx: &App) -> FocusHandle {
        if initial_focus_uses_limit(self.protocol) {
            self.limit.read(cx).focus_handle(cx)
        } else {
            self.mode.read(cx).focus_handle(cx)
        }
    }

    pub fn load(
        &mut self,
        protocol: UpstreamProtocol,
        settings: &ProtocolSettings,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.protocol = protocol;
        let authentication = serde_json::to_value(settings.authentication)
            .unwrap()
            .as_str()
            .unwrap()
            .to_owned();
        self.authentication.update(cx, |s, cx| {
            s.set_selected_value(&authentication, window, cx)
        });
        self.limit.update(cx, |s, cx| {
            s.set_value(settings.default_max_output_tokens.to_string(), window, cx)
        });
        let code = serde_json::to_value(settings.reasoning_profile.mode)
            .unwrap()
            .as_str()
            .unwrap()
            .to_owned();
        self.mode.update(cx, |s, cx| {
            s.set_items(modes(protocol), window, cx);
            s.set_selected_value(&code, window, cx);
        });
        for (effort, input) in EFFORTS.into_iter().zip(&self.mapping) {
            let text = if protocol == UpstreamProtocol::OpenaiResponses {
                settings.responses_efforts.get(effort).cloned()
            } else {
                settings
                    .reasoning_profile
                    .budgets
                    .get(effort)
                    .map(u32::to_string)
                    .or_else(|| settings.reasoning_profile.levels.get(effort).cloned())
            }
            .unwrap_or_default();
            input.update(cx, |s, cx| s.set_value(text, window, cx));
        }
    }
    pub fn read(&self, cx: &App) -> Result<ProtocolSettings, String> {
        let code = self
            .mode
            .read(cx)
            .selected_value()
            .cloned()
            .unwrap_or_else(|| "none".into());
        let mut settings = parse(
            self.protocol,
            &self.limit.read(cx).value(),
            &code,
            EFFORTS
                .into_iter()
                .zip(&self.mapping)
                .map(|(key, input)| (key.to_owned(), input.read(cx).value().trim().to_owned()))
                .collect(),
        )?;
        settings.authentication = serde_json::from_value(serde_json::Value::String(
            self.authentication
                .read(cx)
                .selected_value()
                .cloned()
                .unwrap_or_else(|| "protocol_default".into()),
        ))
        .map_err(|_| "Invalid authentication")?;
        settings.validate(self.protocol)?;
        Ok(settings)
    }
    /// 12px muted field label, as in the design's protocol dialogs.
    fn field_label(text: String) -> Label {
        Label::new(text)
            .text_size(px(font::LABEL))
            .line_height(px(line_height::LABEL))
            .text_color(rgb(color::TEXT_3))
    }

    /// Renders the shared protocol editor (DESIGN_SPEC §5).
    ///
    /// One component serves all four protocols: only the leading fields differ
    /// (authentication / output limit + version / none), while the thinking
    /// mode, the mapping explanation and the monospace mapping column are
    /// shared. The Responses protocol keeps its explicit `none` mapping row.
    /// `mono` is the resolved monospace family from the active theme.
    pub fn render(&self, zh: bool, mono: SharedString) -> AnyElement {
        let t = |cn: &str, en: &str| if zh { cn.to_owned() } else { en.to_owned() };
        let openai = matches!(
            self.protocol,
            UpstreamProtocol::OpenaiChat | UpstreamProtocol::OpenaiResponses
        );
        let anthropic = self.protocol == UpstreamProtocol::AnthropicMessages;
        let authentication = t("API Key 认证", "API Key authentication");
        let limit = t("默认最大输出 Token", "Default maximum output tokens");
        let mode = t("思考模式", "Thinking mode");
        div()
            .id("protocol-options-fields")
            .flex()
            .flex_col()
            .gap(px(space::CARD_GAP))
            .overflow_y_scroll()
            .flex_1()
            .min_h_0()
            .children(openai.then(|| {
                div()
                    .flex()
                    .flex_col()
                    .gap(px(space::SMALL))
                    .child(Self::field_label(authentication.clone()))
                    .child(
                        crate::ui_components::select(&self.authentication, false)
                            .w_full()
                            .accessibility_label(authentication.clone()),
                    )
            }))
            .children(anthropic.then(|| {
                div()
                    .flex()
                    .flex_col()
                    .gap(px(space::SMALL))
                    .child(Self::field_label(limit.clone()))
                    .child(
                        crate::ui_components::text_input(&self.limit, false)
                            .w_full()
                            .aria_label(limit.clone()),
                    )
            }))
            .children(anthropic.then(|| {
                Label::new("Anthropic-Version: 2023-06-01")
                    .text_size(px(font::BODY))
                    .line_height(px(line_height::BODY))
                    .text_color(rgb(color::TEXT_2))
            }))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(space::SMALL))
                    .child(Self::field_label(mode.clone()))
                    .child(
                        crate::ui_components::select(&self.mode, false)
                            .w_full()
                            .accessibility_label(mode.clone()),
                    ),
            )
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_size(px(font::BODY))
                    .line_height(px(line_height::BODY))
                    .child(t("思考强度映射", "Thinking effort mapping")),
            )
            .child(
                div()
                    .p(px(space::CARD_SM))
                    .rounded(px(radius::CONTROL))
                    .bg(rgb(color::BG_PAGE))
                    .border_1()
                    .border_color(rgb(color::BORDER))
                    .child(
                        Label::new(t(
                            "按供应商文档填写映射，留空表示不配置。预算模式填正整数；档位模式填原生 level。Responses 留空沿用原生 effort，max 必须显式映射。",
                            "Use the provider's documented mapping. Budget: positive tokens; level: native level. Empty = unconfigured. Responses uses native efforts by default; max needs an explicit mapping.",
                        ))
                        .text_size(px(font::LABEL))
                        .line_height(px(line_height::LABEL))
                        .text_color(rgb(color::TEXT_2)),
                    ),
            )
            .children(
                EFFORTS
                    .into_iter()
                    .zip(&self.mapping)
                    .filter(|(effort, _)| {
                        *effort != "none" || self.protocol == UpstreamProtocol::OpenaiResponses
                    })
                    .map(|(effort, state)| {
                        div()
                            .flex()
                            .items_center()
                            .gap(px(space::CARD_GAP))
                            .child(
                                Label::new(effort)
                                    .w(px(size::MAPPING_LABEL_WIDTH))
                                    .flex_shrink_0()
                                    .font_family(mono.clone())
                                    .text_size(px(font::LABEL))
                                    .line_height(px(line_height::LABEL))
                                    .text_color(rgb(color::TEXT_2)),
                            )
                            .child(div().flex_1().child(
                                crate::ui_components::text_input(state, false)
                                    .aria_label(format!("{effort} mapping")),
                            ))
                    }),
            )
            .into_any_element()
    }
}
/// Anthropic shows the output-limit input first; the other protocols show the
/// thinking-mode select first.
fn initial_focus_uses_limit(protocol: UpstreamProtocol) -> bool {
    matches!(protocol, UpstreamProtocol::AnthropicMessages)
}

fn parse(
    protocol: UpstreamProtocol,
    limit: &str,
    mode: &str,
    values: BTreeMap<String, String>,
) -> Result<ProtocolSettings, String> {
    let mut settings = ProtocolSettings {
        default_max_output_tokens: limit
            .trim()
            .parse()
            .map_err(|_| "最大输出 Token 须为正整数 / Output limit must be positive")?,
        ..Default::default()
    };
    settings.reasoning_profile.mode =
        serde_json::from_value(serde_json::Value::String(mode.into()))
            .map_err(|_| "Invalid thinking mode")?;
    for (key, value) in values.into_iter().filter(|(_, v)| !v.is_empty()) {
        if protocol == UpstreamProtocol::OpenaiResponses {
            settings.responses_efforts.insert(key, value);
        } else {
            match settings.reasoning_profile.mode {
                ReasoningMode::AnthropicBudget | ReasoningMode::GeminiBudget => {
                    settings.reasoning_profile.budgets.insert(
                        key,
                        value
                            .parse()
                            .map_err(|_| "预算须为正整数 / Budget must be positive tokens")?,
                    );
                }
                ReasoningMode::AnthropicAdaptive | ReasoningMode::GeminiLevel => {
                    settings.reasoning_profile.levels.insert(key, value);
                }
                ReasoningMode::None => {
                    return Err(
                        "请选择思考模式或清空映射 / Select a thinking mode or clear the mappings"
                            .into(),
                    );
                }
            }
        }
    }
    settings.validate(protocol)?;
    Ok(settings)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[::core::prelude::v1::test]
    fn typed_profile_controls_preserve_explicit_budgets_and_reject_invalid_modes() {
        let values = BTreeMap::from([("high".into(), "1024".into())]);
        let s = parse(
            UpstreamProtocol::AnthropicMessages,
            "4096",
            "anthropic_budget",
            values.clone(),
        )
        .unwrap();
        assert_eq!(s.reasoning_profile.budgets["high"], 1024);
        assert!(
            parse(
                UpstreamProtocol::GeminiGenerateContent,
                "4096",
                "anthropic_budget",
                values.clone()
            )
            .is_err()
        );
        assert!(
            parse(
                UpstreamProtocol::AnthropicMessages,
                "0",
                "anthropic_budget",
                values.clone()
            )
            .is_err()
        );
        assert!(parse(UpstreamProtocol::AnthropicMessages, "4096", "none", values).is_err());
        let s = parse(
            UpstreamProtocol::OpenaiResponses,
            "4096",
            "none",
            BTreeMap::from([("max".into(), "xhigh".into())]),
        )
        .unwrap();
        assert_eq!(s.responses_efforts["max"], "xhigh");
    }

    #[::core::prelude::v1::test]
    fn modal_initial_focus_target_matches_rendered_protocol_controls() {
        assert!(initial_focus_uses_limit(
            UpstreamProtocol::AnthropicMessages
        ));
        for protocol in [
            UpstreamProtocol::OpenaiChat,
            UpstreamProtocol::OpenaiResponses,
            UpstreamProtocol::GeminiGenerateContent,
        ] {
            assert!(!initial_focus_uses_limit(protocol), "{protocol:?}");
        }
    }
}
