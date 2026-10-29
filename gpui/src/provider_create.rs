//! UI-owned creation transaction over the unchanged management API.
use crate::{
    config::{self, Config, Provider},
    state::Draft,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub const COMMAND: &str = "ui_create_provider";

#[derive(Clone, Serialize, Deserialize)]
pub struct Pending {
    pub id: String,
    pub return_to: String,
    pub source_id: Option<String>,
    #[serde(default)]
    pub created: bool,
    #[serde(default)]
    pub uncertain: bool,
}

pub struct Outcome {
    pub local_id: String,
    pub provider_id: Option<String>,
    pub draft: Draft,
    pub saved: bool,
    pub uncertain: bool,
}

pub struct Failure {
    pub message: String,
    /// A response or connection failure that proves creation was not dispatched.
    pub certain: bool,
}
impl Failure {
    pub fn certain(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            certain: true,
        }
    }
    pub fn uncertain(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            certain: false,
        }
    }
}

pub fn request(pending: &Pending, draft: &Draft) -> Value {
    json!({"command":COMMAND,"pending":pending,"draft":draft})
}

/// Match the existing save semantics, including blank-key retention and
/// legacy per-model effort. No files or control commands are touched here.
fn apply(
    draft: &Draft,
    seed: &Provider,
    source: Option<&Provider>,
) -> Result<(Provider, Draft), String> {
    let mut provider = seed.clone();
    provider.name = draft.name.trim().into();
    provider.upstream = draft.upstream.trim().into();
    if !draft.key.trim().is_empty() {
        provider.api_key = draft.key.trim().into();
    }
    if provider.api_key.is_empty() {
        return Err("请填写上游 API Key / Enter upstream API key".into());
    }
    provider.protocol = draft.protocol;
    provider.protocol_settings = draft.protocol_settings.clone();
    let mut mapped = draft.clone();
    if let Some(source) = source {
        // duplicate regenerates route IDs. Compare its seed before mapping so
        // a concurrent source edit cannot silently attach the wrong identity.
        if source.models.len() != seed.models.len()
            || source.models.iter().zip(&seed.models).any(|(a, b)| {
                a.name != b.name
                    || a.model != b.model
                    || a.reasoning_effort != b.reasoning_effort
                    || a.capabilities != b.capabilities
            })
        {
            return Err("来源供应商已更改，复制草稿已保留，请检查后重试 / Source changed; copied draft retained for review".into());
        }
        for row in &mut mapped.models {
            if let Some(index) = source
                .models
                .iter()
                .position(|m| row["id"].as_str() == Some(&m.id))
            {
                row["id"] = json!(seed.models[index].id);
            }
        }
    }
    provider.set_model_rows(&mapped.models, &mapped.effort)?;
    if mapped.effort == "keep"
        && mapped
            .models
            .iter()
            .zip(&provider.models)
            .any(|(row, model)| {
                row["reasoning_effort"]
                    .as_str()
                    .is_some_and(|effort| effort != model.reasoning_effort)
            })
    {
        return Err("来源模型的思考配置已更改，复制草稿未保存；请检查思考强度后重试 / Source model efforts changed; review effort settings before saving the copied draft".into());
    }
    Ok((provider, mapped))
}

fn validate(cfg: &Config, provider: Provider, replace: bool) -> Result<(), String> {
    let mut projected = cfg.clone();
    if replace {
        projected.providers.retain(|p| p.id != provider.id);
    }
    projected.providers.push(provider);
    projected.validate()
}

