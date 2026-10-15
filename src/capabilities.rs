//! WorkBuddy model metadata. These declarations never rewrite media or prompts.
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub const EFFORTS: [&str; 5] = ["low", "medium", "high", "xhigh", "max"];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct ModelCapabilities {
    pub supports_tool_call: bool,
    pub supports_images: bool,
    pub supports_reasoning: bool,
    pub only_reasoning: bool,
    pub use_custom_protocol: bool,
    pub reasoning: Reasoning,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_input_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_window: Option<ContextWindow>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct Reasoning {
    pub default_effort: String,
    pub supported_efforts: Vec<String>,
    pub can_disable_thinking: bool,
}
impl Default for Reasoning {
    fn default() -> Self {
        Self {
            default_effort: "high".into(),
            supported_efforts: vec!["high".into(), "max".into()],
            can_disable_thinking: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ContextWindow {
    pub default_length: u32,
    pub supported_lengths: Vec<u32>,
}

impl Default for ModelCapabilities {
    fn default() -> Self {
        Self {
            supports_tool_call: true,
            supports_images: false,
            supports_reasoning: false,
            only_reasoning: false,
            use_custom_protocol: false,
            reasoning: Reasoning::default(),
            max_input_tokens: None,
            max_output_tokens: None,
            context_window: None,
        }
    }
}
impl ModelCapabilities {
    pub fn for_model(model: &str) -> Self {
        let mut value = Self::default();
        let id = model
            .trim()
            .rsplit('/')
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        if matches!(id.as_str(), "deepseek-v4.1-flash" | "deepseek-flash") {
            value.supports_images = true;
            value.reasoning.supported_efforts = vec!["low".into(), "high".into(), "max".into()];
        }
        if matches!(
            id.as_str(),
            "deepseek-v4.1-flash" | "deepseek-flash" | "deepseek-v4-pro" | "deepseek-v4-flash"
        ) {
            value.supports_reasoning = true;
            value.max_input_tokens = Some(1_000_000);
            value.context_window = Some(ContextWindow {
                default_length: 300_000,
                supported_lengths: vec![128_000, 300_000, 1_000_000],
            });
        }
        value
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.only_reasoning && (!self.supports_reasoning || self.reasoning.can_disable_thinking)
        {
            return Err("仅思考模式需要开启思考且不允许关闭 / Reasoning-only models cannot disable thinking".into());
        }
        let efforts = &self.reasoning.supported_efforts;
        let unique = efforts.iter().collect::<std::collections::HashSet<_>>();
        if unique.len() != efforts.len()
            || efforts.iter().any(|s| !EFFORTS.contains(&s.as_str()))
            || !EFFORTS.contains(&self.reasoning.default_effort.as_str())
            || (self.supports_reasoning && !efforts.contains(&self.reasoning.default_effort))
        {
            return Err("默认思考强度须在支持的档位中，档位不可重复 / Select a supported default thinking effort".into());
        }
        if self.max_input_tokens == Some(0) || self.max_output_tokens == Some(0) {
            return Err(
                "输入和输出上限须为正整数，留空使用默认值 / Token limits must be positive or empty"
                    .into(),
            );
        }
        if let Some(window) = &self.context_window {
            let lengths = &window.supported_lengths;
            if window.default_length == 0
                || lengths.len() > 16
                || !lengths.contains(&window.default_length)
                || lengths
                    .iter()
                    .collect::<std::collections::HashSet<_>>()
                    .len()
                    != lengths.len()
                || lengths
                    .iter()
                    .any(|n| *n == 0 || self.max_input_tokens.is_some_and(|max| *n > max))
            {
                return Err("上下文长度须为正整数且不超过输入上限，默认长度须在可选列表中 / Invalid context window choices".into());
            }
        }
        Ok(())
    }

    pub fn apply(&self, entry: &mut Value) {
        let mut fields = serde_json::to_value(self).unwrap();
        if !self.supports_reasoning {
            fields.as_object_mut().unwrap().remove("reasoning");
        }
        entry
            .as_object_mut()
            .unwrap()
            .extend(fields.as_object().unwrap().clone());
        if self.use_custom_protocol {
            let base = entry["url"].as_str().unwrap().trim_end_matches('/');
            entry["url"] = json!(format!("{base}/chat/completions"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn known_vision_defaults_and_explicit_metadata_are_separate() {
        let auto = ModelCapabilities::for_model("deepseek-ai/DeepSeek-V4.1-Flash");
        assert!(auto.supports_images && auto.supports_reasoning);
        auto.validate().unwrap();
        assert!(!ModelCapabilities::for_model("unknown-model").supports_images);
        let mut custom = auto.clone();
        custom.supports_images = false;
        custom.use_custom_protocol = true;
        custom.max_output_tokens = Some(8192);
        let mut entry = json!({"url":"http://127.0.0.1:18080/r/route/v1"});
        custom.apply(&mut entry);
        assert_eq!(entry["supportsImages"], false);
        assert_eq!(entry["maxOutputTokens"], 8192);
        assert_eq!(
            entry["url"],
            "http://127.0.0.1:18080/r/route/v1/chat/completions"
        );
        assert_eq!(
            serde_json::from_value::<ModelCapabilities>(serde_json::to_value(&custom).unwrap())
                .unwrap(),
            custom
        );
    }
    #[test]
    fn invalid_capability_combinations_are_rejected() {
        let mut value = ModelCapabilities {
            only_reasoning: true,
            ..Default::default()
        };
        assert!(value.validate().is_err());
        value.supports_reasoning = true;
        value.reasoning.can_disable_thinking = false;
        value.validate().unwrap();
        value.reasoning.default_effort = "low".into();
        assert!(value.validate().is_err());
        value.reasoning.supported_efforts.push("low".into());
        value.validate().unwrap();
        value.context_window = Some(ContextWindow {
            default_length: 300_000,
            supported_lengths: vec![128_000],
        });
        assert!(value.validate().is_err());
    }
}
