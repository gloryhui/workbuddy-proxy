//! Automatic capability metadata. Never used on the forwarding path and never
//! stores credentials. Explicit model settings remain in config.json.
use crate::capabilities::{ContextWindow, EFFORTS, ModelCapabilities};
use futures_util::{StreamExt, stream};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    sync::Mutex,
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
            context: positive(v.pointer("/limit/context"))
                .or_else(|| positive(v.get("context_length"))),
        }
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
            context
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
#[derive(Default, Serialize, Deserialize)]
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
}
pub struct Catalog {
    path: PathBuf,
    cache: Mutex<Cache>,
    refresh: tokio::sync::Mutex<()>,
    attempts: Mutex<BTreeMap<String, u64>>,
}

#[derive(Serialize)]
pub struct Resolved {
    pub capabilities: ModelCapabilities,
    pub sources: Vec<&'static str>,
    pub image_known: bool,
    pub updated_at: Option<u64>,
}

/// Retain scheme/port and plan paths. Only documented OpenCode path aliases
/// share metadata; Go and the ordinary inference API remain separate.
fn endpoint(base: &str) -> Option<String> {
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
        }
    }
    fn persist(&self, cache: &Cache) {
        let _ = crate::config::write_json(&self.path, cache);
    }

    pub fn resolve(
        &self,
        upstream: &str,
        model: &str,
        manual: Option<&ModelCapabilities>,
    ) -> Resolved {
        if let Some(value) = manual {
            return Resolved {
                capabilities: value.clone(),
                sources: vec!["manual"],
                image_known: true,
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
        if let Some(live) = key.as_ref().and_then(|k| cache.live.get(k))
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
            updated_at,
        }
    }

    pub fn remember(&self, upstream: &str, models: BTreeMap<String, Hints>) {
        if let Some(key) = endpoint(upstream) {
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
            self.persist(&cache);
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

    pub async fn refresh(&self, client: &reqwest::Client, targets: Vec<Target>) {
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
                let mut cache = self.cache.lock().unwrap();
                cache.version = 1;
                cache.providers = providers;
                cache.reindex();
                cache.fetched_at = now();
                self.persist(&cache);
            }
        };
        let live = async {
            let mut seen = BTreeSet::new();
            let targets: Vec<_> = targets
                .into_iter()
                .filter(|t| {
                    let Some(key) = endpoint(&t.upstream) else {
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
                    if let Ok(rows) = fetch_provider(
                        client,
                        &target.upstream,
                        &target.key,
                        Duration::from_secs(4),
                    )
                    .await
                    {
                        self.remember(&target.upstream, rows);
                    }
                })
                .await;
        };
        tokio::join!(public, live);
    }
}

async fn download(
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

pub async fn fetch_provider(
    client: &reqwest::Client,
    base: &str,
    key: &str,
    timeout: Duration,
) -> Result<BTreeMap<String, Hints>, String> {
    if endpoint(base).is_none() {
        return Err("Invalid upstream URL".into());
    }
    let url = format!("{}/models", base.trim_end_matches('/'));
    let bytes = download(client.get(url).bearer_auth(key), LIVE_LIMIT, timeout).await?;
    let data: Value = serde_json::from_slice(&bytes).map_err(|_| "Invalid model list")?;
    let rows = data["data"]
        .as_array()
        .ok_or("Expected OpenAI models data array")?;
    let mut models = BTreeMap::new();
    for row in rows {
        let id = row["id"]
            .as_str()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or("Invalid model list")?;
        if id.len() > 160 || id.chars().any(char::is_control) {
            return Err("Invalid model ID".into());
        }
        models.insert(id.to_owned(), Hints::parse(row));
        if models.len() > 4096 {
            return Err("At most 4096 discovered models; upstream returned too many".into());
        }
    }
    if models.is_empty() {
        return Err("No available models returned".into());
    }
    Ok(models)
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
