//! Read-only recovery after a rejected delete. Never initialize a configuration.
use crate::{
    config::Config,
    state::{Draft, Drafts},
};
use serde_json::Value;
use std::{fs::File, io::Read, path::Path};

const MAX_CONFIG_BYTES: u64 = 2 * 1024 * 1024;

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
