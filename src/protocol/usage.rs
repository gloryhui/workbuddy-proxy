//! Vendor usage -> the existing statistics collector's Chat usage contract.
//! Missing parents remain absent; malformed counts invalidate usage, not relay.
//!
//! 本模块位于协议适配层：输入是 Responses、Anthropic Messages 或 Gemini
//! GenerateContent 返回的 usage 片段，输出是统计 collector 已认识的一组
//! Chat 风格 token 字段。`Usage` 的所有权属于非流式 `Output`，或流式
//! `streaming::Common`；它不会直接写 SQLite，也不创建线程。后续由
//! `proxy::native::observe` 把 `Usage::outcome()` 交给 `RequestGuard`，最终
//! 由统计 writer 落库。
//!
//! 这里处理的是已经解析出的供应商 usage，而不是接口 HTTP、SSE 或输出正文。
//! 统计侧的 native 观察与响应转换是两个同时存在但职责不同的投影：前者只记录
//! 上游原始传输字节数、语义首内容时间、响应模型和规范化 `UsageOutcome`；
//! 后者才构建客户端可见 Chat text/tool calls。原始消息、工具参数、图片及
//! 不透明签名不会作为统计明细进入队列。
//!
//! token 单位均为“个 token”，不是字节或字符。数值缺失时使用
//! `Option::None`，表示未知；已知的 0 则是合法零值。错误 usage 不把正文响应
//! 判为失败，但会形成 Invalid 统计结果，绝不能悄悄按 0 计费。
use super::config::UpstreamProtocol;
use crate::stats::dto::{MAX_TOKEN_VALUE, MAX_TOTAL_TOKENS};
use serde_json::{Map, Value, json};

