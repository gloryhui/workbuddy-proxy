//! native 转换专用的 Chat Completions 请求模型与严格校验。
//!
//! 上游调用方是 `proxy::native::prepare`。它把已经通过 HTTP 鉴权和 8 MiB body
//! 限制的原始字节交给 [`ChatRequest::parse`]；本模块将那些字节整理成一个与供应商
//! 无关、拥有自身数据的 [`ChatRequest`]。下游 `protocol::replay` 读取消息历史，
//! `protocol::encode` 再按 Responses、Anthropic 或 Gemini 的字段重新编码。
//!
//! 解析边界刻意采用“拒绝而不是猜测”：JSON 必须无重复键、无尾随值，字段名必须在
//! 白名单内，布尔/数字/字符串不会做隐式类型转换。重点是保护转换层的可解释性，而不
//! 是兼容 Chat Completions 的全部可选扩展。
//!
//! 本模块纯同步、无网络和无线程。输入切片只在校验期间借用；返回的字符串、工具参数
//! 和图片 data 都由结构体拥有，因此原始 HTTP body 可以在之后释放。`model` 必须存在
//! 才能通过解析，但它只用于调用方选路，不会存入 [`ChatRequest`]；native 请求里的
//! 上游 model 来自已经解析的 `proxy::Route`。
use super::Error;
use serde::de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};

/// native Chat body 的原始字节上限，也是 `proxy::BODY_LIMIT` 的同一数量级边界。
///
/// 这是一次请求完整 JSON 的字节数，不是 token 数。等于上限的 body 可进入 JSON 解析，
/// 只有大于上限才立即返回 `request_too_large`。
pub const INPUT_LIMIT: usize = 8 * 1024 * 1024;
/// 单个 assistant `tool_calls[].function.arguments` 原始文本上限。
///
/// 上限按 UTF-8 字节计；文本还必须是合法、无重复键的 JSON object。
pub const TOOL_ARGUMENT_LIMIT: usize = 256 * 1024;
/// 单次请求可声明的最多 function tool 数量。
///
/// 同一个 assistant 消息携带的 tool call 也使用这个数组数量上限；历史中多轮调用
/// 由工具历史校验分别保证完整性，并不共享一个“总共只能 64 个”的全局计数。
pub const TOOL_LIMIT: usize = 64;

/// `serde_json` 的自定义反序列化种子，在构建 `Value` 时同时执行两项严格检查。
///
/// `depth` 从根值 0 开始，进入每个数组元素或对象值前加一；准备解析更深一层时如果
/// 当前值已大于 64，就拒绝。对象构建还检查当前对象内是否已有同名键，因此重复键
/// 在任意嵌套层级都会失败。它不会推断 JSON Schema 或业务字段，只保证 JSON 树本身
/// 无歧义且有限深。
struct Checked {
    depth: usize,
}
impl<'de> DeserializeSeed<'de> for Checked {
    type Value = Value;
    fn deserialize<D: de::Deserializer<'de>>(self, d: D) -> Result<Value, D::Error> {
        if self.depth > 64 {
            return Err(de::Error::custom("JSON nesting limit exceeded"));
        }
        d.deserialize_any(self)
    }
}
impl<'de> Visitor<'de> for Checked {
    type Value = Value;
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("unambiguous JSON")
    }
    fn visit_bool<E: de::Error>(self, v: bool) -> Result<Value, E> {
        Ok(Value::Bool(v))
    }
    fn visit_i64<E: de::Error>(self, v: i64) -> Result<Value, E> {
        Ok(Value::from(v))
    }
    fn visit_u64<E: de::Error>(self, v: u64) -> Result<Value, E> {
        Ok(Value::from(v))
    }
    fn visit_f64<E: de::Error>(self, v: f64) -> Result<Value, E> {
        serde_json::Number::from_f64(v)
            .map(Value::Number)
            .ok_or_else(|| E::custom("Invalid number"))
    }
    fn visit_str<E: de::Error>(self, v: &str) -> Result<Value, E> {
        Ok(Value::String(v.into()))
    }
    fn visit_string<E: de::Error>(self, v: String) -> Result<Value, E> {
        Ok(Value::String(v))
    }
    fn visit_unit<E: de::Error>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }
    fn visit_none<E: de::Error>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<Value, A::Error> {
        let mut out = Vec::new();
        while let Some(v) = a.next_element_seed(Checked {
            depth: self.depth + 1,
        })? {
            out.push(v);
        }
        Ok(Value::Array(out))
    }
    fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<Value, A::Error> {
        let mut out = Map::new();
        while let Some(key) = a.next_key::<String>()? {
            if out.contains_key(&key) {
                return Err(de::Error::custom("Duplicate JSON key"));
            }
            let value = a.next_value_seed(Checked {
                depth: self.depth + 1,
            })?;
            out.insert(key, value);
        }
        Ok(Value::Object(out))
    }
}

