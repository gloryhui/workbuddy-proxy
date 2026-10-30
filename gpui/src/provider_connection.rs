//! Window-local receipts for explicit tests, never a live upstream health poll.
use crate::{
    config::{Config, Provider},
    diagnostic::TestReport,
    state::Draft,
    upstream_config::{ProtocolSettings, UpstreamProtocol},
};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

/// Captured at the actual Test step, before the existing save/error decoration.
/// `TestReport.provider` is a display name, so identity comes from the request.
pub struct Feedback {
    pub provider_id: String,
    report: Option<TestReport>,
    succeeded: bool,
}
impl Feedback {
    pub fn from_result(provider_id: &str, result: &Result<String, String>) -> Self {
        let text = match result {
            Ok(text) | Err(text) => text,
        };
        Self {
            provider_id: provider_id.into(),
            report: serde_json::from_str::<TestReport>(text)
                .ok()
                .filter(|report| report.kind == "connection_test"),
            succeeded: result.is_ok(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Status {
    #[default]
    Untested,
    Testing,
    Passed,
    Failed,
    Incomplete,
}

// No Debug or Serialize: credentials are compared in memory, never reported.
struct Fields {
    name: String,
    key: String,
    upstream: String,
    protocol: UpstreamProtocol,
    settings: ProtocolSettings,
    effort: String,
}
impl Fields {
    fn new(draft: &Draft) -> Self {
        Self {
            name: draft.name.clone(),
            key: draft.key.clone(),
            upstream: draft.upstream.clone(),
            protocol: draft.protocol,
            settings: draft.protocol_settings.clone(),
            effort: draft.effort.clone(),
        }
    }
    fn matches(&self, live: &LiveFields<'_>) -> bool {
        self.name == live.name
            && self.key == live.key
            && self.upstream == live.upstream
            && self.protocol == live.protocol
            && self.settings == *live.settings
            && self.effort == live.effort
    }
}
pub struct LiveFields<'a> {
    pub name: &'a str,
    pub key: &'a str,
    pub upstream: &'a str,
    pub protocol: UpstreamProtocol,
    pub settings: &'a ProtocolSettings,
    pub effort: &'a str,
}
struct Observed {
    visible: [u8; 32],
    fields: Fields,
}
struct Attempt {
    provider: String,
    effective: Option<[u8; 32]>,
    visible: [u8; 32],
    model_index: usize,
}
struct Receipt {
    visible: [u8; 32],
    saved: Option<[u8; 32]>,
    status: Status,
    model: String,
    http: String,
}

#[derive(Default)]
pub struct Connections {
    observed: BTreeMap<String, Observed>,
    receipts: BTreeMap<String, Receipt>,
    pending: Option<Attempt>,
}

pub fn seed_provider<'a>(
    cfg: &'a Config,
    pending: Option<&crate::provider_create::Pending>,
    id: &str,
) -> Option<&'a Provider> {
    cfg.providers
        .iter()
        .find(|provider| provider.id == id)
        .or_else(|| {
            // A local copy has no saved target yet. Duplicate inherits the source
            // credential/model defaults before applying the edited draft.
            let source = pending
                .filter(|pending| pending.id == id && !pending.created)?
                .source_id
                .as_deref()?;
            cfg.providers.iter().find(|provider| provider.id == source)
        })
}

fn digest(value: &impl serde::Serialize) -> [u8; 32] {
    // Draft/Provider contain only serializable configuration values.
    Sha256::digest(serde_json::to_vec(value).expect("configuration snapshot serializes")).into()
}
fn saved_signature(provider: &Provider) -> [u8; 32] {
    // Core generates route IDs/aliases on save. They are not test inputs.
    digest(&json!({
        "name":provider.name,"key":provider.api_key,"upstream":provider.upstream,
        "protocol":provider.protocol,"settings":provider.protocol_settings,
        "models":provider.models.iter().map(|model| json!({
            "name":model.name,"model":model.model,"effort":model.reasoning_effort,
            "capabilities":model.capabilities,
        })).collect::<Vec<_>>()
    }))
}
fn effective_signature(saved: Option<&Provider>, draft: &Draft) -> Option<[u8; 32]> {
    let mut preview = saved.cloned().unwrap_or_default();
    preview.name = draft.name.trim().into();
    preview.upstream = draft.upstream.trim().into();
    if !draft.key.trim().is_empty() {
        preview.api_key = draft.key.trim().into();
    }
    preview.protocol = draft.protocol;
    preview.protocol_settings = draft.protocol_settings.clone();
    // Reuse the same pure model/effort/default/capability rules as Save.
    // This preview runs on explicit Test only, never in render or idle ticks.
    preview.set_model_rows(&draft.models, &draft.effort).ok()?;
    Some(saved_signature(&preview))
}

impl Connections {
    pub fn begin(&mut self, id: &str, draft: &Draft, saved: Option<&Provider>, index: usize) {
        self.receipts.remove(id);
        self.observed.insert(
            id.into(),
            Observed {
                visible: digest(draft),
                fields: Fields::new(draft),
            },
        );
        self.pending = Some(Attempt {
            provider: id.into(),
            effective: effective_signature(saved, draft),
            visible: digest(draft),
            model_index: index,
        });
    }

