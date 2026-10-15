//! Shared, serializable provider protocol settings (also usable by GPUI).
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, net::IpAddr};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum UpstreamProtocol {
    #[default]
    #[serde(rename = "openai_chat")]
    OpenaiChat,
    #[serde(rename = "openai_responses")]
    OpenaiResponses,
    #[serde(rename = "anthropic_messages")]
    AnthropicMessages,
    #[serde(rename = "gemini_generate_content")]
    GeminiGenerateContent,
}

impl UpstreamProtocol {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OpenaiChat => "openai_chat",
            Self::OpenaiResponses => "openai_responses",
            Self::AnthropicMessages => "anthropic_messages",
            Self::GeminiGenerateContent => "gemini_generate_content",
        }
    }

    pub fn is_native(self) -> bool {
        self != Self::OpenaiChat
    }

    /// Gateway prefixes survive; no absolute-path URL join is used.
    pub fn endpoint(self, base: &str, model: &str, stream: bool) -> Result<reqwest::Url, String> {
        let mut url = validate_base(base)?;
        let path = url.path().trim_end_matches('/');
        let suffix = match self {
            Self::OpenaiChat => "/chat/completions",
            Self::OpenaiResponses => "/responses",
            Self::AnthropicMessages => "/messages",
            Self::GeminiGenerateContent => "",
        };
        for known in ["/chat/completions", "/responses", "/messages"] {
            if path.ends_with(known) && known != suffix {
                return Err("Endpoint does not match the selected protocol".into());
            }
        }
        if path.contains(":generateContent") || path.contains(":streamGenerateContent") {
            return Err("Use an API base URL, not a Gemini generation endpoint".into());
        }
        if self == Self::GeminiGenerateContent {
            let model = gemini_model(model)?;
            let action = if stream {
                "streamGenerateContent"
            } else {
                "generateContent"
            };
            url.set_path(&format!("{path}/models/{model}:{action}"));
            if stream {
                url.set_query(Some("alt=sse"));
            }
        } else if !path.ends_with(suffix) {
            url.set_path(&format!("{path}{suffix}"));
        } else {
            url.set_path(path.to_owned().as_str());
        }
        Ok(url)
    }
}

fn validate_base(base: &str) -> Result<reqwest::Url, String> {
    if base.trim() != base || base.chars().any(char::is_control) {
        return Err("Invalid upstream URL".into());
    }
    let url = reqwest::Url::parse(base).map_err(|_| "Invalid upstream URL")?;
    let loopback = url.host_str().is_some_and(|host| {
        host.eq_ignore_ascii_case("localhost")
            || host
                .trim_matches(['[', ']'])
                .parse::<IpAddr>()
                .is_ok_and(|ip| ip.is_loopback())
    });
    if url.host_str().is_none() || !(url.scheme() == "https" || url.scheme() == "http" && loopback)
    {
        return Err("Upstream must use HTTPS (HTTP is allowed only on loopback)".into());
    }
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("URL must not contain credentials, query or fragment".into());
    }
    Ok(url)
}