/// 一次上游响应经过质量校验后的 usage 快照。
///
/// 流式 `Common::reconcile` 克隆最终 Output 的 usage，分别保留当前观察快照与
/// 最终结果；观察接口本身只借用 `&Usage`。`Clone` 的具体复制边界由字段类型
/// 决定，不能把所有 Rust 类型的 clone 都套成同一种 Java 对象复制操作。
#[derive(Clone, Debug, Default)]
pub struct Usage {
    /// Chat 兼容 usage JSON，例如 `prompt_tokens`、`completion_tokens` 和
    /// `total_tokens`。字段存在表示已知，缺字段表示未知；值 0 是已知零值。
    /// `Value::Null` 专门表示没有可用 usage，不能被统计成完整零用量。
    pub chat: Value,
    /// input、output、total 三个父字段是否全部已知且通过一致性校验。该值
    /// 描述数值完整度，不描述正文、SSE 或 transport 是否成功。
    pub complete: bool,
    /// 仅 Anthropic 能证明“未缓存输入”时存在：input_tokens 本身不含缓存读写，
    /// 该值为未缓存的 token 数。其他协议为 `None`，不能自行套用此分解。
    pub disjoint_uncached: Option<i64>,
    /// 上游存在 usage，但计数字段或父子关系非法、越界或彼此矛盾。
    /// Invalid usage 不会中断本来有效的模型正文，只独立影响统计可信度。
    pub invalid: bool,
}
impl Usage {
    /// 把协议层快照投影成统计 collector 的最终口径。
    ///
    /// 调用方是 `proxy::native::observe`。Invalid 优先于内容是否存在；随后
    /// `Null` 映射为 Missing。其余情况交给 `UsageAccumulator`：父字段完整时
    /// 为 Complete，部分可得时为 Partial。Anthropic 已证明输入分解时再补充
    /// Disjoint breakdown 和 uncached 数字。本函数只读，无 I/O。
    pub fn outcome(&self) -> crate::stats::collector::UsageOutcome {
        use crate::stats::{
            collector::{UsageAccumulator, UsageOutcome},
            dto::{InputBreakdown, UsageReason, UsageStatus},
        };
        if self.invalid {
            return UsageOutcome {
                status: UsageStatus::Invalid,
                reason: Some(UsageReason::InvalidNumbers),
                numbers: Default::default(),
                breakdown: InputBreakdown::Unknown,
                uncached: None,
            };
        }
        if self.chat.is_null() {
            return UsageOutcome::missing(UsageReason::NoUsage);
        }
        let mut accumulator = UsageAccumulator::default();
        accumulator.observe(&self.chat);
        let mut outcome = accumulator.finalize(UsageReason::NoUsage);
        if let Some(uncached) = self.disjoint_uncached {
            outcome.breakdown = InputBreakdown::Disjoint;
            outcome.uncached = Some(uncached);
        }
        outcome
    }
}
/// 读取一个可选的顶层非负整数，并强制协议规定的字段上限。
///
/// 缺失键返回 `Ok(None)`；显式 null、浮点数、字符串、负数或超限都视为不可
/// 信，返回 `Err(())`。`max` 的单位仍是 token 数。
fn count(v: &Value, key: &str, max: i64) -> Result<Option<i64>, ()> {
    match v.get(key) {
        None => Ok(None),
        Some(v) => v
            .as_i64()
            .filter(|n| *n >= 0 && *n <= max)
            .map(Some)
            .ok_or(()),
    }
}
/// 读取 `parent.key` 形式的可选明细计数。
///
/// 父对象缺省或为 null 表示该明细未知；父对象存在但类型错误则使 usage 无效。
/// 这样不会把损坏的嵌套结构误解成“供应商没返回缓存数据”。
fn detail(v: &Value, parent: &str, key: &str) -> Result<Option<i64>, ()> {
    match v.get(parent) {
        None | Some(Value::Null) => Ok(None),
        Some(v) if v.is_object() => count(v, key, MAX_TOKEN_VALUE),
        _ => Err(()),
    }
}
/// 安全相加两个可选计数。
///
/// 任一项未知就返回 None，避免用缺失值补 0；溢出或超过上限返回错误。
fn add(a: Option<i64>, b: Option<i64>, max: i64) -> Result<Option<i64>, ()> {
    a.zip(b)
        .map(|(a, b)| a.checked_add(b).filter(|n| *n <= max).ok_or(()))
        .transpose()
}
/// 仅把已知计数写入 Chat usage 对象；None 留给“字段不存在”。
fn put(m: &mut Map<String, Value>, key: &str, v: Option<i64>) {
    if let Some(v) = v {
        m.insert(key.into(), json!(v));
    }
}

/// Keep only normalized counter fields from cumulative streaming updates.
/// Arbitrary vendor metadata cannot grow a retained map indefinitely. Invalid
/// or decreasing counters stay invalid without aborting otherwise valid text.
///
/// 调用方是流式 Anthropic/Gemini 适配器。协议把多帧 usage 作为累计快照，
/// 因此这里用后值替换白名单字段，不能相加。只保留固定字段可以阻止任意供应商
/// 元数据无界增长；缺失字段沿用旧快照，当前值已为 null 时保持失效。
///
/// 新值必须是非负整数、不超过对应上限，并且不小于同一字段的旧值。下降或非法
/// 更新把该字段设为 null，让后续 normalize 报告 Invalid；这里故意不返回错误，
/// 因为 usage 元数据损坏不应丢弃已经成功解析的模型 text/tool 输出。
pub fn merge_cumulative(current: &mut Value, incoming: &Value, protocol: UpstreamProtocol) {
    let fields: &[&str] = match protocol {
        UpstreamProtocol::AnthropicMessages => &[
            "input_tokens",
            "output_tokens",
            "cache_read_input_tokens",
            "cache_creation_input_tokens",
        ],
        UpstreamProtocol::GeminiGenerateContent => &[
            "promptTokenCount",
            "candidatesTokenCount",
            "thoughtsTokenCount",
            "totalTokenCount",
            "cachedContentTokenCount",
            "toolUsePromptTokenCount",
        ],
        _ => return,
    };
    if !current.is_object() {
        *current = json!({});
    }
    for key in fields {
        let value = if !incoming.is_object() {
            Some(&Value::Null)
        } else {
            incoming.get(*key)
        };
        let Some(value) = value else {
            continue;
        };
        if current.get(*key).is_some_and(Value::is_null) {
            continue;
        }
        let max = if *key == "totalTokenCount" {
            MAX_TOTAL_TOKENS
        } else {
            MAX_TOKEN_VALUE
        };
        let valid = value.as_i64().filter(|n| {
            *n >= 0
                && *n <= max
                && current
                    .get(*key)
                    .and_then(Value::as_i64)
                    .is_none_or(|old| *n >= old)
        });
        current[*key] = valid.map_or(Value::Null, |n| json!(n));
    }
}