/// 把字节解析成严格 JSON 树，同时执行长度和嵌套边界。
///
/// 执行步骤：
///
/// 1. 先比较调用方给定的字节上限；
/// 2. 使用 [`Checked`] 构建 `Value`，拒绝重复对象键、非有限数字和过深嵌套；
/// 3. 调用 `Deserializer::end`，拒绝 `{}` 后继续出现第二个值的输入。
///
/// JSON 语法、重复键、深度或尾随值问题都归一为 `invalid_json`，超限则为
/// `request_too_large`。错误不回显原 body。这里也不会解析 `arguments` 的业务含义；
/// 工具参数调用点会再用一个较小的字节上限递归调用本函数。
pub fn strict_json(bytes: &[u8], limit: usize) -> Result<Value, Error> {
    if bytes.len() > limit {
        return Err(Error::input("request_too_large", "request"));
    }
    let mut decoder = serde_json::Deserializer::from_slice(bytes);
    let value = Checked { depth: 0 }
        .deserialize(&mut decoder)
        .map_err(|_| Error::input("invalid_json", "request"))?;
    decoder
        .end()
        .map_err(|_| Error::input("invalid_json", "request"))?;
    Ok(value)
}

/// 消息中的一个内容部分。
///
/// 统一模型只保留当前 native 映射能明确处理的文本与图片。图片 data 字段保存 base64
/// 文本，不是解码后的二进制；`mime` 只可能是 `image/png` 或 `image/jpeg`。
/// `detail` 是原始 Chat 图片细节档位，是否受具体协议支持由编码阶段判断。
#[derive(Clone, Debug, PartialEq)]
pub enum Part {
    Text(String),
    ImageData {
        mime: String,
        data: String,
        detail: String,
    },
    ImageUrl {
        url: String,
        detail: String,
    },
}

/// 统一消息角色。
///
/// Chat 协议中的 `system` 与 `developer` 会分别保留；Anthropic 和 Gemini 编码时
/// 只能把它们合并到对话开头的 system 区域。`Tool` 表示一条工具执行结果，必须通过
/// `tool_call_id` 与此前的 assistant 调用配对。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    System,
    Developer,
    User,
    Assistant,
    Tool,
}

/// assistant 发起的一次 function tool 调用。
///
/// `arguments` 是已经通过严格 JSON 校验并确认是 object 的解析值；
/// `arguments_text` 同时保存客户端原始参数文本。两者都要保留：native 编码通常需要
/// JSON 对象，而 replay 和部分协议字段需要原始文本语义，不能从重新序列化结果反推
/// 客户端实际发送的“原文”。这不表示原文会进入日志或统计事件。
#[derive(Clone, Debug)]
pub struct ToolCall {
    /// Chat 历史中的调用 ID；同一段历史内必须唯一，长度和字符规则在解析时检查。
    pub id: String,
    /// function 名称；不等于工具声明中的显示描述。
    pub name: String,
    /// 已解析、且已确认是 JSON object 的参数值。
    pub arguments: Value,
    /// 客户端给出的参数 JSON 文本，用于保留实际发送表示。
    pub arguments_text: String,
}

/// 统一历史中的一条消息。
///
/// 一条 assistant 消息可以同时有可见文本和 tool calls；tool 消息则使用
/// `tool_call_id` 指向对应调用。统一模型没有独立的 ToolResult 枚举变体；tool
/// content 的字符串会存成 `parts` 中的 [`Part::Text`]。`parts` 也可能包含用户图片，
/// `calls` 只出现在 assistant 消息。解析成功后这些关系已由
/// [`validate_tool_history`] 验证。
#[derive(Clone, Debug)]
pub struct Message {
    /// 消息在对话中的语义角色。
    pub role: Role,
    /// 按客户端顺序保存的文本/图片部分。
    pub parts: Vec<Part>,
    /// assistant 本轮发起的 function 调用；其他角色为空。
    pub calls: Vec<ToolCall>,
    /// tool 结果指向的调用 ID；只有 [`Role::Tool`] 为 `Some`。
    pub tool_call_id: Option<String>,
}
impl Message {
    /// 按 `parts` 顺序拼接所有文本，忽略图片。
    ///
    /// 当前转换层在 Responses assistant history、工具输出以及 Anthropic/Gemini
    /// 工具结果中使用这个文本投影。它不插入分隔符，也不会把图片转换成描述文本。
    pub fn text(&self) -> String {
        self.parts
            .iter()
            .filter_map(|p| match p {
                Part::Text(s) => Some(s.as_str()),
                _ => None,
            })
            .collect()
    }
}

