//! Read-only first-provider discovery. No proxy instance or config write is needed.
//! The hint decoder and native effort normalization mirror src/catalog.rs; keep
//! their wire semantics covered by the core/GUI parity fixtures when changing either.
use crate::capabilities::{ContextWindow, EFFORTS, ModelCapabilities};
use crate::upstream_config::{ProtocolSettings, ReasoningMode, UpstreamProtocol};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Hints {
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    images: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    efforts: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    can_disable: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    input: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    output: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    context: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    thinking_modes: Option<Vec<String>>,
}

fn positive(v: Option<&Value>) -> Option<u32> {
    v.and_then(Value::as_u64)
        .and_then(|n| u32::try_from(n).ok())
        .filter(|n| *n > 0)
}

impl Hints {
    fn parse(v: &Value) -> Self {
        let modalities = v
            .pointer("/modalities/input")
            .or_else(|| v.get("input_modalities"))
            .or_else(|| v.get("inputModalities"))
            .or_else(|| v.pointer("/architecture/input_modalities"));
        let images = v
            .get("supportsImages")
            .or_else(|| v.get("supports_images"))
            .or_else(|| v.get("supports_image"))
            .and_then(Value::as_bool)
            .or_else(|| {
                let list = modalities?.as_array()?;
                if list.is_empty() || list.iter().any(|x| x.as_str().is_none()) {
                    return None;
                }
                Some(
                    list.iter()
                        .any(|x| x.as_str().is_some_and(|s| s.eq_ignore_ascii_case("image"))),
                )
            });
        let options = v.get("reasoning_options").and_then(Value::as_array);
        let levels = v
            .get("supported_reasoning_levels")
            .or_else(|| v.pointer("/reasoning/supportedEfforts"))
            .or_else(|| {
                options?
                    .iter()
                    .find(|x| x["type"] == "effort")?
                    .get("values")
            });
        let efforts = levels.and_then(Value::as_array).and_then(|levels| {
            if levels.iter().any(|v| v.as_str().is_none()) {
                return None;
            }
            // Keep only levels WorkBuddy understands; do not advertise a
            // numeric effort/budget as if it were an OpenAI effort parameter.
            Some(
                EFFORTS
                    .iter()
                    .filter(|e| levels.iter().any(|v| v.as_str() == Some(**e)))
                    .map(|s| (*s).to_owned())
                    .collect(),
            )
        });
        let can_disable = v
            .pointer("/reasoning/canDisableThinking")
            .and_then(Value::as_bool)
            .or_else(|| {
                levels
                    .and_then(Value::as_array)
                    .map(|items| items.iter().any(|v| v == "none"))
            });
        Self {
            tools: v
                .get("tool_call")
                .or_else(|| v.get("supportsToolCall"))
                .or_else(|| v.get("supports_tool_call"))
                .and_then(Value::as_bool),
            images,
            reasoning: v
                .get("reasoning")
                .and_then(Value::as_bool)
                .or_else(|| v.get("supportsReasoning").and_then(Value::as_bool)),
            efforts,
            can_disable,
            input: positive(v.pointer("/limit/input"))
                .or_else(|| positive(v.get("max_input_tokens")))
                .or_else(|| positive(v.get("maxInputTokens"))),
            output: positive(v.pointer("/limit/output"))
                .or_else(|| positive(v.get("max_output_tokens")))
                .or_else(|| positive(v.get("maxOutputTokens"))),
            thinking_modes: None,
            context: positive(v.pointer("/limit/context"))
                .or_else(|| positive(v.get("context_length"))),
        }
    }

    pub fn native(v: &Value, protocol: UpstreamProtocol) -> Self {
        let mut hints = Self::parse(v);
        if protocol == UpstreamProtocol::AnthropicMessages {
            hints.images = v
                .pointer("/capabilities/image_input/supported")
                .and_then(Value::as_bool);
            hints.reasoning = v
                .pointer("/capabilities/thinking/supported")
                .and_then(Value::as_bool);
            hints.can_disable = v
                .pointer("/capabilities/thinking/types/disabled/supported")
                .and_then(Value::as_bool);
            hints.output = positive(v.get("max_tokens"));
            hints.thinking_modes = v
                .pointer("/capabilities/thinking/types")
                .and_then(Value::as_object)
                .map(|types| {
                    ["enabled", "adaptive"]
                        .into_iter()
                        .filter(|k| {
                            types.get(*k).and_then(|v| v["supported"].as_bool()) == Some(true)
                        })
                        .map(str::to_owned)
                        .collect()
                });
            hints.efforts = v
                .pointer("/capabilities/effort")
                .and_then(Value::as_object)
                .map(|levels| {
                    EFFORTS
                        .iter()
                        .filter(|e| {
                            levels.get(**e).and_then(|v| v["supported"].as_bool()) == Some(true)
                        })
                        .map(|s| (*s).to_owned())
                        .collect()
                });
        } else if protocol == UpstreamProtocol::GeminiGenerateContent {
            hints.input = positive(v.get("inputTokenLimit"));
            hints.output = positive(v.get("outputTokenLimit"));
            hints.reasoning = v.get("thinking").and_then(Value::as_bool);
        }
        hints
    }

