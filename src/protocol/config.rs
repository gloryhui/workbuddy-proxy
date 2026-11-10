//! 可序列化的供应商协议设置；core 路由、模型发现与 GPUI 使用同一份源码定义。
//!
//! 在 core crate 内，它在 `main.rs` 中通过
//! `#[path = "protocol/config.rs"]` 被编译为根级 `crate::upstream_config`；
//! `protocol::config` 只是同一模块的 `pub use` 别名。因此这里定义的
//! [`UpstreamProtocol`]、[`ProtocolSettings`] 等类型在 core 的两条符号路径下仍是
//! 同一类型。GPUI 是独立 crate/进程，会通过自己的 `#[path]` 编译这份源码并持有
//! 独立的内存值；相同 Rust 类型名不代表跨进程共享状态。
//!
//! 模块职责是描述协议选择、端点路径、认证方式和思考参数，并做无副作用的配置校验。
//! 它不保存凭据、不发送网络请求，也不选择具体模型路由；调用方把持久化配置复制成
//! `proxy::Route` 后才在请求时使用。纯同步函数不创建线程，返回的 URL 只是候选值，
//! 真正的 HTTP 发送仍在 `proxy::native::open` 或普通 Chat 的 `Bridge::upstream`。
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, net::IpAddr};

/// 上游请求与响应的 wire protocol。
///
/// `OpenaiChat` 保持原始 Chat Completions 透传；另外三个值会让
/// `proxy::relay_to` 进入 native 转换路径。序列化字符串由 `serde(rename)` 固定，
/// 不支持 `"automatic"`、大小写变体或空值，避免配置拼写被静默猜测成某个协议。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum UpstreamProtocol {
    /// OpenAI Chat Completions；不调用 `protocol::encode`。
    #[default]
    #[serde(rename = "openai_chat")]
    OpenaiChat,
    /// OpenAI Responses API，需要把统一 Chat 历史转换为 Responses input。
    #[serde(rename = "openai_responses")]
    OpenaiResponses,
    /// Anthropic Messages API，system/developer、工具结果和思考字段需转换。
    #[serde(rename = "anthropic_messages")]
    AnthropicMessages,
    /// Gemini Generate Content API，包括流式的 `streamGenerateContent` 路径。
    #[serde(rename = "gemini_generate_content")]
    GeminiGenerateContent,
}

impl UpstreamProtocol {
    /// 返回持久化/统计使用的稳定 wire 名称；不会返回人类可读标题。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OpenaiChat => "openai_chat",
            Self::OpenaiResponses => "openai_responses",
            Self::AnthropicMessages => "anthropic_messages",
            Self::GeminiGenerateContent => "gemini_generate_content",
        }
    }

    /// 是否必须经过 native 请求与响应适配。
    ///
    /// 只有 OpenAI Chat Completions 可以直接代理；该判断描述消息协议，不描述供应商
    /// 身份，也不判断某个上游是否真的兼容 Chat。
    pub fn is_native(self) -> bool {
        self != Self::OpenaiChat
    }

    /// 根据推理端点和相同认证上下文推导模型列表端点。
    ///
    /// 它接受的 base/endpoint 规则与 [`Self::endpoint`] 相同：前三种协议可使用 API
    /// base，也可使用已经拼好的同协议推理端点；后者先由 `endpoint` 验证，再删除对应
    /// 生成后缀并追加 `/models`。Gemini 仍要求 API base 而不是已拼好的生成端点。
    /// 因此网关前缀会保留，返回 URL 不会发起请求；另一协议的完整后缀会先被
    /// `endpoint` 拒绝。
    pub fn models_endpoint(self, base: &str) -> Result<reqwest::Url, String> {
        // Reuse inference validation, including mismatched full endpoints.
        let mut url = self.endpoint(base, "model", false)?;
        let path = url.path().to_owned();
        let suffix = match self {
            Self::OpenaiChat => "/chat/completions",
            Self::OpenaiResponses => "/responses",
            Self::AnthropicMessages => "/messages",
            Self::GeminiGenerateContent => "/models/model:generateContent",
        };
        let prefix = path
            .strip_suffix(suffix)
            .ok_or("Invalid model list endpoint")?;
        url.set_path(&format!("{prefix}/models"));
        url.set_query(None);
        Ok(url)
    }

    /// 为某个模型和流式模式构造最终推理 URL。
    ///
    /// OpenAI Chat、Responses、Anthropic 分别追加 `/chat/completions`、
    /// `/responses`、`/messages`；如果 base 已经以同一后缀结尾，只去掉额外斜杠，
    /// 不重复追加。Gemini 使用 `models/{model}:generateContent`，流式时改为
    /// `streamGenerateContent` 并设置 `alt=sse`。
    ///
    /// URL 的既有 gateway path 会保留。如果 base 以另一协议后缀结尾，或已经包含
    /// Gemini 生成 action，函数明确失败，避免把一次请求发到看似相近但错误的端点。
    /// 该函数只构造 URL；认证 header 由后续调用方按 [`Authentication`] 添加。
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

