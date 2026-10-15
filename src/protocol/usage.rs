//! Vendor usage -> the existing statistics collector's Chat usage contract.
//! Missing parents remain absent; malformed counts invalidate usage, not relay.
use super::config::UpstreamProtocol;
use crate::stats::dto::{MAX_TOKEN_VALUE, MAX_TOTAL_TOKENS};
use serde_json::{Map, Value, json};

#[derive(Clone, Debug, Default)]
pub struct Usage {
    pub chat: Value,
    pub complete: bool,
    pub disjoint_uncached: Option<i64>,
    pub invalid: bool,
}
impl Usage {
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
fn detail(v: &Value, parent: &str, key: &str) -> Result<Option<i64>, ()> {
    match v.get(parent) {
        None | Some(Value::Null) => Ok(None),
        Some(v) if v.is_object() => count(v, key, MAX_TOKEN_VALUE),
        _ => Err(()),
    }
}
fn add(a: Option<i64>, b: Option<i64>, max: i64) -> Result<Option<i64>, ()> {
    a.zip(b)
        .map(|(a, b)| a.checked_add(b).filter(|n| *n <= max).ok_or(()))
        .transpose()
}
fn put(m: &mut Map<String, Value>, key: &str, v: Option<i64>) {
    if let Some(v) = v {
        m.insert(key.into(), json!(v));
    }
}

/// Keep only normalized counter fields from cumulative streaming updates.
/// Arbitrary vendor metadata cannot grow a retained map indefinitely. Invalid
/// or decreasing counters stay invalid without aborting otherwise valid text.
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

/// Interim streaming counters are lower bounds, not a final output bill.
/// Keep proven input and quality errors while withholding unfinished output.
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
                (Some(_), Some(_)) => add(candidates, reasoning, MAX_TOKEN_VALUE)?,
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