    fn apply(&self, value: &mut ModelCapabilities) {
        if let Some(v) = self.tools {
            value.supports_tool_call = v;
        }
        if let Some(v) = self.images {
            value.supports_images = v;
        }
        if self.reasoning == Some(false) {
            value.supports_reasoning = false;
        }
        if let Some(levels) = &self.efforts {
            value.supports_reasoning = self.reasoning != Some(false) && !levels.is_empty();
            if !levels.is_empty() {
                value.reasoning.supported_efforts = levels.clone();
                if !levels.contains(&value.reasoning.default_effort) {
                    value.reasoning.default_effort = levels.last().unwrap().clone();
                }
            }
        }
        // A bare reasoning=true does not tell us which selector values the
        // provider accepts. As in Magpie, only advertise known effort choices.
        if let Some(v) = self.can_disable {
            value.reasoning.can_disable_thinking = v;
        }
        let input = match (self.input, self.context) {
            (Some(input), Some(context)) => Some(input.min(context)),
            (input, context) => input.or(context),
        };
        if let Some(v) = input {
            value.max_input_tokens = Some(v);
        }
        if let Some(v) = self.output {
            value.max_output_tokens = Some(v);
        }
        if let Some(v) = self.context.or(self.input) {
            let length = v.min(value.max_input_tokens.unwrap_or(v));
            value.context_window = Some(ContextWindow {
                default_length: length,
                supported_lengths: vec![length],
            });
        }
        value.only_reasoning = false;
    }
}

