use crate::{
    config::{Config, Connection, Provider},
    session::{Envelope, Sessions},
    stats::{
        self,
        collector::Protocol,
        dto::{ErrorKind, Outcome, Source, UsageMode},
    },
};
use axum::{
    Router,
    body::{Body, to_bytes},
    extract::State,
    http::{HeaderMap, Request, Response, StatusCode},
    routing::{get, post},
};
use futures_util::{StreamExt, stream};
use serde_json::json;
use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU16, AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::sync::{Semaphore, oneshot};
#[path = "proxy_native.rs"]
mod native;

pub const BODY_LIMIT: usize = 8 * 1024 * 1024;
pub const MAX_CONCURRENT: usize = 4;
struct Destination {
    endpoint: reqwest::Url,
    authorization: axum::http::HeaderValue,
    go: bool,
    has_key: bool,
    protocol: crate::upstream_config::UpstreamProtocol,
    settings: crate::upstream_config::ProtocolSettings,
    base: String,
    native_key: axum::http::HeaderValue,
    fingerprint: [u8; 32],
    generation: [u8; 32],
}
struct Route {
    connection: Connection,
    destination: Arc<Destination>,
    provider_id: String,
    fingerprint: [u8; 32],
    generation: [u8; 32],
}
#[derive(Default)]
struct Routes {
    ids: std::collections::HashMap<String, Arc<Route>>,
    aliases: std::collections::HashMap<String, Arc<Route>>,
    models: std::collections::HashMap<String, Vec<Arc<Route>>>,
}
impl Routes {
    fn new(cfg: &Config) -> Result<Self, String> {
        Self::rebuild(cfg, None)
    }
    fn rebuild(cfg: &Config, previous: Option<&Routes>) -> Result<Self, String> {
        let mut routes = Self::default();
        for p in &cfg.providers {
            if p.models.is_empty() {
                continue;
            }
            let c = Connection {
                upstream: p.upstream.clone(),
                protocol: p.protocol,
                protocol_settings: p.protocol_settings.clone(),
                ..Default::default()
            };
            let mut authorization: axum::http::HeaderValue = format!("Bearer {}", p.api_key)
                .parse()
                .map_err(|_| "Invalid API key")?;
            authorization.set_sensitive(true);
            let mut native_key: axum::http::HeaderValue =
                p.api_key.parse().map_err(|_| "Invalid API key")?;
            native_key.set_sensitive(true);
            use sha2::{Digest, Sha256};
            let fingerprint: [u8; 32] = Sha256::digest(
                serde_json::to_vec(&(
                    &p.id,
                    &p.upstream,
                    &p.api_key,
                    p.protocol,
                    &p.protocol_settings,
                ))
                .map_err(|_| "Invalid protocol settings")?,
            )
            .into();
            let generation = previous
                .and_then(|old| {
                    old.ids
                        .values()
                        .find(|r| r.provider_id == p.id && r.destination.fingerprint == fingerprint)
                })
                .map(|r| r.destination.generation)
                .unwrap_or_else(|| Sha256::digest(crate::config::token().as_bytes()).into());
            let destination = Arc::new(Destination {
                endpoint: c.endpoint().parse().map_err(|_| "Invalid upstream URL")?,
                authorization,
                go: c.is_go(),
                has_key: !p.api_key.is_empty(),
                protocol: p.protocol,
                settings: p.protocol_settings.clone(),
                base: p.upstream.clone(),
                native_key,
                fingerprint,
                generation,
            });
            for m in &p.models {
                let connection = Connection {
                    id: m.id.clone(),
                    alias: m.alias.clone(),
                    name: m.name.clone(),
                    provider: p.name.clone(),
                    model: m.model.clone(),
                    reasoning_effort: m.reasoning_effort.clone(),
                    capabilities: m.capabilities.clone(),
                    api_key: String::new(),
                    upstream: String::new(),
                    protocol: p.protocol,
                    protocol_settings: p.protocol_settings.clone(),
                };
                let fingerprint: [u8; 32] = Sha256::digest(
                    serde_json::to_vec(&(
                        destination.generation,
                        &m.id,
                        &m.model,
                        &m.reasoning_effort,
                        &m.capabilities,
                    ))
                    .map_err(|_| "Invalid route settings")?,
                )
                .into();
                let generation = previous
                    .and_then(|old| old.ids.get(&m.id))
                    .filter(|r| r.fingerprint == fingerprint)
                    .map(|r| r.generation)
                    .unwrap_or_else(|| Sha256::digest(crate::config::token().as_bytes()).into());
                let route = Arc::new(Route {
                    connection,
                    destination: destination.clone(),
                    provider_id: p.id.clone(),
                    fingerprint,
                    generation,
                });
                routes.ids.insert(m.id.clone(), route.clone());
                routes.aliases.insert(m.alias.clone(), route.clone());
                routes
                    .aliases
                    .insert(format!("wbp-{}", m.id), route.clone());
                routes
                    .models
                    .entry(m.model.clone())
                    .or_default()
                    .push(route);
            }
        }
        for c in cfg.workbuddy_connections()? {
            let route = routes.ids[&c.id].clone();
            routes.aliases.insert(c.alias, route);
        }
        // Saved names may change before the user resyncs WorkBuddy. Keep its
        // currently installed IDs usable on the same stable, scoped routes.
        for record in &cfg.installed {
            if let Some(id) = cfg.installed_connection_id(record) {
                let route = routes.ids[&id].clone();
                if routes
                    .aliases
                    .get(&record.id)
                    .is_some_and(|old| old.connection.id != id)
                {
                    return Err("Installed model ID conflicts with another route".into());
                }
                routes.aliases.insert(record.id.clone(), route);
            }
        }
        Ok(routes)
    }
    fn resolve(&self, scope: Option<&str>, model: &str) -> Result<Arc<Route>, String> {
        if let Some(id) = scope
            && let Some(r) = self.ids.get(id)
        {
            return if model == r.connection.alias
                || model == r.connection.model
                || model == format!("wbp-{}", r.connection.id)
                || self
                    .aliases
                    .get(model)
                    .is_some_and(|alias| alias.connection.id == r.connection.id)
            {
                Ok(r.clone())
            } else {
                Err("Model does not belong to this connection".into())
            };
        }
        let in_scope = |r: &&Arc<Route>| scope.is_none_or(|id| r.provider_id == id);
        if let Some(r) = self.aliases.get(model).filter(in_scope) {
            // A v0.1 alias can be identical to its raw model ID. It must not
            // silently win over another account when a same-ID model is added.
            if r.connection.model == model
                && self
                    .models
                    .get(model)
                    .into_iter()
                    .flatten()
                    .filter(in_scope)
                    .count()
                    > 1
            {
                return Err("Ambiguous model ID; use its independent model URL / 同名模型请使用独立模型 URL".into());
            }
            return Ok(r.clone());
        }
        let mut matching = self
            .models
            .get(model)
            .into_iter()
            .flatten()
            .filter(in_scope);
        let first = matching
            .next()
            .ok_or("Unknown model alias / 未知模型标识")?;
        if matching.next().is_some() {
            return Err("Ambiguous model ID; use its alias / 同名模型请使用独立列表项".into());
        }
        Ok(first.clone())
    }
}
pub struct Bridge {
    pub config: Mutex<Config>,
    routes: Mutex<Routes>,
    pub catalog: Arc<crate::catalog::Catalog>,
    pub dir: PathBuf,
    pub client: reqwest::Client,
    sessions: Mutex<Sessions>,
    permits: Arc<Semaphore>,
    pub port: AtomicU16,
    pub calls: AtomicU64,
    pub status: Mutex<String>,
    pub logs: Mutex<std::collections::VecDeque<(u64, String)>>,
    started: std::time::Instant,
    pub management_key: String,
    update_busy: AtomicBool,
    pub stop: Mutex<Option<oneshot::Sender<()>>>,
    pub port_changes: Mutex<Option<tokio::sync::mpsc::Sender<crate::port::Request>>>,
    mutations: tokio::sync::Mutex<()>,
    /// Local usage statistics. Never holds a SQLite connection and never blocks
    /// a chat request.
    pub stats: stats::StatsHandle,
    usage_injection_warned: Mutex<std::collections::HashSet<String>>,
    native_replay: Mutex<crate::protocol::replay::Cache>,
}
impl Bridge {
    pub fn new(config: Config, dir: PathBuf) -> Result<Self, String> {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(15))
            .read_timeout(Duration::from_secs(180))
            .pool_max_idle_per_host(2)
            .pool_idle_timeout(Duration::from_secs(60))
            .tcp_nodelay(true)
            .build()
            .map_err(|e| e.to_string())?;
        let stats = stats::StatsHandle::start(dir.clone());
        Ok(Self {
            catalog: Arc::new(crate::catalog::Catalog::load(&dir)),
            routes: Mutex::new(Routes::new(&config)?),
            config: Mutex::new(config),
            dir,
            client,
            sessions: Mutex::new(Sessions::default()),
            native_replay: Mutex::new(crate::protocol::replay::Cache::default()),
            permits: Arc::new(Semaphore::new(MAX_CONCURRENT)),
            port: AtomicU16::new(0),
            calls: AtomicU64::new(0),
            status: Mutex::new("Ready / 就绪".into()),
            logs: Mutex::new(std::collections::VecDeque::new()),
            started: std::time::Instant::now(),
            management_key: crate::config::token(),
            update_busy: AtomicBool::new(false),
            stop: Mutex::new(None),
            port_changes: Mutex::new(None),
            mutations: tokio::sync::Mutex::new(()),
            stats,
            usage_injection_warned: Mutex::new(std::collections::HashSet::new()),
        })
    }
    pub fn connection_address(&self, id: &str) -> String {
        format!(
            "http://127.0.0.1:{}/r/{id}/v1",
            self.port.load(Ordering::Relaxed)
        )
    }
    pub fn sanitize(&self, text: String) -> String {
        let cfg = self.config.lock().unwrap();
        let redact = |mut value: String| {
            for secret in std::iter::once(&cfg.local_key)
                .chain(std::iter::once(&self.management_key))
                .chain(cfg.providers.iter().map(|p| &p.api_key))
                .filter(|s| !s.is_empty())
            {
                value = value.replace(secret, "[redacted]");
            }
            value
                .chars()
                .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
                .take(4096)
                .collect::<String>()
        };
        let (prefix, body) = text
            .strip_prefix("ERROR ")
            .map_or(("", text.as_str()), |body| ("ERROR ", body));
        if let Ok(mut report) = serde_json::from_str::<crate::diagnostic::TestReport>(body) {
            for field in [
                &mut report.provider,
                &mut report.name,
                &mut report.model,
                &mut report.url,
                &mut report.reasoning,
                &mut report.http,
                &mut report.reason,
            ] {
                *field = redact(std::mem::take(field));
            }
            report.response_model = report.response_model.map(redact);
            return format!("{prefix}{}", report.json());
        }
        redact(text)
    }
    pub fn set_status(&self, text: String) {
        *self.status.lock().unwrap() = text.clone();
        let mut logs = self.logs.lock().unwrap();
        if logs.len() == 100 {
            logs.pop_front();
        }
        logs.push_back((crate::journal::now_ms(), text));
    }
    pub fn address(&self) -> String {
        format!("http://127.0.0.1:{}/v1", self.port.load(Ordering::Relaxed))
    }
    pub fn routes(self: Arc<Self>) -> Router {
        Router::new()
            .route("/health", get(health))
            .route("/v1/models", get(models))
            .route("/v1/chat/completions", post(relay))
            .route("/r/{id}/v1/chat/completions", post(relay_connection))
            .route("/r/{id}/v1/models", get(connection_models))
            .route("/_control/show", post(show))
            .route("/_control/ping", post(ping))
            .route("/_control/action", post(action))
            .route("/_control/stop", post(stop))
            .with_state(self)
    }
    async fn upstream(
        &self,
        r: &Route,
        headers: &HeaderMap,
        body: bytes::Bytes,
        session: &str,
        query: Option<&str>,
    ) -> Result<reqwest::Response, reqwest::Error> {
        let mut endpoint = r.destination.endpoint.clone();
        endpoint.set_query(query);
        let mut req = self
            .client
            .post(endpoint)
            .headers(forward_headers(headers, true))
            .header("Authorization", r.destination.authorization.clone())
            .body(body);
        if !headers.contains_key("content-type") {
            req = req.header("Content-Type", "application/json");
        }
        if r.destination.go {
            req = req.header("x-opencode-session", session).header(
                "User-Agent",
                concat!("WorkBuddy-Proxy/", env!("CARGO_PKG_VERSION")),
            );
        }
        req.send().await
    }
    async fn discover(&self, id: &str, payload: &serde_json::Value) -> Result<String, String> {
        let (base, key, protocol, settings) = {
            let cfg = self.config.lock().unwrap();
            let provider = cfg.provider(id)?;
            (
                payload["upstream"]
                    .as_str()
                    .unwrap_or(&provider.upstream)
                    .trim()
                    .to_owned(),
                payload["key"]
                    .as_str()
                    .filter(|s| !s.trim().is_empty())
                    .unwrap_or(&provider.api_key)
                    .trim()
                    .to_owned(),
                payload
                    .get("protocol")
                    .map(|v| serde_json::from_value(v.clone()).map_err(|_| "Invalid protocol"))
                    .transpose()?
                    .unwrap_or(provider.protocol),
                payload
                    .get("protocol_settings")
                    .map(|v| {
                        serde_json::from_value(v.clone()).map_err(|_| "Invalid protocol settings")
                    })
                    .transpose()?
                    .unwrap_or_else(|| provider.protocol_settings.clone()),
            )
        };
        if key.is_empty() {
            return Err("Enter upstream API key".into());
        }
        let _permit = self
            .permits
            .clone()
            .try_acquire_owned()
            .map_err(|_| "Four requests are already in flight")?;
        let models = crate::catalog::fetch_protocol(
            &self.client,
            &base,
            &key,
            protocol,
            &settings,
            Duration::from_secs(30),
        )
        .await?;
        let ids: Vec<_> = models.keys().cloned().collect();
        self.catalog
            .remember_async(base, protocol, key, models)
            .await;
        Ok(json!(ids).to_string())
    }
    /// Reports, once per provider, that `include_usage` could not be merged
    /// because the request carried a non-object `stream_options`.
    fn warn_usage_injection(&self, provider_id: &str) {
        if self
            .usage_injection_warned
            .lock()
            .unwrap()
            .insert(provider_id.to_owned())
        {
            eprintln!(
                "usage_mode=request could not merge stream_options for provider {provider_id}; the chat request was forwarded unchanged"
            );
        }
    }
    fn metadata_targets(&self) -> Vec<crate::catalog::Target> {
        self.config
            .lock()
            .unwrap()
            .providers
            .iter()
            .filter(|p| p.models.iter().any(|m| m.capabilities.is_none()))
            .map(|p| crate::catalog::Target {
                upstream: p.upstream.clone(),
                key: p.api_key.clone(),
                protocol: p.protocol,
                settings: p.protocol_settings.clone(),
            })
            .collect()
    }
    pub fn warm_metadata(&self) {
        let catalog = self.catalog.clone();
        let client = self.client.clone();
        let targets = self.metadata_targets();
        tokio::spawn(async move {
            catalog.refresh(&client, targets).await;
        });
    }
    async fn prepare_metadata(&self) {
        // Finish a first refresh when possible, with a hard bound on sync delay.
        let _ = tokio::time::timeout(
            Duration::from_secs(6),
            self.catalog.refresh(&self.client, self.metadata_targets()),
        )
        .await;
    }
    pub async fn test(&self, id: &str) -> Result<String, String> {
        let c = self.config.lock().unwrap().connection(id)?;
        let mut report = crate::diagnostic::TestReport::from_connection(&c);
        let started = std::time::Instant::now();
        let outcome: Result<(), String> = async {
            let r = self.routes.lock().unwrap().resolve(Some(&c.id), &c.model)?;
            let _permit = self.permits.clone().try_acquire_owned()
                .map_err(|_| "Four requests are already in flight")?;
            // Connection tests are recorded with their own source so the normal
            // statistics views can exclude them by default.
            let mut guard = self.stats.begin(
                stats::RequestSpec {
                    source: Source::ConnectionTest,
                    provider_id: r.provider_id.clone(),
                    provider_name: r.connection.provider.clone(),
                    route_id: r.connection.id.clone(),
                    route_name: r.connection.name.clone(),
                    requested_model: c.model.clone(),
                    upstream_model: r.connection.model.clone(),
            upstream_protocol:r.destination.protocol.as_str().into(),
                    stream: true,
                },
                Protocol::Sse,
                false,
            );
            if !r.destination.has_key {
                if let Some(guard) = guard.as_mut() {
                    guard.finish(Outcome::Rejected, Some(ErrorKind::MissingKey));
                }
                return Err("Save your API key first / 请先保存 API Key".into());
            }
            if r.destination.protocol.is_native(){
                return native::test(self,&r,&mut guard,&mut report,started).await;
            }
            let payload = serde_json::to_vec(&json!({"model":c.model,"stream":true,"max_tokens":1,"messages":[{"role":"user","content":"Reply with OK."}]})).unwrap();
            let payload = match apply_reasoning(payload.into(), &c.reasoning_effort) {
                Ok(payload) => payload,
                Err(error) => {
                    if let Some(guard) = guard.as_mut() {
                        guard.finish(Outcome::Rejected, Some(ErrorKind::Other));
                    }
                    return Err(error);
                }
            };
            if let Some(guard) = guard.as_mut() {
                guard.mark_upstream_started();
            }
            let res = match tokio::time::timeout(Duration::from_secs(45), self.upstream(&r, &HeaderMap::new(), payload, &crate::config::token(), None)).await {
                Ok(Ok(res)) => res,
                Ok(Err(_)) => {
                    if let Some(guard) = guard.as_mut() {
                        guard.finish(Outcome::Failed, Some(ErrorKind::Connect));
                    }
                    return Err("Connection failed; check network and proxy / 请检查网络和代理".into());
                }
                Err(_) => {
                    if let Some(guard) = guard.as_mut() {
                        guard.finish(Outcome::Failed, Some(ErrorKind::Connect));
                    }
                    return Err("Connection timeout / 连接超时".into());
                }
            };
            report.headers_ms = Some(started.elapsed().as_millis() as u64);
            let status = res.status();
            report.http = format!("HTTP {}", status.as_u16());
            if !status.is_success() {
                if let Some(guard) = guard.as_mut() {
                    guard.set_http_status(status.as_u16());
                    guard.finish(Outcome::Failed, Some(ErrorKind::UpstreamHttp));
                }
                return Err("Upstream rejected the test request / 上游拒绝了测试请求".into());
            }
            if let Some(guard) = guard.as_mut() {
                guard.set_http_status(status.as_u16());
                guard.attach_collector(
                    Protocol::classify(header_text(res.headers(), "content-type").as_deref()),
                    Protocol::is_encoded(header_text(res.headers(), "content-encoding").as_deref()),
                );
            }
            let probe = tokio::time::timeout(Duration::from_secs(45), async {
                let mut stream = res.bytes_stream();
                let mut pending = Vec::new();
                while let Some(chunk) = stream.next().await {
                    let b = chunk.map_err(|_| "Invalid upstream stream")?;
                    // Observe the same bytes that are being discarded below.
                    if let Some(guard) = guard.as_mut() {
                        guard.observe(&b);
                    }
                    if pending.len() + b.len() > 65536 { return Err("Invalid upstream response"); }
                    pending.extend_from_slice(&b);
                    while let Some(i) = pending.iter().position(|b| *b == b'\n') {
                        let line = String::from_utf8_lossy(&pending[..i]);
                        if let Some(text) = line.trim().strip_prefix("data:").map(str::trim)
                            && let Ok(v) = serde_json::from_str::<serde_json::Value>(text)
                        {
                            if v.get("error").is_some() {
                                if let Some(guard) = guard.as_mut() {
                                    guard.finish(Outcome::Failed, Some(ErrorKind::SseError));
                                }
                                return Err("Upstream returned an error");
                            }
                            if v["choices"].as_array().is_some_and(|choices| !choices.is_empty()) {
                                report.first_event_ms = Some(started.elapsed().as_millis() as u64);
                                report.response_model = v["model"].as_str().map(|m| m.chars().filter(|c| !c.is_control()).take(160).collect());
                                // The probe intentionally stops at the first
                                // event; statistics record that as a cancelled
                                // probe, never as a failed connection test.
                                if let Some(guard) = guard.as_mut() {
                                    guard.finish(Outcome::Cancelled, Some(ErrorKind::ClientDrop));
                                }
                                return Ok(());
                            }
                        }
                        pending.drain(..=i);
                    }
                }
                Err("No valid streaming response")
            }).await;
            let result = match probe {
                Ok(Ok(())) => Ok(()),
                Ok(Err(reason)) => {
                    if let Some(guard) = guard.as_mut() {
                        guard.finish(Outcome::Failed, Some(ErrorKind::UpstreamStream));
                    }
                    Err(reason)
                }
                Err(_) => {
                    if let Some(guard) = guard.as_mut() {
                        guard.finish(Outcome::Failed, Some(ErrorKind::UpstreamStream));
                    }
                    Err("Stream timeout / 流式响应超时")
                }
            };
            result.map_err(str::to_string)
        }.await;
        report.elapsed_ms = started.elapsed().as_millis() as u64;
        if let Err(reason) = &outcome {
            report.reason = reason.clone();
        }
        let detail = self.sanitize(report.json());
        outcome.map(|_| detail.clone()).map_err(|_| detail)
    }

    /// Publishes the provider list used by the statistics filter options.
    pub fn refresh_stats_providers(&self) {
        let cfg = self.config.lock().unwrap();
        self.stats.set_configured_providers(
            cfg.providers
                .iter()
                .map(|p| (p.id.clone(), p.name.clone()))
                .collect(),
        );
    }
}
// Remove hop-by-hop fields, including names nominated by Connection. Never
// forward the local bearer token or the local Host to an upstream provider.
fn forward_headers(headers: &HeaderMap, request: bool) -> HeaderMap {
    let nominated: Vec<_> = headers
        .get_all("connection")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|s| s.split(',').map(|s| s.trim().to_ascii_lowercase()))
        .collect();
    let mut out = HeaderMap::new();
    for (name, value) in headers {
        if matches!(
            name.as_str(),
            "connection"
                | "keep-alive"
                | "proxy-authenticate"
                | "proxy-authorization"
                | "te"
                | "trailer"
                | "transfer-encoding"
                | "upgrade"
        ) || nominated.iter().any(|n| n == name.as_str())
            || (request
                && matches!(
                    name.as_str(),
                    "authorization" | "host" | "content-length" | "x-api-key" | "x-goog-api-key"
                ))
        {
            continue;
        }
        out.append(name.clone(), value.clone());
    }
    out
}
fn fail(status: StatusCode, message: &str) -> Response<Body> {
    Response::builder()
        .status(status)
        .header("Content-Type", "application/json")
        .body(Body::from(
            json!({"error":{"message":message,"type":"proxy_error"}}).to_string(),
        ))
        .unwrap()
}
fn local(headers: &HeaderMap, port: u16) -> bool {
    let host = headers
        .get("host")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let expected = format!("127.0.0.1:{port}");
    let localname = format!("localhost:{port}");
    if host != expected && host != localname {
        return false;
    }
    headers.get("origin").is_none_or(|v| {
        v.to_str()
            .is_ok_and(|origin| origin == format!("http://{host}"))
    })
}
pub fn authorized(headers: &HeaderMap, key: &str) -> bool {
    let value = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("");
    // Compare fixed-size digests to avoid content-dependent token comparison.
    use sha2::{Digest, Sha256};
    let a = Sha256::digest(value.as_bytes());
    let b = Sha256::digest(key.as_bytes());
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    !key.is_empty() && diff == 0
}
async fn health(State(a): State<Arc<Bridge>>, headers: HeaderMap) -> Response<Body> {
    if !local(&headers, a.port.load(Ordering::Relaxed)) {
        return fail(StatusCode::FORBIDDEN, "Invalid host or origin");
    };
    Response::builder()
        .header("Content-Type", "application/json")
        .body(Body::from(
            json!({"app":"workbuddy-proxy","version":env!("CARGO_PKG_VERSION"),"control_protocol":crate::CONTROL_PROTOCOL,"ready":true})
                .to_string(),
        ))
        .unwrap()
}
async fn models(State(a): State<Arc<Bridge>>, headers: HeaderMap) -> Response<Body> {
    model_list(&a, &headers, None)
}
async fn connection_models(
    State(a): State<Arc<Bridge>>,
    axum::extract::Path(id): axum::extract::Path<String>,
    headers: HeaderMap,
) -> Response<Body> {
    model_list(&a, &headers, Some(&id))
}
fn model_list(a: &Bridge, headers: &HeaderMap, id: Option<&str>) -> Response<Body> {
    let cfg = a.config.lock().unwrap();
    if !local(headers, a.port.load(Ordering::Relaxed)) || !authorized(headers, &cfg.local_key) {
        return fail(StatusCode::UNAUTHORIZED, "Invalid local API key");
    }
    if id.is_some_and(|id| {
        !cfg.providers
            .iter()
            .any(|p| p.id == id || p.models.iter().any(|m| m.id == id))
    }) {
        return fail(StatusCode::NOT_FOUND, "Unknown connection");
    }
    let connections = match cfg.workbuddy_connections() {
        Ok(connections) => connections,
        Err(e) => return fail(StatusCode::INTERNAL_SERVER_ERROR, &e),
    };
    let entries: Vec<_> = connections.iter().filter(|c| !c.api_key.is_empty())
        .filter(|c| id.is_none_or(|id| c.id == id || cfg.provider(&c.id).is_ok_and(|p| p.id == id)))
        .map(|c| json!({"id":c.alias,"object":"model","owned_by":c.provider,"name":format!("{} · {} · {}",c.name,c.provider,c.model)}))
        .collect();
    Response::builder()
        .header("Content-Type", "application/json")
        .body(Body::from(
            json!({"object":"list","data":entries}).to_string(),
        ))
        .unwrap()
}
async fn relay(State(a): State<Arc<Bridge>>, request: Request<Body>) -> Response<Body> {
    relay_to(a, request, None).await
}
async fn relay_connection(
    State(a): State<Arc<Bridge>>,
    axum::extract::Path(id): axum::extract::Path<String>,
    request: Request<Body>,
) -> Response<Body> {
    relay_to(a, request, Some(id)).await
}
async fn relay_to(a: Arc<Bridge>, request: Request<Body>, route: Option<String>) -> Response<Body> {
    let (parts, body) = request.into_parts();
    if !local(&parts.headers, a.port.load(Ordering::Relaxed)) {
        return fail(StatusCode::FORBIDDEN, "Invalid host or origin");
    }
    if !authorized(&parts.headers, &a.config.lock().unwrap().local_key) {
        return fail(
            StatusCode::UNAUTHORIZED,
            "Use the local API key from WorkBuddy Proxy settings",
        );
    }
    let permit = match a.permits.clone().try_acquire_owned() {
        Ok(p) => p,
        Err(_) => {
            return fail(
                StatusCode::TOO_MANY_REQUESTS,
                "Four requests are already in flight; retry when a slot is free",
            );
        }
    };
    // The deadline also prevents a caller with a valid key from holding an input slot forever.
    let bytes =
        match tokio::time::timeout(Duration::from_secs(30), to_bytes(body, BODY_LIMIT)).await {
            Ok(Ok(b)) => b,
            _ => {
                return fail(
                    StatusCode::PAYLOAD_TOO_LARGE,
                    "Request exceeds 8 MiB or input read timed out",
                );
            }
        };
    #[derive(serde::Deserialize)]
    struct ModelField<'a> {
        #[serde(borrow)]
        model: std::borrow::Cow<'a, str>,
        #[serde(default)]
        stream: bool,
    }
    let model: ModelField<'_> = match serde_json::from_slice(&bytes) {
        Ok(e) => e,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "A model string is required"),
    };
    let r = match a
        .routes
        .lock()
        .unwrap()
        .resolve(route.as_deref(), &model.model)
    {
        Ok(r) => r,
        Err(e) => return fail(StatusCode::BAD_REQUEST, &e),
    };
    // Statistics admission happens only after a valid route exists. Anything
    // refused earlier (host, auth, permit, body, routing) writes no row.
    let mut stats_guard = a.stats.begin(
        stats::RequestSpec {
            source: Source::Relay,
            provider_id: r.provider_id.clone(),
            provider_name: r.connection.provider.clone(),
            route_id: r.connection.id.clone(),
            route_name: r.connection.name.clone(),
            requested_model: model.model.to_string(),
            upstream_model: r.connection.model.clone(),
            upstream_protocol: r.destination.protocol.as_str().into(),
            stream: model.stream,
        },
        Protocol::Unknown,
        false,
    );
    if !r.destination.has_key {
        if let Some(guard) = stats_guard.as_mut() {
            guard.finish(Outcome::Rejected, Some(ErrorKind::MissingKey));
        }
        return fail(
            StatusCode::SERVICE_UNAVAILABLE,
            "Save this provider's API key first",
        );
    }
    let c = &r.connection;
    if r.destination.protocol.is_native() {
        return native::relay(
            a,
            r,
            bytes,
            parts.headers,
            parts.uri.query().map(str::to_owned),
            permit,
            stats_guard,
        )
        .await;
    }
    let session = if r.destination.go {
        let envelope: Envelope<'_> = match serde_json::from_slice(&bytes) {
            Ok(e) => e,
            Err(_) => {
                if let Some(guard) = stats_guard.as_mut() {
                    guard.finish(Outcome::Rejected, Some(ErrorKind::Other));
                }
                return fail(StatusCode::BAD_REQUEST, "Go requires a messages array");
            }
        };
        format!(
            "{}-{}",
            c.id,
            a.sessions
                .lock()
                .unwrap()
                .resolve(&parts.headers, &envelope)
        )
    } else {
        String::new()
    };
    let replace_model = (model.model != c.model).then_some(c.model.as_str());
    // Only an explicitly configured provider may ask the upstream for usage.
    let inject_usage = a.stats.usage_mode(&r.provider_id) == UsageMode::Request;
    let bytes = match rewrite_request(bytes, replace_model, &c.reasoning_effort, inject_usage) {
        Ok(outcome) => {
            if outcome.injection_failed {
                a.warn_usage_injection(&r.provider_id);
            }
            outcome.bytes
        }
        Err(e) => {
            if let Some(guard) = stats_guard.as_mut() {
                guard.finish(Outcome::Rejected, Some(ErrorKind::Other));
            }
            return fail(StatusCode::BAD_REQUEST, &e);
        }
    };
    if let Some(guard) = stats_guard.as_mut() {
        guard.mark_upstream_started();
    }
    let res = match a
        .upstream(&r, &parts.headers, bytes, &session, parts.uri.query())
        .await
    {
        Ok(r) => r,
        Err(_) => {
            if let Some(guard) = stats_guard.as_mut() {
                guard.finish(Outcome::Failed, Some(ErrorKind::Connect));
            }
            a.set_status("Network error / 网络错误".into());
            return fail(
                StatusCode::BAD_GATEWAY,
                "Upstream connection failed; check network and proxy settings",
            );
        }
    };
    a.calls.fetch_add(1, Ordering::Relaxed);
    a.set_status(format!(
        "HTTP {} · {} requests",
        res.status().as_u16(),
        a.calls.load(Ordering::Relaxed)
    ));
    let status = res.status();
    if let Some(guard) = stats_guard.as_mut() {
        guard.set_http_status(status.as_u16());
        guard.set_content_length(
            header_text(res.headers(), "content-length")
                .and_then(|value| value.trim().parse().ok()),
        );
        guard.attach_collector(
            Protocol::classify(header_text(res.headers(), "content-type").as_deref()),
            Protocol::is_encoded(header_text(res.headers(), "content-encoding").as_deref()),
        );
    }
    let mut response = Response::builder().status(status);
    *response.headers_mut().unwrap() = forward_headers(res.headers(), false);
    // The original bytes, status, back pressure and cancellation semantics are
    // untouched: every chunk is observed and then yielded as the same buffer.
    // The permit and the guard travel with the stream, so a client cancellation
    // releases both exactly once.
    let stream = stream::unfold(
        (res.bytes_stream(), permit, stats_guard),
        move |(mut source, permit, mut guard)| async move {
            match source.next().await {
                Some(Ok(chunk)) => {
                    if let Some(guard) = guard.as_mut() {
                        guard.observe(&chunk);
                    }
                    Some((Ok(chunk), (source, permit, guard)))
                }
                Some(Err(error)) => {
                    if let Some(guard) = guard.as_mut() {
                        guard.finish(Outcome::Failed, Some(ErrorKind::UpstreamStream));
                    }
                    Some((Err(error), (source, permit, guard)))
                }
                None => {
                    if let Some(guard) = guard.as_mut() {
                        let (outcome, kind) = guard.terminal();
                        guard.finish(outcome, kind);
                    }
                    None
                }
            }
        },
    );
    response.body(Body::from_stream(stream)).unwrap()
}
fn header_text(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
}
async fn show(State(a): State<Arc<Bridge>>, headers: HeaderMap) -> Response<Body> {
    if !local(&headers, a.port.load(Ordering::Relaxed)) || !authorized(&headers, &a.management_key)
    {
        return fail(StatusCode::UNAUTHORIZED, "Unauthorized");
    };
    crate::platform::show();
    Response::new(Body::from("OK"))
}
async fn stop(State(a): State<Arc<Bridge>>, headers: HeaderMap) -> Response<Body> {
    if !local(&headers, a.port.load(Ordering::Relaxed)) || !authorized(&headers, &a.management_key)
    {
        return fail(StatusCode::UNAUTHORIZED, "Unauthorized");
    };
    if let Some(tx) = a.stop.lock().unwrap().take() {
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            let _ = tx.send(());
        });
    }
    Response::new(Body::from("OK"))
}
async fn ping(State(a): State<Arc<Bridge>>, headers: HeaderMap) -> Response<Body> {
    if !local(&headers, a.port.load(Ordering::Relaxed)) || !authorized(&headers, &a.management_key)
    {
        return fail(StatusCode::UNAUTHORIZED, "Unauthorized");
    }
    Response::new(Body::from("workbuddy-proxy"))
}
async fn action(State(a): State<Arc<Bridge>>, request: Request<Body>) -> Response<Body> {
    let (parts, body) = request.into_parts();
    if !local(&parts.headers, a.port.load(Ordering::Relaxed))
        || !authorized(&parts.headers, &a.management_key)
    {
        return fail(StatusCode::UNAUTHORIZED, "Unauthorized");
    }
    let bytes = match to_bytes(body, 131072).await {
        Ok(b) => b,
        Err(_) => return fail(StatusCode::PAYLOAD_TOO_LARGE, "Control payload too large"),
    };
    let p: serde_json::Value = match serde_json::from_slice(&bytes) {
        Ok(p) => p,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "Invalid control payload"),
    };
    let result = a.command(&p).await;
    let (status, message) = match result {
        Ok(m) => (StatusCode::OK, m),
        Err(m) => (StatusCode::BAD_REQUEST, m),
    };
    Response::builder()
        .status(status)
        .header("Content-Type", "application/json")
        .body(Body::from(json!({"message":message}).to_string()))
        .unwrap()
}
impl Bridge {
    pub async fn command(&self, p: &serde_json::Value) -> Result<String, String> {
        // Statistics commands are dispatched before the shared status pipeline:
        // they never set the proxy status string and never reload the config.
        if let Some(command) = p["command"].as_str()
            && stats::StatsHandle::is_silent_command(command)
        {
            // The GUI caps a control reply at 1 MiB; refuse instead of cutting JSON.
            return self.stats.command(p).await.and_then(|message| {
                if message.len() > stats::dto::MAX_RESULT_BYTES {
                    Err(stats::dto::error(
                        stats::dto::ERR_RESULT_TOO_LARGE,
                        "statistics result exceeds 1 MiB; narrow the range",
                    ))
                } else {
                    Ok(message)
                }
            });
        }
        let _mutation = if matches!(
            p["command"].as_str(),
            Some(
                "save"
                    | "add"
                    | "duplicate"
                    | "delete"
                    | "language"
                    | "install"
                    | "restore"
                    | "set_port"
                    | "autostart"
            )
        ) {
            Some(self.mutations.lock().await)
        } else {
            None
        };
        let result = self.command_inner(p).await;
        if result.is_ok() && p["command"] == "save" {
            self.warm_metadata();
        }
        if result.is_ok()
            && matches!(
                p["command"].as_str(),
                Some("save" | "add" | "duplicate" | "delete")
            )
        {
            self.refresh_stats_providers();
        }
        if !matches!(
            p["command"].as_str(),
            Some("snapshot" | "copy" | "check_update" | "record" | "clear_logs" | "model_metadata")
        ) {
            let status = match &result {
                Ok(s) if p["command"] == "fetch" => {
                    let count = serde_json::from_str::<Vec<String>>(s).map_or(0, |m| m.len());
                    format!("Fetched {count} models")
                }
                Ok(s) if p["command"] == "set_port" => {
                    serde_json::from_str::<crate::port::Applied>(s)
                        .map(|v| v.message())
                        .unwrap_or_else(|_| "Port updated".into())
                }
                Ok(s) => s.clone(),
                Err(e) => format!("ERROR {e}"),
            };
            let status = self.sanitize(status);
            if p["ui"].as_bool() == Some(true) {
                *self.status.lock().unwrap() = status;
            } else {
                self.set_status(status);
            }
        }
        result
    }
    async fn command_inner(&self, p: &serde_json::Value) -> Result<String, String> {
        let command = p
            .get("command")
            .and_then(serde_json::Value::as_str)
            .ok_or("Missing command")?;
        let id = p
            .get("provider_id")
            .or_else(|| p.get("connection_id"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        match command {
            "clear_logs" => {
                self.logs.lock().unwrap().clear();
                *self.status.lock().unwrap() = "Ready / 就绪".into();
                Ok("Logs cleared / 日志已清空".into())
            }
            "record" => {
                let message = p["message"].as_str().filter(|s| !s.is_empty() && s.len() <= 16384).ok_or("Invalid log entry")?;
                self.set_status(self.sanitize(message.into()));
                Ok("Recorded".into())
            }
            "set_port" => {
                let port = p["port"]
                    .as_u64()
                    .and_then(|n| u16::try_from(n).ok())
                    .filter(|n| *n > 0)
                    .ok_or("Enter a port from 1 to 65535 / 请输入 1–65535 的端口号")?;
                let sender = self
                    .port_changes
                    .lock()
                    .unwrap()
                    .clone()
                    .ok_or("Listener is unavailable / 监听服务不可用")?;
                let (reply, result) = oneshot::channel();
                sender
                    .send(crate::port::Request { port, reply })
                    .await
                    .map_err(|_| "Listener is unavailable / 监听服务不可用")?;
                let applied = result
                    .await
                    .map_err(|_| "Listener stopped / 监听服务已停止")??;
                serde_json::to_string(&applied).map_err(|e| e.to_string())
            }
            "check_update" => {
                if self.update_busy.swap(true, Ordering::Relaxed) {
                    return Err("Update check already running".into());
                }
                struct Reset<'a>(&'a AtomicBool);
                impl Drop for Reset<'_> {
                    fn drop(&mut self) {
                        self.0.store(false, Ordering::Relaxed);
                    }
                }
                let _reset = Reset(&self.update_busy);
                crate::update::check(
                    &self.client,
                    &self.dir,
                    p["force"].as_bool().unwrap_or(false),
                )
                .await
            }
            "snapshot" => {
                let cfg = self.config.lock().unwrap();
                let logs = self.logs.lock().unwrap();
                Ok(json!({"url":self.address(),"requests":self.calls.load(Ordering::Relaxed),
                    "providers":cfg.providers.len(),"models":cfg.providers.iter().map(|p|p.models.len()).sum::<usize>(),"autostart":cfg.autostart,
                    "uptime":self.started.elapsed().as_secs(),"status":self.status.lock().unwrap().clone(),
                    "logs":logs.iter().map(|(t,s)|json!({"time":t,"message":s})).collect::<Vec<_>>()}).to_string())
            }
            "fetch" => self.discover(id, p).await,
            "model_metadata" => {
                let model = p["model"].as_str().unwrap_or("");
                let upstream = p["upstream"].as_str().unwrap_or("");
                if model.len() > 160 || upstream.len() > 2048 { return Err("Invalid model metadata request".into()); }
                let (protocol, settings, credential) = {
                    let cfg=self.config.lock().unwrap();
                    let provider=cfg.providers.iter().find(|v|v.id==id);
                    (p.get("protocol").map(|v|serde_json::from_value(v.clone()).map_err(|_|"Invalid protocol")).transpose()?.unwrap_or_else(||provider.map(|v|v.protocol).unwrap_or_default()),
                     p.get("protocol_settings").map(|v|serde_json::from_value(v.clone()).map_err(|_|"Invalid protocol settings")).transpose()?.unwrap_or_else(||provider.map(|v|v.protocol_settings.clone()).unwrap_or_default()),
                     p["key"].as_str().filter(|s|!s.trim().is_empty()).map(str::trim).map(str::to_owned).unwrap_or_else(||provider.map(|v|v.api_key.clone()).unwrap_or_default()))
                };
                settings.validate(protocol)?;
                let resolved = self.catalog.resolve_protocol(upstream, model, None, crate::catalog::Wire{protocol,settings:&settings,credential:&credential});
                Ok(json!({"model":model,"upstream":upstream,"protocol":protocol,"protocol_settings":settings,"metadata":resolved}).to_string())
            }
            "autostart" => {
                let enabled = p["enabled"].as_bool()
                    .ok_or("Invalid startup selection / 开机启动选项无效")?;
                let mut cfg = self.config.lock().unwrap();
                let mut next = cfg.clone();
                next.autostart = enabled;
                let path = self.dir.join("config.json");
                crate::config::write_json(&path, &next)?;
                if let Err(error) = crate::autostart::set(enabled, &self.dir) {
                    if let Err(rollback) = crate::config::write_json(&path, &*cfg) {
                        return Err(format!("Startup update failed ({error}); configuration rollback failed ({rollback}) / 开机启动设置失败，配置回滚也失败，请重试"));
                    }
                    return Err(format!("Cannot update login startup: {error} / 无法设置开机启动：{error}"));
                }
                *cfg = next;
                Ok(if enabled {
                    "Login startup enabled / 已开启开机启动"
                } else {
                    "Login startup disabled / 已关闭开机启动"
                }.into())
            }
            "language" => {
                let lang = p["language"].as_str().unwrap_or("");
                if !matches!(lang, "auto" | "zh" | "en") {
                    return Err("Invalid language".into());
                }
                let mut cfg = self.config.lock().unwrap();
                let mut next = cfg.clone();
                next.language = lang.into();
                crate::config::write_json(&self.dir.join("config.json"), &next)?;
                *cfg = next;
                Ok("Language saved".into())
            }
            "save" | "add" | "duplicate" | "delete" => {
                let mut cfg = self.config.lock().unwrap();
                let mut next = cfg.clone();
                match command {
                    "add" | "duplicate" => {
                        let mut c = if command == "duplicate" {
                            cfg.provider(id)?.clone()
                        } else {
                            Provider::default()
                        };
                        c.id = crate::config::token()[..12].into();
                        for m in &mut c.models {
                            m.id = crate::config::token()[..12].into();
                            m.refresh_alias();
                        }
                        let root = if command == "duplicate" {
                            format!(
                                "{} {}",
                                c.name,
                                if cfg.language == "zh" {
                                    "副本"
                                } else {
                                    "copy"
                                }
                            )
                        } else {
                            if cfg.language == "zh" {
                                "供应商".into()
                            } else {
                                "Provider".into()
                            }
                        };
                        c.name = root.clone();
                        let mut n = 1;
                        while next.providers.iter().any(|v| v.name == c.name) {
                            c.name = format!("{root} {n}");
                            n += 1;
                        }
                        next.providers.push(c);
                    }
                    "delete" => {
                        let selected = cfg.provider(id)?.id.clone();
                        next.providers.retain(|p| p.id != selected);
                    }
                    "save" => {
                        let selected = cfg.provider(id)?.id.clone();
                        let c = next
                            .providers
                            .iter_mut()
                            .find(|p| p.id == selected)
                            .unwrap();
                        // Older management clients omit these fields; preserve
                        // the existing protocol/settings rather than downgrading.
                        if let Some(protocol)=p.get("protocol"){
                            c.protocol=serde_json::from_value(protocol.clone()).map_err(|_|"Invalid upstream protocol")?;
                        }
                        if let Some(settings)=p.get("protocol_settings"){
                            c.protocol_settings=serde_json::from_value(settings.clone()).map_err(|_|"Invalid protocol settings")?;
                        }
                        for (field, target) in
                            [("name", &mut c.name), ("upstream", &mut c.upstream)]
                        {
                            if let Some(s) = p.get(field).and_then(serde_json::Value::as_str) {
                                *target = s.trim().into();
                            }
                        }
                        if let Some(key) = p
                            .get("key")
                            .and_then(serde_json::Value::as_str)
                            .filter(|s| !s.trim().is_empty())
                        {
                            c.api_key = key.trim().into();
                        }
                        if c.api_key.is_empty() {
                            return Err("请填写上游 API Key / Enter upstream API key".into());
                        }
                        let effort = p
                            .get("reasoning_effort")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or("keep");
                        if let Some(rows) = p.get("models").and_then(serde_json::Value::as_array) {
                            c.set_model_rows(rows, effort)?;
                        } else if let Some(lines) =
                            p.get("models").and_then(serde_json::Value::as_str)
                        {
                            c.set_models(lines, effort)?;
                        } else {
                            // Compatibility with v0.2 control clients: update a single model only.
                            if let Some(model) = p.get("model").and_then(serde_json::Value::as_str)
                            {
                                let index = c.models.iter().position(|m| m.id == id).unwrap_or(0);
                                let m = c.models.get_mut(index).ok_or("Add models first")?;
                                if m.model != model {
                                    m.model = model.trim().into();
                                    m.refresh_alias();
                                }
                            }
                            if effort != "keep" {
                                for m in &mut c.models {
                                    m.reasoning_effort = effort.into();
                                }
                            }
                        }
                    }
                    _ => unreachable!(),
                }
                next.validate()?;
                let routes = Routes::rebuild(&next,Some(&self.routes.lock().unwrap()))?;
                crate::config::write_json(&self.dir.join("config.json"), &next)?;
                *self.routes.lock().unwrap() = routes;
                *cfg = next;
                Ok(if command == "delete" && cfg.providers.is_empty() {
                    "已删除供应商 / Provider deleted"
                } else if command == "delete" {
                    "已删除，请同步 WorkBuddy 列表 / Deleted; sync WorkBuddy"
                } else {
                    "已保存供应商和模型 / Saved"
                }
                .into())
            }
            "test" => {
                let selected = if let Some(index) = p.get("model_index") {
                    let index = index.as_u64().and_then(|n| usize::try_from(n).ok())
                        .ok_or("Invalid model selection / 无效的模型选择")?;
                    Some(self.config.lock().unwrap().provider(id)?.models.get(index)
                        .ok_or("Selected model no longer exists / 选中的模型已不存在")?.id.clone())
                } else {
                    None
                };
                self.test(selected.as_deref().unwrap_or(id)).await
            }
            "install" => {
                self.prepare_metadata().await;
                let count = self.configure_workbuddy(&crate::workbuddy::path()?)?;
                Ok(format!(
                    "Synced {count} models; reopen WorkBuddy model list / 已同步 {count} 项，请重新打开模型列表"
                ))
            }
            "restore" => self
                .restore_workbuddy(&crate::workbuddy::path()?)
                .map(|_| "Removed synced proxy models; original entries preserved / 已移除代理同步的模型，原有条目已保留".into()),
            "copy" => {
                let cfg = self.config.lock().unwrap();
                let provider = cfg.provider(id)?;
                Ok(format!(
                    "URL: {}\nAPI Key: {}\nModels:\n{}",
                    self.connection_address(&provider.id),
                    cfg.local_key,
                    provider
                        .models
                        .iter()
                        .map(|m| format!(
                            "{} | {}\n  URL: {}",
                            m.alias,
                            m.name,
                            self.connection_address(&m.id)
                        ))
                        .collect::<Vec<_>>()
                        .join("\n")
                ))
            }
            "quit" => {
                if let Some(tx) = self.stop.lock().unwrap().take() {
                    tokio::spawn(async move {
                        tokio::time::sleep(Duration::from_millis(50)).await;
                        let _ = tx.send(());
                    });
                }
                Ok("Stopping / 正在退出".into())
            }
            _ => Err("Unknown command".into()),
        }
    }
}
#[derive(Debug)]
struct RewriteOutcome {
    bytes: bytes::Bytes,
    /// True when `usage_mode=request` wanted `include_usage` but the existing
    /// `stream_options` value was not an object, so it was left untouched.
    injection_failed: bool,
}