/// 客户端声明的一个 function tool。
///
/// 当前只支持 `type == "function"`。`parameters` 必须为 JSON object；缺失时解析器
/// 会补一个 `{"type":"object","properties":{}}`。`strict` 的 `None` 与显式
/// `Some(false)` 在结构上不同，具体是否写出、默认成什么或是否支持由各协议编码器
/// 决定。
#[derive(Clone, Debug)]
pub struct Tool {
    pub name: String,
    pub description: Option<String>,
    pub parameters: Value,
    pub strict: Option<bool>,
}

/// 统一消息要求模型如何使用工具。
///
/// 缺省选择是 `Auto`。`None` 禁止工具调用；`Required` 向模型表达“优先调用工具”
/// 的请求，并不保证上游在所有正常终态都一定返回调用；`Named` 只允许指定函数并已在
/// 解析时确认该函数存在。编码器再把这些语义映射为各协议的字符串或对象形式。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToolChoice {
    Auto,
    None,
    Required,
    Named(String),
}

/// native 转换层使用的供应商无关 Chat 请求。
///
/// 该结构只保存 request body 中会影响 native 转换的语义。客户端用于选路的 `model`
/// 不在这里；调用方先读取它以解析出 `proxy::Route`，encode 随后使用路由携带的真实
/// 上游 model。`ChatRequest` 自身拥有消息、工具和响应格式值，生命周期不依赖原
/// `bytes`。
///
/// `None`、空集合和布尔 `false` 不可互换：
///
/// - `max_output_tokens == None` 表示客户端未指定；Anthropic 可回退到配置默认值；
/// - `stop == []` 表示未设置停止序列，编码器不生成停止字段；
/// - `reasoning_effort == None` 表示不请求思考设置，而 `Some("none")` 是显式关闭；
/// - `temperature == Some(0.0)` 是合法数值，不能被当作“未设置”。
#[derive(Clone, Debug)]
pub struct ChatRequest {
    /// 客户端是否要求流式 Chat 响应；默认 false。
    pub stream: bool,
    /// 是否要求流式响应包含 usage；只有 `stream=true` 且解析合法时才为 true。
    pub include_usage: bool,
    /// 至少一条、已通过角色和工具配对检查的历史消息。
    pub messages: Vec<Message>,
    /// function 工具声明，最多 [`TOOL_LIMIT`] 个且名称唯一。
    pub tools: Vec<Tool>,
    /// 工具选择策略；没有客户端字段或没有 tools 时的编码结果要看协议规则。
    pub tool_choice: ToolChoice,
    /// 是否允许并行工具调用；`None` 表示客户端未指定，不能当作 false。
    pub parallel_tool_calls: Option<bool>,
    /// 统一输出 token 上限；`max_tokens` 与 `max_completion_tokens` 的合并结果。
    pub max_output_tokens: Option<u32>,
    /// 采样温度，解析范围为闭区间 0 到 2。
    pub temperature: Option<f64>,
    /// nucleus sampling 上限，解析范围为闭区间 0 到 1。
    pub top_p: Option<f64>,
    /// 停止序列；字符串标量会变成一个元素，数组最多四个且每个非空。
    pub stop: Vec<String>,
    /// 统一思考 effort；支持集合在本层固定，native 能否使用还取决于配置和能力。
    pub reasoning_effort: Option<String>,
    /// 已做外层白名单检查的 `text`、`json_object` 或 `json_schema` 配置。
    pub response_format: Option<Value>,
}

fn object<'a>(v: &'a Value, path: &str) -> Result<&'a Map<String, Value>, Error> {
    v.as_object()
        .ok_or_else(|| Error::input("invalid_parameter", path))
}
fn keys(m: &Map<String, Value>, allowed: &[&str], path: &str) -> Result<(), Error> {
    if m.keys().any(|k| !allowed.contains(&k.as_str())) {
        return Err(Error::input("unsupported_parameter", path));
    }
    Ok(())
}
fn string<'a>(v: Option<&'a Value>, path: &str) -> Result<&'a str, Error> {
    v.and_then(Value::as_str)
        .ok_or_else(|| Error::input("invalid_parameter", path))
}
fn optional_bool(m: &Map<String, Value>, key: &str, path: &str) -> Result<Option<bool>, Error> {
    m.get(key)
        .map(|v| {
            v.as_bool()
                .ok_or_else(|| Error::input("invalid_parameter", path))
        })
        .transpose()
}
fn token_limit(m: &Map<String, Value>, key: &str) -> Result<Option<u32>, Error> {
    m.get(key)
        .map(|v| {
            v.as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .filter(|n| *n > 0)
                .ok_or_else(|| Error::input("invalid_parameter", key))
        })
        .transpose()
}
fn number(m: &Map<String, Value>, key: &str, max: f64) -> Result<Option<f64>, Error> {
    m.get(key)
        .map(|v| {
            v.as_f64()
                .filter(|n| *n >= 0. && *n <= max)
                .ok_or_else(|| Error::input("invalid_parameter", key))
        })
        .transpose()
}
fn name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
}

