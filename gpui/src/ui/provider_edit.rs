//! In-place profile editing uses local entry snapshots, never compensating API writes.
use super::*;
use std::collections::BTreeMap;

struct Entry {
    draft: Draft,
    selection: Selection,
}

#[derive(Default)]
pub(super) struct ProviderEdits(BTreeMap<String, Entry>);

impl ProviderEdits {
    pub(super) fn contains(&self, id: &str) -> bool {
        self.0.contains_key(id)
    }

    fn begin(&mut self, id: &str, draft: &Draft, selection: &Selection) {
        self.0.entry(id.to_owned()).or_insert_with(|| Entry {
            draft: draft.clone(),
            selection: selection.clone(),
        });
    }

    fn cancel(&mut self, id: &str, drafts: &mut Drafts) -> Option<Selection> {
        let entry = self.0.remove(id)?;
        // A configuration refresh may have removed this supplier. Cancellation
        // must never resurrect it, or touch the currently selected other row.
        *drafts.providers.get_mut(id)? = entry.draft;
        Some(entry.selection)
    }

    pub(super) fn reconcile(&mut self, cfg: &Config, committed: Option<&str>) {
        self.0.retain(|id, _| {
            Some(id.as_str()) != committed && cfg.providers.iter().any(|p| p.id == *id)
        });
    }
}

impl Proxy {
    pub(super) fn toggle_provider_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        if self.pending_selected() {
            self.cancel_provider_draft(window, cx);
            return;
        }
        let id = self.selected();
        if self.provider_edits.contains(&id) {
            if let Some(selection) = self.provider_edits.cancel(&id, &mut self.drafts) {
                self.load_form(window, cx);
                self.selection = selection;
            }
        } else {
            self.capture(cx);
            if let Some(draft) = self.drafts.providers.get(&id) {
                self.provider_edits.begin(&id, draft, &self.selection);
            }
        }
        self.provider_scroll.set_offset(point(px(0.), px(0.)));
        self.name.read(cx).focus_handle(cx).focus(window, cx);
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::ProviderEdits;
    use crate::{
        config::{self, Config},
        state::{Draft, Drafts, Selection},
    };
    use serde_json::json;

    #[test]
    fn cancel_restores_entry_draft_including_preexisting_unsaved_models_and_protocol() {
        let mut drafts = Drafts::default();
        let entry = Draft {
            name: "Already unsaved".into(),
            key: "synthetic-entry-key".into(),
            upstream: "https://entry.example.test/v1".into(),
            protocol: crate::upstream_config::UpstreamProtocol::AnthropicMessages,
            models: vec![
                json!({"id":"entry-model","model":"entry","capabilities":{"context":8192}}),
            ],
            effort: "high".into(),
            ..Draft::default()
        };
        drafts.providers.insert("a".into(), entry.clone());
        drafts.providers.insert("b".into(), Draft::default());
        let mut selection = Selection::default();
        selection.click(0, false, false);
        let mut edits = ProviderEdits::default();
        edits.begin("a", &entry, &selection);
        drafts.providers.get_mut("a").unwrap().models.clear();
        drafts.providers.get_mut("a").unwrap().key = "synthetic-later-key".into();
        // Re-entering the same session cannot replace its original baseline.
        edits.begin("a", &drafts.providers["a"], &Selection::default());
        drafts.selected = "b".into();
        drafts.providers.get_mut("b").unwrap().name = "Keep B draft".into();
        let restored = edits.cancel("a", &mut drafts).unwrap();
        assert!(drafts.providers["a"] == entry);
        assert!(restored == selection);
        assert_eq!(drafts.selected, "b");
        assert_eq!(drafts.providers["b"].name, "Keep B draft");
        assert!(!edits.contains("a"));
    }

    #[test]
    fn commit_or_delete_only_retires_the_affected_session_and_cancel_cannot_resurrect() {
        let mut edits = ProviderEdits::default();
        let mut drafts = Drafts::default();
        for id in ["a", "b", "deleted"] {
            drafts.providers.insert(id.into(), Draft::default());
            edits.begin(id, &Draft::default(), &Selection::default());
        }
        let cfg = Config {
            providers: ["a", "b"]
                .into_iter()
                .map(|id| config::Provider {
                    id: id.into(),
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        };
        edits.reconcile(&cfg, Some("a"));
        assert!(!edits.contains("a"));
        assert!(edits.contains("b"));
        assert!(!edits.contains("deleted"));
        drafts.providers.remove("b");
        assert!(edits.cancel("b", &mut drafts).is_none());
        assert!(!drafts.providers.contains_key("b"));
    }
}