#[derive(Clone, Deserialize, PartialEq)]
pub struct Request {
    pub provider_id: String,
    pub upstream: String,
    pub key: String,
    pub protocol: UpstreamProtocol,
    pub protocol_settings: ProtocolSettings,
}
#[derive(Deserialize, Serialize)]
struct Resolved {
    capabilities: ModelCapabilities,
    sources: Vec<String>,
    image_known: bool,
    reasoning_known: bool,
    updated_at: Option<u64>,
}
pub struct Cache {
    request: Request,
    models: BTreeMap<String, Hints>,
    fetched_at: u64,
}
impl Cache {
    pub fn matches(&self, payload: &Value) -> bool {
        serde_json::from_value::<Request>(payload.clone()).is_ok_and(|mut query| {
            // Core live metadata is scoped by endpoint/credential/wire, not
            // supplier ID. An independent copy with the same wire may reuse it.
            query.provider_id = self.request.provider_id.clone();
            query == self.request
        })
    }
    pub fn rebind(&mut self, old: &str, new: &str) {
        if self.request.provider_id == old {
            self.request.provider_id = new.into();
        }
    }
    pub fn enrich(&self, payload: &Value, metadata: &mut Value) -> Result<(), String> {
        let model = payload["model"].as_str().unwrap_or("");
        let Some(hints) = self.models.get(model).or_else(|| {
            self.models
                .get(model.strip_prefix("models/").unwrap_or(model))
        }) else {
            return Ok(());
        };
        let mut resolved: Resolved =
            serde_json::from_value(metadata.clone()).map_err(|_| "Invalid model metadata")?;
        if hints == &Hints::default() {
            return Ok(());
        }
        hints.apply(&mut resolved.capabilities);
        let invalid = resolved.capabilities.validate().is_err();
        resolved.image_known |= hints.images.is_some();
        resolved.reasoning_known |= hints.reasoning.is_some();
        if invalid {
            resolved.capabilities = ModelCapabilities::for_model(model);
            resolved.sources = vec![if resolved.capabilities != ModelCapabilities::default() {
                "builtin".into()
            } else {
                "unknown".into()
            }];
            resolved.updated_at = None;
        } else {
            resolved
                .sources
                .retain(|s| s != "unknown" && s != "builtin");
            if !resolved.sources.iter().any(|s| s == "upstream") {
                resolved.sources.push("upstream".into());
            }
            resolved.updated_at = Some(resolved.updated_at.unwrap_or(0).max(self.fetched_at));
        }
        let protocol = self.request.protocol;
        let settings = &self.request.protocol_settings;
        if protocol.is_native() {
            let value = &mut resolved.capabilities;
            value.use_custom_protocol = false;
            let live = Some(hints);
            let manual: Option<&ModelCapabilities> = None;
            let profile = &settings.reasoning_profile;
            let mut choices: Vec<String> = EFFORTS
                .iter()
                .filter_map(|e| {
                    let native = if protocol == UpstreamProtocol::OpenaiResponses {
                        let mapped = settings
                            .responses_efforts
                            .get(*e)
                            .map(String::as_str)
                            .unwrap_or(e);
                        if mapped == "max" {
                            return None;
                        }
                        mapped
                    } else {
                        match profile.mode {
                            ReasoningMode::AnthropicBudget | ReasoningMode::GeminiBudget => {
                                profile.budgets.get(*e)?;
                                *e
                            }
                            ReasoningMode::AnthropicAdaptive | ReasoningMode::GeminiLevel => {
                                profile.levels.get(*e)?.as_str()
                            }
                            ReasoningMode::None => return None,
                        }
                    };
                    if let Some(efforts) = live.and_then(|h| h.efforts.as_ref()) {
                        if (matches!(profile.mode, ReasoningMode::AnthropicAdaptive)
                            || protocol == UpstreamProtocol::OpenaiResponses)
                            && !efforts.iter().any(|s| s == native)
                        {
                            return None;
                        }
                    } else if protocol == UpstreamProtocol::OpenaiResponses
                        && !settings.responses_efforts.contains_key(*e)
                        && (!value.supports_reasoning
                            || !value
                                .reasoning
                                .supported_efforts
                                .iter()
                                .any(|s| s == native))
                    {
                        return None;
                    }
                    if manual
                        .is_some_and(|v| !v.reasoning.supported_efforts.iter().any(|s| s == *e))
                    {
                        return None;
                    }
                    Some((*e).to_owned())
                })
                .collect();
            let mode_name = match profile.mode {
                ReasoningMode::AnthropicAdaptive => Some("adaptive"),
                ReasoningMode::AnthropicBudget => Some("enabled"),
                _ => None,
            };
            if mode_name.is_some_and(|mode| {
                live.and_then(|h| h.thinking_modes.as_ref())
                    .is_some_and(|m| !m.iter().any(|s| s == mode))
            }) {
                choices.clear();
            }
            let model_supports = manual
                .map(|v| v.supports_reasoning)
                .or_else(|| live.and_then(|h| h.reasoning))
                .unwrap_or(value.supports_reasoning);
            value.supports_reasoning = model_supports && !choices.is_empty();
            value.reasoning.supported_efforts = choices;
            if let Some(last) = value.reasoning.supported_efforts.last()
                && !value
                    .reasoning
                    .supported_efforts
                    .contains(&value.reasoning.default_effort)
            {
                value.reasoning.default_effort = last.clone();
            }
            // All native encoders implement an explicit `none` request, subject to
            // model evidence; unknown model support is not advertised as disable.
            value.reasoning.can_disable_thinking = manual
                .map(|v| v.reasoning.can_disable_thinking)
                .or_else(|| live.and_then(|h| h.can_disable))
                .unwrap_or(false)
                && match protocol {
                    UpstreamProtocol::GeminiGenerateContent => {
                        profile.mode == ReasoningMode::GeminiBudget
                    }
                    UpstreamProtocol::OpenaiResponses => settings
                        .responses_efforts
                        .get("none")
                        .is_none_or(|v| v == "none"),
                    _ => true,
                };
            value.only_reasoning = value.supports_reasoning
                && !value.reasoning.can_disable_thinking
                && manual.is_some_and(|v| v.only_reasoning);
        }
        *metadata = serde_json::to_value(resolved).map_err(|_| "Invalid model metadata")?;
        Ok(())
    }
}

