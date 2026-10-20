//! Durable, bounded attribution reconciliation on the sole statistics writer.
//! A mapping's watermark freezes its first task. Ledger revisions are the
//! durable progress marker, so a transaction failure never skips a row.
use super::{
    classify::Attribution,
    store,
    types::{Candidate, MAX_BATCH_ITEMS, NormalizedUsage},
};
use rusqlite::{Transaction, params};

pub(super) fn schedule(
    tx: &Transaction<'_>,
    id: &str,
    watermark: Option<i64>,
) -> Result<(), store::StoreError> {
    let watermark = match watermark {
        Some(value) => value,
        None => tx.query_row(
            "SELECT COALESCE(MAX(rowid),0) FROM workbuddy_import_ledger",
            [],
            |row| row.get(0),
        )?,
    };
    tx.execute(
        "UPDATE workbuddy_model_history SET reconcile_state='pending',reconcile_watermark=?2,
        reconciled_rows=0,reconcile_error_code=NULL WHERE mapping_id=?1",
        params![id, watermark],
    )?;
    Ok(())
}

/// Reconstruct only the allowlisted normalized columns; never read a transcript
/// in the writer. Unknown values and precise credit survive a revoke.
fn from_fact(
    tx: &Transaction<'_>,
    ledger: &store::LedgerRow,
) -> Result<Option<Candidate>, store::StoreError> {
    use rusqlite::OptionalExtension;
    let value=tx.query_row("SELECT upstream_model,response_model,usage_status,usage_reason,
        input_tokens,output_tokens,total_tokens,total_tokens_derived,cache_read_tokens,cache_write_tokens,
        reasoning_tokens,input_breakdown,workbuddy_credit_nanos,route_name FROM usage_requests
        WHERE request_id=?1 AND record_origin IN ('workbuddy_builtin','workbuddy_custom')",params![ledger.entry_key],|row| {
        Ok((row.get::<_,Option<String>>(0)?,row.get::<_,Option<String>>(1)?,row.get::<_,String>(13)?,serde_json::json!({
            "status":row.get::<_,String>(2)?,"reason":row.get::<_,Option<String>>(3)?,
            "input_tokens":row.get::<_,Option<i64>>(4)?,"output_tokens":row.get::<_,Option<i64>>(5)?,
            "total_tokens":row.get::<_,Option<i64>>(6)?,"total_tokens_derived":row.get::<_,bool>(7)?,
            "cache_read_tokens":row.get::<_,Option<i64>>(8)?,"cache_write_tokens":row.get::<_,Option<i64>>(9)?,
            "reasoning_tokens":row.get::<_,Option<i64>>(10)?,"input_breakdown":row.get::<_,String>(11)?,
            "workbuddy_credit_nanos":row.get::<_,Option<i64>>(12)?,"diagnostics":super::types::UsageDiagnostics::default(),
        })))
    }).optional()?;
    value
        .map(|(upstream_model, response_model, route_name, usage)| {
            Ok(Candidate {
                entry_key: ledger.entry_key.clone(),
                session_hash: ledger.session_hash.clone(),
                message_hash: ledger.message_hash.clone(),
                requested_model: ledger.requested_model.clone(),
                event_at_ms: ledger.event_at_ms,
                upstream_model,
                response_model,
                tool_name: (ledger.requested_model.as_deref()
                    == Some("workbuddy-credit:generation_tool")
                    && route_name != "workbuddy-credit:generation_tool")
                    .then_some(route_name),
                usage: serde_json::from_value::<NormalizedUsage>(usage)
                    .map_err(|_| store::StoreError::CandidateNotAllowed)?,
            })
        })
        .transpose()
}

fn candidate(
    tx: &Transaction<'_>,
    ledger: &store::LedgerRow,
) -> Result<Option<Candidate>, store::StoreError> {
    if ledger.state == store::LedgerState::Imported {
        return from_fact(tx, ledger);
    }
    ledger
        .candidate_json
        .as_deref()
        .map(|text| {
            let candidate: Candidate =
                serde_json::from_str(text).map_err(|_| store::StoreError::CandidateNotAllowed)?;
            if candidate.entry_key != ledger.entry_key
                || candidate.session_hash != ledger.session_hash
                || candidate.message_hash != ledger.message_hash
                || candidate.requested_model != ledger.requested_model
                || candidate.event_at_ms != ledger.event_at_ms
            {
                return Err(store::StoreError::CandidateNotAllowed);
            }
            Ok(candidate)
        })
        .transpose()
}

