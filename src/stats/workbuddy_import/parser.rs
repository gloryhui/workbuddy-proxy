//! Line-level shell parsing for the WorkBuddy JSONL importer.
//!
//! One complete line in, one typed shell out. The parser only ever looks at the
//! whitelisted keys of DATA-API section 1: `type`, `role`, `sessionId`,
//! `timestamp`, `providerData` and `_fromSubAgent`. Everything else — chat content, tool
//! arguments, images, ids — is skipped by serde and never materialised, so a
//! private transcript can never travel further than this function.
//!
//! Numbers are captured as [`NumberLiteral`] (a plain integer, or "present but
//! not a plain integer") instead of being coerced, and the credit literal keeps
//! its exact decimal text. No `serde_json::Value` is used anywhere.

use crate::stats::workbuddy_import::normalize;
use crate::stats::workbuddy_import::types::{
    Candidate, ENTRY_KEY_DOMAIN, ID_LIMIT_CHARS, MalformedReason, ParseDisposition,
};
use serde::Deserialize;
use serde::de::{self, Visitor};
use serde_json::value::RawValue;
use sha2::{Digest, Sha256};

/// A JSON number as it appeared on the wire.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NumberLiteral {
    /// A plain integer that fits in `i64`.
    Integer(i64),
    /// Present, but not a plain integer: float, string, bool, or out of range.
    Other,
}

impl<'de> Deserialize<'de> for NumberLiteral {
    fn deserialize<D: de::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct NumberVisitor;

        impl Visitor<'_> for NumberVisitor {
            type Value = NumberLiteral;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a JSON number")
            }

            fn visit_i64<E: de::Error>(self, value: i64) -> Result<Self::Value, E> {
                Ok(NumberLiteral::Integer(value))
            }

            fn visit_u64<E: de::Error>(self, value: u64) -> Result<Self::Value, E> {
                Ok(i64::try_from(value)
                    .map(NumberLiteral::Integer)
                    .unwrap_or(NumberLiteral::Other))
            }

            fn visit_f64<E: de::Error>(self, _: f64) -> Result<Self::Value, E> {
                Ok(NumberLiteral::Other)
            }

            fn visit_str<E: de::Error>(self, _: &str) -> Result<Self::Value, E> {
                Ok(NumberLiteral::Other)
            }

            fn visit_bool<E: de::Error>(self, _: bool) -> Result<Self::Value, E> {
                Ok(NumberLiteral::Other)
            }
        }

        deserializer.deserialize_any(NumberVisitor)
    }
}

/// An exact JSON number literal, kept exactly as written.
///
/// Used for `rawUsage.credit`, which must never pass through `f64`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NumberText(String);

impl NumberText {
    /// The literal text, e.g. `1.14`.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for NumberText {
    fn deserialize<D: de::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = Box::<RawValue>::deserialize(deserializer)?;
        Ok(NumberText(raw.get().to_owned()))
    }
}

/// `rawUsage.prompt_tokens_details` (snake_case, Chat style).
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
pub struct PromptTokensDetails {
    #[serde(default)]
    pub cached_tokens: Option<NumberLiteral>,
}

/// `rawUsage.completion_tokens_details`.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
pub struct CompletionTokensDetails {
    #[serde(default)]
    pub reasoning_tokens: Option<NumberLiteral>,
}

/// `usage.inputTokensDetails[0]` (camelCase, normalized style).
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InputTokensDetails {
    #[serde(default)]
    pub cached_tokens: Option<NumberLiteral>,
    #[serde(default)]
    pub cache_creation_input_tokens: Option<NumberLiteral>,
}

/// `usage.outputTokensDetails[0]`.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OutputTokensDetails {
    #[serde(default)]
    pub reasoning_tokens: Option<NumberLiteral>,
}

/// `providerData.rawUsage`: the observed WorkBuddy Chat wrapper (snake_case).
///
/// The top-level `cached_tokens` key is ambiguous and deliberately absent.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
pub struct RawUsage {
    #[serde(default)]
    pub prompt_tokens: Option<NumberLiteral>,
    #[serde(default)]
    pub completion_tokens: Option<NumberLiteral>,
    #[serde(default)]
    pub total_tokens: Option<NumberLiteral>,
    #[serde(default)]
    pub input_tokens: Option<NumberLiteral>,
    #[serde(default)]
    pub output_tokens: Option<NumberLiteral>,
    #[serde(default)]
    pub prompt_cache_hit_tokens: Option<NumberLiteral>,
    #[serde(default)]
    pub prompt_cache_write_tokens: Option<NumberLiteral>,
    #[serde(default)]
    pub cache_read_input_tokens: Option<NumberLiteral>,
    #[serde(default)]
    pub cache_creation_input_tokens: Option<NumberLiteral>,
    #[serde(default)]
    pub completion_thinking_tokens: Option<NumberLiteral>,
    #[serde(default)]
    pub prompt_tokens_details: Option<PromptTokensDetails>,
    #[serde(default)]
    pub completion_tokens_details: Option<CompletionTokensDetails>,
    #[serde(default)]
    pub input_tokens_details: Option<Vec<InputTokensDetails>>,
    #[serde(default)]
    pub output_tokens_details: Option<Vec<OutputTokensDetails>>,
    /// Exact decimal literal, never routed through `f64`.
    #[serde(default)]
    pub credit: Option<NumberText>,
}

/// `providerData.usage`: the normalized wrapper (camelCase).
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NormalizedUsageFields {
    #[serde(default)]
    pub credit: Option<NumberText>,
    #[serde(default)]
    pub input_tokens: Option<NumberLiteral>,
    #[serde(default)]
    pub output_tokens: Option<NumberLiteral>,
    #[serde(default)]
    pub total_tokens: Option<NumberLiteral>,
    #[serde(default)]
    pub input_tokens_details: Option<Vec<InputTokensDetails>>,
    #[serde(default)]
    pub output_tokens_details: Option<Vec<OutputTokensDetails>>,
}

/// The whitelisted part of `providerData`.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct RawProvider {
    #[serde(default)]
    credit: Option<NumberText>,
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    stage_id: Option<String>,
    #[serde(default)]
    conversation_request_id: Option<String>,
    #[serde(default)]
    message_id: Option<String>,
    #[serde(default)]
    request_model_id: Option<String>,
    #[serde(default)]
    request_model_name: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    response_model_id: Option<String>,
    #[serde(default)]
    raw_usage: Option<RawUsage>,
    #[serde(default)]
    usage: Option<NormalizedUsageFields>,
}

