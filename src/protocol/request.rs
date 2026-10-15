//! Strict, bounded Chat input validation for native conversion only.
use super::Error;
use serde::de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};

pub const INPUT_LIMIT: usize = 8 * 1024 * 1024;
pub const TOOL_ARGUMENT_LIMIT: usize = 256 * 1024;
pub const TOOL_LIMIT: usize = 64;

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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    System,
    Developer,
    User,
    Assistant,
    Tool,
}
#[derive(Clone, Debug)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
    pub arguments_text: String,
}
#[derive(Clone, Debug)]
pub struct Message {
    pub role: Role,
    pub parts: Vec<Part>,
    pub calls: Vec<ToolCall>,
    pub tool_call_id: Option<String>,
}
impl Message {
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
#[derive(Clone, Debug)]
pub struct Tool {
    pub name: String,
    pub description: Option<String>,
    pub parameters: Value,
    pub strict: Option<bool>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToolChoice {
    Auto,
    None,
    Required,
    Named(String),
}
#[derive(Clone, Debug)]
pub struct ChatRequest {
    pub stream: bool,
    pub include_usage: bool,
    pub messages: Vec<Message>,
    pub tools: Vec<Tool>,
    pub tool_choice: ToolChoice,
    pub parallel_tool_calls: Option<bool>,
    pub max_output_tokens: Option<u32>,
    pub temperature: Option<f64>,
    pub top_p: Option<f64>,
    pub stop: Vec<String>,
    pub reasoning_effort: Option<String>,
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
    pub fn parse(bytes: &[u8]) -> Result<Self, Error> {
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

/// Each parallel result group may be reordered but must be complete before
/// another turn. Matching is by ID, so same-name calls remain independent.
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