impl ChatRequest {
    /// 解析并校验一次 native Chat 请求。
    ///
    /// 输入 `bytes` 是调用方已鉴权、且通常已受 HTTP body 限制的原始 JSON；函数再次
    /// 应用 [`INPUT_LIMIT`]，因此直接调用者也不能绕过边界。输出是一个拥有数据的
    /// [`ChatRequest`]，不返回选路 model，也不发起任何副作用。
    ///
    /// 失败统一使用 [`Error`]：JSON 层是 `invalid_json`/`request_too_large`；未知字段
    /// 是 `unsupported_parameter`；字段类型、数值范围、角色、图片、工具声明或历史
    /// 配对问题使用对应的结构化 `invalid_*`、`unsupported_*`、`missing_*`、
    /// `duplicate_*` 或 `incomplete_tool_results`。调用方只把错误映射成 HTTP 响应和
    /// 统计终态，不保留原始 prompt。
    ///
    /// 执行顺序是严格 JSON 与顶层白名单、通用标量限制、消息与工具历史、工具声明与
    /// choice、停止/思考/响应格式，最后构造结构体。后面的阶段不会修正前面已发现的
    /// 非法输入。
    pub fn parse(bytes: &[u8]) -> Result<Self, Error> {
        // 阶段一：无歧义 JSON、顶层 object 和尚未由本转换层实现的字段白名单。
        let value = strict_json(bytes, INPUT_LIMIT)?;
        let m = object(&value, "request")?;
        keys(
            m,
            &[
                "model",
                "messages",
                "stream",
                "stream_options",
                "n",
                "tools",
                "tool_choice",
                "parallel_tool_calls",
                "max_tokens",
                "max_completion_tokens",
                "temperature",
                "top_p",
                "stop",
                "reasoning_effort",
                "response_format",
            ],
            "request",
        )?;
        if string(m.get("model"), "model")?.trim().is_empty() {
            return Err(Error::input("invalid_parameter", "model"));
        }
        if m.get("n").is_some_and(|v| v.as_u64() != Some(1)) {
            return Err(Error::input("unsupported_parameter", "n"));
        }
        let stream = optional_bool(m, "stream", "stream")?.unwrap_or(false);
        let include_usage = if let Some(options) = m.get("stream_options") {
            if !stream {
                return Err(Error::input("unsupported_parameter", "stream_options"));
            }
            let options = object(options, "stream_options")?;
            keys(options, &["include_usage"], "stream_options")?;
            optional_bool(options, "include_usage", "stream_options.include_usage")?
                .unwrap_or(false)
        } else {
            false
        };
        // 两个输出上限字段可同时出现，但只能相等；布尔、数值和 token 上限都不做
        // 字符串或浮点等价转换。
        let first = token_limit(m, "max_tokens")?;
        let second = token_limit(m, "max_completion_tokens")?;
        if first.zip(second).is_some_and(|(a, b)| a != b) {
            return Err(Error::input(
                "conflicting_parameters",
                "max_completion_tokens",
            ));
        }
        let messages = m
            .get("messages")
            .and_then(Value::as_array)
            .filter(|v| !v.is_empty())
            .ok_or_else(|| Error::input("invalid_parameter", "messages"))?;
        let messages = messages
            .iter()
            .enumerate()
            .map(|(i, v)| parse_message(v, i))
            .collect::<Result<Vec<_>, _>>()?;
        validate_tool_history(&messages)?;
        // 阶段二：工具声明只接受 function，名称全局唯一。tool_choice 在工具集合之后
        // 解析，因此 Named 可以立即验证目标存在。
        let tools = match m.get("tools") {
            None => Vec::new(),
            Some(v) => {
                let rows = v
                    .as_array()
                    .filter(|v| v.len() <= TOOL_LIMIT)
                    .ok_or_else(|| Error::input("invalid_parameter", "tools"))?;
                rows.iter()
                    .enumerate()
                    .map(|(i, v)| parse_tool(v, i))
                    .collect::<Result<Vec<_>, _>>()?
            }
        };
        let mut names = BTreeSet::new();
        if tools.iter().any(|tool| !names.insert(&tool.name)) {
            return Err(Error::input("duplicate_tool_name", "tools"));
        }
        let tool_choice = match m.get("tool_choice") {
            None => ToolChoice::Auto,
            Some(Value::String(v)) => match v.as_str() {
                "auto" => ToolChoice::Auto,
                "none" => ToolChoice::None,
                "required" => ToolChoice::Required,
                _ => return Err(Error::input("invalid_parameter", "tool_choice")),
            },
            Some(v) => {
                let v = object(v, "tool_choice")?;
                keys(v, &["type", "function"], "tool_choice")?;
                if string(v.get("type"), "tool_choice.type")? != "function" {
                    return Err(Error::input("unsupported_parameter", "tool_choice.type"));
                }
                let f = object(
                    v.get("function").unwrap_or(&Value::Null),
                    "tool_choice.function",
                )?;
                keys(f, &["name"], "tool_choice.function")?;
                let tool = string(f.get("name"), "tool_choice.function.name")?;
                if !names.contains(&tool.to_owned()) {
                    return Err(Error::input("unknown_tool", "tool_choice.function.name"));
                }
                ToolChoice::Named(tool.into())
            }
        };
        if tools.is_empty() && matches!(tool_choice, ToolChoice::Required | ToolChoice::Named(_)) {
            return Err(Error::input("missing_tools", "tool_choice"));
        }
        // 阶段三：解析会在编码阶段映射到供应商字段的通用生成参数。
        let stop = match m.get("stop") {
            None => vec![],
            Some(Value::String(v)) if !v.is_empty() => vec![v.clone()],
            Some(Value::Array(v)) if !v.is_empty() && v.len() <= 4 => v
                .iter()
                .map(|v| {
                    v.as_str()
                        .filter(|s| !s.is_empty())
                        .map(String::from)
                        .ok_or_else(|| Error::input("invalid_parameter", "stop"))
                })
                .collect::<Result<_, _>>()?,
            _ => return Err(Error::input("invalid_parameter", "stop")),
        };
        let reasoning_effort = m
            .get("reasoning_effort")
            .map(|v| {
                string(Some(v), "reasoning_effort").and_then(|s| {
                    if matches!(
                        s,
                        "none" | "minimal" | "low" | "medium" | "high" | "xhigh" | "max"
                    ) {
                        Ok(s.into())
                    } else {
                        Err(Error::input("unsupported_parameter", "reasoning_effort"))
                    }
                })
            })
            .transpose()?;
        let response_format = m
            .get("response_format")
            .map(|v| {
                let f = object(v, "response_format")?;
                match string(f.get("type"), "response_format.type")? {
                    "text" => keys(f, &["type"], "response_format")?,
                    "json_object" => keys(f, &["type"], "response_format")?,
                    "json_schema" => {
                        keys(f, &["type", "json_schema"], "response_format")?;
                        let schema = object(
                            f.get("json_schema").unwrap_or(&Value::Null),
                            "response_format.json_schema",
                        )?;
                        keys(
                            schema,
                            &["name", "description", "schema", "strict"],
                            "response_format.json_schema",
                        )?;
                        if !name(string(
                            schema.get("name"),
                            "response_format.json_schema.name",
                        )?) {
                            return Err(Error::input(
                                "invalid_parameter",
                                "response_format.json_schema.name",
                            ));
                        }
                        object(
                            schema.get("schema").unwrap_or(&Value::Null),
                            "response_format.json_schema.schema",
                        )?;
                        optional_bool(schema, "strict", "response_format.json_schema.strict")?;
                        if let Some(d) = schema.get("description") {
                            string(Some(d), "response_format.json_schema.description")?;
                        }
                    }
                    _ => {
                        return Err(Error::input(
                            "unsupported_parameter",
                            "response_format.type",
                        ));
                    }
                }
                Ok(v.clone())
            })
            .transpose()?;
        Ok(Self {
            stream,
            include_usage,
            messages,
            tools,
            tool_choice,
            parallel_tool_calls: optional_bool(m, "parallel_tool_calls", "parallel_tool_calls")?,
            max_output_tokens: first.or(second),
            temperature: number(m, "temperature", 2.)?,
            top_p: number(m, "top_p", 1.)?,
            stop,
            reasoning_effort,
            response_format,
        })
    }
}