/// 校验上游 base URL 的传输与边界规则。
///
/// 只接受 HTTPS；HTTP 仅为本机回环地址（localhost、IPv4 loopback、IPv6 loopback）
/// 保留。前后空白、控制字符、URL 内嵌用户名/密码、query 和 fragment 都会失败，
/// 因为 query 由协议代码独占管理，凭据也不应藏在 URL 中。
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

/// 规范化并约束 Gemini 路径中的模型段。
///
/// 允许调用方带一次 `models/` 前缀，返回时移除该前缀。模型段必须非空、按字节不超过
/// 256，只能包含 ASCII 字母数字及 `-_.`，并拒绝任意位置的 `..`。该检查保证模型名
/// 不会注入额外路径、query 或 fragment；它不验证上游是否真的提供此模型。
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

/// 把统一 `reasoning_effort` 映射到某家 native 协议字段的策略。
///
/// `None` 表示不配置思考映射；客户端显式请求非 `none` 努力时，编码阶段会因没有对应
/// 配置而失败，而不是自行编造预算。`AnthropicBudget`/`GeminiBudget` 使用显式
/// token 预算，`AnthropicAdaptive`/`GeminiLevel` 使用供应商档位。
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

/// 一组显式的统一努力到 native 思考参数映射。
///
/// 只允许配置了映射的 effort 被发送；不会为缺失档位推算 token 预算，也不会把本地
/// `max` 自动改写成供应商的 `xhigh`。`budgets` 的值是 token 数，`levels` 的值是
/// 供应商档位字符串；具体合法集合由 [`ProtocolSettings::validate`] 按模式检查。
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ReasoningProfile {
    /// 预算或档位映射语义；决定另外两个映射字段是否被允许使用。
    pub mode: ReasoningMode,
    /// 统一 effort -> native token 预算。键和值是否合法在配置校验时决定。
    pub budgets: BTreeMap<String, u32>,
    /// For adaptive Anthropic and Gemini level modes, local effort -> native level.
    pub levels: BTreeMap<String, String>,
}

/// provider 凭据发送方式。
///
/// 默认按协议惯例发送；`AzureApiKey` 是显式选择，不能仅凭 Azure 主机名自动推断。
/// Azure 模式只允许 OpenAI Chat/Responses，因为其他 native 协议并不使用 `api-key`
/// 与 Azure v1 base 的组合。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Authentication {
    #[default]
    ProtocolDefault,
    AzureApiKey,
}

/// 与某个 [`UpstreamProtocol`] 一起持久化的协议级设置。
///
/// `default` 由 `serde(default)` 为旧配置补齐顶层字段，同时 `deny_unknown_fields`
/// 拒绝拼错或未识别的键。该结构可被多处克隆进路由快照，但本身没有共享可变状态。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ProtocolSettings {
    /// 凭据 header 策略；默认值不是 Azure。
    pub authentication: Authentication,
    /// Anthropic 请求所需的版本 header，当前仅接受已审核的固定版本。
    pub anthropic_version: String,
    /// 客户端未显式给出输出上限时，Anthropic 编码可使用的 token 上限。
    ///
    /// 当前只有 Anthropic 请求会以此作为 `max_tokens` 默认值；Responses 和 Gemini
    /// 在客户端未指定时不会因此凭空增加原生上限字段。
    pub default_max_output_tokens: u32,
    /// Anthropic/Gemini 的显式思考预算或档位映射。
    pub reasoning_profile: ReasoningProfile,
    /// Responses effort 的显式覆盖，包括本地 `max` 到 native `xhigh` 的情况。
    ///
    /// 未配置覆盖时本地 effort 原样发送；如果最终仍是 native `max`，编码会拒绝，
    /// 因为当前 Responses 请求转换不把它当作可发送值。
    pub responses_efforts: BTreeMap<String, String>,
}

