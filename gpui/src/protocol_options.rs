//! Typed native provider controls; all edits remain drafts until Save.
use crate::theme::{color, font, line_height};
use crate::upstream_config::{ProtocolSettings, ReasoningMode, UpstreamProtocol};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{
    component::{
        input::InputState,
        label::Label,
        scroll::{ScrollableElement, ScrollableMask},
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
#[derive(Clone)]
struct RawDraft {
    authentication: String,
    limit: String,
    mode: String,
    mapping: [String; 7],
}

pub struct Editor {
    drafts: BTreeMap<&'static str, RawDraft>,
    protocol: UpstreamProtocol,
    limit: Entity<InputState>,
    authentication: Entity<SelectState<Vec<Mode>>>,
    mode: Entity<SelectState<Vec<Mode>>>,
    mapping: [Entity<InputState>; 7],
}
impl Editor {
    pub fn new(window: &mut Window, cx: &mut App) -> Self {
        Self {
            drafts: BTreeMap::new(),
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
        self.drafts.clear();
        self.load_settings(protocol, settings, window, cx);
    }

    fn load_settings(
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
    pub fn protocol(&self) -> UpstreamProtocol {
        self.protocol
    }

    /// Keep raw input, including invalid intermediate text, when moving between
    /// protocol tabs. Only Apply parses and updates the provider draft.
    pub fn switch_to(&mut self, protocol: UpstreamProtocol, window: &mut Window, cx: &mut App) {
        if self.protocol == protocol {
            return;
        }
        let raw = RawDraft {
            authentication: self
                .authentication
                .read(cx)
                .selected_value()
                .cloned()
                .unwrap_or_else(|| "protocol_default".into()),
            limit: self.limit.read(cx).value().to_string(),
            mode: self
                .mode
                .read(cx)
                .selected_value()
                .cloned()
                .unwrap_or_else(|| "none".into()),
            mapping: std::array::from_fn(|i| self.mapping[i].read(cx).value().to_string()),
        };
        let azure = raw.authentication == "azure_api_key"
            && matches!(
                protocol,
                UpstreamProtocol::OpenaiChat | UpstreamProtocol::OpenaiResponses
            );
        self.drafts.insert(self.protocol.as_str(), raw);
        if let Some(raw) = self.drafts.get(protocol.as_str()).cloned() {
            self.protocol = protocol;
            self.authentication.update(cx, |s, cx| {
                s.set_selected_value(&raw.authentication, window, cx)
            });
            self.limit
                .update(cx, |s, cx| s.set_value(raw.limit, window, cx));
            self.mode.update(cx, |s, cx| {
                s.set_items(modes(protocol), window, cx);
                s.set_selected_value(&raw.mode, window, cx);
            });
            for (input, text) in self.mapping.iter().zip(raw.mapping) {
                input.update(cx, |s, cx| s.set_value(text, window, cx));
            }
        } else {
            let mut settings = ProtocolSettings::default();
            if azure {
                settings.authentication = crate::upstream_config::Authentication::AzureApiKey;
            }
            self.load_settings(protocol, &settings, window, cx);
        }
    }

    pub fn focus_targets(&self, cx: &App) -> Vec<(FocusHandle, usize)> {
        let mut targets = vec![
            (self.authentication.read(cx).focus_handle(cx), 0),
            (self.limit.read(cx).focus_handle(cx), 1),
            (self.mode.read(cx).focus_handle(cx), 2),
        ];
        targets.extend(
            self.mapping
                .iter()
                .enumerate()
                .map(|(i, input)| (input.read(cx).focus_handle(cx), i + 3)),
        );
        targets
    }

    pub fn focus_row(&self, control: usize) -> usize {
        let mode_row = usize::from(self.protocol != UpstreamProtocol::GeminiGenerateContent);
        match control {
            0 | 1 => 0,
            2 => mode_row,
            _ => {
                mode_row + 2 + control.saturating_sub(3)
                    - usize::from(self.protocol != UpstreamProtocol::OpenaiResponses)
            }
        }
    }

    fn field_label(text: String) -> Label {
        Label::new(text)
            .text_size(px(font::LABEL))
            .line_height(px(line_height::LABEL))
            .text_color(rgb(color::TEXT_3))
    }

    pub fn render(
        &self,
        zh: bool,
        mono: SharedString,
        scroll: &ScrollHandle,
        height: Pixels,
        needs_gutter: bool,
    ) -> AnyElement {
        let t = |cn: &str, en: &str| if zh { cn.to_owned() } else { en.to_owned() };
        let openai = matches!(
            self.protocol,
            UpstreamProtocol::OpenaiChat | UpstreamProtocol::OpenaiResponses
        );
        let anthropic = self.protocol == UpstreamProtocol::AnthropicMessages;
        let responses = self.protocol == UpstreamProtocol::OpenaiResponses;
        let gemini = self.protocol == UpstreamProtocol::GeminiGenerateContent;
        let field = |label: String, control: AnyElement| {
            div()
                .flex()
                .flex_col()
                .flex_none()
                .gap(px(3.))
                .child(Self::field_label(label))
                .child(control)
        };
        let input = |state: &Entity<InputState>, label: String, h: f32| {
            Styled::h(crate::ui_components::text_input(state, false), px(h))
                .text_size(px(12.))
                .rounded(px(9.))
                .bg(rgb(color::BG_TABLE_HEAD))
                .border_color(rgb(color::BORDER))
                .w_full()
                .aria_label(label)
                .into_any_element()
        };
        let select = |state: &Entity<SelectState<Vec<Mode>>>, label: String| {
            crate::ui_components::select(state, false)
                .w_full()
                .h(px(38.))
                .text_size(px(12.))
                .rounded(px(9.))
                .bg(rgb(color::BG_TABLE_HEAD))
                .border_color(rgb(color::BORDER))
                .accessibility_label(label)
                .into_any_element()
        };
        let auth_label = t("API Key 认证", "API Key authentication");
        let limit_label = t("默认最大输出 Token", "Default maximum output tokens");
        let mode_label = t("思考模式", "Thinking mode");
        let rows = div().id("protocol-options-fields").flex().flex_col().w_full()
            .when(needs_gutter, |rows| rows.pr(px(12.)))
            .pb(px(12.))
            .children(openai.then(|| {
                div().pt(px(18.)).pb(px(31.)).flex_none()
                    .child(field(auth_label.clone(), select(&self.authentication, auth_label)))
            }))
            .children(anthropic.then(|| {
                div().flex().flex_col().pt(px(16.)).pb(px(12.)).flex_none()
                    .child(field(limit_label.clone(), input(&self.limit, limit_label, 38.)))
                    .child(div().mt(px(10.)).h(px(35.)).flex_none().px(px(13.))
                        .flex().items_center().rounded(px(9.)).bg(rgb(color::BG_TABLE_HEAD))
                        .child(Label::new("Anthropic-Version: 2023-06-01")
                            .font_family(mono.clone()).text_size(px(12.)).text_color(rgb(color::TEXT_2))))
            }))
            .child(div().flex_none().pt(px(if gemini { 22. } else { 0. }))
                .pb(px(if gemini { 47. } else if anthropic { 31. } else { 26. }))
                .child(field(mode_label.clone(), select(&self.mode, mode_label))))
            .child(div().flex().flex_col().flex_none().gap(px(if responses { 4. } else { 5. }))
                .pb(px(if responses || gemini { 7. } else if anthropic { 5. } else { 6. }))
                .child(Label::new(t("思考强度映射", "Thinking effort mapping"))
                    .font_weight(FontWeight::BOLD).text_size(px(15.)).line_height(px(22.)))
                .child(div().flex().flex_col().flex_none().justify_center()
                    .min_h(px(if responses { 45. } else if gemini { 56. } else { 53. }))
                    .px(px(13.)).py(px(8.)).rounded(px(10.)).bg(rgb(color::BG_NOTICE))
                    .child(Label::new(t(
                        "留空表示不配置。预算模式填正整数；档位模式填写原生 level。",
                        "Empty = unconfigured. Budgets: positive tokens; levels: native values."))
                        .text_size(px(11.)).line_height(px(16.)).whitespace_normal().h_auto()
                        .text_color(rgb(color::TEXT_2)))
                    .child(Label::new(if responses {
                        t("Responses 留空沿用原生 effort，max 需显式映射。",
                            "Responses keeps native efforts when empty; max requires an explicit mapping.")
                    } else {
                        t("具体映射请遵循供应商文档，避免把 Token 预算误当成档位。",
                            "Follow the provider documentation; token budgets and levels are distinct.")
                    }).text_size(px(11.)).line_height(px(16.)).whitespace_normal().h_auto()
                        .text_color(rgb(color::TEXT_3)))))
            .children(EFFORTS.into_iter().zip(&self.mapping)
                .filter(|(effort, _)| *effort != "none" || responses)
                .map(|(effort, state)| {
                    let row_height = if responses || anthropic { 36. } else { 38. };
                    div().flex().items_center().gap(px(11.)).flex_none()
                        .mb(px(if responses { 0. } else if anthropic { 1. } else if gemini { 11. } else { 3. }))
                        .h(px(row_height))
                        .child(div().w(px(crate::theme::size::MAPPING_LABEL_WIDTH)).h(px(33.)).flex_shrink_0().px(px(12.))
                            .rounded(px(7.)).bg(rgb(color::BG_TABLE_HEAD)).flex().items_center()
                            .child(Label::new(effort).font_family(mono.clone()).text_size(px(12.))
                                .text_color(rgb(color::TEXT_2))))
                        .child(div().flex_1().min_w_0().child(input(state, format!("{effort} mapping"), row_height)))
                }));
        div()
            .relative()
            .w_full()
            .h(height)
            .flex_none()
            .child(rows.h(height).overflow_y_scroll().track_scroll(scroll))
            .child(ScrollableMask::new(Axis::Vertical, scroll))
            .vertical_scrollbar(scroll)
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