/// 解析一个 function tool 声明。
///
/// `path` 来自数组索引，用于错误定位。当前白名单只允许外层 `type`/`function`，
/// function 内只允许 `name`、`description`、`parameters`、`strict`。缺省参数 schema
/// 是一个空 object schema，传给供应商前仍会经过 [`super::encode`] 的保守 schema
/// 子集检查。
fn parse_tool(v: &Value, i: usize) -> Result<Tool, Error> {
    let path = format!("tools[{i}]");
    let m = object(v, &path)?;
    keys(m, &["type", "function"], &path)?;
    if string(m.get("type"), &path)? != "function" {
        return Err(Error::input("unsupported_tool_type", path));
    }
    let f = object(m.get("function").unwrap_or(&Value::Null), &path)?;
    keys(f, &["name", "description", "parameters", "strict"], &path)?;
    let tool_name = string(f.get("name"), &path)?;
    if !name(tool_name) {
        return Err(Error::input("invalid_tool_name", path));
    }
    let parameters = f
        .get("parameters")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({"type":"object","properties":{}}));
    object(&parameters, &path)?;
    let description = f
        .get("description")
        .map(|v| string(Some(v), &path).map(String::from))
        .transpose()?;
    let strict = optional_bool(f, "strict", &path)?;
    Ok(Tool {
        name: tool_name.into(),
        description,
        parameters,
        strict,
    })
}

