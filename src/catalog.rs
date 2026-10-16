//! Cached capability metadata. Forwarding lookups do no network or disk I/O;
//! credentials themselves are never stored here. Explicit settings stay in config.json.
use crate::capabilities::{ContextWindow, EFFORTS, ModelCapabilities};
use crate::upstream_config::{ProtocolSettings, ReasoningMode, UpstreamProtocol};
use futures_util::{StreamExt, stream};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const DAY: u64 = 24 * 60 * 60;
const RETRY: u64 = 60 * 60;
const CATALOG_LIMIT: usize = 16 * 1024 * 1024;
const LIVE_LIMIT: usize = 1024 * 1024;
const CATALOG_URL: &str = "https://models.dev/api.json";

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Missing is different from an explicit false or an explicit empty effort list.
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

    fn native(v: &Value, protocol: UpstreamProtocol) -> Self {
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

    fn overlay(&mut self, higher: &Self) {
        macro_rules! fields { ($($field:ident),*) => { $(if higher.$field.is_some() { self.$field = higher.$field.clone(); })* }; }
        fields!(
            tools,
            images,
            reasoning,
            efforts,
            can_disable,
            input,
            output,
            context,
            thinking_modes
        );
        // A text-only/reasoning-off declaration must also clear lower-priority
        // reasoning choices. A supplied list fully replaces the older list.
        if higher.reasoning == Some(false) {
            self.efforts = Some(vec![]);
        }
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

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct KnownModel {
    canonical: String,
    hints: Hints,
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct ProviderCatalog {
    api: String,
    models: BTreeMap<String, KnownModel>,
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct Live {
    fetched_at: u64,
    models: BTreeMap<String, Hints>,
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct Cache {
    version: u32,
    fetched_at: u64,
    providers: BTreeMap<String, ProviderCatalog>,
    live: BTreeMap<String, Live>,
    #[serde(skip)]
    origins: BTreeMap<String, Option<String>>,
}
impl Cache {
    fn reindex(&mut self) {
        self.origins.clear();
        for p in self.providers.values() {
            for (id, model) in &p.models {
                if model.canonical.is_empty() {
                    continue;
                }
                for key in [
                    tail(id),
                    tail(&model.canonical),
                    model.canonical.to_ascii_lowercase(),
                ] {
                    self.origins
                        .entry(key)
                        .and_modify(|entry| {
                            if entry.as_ref() != Some(&model.canonical) {
                                *entry = None;
                            }
                        })
                        .or_insert_with(|| Some(model.canonical.clone()));
                }
            }
        }
    }
}
#[derive(Clone)]
pub struct Target {
    pub upstream: String,
    pub key: String,
    pub protocol: UpstreamProtocol,
    pub settings: ProtocolSettings,
}
#[derive(Clone, Copy)]
pub struct Wire<'a> {
    pub protocol: UpstreamProtocol,
    pub settings: &'a ProtocolSettings,
    pub credential: &'a str,
}
pub struct Catalog {
    path: PathBuf,
    cache: Mutex<Cache>,
    refresh: tokio::sync::Mutex<()>,
    attempts: Mutex<BTreeMap<String, u64>>,
    persistence: Mutex<()>,
}

#[derive(Serialize)]
pub struct Resolved {
    pub capabilities: ModelCapabilities,
    pub sources: Vec<&'static str>,
    pub image_known: bool,
    pub reasoning_known: bool,
    pub updated_at: Option<u64>,
}

/// Retain scheme/port and plan paths. Only documented OpenCode path aliases
/// share metadata; Go and the ordinary inference API remain separate.
pub(crate) fn endpoint(base: &str) -> Option<String> {
    let mut u = reqwest::Url::parse(base.trim()).ok()?;
    if !matches!(u.scheme(), "http" | "https")
        || u.host_str().is_none()
        || !u.username().is_empty()
        || u.password().is_some()
        || u.query().is_some()
        || u.fragment().is_some()
    {
        return None;
    }
    let mut path = u.path().trim_end_matches('/').to_owned();
    for suffix in ["/chat/completions", "/responses", "/messages"] {
        if let Some(s) = path.strip_suffix(suffix) {
            path = s.to_owned();
            break;
        }
    }
    if u.host_str() == Some("opencode.ai") && (path == "/zen/v1" || path == "/zen/go/v1") {
        path = path.replacen("/zen", "/inference", 1);
    }
    // Versions are syntax, while the preceding plan path identifies service.
    if let Some((prefix, version)) = path.rsplit_once('/')
        && version
            .strip_prefix('v')
            .is_some_and(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
    {
        path = prefix.to_owned();
    }
    u.set_path(&path);
    Some(u.as_str().trim_end_matches('/').to_owned())
}
fn catalog_key(base: &str, wire: Wire<'_>) -> Option<String> {
    let Wire {
        protocol,
        settings,
        credential,
    } = wire;
    let base = settings.api_base(protocol, base).ok()?;
    if protocol == UpstreamProtocol::OpenaiChat && !settings.is_azure() {
        endpoint(&base)
    } else {
        settings.models_endpoint(protocol, &base).ok().map(|u| {
            format!(
                "{}{}|{}|{:x}",
                if settings.is_azure() {
                    "azure_api_key|"
                } else {
                    ""
                },
                protocol.as_str(),
                u,
                Sha256::digest(credential.as_bytes())
            )
        })
    }
}
fn tail(id: &str) -> String {
    id.trim()
        .rsplit('/')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase()
}
fn find<'a, T>(rows: &'a BTreeMap<String, T>, model: &str) -> Option<&'a T> {
    if let Some(v) = rows.get(model) {
        return Some(v);
    }
    let exact: Vec<_> = rows
        .iter()
        .filter(|(id, _)| id.eq_ignore_ascii_case(model))
        .collect();
    if exact.len() == 1 {
        return Some(exact[0].1);
    }
    let matches: Vec<_> = rows
        .iter()
        .filter(|(id, _)| tail(id) == tail(model))
        .collect();
    (matches.len() == 1).then(|| matches[0].1)
}

impl Catalog {
    pub fn load(dir: &std::path::Path) -> Self {
        let path = dir.join("model-catalog.json");
        let mut cache = std::fs::metadata(&path)
            .ok()
            .filter(|m| m.len() <= CATALOG_LIMIT as u64)
            .and_then(|_| std::fs::read(&path).ok())
            .and_then(|b| serde_json::from_slice::<Cache>(&b).ok())
            .filter(|c| c.version == 1)
            .unwrap_or_default();
        cache.reindex();
        Self {
            path,
            cache: Mutex::new(cache),
            refresh: tokio::sync::Mutex::new(()),
            attempts: Mutex::new(BTreeMap::new()),
            persistence: Mutex::new(()),
        }
    }
    fn persist(&self) {
        // Serialize writes without holding the hot metadata lock during JSON
        // encoding or disk I/O. Production callers run this on the blocking pool.
        let _writer = self.persistence.lock().unwrap();
        let snapshot = self.cache.lock().unwrap().clone();
        let _ = crate::config::write_json(&self.path, &snapshot);
    }

    pub async fn remember_async(
        self: &Arc<Self>,
        upstream: String,
        protocol: UpstreamProtocol,
        credential: String,
        settings: ProtocolSettings,
        models: BTreeMap<String, Hints>,
    ) {
        let catalog = self.clone();
        let _ = tokio::task::spawn_blocking(move || {
            catalog.remember_protocol(&upstream, protocol, &credential, &settings, models)
        })
        .await;
    }

    #[cfg(test)]
    pub fn resolve(
        &self,
        upstream: &str,
        model: &str,
        manual: Option<&ModelCapabilities>,
    ) -> Resolved {
        self.resolve_protocol(
            upstream,
            model,
            manual,
            Wire {
                protocol: UpstreamProtocol::OpenaiChat,
                settings: &ProtocolSettings::default(),
                credential: "",
            },
        )
    }

    pub fn resolve_protocol(
        &self,
        upstream: &str,
        model: &str,
        manual: Option<&ModelCapabilities>,
        wire: Wire<'_>,
    ) -> Resolved {
        let Wire {
            protocol,
            settings,
            credential,
        } = wire;
        let mut resolved = self.resolve_raw(upstream, model, manual, wire);
        if !protocol.is_native() {
            return resolved;
        }
        let value = &mut resolved.capabilities;
        value.use_custom_protocol = false;
        let cache = self.cache.lock().unwrap();
        let live = catalog_key(
            upstream,
            Wire {
                protocol,
                settings,
                credential,
            },
        )
        .and_then(|k| cache.live.get(&k))
        .and_then(|l| find(&l.models, model));
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
                if manual.is_some_and(|v| !v.reasoning.supported_efforts.iter().any(|s| s == *e)) {
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
        resolved
    }

    fn resolve_raw(
        &self,
        upstream: &str,
        model: &str,
        manual: Option<&ModelCapabilities>,
        wire: Wire<'_>,
    ) -> Resolved {
        let Wire {
            protocol,
            settings,
            credential,
        } = wire;
        if let Some(value) = manual {
            return Resolved {
                capabilities: value.clone(),
                sources: vec!["manual"],
                image_known: true,
                reasoning_known: true,
                updated_at: None,
            };
        }
        let cache = self.cache.lock().unwrap();
        let key = endpoint(upstream);
        let mut hints = Hints::default();
        let mut sources = vec![];
        let mut updated_at = None;
        // Canonical identity must be unambiguous across providers. Never use
        // whichever reseller happened to sort first as the original model.
        if let Some(Some(canonical)) = cache
            .origins
            .get(&model.trim().to_ascii_lowercase())
            .or_else(|| cache.origins.get(&tail(model)))
            && let Some((provider, id)) = canonical.split_once('/')
            && let Some(m) = cache.providers.get(provider).and_then(|p| {
                p.models.get(id).or_else(|| {
                    let aliases: Vec<_> = p
                        .models
                        .values()
                        .filter(|m| &m.canonical == canonical)
                        .collect();
                    aliases
                        .first()
                        .copied()
                        .filter(|first| aliases.iter().all(|m| m.hints == first.hints))
                })
            })
        {
            hints.overlay(&m.hints);
            sources.push("models.dev:canonical");
            updated_at = Some(cache.fetched_at);
        }
        // More than one catalogue provider may share an endpoint. Apply it
        // only if the matching records agree, rather than picking arbitrarily.
        let matching: Vec<_> = cache
            .providers
            .values()
            .filter(|p| key.is_some() && endpoint(&p.api) == key)
            .filter_map(|p| find(&p.models, model))
            .collect();
        if let Some(m) = matching.first()
            && matching.iter().all(|other| other.hints == m.hints)
        {
            hints.overlay(&m.hints);
            sources.push("models.dev:provider");
            updated_at = Some(cache.fetched_at);
        }
        if let Some(live) = catalog_key(
            upstream,
            Wire {
                protocol,
                settings,
                credential,
            },
        )
        .as_ref()
        .and_then(|k| cache.live.get(k))
            && let Some(m) = find(&live.models, model)
            && m != &Hints::default()
        {
            hints.overlay(m);
            sources.push("upstream");
            updated_at = Some(updated_at.unwrap_or(0).max(live.fetched_at));
        }
        let mut value = ModelCapabilities::for_model(model);
        let built_in = value != ModelCapabilities::default();
        hints.apply(&mut value);
        // Malformed external metadata must never break save/sync. Manual
        // settings are validated separately at the configuration boundary.
        if value.validate().is_err() {
            value = ModelCapabilities::for_model(model);
            sources.clear();
            updated_at = None;
        }
        if sources.is_empty() {
            sources.push(if built_in { "builtin" } else { "unknown" });
        }
        Resolved {
            capabilities: value,
            sources,
            image_known: hints.images.is_some() || built_in,
            reasoning_known: hints.reasoning.is_some() || built_in,
            updated_at,
        }
    }

    #[cfg(test)]
    pub fn remember(&self, upstream: &str, models: BTreeMap<String, Hints>) {
        self.remember_protocol(
            upstream,
            UpstreamProtocol::OpenaiChat,
            "",
            &ProtocolSettings::default(),
            models,
        )
    }
    pub fn remember_protocol(
        &self,
        upstream: &str,
        protocol: UpstreamProtocol,
        credential: &str,
        settings: &ProtocolSettings,
        models: BTreeMap<String, Hints>,
    ) {
        if let Some(key) = catalog_key(
            upstream,
            Wire {
                protocol,
                settings,
                credential,
            },
        ) {
            let mut cache = self.cache.lock().unwrap();
            cache.version = 1;
            cache.live.insert(
                key,
                Live {
                    fetched_at: now(),
                    models,
                },
            );
            // Bound draft endpoint history as well as each individual list.
            while cache.live.len() > 64 {
                let oldest = cache
                    .live
                    .iter()
                    .min_by_key(|(_, v)| v.fetched_at)
                    .unwrap()
                    .0
                    .clone();
                cache.live.remove(&oldest);
            }
            drop(cache);
            self.persist();
        }
    }

    fn due(&self, key: &str, fetched: u64) -> bool {
        let time = now();
        if fetched > 0 && time.saturating_sub(fetched) < DAY {
            return false;
        }
        let mut attempts = self.attempts.lock().unwrap();
        if attempts
            .get(key)
            .is_some_and(|v| time.saturating_sub(*v) < RETRY)
        {
            return false;
        }
        attempts.insert(key.to_owned(), time);
        true
    }

    pub async fn refresh(self: &Arc<Self>, client: &reqwest::Client, targets: Vec<Target>) {
        let _guard = self.refresh.lock().await;
        let public = async {
            let fetched = self.cache.lock().unwrap().fetched_at;
            if !self.due("models.dev", fetched) {
                return;
            }
            // A loopback-only override permits completely offline integration
            // tests without changing the production service or global network.
            let url = std::env::var("WORKBUDDY_PROXY_CATALOG_TEST_URL")
                .ok()
                .filter(|s| {
                    reqwest::Url::parse(s).ok().is_some_and(|u| {
                        u.scheme() == "http" && matches!(u.host_str(), Some("127.0.0.1" | "[::1]"))
                    })
                })
                .unwrap_or_else(|| CATALOG_URL.to_owned());
            let result = async {
                let bytes = download(
                    client.get(url).header(
                        "User-Agent",
                        concat!("WorkBuddy-Proxy/", env!("CARGO_PKG_VERSION")),
                    ),
                    CATALOG_LIMIT,
                    Duration::from_secs(5),
                )
                .await?;
                tokio::task::spawn_blocking(move || parse_public(&bytes))
                    .await
                    .map_err(|_| "Invalid catalogue".to_owned())?
            }
            .await;
            if let Ok(providers) = result {
                let catalog = self.clone();
                let _ = tokio::task::spawn_blocking(move || {
                    let mut incoming = Cache {
                        providers,
                        ..Default::default()
                    };
                    incoming.reindex();
                    {
                        let mut cache = catalog.cache.lock().unwrap();
                        cache.version = 1;
                        cache.providers = incoming.providers;
                        cache.origins = incoming.origins;
                        cache.fetched_at = now();
                    }
                    catalog.persist();
                })
                .await;
            }
        };
        let live = async {
            let mut seen = BTreeSet::new();
            let targets: Vec<_> = targets
                .into_iter()
                .filter(|t| {
                    let Some(key) = catalog_key(
                        &t.upstream,
                        Wire {
                            protocol: t.protocol,
                            settings: &t.settings,
                            credential: &t.key,
                        },
                    ) else {
                        return false;
                    };
                    if t.key.is_empty() || !seen.insert(key.clone()) {
                        return false;
                    }
                    let fetched = self
                        .cache
                        .lock()
                        .unwrap()
                        .live
                        .get(&key)
                        .map_or(0, |v| v.fetched_at);
                    self.due(&key, fetched)
                })
                .collect();
            stream::iter(targets)
                .for_each_concurrent(2, |target| async move {
                    if let Ok(rows) = fetch_protocol(
                        client,
                        &target.upstream,
                        &target.key,
                        target.protocol,
                        &target.settings,
                        Duration::from_secs(4),
                    )
                    .await
                    {
                        self.remember_async(
                            target.upstream,
                            target.protocol,
                            target.key,
                            target.settings,
                            rows,
                        )
                        .await;
                    }
                })
                .await;
        };
        tokio::join!(public, live);
    }
}

pub(crate) async fn download(
    request: reqwest::RequestBuilder,
    limit: usize,
    timeout: Duration,
) -> Result<Vec<u8>, String> {
    tokio::time::timeout(timeout, async {
        let response = request
            .send()
            .await
            .map_err(|_| "Connection failed; check network and proxy")?;
        if !response.status().is_success() {
            return Err(format!("Upstream HTTP {}", response.status().as_u16()));
        }
        if response
            .headers()
            .get("content-encoding")
            .is_some_and(|v| v != "identity")
        {
            return Err("Unsupported model list encoding".into());
        }
        let mut bytes = Vec::new();
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| "Invalid model list")?;
            if bytes.len() + chunk.len() > limit {
                return Err(format!("Model list exceeds {} MiB", limit / 1024 / 1024));
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(bytes)
    })
    .await
    .map_err(|_| "Connection timeout")?
}

pub async fn fetch_protocol(
    client: &reqwest::Client,
    base: &str,
    key: &str,
    protocol: UpstreamProtocol,
    settings: &ProtocolSettings,
    timeout: Duration,
) -> Result<BTreeMap<String, Hints>, String> {
    let endpoint = settings.models_endpoint(protocol, base)?;
    settings.validate(protocol)?;
    tokio::time::timeout(timeout.min(Duration::from_secs(30)), async {
        // Azure inference takes deployment names. The v1 models catalog may
        // contain underlying model IDs instead, so never use it as a fallback.
        if settings.is_azure() {
            let mut deployments = endpoint.clone();
            let root = deployments
                .path()
                .strip_suffix("/v1/models")
                .ok_or("Use the Azure resource endpoint or its /openai/v1 base URL")?;
            deployments.set_path(&format!("{root}/deployments"));
            deployments.set_query(Some("api-version=2022-12-01"));
            let result = download(
                client
                    .get(deployments)
                    .header("api-key", key)
                    .header("Accept-Encoding", "identity"),
                LIVE_LIMIT,
                timeout,
            )
            .await;
            match result {
                Ok(bytes) => {
                    let data: Value = serde_json::from_slice(&bytes)
                        .map_err(|_| "Invalid Azure deployment list")?;
                    let rows = data["data"]
                        .as_array()
                        .ok_or("Invalid Azure deployment list")?;
                    if rows.len() > 4096 {
                        return Err("At most 4096 discovered models".into());
                    }
                    if rows.is_empty() {
                        return Err("No Azure deployments returned; enter your deployment name manually / 未返回 Azure 部署，请手动填写部署名称".into());
                    }
                    let mut models = BTreeMap::new();
                    for row in rows {
                        let id = row["id"]
                            .as_str()
                            .filter(|id| {
                                !id.is_empty()
                                    && *id == id.trim()
                                    && id.len() <= 160
                                    && !id.chars().any(char::is_control)
                            })
                            .ok_or("Invalid Azure deployment name")?;
                        models.insert(id.to_owned(), Hints::native(row, protocol));
                    }
                    return Ok(models);
                }
                Err(error)
                    if matches!(
                        error.as_str(),
                        "Upstream HTTP 400"
                            | "Upstream HTTP 403"
                            | "Upstream HTTP 404"
                            | "Upstream HTTP 410"
                    ) => return Err("Azure deployment discovery unavailable; enter your deployment name manually / 无法获取 Azure 部署，请手动填写部署名称".into()),
                Err(error) => return Err(error),
            }
        }
        let mut models = BTreeMap::new();
        let mut cursor = None::<String>;
        let mut seen = BTreeSet::new();
        let mut remaining = LIVE_LIMIT;
        let mut row_count = 0;
        for _ in 0..16 {
            let mut url = endpoint.clone();
            if protocol == UpstreamProtocol::AnthropicMessages {
                url.query_pairs_mut().append_pair("limit", "1000");
                if let Some(c) = &cursor {
                    url.query_pairs_mut().append_pair("after_id", c);
                }
            } else if protocol == UpstreamProtocol::GeminiGenerateContent {
                url.query_pairs_mut().append_pair("pageSize", "1000");
                if let Some(c) = &cursor {
                    url.query_pairs_mut().append_pair("pageToken", c);
                }
            }
            let request = client.get(url).header("Accept-Encoding", "identity");
            let request = match protocol {
                UpstreamProtocol::OpenaiChat | UpstreamProtocol::OpenaiResponses => {
                    if settings.is_azure() {
                        request.header("api-key", key)
                    } else {
                        request.bearer_auth(key)
                    }
                }
                UpstreamProtocol::AnthropicMessages => request
                    .header("x-api-key", key)
                    .header("anthropic-version", &settings.anthropic_version),
                UpstreamProtocol::GeminiGenerateContent => request.header("x-goog-api-key", key),
            };
            let bytes = download(request, remaining, timeout).await?;
            remaining = remaining.saturating_sub(bytes.len());
            let data: Value = serde_json::from_slice(&bytes).map_err(|_| "Invalid model list")?;
            let rows = data[if protocol == UpstreamProtocol::GeminiGenerateContent {
                "models"
            } else {
                "data"
            }]
            .as_array()
            .ok_or("Invalid model list array")?;
            row_count += rows.len();
            if row_count > 4096 {
                return Err("At most 4096 discovered models".into());
            }
            for row in rows {
                if protocol == UpstreamProtocol::GeminiGenerateContent
                    && !row["supportedGenerationMethods"]
                        .as_array()
                        .is_some_and(|a| a.iter().any(|v| v == "generateContent"))
                {
                    continue;
                }
                let id = row[if protocol == UpstreamProtocol::GeminiGenerateContent {
                    "name"
                } else {
                    "id"
                }]
                .as_str()
                .filter(|s| !s.is_empty() && *s == s.trim())
                .ok_or("Invalid model ID")?;
                let id = if protocol == UpstreamProtocol::GeminiGenerateContent {
                    crate::upstream_config::gemini_model(id)?
                } else {
                    id
                };
                if id.len() > 160 || id.chars().any(char::is_control) {
                    return Err("Invalid model ID".into());
                }
                models.insert(id.to_owned(), Hints::native(row, protocol));
            }
            cursor = match protocol {
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
            };
            let Some(c) = &cursor else {
                return if models.is_empty() {
                    Err("No available models returned".into())
                } else {
                    Ok(models)
                };
            };
            if c.len() > 4096 || c.chars().any(char::is_control) || !seen.insert(c.clone()) {
                return Err("Invalid or repeated model cursor".into());
            }
        }
        Err("Model list exceeds 16 pages".into())
    })
    .await
    .map_err(|_| "Connection timeout".to_owned())?
}

fn parse_public(bytes: &[u8]) -> Result<BTreeMap<String, ProviderCatalog>, String> {
    #[derive(Default, Deserialize, Serialize)]
    #[serde(default)]
    struct RawModel {
        canonical_model_id: Value,
        tool_call: Value,
        reasoning: Value,
        reasoning_options: Value,
        modalities: Value,
        limit: Value,
    }
    #[derive(Deserialize)]
    struct RawProvider {
        #[serde(default)]
        api: String,
        models: BTreeMap<String, RawModel>,
    }
    let data: BTreeMap<String, RawProvider> =
        serde_json::from_slice(bytes).map_err(|_| "Invalid catalogue")?;
    if data.is_empty() {
        return Err("Empty catalogue".into());
    }
    Ok(data
        .into_iter()
        .map(|(id, p)| {
            (
                id,
                ProviderCatalog {
                    api: p.api,
                    models: p
                        .models
                        .into_iter()
                        .map(|(id, m)| {
                            let m = serde_json::to_value(m).unwrap();
                            (
                                id,
                                KnownModel {
                                    canonical: m["canonical_model_id"]
                                        .as_str()
                                        .unwrap_or("")
                                        .to_owned(),
                                    hints: Hints::parse(&m),
                                },
                            )
                        })
                        .collect(),
                },
            )
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn native_live_metadata_is_isolated_and_profile_is_intersected_with_model() {
        let tmp = tempfile::tempdir().unwrap();
        let c = Catalog::load(tmp.path());
        let base = "https://native.invalid/gateway/v1";
        let hints = Hints::native(
            &json!({"id":"m","max_input_tokens":10000,"max_tokens":1000,
            "capabilities":{"image_input":{"supported":true},"effort":{"low":{"supported":true},"high":{"supported":false}},
            "thinking":{"supported":true,"types":{"adaptive":{"supported":true},"enabled":{"supported":false},"disabled":{"supported":false}}}}}),
            UpstreamProtocol::AnthropicMessages,
        );
        c.remember_protocol(
            base,
            UpstreamProtocol::AnthropicMessages,
            "fixture-key",
            &ProtocolSettings::default(),
            BTreeMap::from([("m".into(), hints)]),
        );
        c.remember_protocol(
            base,
            UpstreamProtocol::OpenaiResponses,
            "fixture-key",
            &ProtocolSettings::default(),
            BTreeMap::from([(
                "bare".into(),
                Hints::native(
                    &json!({"supportsReasoning":true}),
                    UpstreamProtocol::OpenaiResponses,
                ),
            )]),
        );
        assert!(
            !c.resolve_protocol(
                base,
                "bare",
                None,
                Wire {
                    protocol: UpstreamProtocol::OpenaiResponses,
                    settings: &ProtocolSettings::default(),
                    credential: "fixture-key"
                }
            )
            .capabilities
            .supports_reasoning
        );
        let mut s = ProtocolSettings::default();
        s.reasoning_profile.mode = ReasoningMode::AnthropicAdaptive;
        s.reasoning_profile.levels =
            BTreeMap::from([("high".into(), "low".into()), ("max".into(), "high".into())]);
        let v = c.resolve_protocol(
            base,
            "m",
            None,
            Wire {
                protocol: UpstreamProtocol::AnthropicMessages,
                settings: &s,
                credential: "fixture-key",
            },
        );
        assert!(v.capabilities.supports_images && v.capabilities.supports_reasoning);
        assert_eq!(v.capabilities.reasoning.supported_efforts, vec!["high"]);
        assert!(!v.capabilities.reasoning.can_disable_thinking);
        assert_eq!(v.capabilities.max_output_tokens, Some(1000));
        v.capabilities.validate().unwrap();
        assert!(!c.resolve(base, "m", None).image_known);
        assert!(
            !c.resolve_protocol(
                base,
                "m",
                None,
                Wire {
                    protocol: UpstreamProtocol::AnthropicMessages,
                    settings: &s,
                    credential: "different-key"
                }
            )
            .image_known
        );
        assert!(
            !c.resolve_protocol(
                base,
                "m",
                None,
                Wire {
                    protocol: UpstreamProtocol::OpenaiResponses,
                    settings: &ProtocolSettings::default(),
                    credential: "fixture-key"
                }
            )
            .image_known
        );
        s.reasoning_profile.mode = ReasoningMode::AnthropicBudget;
        s.reasoning_profile.levels.clear();
        s.reasoning_profile.budgets.insert("high".into(), 1024);
        assert!(
            !c.resolve_protocol(
                base,
                "m",
                None,
                Wire {
                    protocol: UpstreamProtocol::AnthropicMessages,
                    settings: &s,
                    credential: "fixture-key"
                }
            )
            .capabilities
            .supports_reasoning
        );
        let manual = ModelCapabilities {
            use_custom_protocol: true,
            ..Default::default()
        };
        assert!(
            !c.resolve_protocol(
                base,
                "m",
                Some(&manual),
                Wire {
                    protocol: UpstreamProtocol::OpenaiResponses,
                    settings: &ProtocolSettings::default(),
                    credential: "fixture-key"
                }
            )
            .capabilities
            .use_custom_protocol
        );
        // Reload does not collapse native cache keys into the old Chat namespace.
        let loaded = Catalog::load(tmp.path());
        assert!(!loaded.resolve(base, "m", None).image_known);
        assert!(
            loaded
                .resolve_protocol(
                    base,
                    "m",
                    None,
                    Wire {
                        protocol: UpstreamProtocol::AnthropicMessages,
                        settings: &s,
                        credential: "fixture-key"
                    }
                )
                .capabilities
                .supports_images
        );
    }

    #[test]
    fn waiting_for_catalog_persistence_does_not_lock_capability_lookups() {
        let tmp = tempfile::tempdir().unwrap();
        let c = Arc::new(Catalog::load(tmp.path()));
        let guard = c.persistence.lock().unwrap();
        let writer = c.clone();
        let worker = std::thread::spawn(move || {
            writer.remember(
                "https://p.invalid/v1",
                BTreeMap::from([(
                    "model".into(),
                    Hints {
                        images: Some(true),
                        ..Default::default()
                    },
                )]),
            )
        });
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while !c.cache.try_lock().is_ok_and(|cache| {
            cache.live.values().any(|live| {
                live.models
                    .get("model")
                    .is_some_and(|m| m.images == Some(true))
            })
        }) {
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        assert!(c.resolve("https://p.invalid/v1", "model", None).image_known);
        assert!(!worker.is_finished());
        drop(guard);
        worker.join().unwrap();
    }

    fn seeded(dir: &std::path::Path) -> Catalog {
        let catalog = Catalog::load(dir);
        let data = json!({
            "author":{"api":"https://author.invalid/v1","models":{
                "vision-model":{"canonical_model_id":"author/vision-model","tool_call":true,
                    "reasoning":true,"reasoning_options":[{"type":"effort","values":["low","high"]}],
                    "modalities":{"input":["text","image"]},"limit":{"context":1000000,"output":32000}}
            }},
            "plan":{"api":"https://host.invalid/coding/v1","models":{
                "vision-model":{"canonical_model_id":"author/vision-model","modalities":{"input":["text"]},"limit":{"context":128000}}
            }}
        });
        let mut cache = catalog.cache.lock().unwrap();
        cache.version = 1;
        cache.providers = parse_public(&serde_json::to_vec(&data).unwrap()).unwrap();
        cache.fetched_at = 100;
        cache.reindex();
        drop(cache);
        catalog
    }

    #[test]
    fn provider_and_live_declarations_override_canonical_per_field() {
        let dir = tempfile::tempdir().unwrap();
        let catalog = seeded(dir.path());
        let value = catalog.resolve("https://host.invalid/coding/v1", "vision-model", None);
        assert!(!value.capabilities.supports_images);
        assert_eq!(value.capabilities.max_input_tokens, Some(128000));
        assert_eq!(value.capabilities.max_output_tokens, Some(32000));
        assert!(value.capabilities.supports_reasoning);
        // Another plan at the same host must not inherit the text-only override.
        assert!(
            catalog
                .resolve("https://host.invalid/api/v1", "vision-model", None)
                .capabilities
                .supports_images
        );
        catalog.remember("https://host.invalid/coding/v1", BTreeMap::from([("vision-model".into(),
            Hints::parse(&json!({"modalities":{"input":["text","image"]},"tool_call":false,"reasoning":false,"context_length":64000})))]));
        let value = catalog.resolve("https://host.invalid/coding/v1", "vision-model", None);
        assert!(value.capabilities.supports_images);
        assert!(!value.capabilities.supports_tool_call);
        assert!(!value.capabilities.supports_reasoning);
        assert_eq!(value.capabilities.max_input_tokens, Some(64000));
        assert_eq!(value.sources.last(), Some(&"upstream"));
        value.capabilities.validate().unwrap();
        let manual = ModelCapabilities::default();
        assert_eq!(
            catalog
                .resolve(
                    "https://host.invalid/coding/v1",
                    "vision-model",
                    Some(&manual)
                )
                .capabilities,
            manual
        );
    }

    #[test]
    fn offline_cache_restores_public_and_live_metadata_without_credentials() {
        let dir = tempfile::tempdir().unwrap();
        let catalog = seeded(dir.path());
        catalog.remember(
            "https://host.invalid/coding/v1",
            BTreeMap::from([(
                "new-model".into(),
                Hints::parse(&json!({"supportsImages":true,"max_output_tokens":8192})),
            )]),
        );
        let restored = Catalog::load(dir.path());
        assert!(
            restored
                .resolve("https://host.invalid/coding/v1", "new-model", None)
                .capabilities
                .supports_images
        );
        assert!(
            restored
                .resolve("https://other.invalid/v1", "vision-model", None)
                .capabilities
                .supports_images
        );
        let file = std::fs::read_to_string(dir.path().join("model-catalog.json")).unwrap();
        assert!(!file.contains("api_key") && !file.contains("Authorization"));
        // A truncated cache must not prevent starting or using built-in defaults.
        std::fs::write(dir.path().join("model-catalog.json"), "{").unwrap();
        assert!(
            Catalog::load(dir.path())
                .resolve("", "deepseek-flash", None)
                .capabilities
                .supports_images
        );
    }

    #[test]
    fn ambiguity_and_unrecognised_variants_do_not_borrow_other_models() {
        let dir = tempfile::tempdir().unwrap();
        let catalog = seeded(dir.path());
        catalog.cache.lock().unwrap().providers.insert(
            "other".into(),
            ProviderCatalog {
                api: "https://other.invalid".into(),
                models: BTreeMap::from([(
                    "vision-model".into(),
                    KnownModel {
                        canonical: "other/vision-model".into(),
                        hints: Hints {
                            images: Some(false),
                            ..Default::default()
                        },
                    },
                )]),
            },
        );
        catalog.cache.lock().unwrap().reindex();
        let resolved = catalog.resolve("https://unknown.invalid/v1", "vision-model", None);
        assert!(!resolved.image_known);
        assert_eq!(resolved.sources, vec!["unknown"]);
        assert!(
            !catalog
                .resolve("https://author.invalid/v1", "vision-model-new", None)
                .image_known
        );
        assert!(
            catalog
                .resolve("https://author.invalid/v1", "vision-model", None)
                .capabilities
                .supports_images
        );
    }

    #[test]
    fn endpoint_aliases_preserve_plan_and_local_port_identity() {
        assert_eq!(
            endpoint("https://opencode.ai/zen/go/v1/"),
            endpoint("https://opencode.ai/inference/go/v1")
        );
        assert_ne!(
            endpoint("https://opencode.ai/zen/go/v1"),
            endpoint("https://opencode.ai/zen/v1")
        );
        assert_eq!(
            endpoint("https://host.invalid/v1/chat/completions"),
            endpoint("https://host.invalid")
        );
        assert_ne!(
            endpoint("http://localhost:18080/v1"),
            endpoint("http://localhost:18081/v1")
        );
        assert!(endpoint("https://user:secret@host.invalid/v1").is_none());
        assert!(endpoint("https://host.invalid/v1?api_key=secret").is_none());
    }

    #[test]
    fn canonical_identity_can_use_a_unique_author_api_alias() {
        let dir = tempfile::tempdir().unwrap();
        let catalog = Catalog::load(dir.path());
        let data = json!({"author":{"models":{"api-alias":{
            "canonical_model_id":"author/internal-model-id",
            "modalities":{"input":["text","image"]},"limit":{"input":128000,"context":64000}
        }}}});
        let mut cache = catalog.cache.lock().unwrap();
        cache.providers = parse_public(&serde_json::to_vec(&data).unwrap()).unwrap();
        cache.reindex();
        drop(cache);
        let value = catalog.resolve("https://unknown.invalid/v1", "internal-model-id", None);
        assert!(value.capabilities.supports_images);
        assert_eq!(value.sources, vec!["models.dev:canonical"]);
        assert_eq!(value.capabilities.max_input_tokens, Some(64000));
    }

    #[test]
    fn unknown_values_are_not_false_and_effort_only_models_are_not_invented() {
        let unknown =
            Hints::parse(&json!({"modalities":{"input":[]},"limit":{"input":0,"output":-1}}));
        assert_eq!(unknown.images, None);
        assert_eq!(unknown.input, None);
        let mut capabilities = ModelCapabilities::default();
        Hints::parse(
            &json!({"reasoning":true,"reasoning_options":[{"type":"budget","values":[4096,8192]}]}),
        )
        .apply(&mut capabilities);
        assert!(!capabilities.supports_reasoning);
        Hints::parse(
            &json!({"reasoning":true,"supported_reasoning_levels":["none","low","high","made-up"]}),
        )
        .apply(&mut capabilities);
        assert!(capabilities.supports_reasoning && capabilities.reasoning.can_disable_thinking);
        assert_eq!(
            capabilities.reasoning.supported_efforts,
            vec!["low", "high"]
        );
        capabilities.validate().unwrap();
    }
}