fn apply_reasoning(body: bytes::Bytes, effort: &str) -> Result<bytes::Bytes, String> {
    rewrite_request(body, None, effort, false).map(|outcome| outcome.bytes)
}

// Borrow messages/tools unchanged. Alias, thinking and usage overrides share one
// serialization and one bounded output allocation, even with a large prompt.
fn rewrite_request(
    body: bytes::Bytes,
    model: Option<&str>,
    effort: &str,
    inject_usage: bool,
) -> Result<RewriteOutcome, String> {
    if model.is_none() && effort == "default" && !inject_usage {
        // Byte-for-byte passthrough for the common observe path.
        return Ok(RewriteOutcome {
            bytes: body,
            injection_failed: false,
        });
    }
    use serde_json::value::RawValue;
    if effort == "default" && !inject_usage {
        #[derive(serde::Deserialize)]
        struct Field<'a> {
            #[serde(borrow)]
            model: &'a RawValue,
        }
        let field: Field<'_> = serde_json::from_slice(&body).map_err(|_| "Invalid chat request")?;
        let raw = field.model.get().as_bytes();
        let offset = raw.as_ptr() as usize - body.as_ptr() as usize;
        let replacement = serde_json::to_vec(&model.unwrap()).unwrap();
        let mut out = Vec::with_capacity(body.len() - raw.len() + replacement.len());
        out.extend_from_slice(&body[..offset]);
        out.extend_from_slice(&replacement);
        out.extend_from_slice(&body[offset + raw.len()..]);
        return Ok(RewriteOutcome {
            bytes: out.into(),
            injection_failed: false,
        });
    }
    let mut fields: std::collections::BTreeMap<String, &RawValue> =
        serde_json::from_slice(&body).map_err(|_| "Invalid chat request")?;
    let model_value =
        model.map(|m| RawValue::from_string(serde_json::to_string(m).unwrap()).unwrap());
    if let Some(value) = &model_value {
        fields.insert("model".into(), value);
    }
    let deepseek = fields
        .get("model")
        .and_then(|v| serde_json::from_str::<String>(v.get()).ok())
        .is_some_and(|m| m.starts_with("deepseek-"));
    let effort_value = (effort != "default")
        .then(|| RawValue::from_string(serde_json::to_string(effort).unwrap()).unwrap());
    let thinking_value = (effort != "default").then(|| {
        RawValue::from_string(
            if effort == "none" {
                r#"{"type":"disabled"}"#
            } else {
                r#"{"type":"enabled"}"#
            }
            .into(),
        )
        .unwrap()
    });
    if let Some(value) = &effort_value {
        if deepseek {
            fields.insert("thinking".into(), thinking_value.as_ref().unwrap());
            if effort == "none" {
                fields.remove("reasoning_effort");
            } else {
                fields.insert("reasoning_effort".into(), value);
            }
        } else {
            fields.insert("reasoning_effort".into(), value);
        }
    }
    let mut injection_failed = false;
    let injected_options: Option<Box<RawValue>> = if inject_usage && request_is_stream(&fields) {
        match fields.get("stream_options") {
            None => Some(
                RawValue::from_string(r#"{"include_usage":true}"#.to_owned())
                    .map_err(|_| "Invalid chat request")?,
            ),
            Some(existing) => {
                let text = existing.get().trim_start();
                if text.starts_with('{') {
                    // Merge, keeping every unknown `stream_options` field.
                    let mut options: serde_json::Map<String, serde_json::Value> =
                        serde_json::from_str(existing.get()).map_err(|_| "Invalid chat request")?;
                    options.insert("include_usage".into(), serde_json::Value::Bool(true));
                    Some(
                        RawValue::from_string(serde_json::to_string(&options).unwrap())
                            .map_err(|_| "Invalid chat request")?,
                    )
                } else {
                    // Never turn a working chat request into a 400: keep the
                    // original value and surface a health warning instead.
                    injection_failed = true;
                    None
                }
            }
        }
    } else {
        None
    };
    if let Some(value) = &injected_options {
        fields.insert("stream_options".into(), value.as_ref());
    }
    serde_json::to_vec(&fields)
        .map(bytes::Bytes::from)
        .map(|bytes| RewriteOutcome {
            bytes,
            injection_failed,
        })
        .map_err(|e| e.to_string())
}