    /// Called after draft changes or form loads, not per paint/unchanged tick.
    pub fn observe(&mut self, id: &str, draft: &Draft) {
        if self.observed.contains_key(id) {
            self.observed.insert(
                id.into(),
                Observed {
                    visible: digest(draft),
                    fields: Fields::new(draft),
                },
            );
        }
    }

    pub fn rebind(&mut self, from: &str, to: &str) {
        if let Some(attempt) = self.pending.as_mut().filter(|a| a.provider == from) {
            attempt.provider = to.into();
            if let Some(observed) = self.observed.remove(from) {
                self.observed.insert(to.into(), observed);
            }
        }
    }

    pub fn finish(&mut self, cfg: &Config, feedback: Option<Feedback>) {
        let Some(attempt) = self.pending.take() else {
            return;
        };
        let provider = cfg.providers.iter().find(|p| p.id == attempt.provider);
        let confirmed = provider.filter(|p| Some(saved_signature(p)) == attempt.effective);
        let mut receipt = Receipt {
            visible: confirmed.map_or(attempt.visible, |p| digest(&Draft::from_provider(p))),
            saved: confirmed.map(saved_signature),
            status: Status::Incomplete,
            model: String::new(),
            http: String::new(),
        };
        if let (Some(provider), Some(feedback)) = (confirmed, feedback)
            && feedback.provider_id == attempt.provider
            && let Some(report) = feedback.report
            && let Some(model) = provider.models.get(attempt.model_index)
            && let Ok(connection) = cfg.connection(&model.id)
            && report.provider == provider.name
            && report.name == model.name
            && report.model == model.model
            && report.protocol == provider.protocol.as_str()
            && report.url == connection.endpoint()
            && report.reasoning == model.reasoning_effort
        {
            receipt.status = if feedback.succeeded
                && report.reason.is_empty()
                && report
                    .http
                    .strip_prefix("HTTP ")
                    .and_then(|s| s.parse::<u16>().ok())
                    .is_some_and(|n| (200..300).contains(&n))
            {
                Status::Passed
            } else if !feedback.succeeded && !report.reason.is_empty() {
                Status::Failed
            } else {
                Status::Incomplete
            };
            receipt.model = report.model;
            receipt.http = report.http;
        }
        self.receipts.insert(attempt.provider, receipt);
    }

    pub fn view(&self, id: &str, fields: LiveFields<'_>) -> (Status, &str, &str) {
        let Some(current) = self.observed.get(id).filter(|c| c.fields.matches(&fields)) else {
            return (Status::Untested, "", "");
        };
        if self
            .pending
            .as_ref()
            .is_some_and(|a| a.provider == id && a.visible == current.visible)
        {
            return (Status::Testing, "", "");
        }
        self.receipts
            .get(id)
            .filter(|r| r.visible == current.visible)
            .map_or((Status::Untested, "", ""), |r| {
                (r.status, r.model.as_str(), r.http.as_str())
            })
    }

    pub fn reconcile(&mut self, cfg: &Config, drafts: &BTreeMap<String, Draft>) {
        self.receipts.retain(|id, receipt| {
            drafts.contains_key(id)
                && receipt.saved.is_none_or(|signature| {
                    cfg.providers
                        .iter()
                        .find(|p| p.id == *id)
                        .is_some_and(|p| saved_signature(p) == signature)
                })
        });
        self.observed.retain(|id, _| drafts.contains_key(id));
    }

