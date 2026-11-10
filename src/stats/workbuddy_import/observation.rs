//! Non-sensitive configuration evidence and durable synchronization intents.
//! All mutations borrow the sole statistics writer's transaction. Current
//! configuration proves a future interval; it never authorizes older history.
//!
//! 中文导览：本模块记录“代理配置在何时如何管理过某个模型”，供历史维护和归属协调；
//! 它不是普通 WorkBuddy 行导入的当前分类器。`store::attribute_candidate` 目前不读这些
//! interval。调用者把 `models.json`/代理配置经 classify 投影成不含密钥和 URL 的
//! `ConfigObservation`，再通过 stats writer 提交到这里。
//!
//! 配置同步有两个时点：`prepare_sync` 在外部文件被改写前持久化 expected intent，
//! `finish_sync` 在文件已改完后读取实际快照并确认。两者之间故意存在“不确定窗口”，
//! 若进程崩溃，restart 会按真实配置完成或保守地记为 conflict/unknown。任何函数都不应
//! 把“准备写配置”解释成“已经证明配置生效”。
use super::{classify::ConfigObservation, store};
use rusqlite::{Transaction, params};
use std::collections::HashMap;

#[derive(Clone, Debug)]
pub(crate) struct ManagedModel {
    pub requested_model: String,
    /// None records a removal intent, never a guessed destination.
    ///
    /// `Some(route_id)` 是准备写入/保留的 route；`None` 是明确的移除意图。
    /// `None` 不表示“还没决定”，也不能在 `finish_sync` 时随便匹配任意现存 route。
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
///
/// 服务关停、导入暂停或配置暂时不可观察时调用。它只结束仍然开放的 verified
/// 区间，不洗掉过去已经成立的区间和事实；恢复后要有新的观察证据才能继续向后延展。
/// 这里处理的是历史证据的新鲜度，不会删除已经落到 `usage_requests` 的旧统计。
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
///
/// 生产调用方是 `Bridge::sync_workbuddy_with_history`：它先等待 stats writer 成功
/// 持久化本函数产生的 `Prepared` intent 并返回 token，之后才恢复或改写 WorkBuddy 模型
/// 文件。失败若发生在 prepare/preview 阶段，调用方不会触碰 `models.json` 和代理配置。
///
/// 本函数会关闭各模型截至 `at` 的旧开放区间，再为计划中的每个模型写入相同起点、
/// `ManagedSync` evidence、`Prepared` state 和 expected route。`Prepared` 还不能作为
/// 历史分类证据；它只是“外部写入前已经留下计划”的 durable journal。返回值 `None`
/// 表示没有已建立的 namespace，不启用历史日志；`Some(token)` 是后续 finish/recovery
/// 找回这组 intent 的批标识。导入开关关闭但 namespace 已存在时仍继续记录，避免暂停期
/// 被日后回填误当成旧的 direct 规则。
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
///
/// `matched` 只由调用方用“prepare 时 expected route”和“finish 时 actual snapshot”
/// 比较得到。成功匹配时，`[prepare_at, finish_at)` 仍是没有连续观测证明的 gap；若两个
/// 时间相同，代码强制至少留出 1ms unknown，避免零宽成功后把同一毫秒倒填成 Proxy。
/// finish 时间早于 prepare 起点说明时钟回退，无法证明先后关系，mapping 直接 conflict。
/// 只有 gap 结束之后的未来区间才插入新的 verified mapping，绝不回写更早历史。
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
    // finish 面对的是外部 profile 已改写后的完整 observation 列表，而不是直接信任
    // prepare 的意图。`current_routes` 会先压制同一个 model 出现在多个 route 的歧义；
    // 随后只处理 evidence=managed_sync、state=prepared 且 token 前缀匹配的 intent。
    // `confirmed` 是整组 intent 的逻辑与：任一 route 不匹配、移除意图下模型仍在、或
    // 时间无法证明，调用方得到 false；已确认的未来 mapping 仍可在同次调用中建立。
    // 这相当于 Java 服务里的保存操作已完成一部分持久化，但业务确认结果是不确定的。
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
///
/// 启动观察路径先排除仍有进程内 live sync 的情况，再由 `observe` 调用这里处理上次
/// 崩溃遗留的 prepared intent。普通批次会处理当前快照中出现的模型：匹配则确认，route
/// 不符或歧义则保守地形成 unknown；完全未出现的模型留给最终批次。只有
/// `final_batch=true` 才结束这些缺失模型的不确定区间。它不会仅凭“现在没看见”制造
/// proxy 证据，也不会假定缺失必然变成 direct。
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
///
/// caller 必须以一次完整且成功解析的配置快照调用；读取/解析失败不能传 `&[]` 冒充
/// “配置为空”，否则会把所有现存规则误判为消失。函数先按 `current_routes` 汇总同一模型
/// 的 route，不同有效观察给出不同 route 时退化为 `None`，不任意挑选。随后：
/// - 关闭与新观察不相容的旧开放区间，并清除随后未来区间的 uncertainty；
/// - 为当前模型插入从 `at` 开始的 verified proxy interval；
/// - final batch 额外对本次未出现的开放区间执行 `close_unseen`。
///
/// 所有写入仍借用调用者 transaction；任一步返回错误时，是否整体回滚取决于 writer
/// 外层事务边界。这里的 `at` 是本次成功观察时间，观察只能证明此后的未来，不能补写更早
/// 的调用归属。
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
///
/// 候选 bound 取 `MAX(valid_from_ms + 1, updated_at_ms + 1)`：旧规则至少覆盖到上次已知
/// 时间，从下一条边界开始记 unknown。这样新观察不会把过去纯凭猜测改写成 direct/proxy，
/// 也不会让一条 stale open interval 无期限延续。`updated_at_ms < at` 是筛选条件，避免把
/// 本次批次刚刷新过的 mapping 当成 absent 再关闭。函数只询问历史 mapping 与观察、管理
/// 同步状态；当前普通导入的 `attribute_candidate` 仍不消费这些区间。
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