fn reclassify(
    tx: &Transaction<'_>,
    ledger: &store::LedgerRow,
    revision: i64,
    retention: i64,
    now: i64,
) -> Result<bool, store::StoreError> {
    let candidate = candidate(tx, ledger)?;
    // Missing candidates on a revoked proxy exclusion stay visible as pending;
    // the source worker schedules a full reread instead of inventing usage.
    let shell = Candidate {
        entry_key: ledger.entry_key.clone(),
        session_hash: ledger.session_hash.clone(),
        message_hash: ledger.message_hash.clone(),
        requested_model: ledger.requested_model.clone(),
        event_at_ms: ledger.event_at_ms,
        upstream_model: None,
        response_model: None,
        tool_name: None,
        usage: NormalizedUsage::missing(),
    };
    let (attribution, mapping_id) = store::attribute_candidate(
        tx,
        &ledger.namespace_id,
        candidate.as_ref().unwrap_or(&shell),
    )?;
    let state = match attribution {
        Attribution::Proxy => store::LedgerState::ExcludedProxy,
        _ if store::is_expired(ledger.event_at_ms, now, retention) => {
            store::LedgerState::ExcludedRetention
        }
        Attribution::Builtin | Attribution::Direct if candidate.is_some() => {
            store::LedgerState::Imported
        }
        _ => store::LedgerState::PendingClassification,
    };
    let withdrawn = if ledger.state == store::LedgerState::Imported {
        store::withdraw_workbuddy_fact(tx, &ledger.entry_key)?
    } else {
        false
    };
    let item = candidate
        .map(|candidate| store::CommitItem::from_candidate(candidate, attribution))
        .transpose()?;
    if state == store::LedgerState::Imported {
        let item = item
            .as_ref()
            .ok_or(store::StoreError::CandidateNotAllowed)?;
        store::insert_fact_row(
            tx,
            item,
            item.origin.ok_or(store::StoreError::CandidateNotAllowed)?,
            now,
        )?;
    }
    let payload = if state == store::LedgerState::PendingClassification {
        item.as_ref()
            .and_then(|item| item.candidate_json.as_deref())
    } else {
        None
    };
    store::set_state(
        tx,
        &ledger.entry_key,
        state,
        payload,
        now.max(ledger.created_at_ms),
    )?;
    tx.execute(
        "UPDATE workbuddy_import_ledger SET mapping_id=?2,mapping_revision=?3 WHERE entry_key=?1",
        params![ledger.entry_key, mapping_id, revision],
    )?;
    Ok(withdrawn)
}

/// One batch and its task progress commit together in the caller's transaction.
pub(super) fn run(
    tx: &Transaction<'_>,
    id: &str,
    _retention: i64,
    _now: i64,
) -> Result<bool, store::StoreError> {
    tx.execute("UPDATE workbuddy_model_history SET reconcile_state='completed',reconcile_error_code=NULL WHERE mapping_id=?1",params![id])?;
    Ok(false)
}

/// Late pending records are handled independently of a user's frozen task.
/// The durable ledger revision makes this bounded pass restart-safe as well.
pub(super) fn catch_up(
    tx: &Transaction<'_>,
    retention: i64,
    now: i64,
) -> Result<usize, store::StoreError> {
    let settings = store::load_import_settings(tx)?;
    let mut statement=tx.prepare("SELECT l.entry_key FROM workbuddy_import_ledger l
        WHERE l.namespace_id=?1 AND (
          (l.state='pending_classification' AND l.candidate_json IS NOT NULL AND l.requested_model IS NOT NULL)
          OR (l.state='excluded_proxy' AND NOT EXISTS(SELECT 1 FROM workbuddy_proxy_identity p
            JOIN usage_requests r ON r.request_id=p.request_id WHERE p.session_hash=l.session_hash
            AND p.message_hash=l.message_hash AND r.record_origin='proxy')))
        ORDER BY l.rowid LIMIT ?2")?;
    let keys = statement
        .query_map(
            params![settings.namespace_id, MAX_BATCH_ITEMS as i64],
            |row| row.get::<_, String>(0),
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let count = keys.len() + store::resume_proxy_dedup(tx, now)?;
    for key in keys {
        let row = store::load(tx, &key)?.ok_or(store::StoreError::NotFound)?;
        reclassify(tx, &row, settings.mapping_revision, retention, now)?;
    }
    Ok(count)
}

/// Status is recovered from durable history, including after core restart.
pub(super) fn task_status(
    _: &Transaction<'_>,
) -> Result<Option<serde_json::Value>, store::StoreError> {
    Ok(None)
}
