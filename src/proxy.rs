use crate::{
    config::{Config, Connection, Provider},
    session::{Envelope, Sessions},
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

pub const BODY_LIMIT: usize = 8 * 1024 * 1024;
pub const MAX_CONCURRENT: usize = 4;
struct Destination {
    endpoint: reqwest::Url,
    authorization: axum::http::HeaderValue,
    go: bool,
    has_key: bool,
}
struct Route {
    connection: Connection,
    destination: Arc<Destination>,
    provider_id: String,
}
#[derive(Default)]
struct Routes {
    ids: std::collections::HashMap<String, Arc<Route>>,
    aliases: std::collections::HashMap<String, Arc<Route>>,
    models: std::collections::HashMap<String, Vec<Arc<Route>>>,
}
impl Routes {
    fn new(cfg: &Config) -> Result<Self, String> {
        let mut routes = Self::default();
        for p in &cfg.providers {
            if p.models.is_empty() {
                continue;
            }
            let c = Connection {
                upstream: p.upstream.clone(),
                ..Default::default()
            };
            let mut authorization: axum::http::HeaderValue = format!("Bearer {}", p.api_key)
                .parse()
                .map_err(|_| "Invalid API key")?;
            authorization.set_sensitive(true);
            let destination = Arc::new(Destination {
                endpoint: c.endpoint().parse().map_err(|_| "Invalid upstream URL")?,
                authorization,
                go: c.is_go(),
                has_key: !p.api_key.is_empty(),
            });
            for m in &p.models {
                let connection = Connection {
                    id: m.id.clone(),
                    alias: m.alias.clone(),
                    name: m.name.clone(),
                    provider: p.name.clone(),
                    model: m.model.clone(),
                    reasoning_effort: m.reasoning_effort.clone(),
                    api_key: String::new(),
                    upstream: String::new(),
                };
                let route = Arc::new(Route {
                    connection,
                    destination: destination.clone(),
                    provider_id: p.id.clone(),
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
        Ok(routes)
    }
    fn resolve(&self, scope: Option<&str>, model: &str) -> Result<Arc<Route>, String> {
        if let Some(id) = scope
            && let Some(r) = self.ids.get(id)
        {
            return if model == r.connection.alias
                || model == r.connection.model
                || model == format!("wbp-{}", r.connection.id)
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
        Ok(Self {
            routes: Mutex::new(Routes::new(&config)?),
            config: Mutex::new(config),
            dir,
            client,
            sessions: Mutex::new(Sessions::default()),
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
        })
    }
    pub fn connection_address(&self, id: &str) -> String {
        format!(
            "http://127.0.0.1:{}/r/{id}/v1",
            self.port.load(Ordering::Relaxed)
        )
    }
    pub fn set_status(&self, text: String) {
        *self.status.lock().unwrap() = text.clone();
        let mut logs = self.logs.lock().unwrap();
        if logs.len() == 100 {
            logs.pop_front();
        }
        logs.push_back((self.started.elapsed().as_secs(), text));
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
        let (base, key) = {
            let cfg = self.config.lock().unwrap();
            let provider = cfg.provider(id)?;
            let base = payload["upstream"]
                .as_str()
                .unwrap_or(&provider.upstream)
                .trim()
                .to_owned();
            let key = payload["key"]
                .as_str()
                .filter(|s| !s.trim().is_empty())
                .unwrap_or(&provider.api_key)
                .trim()
                .to_owned();
            (base, key)
        };
        if key.is_empty() {
            return Err("Enter upstream API key".into());
        }
        let url = reqwest::Url::parse(&format!("{}/models", base.trim_end_matches('/')))
            .map_err(|_| "Invalid upstream URL")?;
        if !matches!(url.scheme(), "http" | "https")
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err("Invalid upstream URL".into());
        }
        let _permit = self
            .permits
            .clone()
            .try_acquire_owned()
            .map_err(|_| "Four requests are already in flight")?;
        let models = tokio::time::timeout(Duration::from_secs(30), async {
            let response = self
                .client
                .get(url)
                .bearer_auth(key)
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
                if bytes.len() + chunk.len() > 1024 * 1024 {
                    return Err("Model list exceeds 1 MiB".into());
                }
                bytes.extend_from_slice(&chunk);
            }
            let data: serde_json::Value =
                serde_json::from_slice(&bytes).map_err(|_| "Invalid model list")?;
            let rows = data["data"]
                .as_array()
                .ok_or("Expected OpenAI models data array")?;
            let mut ids = std::collections::BTreeSet::new();
            for row in rows {
                if let Some(id) = row["id"].as_str().map(str::trim).filter(|s| !s.is_empty()) {
                    if id.len() > 160 || id.chars().any(char::is_control) {
                        return Err("Invalid model ID".into());
                    }
                    ids.insert(id.to_owned());
                    if ids.len() > crate::config::MAX_MODELS {
                        return Err("At most 256 models; upstream returned too many".into());
                    }
                } else {
                    return Err("Invalid model list".into());
                }
            }
            if ids.is_empty() {
                return Err("No available models returned".into());
            }
            if ids.len() > crate::config::MAX_MODELS {
                return Err("At most 256 models; upstream returned too many".into());
            }
            Ok(ids.into_iter().collect::<Vec<_>>())
        })
        .await
        .map_err(|_| "Connection timeout")??;
        self.set_status(format!("Fetched {} models", models.len()));
        Ok(json!(models).to_string())
    }
    pub async fn test(&self, id: &str) -> Result<String, String> {
        let c = self.config.lock().unwrap().connection(id)?;
        let r = self.routes.lock().unwrap().resolve(Some(&c.id), &c.model)?;
        let _permit = self
            .permits
            .clone()
            .try_acquire_owned()
            .map_err(|_| "Four requests are already in flight")?;
        if !r.destination.has_key {
            return Err("Save your API key first / 请先保存 API Key".into());
        }
        let payload=serde_json::to_vec(&json!({"model":c.model,"stream":true,"max_tokens":1,"messages":[{"role":"user","content":"Reply with OK."}]})).unwrap();
        let payload = apply_reasoning(payload.into(), &c.reasoning_effort)?;
        let started = std::time::Instant::now();
        let res = tokio::time::timeout(
            Duration::from_secs(45),
            self.upstream(
                &r,
                &HeaderMap::new(),
                payload,
                &crate::config::token(),
                None,
            ),
        )
        .await
        .map_err(|_| "Connection timeout / 连接超时")?
        .map_err(|_| "Connection failed; check network and proxy / 请检查网络和代理")?;
        let status = res.status();
        if !status.is_success() {
            return Err(format!(
                "Upstream HTTP {} / 上游返回 HTTP {}",
                status.as_u16(),
                status.as_u16()
            ));
        }
        tokio::time::timeout(Duration::from_secs(45), async {
            let mut stream = res.bytes_stream();
            let mut pending = Vec::new();
            while let Some(chunk) = stream.next().await {
                let b = chunk.map_err(|_| "Invalid upstream stream")?;
                pending.extend_from_slice(&b);
                if pending.len() > 65536 {
                    return Err("Invalid upstream response");
                }
                while let Some(i) = pending.iter().position(|b| *b == b'\n') {
                    let line = String::from_utf8_lossy(&pending[..i]);
                    let data = line.trim().strip_prefix("data:").map(str::trim);
                    if let Some(text) = data
                        && let Ok(v) = serde_json::from_str::<serde_json::Value>(text)
                    {
                        if v.get("error").is_some() {
                            return Err("Upstream returned an error");
                        };
                        if v.get("choices").is_some() {
                            return Ok(());
                        }
                    }
                    pending.drain(..=i);
                }
            }
            Err("No valid streaming response")
        })
        .await
        .map_err(|_| "Stream timeout / 流式响应超时")?
        .map_err(str::to_string)?;
        Ok(format!("HTTP 200 · {} ms", started.elapsed().as_millis()))
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
            || (request && matches!(name.as_str(), "authorization" | "host" | "content-length"))
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
            json!({"app":"workbuddy-proxy","version":env!("CARGO_PKG_VERSION"),"ready":true})
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
    let entries: Vec<_> = cfg.providers.iter().filter(|p| !p.api_key.is_empty())
        .flat_map(|p| p.models.iter().filter(move |m| id.is_none_or(|id| p.id==id || m.id==id))
            .map(move |m| json!({"id":m.alias,"object":"model","owned_by":p.name,"name":format!("{} · {} · {}",m.name,p.name,m.model)})))
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
    if !r.destination.has_key {
        return fail(
            StatusCode::SERVICE_UNAVAILABLE,
            "Save this provider's API key first",
        );
    }
    let c = &r.connection;
    let session = if r.destination.go {
        let envelope: Envelope<'_> = match serde_json::from_slice(&bytes) {
            Ok(e) => e,
            Err(_) => return fail(StatusCode::BAD_REQUEST, "Go requires a messages array"),
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
    let bytes = match rewrite_request(bytes, replace_model, &c.reasoning_effort) {
        Ok(b) => b,
        Err(e) => return fail(StatusCode::BAD_REQUEST, &e),
    };
    let res = match a
        .upstream(&r, &parts.headers, bytes, &session, parts.uri.query())
        .await
    {
        Ok(r) => r,
        Err(_) => {
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
    let mut response = Response::builder().status(status);
    *response.headers_mut().unwrap() = forward_headers(res.headers(), false);
    // No SSE parsing, buffering, rewriting or extra response copies. The permit
    // travels with the stream and is released on completion OR client cancellation.
    let stream = stream::unfold(
        (res.bytes_stream(), permit),
        |(mut source, permit)| async move { source.next().await.map(|chunk| (chunk, (source, permit))) },
    );
    response.body(Body::from_stream(stream)).unwrap()
}
async fn show(State(a): State<Arc<Bridge>>, headers: HeaderMap) -> Response<Body> {
    if !local(&headers, a.port.load(Ordering::Relaxed)) || !authorized(&headers, &a.management_key)
    {
        return fail(StatusCode::UNAUTHORIZED, "Unauthorized");
    };
    crate::native::show();
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
            )
        ) {
            Some(self.mutations.lock().await)
        } else {
            None
        };
        let result = self.command_inner(p).await;
        if !matches!(
            p["command"].as_str(),
            Some("snapshot" | "copy" | "fetch" | "check_update")
        ) {
            self.set_status(match &result {
                Ok(s) if p["command"] == "set_port" => {
                    serde_json::from_str::<crate::port::Applied>(s)
                        .map(|v| v.message())
                        .unwrap_or_else(|_| "Port updated".into())
                }
                Ok(s) => s.clone(),
                Err(e) => format!("ERROR {e}"),
            });
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
                    "providers":cfg.providers.len(),"models":cfg.providers.iter().map(|p|p.models.len()).sum::<usize>(),
                    "uptime":self.started.elapsed().as_secs(),"status":self.status.lock().unwrap().clone(),
                    "logs":logs.iter().map(|(t,s)|json!({"time":t,"message":s})).collect::<Vec<_>>()}).to_string())
            }
            "fetch" => self.discover(id, p).await,
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
                let routes = Routes::new(&next)?;
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
            "test" => self.test(id).await,
            "install" => {
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
fn apply_reasoning(body: bytes::Bytes, effort: &str) -> Result<bytes::Bytes, String> {
    rewrite_request(body, None, effort)
}
// Borrow messages/tools unchanged. Alias and thinking overrides share one
// serialization and one bounded output allocation, even with a large prompt.
fn rewrite_request(
    body: bytes::Bytes,
    model: Option<&str>,
    effort: &str,
) -> Result<bytes::Bytes, String> {
    if model.is_none() && effort == "default" {
        return Ok(body);
    }
    use serde_json::value::RawValue;
    if effort == "default" {
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
        return Ok(out.into());
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
    serde_json::to_vec(&fields)
        .map(bytes::Bytes::from)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
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
        let out = rewrite_request(input.clone(), Some("real-model"), "default").unwrap();
        let expected = std::str::from_utf8(&input)
            .unwrap()
            .replace(r#""alias\/m""#, r#""real-model""#);
        assert_eq!(&out[..], expected.as_bytes());
        assert_eq!(
            rewrite_request(input.clone(), None, "default")
                .unwrap()
                .as_ptr(),
            input.as_ptr()
        );
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
        h.append("set-cookie", "a=1".parse().unwrap());
        h.append("set-cookie", "b=2".parse().unwrap());
        let r = forward_headers(&h, true);
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