fn request_is_stream(
    fields: &std::collections::BTreeMap<String, &serde_json::value::RawValue>,
) -> bool {
    fields
        .get("stream")
        .and_then(|value| serde_json::from_str::<bool>(value.get()).ok())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn effective_route_changes_invalidate_replay_even_after_settings_change_back() {
        let mut cfg = Config::default();
        let p = &mut cfg.providers[0];
        p.upstream = "https://example.test/v1".into();
        p.api_key = "fixture".into();
        p.protocol = crate::upstream_config::UpstreamProtocol::OpenaiResponses;
        p.models.push(crate::config::Model::new("model-a", "A"));
        let id = p.models[0].id.clone();
        let first = Routes::new(&cfg).unwrap();
        let original = first.ids[&id].generation;
        cfg.providers[0].name = "Cosmetic rename".into();
        let renamed = Routes::rebuild(&cfg, Some(&first)).unwrap();
        assert_eq!(renamed.ids[&id].generation, original);
        cfg.providers[0].models[0].model = "model-b".into();
        let changed = Routes::rebuild(&cfg, Some(&renamed)).unwrap();
        assert_ne!(changed.ids[&id].generation, original);
        cfg.providers[0].models[0].model = "model-a".into();
        let back = Routes::rebuild(&cfg, Some(&changed)).unwrap();
        assert_ne!(back.ids[&id].generation, original);
        cfg.providers[0].api_key = "rotated".into();
        let key = Routes::rebuild(&cfg, Some(&back)).unwrap();
        cfg.providers[0].api_key = "fixture".into();
        let key_back = Routes::rebuild(&cfg, Some(&key)).unwrap();
        assert_ne!(key_back.ids[&id].generation, back.ids[&id].generation);
    }
    #[test]
    fn thinking_override_preserves_large_raw_fields() {
        let body = bytes::Bytes::from_static(br#"{ "model":"deepseek-v4.1-flash", "messages": [ {"role":"user","content":"test"} ], "tools":[{"type":"function"}], "reasoning_effort":"high" }"#);
        assert_eq!(
            apply_reasoning(body.clone(), "default").unwrap().as_ptr(),
            body.as_ptr()
        );
        let high = apply_reasoning(body.clone(), "max").unwrap();
        assert!(
            std::str::from_utf8(&high)
                .unwrap()
                .contains(r#"[ {"role":"user","content":"test"} ]"#)
        );
        let parsed: serde_json::Value = serde_json::from_slice(&high).unwrap();
        assert_eq!(parsed["reasoning_effort"], "max");
        assert_eq!(parsed["thinking"]["type"], "enabled");
        let off: serde_json::Value =
            serde_json::from_slice(&apply_reasoning(body, "none").unwrap()).unwrap();
        assert!(off.get("reasoning_effort").is_none());
        assert_eq!(off["thinking"]["type"], "disabled");
    }
    #[test]
    fn alias_rewrite_preserves_every_other_byte() {
        let input=bytes::Bytes::from_static(br#"{ "opaque": {"model":"leave"}, "model" : "alias\/m", "messages" : [ { "content" : "unicode \u4e2d" } ], "x" : 1.2300e+2 }"#);
        let out = rewrite_request(input.clone(), Some("real-model"), "default", false)
            .unwrap()
            .bytes;
        let expected = std::str::from_utf8(&input)
            .unwrap()
            .replace(r#""alias\/m""#, r#""real-model""#);
        assert_eq!(&out[..], expected.as_bytes());
        assert_eq!(
            rewrite_request(input.clone(), None, "default", false)
                .unwrap()
                .bytes
                .as_ptr(),
            input.as_ptr()
        );
    }
    #[test]
    fn diagnostic_redaction_preserves_json_and_localized_details() {
        let mut cfg = Config::default();
        cfg.providers[0].api_key = "kind".into();
        cfg.providers[0].upstream = "https://example.com/v1".into();
        let folder = tempfile::tempdir().unwrap();
        let bridge = Bridge::new(cfg, folder.path().into()).unwrap();
        let c = Connection {
            provider: "供应商 kind".into(),
            name: "演示模型".into(),
            ..Default::default()
        };
        let mut report = crate::diagnostic::TestReport::from_connection(&c);
        report.reason = "Upstream rejected the test request / 上游拒绝了测试请求".into();
        report.http = "HTTP 401".into();
        let sanitized = bridge.sanitize(format!("ERROR {}", report.json()));
        let json = sanitized.strip_prefix("ERROR ").unwrap();
        let parsed: crate::diagnostic::TestReport = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.kind, "connection_test");
        assert_eq!(parsed.provider, "供应商 [redacted]");
        let zh = crate::i18n::message(&sanitized, true);
        assert!(zh.starts_with("ERROR 测试连接失败："));
        assert!(zh.contains("检查 API Key 是否正确、有效。"));
        assert!(!zh.contains("Connection test failed"));
        let en = crate::i18n::message(&sanitized, false);
        assert!(en.starts_with("ERROR Connection test failed:"));
        assert!(en.contains("Check whether the API key is valid."));
    }
    #[test]
    fn grouped_routes_share_credentials_and_reject_ambiguity() {
        let mut cfg = Config::default();
        cfg.providers[0].upstream = "https://example.com/v1".into();
        cfg.providers[0].api_key = "fixture".into();
        cfg.providers[0]
            .set_models("same | First\nsame | Second\nother", "default")
            .unwrap();
        let routes = Routes::new(&cfg).unwrap();
        let p = &cfg.providers[0];
        let first = routes.resolve(None, &p.models[0].alias).unwrap();
        let other = routes.resolve(Some(&p.id), "other").unwrap();
        assert!(Arc::ptr_eq(&first.destination, &other.destination));
        assert!(routes.resolve(Some(&p.id), "same").is_err());
        assert!(routes.resolve(Some(&p.models[0].id), "other").is_err());
        assert!(routes.resolve(Some("unknown"), &p.models[0].alias).is_err());
    }
    #[test]
    fn readable_and_legacy_ids_keep_scoped_routes_after_renaming() {
        let mut cfg = Config::default();
        cfg.providers[0].name = "opencode".into();
        cfg.providers[0].upstream = "https://example.com/v1".into();
        cfg.providers[0]
            .set_models("same | First\nsame | Second", "default")
            .unwrap();
        let first = &mut cfg.providers[0].models[0];
        first.alias = format!("wbp-{}/same", first.id);
        let readable = cfg.workbuddy_connections().unwrap();
        cfg.installed.push(crate::config::Installation {
            id: readable[0].alias.clone(),
            previous: None,
            expected: Some(
                json!({"url":format!("http://127.0.0.1:18080/r/{}/v1", readable[0].id)}),
            ),
        });
        cfg.providers[0].name = "renamed".into();
        let routes = Routes::new(&cfg).unwrap();
        let current = cfg.workbuddy_connections().unwrap();
        for model in [
            &readable[0].alias,
            &current[0].alias,
            &cfg.providers[0].models[0].alias,
            &format!("wbp-{}", readable[0].id),
        ] {
            for scope in [
                None,
                Some(cfg.providers[0].id.as_str()),
                Some(readable[0].id.as_str()),
            ] {
                assert_eq!(
                    routes.resolve(scope, model).unwrap().connection.id,
                    readable[0].id
                );
            }
            assert!(routes.resolve(Some(&readable[1].id), model).is_err());
        }
        assert!(routes.resolve(None, "same").is_err());
        assert!(routes.resolve(Some("unknown"), &current[0].alias).is_err());
    }
    #[test]
    fn legacy_bare_alias_does_not_bypass_same_model_isolation() {
        let mut cfg = Config::default();
        cfg.providers[0].upstream = "https://example.com/v1".into();
        cfg.providers[0]
            .set_models("same | One\nsame | Two", "default")
            .unwrap();
        cfg.providers[0].models[0].alias = "same".into();
        let routes = Routes::new(&cfg).unwrap();
        assert!(routes.resolve(None, "same").is_err());
        assert!(routes.resolve(Some(&cfg.providers[0].id), "same").is_err());
        assert!(
            routes
                .resolve(Some(&cfg.providers[0].models[0].id), "same")
                .is_ok()
        );
    }
    #[test]
    fn end_to_end_headers_preserved_without_hop_fields_or_local_key() {
        let mut h = HeaderMap::new();
        h.insert("connection", "X-Hop, keep-alive".parse().unwrap());
        h.insert("x-hop", "remove".parse().unwrap());
        h.insert("x-custom", "keep".parse().unwrap());
        h.insert("authorization", "Bearer local".parse().unwrap());
        h.insert("content-length", "123".parse().unwrap());
        h.insert("x-api-key", "client-vendor-key".parse().unwrap());
        h.insert("x-goog-api-key", "client-google-key".parse().unwrap());
        h.append("set-cookie", "a=1".parse().unwrap());
        h.append("set-cookie", "b=2".parse().unwrap());
        let r = forward_headers(&h, true);
        assert!(!r.contains_key("x-api-key") && !r.contains_key("x-goog-api-key"));
        assert!(!r.contains_key("authorization"));
        assert!(!r.contains_key("content-length"));
        assert!(!r.contains_key("x-hop"));
        assert_eq!(r["x-custom"], "keep");
        let r = forward_headers(&h, false);
        assert_eq!(r.get_all("set-cookie").iter().count(), 2);
        assert_eq!(r["content-length"], "123");
    }
    #[test]
    fn local_access_rules() {
        let mut h = HeaderMap::new();
        h.insert("host", "127.0.0.1:18080".parse().unwrap());
        assert!(local(&h, 18080));
        h.insert("origin", "https://evil.example".parse().unwrap());
        assert!(!local(&h, 18080));
        h.remove("origin");
        h.insert("authorization", "Bearer local".parse().unwrap());
        assert!(authorized(&h, "local"));
        assert!(!authorized(&h, "wrong"));
    }
}
