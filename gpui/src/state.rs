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
    pub effort: String,
    pub models: Vec<Value>,
}
impl Draft {
    pub fn payload(&self, id: &str) -> Value {
        json!({"command":"save","provider_id":id,"name":self.name,"key":self.key,"upstream":self.upstream,"reasoning_effort":self.effort,"models":self.models})
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
            .and_then(|s| serde_json::from_slice::<Self>(&s).ok())
            .filter(|s| s.base == Self::fingerprint(dir));
        stored.unwrap_or_else(|| Self::from_config(dir, cfg))
    }
    pub fn from_config(dir: &Path, cfg: &Config) -> Self {
        Self{base:Self::fingerprint(dir),selected:cfg.providers.first().map(|p|p.id.clone()).unwrap_or_default(),providers:cfg.providers.iter().map(|p|(p.id.clone(),Draft{name:if p.is_draft(){String::new()}else{p.name.clone()},key:p.api_key.clone(),upstream:p.upstream.clone(),effort:if p.models.iter().all(|m|p.models.first().is_some_and(|a|a.reasoning_effort==m.reasoning_effort)){p.models.first().map(|m|m.reasoning_effort.clone()).unwrap_or("keep".into())}else{"keep".into()},models:p.models.iter().map(|m|json!({"id":m.id,"name":m.name,"model":m.model,"reasoning_effort":m.reasoning_effort})).collect()})).collect()}
    }
    pub fn persist(&mut self, dir: &Path) -> Result<(), String> {
        self.base = Self::fingerprint(dir);
        config::write_json(&dir.join("gpui-drafts.json"), self)
    }
}
#[derive(Default)]
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