    /// Returns whether visible receipts changed; unchanged offline polls stay quiet.
    pub fn forget_results(&mut self) -> bool {
        let changed = !self.receipts.is_empty();
        self.receipts.clear();
        self.observed
            .retain(|id, _| self.pending.as_ref().is_some_and(|a| a.provider == *id));
        changed
    }

    pub fn discard(&mut self, id: &str) {
        self.observed.remove(id);
        self.receipts.remove(id);
        if self.pending.as_ref().is_some_and(|a| a.provider == id) {
            self.pending = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Model;

    fn fixture() -> Config {
        let a = Provider {
            id: "a".into(),
            name: "Same display name".into(),
            upstream: "https://synthetic.example.test/v1".into(),
            api_key: "synthetic-key".into(),
            models: vec![Model {
                id: "ma".into(),
                alias: "alias-a".into(),
                name: "Mock".into(),
                model: "mock-model".into(),
                reasoning_effort: "default".into(),
                capabilities: None,
            }],
            ..Provider::default()
        };
        let mut b = a.clone();
        b.id = "b".into();
        b.models[0].id = "mb".into();
        Config {
            providers: vec![a, b],
            ..Config::default()
        }
    }
    fn fields(d: &Draft) -> LiveFields<'_> {
        LiveFields {
            name: &d.name,
            key: &d.key,
            upstream: &d.upstream,
            protocol: d.protocol,
            settings: &d.protocol_settings,
            effort: &d.effort,
        }
    }
    fn feedback(cfg: &Config, id: &str, index: usize, success: bool) -> Feedback {
        let p = cfg.providers.iter().find(|p| p.id == id).unwrap();
        let mut report = TestReport::from_connection(&cfg.connection(&p.models[index].id).unwrap());
        report.http = if success { "HTTP 200" } else { "HTTP 503" }.into();
        report.reason = if success {
            ""
        } else {
            "Synthetic upstream refusal"
        }
        .into();
        report.first_event_ms = success.then_some(0);
        Feedback::from_result(
            id,
            &if success {
                Ok(report.json())
            } else {
                Err(report.json())
            },
        )
    }
    fn passed(cfg: &Config) -> (Connections, Draft) {
        let d = Draft::from_provider(&cfg.providers[0]);
        let mut s = Connections::default();
        s.begin("a", &d, Some(&cfg.providers[0]), 0);
        s.finish(cfg, Some(feedback(cfg, "a", 0, true)));
        (s, d)
    }

    #[test]
    fn only_an_explicit_verified_report_can_become_green() {
        let cfg = fixture();
        let d = Draft::from_provider(&cfg.providers[0]);
        let mut s = Connections::default();
        assert_eq!(s.view("a", fields(&d)).0, Status::Untested);
        s.begin("a", &d, Some(&cfg.providers[0]), 0);
        assert_eq!(s.view("a", fields(&d)).0, Status::Testing);
        s.finish(&cfg, Some(feedback(&cfg, "a", 0, true)));
        assert_eq!(
            s.view("a", fields(&d)),
            (Status::Passed, "mock-model", "HTTP 200")
        );
        let other = Draft::from_provider(&cfg.providers[1]);
        assert_eq!(s.view("b", fields(&other)).0, Status::Untested);
        assert_eq!(s.view("a", fields(&d)).0, Status::Passed);
    }

    #[test]
    fn live_field_changes_hide_receipt_before_the_draft_tick() {
        let cfg = fixture();
        let (s, d) = passed(&cfg);
        for field in ["name", "key", "upstream", "effort", "protocol"] {
            let mut edited = d.clone();
            match field {
                "name" => edited.name.push('x'),
                "key" => edited.key.push('x'),
                "upstream" => edited.upstream.push_str("/changed"),
                "effort" => edited.effort = "high".into(),
                _ => edited.protocol = UpstreamProtocol::AnthropicMessages,
            }
            assert_eq!(s.view("a", fields(&edited)).0, Status::Untested);
        }
        // Cancel restoring the exact tested inputs may show the historical receipt.
        assert_eq!(s.view("a", fields(&d)).0, Status::Passed);
    }

    #[test]
    fn changed_models_or_capabilities_cannot_keep_the_success_receipt() {
        let cfg = fixture();
        let (mut s, d) = passed(&cfg);
        let mut edited = d.clone();
        edited.models[0]["model"] = json!("different-model");
        s.observe("a", &edited);
        assert_eq!(s.view("a", fields(&edited)).0, Status::Untested);
        edited = d.clone();
        edited.models[0]["capabilities"] = json!({"context":8192});
        s.observe("a", &edited);
        assert_eq!(s.view("a", fields(&edited)).0, Status::Untested);
        s.observe("a", &d);
        assert_eq!(s.view("a", fields(&d)).0, Status::Passed);
    }

    #[test]
    fn failure_replaces_old_success_and_incomplete_save_is_not_a_probe() {
        let cfg = fixture();
        let (mut s, d) = passed(&cfg);
        s.begin("a", &d, Some(&cfg.providers[0]), 0);
        s.finish(&cfg, Some(feedback(&cfg, "a", 0, false)));
        assert_eq!(s.view("a", fields(&d)).0, Status::Failed);
        s.begin("a", &d, Some(&cfg.providers[0]), 0);
        s.finish(&cfg, None);
        assert_eq!(s.view("a", fields(&d)).0, Status::Incomplete);
        s.begin("a", &d, Some(&cfg.providers[0]), 0);
        s.finish(&cfg, Some(Feedback::from_result("a", &Ok("Saved".into()))));
        assert_eq!(s.view("a", fields(&d)).0, Status::Incomplete);
    }

    #[test]
    fn identity_comes_from_the_request_not_a_duplicate_display_name() {
        let cfg = fixture();
        let d = Draft::from_provider(&cfg.providers[0]);
        let mut s = Connections::default();
        s.begin("a", &d, Some(&cfg.providers[0]), 0);
        s.finish(&cfg, Some(feedback(&cfg, "b", 0, true)));
        assert_eq!(s.view("a", fields(&d)).0, Status::Incomplete);
    }

    #[test]
    fn wrong_protocol_model_endpoint_or_payload_never_turns_green() {
        let cfg = fixture();
        let d = Draft::from_provider(&cfg.providers[0]);
        let c = cfg.connection("ma").unwrap();
        for key in ["kind", "provider", "model", "protocol", "url", "http"] {
            let mut report = TestReport::from_connection(&c);
            report.http = "HTTP 200".into();
            let mut v = serde_json::to_value(report).unwrap();
            v[key] = json!("wrong-value");
            let mut s = Connections::default();
            s.begin("a", &d, Some(&cfg.providers[0]), 0);
            s.finish(&cfg, Some(Feedback::from_result("a", &Ok(v.to_string()))));
            assert_eq!(s.view("a", fields(&d)).0, Status::Incomplete);
        }
    }

    #[test]
    fn core_save_normalization_and_new_route_ids_still_bind_to_the_tested_inputs() {
        let mut cfg = fixture();
        let mut d = Draft::from_provider(&cfg.providers[0]);
        d.name = format!(" {} ", d.name);
        d.upstream = format!(" {} ", d.upstream);
        d.key.clear(); // Existing Save keeps the saved credential when blank.
        d.effort = "high".into();
        d.models.push(json!({"model":"new-model"}));
        let mut s = Connections::default();
        s.begin("a", &d, Some(&cfg.providers[0]), 1);
        let p = &mut cfg.providers[0];
        p.name = d.name.trim().into();
        p.upstream = d.upstream.trim().into();
        p.set_model_rows(&d.models, &d.effort).unwrap();
        let saved = Draft::from_provider(&cfg.providers[0]);
        s.observe("a", &saved);
        s.finish(&cfg, Some(feedback(&cfg, "a", 1, true)));
        assert_eq!(
            s.view("a", fields(&saved)),
            (Status::Passed, "new-model", "HTTP 200")
        );
    }

    #[test]
    fn pending_creation_rebinds_to_the_actual_id_and_deletion_drops_receipts() {
        let mut cfg = fixture();
        let d = Draft::from_provider(&cfg.providers[0]);
        let mut s = Connections::default();
        s.begin("local-new", &d, None, 0);
        cfg.providers[0].id = "created".into();
        s.rebind("local-new", "created");
        s.observe("created", &d);
        s.finish(&cfg, Some(feedback(&cfg, "created", 0, true)));
        assert_eq!(s.view("created", fields(&d)).0, Status::Passed);
        assert_eq!(s.view("local-new", fields(&d)).0, Status::Untested);
        cfg.providers.remove(0);
        s.reconcile(&cfg, &BTreeMap::new());
        assert_eq!(s.view("created", fields(&d)).0, Status::Untested);
    }

    #[test]
    fn saved_configuration_drift_and_core_loss_invalidate_past_success() {
        let mut cfg = fixture();
        let (mut s, d) = passed(&cfg);
        cfg.providers[0].api_key = "synthetic-replacement".into();
        s.reconcile(&cfg, &BTreeMap::from([("a".into(), d.clone())]));
        assert_eq!(s.view("a", fields(&d)).0, Status::Untested);
        let (mut s, d) = passed(&cfg);
        assert!(s.forget_results());
        assert_eq!(s.view("a", fields(&d)).0, Status::Untested);
        assert!(!s.forget_results());
        s.begin("a", &d, Some(&cfg.providers[0]), 0);
        assert!(!s.forget_results());
        assert_eq!(s.view("a", fields(&d)).0, Status::Testing);
        s.finish(&cfg, Some(feedback(&cfg, "a", 0, true)));
        assert_eq!(s.view("a", fields(&d)).0, Status::Passed);
    }

    #[test]
    fn canceled_local_attempts_leave_no_receipt_and_late_feedback_cannot_rebind_them() {
        let cfg = fixture();
        let (mut s, d) = passed(&cfg);
        s.begin("local-cancel", &d, None, 0);
        s.discard("local-cancel");
        s.finish(&cfg, Some(feedback(&cfg, "a", 0, true)));
        assert_eq!(s.view("local-cancel", fields(&d)).0, Status::Untested);
        assert!(!s.observed.contains_key("local-cancel"));
        assert!(!s.receipts.contains_key("local-cancel"));
        assert_eq!(s.view("a", fields(&d)).0, Status::Passed);
    }

    #[test]
    fn copy_seed_preserves_blank_key_capabilities_and_legacy_effort_without_cross_binding() {
        let mut cfg = fixture();
        cfg.providers[0].models[0].reasoning_effort = "high".into();
        cfg.providers[0].models[0].capabilities =
            Some(serde_json::from_value(json!({"maxOutputTokens":8192})).unwrap());
        let mut d = Draft::from_provider(&cfg.providers[0]);
        d.name = "Copied provider".into();
        d.key.clear();
        d.effort = "keep".into();
        d.models[0].as_object_mut().unwrap().remove("capabilities");
        let mut pending = crate::provider_create::Pending {
            id: "local-copy".into(),
            return_to: "a".into(),
            source_id: Some("a".into()),
            created: false,
            uncertain: false,
        };
        let mut s = Connections::default();
        s.begin(
            &pending.id,
            &d,
            seed_provider(&cfg, Some(&pending), &pending.id),
            0,
        );
        // Duplicate/Save inherits the source credential and missing model fields,
        // generates new route IDs, then the original Test uses the actual ID.
        let mut copy = cfg.providers[0].clone();
        copy.id = "created-copy".into();
        copy.name = d.name.clone();
        copy.models[0].id = "new-route".into();
        copy.models[0].alias = "new-alias".into();
        copy.set_model_rows(&d.models, &d.effort).unwrap();
        cfg.providers.push(copy);
        let saved = Draft::from_provider(cfg.providers.last().unwrap());
        s.rebind(&pending.id, "created-copy");
        s.observe("created-copy", &saved);
        s.finish(&cfg, Some(feedback(&cfg, "created-copy", 0, true)));
        assert_eq!(s.view("created-copy", fields(&saved)).0, Status::Passed);
        assert_eq!(
            s.view("a", fields(&Draft::from_provider(&cfg.providers[0])))
                .0,
            Status::Untested
        );
        assert!(seed_provider(&cfg, Some(&pending), "b").is_some_and(|p| p.id == "b"));
        assert!(seed_provider(&cfg, Some(&pending), "other-local-draft").is_none());
        pending.created = true;
        assert!(seed_provider(&cfg, Some(&pending), &pending.id).is_none());
    }
}