/// The whitelisted outer shape of one line.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawLine {
    #[serde(rename = "_fromSubAgent", default)]
    from_sub_agent: bool,
    #[serde(rename = "type", default)]
    kind: Option<String>,
    #[serde(default)]
    role: Option<String>,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    timestamp: Option<NumberLiteral>,
    #[serde(default)]
    provider_data: Option<RawProvider>,
}

/// Why a line could not be turned into a shell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShellError {
    /// The line is not a JSON object.
    JsonSyntax,
}

impl ShellError {
    /// Stable diagnostic code.
    pub fn as_str(self) -> &'static str {
        match self {
            ShellError::JsonSyntax => "json_syntax_error",
        }
    }
}

/// One usage-bearing record before identity and token rules.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UsageShell {
    pub session_id: Option<String>,
    pub message_id: Option<String>,
    pub requested_model: Option<String>,
    pub request_model_name: Option<String>,
    pub model: Option<String>,
    pub response_model_id: Option<String>,
    /// Integer-millisecond timestamp, when it was one.
    pub timestamp: Option<i64>,
    /// True when a timestamp was present but not a plain integer.
    pub timestamp_malformed: bool,
    pub raw_usage: Option<RawUsage>,
    pub usage: Option<NormalizedUsageFields>,
}

impl UsageShell {
    /// True when at least one usage object was present.
    ///
    /// A record with neither object is `ignored_no_usage`: it must never become
    /// a "zero token" fact.
    pub fn has_any_usage_object(&self) -> bool {
        self.raw_usage.is_some() || self.usage.is_some()
    }
}

/// What one complete line is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Shell {
    /// `message`+`assistant`, or `function_call` with a missing/assistant role.
    Usage(Box<UsageShell>),
    /// Any other record type or role: never carries importable usage.
    NotUsage,
}

/// Parses one complete line into its shell.
///
/// Model shells accept `message` with `role=assistant`, and `function_call` or
/// `model-usage` with a missing or `assistant` role. Forwarded child messages,
/// reasoning and tool results are `NotUsage`. Standalone credit receipts are
/// handled separately by `evaluate` (DATA-API section 1).
pub fn parse_shell(line: &[u8]) -> Result<Shell, ShellError> {
    parse_line(line).map(shell_from_line)
}

fn parse_line(line: &[u8]) -> Result<RawLine, ShellError> {
    // serde happily reads a sequence into a struct (fields in order), so an
    // array like `[]` would silently become an empty shell. A JSONL record must
    // be an object.
    let trimmed = line
        .iter()
        .position(|byte| !byte.is_ascii_whitespace())
        .map(|start| &line[start..])
        .unwrap_or_default();
    if !trimmed.starts_with(b"{") {
        return Err(ShellError::JsonSyntax);
    }
    serde_json::from_slice(line).map_err(|_| ShellError::JsonSyntax)
}

fn shell_from_line(raw: RawLine) -> Shell {
    // Team-member credit receipts account for forwarded child messages. The
    // copies on the leader's history are not independent model charges.
    if raw.from_sub_agent {
        return Shell::NotUsage;
    }
    let kind = raw.kind.as_deref().unwrap_or_default();
    let role = raw.role.as_deref();
    let usage_bearing = match kind {
        "message" => role == Some("assistant"),
        "function_call" | "model-usage" => role.is_none() || role == Some("assistant"),
        _ => false,
    };
    if !usage_bearing {
        return Shell::NotUsage;
    }
    let provider = raw.provider_data.unwrap_or_default();
    let timestamp = match raw.timestamp {
        Some(NumberLiteral::Integer(value)) => Some(value),
        _ => None,
    };
    Shell::Usage(Box::new(UsageShell {
        session_id: raw.session_id,
        message_id: provider.message_id,
        requested_model: provider.request_model_id,
        request_model_name: provider.request_model_name,
        model: provider.model,
        response_model_id: provider.response_model_id,
        timestamp,
        timestamp_malformed: matches!(raw.timestamp, Some(NumberLiteral::Other)),
        raw_usage: provider.raw_usage,
        usage: provider.usage,
    }))
}

/// Parses one complete line end to end into its final disposition.
///
/// Order of decisions, and why:
///
/// 1. a line that is not a JSON object is `json_syntax_error`;
/// 2. a record type or role that never carries usage is `ignored`;
/// 3. a usage-bearing record with no usage object at all is `ignored_no_usage`
///    (there is nothing to import, so a missing id is not worth a warning);
/// 4. a record without `messageId` is `pending_identity`; a structurally broken
///    identity is `malformed`; a bad timestamp is `invalid_timestamp`;
/// 5. everything else becomes a candidate — including records whose usage is
///    `missing` or `invalid`, because the row is still written with its status
///    and reason instead of being silently dropped.
pub fn evaluate(line: &[u8], file_stem: &str, namespace_id: &str, now_ms: i64) -> ParseDisposition {
    let raw = match parse_line(line) {
        Ok(raw) => raw,
        Err(ShellError::JsonSyntax) => return ParseDisposition::JsonSyntaxError,
    };
    if raw.kind.as_deref() == Some("credit-usage") {
        let provider = raw.provider_data.unwrap_or_default();
        return super::credits::evaluate(
            super::credits::Charge {
                session_id: raw.session_id,
                timestamp: raw.timestamp,
                credit: provider.credit,
                source: provider.source,
                stage_id: provider.stage_id,
                conversation_request_id: provider.conversation_request_id,
            },
            file_stem,
            namespace_id,
            now_ms,
        );
    }
    let is_model_receipt = raw.kind.as_deref() == Some("model-usage");
    let shell = shell_from_line(raw);
    let Shell::Usage(shell) = shell else {
        return ParseDisposition::Ignored;
    };
    if !shell.has_any_usage_object() {
        return ParseDisposition::IgnoredNoUsage;
    }
    let mut shell = shell;
    // WorkBuddy's response_done receipts omit sessionId; unlike normal
    // assistant messages, model-usage records belong to the enclosing file.
    if shell.session_id.is_none() && is_model_receipt {
        shell.session_id = Some(file_stem.to_owned());
    }
    let identity = match identify(&shell, file_stem, namespace_id, now_ms) {
        Identity::Ready(identity) => identity,
        Identity::Malformed(reason) => return ParseDisposition::Malformed(reason),
        Identity::Pending => return ParseDisposition::PendingIdentity,
        Identity::InvalidTimestamp => return ParseDisposition::InvalidTimestamp,
    };
    let mut usage = normalize::normalize_usage(shell.raw_usage.as_ref(), shell.usage.as_ref());
    normalize::normalize_cache_and_reasoning(
        &mut usage,
        shell.raw_usage.as_ref(),
        shell.usage.as_ref(),
    );
    normalize::normalize_credit(&mut usage, shell.raw_usage.as_ref());
    if shell
        .raw_usage
        .as_ref()
        .and_then(|raw| raw.credit.as_ref())
        .is_none()
        && let Some(credit) = shell.usage.as_ref().and_then(|usage| usage.credit.as_ref())
    {
        normalize::normalize_credit(
            &mut usage,
            Some(&RawUsage {
                credit: Some(credit.clone()),
                ..Default::default()
            }),
        );
    }
    ParseDisposition::Candidate(Box::new(Candidate {
        entry_key: identity.entry_key,
        session_hash: identity.session_hash,
        message_hash: identity.message_hash,
        requested_model: identity.requested_model,
        event_at_ms: identity.event_at_ms,
        upstream_model: identity.upstream_model,
        response_model: identity.response_model,
        usage,
    }))
}