/// 解析一条消息并执行角色相关的内容形状检查。
///
/// - system/developer/user/assistant 的 `content` 可为字符串；这些角色的数组形式
///   都可包含 text；image_url content 只允许出现在 user；
/// - tool 的 `content` 必须是字符串，并通过 `tool_call_id` 指向真实调用；
/// - assistant 只有在存在 `tool_calls` 键时才能省略 content 或使用 null；随后该键
///   仍必须解析成非空且有效的调用数组，调用为空仍会失败；
/// - `tool_calls` 和 `tool_call_id` 不得出现在与角色不匹配的消息上。
///
/// `path` 是从顶层开始的稳定结构路径。错误不会包含实际文本。解析完成后还要由
/// [`validate_tool_history`] 检查跨消息的“调用全部得到结果”关系。
fn parse_message(v: &Value, i: usize) -> Result<Message, Error> {
    let path = format!("messages[{i}]");
    let m = object(v, &path)?;
    keys(m, &["role", "content", "tool_calls", "tool_call_id"], &path)?;
    let role = match string(m.get("role"), &format!("{path}.role"))? {
        "system" => Role::System,
        "developer" => Role::Developer,
        "user" => Role::User,
        "assistant" => Role::Assistant,
        "tool" => Role::Tool,
        _ => return Err(Error::input("unsupported_role", format!("{path}.role"))),
    };
    let mut parts = Vec::new();
    match m.get("content") {
        Some(Value::String(s)) => parts.push(Part::Text(s.clone())),
        Some(Value::Array(items)) if role != Role::Tool => {
            for (j, item) in items.iter().enumerate() {
                let path = format!("{path}.content[{j}]");
                let m = object(item, &path)?;
                match string(m.get("type"), &path)? {
                    "text" => {
                        keys(m, &["type", "text"], &path)?;
                        parts.push(Part::Text(string(m.get("text"), &path)?.into()));
                    }
                    "image_url" if role == Role::User => {
                        keys(m, &["type", "image_url"], &path)?;
                        let image = object(m.get("image_url").unwrap_or(&Value::Null), &path)?;
                        keys(image, &["url", "detail"], &path)?;
                        let detail = image
                            .get("detail")
                            .map(|v| string(Some(v), &path))
                            .transpose()?
                            .unwrap_or("auto");
                        if !matches!(detail, "auto" | "low" | "high") {
                            return Err(Error::input("unsupported_image_detail", path));
                        }
                        parts.push(parse_image(
                            string(image.get("url"), &path)?,
                            detail,
                            &path,
                        )?);
                    }
                    _ => return Err(Error::input("unsupported_content", path)),
                }
            }
        }
        None | Some(Value::Null) if role == Role::Assistant && m.contains_key("tool_calls") => (),
        _ => return Err(Error::input("invalid_content", format!("{path}.content"))),
    }
    let calls = if let Some(calls) = m.get("tool_calls") {
        if role != Role::Assistant {
            return Err(Error::input(
                "invalid_tool_history",
                format!("{path}.tool_calls"),
            ));
        }
        let calls = calls
            .as_array()
            .filter(|v| !v.is_empty() && v.len() <= TOOL_LIMIT)
            .ok_or_else(|| Error::input("invalid_tool_history", format!("{path}.tool_calls")))?;
        calls
            .iter()
            .enumerate()
            .map(|(j, v)| {
                let path = format!("{path}.tool_calls[{j}]");
                let call = object(v, &path)?;
                keys(call, &["id", "type", "function"], &path)?;
                if string(call.get("type"), &path)? != "function" {
                    return Err(Error::input("unsupported_tool_type", path));
                }
                let id = string(call.get("id"), &path)?;
                if id.is_empty()
                    || id.len() > 256
                    || id.chars().any(char::is_whitespace)
                    || id.chars().any(char::is_control)
                {
                    return Err(Error::input("invalid_tool_id", path));
                }
                let function = object(call.get("function").unwrap_or(&Value::Null), &path)?;
                keys(function, &["name", "arguments"], &path)?;
                let tool_name = string(function.get("name"), &path)?;
                if !name(tool_name) {
                    return Err(Error::input("invalid_tool_name", path));
                }
                let text = string(
                    function.get("arguments"),
                    &format!("{path}.function.arguments"),
                )?;
                let arguments =
                    strict_json(text.as_bytes(), TOOL_ARGUMENT_LIMIT).map_err(|_| {
                        Error::input(
                            "invalid_tool_arguments",
                            format!("{path}.function.arguments"),
                        )
                    })?;
                object(&arguments, &format!("{path}.function.arguments"))?;
                // 解析值与原始文本分别保留；后者不是日志副本，而是协议转换数据。
                Ok(ToolCall {
                    id: id.into(),
                    name: tool_name.into(),
                    arguments,
                    arguments_text: text.into(),
                })
            })
            .collect::<Result<Vec<_>, Error>>()?
    } else {
        vec![]
    };
    let tool_call_id = if role == Role::Tool {
        Some(string(m.get("tool_call_id"), &format!("{path}.tool_call_id"))?.into())
    } else if m.contains_key("tool_call_id") {
        return Err(Error::input(
            "invalid_tool_history",
            format!("{path}.tool_call_id"),
        ));
    } else {
        None
    };
    Ok(Message {
        role,
        parts,
        calls,
        tool_call_id,
    })
}