pub fn fetch(payload: &Value) -> Result<(String, Cache), String> {
    let mut request: Request =
        serde_json::from_value(payload.clone()).map_err(|_| "Invalid model discovery request")?;
    request.upstream = request.upstream.trim().into();
    request.key = request.key.trim().into();
    if request.key.is_empty() {
        return Err("Enter upstream API key".into());
    }
    request.protocol_settings.validate(request.protocol)?;
    let mut endpoint = request
        .protocol_settings
        .models_endpoint(request.protocol, &request.upstream)?;
    let azure = request.protocol_settings.is_azure();
    if azure {
        let root = endpoint
            .path()
            .strip_suffix("/v1/models")
            .ok_or("Use the Azure resource endpoint or its /openai/v1 base URL")?
            .to_owned();
        endpoint.set_path(&format!("{root}/deployments"));
        endpoint.set_query(Some("api-version=2022-12-01"));
    }
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut remaining = 1024 * 1024_usize;
    let mut count = 0;
    let mut cursor = None::<String>;
    let mut seen = BTreeSet::new();
    let mut models = BTreeMap::new();
    for _ in 0..16 {
        let timeout = deadline.saturating_duration_since(Instant::now());
        if timeout.is_zero() {
            return Err("Connection timeout".into());
        }
        let mut url = endpoint.clone();
        if !azure && request.protocol == UpstreamProtocol::AnthropicMessages {
            url.query_pairs_mut().append_pair("limit", "1000");
            if let Some(cursor) = &cursor {
                url.query_pairs_mut().append_pair("after_id", cursor);
            }
        } else if !azure && request.protocol == UpstreamProtocol::GeminiGenerateContent {
            url.query_pairs_mut().append_pair("pageSize", "1000");
            if let Some(cursor) = &cursor {
                url.query_pairs_mut().append_pair("pageToken", cursor);
            }
        }
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .proxy(None)
            .max_redirects(0)
            .http_status_as_error(false)
            .timeout_global(Some(timeout))
            .build()
            .into();
        let get = agent
            .get(url.as_str())
            .header("Accept-Encoding", "identity");
        let get = if azure {
            get.header("api-key", &request.key)
        } else {
            match request.protocol {
                UpstreamProtocol::OpenaiChat | UpstreamProtocol::OpenaiResponses => {
                    get.header("Authorization", format!("Bearer {}", request.key))
                }
                UpstreamProtocol::AnthropicMessages => {
                    get.header("x-api-key", &request.key).header(
                        "anthropic-version",
                        &request.protocol_settings.anthropic_version,
                    )
                }
                UpstreamProtocol::GeminiGenerateContent => {
                    get.header("x-goog-api-key", &request.key)
                }
            }
        };
        let mut response = get
            .call()
            .map_err(|_| "Connection failed; check network and proxy")?;
        if !response.status().is_success() {
            let status = response.status().as_u16();
            if azure && [400, 403, 404, 410].contains(&status) {
                return Err("Azure deployment discovery unavailable; enter your deployment name manually / 无法获取 Azure 部署，请手动填写部署名称".into());
            }
            return Err(format!("Upstream HTTP {status}"));
        }
        if response
            .headers()
            .get("content-encoding")
            .is_some_and(|v| v != "identity")
        {
            return Err("Unsupported model list encoding".into());
        }
        let bytes = response
            .body_mut()
            .with_config()
            .limit((remaining + 1) as u64)
            .read_to_vec()
            .map_err(|_| "Invalid or oversized model list")?;
        if bytes.len() > remaining {
            return Err("Model list exceeds 1 MiB".into());
        }
        remaining -= bytes.len();
        let data: Value = serde_json::from_slice(&bytes).map_err(|_| "Invalid model list")?;
        let rows = data[if request.protocol == UpstreamProtocol::GeminiGenerateContent && !azure {
            "models"
        } else {
            "data"
        }]
        .as_array()
        .ok_or("Invalid model list array")?;
        count += rows.len();
        if count > 4096 {
            return Err("At most 4096 discovered models".into());
        }
        for row in rows {
            if request.protocol == UpstreamProtocol::GeminiGenerateContent
                && !row["supportedGenerationMethods"]
                    .as_array()
                    .is_some_and(|a| a.iter().any(|v| v == "generateContent"))
            {
                continue;
            }
            let id = row[if request.protocol == UpstreamProtocol::GeminiGenerateContent {
                "name"
            } else {
                "id"
            }]
            .as_str()
            .filter(|s| !s.is_empty() && *s == s.trim())
            .ok_or("Invalid model ID")?;
            let id = if request.protocol == UpstreamProtocol::GeminiGenerateContent {
                crate::upstream_config::gemini_model(id)?
            } else {
                id
            };
            if id.len() > 160 || id.chars().any(char::is_control) {
                return Err("Invalid model ID".into());
            }
            models.insert(id.into(), Hints::native(row, request.protocol));
        }
        cursor = if azure {
            None
        } else {
            match request.protocol {
                UpstreamProtocol::AnthropicMessages => match data["has_more"].as_bool() {
                    Some(true) => Some(
                        data["last_id"]
                            .as_str()
                            .filter(|s| !s.is_empty())
                            .ok_or("Missing model cursor")?
                            .to_owned(),
                    ),
                    Some(false) => None,
                    None => return Err("Missing model pagination state".into()),
                },
                UpstreamProtocol::GeminiGenerateContent => match data.get("nextPageToken") {
                    None => None,
                    Some(Value::String(s)) if s.is_empty() => None,
                    Some(Value::String(s)) => Some(s.clone()),
                    _ => return Err("Invalid model cursor".into()),
                },
                _ => None,
            }
        };
        let Some(next) = &cursor else {
            if models.is_empty() {
                return Err(if azure { "No Azure deployments returned; enter your deployment name manually / 未返回 Azure 部署，请手动填写部署名称" } else { "No available models returned" }.into());
            }
            let ids = json!(models.keys().collect::<Vec<_>>()).to_string();
            return Ok((
                ids,
                Cache {
                    request,
                    models,
                    fetched_at: SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs(),
                },
            ));
        };
        if next.len() > 4096 || next.chars().any(char::is_control) || !seen.insert(next.clone()) {
            return Err("Invalid or repeated model cursor".into());
        }
    }
    Err("Model list exceeds 16 pages".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The live immutable core is the reference for parsing and normalization.
    #[test]
    #[ignore = "requires the isolated real-core fixture manifest"]
    fn core_parity() {
        let manifest = std::env::var("WORKBUDDY_PROXY_DISCOVERY_PARITY_FIXTURE")
            .expect("fixture manifest required");
        let fixture: Value = serde_json::from_slice(&std::fs::read(manifest).unwrap()).unwrap();
        let dir = std::path::Path::new(fixture["core_dir"].as_str().unwrap());
        let mut results = Vec::new();
        for case in fixture["cases"].as_array().unwrap() {
            let payload = case["payload"].clone();
            let mut core_payload = payload.clone();
            core_payload["command"] = json!("fetch");
            let reference = crate::backend::call(dir, &core_payload);
            let candidate = fetch(&payload);
            if case["error"].as_bool() == Some(true) {
                assert!(reference.is_err(), "core must reject {}", case["name"]);
                assert!(candidate.is_err(), "UI must reject {}", case["name"]);
            } else {
                let ids: Vec<String> = serde_json::from_str(&reference.unwrap()).unwrap();
                let (ui_ids, cache) =
                    candidate.unwrap_or_else(|error| panic!("{}: {error}", case["name"]));
                assert_eq!(ids, serde_json::from_str::<Vec<String>>(&ui_ids).unwrap());
                for model in ids.iter().take(4) {
                    let mut query = payload.clone();
                    query["command"] = json!("model_metadata");
                    query["model"] = json!(model);
                    let reference: Value =
                        serde_json::from_str(&crate::backend::call(dir, &query).unwrap()).unwrap();
                    let mut base = query.clone();
                    base["key"] = json!("synthetic-resolver-key");
                    let mut candidate: Value =
                        serde_json::from_str(&crate::backend::call(dir, &base).unwrap()).unwrap();
                    assert!(cache.matches(&query));
                    let mut copy = query.clone();
                    copy["provider_id"] = json!("independent-copy");
                    assert!(cache.matches(&copy));
                    copy["key"] = json!("another-account");
                    assert!(!cache.matches(&copy));
                    cache.enrich(&query, &mut candidate["metadata"]).unwrap();
                    for field in ["capabilities", "sources", "image_known", "reasoning_known"] {
                        assert_eq!(
                            candidate["metadata"][field], reference["metadata"][field],
                            "case {} model {model} field {field}",
                            case["name"]
                        );
                    }
                }
            }
            results.push(json!({"case":case["name"],"passed":true}));
        }
        std::fs::write(
            fixture["output"].as_str().unwrap(),
            json!({"mode":"real immutable core / UI discovery parity","cases":results}).to_string(),
        )
        .unwrap();
    }
}