/// Startup contract check: the shell rules stay closed and content stays out.
///
/// Runs on the real startup path with fixed synthetic lines, so a rule change
/// that would start admitting transcripts fails closed instead of importing
/// them later.
pub fn parser_contract_check() -> Result<usize, &'static str> {
    let assistant = br#"{"type":"message","role":"assistant","sessionId":"s","timestamp":1,
        "content":"SYNTHETIC_PROMPT_MUST_NOT_PERSIST",
        "providerData":{"messageId":"m","requestModelId":"hy3","rawUsage":{"prompt_tokens":1}}}"#;
    match parse_shell(assistant) {
        Ok(Shell::Usage(shell)) => {
            if shell.message_id.as_deref() != Some("m") || shell.raw_usage.is_none() {
                return Err("workbuddy_import_shell_shape_drifted");
            }
        }
        _ => return Err("workbuddy_import_shell_assistant_rejected"),
    }
    let tool = br#"{"type":"function_call","providerData":{"messageId":"m2","rawUsage":{}}}"#;
    if !matches!(parse_shell(tool), Ok(Shell::Usage(_))) {
        return Err("workbuddy_import_shell_function_call_rejected");
    }
    for excluded in [
        br#"{"type":"message","role":"user","providerData":{"messageId":"u"}}"#.as_slice(),
        br#"{"type":"reasoning","role":"assistant","providerData":{"messageId":"r"}}"#.as_slice(),
        br#"{"type":"function_call_result","providerData":{"messageId":"t"}}"#.as_slice(),
        br#"{"type":"message","providerData":{"messageId":"n"}}"#.as_slice(),
    ] {
        if !matches!(parse_shell(excluded), Ok(Shell::NotUsage)) {
            return Err("workbuddy_import_shell_excluded_accepted");
        }
    }
    if !matches!(parse_shell(b"{"), Err(ShellError::JsonSyntax)) {
        return Err("workbuddy_import_shell_syntax_accepted");
    }
    // An array is valid JSON but not a record.
    if !matches!(parse_shell(b"[]"), Err(ShellError::JsonSyntax)) {
        return Err("workbuddy_import_shell_non_object_accepted");
    }
    let credit = br#"{"type":"message","role":"assistant","providerData":{"messageId":"c",
        "rawUsage":{"credit":1.14}}}"#;
    match parse_shell(credit) {
        Ok(Shell::Usage(shell)) => {
            if !shell.has_any_usage_object() {
                return Err("workbuddy_import_shell_usage_flag_drifted");
            }
            let text = shell
                .raw_usage
                .as_ref()
                .and_then(|usage| usage.credit.as_ref())
                .ok_or("workbuddy_import_shell_credit_missing")?;
            if text.as_str() != "1.14" {
                return Err("workbuddy_import_shell_credit_text_drifted");
            }
        }
        _ => return Err("workbuddy_import_shell_credit_rejected"),
    }
    if ShellError::JsonSyntax.as_str() != "json_syntax_error" {
        return Err("workbuddy_import_shell_error_code_drifted");
    }
    let shell = match parse_shell(assistant) {
        Ok(Shell::Usage(shell)) => shell,
        _ => return Err("workbuddy_import_shell_assistant_rejected"),
    };
    match identify(&shell, "s", "0".repeat(48).as_str(), 1) {
        Identity::Ready(identity) => {
            if identity.entry_key.len() != 64 || identity.message_hash.len() != 64 {
                return Err("workbuddy_import_identity_shape_drifted");
            }
            if identity.event_at_ms != 1 {
                return Err("workbuddy_import_identity_time_drifted");
            }
        }
        _ => return Err("workbuddy_import_identity_rejected"),
    }
    if !matches!(
        identify(&shell, "other", "0".repeat(48).as_str(), 1),
        Identity::Malformed(MalformedReason::SessionMismatch)
    ) {
        return Err("workbuddy_import_identity_session_accepted");
    }
    if !matches!(
        identify(
            &shell,
            "s",
            "0".repeat(48).as_str(),
            1 - MAX_TIMESTAMP_SKEW_MS - 1
        ),
        Identity::InvalidTimestamp
    ) {
        return Err("workbuddy_import_identity_time_accepted");
    }
    let namespace = "0".repeat(48);
    match evaluate(assistant, "s", &namespace, 1) {
        ParseDisposition::Candidate(candidate) => {
            if candidate.entry_key.len() != 64
                || candidate.usage.status != crate::stats::dto::UsageStatus::Partial
            {
                return Err("workbuddy_import_evaluate_candidate_drifted");
            }
        }
        _ => return Err("workbuddy_import_evaluate_candidate_missing"),
    }
    let user_line = br#"{"type":"message","role":"user","providerData":{"messageId":"u"}}"#;
    if !matches!(
        evaluate(user_line, "s", &namespace, 1),
        ParseDisposition::Ignored
    ) {
        return Err("workbuddy_import_evaluate_ignored_drifted");
    }
    let no_usage = br#"{"type":"message","role":"assistant","sessionId":"s","timestamp":1,
        "providerData":{"messageId":"m"}}"#;
    if !matches!(
        evaluate(no_usage, "s", &namespace, 1),
        ParseDisposition::IgnoredNoUsage
    ) {
        return Err("workbuddy_import_evaluate_no_usage_drifted");
    }
    if !matches!(
        evaluate(b"{", "s", &namespace, 1),
        ParseDisposition::JsonSyntaxError
    ) {
        return Err("workbuddy_import_evaluate_syntax_drifted");
    }
    Ok(1)
}

