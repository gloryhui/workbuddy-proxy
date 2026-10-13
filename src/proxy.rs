use crate::{
    config::Config,
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
    pub fn settings(&self) -> Config {
        let c = self.config.lock().unwrap();
        Config {
            api_key: c.api_key.clone(),
            local_key: c.local_key.clone(),
            model: c.model.clone(),
            upstream: c.upstream.clone(),
            port: c.port,
            installed: None,
        }
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
            .route("/_control/show", post(show))
            .route("/_control/ping", post(ping))
            .route("/_control/action", post(action))
            .route("/_control/stop", post(stop))
            .with_state(self)
    }
    pub async fn upstream(
        &self,
        c: &Config,
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
    pub async fn test(&self) -> Result<String, String> {
        let c = self.settings();
        if c.api_key.is_empty() {
            return Err("Save your API key first / 请先保存 API Key".into());
        }
        let payload=serde_json::to_vec(&json!({"model":c.model,"stream":true,"max_tokens":1,"messages":[{"role":"user","content":"Reply with OK."}]})).unwrap();
        let started = std::time::Instant::now();
        let res = tokio::time::timeout(
            Duration::from_secs(45),
            self.upstream(&c, payload.into(), &crate::config::token()),
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
    let c = a.settings();
    if !local(&headers, a.port.load(Ordering::Relaxed)) || !authorized(&headers, &c.local_key) {
        return fail(StatusCode::UNAUTHORIZED, "Invalid local API key");
    };
    Response::builder().header("Content-Type","application/json").body(Body::from(json!({"object":"list","data":[{"id":c.model,"object":"model","owned_by":"configured-upstream"}]}).to_string())).unwrap()
}
async fn relay(State(a): State<Arc<Bridge>>, request: Request<Body>) -> Response<Body> {
    let (parts, body) = request.into_parts();
    if !local(&parts.headers, a.port.load(Ordering::Relaxed)) {
        return fail(StatusCode::FORBIDDEN, "Invalid host or origin");
    }
    let c = a.settings();
    if !authorized(&parts.headers, &c.local_key) {
        return fail(
            StatusCode::UNAUTHORIZED,
            "Use the local API key from WorkBuddy Proxy settings",
        );
    }
    if c.api_key.is_empty() {
        return fail(
            StatusCode::SERVICE_UNAVAILABLE,
            "Save upstream API key in WorkBuddy Proxy first",
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
    let session = if c.is_go() {
        a.sessions
            .lock()
            .unwrap()
            .resolve(&parts.headers, &envelope)
    } else {
        String::new()
    };
    drop(envelope);
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
#[derive(serde::Deserialize)]
struct Control {
    command: String,
    #[serde(default)]
    key: String,
    #[serde(default)]
    model: String,
    #[serde(default)]
    upstream: String,
}
async fn action(State(a): State<Arc<Bridge>>, request: Request<Body>) -> Response<Body> {
    let (parts, body) = request.into_parts();
    if !local(&parts.headers, a.port.load(Ordering::Relaxed))
        || !authorized(&parts.headers, &a.management_key)
    {
        return fail(StatusCode::UNAUTHORIZED, "Unauthorized");
    }
    let bytes = match to_bytes(body, 8192).await {
        Ok(b) => b,
        Err(_) => return fail(StatusCode::PAYLOAD_TOO_LARGE, "Control payload too large"),
    };
    let p: Control = match serde_json::from_slice(&bytes) {
        Ok(p) => p,
        Err(_) => return fail(StatusCode::BAD_REQUEST, "Invalid control payload"),
    };
    let result: Result<String, String> = match p.command.as_str() {
        "save" => {
            let mut cfg = a.config.lock().unwrap();
            let mut next = cfg.clone();
            if !p.key.trim().is_empty() {
                next.api_key = p.key.trim().into();
            }
            next.model = p.model.trim().into();
            next.upstream = p.upstream.trim().into();
            next.validate().and_then(|_| {
                if next.api_key.is_empty() {
                    return Err("Enter upstream API key".into());
                }
                crate::config::write_json(&a.dir.join("config.json"), &next)?;
                *cfg = next;
                Ok("Saved / 已保存".into())
            })
        }
        "test" => a.test().await,
        "install" => crate::workbuddy::path()
            .and_then(|p| a.configure_workbuddy(&p))
            .map(|_| {
                "WorkBuddy configured; reopen model settings / 已配置，请重新打开模型设置".into()
            }),
        "restore" => crate::workbuddy::path()
            .and_then(|p| a.restore_workbuddy(&p))
            .map(|_| "Previous model restored / 已恢复原配置".into()),
        "copy" => {
            let c = a.settings();
            Ok(format!(
                "URL: {}\nAPI Key: {}\nModel: {}",
                a.address(),
                c.local_key,
                c.model
            ))
        }
        "quit" => {
            if let Some(tx) = a.stop.lock().unwrap().take() {
                tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    let _ = tx.send(());
                });
            }
            Ok("Stopping / 正在退出".into())
        }
        _ => Err("Unknown command".into()),
    };
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

#[cfg(test)]
mod tests {
    use super::*;
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
