use crate::config::{self, Config};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

#[derive(Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Draft {
    pub name: String,
    pub key: String,
    pub upstream: String,
    #[serde(default)]
    pub protocol: crate::upstream_config::UpstreamProtocol,
    #[serde(default)]
    pub protocol_settings: crate::upstream_config::ProtocolSettings,
    pub effort: String,
    pub models: Vec<Value>,
}
impl Draft {
    /// Pure projection for one supplier; unlike `Drafts::from_config`, this
    /// does not fingerprint/read the configuration file. Renderers may compare
    /// the current draft with this saved snapshot without doing frame-time IO.
    pub fn from_provider(provider: &config::Provider) -> Self {
        Self {
            name: if provider.is_draft() { String::new() } else { provider.name.clone() },
            key: provider.api_key.clone(),
            upstream: provider.upstream.clone(),
            protocol: provider.protocol,
            protocol_settings: provider.protocol_settings.clone(),
            effort: if provider.models.iter().all(|model| provider.models.first().is_some_and(|first| first.reasoning_effort == model.reasoning_effort)) {
                provider.models.first().map(|model| model.reasoning_effort.clone()).unwrap_or_else(|| "keep".into())
            } else { "keep".into() },
            models: provider.models.iter().map(|model| json!({
                "id":model.id, "name":model.name, "model":model.model,
                "reasoning_effort":model.reasoning_effort, "capabilities":model.capabilities,
            })).collect(),
        }
    }