pub fn gemini_model(model: &str) -> Result<&str, String> {
    let model = model.strip_prefix("models/").unwrap_or(model);
    if model.is_empty()
        || model.len() > 256
        || model.contains("..")
        || !model
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
    {
        return Err("Gemini model must be one nonempty model ID".into());
    }
    Ok(model)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningMode {
    #[default]
    None,
    AnthropicBudget,
    AnthropicAdaptive,
    GeminiBudget,
    GeminiLevel,
}

/// Explicit effort maps: no implicit token budgets or `max` coercion.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ReasoningProfile {
    pub mode: ReasoningMode,
    pub budgets: BTreeMap<String, u32>,
    /// For adaptive Anthropic and Gemini level modes, local effort -> native level.
    pub levels: BTreeMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ProtocolSettings {
    pub anthropic_version: String,
    pub default_max_output_tokens: u32,
    pub reasoning_profile: ReasoningProfile,
    /// Optional explicit Responses effort overrides, including local `max`.
    pub responses_efforts: BTreeMap<String, String>,
}

impl Default for ProtocolSettings {
    fn default() -> Self {
        Self {
            anthropic_version: "2023-06-01".into(),
            default_max_output_tokens: 4096,
            reasoning_profile: ReasoningProfile::default(),
            responses_efforts: BTreeMap::new(),
        }
    }
}

fn effort_key(s: &str) -> bool {
    matches!(
        s,
        "none" | "minimal" | "low" | "medium" | "high" | "xhigh" | "max"
    )
}

impl ProtocolSettings {
    pub fn validate(&self, protocol: UpstreamProtocol) -> Result<(), String> {
        if self.default_max_output_tokens == 0 {
            return Err("Invalid default output token limit".into());
        }
        // Header-safe version; support reviewed API versions explicitly.
        if self.anthropic_version != "2023-06-01" {
            return Err("Unsupported Anthropic version".into());
        }
        let profile = &self.reasoning_profile;
        let budget_mode = matches!(
            profile.mode,
            ReasoningMode::AnthropicBudget | ReasoningMode::GeminiBudget
        );
        let level_mode = matches!(
            profile.mode,
            ReasoningMode::AnthropicAdaptive | ReasoningMode::GeminiLevel
        );
        let matches_protocol = match profile.mode {
            ReasoningMode::None => true,
            ReasoningMode::AnthropicBudget | ReasoningMode::AnthropicAdaptive => {
                protocol == UpstreamProtocol::AnthropicMessages
            }
            ReasoningMode::GeminiBudget | ReasoningMode::GeminiLevel => {
                protocol == UpstreamProtocol::GeminiGenerateContent
            }
        };
        if !matches_protocol
            || !budget_mode && !profile.budgets.is_empty()
            || !level_mode && !profile.levels.is_empty()
        {
            return Err("Reasoning profile does not match protocol/mode".into());
        }
        for (effort, budget) in &profile.budgets {
            if !effort_key(effort) || *budget == 0 {
                return Err("Invalid explicit reasoning budget".into());
            }
            if profile.mode == ReasoningMode::AnthropicBudget && *budget < 1024 {
                return Err("Anthropic thinking budget must be at least 1024".into());
            }
        }
        for (effort, level) in &profile.levels {
            let valid = match profile.mode {
                ReasoningMode::AnthropicAdaptive => {
                    matches!(level.as_str(), "low" | "medium" | "high" | "max")
                }
                ReasoningMode::GeminiLevel => {
                    matches!(level.as_str(), "minimal" | "low" | "medium" | "high")
                }
                _ => false,
            };
            if !effort_key(effort) || !valid {
                return Err("Invalid explicit reasoning level".into());
            }
        }
        if !self.responses_efforts.is_empty() && protocol != UpstreamProtocol::OpenaiResponses {
            return Err("Responses effort overrides require Responses protocol".into());
        }
        for (effort, native) in &self.responses_efforts {
            if !effort_key(effort)
                || !matches!(
                    native.as_str(),
                    "none" | "minimal" | "low" | "medium" | "high" | "xhigh"
                )
            {
                return Err("Invalid Responses effort override".into());
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn protocol_values_are_explicit_and_settings_are_closed() {
        assert!(serde_json::from_str::<UpstreamProtocol>("null").is_err());
        assert!(serde_json::from_str::<UpstreamProtocol>("\"automatic\"").is_err());
        assert!(serde_json::from_str::<ProtocolSettings>(r#"{"headers":{}}"#).is_err());
        assert!(
            serde_json::from_str::<ProtocolSettings>(r#"{"reasoning_profile":{"unknown":1}}"#)
                .is_err()
        );
        assert_eq!(
            serde_json::from_str::<ProtocolSettings>("{}").unwrap(),
            ProtocolSettings::default()
        );
    }
    #[test]
    fn endpoints_preserve_gateway_prefix_and_own_the_query() {
        for (protocol, suffix) in [
            (UpstreamProtocol::OpenaiChat, "chat/completions"),
            (UpstreamProtocol::OpenaiResponses, "responses"),
            (UpstreamProtocol::AnthropicMessages, "messages"),
        ] {
            let base = "https://example.test:8443/gateway/v1";
            assert_eq!(
                protocol.endpoint(base, "m", true).unwrap().as_str(),
                format!("{base}/{suffix}")
            );
            assert_eq!(
                protocol
                    .endpoint(&format!("{base}/{suffix}/"), "m", false)
                    .unwrap()
                    .as_str(),
                format!("{base}/{suffix}")
            );
        }
        let protocol = UpstreamProtocol::GeminiGenerateContent;
        assert_eq!(
            protocol
                .endpoint(
                    "https://example.test/p/v1beta/",
                    "models/gemini-test.1",
                    true
                )
                .unwrap()
                .as_str(),
            "https://example.test/p/v1beta/models/gemini-test.1:streamGenerateContent?alt=sse"
        );
    }
    #[test]
    fn incompatible_or_unsafe_urls_fail_without_guessing() {
        for base in ["https://x/v1/chat/completions", "https://x/v1/messages"] {
            assert!(
                UpstreamProtocol::OpenaiResponses
                    .endpoint(base, "m", false)
                    .is_err()
            );
        }
        for base in [
            "http://example.test/v1",
            "https://secret@x/v1",
            "https://x/v1?key=secret",
            "https://x/v1#frag",
            " https://x/v1",
            "https://x/a\nb",
        ] {
            assert!(
                UpstreamProtocol::OpenaiResponses
                    .endpoint(base, "m", false)
                    .is_err()
            );
        }
        for base in [
            "http://localhost:1234/v1",
            "http://127.0.0.1:1/v1",
            "http://[::1]:1234/v1",
        ] {
            assert!(
                UpstreamProtocol::OpenaiResponses
                    .endpoint(base, "m", false)
                    .is_ok()
            );
        }
        for model in [
            "models/models/m",
            "../a",
            "a/b",
            "a%2Fb",
            "a%252Fb",
            "a?x",
            "a#x",
            "",
        ] {
            assert!(gemini_model(model).is_err());
        }
    }
    #[test]
    fn thinking_settings_never_invent_budgets_or_accept_wrong_modes() {
        let mut settings = ProtocolSettings::default();
        settings.reasoning_profile.mode = ReasoningMode::AnthropicBudget;
        settings
            .reasoning_profile
            .budgets
            .insert("high".into(), 4096);
        assert!(
            settings
                .validate(UpstreamProtocol::AnthropicMessages)
                .is_ok()
        );
        assert!(
            settings
                .validate(UpstreamProtocol::GeminiGenerateContent)
                .is_err()
        );
        settings.reasoning_profile.budgets.insert("low".into(), 1);
        assert!(
            settings
                .validate(UpstreamProtocol::AnthropicMessages)
                .is_err()
        );
    }
}
