//! Non-sensitive configuration evidence and durable synchronization intents.
//! All mutations borrow the sole statistics writer's transaction. Current
//! configuration proves a future interval; it never authorizes older history.
use super::{classify::ConfigObservation, store};
use rusqlite::{Transaction, params};
use std::collections::HashMap;

#[derive(Clone, Debug)]
pub(crate) struct ManagedModel {
    pub requested_model: String,
    /// None records a removal intent, never a guessed destination.
    pub route_id: Option<String>,
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.chars().count() <= crate::stats::dto::MAX_ID_CHARS
        && !id.chars().any(char::is_control)
}

fn bump_revision(tx: &Transaction<'_>) -> Result<(), store::StoreError> {
    tx.execute("UPDATE stats_settings SET workbuddy_mapping_revision=workbuddy_mapping_revision+1 WHERE singleton=1", [])?;
    Ok(())
}

fn close_covering(
    tx: &Transaction<'_>,
    ns: &str,
    model: &str,
    at: i64,
) -> Result<usize, store::StoreError> {
    Ok(tx.execute(
        "UPDATE workbuddy_model_history SET valid_to_ms=?3,revision=revision+1,
        updated_at_ms=MAX(updated_at_ms,?3) WHERE namespace_id=?1 AND requested_model=?2
        AND state='verified' AND valid_from_ms<?3 AND (valid_to_ms IS NULL OR valid_to_ms>?3)",
        params![ns, model, at],
    )?)
}

