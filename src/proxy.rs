use crate::{
    config::{Config, Connection},
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
        atomic::{AtomicU16, AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::sync::{Semaphore, oneshot};

pub const BODY_LIMIT: usize = 8 * 1024 * 1024;
pub const MAX_CONCURRENT: usize = 4;
pub struct Bridge {
    pub config: Mutex<Config>,
    pub dir: PathBuf,
    pub client: reqwest::Client,
    sessions: Mutex<Sessions>,
    permits: Arc<Semaphore>,
    pub port: AtomicU16,
    pub calls: AtomicU64,
    pub status: Mutex<String>,
    pub management_key: String,
    pub stop: Mutex<Option<oneshot::Sender<()>>>,
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
            config: Mutex::new(config),
            dir,
            client,
            sessions: Mutex::new(Sessions::default()),
            permits: Arc::new(Semaphore::new(MAX_CONCURRENT)),
            port: AtomicU16::new(0),
            calls: AtomicU64::new(0),
            status: Mutex::new("Ready / 就绪".into()),
            management_key: crate::config::token(),
            stop: Mutex::new(None),
        })
    }
    pub fn connection_address(&self, id: &str) -> String {
        format!(
            "http://127.0.0.1:{}/r/{id}/v1",
            self.port.load(Ordering::Relaxed)
        )
    }
    pub fn set_status(&self, text: String) {
        *self.status.lock().unwrap() = text;
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
    pub async fn upstream(
        &self,
        c: &Connection,
        body: bytes::Bytes,
        session: &str,
    ) -> Result<reqwest::Response, reqwest::Error> {
        let mut req = self
            .client
            .post(c.endpoint())
            .header("Authorization", format!("Bearer {}", c.api_key))
            .header("Content-Type", "application/json")
            .header("Accept", "text/event-stream, application/json")
            .header(
                "User-Agent",
                concat!("WorkBuddy-Proxy/", env!("CARGO_PKG_VERSION")),
            )
            .body(body);
        if c.is_go() {
            req = req.header("x-opencode-session", session)
        }
        req.send().await
    }
    pub async fn test(&self, id: &str) -> Result<String, String> {
        let c = self.config.lock().unwrap().connection(id)?.clone();
        let _permit = self
            .permits
            .clone()
            .try_acquire_owned()
            .map_err(|_| "Four requests are already in flight")?;
        if c.api_key.is_empty() {
            return Err("Save your API key first / 请先保存 API Key".into());
        }
        let payload=serde_json::to_vec(&json!({"model":c.model,"stream":true,"max_tokens":1,"messages":[{"role":"user","content":"Reply with OK."}]})).unwrap();
        let payload = apply_reasoning(payload.into(), &c.reasoning_effort)?;
        let started = std::time::Instant::now();
        let res = tokio::time::timeout(
            Duration::from_secs(45),
            self.upstream(&c, payload, &crate::config::token()),
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
    if id.is_some_and(|id| !cfg.connections.iter().any(|c| c.id == id)) {
        return fail(StatusCode::NOT_FOUND, "Unknown connection");
    }
    let entries: Vec<_> = cfg
        .connections
        .iter()
        .filter(|c| !c.api_key.is_empty() && id.is_none_or(|id| c.id == id))
        .map(|c| json!({"id":c.alias,"object":"model","owned_by":c.provider,"name":c.label()}))
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
    let envelope: Envelope<'_> = match serde_json::from_slice(&bytes) {
        Ok(e) => e,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "model and messages are required"),
    };
    if envelope.model.is_empty() || envelope.messages.is_empty() {
        return fail(
            StatusCode::BAD_REQUEST,
            "model and messages must not be empty",
        );
    }
    let c = match a
        .config
        .lock()
        .unwrap()
        .resolve(route.as_deref(), envelope.model)
    {
        Ok(c) => c,
        Err(e) => return fail(StatusCode::BAD_REQUEST, &e),
    };
    if c.api_key.is_empty() {
        return fail(
            StatusCode::SERVICE_UNAVAILABLE,
            "Save this connection's API key first",
        );
    }
    let session = if c.is_go() {
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
    drop(envelope);
    let bytes = match prepare_request(bytes, &c) {
        Ok(b) => b,
        Err(e) => return fail(StatusCode::BAD_REQUEST, &e),
    };
    let res = match a.upstream(&c, bytes, &session).await {
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
    let mut response = Response::builder()
        .status(status)
        .header("Cache-Control", "no-store")
        .header("X-Content-Type-Options", "nosniff");
    for name in ["content-type", "retry-after", "content-encoding"] {
        if let Some(value) = res.headers().get(name) {
            response = response.header(name, value)
        }
    }
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
    let bytes = match to_bytes(body, 16384).await {
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
        let command = p
            .get("command")
            .and_then(serde_json::Value::as_str)
            .ok_or("Missing command")?;
        let id = p
            .get("connection_id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        match command {
            "save" | "add" | "duplicate" | "delete" => {
                let mut cfg = self.config.lock().unwrap();
                let mut next = cfg.clone();
                match command {
                    "add" | "duplicate" => {
                        let mut c = if command == "duplicate" {
                            cfg.connection(id)?.clone()
                        } else {
                            Connection::default()
                        };
                        c.id = crate::config::token()[..12].into();
                        c.refresh_alias();
                        let root = if command == "duplicate" {
                            format!("{} 副本", c.name)
                        } else {
                            "连接".into()
                        };
                        c.name = root.clone();
                        let mut n = 1;
                        while next.connections.iter().any(|v| v.name == c.name) {
                            c.name = format!("{root} {n}");
                            n += 1;
                        }
                        next.connections.push(c);
                    }
                    "delete" => {
                        let selected = cfg.connection(id)?.id.clone();
                        next.connections.retain(|c| c.id != selected);
                    }
                    "save" => {
                        let selected = cfg.connection(id)?.id.clone();
                        let c = next
                            .connections
                            .iter_mut()
                            .find(|c| c.id == selected)
                            .unwrap();
                        let old_model = c.model.clone();
                        for (field, target) in [
                            ("model", &mut c.model),
                            ("upstream", &mut c.upstream),
                            ("name", &mut c.name),
                            ("provider", &mut c.provider),
                            ("reasoning_effort", &mut c.reasoning_effort),
                        ] {
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
                            return Err("Enter upstream API key / 请填写上游 Key".into());
                        }
                        if c.alias != old_model || c.model != old_model {
                            c.refresh_alias();
                        }
                    }
                    _ => unreachable!(),
                }
                next.validate()?;
                crate::config::write_json(&self.dir.join("config.json"), &next)?;
                *cfg = next;
                Ok(match command { "save" => "Saved / 已保存", "delete" => "Deleted locally; sync WorkBuddy to update its list / 已删除，请同步 WorkBuddy 列表", _ => "Added / 已新增连接" }.into())
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
                .map(|_| "Previous entries restored / 已恢复原列表项".into()),
            "copy" => {
                let cfg = self.config.lock().unwrap();
                let c = cfg.connection(id)?;
                Ok(format!(
                    "URL: {}\nAPI Key: {}\nModel: {}\nName: {}",
                    self.connection_address(&c.id),
                    cfg.local_key,
                    c.alias,
                    c.label()
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
fn prepare_request(body: bytes::Bytes, c: &Connection) -> Result<bytes::Bytes, String> {
    let e: Envelope<'_> = serde_json::from_slice(&body).map_err(|_| "Invalid chat request")?;
    let replace_model = (e.model != c.model).then_some(c.model.as_str());
    rewrite_request(body, replace_model, &c.reasoning_effort)
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