/// 解析图片 URL 或 data URL，但不抓取、解码或检查图片内容。
///
/// data URL 只接受 PNG/JPEG MIME 和具有合法 padding/末位约束的 base64 文本；函数
/// 并不把 base64 解码成像素，也不验证 PNG/JPEG magic。远程 URL 只接受无用户名、
/// 无密码、无 fragment、无控制字符的 HTTPS 地址。整体请求的 8 MiB 上限仍是总大小
/// 边界；这里没有额外的单图字节上限。
fn parse_image(url: &str, detail: &str, path: &str) -> Result<Part, Error> {
    if let Some(data) = url.strip_prefix("data:") {
        let (mime, data) = data
            .split_once(";base64,")
            .ok_or_else(|| Error::input("invalid_image", path))?;
        if !matches!(mime, "image/png" | "image/jpeg") || !valid_base64(data) {
            return Err(Error::input("invalid_image", path));
        }
        return Ok(Part::ImageData {
            mime: mime.into(),
            data: data.into(),
            detail: detail.into(),
        });
    }
    let parsed = reqwest::Url::parse(url).map_err(|_| Error::input("invalid_image", path))?;
    if parsed.scheme() != "https"
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.fragment().is_some()
        || url.chars().any(char::is_control)
    {
        return Err(Error::input("unsupported_image_url", path));
    }
    Ok(Part::ImageUrl {
        url: url.into(),
        detail: detail.into(),
    })
}