/// Stop extending any previously verified interval across a period in which
/// configuration is not observed. Old facts keep their valid historical range;
/// future direct calls require fresh explicit evidence after resumption.
pub(crate) fn suspend_history(tx: &Transaction<'_>, at: i64) -> Result<(), store::StoreError> {
    let mut statement = tx.prepare(
        "SELECT mapping_id FROM workbuddy_model_history
        WHERE state='verified' AND (valid_to_ms IS NULL OR valid_to_ms>?1)",
    )?;
    let ids = statement
        .query_map(params![at], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for id in &ids {
        retire_unknown(tx, id, at, at)?;
    }
    if !ids.is_empty() {
        bump_revision(tx)?;
    }
    Ok(())
}

fn retire_unknown(
    tx: &Transaction<'_>,
    id: &str,
    boundary: i64,
    now: i64,
) -> Result<(), store::StoreError> {
    let mapping = store::load_mapping(tx, id)?.ok_or(store::StoreError::NotFound)?;
    if mapping.valid_from_ms >= boundary {
        store::set_mapping_state(
            tx,
            id,
            store::MappingState::Conflict,
            now.max(mapping.created_at_ms),
        )?;
        super::reconcile::schedule(tx, id, None)?;
    } else {
        tx.execute(
            "UPDATE workbuddy_model_history SET valid_to_ms=?2,revision=revision+1,
            updated_at_ms=MAX(updated_at_ms,?3) WHERE mapping_id=?1",
            params![id, boundary, now],
        )?;
        super::history::unknown(
            tx,
            &mapping.namespace_id,
            &mapping.requested_model,
            boundary,
            mapping.valid_to_ms,
            now,
        )?;
    }
    Ok(())
}

/// Commit the entire plan before the caller may mutate models.json. Metadata
/// journaling continues after opt-out for an existing namespace, protecting a
/// later backfill from attributing an off-period to stale direct rules.
pub(crate) fn prepare_sync(
    tx: &Transaction<'_>,
    models: &[ManagedModel],
    at: i64,
) -> Result<Option<String>, store::StoreError> {
    let settings = store::load_import_settings(tx)?;
    if settings.namespace_id.is_empty() {
        return Ok(None);
    }
    let token = crate::config::token();
    for (index, model) in models.iter().enumerate() {
        if !valid_id(&model.requested_model)
            || model.route_id.as_deref().is_some_and(|id| !valid_id(id))
        {
            return Err(store::StoreError::CandidateNotAllowed);
        }
        close_covering(tx, &settings.namespace_id, &model.requested_model, at)?;
        super::history::insert(
            tx,
            &store::NewMapping {
                mapping_id: format!("{token}:{index}"),
                namespace_id: settings.namespace_id.clone(),
                requested_model: model.requested_model.clone(),
                classification: store::MappingClass::Proxy,
                valid_from_ms: at,
                valid_to_ms: None,
                evidence: store::MappingEvidence::ManagedSync,
                state: store::MappingState::Prepared,
                route_id: model.route_id.clone(),
                provider_id: None,
                now_ms: at,
            },
        )?;
    }
    if !models.is_empty() {
        bump_revision(tx)?;
    }
    Ok(Some(token))
}

fn current_routes(observations: &[ConfigObservation]) -> HashMap<&str, Option<&str>> {
    let mut routes = HashMap::new();
    for observation in observations
        .iter()
        .filter(|observation| observation.is_usable())
    {
        if let Some(model) = observation.model_id.as_deref() {
            let route = observation.route_id.as_deref();
            routes
                .entry(model)
                .and_modify(|current| {
                    if *current != route {
                        *current = None;
                    }
                })
                .or_insert(route);
        }
    }
    routes
}

/// End the uncertain gap, then independently record the verified future.
/// Equal millisecond timestamps require a one-millisecond uncertainty interval;
/// clock regression cannot prove an ordered boundary and fails closed.
fn resolve_intent(
    tx: &Transaction<'_>,
    mapping: &store::ModelMapping,
    matched: bool,
    at: i64,
) -> Result<bool, store::StoreError> {
    if at < mapping.valid_from_ms {
        store::set_mapping_state(
            tx,
            &mapping.mapping_id,
            store::MappingState::Conflict,
            at.max(mapping.created_at_ms),
        )?;
        return Ok(false);
    }
    let end = at.max(mapping.valid_from_ms.saturating_add(1));
    tx.execute(
        "UPDATE workbuddy_model_history SET state='conflict',valid_to_ms=?2,
        updated_at_ms=MAX(updated_at_ms,?2),revision=revision+1 WHERE mapping_id=?1",
        params![mapping.mapping_id, end],
    )?;
    super::reconcile::schedule(tx, &mapping.mapping_id, None)?;
    if !matched || mapping.route_id.is_none() {
        super::history::unknown(
            tx,
            &mapping.namespace_id,
            &mapping.requested_model,
            end,
            None,
            end,
        )?;
        return Ok(matched);
    }
    super::history::clear_uncertainty(
        tx,
        &mapping.namespace_id,
        &mapping.requested_model,
        end,
        None,
        end,
    )?;
    close_covering(tx, &mapping.namespace_id, &mapping.requested_model, end)?;
    if store::overlapping_verified(
        tx,
        &mapping.namespace_id,
        &mapping.requested_model,
        end,
        None,
        None,
    )? > 0
    {
        return Ok(false);
    }
    super::history::insert(
        tx,
        &store::NewMapping {
            mapping_id: crate::config::token(),
            namespace_id: mapping.namespace_id.clone(),
            requested_model: mapping.requested_model.clone(),
            classification: store::MappingClass::Proxy,
            valid_from_ms: end,
            valid_to_ms: None,
            evidence: store::MappingEvidence::ManagedSync,
            state: store::MappingState::Verified,
            route_id: mapping.route_id.clone(),
            provider_id: None,
            now_ms: end,
        },
    )?;
    Ok(true)
}

pub(crate) fn finish_sync(
    tx: &Transaction<'_>,
    token: &str,
    observations: &[ConfigObservation],
    at: i64,
) -> Result<bool, store::StoreError> {
    let settings = store::load_import_settings(tx)?;
    let routes = current_routes(observations);
    let prefix = format!("{token}:");
    let prepared = store::load_mappings_in_state(tx, store::MappingState::Prepared)?;
    let mut confirmed = true;
    let mut changed = false;
    for mapping in prepared.into_iter().filter(|mapping| {
        mapping.namespace_id == settings.namespace_id
            && mapping.evidence == store::MappingEvidence::ManagedSync
            && mapping.mapping_id.starts_with(&prefix)
    }) {
        let matched = match mapping.route_id.as_deref() {
            Some(expected) => routes
                .get(mapping.requested_model.as_str())
                .is_some_and(|actual| *actual == Some(expected)),
            None => !routes.contains_key(mapping.requested_model.as_str()),
        };
        confirmed &= resolve_intent(tx, &mapping, matched, at)?;
        changed = true;
    }
    if changed {
        bump_revision(tx)?;
    }
    Ok(confirmed)
}

/// Recover abandoned intents only after the caller has excluded a live sync.
/// Matching positives are resolved in each batch; the final batch ends any
/// remaining unknown gaps without manufacturing proxy proof for absent models.
fn recover_intents(
    tx: &Transaction<'_>,
    ns: &str,
    observations: &[ConfigObservation],
    at: i64,
    final_batch: bool,
) -> Result<bool, store::StoreError> {
    let routes = current_routes(observations);
    let mut changed = false;
    for mapping in store::load_mappings_in_state(tx, store::MappingState::Prepared)?
        .into_iter()
        .filter(|mapping| {
            mapping.namespace_id == ns && mapping.evidence == store::MappingEvidence::ManagedSync
        })
    {
        let present = routes.get(mapping.requested_model.as_str());
        if present.is_none() && !final_batch {
            continue;
        }
        let matched = match mapping.route_id.as_deref() {
            Some(expected) => present.is_some_and(|actual| *actual == Some(expected)),
            None => present.is_none(),
        };
        resolve_intent(tx, &mapping, matched, at)?;
        changed = true;
    }
    Ok(changed)
}

/// Updates one bounded batch from a complete, successfully parsed snapshot.
/// An unreadable profile must never be submitted as an empty snapshot.
pub(super) fn observe(
    tx: &Transaction<'_>,
    observations: &[ConfigObservation],
    at: i64,
    final_batch: bool,
) -> Result<(), store::StoreError> {
    let settings = store::load_import_settings(tx)?;
    if !settings.enabled {
        return Ok(());
    }
    let mut changed = recover_intents(tx, &settings.namespace_id, observations, at, final_batch)?;
    for (model, route) in current_routes(observations) {
        if !valid_id(model) {
            continue;
        }
        if let store::MappingMatch::One(mapping) =
            store::match_mapping(tx, &settings.namespace_id, model, at)?
        {
            if mapping.updated_at_ms > at {
                continue;
            }
            if mapping.classification == store::MappingClass::Proxy
                && mapping.route_id.as_deref() == route
            {
                tx.execute(
                    "UPDATE workbuddy_model_history SET updated_at_ms=?2 WHERE mapping_id=?1",
                    params![mapping.mapping_id, at],
                )?;
                continue;
            }
            close_covering(tx, &settings.namespace_id, model, at)?;
        }
        if store::overlapping_verified(tx, &settings.namespace_id, model, at, None, None)? > 0 {
            continue;
        }
        super::history::clear_uncertainty(tx, &settings.namespace_id, model, at, None, at)?;
        super::history::insert(
            tx,
            &store::NewMapping {
                mapping_id: crate::config::token(),
                namespace_id: settings.namespace_id.clone(),
                requested_model: model.into(),
                classification: store::MappingClass::Proxy,
                valid_from_ms: at,
                valid_to_ms: None,
                evidence: store::MappingEvidence::ObservedConfig,
                state: store::MappingState::Verified,
                route_id: route.map(str::to_owned),
                provider_id: None,
                now_ms: at,
            },
        )?;
        changed = true;
    }
    if final_batch {
        changed |= close_unseen(tx, &settings.namespace_id, at)?;
    }
    if changed {
        bump_revision(tx)?;
    }
    Ok(())
}

/// A changed or removed profile is unknown since the previous successful
/// observation, rather than remaining trusted until the new observation.
fn close_unseen(tx: &Transaction<'_>, ns: &str, at: i64) -> Result<bool, store::StoreError> {
    let mut statement=tx.prepare("SELECT mapping_id,MAX(valid_from_ms+1,updated_at_ms+1) FROM workbuddy_model_history
        WHERE namespace_id=?1 AND evidence IN ('observed_config','managed_sync') AND state='verified'
        AND valid_to_ms IS NULL AND valid_from_ms<?2 AND updated_at_ms<?2")?;
    let rows = statement
        .query_map(params![ns, at], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for (id, boundary) in &rows {
        retire_unknown(tx, id, *boundary, at)?;
    }
    Ok(!rows.is_empty())
}

#[cfg(test)]
mod tests;