impl Default for ProtocolSettings {
    fn default() -> Self {
        Self {
            authentication: Authentication::default(),
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
    /// 是否显式选择 Azure `api-key` 认证；不根据 URL 主机自动判断。
    pub fn is_azure(&self) -> bool {
        self.authentication == Authentication::AzureApiKey
    }

    /// 把持久化 base 归一为本次请求使用的 API base。
    ///
    /// 普通认证保留调用方给出的 URL path。Azure v1 模式下，资源根或 `/openai` 会
    /// 规范为 `/openai/v1`；已经带 gateway 前缀的 `/openai/v1` 原样保留。显式的
    /// `/deployments/...` 端点被拒绝，因为当前实现把模型名放在请求 body 中，而不是
    /// 把它拼进 Azure deployment URL。
    ///
    /// 输入仍是未认证配置值，输出是字符串而非已发送请求。认证模式必须显式配置；
    /// 仅看到 Azure 主机名不会改变 header 策略。
    pub fn api_base(&self, protocol: UpstreamProtocol, base: &str) -> Result<String, String> {
        self.validate(protocol)?;
        let mut url = validate_base(base)?;
        if self.is_azure() {
            let path = url.path().trim_end_matches('/');
            match path {
                "" => url.set_path("/openai/v1"),
                "/openai" => url.set_path("/openai/v1"),
                _ if path.contains("/deployments/") => {
                    return Err("Use the Azure resource endpoint or its /openai/v1 base URL".into());
                }
                _ => {}
            }
        }
        Ok(url.to_string().trim_end_matches('/').to_owned())
    }

    /// 在完成设置校验和 API base 归一后构造最终推理 URL。
    ///
    /// 失败不会产生半配置状态；成功也只是返回可交给 HTTP client 的 URL。
    pub fn endpoint(
        &self,
        protocol: UpstreamProtocol,
        base: &str,
        model: &str,
        stream: bool,
    ) -> Result<reqwest::Url, String> {
        protocol.endpoint(&self.api_base(protocol, base)?, model, stream)
    }

    /// 在相同设置和认证模式下构造模型列表 URL。
    pub fn models_endpoint(
        &self,
        protocol: UpstreamProtocol,
        base: &str,
    ) -> Result<reqwest::Url, String> {
        protocol.models_endpoint(&self.api_base(protocol, base)?)
    }

    /// 校验认证、Anthropic 版本、输出默认值和思考映射的一致性。
    ///
    /// 这里检查的是配置是否可解释，而不是模型能力。例如 Anthropic 预算至少为 1024
    /// token、Gemini/Anthropic 的映射不能串用、未启用的映射容器不得含值、Responses
    /// override 的键和值必须位于当前白名单。模型是否支持图片、工具或某个 effort，
    /// 由 `proxy::native::prepare` 结合目录能力另行判断。
    ///
    /// 函数无副作用，失败返回可直接用于配置界面的静态错误文本。
    pub fn validate(&self, protocol: UpstreamProtocol) -> Result<(), String> {
        if self.is_azure()
            && !matches!(
                protocol,
                UpstreamProtocol::OpenaiChat | UpstreamProtocol::OpenaiResponses
            )
        {
            return Err("Azure API Key requires OpenAI Chat or Responses".into());
        }
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
            if !effort_key(effort) || effort == "none" || *budget == 0 {
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
            if !effort_key(effort) || effort == "none" || !valid {
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
    fn azure_key_endpoints_are_explicit_and_legacy_settings_keep_their_defaults() {
        let settings = ProtocolSettings {
            authentication: Authentication::AzureApiKey,
            ..Default::default()
        };
        for base in [
            "https://resource.openai.azure.com",
            "https://resource.openai.azure.com/",
            "https://resource.openai.azure.com/openai",
            "https://resource.openai.azure.com/openai/v1/",
        ] {
            for (protocol, suffix) in [
                (UpstreamProtocol::OpenaiChat, "chat/completions"),
                (UpstreamProtocol::OpenaiResponses, "responses"),
            ] {
                assert_eq!(
                    settings
                        .endpoint(protocol, base, "deployment-name", true)
                        .unwrap()
                        .as_str(),
                    format!("https://resource.openai.azure.com/openai/v1/{suffix}")
                );
                assert_eq!(
                    settings.models_endpoint(protocol, base).unwrap().as_str(),
                    "https://resource.openai.azure.com/openai/v1/models"
                );
            }
        }
        assert_eq!(
            settings
                .endpoint(
                    UpstreamProtocol::OpenaiResponses,
                    "https://gateway.test/custom/openai/v1",
                    "deployment",
                    false
                )
                .unwrap()
                .as_str(),
            "https://gateway.test/custom/openai/v1/responses"
        );
        for protocol in [
            UpstreamProtocol::AnthropicMessages,
            UpstreamProtocol::GeminiGenerateContent,
        ] {
            assert!(settings.validate(protocol).is_err());
        }
        for base in [
            "",
            "https://<resource>.openai.azure.com",
            "https://resource.openai.azure.com/openai/deployments/name",
            "https://resource.openai.azure.com?api-version=old",
        ] {
            assert!(
                settings
                    .endpoint(UpstreamProtocol::OpenaiChat, base, "deployment", false)
                    .is_err()
            );
        }
        assert_eq!(
            ProtocolSettings::default()
                .endpoint(
                    UpstreamProtocol::OpenaiChat,
                    "https://resource.openai.azure.com",
                    "model",
                    false
                )
                .unwrap()
                .path(),
            "/chat/completions"
        );
        assert_eq!(
            serde_json::from_str::<ProtocolSettings>("{}")
                .unwrap()
                .authentication,
            Authentication::ProtocolDefault
        );
    }
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