/// 把一个供应商 usage 对象严格规范化为 Chat usage。
///
/// `native=Null` 表示供应商没有给出 usage，返回默认 Missing 语义。对象解析
/// 失败则返回 `invalid=true` 且 `chat=Null`；不把供应商原文回显给客户端。
/// 本函数用于流式最终快照与非流式 `Output`。
pub fn normalize(protocol: UpstreamProtocol, native: &Value) -> Usage {
    if native.is_null() {
        return Usage::default();
    }
    match normalize_checked(protocol, native) {
        Ok(usage) => usage,
        // Keep quality separate from the public JSON counters. Null denotes
        // unknown in the existing collector and cannot serve as an invalid
        // sentinel. Bad vendor values are never echoed to the client.
        Err(()) => Usage {
            chat: Value::Null,
            complete: false,
            disjoint_uncached: None,
            invalid: true,
        },
    }
}

/// 规范流式过程中尚不能视为最终账单的累计计数。
///
/// Interim counters are lower bounds, not a final output bill.
/// Keep proven input and quality errors while withholding unfinished output.
///
/// Anthropic 会移除尚未被终态确认的 `output_tokens`；Gemini 会移除候选数、
/// thought 数和 total，只保留 prompt/cache 等已证明输入。Responses 只在完整
/// terminal snapshot 上提供用法，因此这里不做额外删减。这个函数不回滚已发射
/// 的正文，也不把不完整 usage 当 0。
pub fn normalize_partial(protocol: UpstreamProtocol, native: &Value) -> Usage {
    let full = normalize(protocol, native);
    if full.invalid {
        return full;
    }
    let mut partial = native.clone();
    if let Some(object) = partial.as_object_mut() {
        match protocol {
            UpstreamProtocol::AnthropicMessages => {
                object.remove("output_tokens");
            }
            UpstreamProtocol::GeminiGenerateContent => {
                for key in [
                    "candidatesTokenCount",
                    "thoughtsTokenCount",
                    "totalTokenCount",
                ] {
                    object.remove(key);
                }
            }
            _ => (),
        }
    }
    normalize(protocol, &partial)
}
fn normalize_checked(protocol: UpstreamProtocol, v: &Value) -> Result<Usage, ()> {
    if !v.is_object() {
        return Err(());
    }
    // 先按协议解释 native 字段，再统一检查 input/output/total、缓存子集和
    // reasoning 子集的约束。每个 `None` 都表示该父字段未知，不是零。
    let (input, output, total, read, write, reasoning) = match protocol {
        UpstreamProtocol::OpenaiResponses => {
            let input = count(v, "input_tokens", MAX_TOKEN_VALUE)?;
            let output = count(v, "output_tokens", MAX_TOKEN_VALUE)?;
            (
                input,
                output,
                count(v, "total_tokens", MAX_TOTAL_TOKENS)?.or(add(
                    input,
                    output,
                    MAX_TOTAL_TOKENS,
                )?),
                detail(v, "input_tokens_details", "cached_tokens")?,
                None,
                detail(v, "output_tokens_details", "reasoning_tokens")?,
            )
        }
        UpstreamProtocol::AnthropicMessages => {
            let uncached = count(v, "input_tokens", MAX_TOKEN_VALUE)?;
            let read = count(v, "cache_read_input_tokens", MAX_TOKEN_VALUE)?;
            let write = count(v, "cache_creation_input_tokens", MAX_TOKEN_VALUE)?;
            // input_tokens is uncached input on Messages, not total input.
            // Missing cache components cannot be silently priced as zero.
            // 例如 uncached=10、cache read=40、cache create=5，则统一 prompt
            // 是 55；只要 read/write 有一项未知，就不能证明这个 55。
            let input = add(
                add(uncached, read, MAX_TOKEN_VALUE)?,
                write,
                MAX_TOKEN_VALUE,
            )?;
            let output = count(v, "output_tokens", MAX_TOKEN_VALUE)?;
            (
                input,
                output,
                add(input, output, MAX_TOTAL_TOKENS)?,
                read,
                write,
                None,
            )
        }
        UpstreamProtocol::GeminiGenerateContent => {
            let prompt = count(v, "promptTokenCount", MAX_TOKEN_VALUE)?;
            let tool_prompt = count(v, "toolUsePromptTokenCount", MAX_TOKEN_VALUE)?;
            // No hosted tools are enabled by this adapter. Nonzero hosted
            // tool prompt counts have no reviewed local billing allocation.
            if tool_prompt.is_some_and(|n| n > 0) {
                return Err(());
            }
            let candidates = count(v, "candidatesTokenCount", MAX_TOKEN_VALUE)?;
            let reasoning = count(v, "thoughtsTokenCount", MAX_TOKEN_VALUE)?;
            let total = count(v, "totalTokenCount", MAX_TOTAL_TOKENS)?;
            let output = match (candidates, reasoning) {
                // 两项都存在时，Chat completion 口径包含可见候选与 thought，
                // 并且后续会检查二者都不大于合并后的 output。
                (Some(_), Some(_)) => add(candidates, reasoning, MAX_TOKEN_VALUE)?,
                // 若只是缺一项，只有 total 与 prompt 都已知时才可推导剩余
                // output；推导失败就保持未知，不能拿 0 补齐。
                _ => total
                    .zip(prompt)
                    .and_then(|(t, p)| t.checked_sub(p))
                    .filter(|n| *n >= 0 && *n <= MAX_TOKEN_VALUE),
            };
            if candidates.zip(output).is_some_and(|(c, o)| c > o)
                || reasoning.zip(output).is_some_and(|(r, o)| r > o)
            {
                return Err(());
            }
            (
                prompt,
                output,
                total.or(add(prompt, output, MAX_TOTAL_TOKENS)?),
                count(v, "cachedContentTokenCount", MAX_TOKEN_VALUE)?,
                None,
                reasoning,
            )
        }
        UpstreamProtocol::OpenaiChat => return Err(()),
    };
    // 父三元组必须自洽；缓存读写必须属于输入；reasoning 必须是 output 的
    // 子集。Anthropic 的 input 已转为“总输入”，read+write 也必须不超过它。
    if input
        .zip(output)
        .zip(total)
        .is_some_and(|((i, o), t)| i + o != t)
        || read.zip(input).is_some_and(|(r, i)| r > i)
        || write.zip(input).is_some_and(|(w, i)| w > i)
        || read
            .zip(write)
            .zip(input)
            .is_some_and(|((r, w), i)| r + w > i)
        || reasoning.zip(output).is_some_and(|(r, o)| r > o)
    {
        return Err(());
    }
    let mut m = Map::new();
    put(&mut m, "prompt_tokens", input);
    put(&mut m, "completion_tokens", output);
    put(&mut m, "total_tokens", total);
    let mut details = Map::new();
    put(&mut details, "cached_tokens", read);
    put(&mut details, "cache_write_tokens", write);
    if !details.is_empty() {
        m.insert("prompt_tokens_details".into(), Value::Object(details));
    }
    if let Some(n) = reasoning {
        m.insert(
            "completion_tokens_details".into(),
            json!({"reasoning_tokens":n}),
        );
    }
    Ok(Usage {
        chat: Value::Object(m),
        complete: input.is_some() && output.is_some() && total.is_some(),
        disjoint_uncached: if protocol == UpstreamProtocol::AnthropicMessages {
            input.zip(read).zip(write).map(|((i, r), w)| i - r - w)
        } else {
            None
        },
        invalid: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn anthropic_cache_components_are_disjoint_and_missing_is_unknown() {
        let u = normalize(
            UpstreamProtocol::AnthropicMessages,
            &json!({"input_tokens":10,"cache_read_input_tokens":40,"cache_creation_input_tokens":5,"output_tokens":7}),
        );
        assert!(u.complete);
        assert_eq!(u.chat["prompt_tokens"], 55);
        assert_eq!(u.chat["total_tokens"], 62);
        let u = normalize(
            UpstreamProtocol::AnthropicMessages,
            &json!({"input_tokens":10,"output_tokens":7}),
        );
        assert!(!u.complete);
        assert!(u.chat.get("prompt_tokens").is_none());
        assert_eq!(u.chat["completion_tokens"], 7);
    }
    #[test]
    fn gemini_thoughts_are_counted_once_and_totals_must_agree() {
        let v = json!({"promptTokenCount":10,"candidatesTokenCount":5,"thoughtsTokenCount":3,"totalTokenCount":18});
        let u = normalize(UpstreamProtocol::GeminiGenerateContent, &v);
        assert!(u.complete);
        assert_eq!(u.chat["completion_tokens"], 8);
        let u = normalize(
            UpstreamProtocol::GeminiGenerateContent,
            &json!({"promptTokenCount":10,"candidatesTokenCount":8,"thoughtsTokenCount":3,"totalTokenCount":18}),
        );
        assert!(!u.complete);
        assert!(u.chat["prompt_tokens"].is_null());
        let u = normalize(
            UpstreamProtocol::GeminiGenerateContent,
            &json!({"promptTokenCount":10,"candidatesTokenCount":5,"totalTokenCount":18}),
        );
        assert!(u.complete);
        assert_eq!(u.chat["completion_tokens"], 8);
        assert!(u.chat.get("completion_tokens_details").is_none());
    }
    #[test]
    fn negative_fractional_huge_and_conflicting_usage_never_becomes_a_zero_bill() {
        for v in [
            json!({"input_tokens":-1,"output_tokens":1}),
            json!({"input_tokens":1.5,"output_tokens":1}),
            json!({"input_tokens":1,"output_tokens":2,"total_tokens":4}),
            json!({"input_tokens":18446744073709551615u64}),
        ] {
            let u = normalize(UpstreamProtocol::OpenaiResponses, &v);
            assert!(!u.complete);
            assert!(u.chat["prompt_tokens"].is_null());
        }
        assert!(!normalize(UpstreamProtocol::OpenaiResponses, &Value::Null).complete);
    }

    #[test]
    fn cumulative_metadata_is_bounded_and_decreases_stay_invalid() {
        let protocol = UpstreamProtocol::AnthropicMessages;
        let mut snapshot = json!({});
        merge_cumulative(
            &mut snapshot,
            &json!({"input_tokens":10,"cache_read_input_tokens":0,"cache_creation_input_tokens":0,"output_tokens":5,"unbounded_metadata":"private"}),
            protocol,
        );
        assert_eq!(snapshot.as_object().unwrap().len(), 4);
        assert!(normalize(protocol, &snapshot).complete);
        merge_cumulative(&mut snapshot, &json!({"output_tokens":4}), protocol);
        assert!(!normalize(protocol, &snapshot).complete);
        merge_cumulative(&mut snapshot, &json!({"output_tokens":7}), protocol);
        assert!(snapshot["output_tokens"].is_null());
        let mut snapshot = json!({});
        merge_cumulative(
            &mut snapshot,
            &json!("malformed"),
            UpstreamProtocol::GeminiGenerateContent,
        );
        assert!(!normalize(UpstreamProtocol::GeminiGenerateContent, &snapshot).complete);
    }
}