/// 检查 base64 的字母表、长度、padding 数量及末尾未用 bit。
///
/// 这是结构校验，不证明解码后一定是有效图片。空串、长度非 4 的倍数、超过两个
/// padding、非零 padding 尾位都会返回 false。
fn valid_base64(s: &str) -> bool {
    fn digit(b: u8) -> Option<u8> {
        match b {
            b'A'..=b'Z' => Some(b - b'A'),
            b'a'..=b'z' => Some(b - b'a' + 26),
            b'0'..=b'9' => Some(b - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let b = s.as_bytes();
    if b.is_empty() || !b.len().is_multiple_of(4) {
        return false;
    }
    let padding = b.iter().rev().take_while(|b| **b == b'=').count();
    if padding > 2 || !b[..b.len() - padding].iter().all(|b| digit(*b).is_some()) {
        return false;
    }
    match padding {
        1 => digit(b[b.len() - 2]).is_some_and(|v| v & 3 == 0),
        2 => digit(b[b.len() - 3]).is_some_and(|v| v & 15 == 0),
        _ => true,
    }
}

/// 校验 assistant tool calls 与后续 tool 结果的跨消息关系。
///
/// 每个 assistant 调用都会按 ID 加入 pending；紧随其后的 tool 消息可按任意顺序
/// 消除 pending，但必须全部完成，遇到下一条非 tool 消息时仍有 pending 就报
/// `incomplete_tool_results`。tool 结果不能指向未知或已经消除的 ID；相同调用 ID
/// 在整段历史中只能声明一次。匹配完全依赖 ID，因此同名 function 的并行调用仍彼此
/// 独立。
///
/// 校验器不要求结果文本是 JSON，因为工具执行结果可以只是普通文本；它也不执行工具
/// 本身。成功返回表示历史可被各 native 编码器安全分组，不表示上游接受这些调用。
fn validate_tool_history(messages: &[Message]) -> Result<(), Error> {
    let mut pending = BTreeMap::new();
    let mut seen = BTreeSet::new();
    for (i, message) in messages.iter().enumerate() {
        if message.role == Role::Tool {
            let id = message.tool_call_id.as_ref().expect("parsed tool ID");
            if pending.remove(id).is_none() {
                return Err(Error::input(
                    "invalid_tool_history",
                    format!("messages[{i}].tool_call_id"),
                ));
            }
        } else {
            if !pending.is_empty() {
                return Err(Error::input(
                    "incomplete_tool_results",
                    format!("messages[{i}]"),
                ));
            }
            for call in &message.calls {
                if !seen.insert(call.id.clone()) {
                    return Err(Error::input(
                        "duplicate_tool_id",
                        format!("messages[{i}].tool_calls"),
                    ));
                }
                pending.insert(call.id.clone(), ());
            }
        }
    }
    if !pending.is_empty() {
        return Err(Error::input("incomplete_tool_results", "messages"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn parse(extra: Value) -> Result<ChatRequest, Error> {
        let mut v = json!({"model":"local-alias","messages":[{"role":"user","content":"hello"}]});
        for (k, value) in extra.as_object().unwrap() {
            v[k] = value.clone();
        }
        ChatRequest::parse(&serde_json::to_vec(&v).unwrap())
    }
    #[test]
    fn duplicate_nested_keys_depth_and_trailing_values_are_rejected() {
        for bytes in [
            br#"{"model":"a","model":"b"}"#.as_slice(),
            br#"{"tools":[{"x":1,"x":2}]}"#,
            b"{} {}",
        ] {
            assert!(strict_json(bytes, INPUT_LIMIT).is_err());
        }
        let deep = format!("{}0{}", "[".repeat(65), "]".repeat(65));
        assert!(strict_json(deep.as_bytes(), INPUT_LIMIT).is_err());
        assert!(strict_json(b"{}", 1).is_err());
    }
    #[test]
    fn booleans_limits_and_unknown_parameters_never_coerce() {
        assert!(
            parse(json!({"stream":true,"stream_options":{"include_usage":true}}))
                .unwrap()
                .include_usage
        );
        for extra in [
            json!({"stream":"true"}),
            json!({"stream":null}),
            json!({"n":2}),
            json!({"n":1.0}),
            json!({"max_tokens":0}),
            json!({"max_tokens":"1"}),
            json!({"max_tokens":1,"max_completion_tokens":2}),
            json!({"stream_options":{"include_usage":true}}),
            json!({"user":"secret"}),
            json!({"seed":0}),
        ] {
            assert!(parse(extra.clone()).is_err(), "{extra}");
        }
        assert_eq!(
            parse(json!({"max_tokens":12,"max_completion_tokens":12}))
                .unwrap()
                .max_output_tokens,
            Some(12)
        );
        let error = parse(json!({"credential-value-as-key":"credential-value"})).unwrap_err();
        assert!(!error.envelope().to_string().contains("credential"));
    }
    #[test]
    fn same_name_parallel_calls_reordered_results_and_continuous_rounds_are_valid() {
        let call = |id: &str, args: &str| json!({"id":id,"type":"function","function":{"name":"find","arguments":args}});
        let mut messages = json!([
            {"role":"user","content":"hello"},
            {"role":"assistant","content":"checking","tool_calls":[call("call-a",r#"{"q":1}"#),call("call-b",r#"{"q":2}"#)]},
            {"role":"tool","tool_call_id":"call-b","content":"not JSON"},
            {"role":"tool","tool_call_id":"call-a","content":"{\"a\":1}"},
            {"role":"assistant","content":null,"tool_calls":[call("call-c","{}")]},
            {"role":"tool","tool_call_id":"call-c","content":"done"}
        ]);
        let parsed = parse(json!({"messages":messages.clone()})).unwrap();
        assert_eq!(parsed.messages[1].calls.len(), 2);
        assert_eq!(parsed.messages[2].tool_call_id.as_deref(), Some("call-b"));
        assert_eq!(parsed.messages[1].text(), "checking");
        messages.as_array_mut().unwrap().remove(3);
        assert_eq!(
            parse(json!({"messages":messages})).unwrap_err().code,
            "incomplete_tool_results"
        );
    }
    #[test]
    fn malformed_arguments_and_tool_result_identity_fail_closed() {
        for args in ["", "[]", "null", r#"{"q":1,"q":2}"#] {
            let messages = json!([{ "role":"assistant","content":null,"tool_calls":[
                {"id":"a","type":"function","function":{"name":"f","arguments":args}}
            ]},{"role":"tool","tool_call_id":"a","content":"x"}]);
            assert!(parse(json!({"messages":messages})).is_err());
        }
        assert!(
            parse(json!({"messages":[{"role":"tool","tool_call_id":"no-call","content":"x"}]}))
                .is_err()
        );
        assert!(parse(json!({"tool_choice":"required"})).is_err());
        assert!(parse(json!({"tools":[{"type":"function","function":{"name":"f"}}],"tool_choice":{"type":"function","function":{"name":"unknown"}}})).is_err());
    }
    #[test]
    fn image_encoding_is_validated_without_fetching_or_decoding() {
        for good in [
            "data:image/png;base64,AA==",
            "data:image/jpeg;base64,AAA=",
            "https://images.example.test/a.png",
        ] {
            assert!(parse_image(good, "auto", "messages[0].content[0]").is_ok());
        }
        for bad in [
            "data:image/gif;base64,AA==",
            "data:image/png;base64,AB==",
            "data:image/png;base64,AA=",
            "data:image/png;base64,A===",
            "http://localhost/a",
            "https://secret@x/a",
        ] {
            assert!(parse_image(bad, "auto", "messages[0].content[0]").is_err());
        }
    }
}