/// The accepted clock skew between a source timestamp and "now".
pub const MAX_TIMESTAMP_SKEW_MS: i64 = 24 * 60 * 60 * 1000;

/// Identity of one usage-bearing message (DATA-API section 1).
///
/// Only hashes leave this module: the raw session and message ids are never
/// stored, logged or returned in an error.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MessageIdentity {
    /// 64 lowercase hex; equals `usage_requests.request_id` and `ledger.entry_key`.
    pub entry_key: String,
    /// SHA256 of the session id.
    pub session_hash: String,
    /// SHA256 of `providerData.messageId`.
    pub message_hash: String,
    pub requested_model: Option<String>,
    /// Integer-millisecond source time, frozen once accepted.
    pub event_at_ms: i64,
    /// `responseModelId` -> `model` -> `requestModelId`, first usable value.
    pub upstream_model: Option<String>,
    /// Only an explicitly reported response model.
    pub response_model: Option<String>,
}

/// Why a usage-bearing line cannot be identified yet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Identity {
    /// A usable identity: the record may continue to classification.
    Ready(Box<MessageIdentity>),
    /// Structurally unusable identity fields; never rewritten or truncated.
    Malformed(MalformedReason),
    /// `providerData.messageId` absent: no usage row and no ledger row.
    Pending,
    /// Timestamp missing, not an integer, negative, or beyond the clock skew.
    InvalidTimestamp,
}