/// Validate before creation; bind exactly one newly generated ID; never retry
/// an ambiguous create automatically. Retrying a known target only saves it.
pub fn commit(
    pending: &Pending,
    draft: &Draft,
    mut read: impl FnMut() -> Result<Config, String>,
    mut call: impl FnMut(&Value) -> Result<String, Failure>,
) -> (Result<String, String>, Outcome) {
    let mut outcome = Outcome {
        local_id: pending.id.clone(),
        provider_id: pending.created.then(|| pending.id.clone()),
        draft: draft.clone(),
        saved: false,
        uncertain: pending.uncertain,
    };
    let result = (|| {
        if pending.uncertain {
            return Err("上次创建结果尚未确认，请先检查供应商列表；草稿已保留 / Previous creation is unconfirmed; review the provider list; draft retained".into());
        }
        let before = read()?;
        if pending.created {
            let seed = before
                .providers
                .iter()
                .find(|p| p.id == pending.id)
                .ok_or("Provider no longer exists / 供应商已不存在")?;
            let (projected, mapped) = apply(draft, seed, None)?;
            validate(&before, projected, true)?;
            outcome.draft = mapped;
        } else {
            let source = pending
                .source_id
                .as_ref()
                .and_then(|id| before.providers.iter().find(|p| p.id == *id));
            let mut simulated = source.cloned().unwrap_or_default();
            simulated.id = pending.id.clone();
            for model in &mut simulated.models {
                model.id = config::token()[..12].into();
                model.refresh_alias();
            }
            let (projected, _) = apply(draft, &simulated, source)?;
            validate(&before, projected, false)?;
            // duplicate itself is also subject to the core's capacity gate,
            // even if the edited copy will subsequently contain fewer rows.
            let mut intermediate = simulated.clone();
            intermediate.name = draft.name.trim().into();
            validate(&before, intermediate, false)?;
            let create = if let Some(source) = source {
                json!({"command":"duplicate","provider_id":source.id,"ui":true})
            } else {
                json!({"command":"add","ui":true})
            };
            if let Err(failure) = call(&create) {
                outcome.uncertain = !failure.certain;
                return Err(if outcome.uncertain {
                    format!(
                        "Creation result unconfirmed; draft retained: {} / 创建结果未确认，草稿已保留：{}",
                        crate::i18n::message(&failure.message, false),
                        crate::i18n::message(&failure.message, true)
                    )
                } else {
                    failure.message
                });
            }
            let after = read().map_err(|_| {
                outcome.uncertain = true;
                "Creation result unconfirmed; draft retained / 创建结果未确认，草稿已保留"
                    .to_string()
            })?;
            let added: Vec<_> = after
                .providers
                .iter()
                .filter(|p| !before.providers.iter().any(|old| old.id == p.id))
                .collect();
            if added.len() != 1 {
                outcome.uncertain = true;
                return Err(
                    "Creation result unconfirmed; draft retained / 创建结果未确认，草稿已保留"
                        .into(),
                );
            }
            let target = added[0];
            outcome.provider_id = Some(target.id.clone());
            let (projected, mapped) = apply(draft, target, source)?;
            validate(&after, projected, true)?;
            outcome.draft = mapped;
        }
        let id = outcome
            .provider_id
            .as_ref()
            .ok_or("Missing creation target")?;
        match call(&outcome.draft.payload(id)) {
            Ok(message) => {
                outcome.saved = true;
                Ok(message)
            }
            Err(failure) => Err(format!(
                "Provider created; save incomplete; draft retained: {} / 供应商已创建，保存未完成，草稿已保留：{}",
                crate::i18n::message(&failure.message, false),
                crate::i18n::message(&failure.message, true)
            )),
        }
    })();
    (result, outcome)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn source() -> Provider {
        let mut p = Provider {
            name: "Source".into(),
            upstream: "https://example.test/v1".into(),
            api_key: "synthetic-key".into(),
            ..Default::default()
        };
        p.set_models("one\ntwo", "low").unwrap();
        p.models[1].reasoning_effort = "high".into();
        p
    }

    #[test]
    fn copy_commit_updates_only_the_new_provider_with_mixed_efforts_and_fresh_routes() {
        let source = source();
        let cfg = RefCell::new(Config {
            providers: vec![source.clone()],
            ..Default::default()
        });
        let mut draft = Draft::from_provider(&source);
        draft.name = "Unsaved copy".into();
        draft.models[0]["name"] = json!("Renamed row");
        let pending = Pending {
            id: "draft-new".into(),
            return_to: source.id.clone(),
            source_id: Some(source.id.clone()),
            created: false,
            uncertain: false,
        };
        let calls = RefCell::new(Vec::new());
        let (result, outcome) = commit(
            &pending,
            &draft,
            || Ok(cfg.borrow().clone()),
            |payload| {
                calls
                    .borrow_mut()
                    .push(payload["command"].as_str().unwrap().to_string());
                if payload["command"] == "duplicate" {
                    let mut copy = source.clone();
                    copy.id = "created-copy".into();
                    copy.name = "Source copy".into();
                    for model in &mut copy.models {
                        model.id = config::token()[..12].into();
                        model.refresh_alias();
                    }
                    cfg.borrow_mut().providers.push(copy);
                } else {
                    let mut state = cfg.borrow_mut();
                    let p = state.providers.last_mut().unwrap();
                    p.name = payload["name"].as_str().unwrap().into();
                    p.set_model_rows(
                        payload["models"].as_array().unwrap(),
                        payload["reasoning_effort"].as_str().unwrap(),
                    )
                    .unwrap();
                }
                Ok("saved".into())
            },
        );
        assert!(result.is_ok() && outcome.saved);
        assert_eq!(*calls.borrow(), ["duplicate", "save"]);
        let state = cfg.borrow();
        assert_eq!(state.providers[0].name, source.name);
        assert_eq!(state.providers[0].models[0].name, source.models[0].name);
        let copy = &state.providers[1];
        assert_eq!(copy.name, "Unsaved copy");
        assert_eq!(copy.models[0].name, "Renamed row");
        assert_eq!(copy.models[0].reasoning_effort, "low");
        assert_eq!(copy.models[1].reasoning_effort, "high");
        assert!(
            copy.models
                .iter()
                .zip(&source.models)
                .all(|(a, b)| a.id != b.id && a.alias != b.alias)
        );
    }

    #[test]
    fn invalid_and_changed_legacy_drafts_do_not_create_and_unknown_results_do_not_retry() {
        let source = source();
        let cfg = Config {
            providers: vec![source.clone()],
            ..Default::default()
        };
        let mut draft = Draft::from_provider(&source);
        draft.name.clear();
        let pending = Pending {
            id: "draft-new".into(),
            return_to: source.id.clone(),
            source_id: Some(source.id.clone()),
            created: false,
            uncertain: false,
        };
        let (result, _) = commit(
            &pending,
            &draft,
            || Ok(cfg.clone()),
            |_| panic!("invalid draft must not write"),
        );
        assert!(result.is_err());
        draft.name = "Copy".into();
        draft.models[0]["reasoning_effort"] = json!("max");
        let (result, _) = commit(
            &pending,
            &draft,
            || Ok(cfg.clone()),
            |_| panic!("unrepresentable legacy effort must not write"),
        );
        assert!(result.is_err());
        draft.models[0]["reasoning_effort"] = json!("low");
        let (_, outcome) = commit(
            &pending,
            &draft,
            || Ok(cfg.clone()),
            |_| Err(Failure::uncertain("timeout")),
        );
        assert!(outcome.uncertain);
        let blocked = Pending {
            uncertain: true,
            ..pending
        };
        let (result, _) = commit(
            &blocked,
            &draft,
            || panic!("blocked before IO"),
            |_| panic!("must not create twice"),
        );
        assert!(result.is_err());
    }
    #[test]
    fn a_failed_save_retries_only_the_bound_copy_and_preserves_renamed_row_efforts() {
        let source = source();
        let cfg = RefCell::new(Config {
            providers: vec![source.clone()],
            ..Default::default()
        });
        let mut draft = Draft::from_provider(&source);
        draft.name = "Copy".into();
        draft.models[0]["name"] = json!("Renamed");
        draft.models[0]["model"] = json!("changed-upstream-id");
        let pending = Pending {
            id: "draft-copy".into(),
            return_to: source.id.clone(),
            source_id: Some(source.id.clone()),
            created: false,
            uncertain: false,
        };
        let (result, outcome) = commit(
            &pending,
            &draft,
            || Ok(cfg.borrow().clone()),
            |payload| {
                if payload["command"] == "duplicate" {
                    let mut copy = source.clone();
                    copy.id = "new-target".into();
                    copy.name = "Source copy".into();
                    for model in &mut copy.models {
                        model.id = config::token()[..12].into();
                        model.refresh_alias();
                    }
                    cfg.borrow_mut().providers.push(copy);
                    Ok("created".into())
                } else {
                    Err(Failure::certain("synthetic save failure"))
                }
            },
        );
        assert!(result.is_err());
        assert!(!outcome.saved);
        assert_eq!(outcome.provider_id.as_deref(), Some("new-target"));
        assert_eq!(
            outcome.draft.models[0]["id"],
            cfg.borrow().providers[1].models[0].id
        );
        let retry = Pending {
            id: "new-target".into(),
            created: true,
            ..pending
        };
        let (result, outcome) = commit(
            &retry,
            &outcome.draft,
            || Ok(cfg.borrow().clone()),
            |payload| {
                assert_eq!(payload["command"], "save");
                assert_eq!(payload["provider_id"], "new-target");
                let mut state = cfg.borrow_mut();
                let target = &mut state.providers[1];
                target.name = payload["name"].as_str().unwrap().into();
                target
                    .set_model_rows(
                        payload["models"].as_array().unwrap(),
                        payload["reasoning_effort"].as_str().unwrap(),
                    )
                    .unwrap();
                Ok("saved".into())
            },
        );
        assert!(result.is_ok() && outcome.saved);
        let state = cfg.borrow();
        assert_eq!(state.providers.len(), 2);
        assert_eq!(state.providers[1].models[0].reasoning_effort, "low");
        assert_eq!(state.providers[1].models[1].reasoning_effort, "high");
        assert_eq!(state.providers[0].models[0].model, source.models[0].model);
    }

    #[test]
    fn ambiguous_identity_and_capacity_never_save_an_arbitrary_last_provider() {
        let source = source();
        let cfg = RefCell::new(Config {
            providers: vec![source.clone()],
            ..Default::default()
        });
        let mut draft = Draft::from_provider(&source);
        draft.name = "Copy".into();
        let pending = Pending {
            id: "draft-copy".into(),
            return_to: source.id.clone(),
            source_id: Some(source.id.clone()),
            created: false,
            uncertain: false,
        };
        let (result, outcome) = commit(
            &pending,
            &draft,
            || Ok(cfg.borrow().clone()),
            |payload| {
                assert_eq!(payload["command"], "duplicate");
                for name in ["Our copy", "Concurrent provider"] {
                    let p = Provider {
                        name: name.into(),
                        ..Default::default()
                    };
                    cfg.borrow_mut().providers.push(p);
                }
                Ok("created".into())
            },
        );
        assert!(result.is_err());
        assert!(outcome.uncertain);
        assert!(outcome.provider_id.is_none());
        let mut full = Config {
            providers: vec![source],
            ..Default::default()
        };
        for i in 1..64 {
            full.providers.push(Provider {
                name: format!("Provider {i}"),
                ..Default::default()
            });
        }
        full.validate().unwrap();
        let (result, _) = commit(
            &pending,
            &draft,
            || Ok(full.clone()),
            |_| panic!("capacity rejection must precede all writes"),
        );
        assert!(result.is_err());
    }

    #[test]
    fn pending_forms_roundtrip_without_changing_core_config_or_other_provider_drafts() {
        let folder = tempfile::tempdir().unwrap();
        let source = source();
        let cfg = Config {
            providers: vec![source.clone()],
            ..Default::default()
        };
        config::write_json(&folder.path().join("config.json"), &cfg).unwrap();
        let original = std::fs::read(folder.path().join("config.json")).unwrap();
        let mut drafts = crate::state::Drafts::from_config(folder.path(), &cfg);
        drafts.providers.get_mut(&source.id).unwrap().name = "Earlier unsaved source".into();
        let pending = Pending {
            id: "draft-local".into(),
            return_to: source.id.clone(),
            source_id: None,
            created: false,
            uncertain: false,
        };
        drafts.providers.insert(
            pending.id.clone(),
            Draft {
                name: "Unfinished new provider".into(),
                effort: "keep".into(),
                ..Default::default()
            },
        );
        drafts.selected = pending.id.clone();
        drafts.pending = Some(pending);
        drafts.persist(folder.path()).unwrap();
        let loaded = crate::state::Drafts::load(folder.path(), &cfg);
        assert_eq!(loaded.pending.unwrap().id, "draft-local");
        assert_eq!(loaded.selected, "draft-local");
        assert_eq!(
            loaded.providers["draft-local"].name,
            "Unfinished new provider"
        );
        assert_eq!(loaded.providers[&source.id].name, "Earlier unsaved source");
        assert_eq!(
            std::fs::read(folder.path().join("config.json")).unwrap(),
            original
        );
    }
    #[test]
    #[ignore = "requires the isolated real-core fixture manifest"]
    fn real_core_contract() {
        use crate::backend::{Backend, Job, Reply};
        let path = std::env::var("WORKBUDDY_PROXY_DISCOVERY_PARITY_FIXTURE").unwrap();
        let fixture: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        let dir = std::path::Path::new(fixture["core_dir"].as_str().unwrap());
        let backend = Backend::new(dir.to_path_buf());
        let job = |tag: &str, steps: Vec<Value>| -> Reply {
            backend
                .jobs
                .send(Job {
                    tag: tag.into(),
                    steps,
                })
                .unwrap();
            backend
                .replies
                .recv_timeout(std::time::Duration::from_secs(110))
                .unwrap()
        };
        let before = Config::load(dir).unwrap();
        let source = before.providers[0].clone();
        let mut draft = Draft {
            name: "New account".into(),
            key: "synthetic-new-account-key".into(),
            upstream: "http://127.0.0.1:1/v1".into(),
            effort: "keep".into(),
            models: vec![json!({"name":"New model","model":"new-model"})],
            ..Default::default()
        };
        let pending = Pending {
            id: "draft-new".into(),
            return_to: source.id.clone(),
            source_id: None,
            created: false,
            uncertain: false,
        };
        let original = std::fs::read(dir.join("config.json")).unwrap();
        draft.name.clear();
        let reply = job("save", vec![request(&pending, &draft)]);
        assert!(reply.result.is_err());
        assert!(reply.creation.unwrap().provider_id.is_none());
        assert_eq!(std::fs::read(dir.join("config.json")).unwrap(), original);
        draft.name = "New account".into();
        // A refused connection is known not to dispatch creation and can retry.
        let instance = std::fs::read(dir.join("instance.json")).unwrap();
        let mut info: Value = serde_json::from_slice(&instance).unwrap();
        info["port"] = json!(1);
        std::fs::write(dir.join("instance.json"), info.to_string()).unwrap();
        let reply = job("save", vec![request(&pending, &draft)]);
        std::fs::write(dir.join("instance.json"), &instance).unwrap();
        assert!(reply.result.is_err());
        let outcome = reply.creation.unwrap();
        assert!(!outcome.uncertain && outcome.provider_id.is_none());
        assert_eq!(std::fs::read(dir.join("config.json")).unwrap(), original);
        let reply = job("save", vec![request(&pending, &draft)]);
        assert!(reply.result.is_ok());
        let created = reply.creation.unwrap();
        assert!(created.saved);
        let new_id = created.provider_id.unwrap();
        assert_ne!(new_id, pending.id);
        let cfg = Config::load(dir).unwrap();
        assert_eq!(cfg.providers.len(), 2);
        assert_eq!(
            serde_json::to_value(&cfg.providers[0]).unwrap(),
            serde_json::to_value(&source).unwrap()
        );
        // Copy keeps the old source-save boundary, but its editable copy is new.
        let mut source_draft = Draft::from_provider(&source);
        source_draft.name = "Updated source".into();
        let reply = job("copy_prepare", vec![source_draft.payload(&source.id)]);
        assert!(reply.result.is_ok());
        assert_eq!(reply.saved_provider.as_deref(), Some(source.id.as_str()));
        let cfg = Config::load(dir).unwrap();
        assert_eq!(cfg.providers.len(), 2);
        let saved_source = cfg
            .providers
            .iter()
            .find(|p| p.id == source.id)
            .unwrap()
            .clone();
        let mut copy = Draft::from_provider(&saved_source);
        copy.name = "Independent copy".into();
        copy.key = "synthetic-copy-key".into();
        copy.models[0]["name"] = json!("Renamed copy model");
        copy.models[0]["model"] = json!("changed-upstream-model");
        copy.models[0]["capabilities"] = json!(crate::capabilities::ModelCapabilities {
            supports_images: true,
            supports_tool_call: true,
            ..Default::default()
        });
        let copied = Pending {
            id: "draft-copy".into(),
            return_to: source.id.clone(),
            source_id: Some(source.id.clone()),
            created: false,
            uncertain: false,
        };
        let reply = job("save", vec![request(&copied, &copy)]);
        assert!(reply.result.is_ok());
        let outcome = reply.creation.unwrap();
        assert!(outcome.saved);
        let cfg = Config::load(dir).unwrap();
        assert_eq!(cfg.providers.len(), 3);
        let target = cfg
            .providers
            .iter()
            .find(|p| Some(&p.id) == outcome.provider_id.as_ref())
            .unwrap();
        assert_eq!(target.name, "Independent copy");
        assert_eq!(target.models[0].reasoning_effort, "low");
        assert_eq!(target.models[1].reasoning_effort, "high");
        assert!(
            target.models[0]
                .capabilities
                .as_ref()
                .unwrap()
                .supports_images
        );
        assert!(
            target
                .models
                .iter()
                .zip(&saved_source.models)
                .all(|(a, b)| a.id != b.id && a.alias != b.alias)
        );
        assert_eq!(
            serde_json::to_value(cfg.providers.iter().find(|p| p.id == source.id).unwrap())
                .unwrap(),
            serde_json::to_value(&saved_source).unwrap()
        );
        // Fail only the post-create save at the test proxy, then retry by ID.
        std::fs::write(fixture["fail_save_marker"].as_str().unwrap(), b"1").unwrap();
        copy.name = "Retry copy".into();
        let retry_pending = Pending {
            id: "draft-retry".into(),
            ..copied.clone()
        };
        let reply = job("save", vec![request(&retry_pending, &copy)]);
        assert!(reply.result.is_err());
        let partial = reply.creation.unwrap();
        assert!(!partial.saved && partial.provider_id.is_some());
        let counts = || -> Value {
            serde_json::from_slice(&std::fs::read(fixture["rpc_counts"].as_str().unwrap()).unwrap())
                .unwrap()
        };
        let creates = counts()["creates"].clone();
        let bound = Pending {
            id: partial.provider_id.clone().unwrap(),
            created: true,
            ..retry_pending
        };
        let reply = job("save", vec![request(&bound, &partial.draft)]);
        assert!(reply.result.is_ok());
        assert_eq!(counts()["creates"], creates);
        let cfg = Config::load(dir).unwrap();
        assert_eq!(cfg.providers.len(), 4);
        let target = cfg.providers.iter().find(|p| p.id == bound.id).unwrap();
        assert_eq!(target.models[0].reasoning_effort, "low");
        assert_eq!(target.models[0].model, "changed-upstream-model");
        // The API saves before Test. A later test error must not hide that save.
        let mut committed = Draft::from_provider(&saved_source);
        committed.name = "Saved before failed test".into();
        let reply = job(
            "test",
            vec![
                committed.payload(&source.id),
                json!({"command":"test","provider_id":source.id}),
            ],
        );
        assert!(reply.result.is_err());
        assert_eq!(reply.saved_provider.as_deref(), Some(source.id.as_str()));
        assert_eq!(
            Config::load(dir)
                .unwrap()
                .providers
                .iter()
                .find(|p| p.id == source.id)
                .unwrap()
                .name,
            "Saved before failed test"
        );
        // First-provider discovery initially lives only in the frontend. A
        // successful save delegates to the core's existing background refresh.
        for fail_refresh in [false, true] {
            let mut discover = fixture["cases"][0]["payload"].clone();
            discover["upstream"] = json!(format!(
                "{}/fresh-transfer-{fail_refresh}/v1",
                discover["upstream"]
                    .as_str()
                    .unwrap()
                    .strip_suffix("/chat/v1")
                    .unwrap()
            ));
            discover["provider_id"] = json!(format!("draft-transfer-{fail_refresh}"));
            discover["command"] = json!("ui_discover_models");
            let mut metadata = discover.clone();
            metadata["command"] = json!("model_metadata");
            metadata["model"] = json!("responses-one");
            let before: Value =
                serde_json::from_str(&crate::backend::call(dir, &metadata).unwrap()).unwrap();
            assert_eq!(before["metadata"]["sources"], json!(["unknown"]));
            let reply = job("fetch", vec![discover.clone()]);
            assert!(reply.result.is_ok());
            let local = job("metadata", vec![metadata.clone()]);
            let local: Value = serde_json::from_str(&local.result.unwrap()).unwrap();
            assert_eq!(local["metadata"]["capabilities"]["supportsReasoning"], true);
            let draft = Draft {
                name: format!("Transfer {fail_refresh}"),
                upstream: discover["upstream"].as_str().unwrap().into(),
                key: discover["key"].as_str().unwrap().into(),
                effort: "keep".into(),
                models: vec![json!({"name":"automatic model","model":"responses-one"})],
                ..Default::default()
            };
            let pending = Pending {
                id: discover["provider_id"].as_str().unwrap().into(),
                return_to: source.id.clone(),
                source_id: None,
                created: false,
                uncertain: false,
            };
            if fail_refresh {
                std::fs::write(fixture["fail_warm_marker"].as_str().unwrap(), b"1").unwrap();
            }
            let reply = job("save", vec![request(&pending, &draft)]);
            assert!(reply.result.is_ok());
            let outcome = reply.creation.unwrap();
            assert!(outcome.saved);
            assert_eq!(reply.saved_provider, outcome.provider_id);
            metadata["provider_id"] = json!(outcome.provider_id);
            let mut core: Value =
                serde_json::from_str(&crate::backend::call(dir, &metadata).unwrap()).unwrap();
            if !fail_refresh {
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
                while core["metadata"]["sources"] == json!(["unknown"])
                    && std::time::Instant::now() < deadline
                {
                    std::thread::sleep(std::time::Duration::from_millis(30));
                    core = serde_json::from_str(&crate::backend::call(dir, &metadata).unwrap())
                        .unwrap();
                }
            }
            let frontend = job("metadata", vec![metadata.clone()]);
            let frontend: Value = serde_json::from_str(&frontend.result.unwrap()).unwrap();
            assert_eq!(frontend["metadata"], core["metadata"]);
            assert_eq!(
                core["metadata"]["capabilities"]["supportsReasoning"],
                !fail_refresh
            );
            let saved = Config::load(dir).unwrap();
            let target = saved
                .providers
                .iter()
                .find(|p| Some(&p.id) == outcome.provider_id.as_ref())
                .unwrap();
            assert!(target.models[0].capabilities.is_none());
        }
        // Concurrent creation makes ID inference ambiguous: no arbitrary save.
        std::fs::write(fixture["ambiguous_marker"].as_str().unwrap(), b"1").unwrap();
        draft.name = "Unconfirmed account".into();
        let ambiguous = Pending {
            id: "draft-ambiguous".into(),
            ..pending
        };
        let reply = job("save", vec![request(&ambiguous, &draft)]);
        assert!(reply.result.is_err());
        let outcome = reply.creation.unwrap();
        assert!(outcome.uncertain && outcome.provider_id.is_none());
        let creates = counts()["creates"].clone();
        let blocked = Pending {
            uncertain: true,
            ..ambiguous
        };
        let reply = job("save", vec![request(&blocked, &draft)]);
        assert!(reply.result.is_err());
        assert_eq!(counts()["creates"], creates);
        std::fs::write(fixture["creation_output"].as_str().unwrap(),json!({"mode":"real core frontend draft transaction","invalid_before_create":true,"refused_connection_retry":true,"source_save_boundary_preserved":true,"copy_capabilities_and_mixed_efforts_preserved":true,"fresh_route_ids_and_aliases":true,"partial_save_retries_bound_target_only":true,"save_then_test_error_reports_commit":true,"saved_automatic_metadata_uses_core_refresh_and_resolver":true,"core_refresh_failure_keeps_saved_target_and_discards_ui_only_hints":true,"ambiguous_identity_does_not_save_or_repeat_create":true}).to_string()).unwrap();
    }
}
