//! Standalone WorkBuddy charges. Tool response payloads are mirrors, never a
//! second billing source. Keep credit stages separate from model messages.
use super::parser::{self, NumberLiteral, NumberText};
use super::types::{Candidate, MalformedReason, NormalizedUsage, ParseDisposition};
use sha2::{Digest, Sha256};

pub(super) struct Charge {
    pub session_id: Option<String>,
    pub timestamp: Option<NumberLiteral>,
    pub credit: Option<NumberText>,
    pub source: Option<String>,
    pub stage_id: Option<String>,
    pub conversation_request_id: Option<String>,
}

fn hash(fields: &[&[u8]]) -> String {
    let mut digest = Sha256::new();
    for field in fields {
        digest.update((field.len() as u32).to_be_bytes());
        digest.update(field);
    }
    digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn valid_id(id: &str) -> Result<(), MalformedReason> {
    if id.chars().count() > super::types::ID_LIMIT_CHARS {
        return Err(MalformedReason::OverlongId);
    }
    if id.chars().any(char::is_control) {
        return Err(MalformedReason::ControlCharacterInId);
    }
    Ok(())
}

pub(super) fn evaluate(row: Charge, stem: &str, namespace: &str, now: i64) -> ParseDisposition {
    let session = row.session_id.as_deref().unwrap_or(stem);
    if session != stem {
        return ParseDisposition::Malformed(MalformedReason::SessionMismatch);
    }
    let (Some(source), Some(stage), Some(request)) = (
        row.source.as_deref().filter(|s| !s.is_empty()),
        row.stage_id.as_deref().filter(|s| !s.is_empty()),
        row.conversation_request_id
            .as_deref()
            .filter(|s| !s.is_empty()),
    ) else {
        return ParseDisposition::PendingIdentity;
    };
    for id in [session, source, stage, request] {
        if let Err(reason) = valid_id(id) {
            return ParseDisposition::Malformed(reason);
        }
    }
    let Some(NumberLiteral::Integer(timestamp)) = row.timestamp else {
        return ParseDisposition::InvalidTimestamp;
    };
    if timestamp < 0 || timestamp > now.saturating_add(parser::MAX_TIMESTAMP_SKEW_MS) {
        return ParseDisposition::InvalidTimestamp;
    }
    let Some(credit) = row.credit else {
        return ParseDisposition::IgnoredNoUsage;
    };
    // The official collector keys a stage by conversationRequestId + stageId,
    // not its random history id or the model's messageId. Distinct domains
    // also prevent a charge from colliding with its parent model response.
    let message_hash = hash(&[
        b"wbp-wb-credit-stage-v1",
        request.as_bytes(),
        stage.as_bytes(),
    ]);
    let entry_key = hash(&[
        b"wbp-wb-credit-entry-v1",
        namespace.as_bytes(),
        request.as_bytes(),
        stage.as_bytes(),
    ]);
    let mut usage = NormalizedUsage::missing();
    match super::normalize::parse_credit_nanos(credit.as_str()) {
        Ok(value) => usage.workbuddy_credit_nanos = Some(value),
        Err(_) => usage.diagnostics.credit_invalid = true,
    }
    let model = format!("workbuddy-credit:{source}");
    if let Err(reason) = valid_id(&model) {
        return ParseDisposition::Malformed(reason);
    }
    ParseDisposition::Candidate(Box::new(Candidate {
        entry_key,
        session_hash: parser::hash_id(session),
        message_hash,
        requested_model: Some(model.clone()),
        upstream_model: Some(model),
        response_model: None,
        event_at_ms: timestamp,
        usage,
    }))
}

#[cfg(test)]
mod tests {
    use super::super::{classify::Attribution, store::CommitItem};
    use super::*;
    use serde_json::json;

    fn event(
        source: &str,
        stage: &str,
        request: &str,
        credit: serde_json::Value,
    ) -> serde_json::Value {
        json!({"type":"credit-usage","id":"random-history-id","parentId":"tool-call", "timestamp":100,
            "providerData":{"conversationRequestId":request,"messageId":"parent-model-message",
                "source":source,"stageId":stage,"credit":credit}})
    }

    fn parse(value: &serde_json::Value) -> ParseDisposition {
        parser::evaluate(
            value.to_string().as_bytes(),
            "session",
            &"a".repeat(48),
            1000,
        )
    }

    fn candidate(value: &serde_json::Value) -> Candidate {
        match parse(value) {
            ParseDisposition::Candidate(value) => *value,
            other => panic!("expected a candidate, got {other:?}"),
        }
    }

    #[test]
    fn a_generation_receipt_has_exact_credit_but_no_fabricated_tokens() {
        let value = candidate(&event("generation_tool", "image:1", "request", json!(5.71)));
        assert_eq!(value.usage.workbuddy_credit_nanos, Some(5_710_000_000));
        assert_eq!(
            value.requested_model.as_deref(),
            Some("workbuddy-credit:generation_tool")
        );
        assert_eq!(value.usage.status, crate::stats::dto::UsageStatus::Missing);
        assert!(value.usage.has_no_token_values());
        assert_eq!(value.event_at_ms, 100);
    }

    #[test]
    fn charge_identity_uses_request_and_stage_not_history_or_model_message_ids() {
        let original = event("generation_tool", "image:1", "request", json!(5.71));
        let a = CommitItem::from_candidate(candidate(&original), Attribution::Builtin).unwrap();
        let mut replay = original.clone();
        replay["id"] = json!("new-random-id");
        replay["timestamp"] = json!(200);
        replay["providerData"]["messageId"] = json!("different-model-message");
        let b = CommitItem::from_candidate(candidate(&replay), Attribution::Builtin).unwrap();
        assert_eq!(a.entry_key, b.entry_key);
        assert_eq!(a.normalized_hash, b.normalized_hash);
        let copied = match parser::evaluate(
            replay.to_string().as_bytes(),
            "copied-session",
            &"a".repeat(48),
            1000,
        ) {
            ParseDisposition::Candidate(value) => value,
            other => panic!("expected copied receipt, got {other:?}"),
        };
        assert_eq!(
            copied.entry_key, a.entry_key,
            "a copied file is not another charge"
        );
        assert_ne!(
            a.entry_key,
            candidate(&event("generation_tool", "image:2", "request", json!(5.71))).entry_key
        );
        assert_ne!(
            a.entry_key,
            candidate(&event(
                "generation_tool",
                "image:1",
                "other-request",
                json!(5.71)
            ))
            .entry_key
        );
    }

    #[test]
    fn tool_result_amounts_are_not_another_billing_source() {
        let result = json!({"type":"function_call_result","sessionId":"session","timestamp":100,
            "providerData":{"messageId":"tool-result","rawUsage":{"credit":5.71},
                "toolResult":{"rawResponse":{"type":"image_gen_tool_result","status":"completed","credit":5.71,"prompt":"PRIVATE_PROMPT"}}}});
        assert_eq!(parse(&result), ParseDisposition::Ignored);
        let receipt = candidate(&event("generation_tool", "image:1", "request", json!(5.71)));
        assert!(
            !serde_json::to_string(&receipt)
                .unwrap()
                .contains("PRIVATE_PROMPT")
        );
    }

    #[test]
    fn team_member_and_future_explicit_sources_are_kept_separate() {
        for source in ["team_member", "future_metered_service"] {
            let value = candidate(&event(source, "stage", "request", json!(1.14)));
            assert_eq!(value.usage.workbuddy_credit_nanos, Some(1_140_000_000));
            assert_eq!(
                value.requested_model,
                Some(format!("workbuddy-credit:{source}"))
            );
        }
        let forwarded = json!({"type":"model-usage","_fromSubAgent":true,"timestamp":100,
            "providerData":{"messageId":"child-response","requestModelId":"deepseek",
                "rawUsage":{"prompt_tokens":100,"completion_tokens":20,"credit":1.14}}});
        assert_eq!(parse(&forwarded), ParseDisposition::Ignored);
    }

    #[test]
    fn incomplete_or_invalid_charge_identity_is_never_guessed() {
        for key in ["source", "stageId", "conversationRequestId"] {
            let mut value = event("generation_tool", "stage", "request", json!(5.71));
            value["providerData"].as_object_mut().unwrap().remove(key);
            assert_eq!(parse(&value), ParseDisposition::PendingIdentity);
        }
        let mut value = event("generation_tool", "stage", "request", json!(5.71));
        value["sessionId"] = json!("wrong-session");
        assert_eq!(
            parse(&value),
            ParseDisposition::Malformed(MalformedReason::SessionMismatch)
        );
        value.as_object_mut().unwrap().remove("sessionId");
        value["timestamp"] = json!(-1);
        assert_eq!(parse(&value), ParseDisposition::InvalidTimestamp);
        value["timestamp"] = json!(100);
        value["providerData"]["stageId"] = json!("bad\nstage");
        assert_eq!(
            parse(&value),
            ParseDisposition::Malformed(MalformedReason::ControlCharacterInId)
        );
    }

    #[test]
    fn zero_is_known_but_invalid_credit_is_not_coerced_into_zero() {
        assert_eq!(
            candidate(&event("generation_tool", "zero", "request", json!(0)))
                .usage
                .workbuddy_credit_nanos,
            Some(0)
        );
        for value in [json!(-1), json!("5.71"), json!(0.1234567891)] {
            let value = candidate(&event("generation_tool", "invalid", "request", value));
            assert_eq!(value.usage.workbuddy_credit_nanos, None);
            assert!(value.usage.diagnostics.credit_invalid);
        }
    }

    #[test]
    fn model_receipts_and_assistant_messages_share_the_existing_message_identity() {
        let receipt = json!({"type":"model-usage","timestamp":100,"providerData":{"messageId":"response", "requestModelId":"deepseek", "rawUsage":{"prompt_tokens":100,"completion_tokens":20,"credit":1.14}}});
        let a = CommitItem::from_candidate(candidate(&receipt), Attribution::Builtin).unwrap();
        let mut message = receipt;
        message["type"] = json!("message");
        message["role"] = json!("assistant");
        message["sessionId"] = json!("session");
        let b = CommitItem::from_candidate(candidate(&message), Attribution::Builtin).unwrap();
        assert_eq!(a.entry_key, b.entry_key);
        assert_eq!(a.normalized_hash, b.normalized_hash);
    }

    #[test]
    fn normalized_usage_credit_is_a_fallback_not_an_additional_charge() {
        let mut row = json!({"type":"model-usage","timestamp":100,"providerData":{"messageId":"response", "requestModelId":"deepseek", "usage":{"inputTokens":100,"outputTokens":20,"credit":1.14}}});
        assert_eq!(
            candidate(&row).usage.workbuddy_credit_nanos,
            Some(1_140_000_000)
        );
        row["providerData"]["rawUsage"] = json!({"credit":2.13});
        assert_eq!(
            candidate(&row).usage.workbuddy_credit_nanos,
            Some(2_130_000_000)
        );
        row["providerData"]["rawUsage"]["credit"] = json!("bad");
        let bad = candidate(&row);
        assert_eq!(bad.usage.workbuddy_credit_nanos, None);
        assert!(bad.usage.diagnostics.credit_invalid);
    }
}