/// `u32 big-endian byte length + UTF-8 bytes`, as the key contract requires.
fn push_field(hasher: &mut Sha256, value: &[u8]) {
    hasher.update((value.len() as u32).to_be_bytes());
    hasher.update(value);
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// 64 lowercase hex of one identifier, length-prefixed.
pub fn hash_id(value: &str) -> String {
    let mut hasher = Sha256::new();
    push_field(&mut hasher, value.as_bytes());
    hex(&hasher.finalize())
}

/// The message de-duplication key.
///
/// The domain, the namespace, the session id and the message id are all
/// length-prefixed, so no concatenation or trimming can make two different
/// tuples collide. `conversationRequestId`, trace ids, file offsets and the
/// top-level `id` deliberately do not take part.
pub fn entry_key(namespace_id: &str, session_id: &str, message_id: &str) -> String {
    let mut hasher = Sha256::new();
    push_field(&mut hasher, ENTRY_KEY_DOMAIN);
    push_field(&mut hasher, namespace_id.as_bytes());
    push_field(&mut hasher, session_id.as_bytes());
    push_field(&mut hasher, message_id.as_bytes());
    hex(&hasher.finalize())
}

/// Rejects identifiers that must never be truncated into colliding values.
fn check_id(value: &str) -> Result<(), MalformedReason> {
    if value.chars().count() > ID_LIMIT_CHARS {
        return Err(MalformedReason::OverlongId);
    }
    if value.chars().any(char::is_control) {
        return Err(MalformedReason::ControlCharacterInId);
    }
    Ok(())
}

/// Establishes the identity of a usage-bearing shell.
///
/// The session id must equal the file stem, `messageId` is mandatory, and the
/// timestamp must be integer milliseconds that are non-negative and no more
/// than [`MAX_TIMESTAMP_SKEW_MS`] in the future. A record that fails any of
/// these is diagnosed; the import time is never substituted for the source time.
pub fn identify(shell: &UsageShell, file_stem: &str, namespace_id: &str, now_ms: i64) -> Identity {
    let session_id = match shell.session_id.as_deref() {
        None => return Identity::Malformed(MalformedReason::MissingSessionId),
        Some(value) => value,
    };
    if session_id != file_stem {
        return Identity::Malformed(MalformedReason::SessionMismatch);
    }
    if let Err(reason) = check_id(session_id) {
        return Identity::Malformed(reason);
    }
    let message_id = match shell.message_id.as_deref() {
        None => return Identity::Pending,
        Some(value) => value,
    };
    if let Err(reason) = check_id(message_id) {
        return Identity::Malformed(reason);
    }
    for value in [
        shell.requested_model.as_deref(),
        shell.request_model_name.as_deref(),
        shell.model.as_deref(),
        shell.response_model_id.as_deref(),
    ]
    .into_iter()
    .flatten()
    {
        if let Err(reason) = check_id(value) {
            return Identity::Malformed(reason);
        }
    }
    let event_at_ms = match shell.timestamp {
        Some(value) if value >= 0 && value <= now_ms.saturating_add(MAX_TIMESTAMP_SKEW_MS) => value,
        _ => return Identity::InvalidTimestamp,
    };
    let upstream_model = shell
        .response_model_id
        .clone()
        .or_else(|| shell.model.clone())
        .or_else(|| shell.requested_model.clone());
    Identity::Ready(Box::new(MessageIdentity {
        entry_key: entry_key(namespace_id, session_id, message_id),
        session_hash: hash_id(session_id),
        message_hash: hash_id(message_id),
        requested_model: shell.requested_model.clone(),
        event_at_ms,
        upstream_model,
        response_model: shell.response_model_id.clone(),
    }))
}

#[cfg(test)]
mod shell_tests {
    use super::*;

    fn usage(line: &str) -> UsageShell {
        match parse_shell(line.as_bytes()) {
            Ok(Shell::Usage(shell)) => *shell,
            other => panic!("expected a usage shell, got {other:?}"),
        }
    }

    fn ignored(line: &str) {
        assert_eq!(parse_shell(line.as_bytes()), Ok(Shell::NotUsage), "{line}");
    }

    #[test]
    fn an_assistant_message_is_usage_bearing() {
        let shell = usage(
            r#"{"type":"message","role":"assistant","sessionId":"session-test","timestamp":1792108800000,
                "providerData":{"messageId":"b1","requestModelId":"hy3","requestModelName":"hy3",
                "model":"hy3","responseModelId":"hy3-latest","rawUsage":{"prompt_tokens":100}}}"#,
        );
        assert_eq!(shell.session_id.as_deref(), Some("session-test"));
        assert_eq!(shell.message_id.as_deref(), Some("b1"));
        assert_eq!(shell.requested_model.as_deref(), Some("hy3"));
        assert_eq!(shell.request_model_name.as_deref(), Some("hy3"));
        assert_eq!(shell.model.as_deref(), Some("hy3"));
        assert_eq!(shell.response_model_id.as_deref(), Some("hy3-latest"));
        assert_eq!(shell.timestamp, Some(1_792_108_800_000));
        assert!(!shell.timestamp_malformed);
        assert!(shell.has_any_usage_object());
    }

    #[test]
    fn a_role_less_function_call_is_usage_bearing() {
        let shell = usage(
            r#"{"type":"function_call","sessionId":"s","timestamp":1,
                "providerData":{"messageId":"b2","rawUsage":{"prompt_tokens":50}}}"#,
        );
        assert_eq!(shell.message_id.as_deref(), Some("b2"));
        assert!(shell.has_any_usage_object());
    }

    #[test]
    fn an_assistant_function_call_is_usage_bearing() {
        let shell = usage(
            r#"{"type":"function_call","role":"assistant","providerData":{"messageId":"b3"}}"#,
        );
        assert_eq!(shell.message_id.as_deref(), Some("b3"));
        assert!(!shell.has_any_usage_object(), "no usage object was present");
    }

    #[test]
    fn user_messages_reasoning_and_tool_results_are_excluded() {
        ignored(
            r#"{"type":"message","role":"user","providerData":{"messageId":"ignore1","rawUsage":{"prompt_tokens":999}}}"#,
        );
        ignored(
            r#"{"type":"reasoning","role":"assistant","providerData":{"messageId":"ignore2","rawUsage":{"prompt_tokens":999}}}"#,
        );
        ignored(
            r#"{"type":"function_call_result","role":"assistant","providerData":{"messageId":"ignore3","rawUsage":{"prompt_tokens":999}}}"#,
        );
        ignored(r#"{"type":"function_call_result","providerData":{"messageId":"ignore4"}}"#);
        ignored(r#"{"type":"message","role":"system","providerData":{"messageId":"ignore5"}}"#);
        ignored(r#"{"type":"message","providerData":{"messageId":"ignore6"}}"#);
        ignored(r#"{"type":"session-meta","providerData":{"messageId":"ignore7"}}"#);
        ignored(r#"{"role":"assistant","providerData":{"messageId":"ignore8"}}"#);
        ignored(r#"{"type":"function_call","role":"user","providerData":{"messageId":"ignore9"}}"#);
    }

    #[test]
    fn transcript_content_and_tool_arguments_never_reach_the_shell() {
        // The line carries a huge body and tool arguments; the shell only ever
        // exposes the whitelisted keys, so nothing else can be persisted.
        let body = "x".repeat(4096);
        let line = format!(
            r#"{{"type":"message","role":"assistant","id":"top-b1","content":"{body}",
                 "toolArguments":{{"path":"{body}"}},
                 "providerData":{{"messageId":"b1","rawUsage":{{"prompt_tokens":7}},
                 "toolCalls":[{{"arguments":"{body}"}}]}}}}"#
        );
        let shell = usage(&line);
        assert_eq!(shell.message_id.as_deref(), Some("b1"));
        let debug = format!("{shell:?}");
        assert!(
            !debug.contains(&body),
            "the shell must not carry transcript text"
        );
        assert!(!debug.contains("toolArguments"));
    }

    #[test]
    fn both_usage_wrappers_are_read_with_their_own_key_style() {
        let shell = usage(
            r#"{"type":"message","role":"assistant","providerData":{"messageId":"b1",
                "rawUsage":{"prompt_tokens":100,"completion_tokens":20,"total_tokens":120,
                    "prompt_tokens_details":{"cached_tokens":40},
                    "prompt_cache_hit_tokens":4096,"cache_read_input_tokens":0,
                    "completion_tokens_details":{"reasoning_tokens":3},
                    "credit":1.14,"cached_tokens":777},
                "usage":{"inputTokens":1,"outputTokens":2,"totalTokens":3,
                    "inputTokensDetails":[{"cachedTokens":4,"cacheCreationInputTokens":5}],
                    "outputTokensDetails":[{"reasoningTokens":6}]}}}"#,
        );
        let raw = shell.raw_usage.as_ref().unwrap();
        assert_eq!(raw.prompt_tokens, Some(NumberLiteral::Integer(100)));
        assert_eq!(
            raw.prompt_cache_hit_tokens,
            Some(NumberLiteral::Integer(4096))
        );
        assert_eq!(raw.cache_read_input_tokens, Some(NumberLiteral::Integer(0)));
        assert_eq!(
            raw.prompt_tokens_details.as_ref().unwrap().cached_tokens,
            Some(NumberLiteral::Integer(40))
        );
        assert_eq!(
            raw.completion_tokens_details
                .as_ref()
                .unwrap()
                .reasoning_tokens,
            Some(NumberLiteral::Integer(3))
        );
        assert_eq!(raw.credit.as_ref().unwrap().as_str(), "1.14");
        let normalized = shell.usage.as_ref().unwrap();
        assert_eq!(normalized.input_tokens, Some(NumberLiteral::Integer(1)));
        assert_eq!(normalized.total_tokens, Some(NumberLiteral::Integer(3)));
        let details = &normalized.input_tokens_details.as_ref().unwrap()[0];
        assert_eq!(details.cached_tokens, Some(NumberLiteral::Integer(4)));
        assert_eq!(
            details.cache_creation_input_tokens,
            Some(NumberLiteral::Integer(5))
        );
        assert_eq!(
            normalized.output_tokens_details.as_ref().unwrap()[0].reasoning_tokens,
            Some(NumberLiteral::Integer(6))
        );
    }

    #[test]
    fn non_integer_numbers_are_flagged_not_coerced() {
        let shell = usage(
            r#"{"type":"message","role":"assistant","timestamp":"not-a-number",
                "providerData":{"messageId":"b1","rawUsage":{"prompt_tokens":1.5,
                "completion_tokens":"20","total_tokens":true,"output_tokens":-1}}}"#,
        );
        assert_eq!(shell.timestamp, None);
        assert!(
            shell.timestamp_malformed,
            "a non-integer timestamp must be flagged"
        );
        let raw = shell.raw_usage.as_ref().unwrap();
        assert_eq!(raw.prompt_tokens, Some(NumberLiteral::Other));
        assert_eq!(raw.completion_tokens, Some(NumberLiteral::Other));
        assert_eq!(raw.total_tokens, Some(NumberLiteral::Other));
        assert_eq!(
            raw.output_tokens,
            Some(NumberLiteral::Integer(-1)),
            "negative stays readable"
        );
    }

    #[test]
    fn scientific_credit_keeps_its_literal_text() {
        let shell = usage(
            r#"{"type":"message","role":"assistant","providerData":{"messageId":"b1",
                "rawUsage":{"credit":1e3}}}"#,
        );
        assert_eq!(
            shell
                .raw_usage
                .as_ref()
                .unwrap()
                .credit
                .as_ref()
                .unwrap()
                .as_str(),
            "1e3",
            "the credit rule must see the original notation"
        );
    }

    #[test]
    fn a_missing_usage_object_is_not_a_zero_record() {
        let shell =
            usage(r#"{"type":"message","role":"assistant","providerData":{"messageId":"b1"}}"#);
        assert!(!shell.has_any_usage_object());
        assert!(shell.raw_usage.is_none() && shell.usage.is_none());
    }

    #[test]
    fn a_broken_line_reports_the_fixed_syntax_code() {
        assert_eq!(
            parse_shell(b"{\"type\":\"message\","),
            Err(ShellError::JsonSyntax)
        );
        assert_eq!(parse_shell(b""), Err(ShellError::JsonSyntax));
        assert_eq!(parse_shell(b"[]"), Err(ShellError::JsonSyntax));
        assert_eq!(ShellError::JsonSyntax.as_str(), "json_syntax_error");
    }

    #[test]
    fn the_parser_contract_holds() {
        let checks = parser_contract_check().expect("the shipped shell rules must hold");
        assert!(checks >= 1);
    }
}