    pub fn payload(&self, id: &str) -> Value {
        json!({"command":"save","provider_id":id,"name":self.name,"key":self.key,"upstream":self.upstream,"protocol":self.protocol,"protocol_settings":self.protocol_settings,"reasoning_effort":self.effort,"models":self.models})
    }
}
#[derive(Default, Serialize, Deserialize)]
pub struct Drafts {
    pub base: String,
    pub selected: String,
    pub providers: BTreeMap<String, Draft>,
}
impl Drafts {
    fn fingerprint(dir: &Path) -> String {
        format!(
            "{:x}",
            Sha256::digest(std::fs::read(dir.join("config.json")).unwrap_or_default())
        )
    }
    pub fn load(dir: &Path, cfg: &Config) -> Self {
        let stored = std::fs::read(dir.join("gpui-drafts.json"))
            .ok()
            .filter(|s| s.len() <= 4 * 1024 * 1024)
            .and_then(|s| {
                let mut value: Value = serde_json::from_slice(&s).ok()?;
                if let Some(providers) = value.get_mut("providers").and_then(Value::as_object_mut) {
                    for provider in &cfg.providers {
                        if let Some(draft) = providers
                            .get_mut(&provider.id)
                            .and_then(Value::as_object_mut)
                        {
                            draft
                                .entry("protocol")
                                .or_insert_with(|| json!(provider.protocol));
                            draft
                                .entry("protocol_settings")
                                .or_insert_with(|| json!(provider.protocol_settings));
                        }
                    }
                }
                serde_json::from_value::<Self>(value).ok()
            })
            .filter(|s| s.base == Self::fingerprint(dir));
        stored.unwrap_or_else(|| Self::from_config(dir, cfg))
    }
    pub fn from_config(dir: &Path, cfg: &Config) -> Self {
        Self {
            base: Self::fingerprint(dir),
            selected: cfg
                .providers
                .first()
                .map(|provider| provider.id.clone())
                .unwrap_or_default(),
            providers: cfg
                .providers
                .iter()
                .map(|provider| (provider.id.clone(), Draft::from_provider(provider)))
                .collect(),
        }
    }
    pub fn persist(&mut self, dir: &Path) -> Result<(), String> {
        self.base = Self::fingerprint(dir);
        config::write_json(&dir.join("gpui-drafts.json"), self)
    }
}
#[derive(Clone, Default, PartialEq)]
pub struct Selection {
    pub rows: BTreeSet<usize>,
    pub anchor: usize,
}
impl Selection {
    pub fn click(&mut self, row: usize, extend: bool, toggle: bool) {
        if extend {
            if !toggle {
                self.rows.clear();
            }
            self.rows
                .extend(self.anchor.min(row)..=self.anchor.max(row));
        } else if toggle {
            if !self.rows.remove(&row) {
                self.rows.insert(row);
            }
            self.anchor = row;
        } else {
            self.rows.clear();
            self.rows.insert(row);
            self.anchor = row;
        }
    }
    pub fn remove(&mut self, models: &mut Vec<Value>) {
        for row in self.rows.iter().rev() {
            if *row < models.len() {
                models.remove(*row);
            }
        }
        self.rows.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_private_draft_inherits_current_protocol_without_losing_typed_edits() {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = Config::default();
        let p = &mut cfg.providers[0];
        p.protocol = crate::upstream_config::UpstreamProtocol::AnthropicMessages;
        p.upstream = "https://example.test/v1".into();
        p.api_key = "fixture".into();
        let id = p.id.clone();
        config::write_json(&dir.path().join("config.json"), &cfg).unwrap();
        let drafts = Drafts::from_config(dir.path(), &cfg);
        let mut value = serde_json::to_value(drafts).unwrap();
        let draft = value["providers"][&id].as_object_mut().unwrap();
        draft.remove("protocol");
        draft.remove("protocol_settings");
        draft.insert("name".into(), json!("unsaved provider name"));
        config::write_json(&dir.path().join("gpui-drafts.json"), &value).unwrap();
        let loaded = Drafts::load(dir.path(), &cfg);
        let payload = loaded.providers[&id].payload(&id);
        assert_eq!(payload["protocol"], "anthropic_messages");
        assert_eq!(payload["name"], "unsaved provider name");
    }
    #[test]
    fn model_capabilities_survive_private_drafts_and_save_payloads() {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = Config::default();
        cfg.providers[0].set_model_rows(&[json!({
            "model":"custom-vision", "capabilities":{"supportsImages":true,"supportsToolCall":false}
        })], "default").unwrap();
        config::write_json(&dir.path().join("config.json"), &cfg).unwrap();
        let mut drafts = Drafts::from_config(dir.path(), &cfg);
        drafts.persist(dir.path()).unwrap();
        let restored = Drafts::load(dir.path(), &cfg);
        let provider = &cfg.providers[0].id;
        let payload = restored.providers[provider].payload(provider);
        assert_eq!(payload["models"][0]["capabilities"]["supportsImages"], true);
        assert_eq!(
            payload["models"][0]["capabilities"]["supportsToolCall"],
            false
        );
    }
    #[test]
    fn modifier_selection_and_batch_delete_keep_unselected_models() {
        let mut s = Selection::default();
        s.click(1, false, false);
        s.click(3, true, false);
        s.click(2, false, true);
        s.click(5, false, true);
        assert_eq!(s.rows, BTreeSet::from([1, 3, 5]));
        let mut rows = (0..6).map(|i| json!({"model":i})).collect();
        s.remove(&mut rows);
        assert_eq!(
            rows,
            vec![json!({"model":0}), json!({"model":2}), json!({"model":4})]
        );
    }
    #[test]
    fn drafts_survive_reopen_without_overwriting_saved_config_and_reject_stale_state() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = Config::load(dir.path()).unwrap();
        let old = std::fs::read(dir.path().join("config.json")).unwrap();
        let mut drafts = Drafts::from_config(dir.path(), &cfg);
        drafts.providers.values_mut().next().unwrap().name = "unsaved".into();
        drafts.persist(dir.path()).unwrap();
        assert_eq!(std::fs::read(dir.path().join("config.json")).unwrap(), old);
        assert_eq!(
            Drafts::load(dir.path(), &cfg)
                .providers
                .values()
                .next()
                .unwrap()
                .name,
            "unsaved"
        );
        let mut updated = cfg.clone();
        updated.language = "en".into();
        config::write_json(&dir.path().join("config.json"), &updated).unwrap();
        assert_ne!(
            Drafts::load(dir.path(), &updated)
                .providers
                .values()
                .next()
                .unwrap()
                .name,
            "unsaved"
        );
    }
}
