//! Native HTTP boundary shared by relay and connection probes. The original
//! Chat route stays in proxy.rs and never enters these conversion paths.
//!
//! 入口是 `proxy::relay_to`/`proxy::Bridge::test` 在目标协议为 Responses、
//! Anthropic Messages 或 Gemini 时的调用。`prepare` 负责解析/编码请求，`open` 负责
//! 上游 HTTP，`NativeStream` 把上游事件增量转换为 Chat 流。统计 guard 和 permit 由
//! 调用方传入并沿响应流持有；本模块会读取并转换 Chat 请求，但不会把未被转换的
//! 原始 Chat 响应字节直接透传。
use super::*;
use crate::protocol::{
    self,
    config::UpstreamProtocol as UP,
    output::{Identity, Output},
    replay,
    request::ChatRequest,
    sse, streaming,
};
use axum::http::{HeaderValue, header};
use bytes::Bytes;
use futures_util::Stream;
use std::{
    collections::VecDeque,
    io::{self, Write},
    pin::Pin,
    time::Instant,
};
type Error = protocol::Error;
type Guard = Option<stats::event::RequestGuard>;
type SourceStream = Pin<Box<dyn Stream<Item = Result<Bytes, reqwest::Error>> + Send>>;

/// `Limited` 实现 `io::Write`，累计 `Vec<u8>` 并在超过 `BODY_LIMIT` 时拒绝继续写。
struct Limited(Vec<u8>);
impl Write for Limited {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.0.len() + bytes.len() > BODY_LIMIT {
            return Err(io::Error::other("converted request too large"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
struct Prepared {
    /// 已解析、已应用思考/能力约束并可用于 replay 的语义请求。
    request: ChatRequest,
    /// 转换后的 native 上游 JSON，大小在 `BODY_LIMIT` 内。
    body: Bytes,
}
/// 把客户端 Chat 请求编码成目标 native 协议。
///
/// 输入是共享目录/replay cache、选定路由和已通过鉴权与大小限制的 Chat 请求 body。
/// 输出同时保留供响应适配和 replay 使用的语义请求，以及不超过 `BODY_LIMIT` 的 native
/// JSON。能力校验、思考强度和输出上限在编码前完成；失败返回结构化协议错误，调用方
/// 负责把该错误映射为 HTTP 响应和统计终态。函数不访问网络。
fn prepare(b: &Bridge, r: &Route, bytes: &[u8]) -> Result<Prepared, Error> {
    // 阶段一：解析客户端 Chat 请求并应用路由上的思考强度。阶段二：从目录或显式
    // capabilities 解析能力，在编码前拒绝不支持的图片、工具或思考设置。阶段三：
    // 让 replay cache 根据历史 tool call 解析 opaque 上游状态，再编码成目标协议。
    // 转换失败返回结构化 `protocol::Error`，原始 prompt 不进入统计队列。
    let mut request = ChatRequest::parse(bytes)?;
    match r.connection.reasoning_effort.as_str() {
        "default" => (),
        effort => request.reasoning_effort = Some(effort.into()),
    }
    let resolved = b.catalog.resolve_protocol(
        &r.connection.upstream,
        &r.connection.model,
        r.connection.capabilities.as_ref(),
        crate::catalog::Wire {
            protocol: r.destination.protocol,
            settings: &r.destination.settings,
            credential: &r.connection.api_key,
        },
    );
    let caps = &resolved.capabilities;
    if resolved.image_known
        && !caps.supports_images
        && request
            .messages
            .iter()
            .flat_map(|m| &m.parts)
            .any(|p| !matches!(p, protocol::request::Part::Text(_)))
    {
        return Err(Error::input(
            "model_does_not_support_images",
            "messages.content",
        ));
    }
    if !caps.supports_tool_call && !request.tools.is_empty() {
        return Err(Error::input("model_does_not_support_tools", "tools"));
    }
    if caps.only_reasoning && request.reasoning_effort.as_deref() == Some("none") {
        return Err(Error::input("model_requires_reasoning", "reasoning_effort"));
    }
    if resolved.reasoning_known
        && request
            .reasoning_effort
            .as_deref()
            .is_some_and(|e| e != "none")
        && !caps.supports_reasoning
    {
        return Err(Error::input(
            "model_does_not_support_reasoning",
            "reasoning_effort",
        ));
    }
    if resolved.reasoning_known
        && request.reasoning_effort.as_deref().is_some_and(|e| {
            e != "none" && !caps.reasoning.supported_efforts.iter().any(|v| v == e)
        })
    {
        return Err(Error::input(
            "unsupported_model_reasoning_effort",
            "reasoning_effort",
        ));
    }
    if resolved.reasoning_known
        && request.reasoning_effort.as_deref() == Some("none")
        && !caps.reasoning.can_disable_thinking
    {
        return Err(Error::input(
            "model_cannot_disable_reasoning",
            "reasoning_effort",
        ));
    }
    if let Some(cap) = caps.max_output_tokens {
        if request.max_output_tokens.is_some_and(|n| n > cap) {
            return Err(Error::input(
                "output_limit_exceeded",
                "max_completion_tokens",
            ));
        }
        if request.max_output_tokens.is_none() && r.destination.protocol == UP::AnthropicMessages {
            request.max_output_tokens =
                Some(r.destination.settings.default_max_output_tokens.min(cap));
        }
    }
    let history = b.native_replay.lock().unwrap().resolve(
        &r.generation,
        &request.messages,
        Instant::now(),
    )?;
    let native = protocol::encode::encode(
        r.destination.protocol,
        &r.connection.model,
        &r.destination.settings,
        &request,
        &history,
    )?;
    let mut serialized = Limited(Vec::new());
    serde_json::to_writer(&mut serialized, &native)
        .map_err(|_| Error::input("converted_request_too_large", "messages"))?;
    Ok(Prepared {
        request,
        body: serialized.0.into(),
    })
}
/// 通过所选 native 协议的认证方式和端点发送已编码请求。
///
/// 输入为路由、准备后的 body 和原客户端 headers；输出只表示收到了上游响应头，body
/// 尚未读取或转换。连接/超时错误在这里归一为协议错误，调用方会把它们写为 HTTP
/// 502-ish failure 和 `Outcome::Failed/ErrorKind::Connect`。本函数不提交统计终态。
async fn open(
    b: &Bridge,
    r: &Route,
    p: &Prepared,
    headers: &HeaderMap,
) -> Result<reqwest::Response, Error> {
    // native 请求强制 `Accept-Encoding: identity`，因为流式协议解析需要看到原始 JSON/
    // SSE 字节；随后按协议选择认证 header。调用方的 `x-request-id` 只在 ASCII 且
    // 不超过 128 字节时透传。
    let endpoint = r
        .destination
        .settings
        .endpoint(
            r.destination.protocol,
            &r.destination.base,
            &r.connection.model,
            p.request.stream,
        )
        .map_err(|_| Error::input("invalid_upstream_endpoint", "upstream"))?;
    let mut request = b
        .client
        .post(endpoint)
        .header(header::CONTENT_TYPE, "application/json")
        .header(
            header::ACCEPT,
            if p.request.stream {
                "text/event-stream"
            } else {
                "application/json"
            },
        )
        .header(header::ACCEPT_ENCODING, "identity")
        .body(p.body.clone());
    request = match r.destination.protocol {
        UP::OpenaiResponses => {
            if r.destination.settings.is_azure() {
                request.header("api-key", r.destination.native_key.clone())
            } else {
                request.header(header::AUTHORIZATION, r.destination.authorization.clone())
            }
        }
        UP::AnthropicMessages => request
            .header("x-api-key", r.destination.native_key.clone())
            .header(
                "anthropic-version",
                &r.destination.settings.anthropic_version,
            ),
        UP::GeminiGenerateContent => {
            request.header("x-goog-api-key", r.destination.native_key.clone())
        }
        UP::OpenaiChat => return Err(Error::input("invalid_protocol", "protocol")),
    };
    if let Some(id) = headers
        .get("x-request-id")
        .filter(|v| v.as_bytes().len() <= 128 && v.as_bytes().is_ascii())
    {
        request = request.header("x-request-id", id.clone());
    }
    request.send().await.map_err(|e| {
        Error::upstream(if e.is_timeout() {
            "upstream_timeout"
        } else {
            "upstream_connection_error"
        })
    })
}
fn safe_headers(headers: &HeaderMap, r: &Route) -> HeaderMap {
    let mut out = HeaderMap::new();
    for (name, value) in headers {
        if !(matches!(name.as_str(), "retry-after" | "request-id" | "x-request-id")
            || name.as_str().starts_with("x-ratelimit-"))
        {
            continue;
        }
        if value.as_bytes().len() > 256 || !value.as_bytes().is_ascii() {
            continue;
        }
        if r.destination
            .native_key
            .to_str()
            .ok()
            .filter(|s| !s.is_empty())
            .is_some_and(|key| value.to_str().is_ok_and(|v| v.contains(key)))
        {
            continue;
        }
        out.insert(name.clone(), value.clone());
    }
    out
}
fn reject(guard: &mut Guard, e: &Error, kind: ErrorKind) {
    if let Some(g) = guard.as_mut() {
        g.finish(
            if e.status == 400 && e.param.is_some() || e.code == "conversation_state_expired" {
                Outcome::Rejected
            } else {
                Outcome::Failed
            },
            Some(if e.code == "upstream_http_error" {
                ErrorKind::UpstreamHttp
            } else {
                kind
            }),
        );
    }
}
fn failure(e: Error, headers: HeaderMap) -> Response<Body> {
    let mut response = Response::builder()
        .status(StatusCode::from_u16(e.status).unwrap_or(StatusCode::BAD_GATEWAY))
        .body(Body::from(e.envelope().to_string()))
        .unwrap();
    *response.headers_mut() = headers;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    response
}
fn validate_headers(response: &reqwest::Response, stream: bool) -> Result<(), Error> {
    // 压缩响应或不是所声明的 SSE/JSON content-type 时不能可靠解析，因此在这里拒绝，
    // 不把无法验证的字节伪装成 Chat 结果。
    if Protocol::is_encoded(header_text(response.headers(), "content-encoding").as_deref()) {
        return Err(Error::upstream("unsupported_content_encoding"));
    }
    let actual = Protocol::classify(header_text(response.headers(), "content-type").as_deref());
    if actual
        != if stream {
            Protocol::Sse
        } else {
            Protocol::Json
        }
    {
        return Err(Error::upstream("unsupported_upstream_content_type"));
    }
    Ok(())
}
fn pending(r: &Route, p: &ChatRequest, out: &mut Output) -> Result<Option<replay::Pending>, Error> {
    out.validate_request(p)?;
    out.replay
        .take()
        .map(|turn| replay::Pending::new(r.generation, &p.messages, &out.assistant, turn))
        .transpose()
}
fn event(value: &serde_json::Value) -> Bytes {
    Bytes::from(format!("data: {value}\n\n"))
}
fn observe(
    guard: &mut Guard,
    n: usize,
    content: bool,
    usage: Option<&protocol::usage::Usage>,
    model: Option<&str>,
) {
    if let Some(g) = guard.as_mut() {
        g.observe_native(n, content, usage.map(|u| u.outcome()), model);
    }
}

struct CacheTicket {
    bridge: Arc<Bridge>,
    id: String,
    /// 只有在转换结果真正交给 HTTP body stream 后才置为 true。
    delivered: bool,
}
impl Drop for CacheTicket {
    fn drop(&mut self) {
        if !self.delivered
            && let Ok(mut cache) = self.bridge.native_replay.lock()
        {
            cache.discard(&self.id);
        }
    }
}
fn publish(
    b: &Arc<Bridge>,
    pending: Option<replay::Pending>,
    out: &Output,
) -> Result<Option<CacheTicket>, Error> {
    // 只有 adapter 已解析完整输出并生成后续 replay 状态时才走到这里。成功立即把状态
    // commit 到进程内 cache，同时返回 delivered=false 的 ticket；真正交给 body stream
    // 前若 ticket 被 drop，会按 id 撤销这次发布。返回 `None` 表示本响应无需 replay。
    let Some(pending) = pending else {
        return Ok(None);
    };
    let id = out
        .assistant
        .calls
        .first()
        .ok_or_else(|| Error::upstream("invalid_replay_state"))?
        .id
        .clone();
    b.native_replay
        .lock()
        .unwrap()
        .commit(pending, Instant::now())?;
    Ok(Some(CacheTicket {
        bridge: b.clone(),
        id,
        delivered: false,
    }))
}

/// 转换一次原生协议代理请求并返回 Chat 兼容响应。
///
/// 调用来源是 `proxy::relay_to` 在统计 admission 和本地 key 检查之后识别到 native
/// 协议的分支。输入包含原始 Chat 请求、上游路由、原请求 headers/query、并发准入许可
/// `OwnedSemaphorePermit` 和可选 `RequestGuard`；输出可能是输入校验/转换/连接/HTTP/
/// 协议失败信封，或成功转换后的 JSON/SSE body。成功响应把 permit 与 guard 交给
/// body stream 持有。
///
/// 非流式路径要先读到上游 EOF、完成解析和 replay 校验，再构造响应；流式路径由
/// `NativeStream::next` 增量产出。guard 可从显式失败、客户端 drop 或关闭路径提交终态；
/// 上游 EOF 只说明转换输入结束，不能证明客户端 socket 已完整接收输出。
pub(super) async fn relay(
    b: Arc<Bridge>,
    r: Arc<Route>,
    bytes: Bytes,
    headers: HeaderMap,
    query: Option<String>,
    permit: tokio::sync::OwnedSemaphorePermit,
    mut guard: Guard,
) -> Response<Body> {
    // `permit` 和 `guard` 的所有权随本函数进入响应 body future。显式 finalize、客户端
    // drop、协议错误和上游 EOF 都汇入 RequestGuard 的终态规则；这里的 permit 只限制
    // 本进程并发准入，不是对上游真实并发数的外部保证。
    if query.as_ref().is_some_and(|q| !q.is_empty()) {
        let e = Error::input("unsupported_query", "query");
        reject(&mut guard, &e, ErrorKind::Other);
        return failure(e, HeaderMap::new());
    }
    let prepared = match prepare(&b, &r, &bytes) {
        Ok(p) => p,
        Err(e) => {
            reject(&mut guard, &e, ErrorKind::Other);
            return failure(e, HeaderMap::new());
        }
    };
    if let Some(g) = guard.as_mut() {
        g.mark_upstream_started();
        g.observe_native(0, false, None, None);
    }
    let response = match open(&b, &r, &prepared, &headers).await {
        Ok(res) => res,
        Err(e) => {
            reject(&mut guard, &e, ErrorKind::Connect);
            return failure(e, HeaderMap::new());
        }
    };
    b.calls.fetch_add(1, Ordering::Relaxed);
    b.set_status(format!(
        "{} · HTTP {}",
        r.destination.protocol.as_str(),
        response.status().as_u16()
    ));
    let mut safe = safe_headers(response.headers(), &r);
    if let Some(g) = guard.as_mut() {
        g.set_http_status(response.status().as_u16());
    }
    if !response.status().is_success() {
        let mut e = Error::upstream("upstream_http_error");
        e.status = response.status().as_u16();
        if let Some(g) = guard.as_mut() {
            g.finish(Outcome::Failed, Some(ErrorKind::UpstreamHttp));
        }
        return failure(e, safe);
    }
    if let Err(e) = validate_headers(&response, prepared.request.stream) {
        reject(&mut guard, &e, ErrorKind::UpstreamStream);
        return failure(e, safe);
    }
    if prepared.request.stream {
        safe.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("text/event-stream"),
        );
        safe.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
        let state = NativeStream {
            source: Some(Box::pin(response.bytes_stream())),
            input: Bytes::new(),
            offset: 0,
            parser: sse::Parser::default(),
            decoder: Some(
                streaming::Decoder::new(r.destination.protocol).expect("native protocol"),
            ),
            prepared: prepared.request,
            identity: Identity::new(&r.connection.model),
            bridge: b,
            route: r,
            permit: Some(permit),
            guard,
            pending_delta: None,
            controls: VecDeque::new(),
            role: false,
            finished: false,
            terminal_deadline: None,
            ticket: None,
        };
        // `unfold` 每次只保留一份 `NativeStream` 状态。`DeltaCursor` 把超大 delta
        // 切成小片，避免为了一个响应构建无界事件 VecDeque。
        let stream = stream::unfold(state, |mut state| async move {
            state.next().await.map(|item| (item, state))
        });
        let mut out = Response::new(Body::from_stream(stream));
        *out.headers_mut() = safe;
        return out;
    }
    let mut source = response.bytes_stream();
    let mut native = Vec::new();
    while let Some(chunk) = source.next().await {
        let chunk = match chunk {
            Ok(chunk) => chunk,
            Err(_) => {
                let e = Error::upstream("upstream_stream_error");
                reject(&mut guard, &e, ErrorKind::UpstreamStream);
                return failure(e, safe);
            }
        };
        observe(&mut guard, chunk.len(), false, None, None);
        if native.len() + chunk.len() > protocol::output::OUTPUT_LIMIT {
            let e = Error::upstream("upstream_output_too_large");
            reject(&mut guard, &e, ErrorKind::UpstreamStream);
            return failure(e, safe);
        }
        native.extend_from_slice(&chunk);
    }
    let mut out = match protocol::output::parse_json(r.destination.protocol, &native) {
        Ok(out) => out,
        Err(e) => {
            reject(&mut guard, &e, ErrorKind::UpstreamStream);
            return failure(e, safe);
        }
    };
    let mut identity = Identity::new(&r.connection.model);
    if let Some(model) = &out.model {
        identity.model = model.clone();
    }
    observe(
        &mut guard,
        0,
        !out.assistant.parts.is_empty() || !out.assistant.calls.is_empty(),
        Some(&out.usage),
        out.model.as_deref(),
    );
    let cached = match pending(&r, &prepared.request, &mut out) {
        Ok(cached) => cached,
        Err(e) => {
            reject(&mut guard, &e, ErrorKind::UpstreamStream);
            return failure(e, safe);
        }
    };
    let payload = Bytes::from(out.json(&identity).to_string());
    // 非流式响应会先等上游 EOF、完整 JSON 解析和 replay 校验全部成功，才发布缓存。
    // body stream 第一次被轮询、准备把完整 body 交给下一层时，ticket 才被标记
    // delivered；在此之前 drop 会撤销尚未交付的 replay 状态。delivered 仍只描述
    // “交给 HTTP body 层”，不证明客户端网络接收。
    let ticket = match publish(&b, cached, &out) {
        Ok(ticket) => ticket,
        Err(e) => {
            reject(&mut guard, &e, ErrorKind::UpstreamStream);
            return failure(e, safe);
        }
    };
    let body = stream::unfold(
        (Some(payload), guard, permit, ticket),
        |(payload, mut guard, permit, mut ticket)| async move {
            if let Some(payload) = payload {
                // The complete validated JSON is handed to the HTTP body in
                // one chunk. Before this point cancellation revokes the state.
                if let Some(ticket) = ticket.as_mut() {
                    ticket.delivered = true;
                }
                if let Some(g) = guard.as_mut() {
                    g.native_complete();
                    g.finish(Outcome::Completed, None);
                }
                return Some((Ok::<_, io::Error>(payload), (None, guard, permit, ticket)));
            }
            None
        },
    );
    safe.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    let mut response = Response::new(Body::from_stream(body));
    *response.headers_mut() = safe;
    response
}

/// Keeps one decoded delta, emitting one bounded Chat chunk at a time. A large
/// fallback snapshot cannot expand into an unbounded queue of serialized events.
struct DeltaCursor {
    strings: VecDeque<(String, String, usize)>,
    tools: VecDeque<serde_json::Value>,
    tool: Option<(serde_json::Value, String, usize, bool)>,
}
fn end(text: &str, start: usize) -> usize {
    let mut end = (start + 16 * 1024).min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    end
}
impl DeltaCursor {
    fn new(mut value: serde_json::Value) -> Self {
        let m = value.as_object_mut().expect("adapter delta object");
        let strings = ["content", "reasoning_content", "refusal"]
            .into_iter()
            .filter_map(|key| {
                m.remove(key).and_then(|v| match v {
                    serde_json::Value::String(s) => Some((key.into(), s, 0)),
                    _ => None,
                })
            })
            .collect();
        let tools = m
            .remove("tool_calls")
            .and_then(|v| match v {
                serde_json::Value::Array(rows) => Some(rows),
                _ => None,
            })
            .unwrap_or_default()
            .into();
        Self {
            strings,
            tools,
            tool: None,
        }
    }
    fn next(&mut self) -> Option<serde_json::Value> {
        // 文本字段和 tool arguments 都按 Unicode 字符边界截到 16 KiB 左右；每次只
        // 返回一个 Chat delta。协议兼容性依赖这个“渐进切片”语义，而不是把完整
        // snapshot 一次性发给客户端。
        if let Some((channel, text, offset)) = self.strings.front_mut() {
            let last = end(text, *offset);
            let value = json!({channel.as_str():&text[*offset..last]});
            *offset = last;
            if last == text.len() {
                self.strings.pop_front();
            }
            return Some(value);
        }
        if self.tool.is_none() {
            let mut row = self.tools.pop_front()?;
            let args = row["function"]
                .as_object_mut()
                .and_then(|f| f.remove("arguments"))
                .and_then(|v| match v {
                    serde_json::Value::String(s) => Some(s),
                    _ => None,
                })
                .unwrap_or_default();
            self.tool = Some((row, args, 0, true));
        }
        let (row, args, offset, first) = self.tool.as_mut().expect("current tool");
        let last = end(args, *offset);
        let mut value = if *first {
            row.clone()
        } else {
            json!({"index":row["index"],"function":{}})
        };
        value["function"]["arguments"] = json!(&args[*offset..last]);
        *offset = last;
        *first = false;
        if last == args.len() {
            self.tool = None;
        }
        Some(json!({"tool_calls":[value]}))
    }
}
struct NativeStream {
    source: Option<SourceStream>,
    input: Bytes,
    offset: usize,
    parser: sse::Parser,
    decoder: Option<streaming::Decoder>,
    prepared: ChatRequest,
    identity: Identity,
    bridge: Arc<Bridge>,
    route: Arc<Route>,
    permit: Option<tokio::sync::OwnedSemaphorePermit>,
    guard: Guard,
    pending_delta: Option<DeltaCursor>,
    controls: VecDeque<Bytes>,
    role: bool,
    finished: bool,
    terminal_deadline: Option<Instant>,
    ticket: Option<CacheTicket>,
}
impl NativeStream {
    fn fail(&mut self, e: Error) -> Option<Result<Bytes, io::Error>> {
        if let Some(g) = self.guard.as_mut() {
            g.finish(Outcome::Failed, Some(ErrorKind::UpstreamStream));
        }
        self.source = None;
        self.decoder = None;
        self.permit = None;
        self.finished = true;
        self.pending_delta = None;
        self.controls.clear();
        self.ticket = None;
        Some(Ok(event(&e.envelope())))
    }
    /// Produces the next converted Chat stream item.
    ///
    /// `Some` 交付一个控制帧或增量 delta，`None` 表示转换流结束。正常结束要求上游
    /// SSE/termination、adapter finalize 和 replay 发布依次成功；协议、超时或 transport
    /// 错误会返回一条协议错误事件并显式结束 guard。本函数随 body stream 一起被轮询，
    /// 因此客户端停止轮询并 drop 外层 body 时，默认取消路径由 `RequestGuard::Drop` 处理。
    async fn next(&mut self) -> Option<Result<Bytes, io::Error>> {
        // 先发控制帧，再发可能被切片的 delta；最后才等待上游下一块。terminal marker
        // 之后仍有短暂宽限期等待干净 EOF。只有 parser EOF 和 decoder.finish 都成功，
        // `pending` 的 replay 才会发布；检测到错误时走 `fail`。
        loop {
            if let Some(control) = self.controls.pop_front() {
                if control.as_ref() == b"data: [DONE]\n\n" {
                    if let Some(ticket) = self.ticket.as_mut() {
                        ticket.delivered = true;
                    }
                    if let Some(g) = self.guard.as_mut() {
                        g.native_complete();
                        g.finish(Outcome::Completed, None);
                    }
                }
                return Some(Ok(control));
            }
            if let Some(cursor) = self.pending_delta.as_mut() {
                if let Some(delta) = cursor.next() {
                    return Some(Ok(event(&self.identity.chunk(delta))));
                }
                self.pending_delta = None;
            }
            if self.finished {
                if let Some(g) = self.guard.as_mut() {
                    g.native_complete();
                    g.finish(Outcome::Completed, None);
                }
                return None;
            }
            if self.offset < self.input.len() {
                let (used, frame) = match self.parser.feed(&self.input[self.offset..]) {
                    Ok(v) => v,
                    Err(e) => return self.fail(e),
                };
                self.offset += used;
                if let Some(frame) = frame {
                    let frame = match frame.into_event() {
                        Ok(Some(event)) => event,
                        Ok(None) => continue,
                        Err(e) => return self.fail(e),
                    };
                    let decoder = self.decoder.as_mut().expect("live decoder");
                    let step = match decoder.event(frame) {
                        Ok(step) => step,
                        Err(e) => return self.fail(e),
                    };
                    let has_content = step
                        .delta
                        .get("content")
                        .and_then(serde_json::Value::as_str)
                        .is_some_and(|s| !s.is_empty())
                        || step.delta.get("tool_calls").is_some();
                    observe(
                        &mut self.guard,
                        0,
                        has_content,
                        Some(decoder.usage()),
                        decoder.model(),
                    );
                    if step.confirmed && !self.role {
                        if let Some(model) = decoder.model() {
                            self.identity.model = model.into();
                        }
                        self.role = true;
                        self.controls
                            .push_back(event(&self.identity.chunk(json!({"role":"assistant"}))));
                    }
                    if decoder.terminal_seen() && self.terminal_deadline.is_none() {
                        self.terminal_deadline = Some(Instant::now() + Duration::from_secs(5));
                    }
                    self.pending_delta = Some(DeltaCursor::new(step.delta));
                    continue;
                }
            }
            self.input = Bytes::new();
            self.offset = 0;
            let wait = self
                .terminal_deadline
                .map(|end| end.saturating_duration_since(Instant::now()))
                .unwrap_or(Duration::from_secs(180));
            let next =
                match tokio::time::timeout(wait, self.source.as_mut().expect("live source").next())
                    .await
                {
                    Ok(next) => next,
                    Err(_) => return self.fail(Error::upstream("upstream_timeout")),
                };
            match next {
                Some(Ok(chunk)) => {
                    observe(&mut self.guard, chunk.len(), false, None, None);
                    self.input = chunk;
                }
                Some(Err(_)) => return self.fail(Error::upstream("upstream_stream_error")),
                None => {
                    // 这是上游传输结束点，不是客户端 socket 已确认收到的证明。转换后的
                    // 事件会继续由 body stream 交给 HTTP server；客户端取消由外层
                    // RequestGuard 的 Drop 另行分类。
                    if let Err(e) = self.parser.finish() {
                        return self.fail(e);
                    }
                    let mut out = match self.decoder.take().expect("live decoder").finish() {
                        Ok(out) => out,
                        Err(e) => return self.fail(e),
                    };
                    observe(
                        &mut self.guard,
                        0,
                        false,
                        Some(&out.usage),
                        out.model.as_deref(),
                    );
                    let cached = match pending(&self.route, &self.prepared, &mut out) {
                        Ok(cached) => cached,
                        Err(e) => return self.fail(e),
                    };
                    self.ticket = match publish(&self.bridge, cached, &out) {
                        Ok(ticket) => ticket,
                        Err(e) => return self.fail(e),
                    };
                    self.controls
                        .push_back(event(&self.identity.finish_chunk(out.finish)));
                    if self.prepared.include_usage && out.usage.complete {
                        self.controls
                            .push_back(event(&self.identity.usage_chunk(&out.usage)));
                    }
                    self.controls
                        .push_back(Bytes::from_static(b"data: [DONE]\n\n"));
                    self.source = None;
                    self.finished = true;
                }
            }
        }
    }
}

pub(super) async fn test(
    b: &Bridge,
    r: &Route,
    guard: &mut Guard,
    report: &mut crate::diagnostic::TestReport,
    started: Instant,
) -> Result<(), String> {
    // 连接测试构造最小的流式 Chat 请求，只等待 adapter 确认首事件。确认后主动停止读取，
    // 因而把统计记为 cancelled/client_drop，而测试本身返回成功；HTTP/协议错误仍返回
    // Err。整个异步探测由 45 秒 timeout 包围，超时具有独立错误文案。
    report.url = r
        .destination
        .settings
        .endpoint(
            r.destination.protocol,
            &r.destination.base,
            &r.connection.model,
            true,
        )
        .map_err(|_| "Invalid upstream endpoint")?
        .to_string();
    let result = async {
        let bytes=serde_json::to_vec(&json!({"model":r.connection.model,"stream":true,"messages":[{"role":"user","content":"Reply with OK."}]})).unwrap();
        let prepared = prepare(b, r, &bytes)?;
        if let Some(g) = guard.as_mut() {
            g.mark_upstream_started();
            g.observe_native(0, false, None, None);
        }
        let response = open(b, r, &prepared, &HeaderMap::new()).await?;
        report.headers_ms = Some(started.elapsed().as_millis() as u64);
        report.http = format!("HTTP {}", response.status().as_u16());
        if let Some(g) = guard.as_mut() {
            g.set_http_status(response.status().as_u16());
        }
        if !response.status().is_success() {
            let mut error = Error::upstream("upstream_http_error");
            error.status = response.status().as_u16();
            return Err(error);
        }
        validate_headers(&response, true)?;
        let mut source = response.bytes_stream();
        let mut parser = sse::Parser::default();
        let mut decoder = streaming::Decoder::new(r.destination.protocol)?;
        while let Some(chunk) = source.next().await {
            let chunk = chunk.map_err(|_| Error::upstream("upstream_stream_error"))?;
            observe(guard, chunk.len(), false, None, None);
            let mut offset = 0;
            while offset < chunk.len() {
                let (n, frame) = parser.feed(&chunk[offset..])?;
                offset += n;
                if let Some(frame) = frame
                    && let Some(event) = frame.into_event()?
                {
                    let step = decoder.event(event)?;
                    observe(
                        guard,
                        0,
                        !step.delta.as_object().expect("delta object").is_empty(),
                        Some(decoder.usage()),
                        decoder.model(),
                    );
                    if step.confirmed {
                        report.first_event_ms = Some(started.elapsed().as_millis() as u64);
                        report.response_model = decoder.model().map(str::to_owned);
                        if let Some(g) = guard.as_mut() {
                            g.finish(Outcome::Cancelled, Some(ErrorKind::ClientDrop));
                        }
                        return Ok(());
                    }
                }
            }
        }
        Err(Error::upstream("incomplete_upstream_stream"))
    };
    match tokio::time::timeout(Duration::from_secs(45), result).await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(e)) => {
            reject(guard, &e, ErrorKind::UpstreamStream);
            Err(format!(
                "{} ({})",
                e.envelope()["error"]["message"]
                    .as_str()
                    .unwrap_or("Protocol error"),
                e.code
            ))
        }
        Err(_) => {
            if let Some(g) = guard.as_mut() {
                g.finish(Outcome::Failed, Some(ErrorKind::Connect));
            }
            Err("Connection timeout / 连接超时".into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture {
        _dir: tempfile::TempDir,
        b: Arc<Bridge>,
        r: Arc<Route>,
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            self.b.stats.shutdown();
        }
    }
    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let mut config = Config::default();
        let p = &mut config.providers[0];
        p.protocol = UP::OpenaiResponses;
        p.upstream = "https://example.test/gateway/v1".into();
        p.api_key = "fixture-provider-key".into();
        p.models
            .push(crate::config::Model::new("native-test", "Native"));
        let id = p.models[0].id.clone();
        let b = Arc::new(Bridge::new(config, dir.path().into()).unwrap());
        let r = b.routes.lock().unwrap().ids[&id].clone();
        Fixture { _dir: dir, b, r }
    }
    #[test]
    fn bounded_delta_cursor_preserves_unicode_and_arguments_without_building_an_event_queue() {
        let content = "中文\0".repeat(20000);
        let args = serde_json::to_string(&json!({"q":content})).unwrap();
        let mut cursor = DeltaCursor::new(
            json!({"content":content,"tool_calls":[{"index":0,"id":"wbpt_test","type":"function","function":{"name":"find","arguments":args}}]}),
        );
        let mut text = String::new();
        let mut arguments = String::new();
        let mut metadata = 0;
        while let Some(delta) = cursor.next() {
            assert!(serde_json::to_vec(&delta).unwrap().len() < 128 * 1024);
            if let Some(s) = delta["content"].as_str() {
                text.push_str(s);
            }
            if let Some(call) = delta["tool_calls"].as_array().and_then(|v| v.first()) {
                if call.get("id").is_some() {
                    metadata += 1;
                }
                arguments.push_str(call["function"]["arguments"].as_str().unwrap());
            }
        }
        assert_eq!(text, content);
        assert_eq!(arguments, args);
        assert_eq!(metadata, 1);
    }
    #[test]
    fn undelivered_replay_ticket_is_revoked_and_delivered_ticket_survives() {
        let f = fixture();
        let request=ChatRequest::parse(br#"{"model":"native-test","messages":[{"role":"user","content":"hello"}],"tools":[{"type":"function","function":{"name":"find"}}]}"#).unwrap();
        for delivered in [false, true] {
            let mut out=protocol::output::decode(UP::OpenaiResponses,json!({"status":"completed","output":[{"type":"function_call","call_id":"native-id","name":"find","arguments":"{}"}]})).unwrap();
            let pending = pending(&f.r, &request, &mut out).unwrap();
            let mut ticket = publish(&f.b, pending, &out).unwrap().unwrap();
            let mut messages = request.messages.clone();
            messages.push(out.assistant.clone());
            assert!(
                f.b.native_replay
                    .lock()
                    .unwrap()
                    .resolve(&f.r.generation, &messages, Instant::now())
                    .is_ok()
            );
            ticket.delivered = delivered;
            drop(ticket);
            assert_eq!(
                f.b.native_replay
                    .lock()
                    .unwrap()
                    .resolve(&f.r.generation, &messages, Instant::now())
                    .is_ok(),
                delivered
            );
        }
    }
    #[test]
    fn converted_response_headers_never_reuse_payload_metadata_or_cookies() {
        let f = fixture();
        let mut headers = HeaderMap::new();
        for (key, value) in [
            ("content-length", "123"),
            ("content-encoding", "gzip"),
            ("etag", "old"),
            ("set-cookie", "secret"),
            ("digest", "old"),
            ("x-request-id", "fixture-provider-key"),
            ("retry-after", "2"),
            ("x-ratelimit-limit-requests", "100"),
        ] {
            headers.insert(
                axum::http::HeaderName::from_static(key),
                HeaderValue::from_static(value),
            );
        }
        let safe = safe_headers(&headers, &f.r);
        assert_eq!(safe.len(), 2);
        assert_eq!(safe["retry-after"], "2");
        assert_eq!(safe["x-ratelimit-limit-requests"], "100");
    }
}
