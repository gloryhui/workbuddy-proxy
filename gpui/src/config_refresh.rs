//! Read-only reconciliation of live external changes and rejected deletes.
//! Never initialize a configuration or overwrite an unsaved draft.
//!
//! 这些函数不都在同一线程：`backend::Backend` 的 `settings-control` worker
//! 会用 `existing_config` 读取 poll 或失败删除后的现有文件；
//! `matches_core` 与 `refresh_drafts` 则由 `ui::Proxy` 在 GPUI UI 线程调用。
//! worker 对磁盘读取设置大小上限；UI 侧核对内存配置与 Core 快照后才合并草稿。
//! 本模块不会发送保存请求或直接重绘；`Proxy` 完成合并后调用
//! `cx.notify()`。
use crate::{
    config::Config,
    state::{Draft, Drafts},
};
use serde_json::Value;
use std::{fs::File, io::Read, path::Path};

const MAX_CONFIG_BYTES: u64 = 2 * 1024 * 1024;

/// 只读取一个已经存在且结构合法的配置快照。
///
/// `None` 是一个失败边界，而不是“空配置”：文件不存在、超过 2 MiB、旧版
/// 结构无法按当前 `Config` 解码、校验失败或 local key 为空时，调用方都不能
/// 用默认值覆盖当前界面。函数不创建、不迁移、不重写文件。
pub(crate) fn existing_config(dir: &Path) -> Option<Config> {
    let file = File::open(dir.join("config.json")).ok()?;
    let metadata = file.metadata().ok()?;
    if !metadata.is_file() || metadata.len() > MAX_CONFIG_BYTES {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(MAX_CONFIG_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() as u64 > MAX_CONFIG_BYTES {
        return None;
    }
    let value: Value = serde_json::from_slice(&bytes).ok()?;
    // A running Core has already migrated its configuration. Do not decode a
    // legacy object through serde defaults or perform a migration on an error.
    value.get("providers")?.as_array()?;
    let cfg: Config = serde_json::from_value(value).ok()?;
    cfg.validate().ok()?;
    (!cfg.local_key.is_empty()).then_some(cfg)
}

/// 判断 `after` 是否确实是同一个 Core 实例刚返回的可信配置。
///
/// 除配置自身有效和身份 key 一致外，还要求 snapshot 的供应商/模型计数与
/// `after` 相符，且公开地址必须是严格的 loopback `http`、与配置端口一致、
/// 路径为 `/v1` 或 `/v1/`，没有用户信息、查询串或片段。任何一项不满足都
/// 返回 false，防止把陌生或错配的轮询结果合并进编辑界面。
pub(crate) fn matches_core(before: &Config, after: &Config, snapshot: &Value) -> bool {
    let Some(address) = snapshot["url"]
        .as_str()
        .and_then(|s| url::Url::parse(s).ok())
    else {
        return false;
    };
    !before.local_key.is_empty()
        && before.local_key == after.local_key
        && after.validate().is_ok()
        && snapshot["providers"].as_u64() == Some(after.providers.len() as u64)
        && snapshot["models"].as_u64()
            == Some(after.providers.iter().map(|p| p.models.len() as u64).sum())
        && address.scheme() == "http"
        && matches!(
            address.host_str(),
            Some("127.0.0.1" | "localhost" | "[::1]")
        )
        && address.username().is_empty()
        && address.password().is_none()
        && address.port_or_known_default() == Some(after.port)
        && matches!(address.path(), "/v1" | "/v1/")
        && address.query().is_none()
        && address.fragment().is_none()
}

/// Retain the user's dirty/editing drafts and local pending item. Clean saved
/// rows follow the new configuration, and an absent selection moves safely.
/// Returns whether the currently displayed form actually needs to be reloaded.
///
/// 这里用三种事实决定是否保留旧草稿：当前表单是否正在编辑、该草稿是否与
/// 刷新前配置的干净投影不同、以及它是否属于本地创建流程。正在编辑或已有
/// 未保存值的内容不会被外部变化覆盖；其余行更新为 `after`。但 `after` 中已
/// 不存在的供应商会先从 `providers` 中删除，即使该行有未保存输入；只有仍存在
/// 于 `after`（或属于 pending 创建记录）的行才能由“编辑/脏草稿”规则保留。
/// 删除当前选中项时只把选择安全地移到第一个有效供应商。
///
/// `Clone` 用于比较/保存干净的 `Draft` 快照，但它的代价和深拷贝边界由
/// `Draft` 的派生实现及字段类型决定，不能把所有 `.clone()` 都当 Java 深拷贝。
pub(crate) fn refresh_drafts(
    drafts: &mut Drafts,
    before: &Config,
    after: &Config,
    editing: impl Fn(&str) -> bool,
) -> bool {
    let old_selected = drafts.selected.clone();
    let old_form = drafts.providers.get(&old_selected).cloned();
    if drafts
        .pending
        .as_ref()
        .is_some_and(|p| p.created && !after.providers.iter().any(|saved| saved.id == p.id))
    {
        drafts.pending = None;
    }
    drafts.providers.retain(|id, _| {
        after.providers.iter().any(|p| &p.id == id)
            || drafts.pending.as_ref().is_some_and(|p| &p.id == id)
    });
    for provider in &after.providers {
        let keep = drafts.providers.get(&provider.id).is_some_and(|draft| {
            editing(&provider.id)
                || drafts.pending.as_ref().is_some_and(|p| p.id == provider.id)
                || before
                    .providers
                    .iter()
                    .find(|p| p.id == provider.id)
                    .is_none_or(|old| draft != &Draft::from_provider(old))
        });
        if !keep {
            drafts
                .providers
                .insert(provider.id.clone(), Draft::from_provider(provider));
        }
    }
    if !drafts.providers.contains_key(&drafts.selected) {
        drafts.selected = after
            .providers
            .first()
            .map(|p| p.id.clone())
            .unwrap_or_default();
    }
    old_selected != drafts.selected || old_form.as_ref() != drafts.providers.get(&drafts.selected)
}

#[cfg(test)]
mod tests {
    use super::{existing_config, matches_core, refresh_drafts};
    use crate::{
        config::{Config, Model, Provider},
        state::{Draft, Drafts},
    };
    use serde_json::json;

    fn config(ids: &[&str]) -> Config {
        Config {
            local_key: "synthetic-local-key".into(),
            port: 19245,
            providers: ids
                .iter()
                .map(|id| Provider {
                    id: (*id).into(),
                    name: (*id).into(),
                    api_key: "synthetic-upstream-key".into(),
                    upstream: "http://127.0.0.1:1/v1".into(),
                    models: vec![Model {
                        id: format!("route-{id}"),
                        model: format!("model-{id}"),
                        ..Default::default()
                    }],
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        }
    }
    fn snapshot(cfg: &Config) -> serde_json::Value {
        json!({"url":format!("http://127.0.0.1:{}/v1",cfg.port),"providers":cfg.providers.len(),"models":cfg.providers.iter().map(|p|p.models.len()).sum::<usize>()})
    }

    #[test]
    fn missing_invalid_and_legacy_files_are_never_initialized_or_rewritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        assert!(existing_config(dir.path()).is_none());
        assert!(!path.exists());
        for bytes in [
            b"invalid".as_slice(),
            b"{}",
            b"{\"api_key\":\"synthetic\",\"model\":\"legacy\"}",
        ] {
            std::fs::write(&path, bytes).unwrap();
            assert!(existing_config(dir.path()).is_none());
            assert_eq!(std::fs::read(&path).unwrap(), bytes);
        }
        let cfg = config(&["prov-a"]);
        let bytes = serde_json::to_vec(&cfg).unwrap();
        std::fs::write(&path, &bytes).unwrap();
        assert!(existing_config(dir.path()).is_some());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }

    #[test]
    fn only_matching_live_counts_endpoint_and_identity_allow_recovery() {
        let before = config(&["prov-a", "prov-b"]);
        let after = config(&["prov-b"]);
        assert!(matches_core(&before, &after, &snapshot(&after)));
        assert!(!matches_core(&before, &after, &json!({})));
        assert!(!matches_core(&before, &after, &snapshot(&before)));
        let mut changed_key = after.clone();
        changed_key.local_key = "different-synthetic-key".into();
        assert!(!matches_core(
            &before,
            &changed_key,
            &snapshot(&changed_key)
        ));
        for url in [
            "http://example.test:19245/v1",
            "http://127.0.0.1:12345/v1",
            "http://user@127.0.0.1:19245/v1",
        ] {
            let mut s = snapshot(&after);
            s["url"] = json!(url);
            assert!(!matches_core(&before, &after, &s));
        }
        let empty = config(&[]);
        assert!(matches_core(&before, &empty, &snapshot(&empty)));
    }

    #[test]
    fn surviving_dirty_draft_is_not_replaced_when_deleted_selection_moves() {
        let dir = tempfile::tempdir().unwrap();
        let before = config(&["prov-a", "prov-b"]);
        let after = config(&["prov-b"]);
        let mut drafts = Drafts::from_config(dir.path(), &before);
        let dirty = drafts.providers.get_mut("prov-b").unwrap();
        dirty.name = "Keep unsaved name".into();
        dirty.key = "synthetic-draft-key".into();
        dirty.models.push(json!({"id":"draft-route","model":"extra-model","reasoning_effort":"high","capabilities":{"context":8192}}));
        let original = dirty.clone();
        assert!(refresh_drafts(&mut drafts, &before, &after, |_| false));
        assert_eq!(drafts.selected, "prov-b");
        assert!(drafts.providers["prov-b"] == original);
        assert!(!drafts.providers.contains_key("prov-a"));
    }

    #[test]
    fn clean_rows_follow_external_changes_but_active_editor_keeps_its_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let before = config(&["prov-b"]);
        let mut after = before.clone();
        after.providers[0].name = "Externally renamed".into();
        let mut clean = Drafts::from_config(dir.path(), &before);
        assert!(refresh_drafts(&mut clean, &before, &after, |_| false));
        assert_eq!(clean.providers["prov-b"].name, "Externally renamed");
        let mut editing = Drafts::from_config(dir.path(), &before);
        let original = editing.providers["prov-b"].clone();
        assert!(!refresh_drafts(&mut editing, &before, &after, |id| id == "prov-b"));
        assert!(editing.providers["prov-b"] == original);
    }

    #[test]
    fn external_save64_refreshes_clean_selection_without_replacing_another_dirty_draft() {
        let dir = tempfile::tempdir().unwrap();
        let before = config(&["prov-a", "prov-b"]);
        let mut after = before.clone();
        after.providers[0].models.extend((1..64).map(|i| Model {
            id: format!("route-{i}"),
            alias: format!("prov-a/model-{i}"),
            name: format!("model-{i}"),
            model: format!("model-{i}"),
            ..Default::default()
        }));
        let mut drafts = Drafts::from_config(dir.path(), &before);
        drafts.selected = "prov-a".into();
        drafts.providers.get_mut("prov-b").unwrap().name = "Unsaved local name".into();
        let dirty = drafts.providers["prov-b"].clone();
        assert!(matches_core(&before, &after, &snapshot(&after)));
        assert!(refresh_drafts(&mut drafts, &before, &after, |_| false));
        assert_eq!(drafts.selected, "prov-a");
        assert_eq!(drafts.providers["prov-a"].models.len(), 64);
        assert!(drafts.providers["prov-b"] == dirty);
        // Repeated live polls must leave the local draft intact, and the
        // already-refreshed form must not be reloaded on every idle poll.
        assert!(!refresh_drafts(&mut drafts, &after, &after, |_| false));
        assert!(drafts.providers["prov-b"] == dirty);
    }

    #[test]
    fn deleting_the_last_saved_provider_enters_empty_state() {
        let dir = tempfile::tempdir().unwrap();
        let before = config(&["prov-a"]);
        let mut drafts = Drafts::from_config(dir.path(), &before);
        assert!(refresh_drafts(&mut drafts, &before, &config(&[]), |_| {
            false
        }));
        assert!(drafts.selected.is_empty() && drafts.providers.is_empty());
    }

    #[test]
    fn local_uncreated_draft_survives_but_deleted_created_pending_is_removed() {
        let dir = tempfile::tempdir().unwrap();
        let before = config(&["prov-a"]);
        let after = config(&[]);
        let mut drafts = Drafts::from_config(dir.path(), &before);
        let local = Draft {
            name: "Unsaved local provider".into(),
            ..Default::default()
        };
        drafts.providers.insert("local-new".into(), local.clone());
        drafts.pending = Some(crate::provider_create::Pending {
            id: "local-new".into(),
            return_to: "prov-a".into(),
            source_id: None,
            created: false,
            uncertain: false,
        });
        drafts.selected = "local-new".into();
        assert!(!refresh_drafts(&mut drafts, &before, &after, |_| false));
        assert!(drafts.providers["local-new"] == local);
        assert!(drafts.pending.is_some());
        drafts.pending.as_mut().unwrap().created = true;
        assert!(refresh_drafts(&mut drafts, &before, &after, |_| false));
        assert!(drafts.providers.is_empty() && drafts.selected.is_empty());
        assert!(drafts.pending.is_none());
    }
}
