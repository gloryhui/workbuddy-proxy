//! Unsaved new/copy forms live in the existing private draft store.
use super::*;

#[derive(Clone, Copy)]
pub(super) enum CreationKind {
    New,
    Copy,
}

impl Proxy {
    pub(super) fn pending_selected(&self) -> bool {
        self.drafts
            .pending
            .as_ref()
            .is_some_and(|p| p.id == self.drafts.selected)
    }

    pub(super) fn begin_provider_draft(
        &mut self,
        copy: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.busy {
            return;
        }
        if self.drafts.pending.is_none() && self.cfg.providers.len() >= config::MAX_PROVIDERS {
            self.error = true;
            self.status = self.t("最多支持64个供应商", "At most 64 providers").into();
            cx.notify();
            return;
        }
        if copy && self.pending_selected() {
            if self.drafts.pending.as_ref().is_some_and(|p| !p.created)
                && self.cfg.providers.len() + 1 >= config::MAX_PROVIDERS
            {
                self.error = true;
                self.status = self.t("最多支持64个供应商", "At most 64 providers").into();
                cx.notify();
                return;
            }
            let steps = self.save_steps(cx);
            self.send("copy_prepare", steps, cx);
            return;
        }
        if copy && self.drafts.pending.is_none() {
            let steps = self.dirty_steps(cx);
            if !steps.is_empty() {
                // Preserve the existing Copy action's source-save boundary;
                // only creation of the independent copy is deferred.
                self.send("copy_prepare", steps, cx);
                return;
            }
        }
        self.capture(cx);
        if let Some(pending) = &self.drafts.pending {
            self.drafts.selected = pending.id.clone();
        } else {
            if self.cfg.providers.len() >= config::MAX_PROVIDERS {
                self.error = true;
                self.status = self.t("最多支持64个供应商", "At most 64 providers").into();
                cx.notify();
                return;
            }
            let previous = self.selected();
            let source = copy
                .then(|| self.cfg.providers.iter().find(|p| p.id == previous))
                .flatten();
            if copy && source.is_none() {
                return;
            }
            let mut draft = if copy {
                self.drafts
                    .providers
                    .get(&previous)
                    .cloned()
                    .unwrap_or_default()
            } else {
                Draft {
                    effort: "keep".into(),
                    ..Default::default()
                }
            };
            if let Some(source) = source {
                let name = if draft.name.trim().is_empty() {
                    &source.name
                } else {
                    &draft.name
                };
                let root = format!("{name} {}", self.t("副本", "copy"));
                let mut candidate = root.clone();
                let mut suffix = 1;
                while self.cfg.providers.iter().any(|p| p.name == candidate)
                    || self.drafts.providers.values().any(|d| d.name == candidate)
                {
                    candidate = format!("{root} {suffix}");
                    suffix += 1;
                }
                draft.name = candidate;
            }
            let id = format!("draft-{}", &config::token()[..12]);
            self.drafts.pending = Some(crate::provider_create::Pending {
                id: id.clone(),
                return_to: previous,
                source_id: source.map(|p| p.id.clone()),
                created: false,
                uncertain: false,
            });
            self.drafts.providers.insert(id.clone(), draft);
            self.drafts.selected = id;
        }
        self.switch_page(Page::Providers, window, cx);
        self.load_form(window, cx);
        self.provider_scroll.set_offset(point(px(0.), px(0.)));
        self.name.read(cx).focus_handle(cx).focus(window, cx);
        let _ = self.drafts.persist(&self.dir);
        cx.notify();
    }

    pub(super) fn cancel_provider_draft(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy || !self.pending_selected() {
            return;
        }
        let Some(pending) = self.drafts.pending.take() else {
            return;
        };
        self.connections.discard(&pending.id);
        if let Some(saved) = self.cfg.providers.iter().find(|p| p.id == pending.id) {
            self.drafts
                .providers
                .insert(pending.id, Draft::from_provider(saved));
        } else {
            self.drafts.providers.remove(&pending.id);
        }
        self.drafts.selected = pending.return_to;
        self.load_form(window, cx);
        let _ = self.drafts.persist(&self.dir);
        cx.notify();
    }

    pub(super) fn bind_created_draft(&mut self, outcome: crate::provider_create::Outcome) -> bool {
        let Some(pending) = self
            .drafts
            .pending
            .as_mut()
            .filter(|p| p.id == outcome.local_id)
        else {
            return false;
        };
        pending.uncertain = outcome.uncertain;
        if let Some(id) = outcome.provider_id {
            self.drafts.providers.remove(&pending.id);
            if self.drafts.selected == pending.id {
                self.models = outcome.draft.models.clone();
                self.drafts.selected = id.clone();
            }
            self.drafts.providers.insert(id.clone(), outcome.draft);
            if outcome.saved {
                self.saved_creations.insert(
                    id.clone(),
                    if pending.source_id.is_some() {
                        CreationKind::Copy
                    } else {
                        CreationKind::New
                    },
                );
            }
            pending.id = id;
            pending.created = true;
            if outcome.saved {
                self.drafts.pending = None;
            }
        }
        outcome.saved
    }

    pub(super) fn fetch_provider_models(&mut self, cx: &mut Context<Self>) {
        let mut id = self.selected();
        if self.pending_selected() && !self.cfg.providers.iter().any(|p| p.id == id) {
            // discover requires a saved context, but every credential and wire
            // field is explicitly overridden. Never borrow another row's key.
            if self.key.read(cx).value().trim().is_empty() {
                self.error = true;
                self.status = self
                    .t("请填写上游 API Key", "Enter upstream API key")
                    .into();
                cx.notify();
                return;
            }
            let Some(context) = self.cfg.providers.first() else {
                self.send("fetch",vec![json!({"command":"ui_discover_models","provider_id":id,"upstream":self.url.read(cx).value().trim(),"key":self.key.read(cx).value().trim(),"protocol":self.protocol_payload()["protocol"],"protocol_settings":self.protocol_payload()["protocol_settings"]})],cx);
                return;
            };
            id = context.id.clone();
        }
        self.send("fetch", vec![json!({"command":"fetch","provider_id":id,"upstream":self.url.read(cx).value().to_string(),"key":self.key.read(cx).value().to_string(),"protocol":self.protocol_payload()["protocol"],"protocol_settings":self.protocol_payload()["protocol_settings"]})],cx);
    }
}
