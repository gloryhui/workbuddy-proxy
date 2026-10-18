//! Splits uncertain history without granting direct/proxy proof. Used by both
//! actual profile observations and explicit user attribution confirmations.
use super::{reconcile, store};
use rusqlite::{Transaction, params};

pub(super) fn insert(
    tx: &Transaction<'_>,
    mapping: &store::NewMapping,
) -> Result<(), store::StoreError> {
    store::insert_mapping(tx, mapping)?;
    reconcile::schedule(tx, &mapping.mapping_id, None)
}

pub(super) fn unknown(
    tx: &Transaction<'_>,
    ns: &str,
    model: &str,
    from: i64,
    to: Option<i64>,
    now: i64,
) -> Result<(), store::StoreError> {
    insert(
        tx,
        &store::NewMapping {
            mapping_id: crate::config::token(),
            namespace_id: ns.into(),
            requested_model: model.into(),
            classification: store::MappingClass::Proxy,
            valid_from_ms: from,
            valid_to_ms: to,
            evidence: store::MappingEvidence::ObservedConfig,
            state: store::MappingState::Conflict,
            route_id: None,
            provider_id: None,
            now_ms: now,
        },
    )
}

/// Cut only conflict intervals in the newly proven/explicitly confirmed range.
/// Prepared intents and verified proxy intervals are never overridden here.
pub(super) fn clear_uncertainty(
    tx: &Transaction<'_>,
    ns: &str,
    model: &str,
    from: i64,
    to: Option<i64>,
    now: i64,
) -> Result<(), store::StoreError> {
    let mut statement=tx.prepare("SELECT mapping_id FROM workbuddy_model_history
        WHERE namespace_id=?1 AND requested_model=?2 AND state='conflict'
        AND valid_from_ms<COALESCE(?4,9223372036854775807) AND ?3<COALESCE(valid_to_ms,9223372036854775807)
        ORDER BY valid_from_ms,mapping_id LIMIT 257")?;
    let ids = statement
        .query_map(params![ns, model, from, to], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if ids.len() > 256 {
        return Err(store::StoreError::Overlapping);
    }
    for id in ids {
        let mapping = store::load_mapping(tx, &id)?.ok_or(store::StoreError::NotFound)?;
        store::set_mapping_state(
            tx,
            &id,
            store::MappingState::Revoked,
            now.max(mapping.created_at_ms),
        )?;
        tx.execute(
            "UPDATE workbuddy_model_history SET reconcile_state='completed' WHERE mapping_id=?1",
            params![id],
        )?;
        if mapping.valid_from_ms < from {
            unknown(tx, ns, model, mapping.valid_from_ms, Some(from), now)?;
        }
        if let Some(end) = to
            && mapping.valid_to_ms.is_none_or(|old_end| old_end > end)
        {
            unknown(tx, ns, model, end, mapping.valid_to_ms, now)?;
        }
    }
    Ok(())
}