#[cfg(test)]
mod identity_tests {
    use super::*;

    const NOW: i64 = 1_792_108_800_000;
    const NAMESPACE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    fn shell(line: &str) -> UsageShell {
        match parse_shell(line.as_bytes()) {
            Ok(Shell::Usage(shell)) => *shell,
            other => panic!("expected a usage shell, got {other:?}"),
        }
    }

    fn ready(line: &str, stem: &str) -> MessageIdentity {
        match identify(&shell(line), stem, NAMESPACE, NOW) {
            Identity::Ready(identity) => *identity,
            other => panic!("expected a ready identity, got {other:?}"),
        }
    }

    #[test]
    fn the_same_message_in_two_files_gets_one_key() {
        // X01: the same namespace/session/message copied into another project
        // file must produce exactly one identity.
        let line = r#"{"type":"message","role":"assistant","sessionId":"session-a","timestamp":1,
            "providerData":{"messageId":"b1","requestModelId":"hy3"}}"#;
        let first = ready(line, "session-a");
        let second = ready(line, "session-a");
        assert_eq!(first.entry_key, second.entry_key);
        assert_eq!(first.entry_key.len(), 64);
        assert!(
            first
                .entry_key
                .chars()
                .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
        );
        assert_eq!(first.session_hash, hash_id("session-a"));
        assert_eq!(first.message_hash, hash_id("b1"));
        assert_ne!(first.entry_key, first.session_hash);
        assert_eq!(first.requested_model.as_deref(), Some("hy3"));
        assert_eq!(first.event_at_ms, 1);
    }

    #[test]
    fn different_messages_in_one_session_get_different_keys() {
        let one = ready(
            r#"{"type":"message","role":"assistant","sessionId":"session-a","timestamp":1,
                "providerData":{"messageId":"b1"}}"#,
            "session-a",
        );
        let two = ready(
            r#"{"type":"message","role":"assistant","sessionId":"session-a","timestamp":1,
                "providerData":{"messageId":"b2"}}"#,
            "session-a",
        );
        assert_ne!(one.entry_key, two.entry_key);
        assert_ne!(one.message_hash, two.message_hash);
        assert_eq!(one.session_hash, two.session_hash);
    }

    #[test]
    fn a_conversation_request_id_is_not_a_dedup_key() {
        // X02: two steps share conversationRequestId but are different responses.
        let first = ready(
            r#"{"type":"message","role":"assistant","sessionId":"session-a","timestamp":1,
                "providerData":{"messageId":"b1","conversationRequestId":"shared-conversation"}}"#,
            "session-a",
        );
        let second = ready(
            r#"{"type":"function_call","sessionId":"session-a","timestamp":10,
                "providerData":{"messageId":"b2","conversationRequestId":"shared-conversation"}}"#,
            "session-a",
        );
        assert_ne!(
            first.entry_key, second.entry_key,
            "a shared conversation id must not collapse steps"
        );
        // The key is derived only from namespace + session + message.
        assert_eq!(first.entry_key, entry_key(NAMESPACE, "session-a", "b1"));
        assert_eq!(second.entry_key, entry_key(NAMESPACE, "session-a", "b2"));
    }

    #[test]
    fn a_different_namespace_isolates_the_same_message() {
        let line = r#"{"type":"message","role":"assistant","sessionId":"session-a","timestamp":1,
            "providerData":{"messageId":"b1"}}"#;
        let mine = ready(line, "session-a");
        let other = match identify(
            &shell(line),
            "session-a",
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            NOW,
        ) {
            Identity::Ready(identity) => *identity,
            other => panic!("expected a ready identity, got {other:?}"),
        };
        assert_ne!(mine.entry_key, other.entry_key);
    }

    #[test]
    fn the_session_must_match_the_file_stem() {
        let missing = r#"{"type":"message","role":"assistant","timestamp":1,
            "providerData":{"messageId":"b1"}}"#;
        assert_eq!(
            identify(&shell(missing), "session-a", NAMESPACE, NOW),
            Identity::Malformed(MalformedReason::MissingSessionId)
        );
        let mismatched = r#"{"type":"message","role":"assistant","sessionId":"session-b","timestamp":1,
            "providerData":{"messageId":"b1"}}"#;
        assert_eq!(
            identify(&shell(mismatched), "session-a", NAMESPACE, NOW),
            Identity::Malformed(MalformedReason::SessionMismatch)
        );
    }

    #[test]
    fn a_missing_message_id_is_pending_identity() {
        // X05: no trace id, offset or random key may stand in for it.
        let line = r#"{"type":"message","role":"assistant","sessionId":"session-a","timestamp":1,
            "providerData":{"requestModelId":"hy3","conversationRequestId":"c"}}"#;
        assert_eq!(
            identify(&shell(line), "session-a", NAMESPACE, NOW),
            Identity::Pending
        );
    }

    #[test]
    fn timestamps_must_be_integer_milliseconds_within_the_skew() {
        let at = |timestamp: &str| {
            let line = format!(
                r#"{{"type":"message","role":"assistant","sessionId":"session-a",{timestamp}
                    "providerData":{{"messageId":"b1"}}}}"#
            );
            identify(&shell(&line), "session-a", NAMESPACE, NOW)
        };
        assert_eq!(
            at(""),
            Identity::InvalidTimestamp,
            "a missing timestamp is not import time"
        );
        assert_eq!(
            at(r#""timestamp":"not-a-number","#),
            Identity::InvalidTimestamp
        );
        assert_eq!(at(r#""timestamp":1.5,"#), Identity::InvalidTimestamp);
        assert_eq!(at(r#""timestamp":-1,"#), Identity::InvalidTimestamp);
        assert!(
            matches!(at(r#""timestamp":0,"#), Identity::Ready(_)),
            "epoch zero is legal"
        );
        let boundary = NOW + MAX_TIMESTAMP_SKEW_MS;
        assert!(
            matches!(
                at(&format!(r#""timestamp":{boundary},"#)),
                Identity::Ready(_)
            ),
            "exactly 24h ahead is still accepted"
        );
        assert_eq!(
            at(&format!(r#""timestamp":{},"#, boundary + 1)),
            Identity::InvalidTimestamp,
            "beyond the skew is refused"
        );
    }

    #[test]
    fn control_characters_and_overlong_ids_are_malformed_not_truncated() {
        let overlong = "m".repeat(161);
        let line = format!(
            r#"{{"type":"message","role":"assistant","sessionId":"session-a","timestamp":1,
                "providerData":{{"messageId":"{overlong}"}}}}"#
        );
        assert_eq!(
            identify(&shell(&line), "session-a", NAMESPACE, NOW),
            Identity::Malformed(MalformedReason::OverlongId)
        );
        let control = r#"{"type":"message","role":"assistant","sessionId":"session-a","timestamp":1,
            "providerData":{"messageId":"b\u0001ad"}}"#;
        assert_eq!(
            identify(&shell(control), "session-a", NAMESPACE, NOW),
            Identity::Malformed(MalformedReason::ControlCharacterInId)
        );
        let bad_model = r#"{"type":"message","role":"assistant","sessionId":"session-a","timestamp":1,
            "providerData":{"messageId":"b1","requestModelId":"hy\u0002"}}"#;
        assert_eq!(
            identify(&shell(bad_model), "session-a", NAMESPACE, NOW),
            Identity::Malformed(MalformedReason::ControlCharacterInId)
        );
    }

    #[test]
    fn the_upstream_model_prefers_the_reported_response_model() {
        let both = ready(
            r#"{"type":"message","role":"assistant","sessionId":"session-a","timestamp":1,
                "providerData":{"messageId":"b1","requestModelId":"custom-local:x",
                "model":"glm-5.2","responseModelId":"glm-5.2-2026"}}"#,
            "session-a",
        );
        assert_eq!(both.upstream_model.as_deref(), Some("glm-5.2-2026"));
        assert_eq!(both.response_model.as_deref(), Some("glm-5.2-2026"));

        let fallback = ready(
            r#"{"type":"function_call","sessionId":"session-a","timestamp":1,
                "providerData":{"messageId":"b2","requestModelId":"auto","model":"glm-5.2"}}"#,
            "session-a",
        );
        assert_eq!(fallback.upstream_model.as_deref(), Some("glm-5.2"));
        assert_eq!(
            fallback.response_model, None,
            "no response model was reported"
        );

        let requested_only = ready(
            r#"{"type":"message","role":"assistant","sessionId":"session-a","timestamp":1,
                "providerData":{"messageId":"b3","requestModelId":"hy3"}}"#,
            "session-a",
        );
        assert_eq!(requested_only.upstream_model.as_deref(), Some("hy3"));
        assert_eq!(requested_only.response_model, None);
    }

    #[test]
    fn the_key_encoding_is_length_prefixed() {
        // Without length prefixes, ("ab","c") and ("a","bc") would collide.
        assert_ne!(
            entry_key(NAMESPACE, "ab", "c"),
            entry_key(NAMESPACE, "a", "bc")
        );
        assert_ne!(
            entry_key(NAMESPACE, "a", "bc"),
            entry_key(NAMESPACE, "a", "b")
        );
        assert_eq!(
            entry_key(NAMESPACE, "a", "b"),
            entry_key(NAMESPACE, "a", "b")
        );
        assert_eq!(hash_id("").len(), 64);
    }
}

#[cfg(test)]
mod disposition_tests {
    use super::*;
    use crate::stats::dto::UsageStatus;
    use crate::stats::workbuddy_import::types::{DiagnosticCounts, DiagnosticKind};

    const NOW: i64 = 1_792_108_800_000;
    const NAMESPACE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    fn disposition(line: &str) -> ParseDisposition {
        evaluate(line.as_bytes(), "session-a", NAMESPACE, NOW)
    }

    fn candidate(line: &str) -> Candidate {
        match disposition(line) {
            ParseDisposition::Candidate(candidate) => *candidate,
            other => panic!("expected a candidate, got {other:?}"),
        }
    }

    #[test]
    fn a_complete_record_becomes_a_candidate() {
        let found = candidate(
            r#"{"type":"message","role":"assistant","sessionId":"session-a","timestamp":1792108800000,
                "providerData":{"messageId":"b1","requestModelId":"hy3",
                "rawUsage":{"prompt_tokens":100,"completion_tokens":20,"total_tokens":120,
                "credit":1.14}}}"#,
        );
        assert_eq!(found.entry_key.len(), 64);
        assert_eq!(found.requested_model.as_deref(), Some("hy3"));
        assert_eq!(found.event_at_ms, NOW);
        assert_eq!(found.usage.status, UsageStatus::Complete);
        assert_eq!(found.usage.input_tokens, Some(100));
        assert_eq!(found.usage.workbuddy_credit_nanos, Some(1_140_000_000));
    }

    #[test]
    fn the_whole_chain_runs_in_one_call() {
        // Tokens, cache, reasoning and credit all come from the same line.
        let found = candidate(
            r#"{"type":"function_call","sessionId":"session-a","timestamp":1,
                "providerData":{"messageId":"b2","model":"glm-5.2",
                "rawUsage":{"prompt_tokens":1000,"completion_tokens":200,"total_tokens":1200,
                "prompt_cache_hit_tokens":400,"completion_thinking_tokens":50,"credit":2.5}}}"#,
        );
        assert_eq!(found.usage.input_tokens, Some(1000));
        assert_eq!(found.usage.cache_read_tokens, Some(400));
        assert_eq!(found.usage.reasoning_tokens, Some(50));
        assert_eq!(found.usage.workbuddy_credit_nanos, Some(2_500_000_000));
        assert_eq!(found.upstream_model.as_deref(), Some("glm-5.2"));
    }

    #[test]
    fn a_record_without_a_usage_object_is_ignored_no_usage() {
        assert_eq!(
            disposition(
                r#"{"type":"message","role":"assistant","sessionId":"session-a","timestamp":1,
                    "providerData":{"messageId":"b1","requestModelId":"hy3"}}"#
            ),
            ParseDisposition::IgnoredNoUsage
        );
        // Even without a message id: there is nothing to import, so no warning.
        assert_eq!(
            disposition(
                r#"{"type":"message","role":"assistant","sessionId":"session-a","timestamp":1,
                    "providerData":{"requestModelId":"hy3"}}"#
            ),
            ParseDisposition::IgnoredNoUsage
        );
    }

    #[test]
    fn a_usage_bearing_record_without_message_id_is_pending_identity() {
        assert_eq!(
            disposition(
                r#"{"type":"message","role":"assistant","sessionId":"session-a","timestamp":1,
                    "providerData":{"requestModelId":"hy3","rawUsage":{"prompt_tokens":5}}}"#
            ),
            ParseDisposition::PendingIdentity
        );
    }

    #[test]
    fn non_usage_records_are_ignored_before_anything_else() {
        for line in [
            r#"{"type":"message","role":"user","sessionId":"session-a","timestamp":1,"providerData":{"messageId":"b1","rawUsage":{"prompt_tokens":9}}}"#,
            r#"{"type":"reasoning","role":"assistant","sessionId":"session-a","timestamp":1,"providerData":{"messageId":"b2","rawUsage":{"prompt_tokens":9}}}"#,
            r#"{"type":"function_call_result","sessionId":"session-a","timestamp":1,"providerData":{"messageId":"b3","rawUsage":{"prompt_tokens":9}}}"#,
        ] {
            assert_eq!(disposition(line), ParseDisposition::Ignored, "{line}");
        }
    }

    #[test]
    fn identity_and_time_problems_keep_their_own_dispositions() {
        assert_eq!(
            disposition(
                r#"{"type":"message","role":"assistant","sessionId":"other","timestamp":1,
                    "providerData":{"messageId":"b1","rawUsage":{"prompt_tokens":5}}}"#
            ),
            ParseDisposition::Malformed(MalformedReason::SessionMismatch)
        );
        assert_eq!(
            disposition(
                r#"{"type":"message","role":"assistant","timestamp":1,
                    "providerData":{"messageId":"b1","rawUsage":{"prompt_tokens":5}}}"#
            ),
            ParseDisposition::Malformed(MalformedReason::MissingSessionId)
        );
        assert_eq!(
            disposition(
                r#"{"type":"message","role":"assistant","sessionId":"session-a","timestamp":1.5,
                    "providerData":{"messageId":"b1","rawUsage":{"prompt_tokens":5}}}"#
            ),
            ParseDisposition::InvalidTimestamp
        );
        assert_eq!(disposition("{"), ParseDisposition::JsonSyntaxError);
        assert_eq!(disposition("[]"), ParseDisposition::JsonSyntaxError);
    }

    #[test]
    fn an_invalid_or_missing_usage_still_becomes_a_candidate() {
        // The row must be written with its status and reason, not dropped.
        let broken = candidate(
            r#"{"type":"message","role":"assistant","sessionId":"session-a","timestamp":1,
                "providerData":{"messageId":"b1","rawUsage":{"prompt_tokens":-1}}}"#,
        );
        assert_eq!(broken.usage.status, UsageStatus::Invalid);
        assert_eq!(
            broken.usage.reason,
            Some(crate::stats::dto::UsageReason::InvalidNumbers)
        );

        let empty = candidate(
            r#"{"type":"message","role":"assistant","sessionId":"session-a","timestamp":1,
                "providerData":{"messageId":"b2","rawUsage":{}}}"#,
        );
        assert_eq!(empty.usage.status, UsageStatus::Missing);
        assert!(empty.usage.has_no_token_values());

        // A bad credit alone never invalidates the record.
        let bad_credit = candidate(
            r#"{"type":"message","role":"assistant","sessionId":"session-a","timestamp":1,
                "providerData":{"messageId":"b3","rawUsage":{"prompt_tokens":5,"credit":1e3}}}"#,
        );
        assert_eq!(bad_credit.usage.status, UsageStatus::Partial);
        assert!(bad_credit.usage.diagnostics.credit_invalid);
    }

    #[test]
    fn every_disposition_contributes_exactly_one_diagnostic() {
        let cases = [
            ("{", DiagnosticKind::JsonSyntaxError),
            (
                r#"{"type":"message","role":"user","providerData":{"messageId":"x"}}"#,
                DiagnosticKind::Ignored,
            ),
            (
                r#"{"type":"message","role":"assistant","sessionId":"session-a","timestamp":1,
                    "providerData":{"messageId":"x"}}"#,
                DiagnosticKind::IgnoredNoUsage,
            ),
            (
                r#"{"type":"message","role":"assistant","sessionId":"session-a","timestamp":1,
                    "providerData":{"rawUsage":{"prompt_tokens":1}}}"#,
                DiagnosticKind::PendingIdentity,
            ),
            (
                r#"{"type":"message","role":"assistant","sessionId":"session-a",
                    "providerData":{"messageId":"x","rawUsage":{"prompt_tokens":1}}}"#,
                DiagnosticKind::InvalidTimestamp,
            ),
            (
                r#"{"type":"message","role":"assistant","sessionId":"other","timestamp":1,
                    "providerData":{"messageId":"x","rawUsage":{"prompt_tokens":1}}}"#,
                DiagnosticKind::Malformed(MalformedReason::SessionMismatch),
            ),
        ];
        let mut counts = DiagnosticCounts::default();
        for (line, expected) in cases {
            let found = disposition(line);
            assert!(!found.is_candidate(), "{line}");
            assert_eq!(found.diagnostic(), Some(expected), "{line}");
            counts.add(expected, 1);
        }
        assert_eq!(counts.total(), cases.len() as i64);
        // A candidate contributes no diagnostic of its own.
        let found = disposition(
            r#"{"type":"message","role":"assistant","sessionId":"session-a","timestamp":1,
                "providerData":{"messageId":"b1","rawUsage":{"prompt_tokens":1}}}"#,
        );
        assert!(found.is_candidate());
        assert_eq!(found.diagnostic(), None);
    }

    #[test]
    fn the_disposition_contract_holds() {
        assert!(parser_contract_check().is_ok());
    }
}
