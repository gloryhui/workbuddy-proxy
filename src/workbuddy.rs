use crate::{
    config::{self, Installation},
    proxy::Bridge,
};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

struct Document {
    root: Value,
}
impl Document {
    fn read(path: &Path) -> Result<Self, String> {
        let bytes = match fs::read(path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self { root: json!([]) });
            }
            Err(e) => return Err(e.to_string()),
        };
        if bytes.len() > 2 * 1024 * 1024 {
            return Err("WorkBuddy configuration exceeds 2 MiB".into());
        }
        let d = Self {
            root: serde_json::from_slice(&bytes)
                .map_err(|_| "Invalid WorkBuddy models.json; no file changed")?,
        };
        if d.entries().is_none() {
            return Err("Unsupported WorkBuddy configuration format".into());
        }
        Ok(d)
    }
    fn entries(&self) -> Option<&Vec<Value>> {
        self.root
            .as_array()
            .or_else(|| self.root.get("models").and_then(Value::as_array))
    }
    fn entries_mut(&mut self) -> &mut Vec<Value> {
        if self.root.is_array() {
            self.root.as_array_mut().unwrap()
        } else {
            self.root.get_mut("models").unwrap().as_array_mut().unwrap()
        }
    }
}
pub(crate) fn backup(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dst = path.with_file_name(format!("models.json.workbuddy-proxy-{stamp}.bak"));
    fs::copy(path, &dst).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(dst, fs::Permissions::from_mode(0o600)).map_err(|e| e.to_string())?;
    }
    Ok(())
}
fn route(value: &Value) -> Option<String> {
    if let Some(id) = value
        .pointer("/_workbuddyProxy/route")
        .and_then(Value::as_str)
    {
        return Some(id.into());
    }
    let url = reqwest::Url::parse(value.get("url")?.as_str()?).ok()?;
    url.path()
        .trim_end_matches('/')
        .strip_prefix("/r/")?
        .strip_suffix("/v1")
        .map(str::to_owned)
}
fn installation<'a>(current: &Value, cfg: &'a config::Config) -> Option<&'a Installation> {
    let id = current.get("id").and_then(Value::as_str);
    let current_route = route(current);
    cfg.installed.iter().find(|record| {
        id == Some(record.id.as_str())
            || (managed_signature(current, &cfg.local_key)
                && current_route.is_some()
                && record.expected.as_ref().and_then(route) == current_route)
    })
}
// The marker survives reinstalling the proxy and editing the WorkBuddy entry.
// Older versions wrote no marker: recognize their loopback endpoint plus the
// generated 48-hex local key (or this installation's key), never the alias alone.
fn managed_signature(current: &Value, local_key: &str) -> bool {
    if current
        .pointer("/_workbuddyProxy/managed")
        .and_then(Value::as_bool)
        == Some(true)
    {
        return true;
    }
    let Some(key) = current.get("apiKey").and_then(Value::as_str) else {
        return false;
    };
    if current.get("vendor").and_then(Value::as_str) != Some("Custom")
        || !(key == local_key || (key.len() == 48 && key.bytes().all(|c| c.is_ascii_hexdigit())))
    {
        return false;
    }
    let Some(url) = current
        .get("url")
        .and_then(Value::as_str)
        .and_then(|s| reqwest::Url::parse(s).ok())
    else {
        return false;
    };
    let path = url.path().trim_end_matches('/');
    url.scheme() == "http"
        && matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "[::1]"))
        && url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none()
        && (path == "/v1"
            || path
                .strip_prefix("/r/")
                .and_then(|p| p.strip_suffix("/v1"))
                .is_some_and(|id| !id.is_empty() && !id.contains('/')))
}
fn managed(current: &Value, cfg: &config::Config) -> bool {
    installation(current, cfg).is_some() || managed_signature(current, &cfg.local_key)
}
fn entry(c: &config::Connection, url: &str, key: &str) -> Value {
    let mut e = json!({"id":c.alias,"name":c.label(),"vendor":"Custom","url":url,"apiKey":key,"supportsToolCall":true,"supportsImages":false,"useCustomProtocol":false,"_workbuddyProxy":{"managed":true,"route":c.id}});
    if matches!(
        c.model.as_str(),
        "deepseek-v4.1-flash" | "deepseek-v4-pro" | "deepseek-v4-flash"
    ) {
        e["supportsReasoning"] = json!(true);
        e["onlyReasoning"] = json!(false);
        e["reasoning"] = json!({"defaultEffort":"high","supportedEfforts":["high","max"],"canDisableThinking":true});
        e["maxInputTokens"] = json!(1_000_000);
        e["contextWindow"] =
            json!({"defaultLength":300_000,"supportedLengths":[128_000,300_000,1_000_000]});
    }
    e
}
// Only change the URL port of entries using this proxy's local key. Preserve
// all model names, IDs, reasoning options, other vendors and restore backups.
pub(crate) fn repoint_port(
    cfg: &mut config::Config,
    path: &Path,
    old_port: u16,
    new_port: u16,
) -> Result<Option<(Value, usize)>, String> {
    if !path.exists() {
        return Ok(None);
    }
    fn update(value: &mut Value, key: &str, old: u16, new: u16) -> bool {
        if value.get("apiKey").and_then(Value::as_str) != Some(key) {
            return false;
        }
        let Some(mut url) = value
            .get("url")
            .and_then(Value::as_str)
            .and_then(|s| reqwest::Url::parse(s).ok())
        else {
            return false;
        };
        let path = url.path().trim_end_matches('/');
        if url.scheme() != "http"
            || !matches!(url.host_str(), Some("127.0.0.1" | "localhost"))
            || url.port_or_known_default() != Some(old)
            || !(path == "/v1" || (path.starts_with("/r/") && path.ends_with("/v1")))
        {
            return false;
        }
        if url.set_port(Some(new)).is_err() {
            return false;
        }
        value["url"] = json!(url.as_str());
        true
    }
    let mut document = Document::read(path)?;
    let mut count = 0;
    for entry in document.entries_mut() {
        count += usize::from(update(entry, &cfg.local_key, old_port, new_port));
    }
    for record in &mut cfg.installed {
        if let Some(expected) = &mut record.expected {
            update(expected, &cfg.local_key, old_port, new_port);
        }
    }
    Ok((count > 0).then_some((document.root, count)))
}
impl Bridge {
    pub fn configure_workbuddy(&self, path: &Path) -> Result<usize, String> {
        let mut cfg = self.config.lock().unwrap();
        let mut d = Document::read(path)?;
        let connections = cfg.workbuddy_connections()?;
        let active: Vec<_> = connections
            .iter()
            .filter(|c| !c.api_key.is_empty())
            .collect();
        if active.is_empty()
            && cfg.installed.is_empty()
            && !d.entries().unwrap().iter().any(|e| managed(e, &cfg))
        {
            return Err("Save at least one connection first / 请先保存至少一个连接".into());
        }
        // An unrelated entry with the same display ID is still user-owned.
        // Managed entries are authoritative even if edited, duplicated or left
        // behind by an older installation with a different local key/route.
        for c in &active {
            if d.entries()
                .unwrap()
                .iter()
                .any(|e| e.get("id").and_then(Value::as_str) == Some(&c.alias) && !managed(e, &cfg))
            {
                return Err(
                    "Model alias already exists in WorkBuddy; refusing to overwrite".into(),
                );
            }
        }
        let mut next = cfg.clone();
        next.installed.clear();
        let mut records = HashMap::new();
        for record in &cfg.installed {
            if let Some(id) = cfg.installed_connection_id(record) {
                records.entry(id).or_insert(record);
            }
        }
        let mut emitted = HashSet::new();
        let mut restored = HashSet::new();
        let mut entries = Vec::new();
        for current in std::mem::take(d.entries_mut()) {
            let record = installation(&current, &cfg);
            if record.is_some() || managed_signature(&current, &cfg.local_key) {
                let id = record
                    .and_then(|r| cfg.installed_connection_id(r))
                    .or_else(|| route(&current));
                let connection = active
                    .iter()
                    .find(|c| id.as_deref() == Some(c.id.as_str()))
                    .or_else(|| {
                        active
                            .iter()
                            .find(|c| current.get("id").and_then(Value::as_str) == Some(&c.alias))
                    });
                if let Some(c) = connection {
                    if emitted.insert(c.id.clone()) {
                        entries.push(entry(c, &self.connection_address(&c.id), &cfg.local_key));
                    }
                } else if let Some(record) = record
                    && let Some(previous) = &record.previous
                    && restored.insert(record.id.clone())
                {
                    entries.push(previous.clone());
                }
            } else {
                entries.push(current);
            }
        }
        *d.entries_mut() = entries;
        for c in &active {
            let e = entry(c, &self.connection_address(&c.id), &cfg.local_key);
            next.installed.push(Installation {
                id: c.alias.clone(),
                previous: records.get(&c.id).and_then(|r| r.previous.clone()),
                expected: Some(e.clone()),
            });
            if !emitted.contains(&c.id) {
                d.entries_mut().push(e);
            }
        }
        // Restoring an original entry must not create an ambiguous active ID.
        for c in &active {
            if d.entries()
                .unwrap()
                .iter()
                .filter(|e| e.get("id").and_then(Value::as_str) == Some(&c.alias))
                .count()
                > 1
            {
                return Err("Model alias conflicts with a restored WorkBuddy entry".into());
            }
        }
        config::make_private_dir(path.parent().ok_or("Invalid configuration path")?)?;
        backup(path)?;
        config::write_json(&self.dir.join("config.json"), &next)?;
        if let Err(e) = config::write_json(path, &d.root) {
            let _ = config::write_json(&self.dir.join("config.json"), &*cfg);
            return Err(e);
        }
        let count = active.len();
        *cfg = next;
        Ok(count)
    }
    pub fn restore_workbuddy(&self, path: &Path) -> Result<(), String> {
        let mut cfg = self.config.lock().unwrap();
        let mut d = Document::read(path)?;
        let mut entries = Vec::new();
        let mut restored = HashSet::new();
        for current in std::mem::take(d.entries_mut()) {
            if let Some(record) = installation(&current, &cfg) {
                if let Some(previous) = &record.previous
                    && restored.insert(record.id.clone())
                {
                    entries.push(previous.clone());
                }
            } else if !managed_signature(&current, &cfg.local_key) {
                entries.push(current);
            }
        }
        *d.entries_mut() = entries;
        if path.exists() {
            backup(path)?;
            config::write_json(path, &d.root)?;
        }
        let mut next = cfg.clone();
        next.installed.clear();
        config::write_json(&self.dir.join("config.json"), &next)?;
        *cfg = next;
        Ok(())
    }
}
pub fn path() -> Result<std::path::PathBuf, String> {
    Ok(config::home()?.join(".workbuddy/models.json"))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn readable_sync_migrates_in_place_and_keeps_restore_history() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("models.json");
        let mut cfg = config::Config::default();
        cfg.providers[0].name = "opencode".into();
        cfg.providers[0].upstream = "https://example.com/v1".into();
        cfg.providers[0].api_key = "fixture-key".into();
        cfg.providers[0]
            .set_models("deepseek-v4.1-flash | ds", "default")
            .unwrap();
        let c = cfg.connections().remove(0);
        let url = format!("http://127.0.0.1:18080/r/{}/v1", c.id);
        let old = entry(&c, &url, &cfg.local_key);
        let previous = json!({"id":c.alias,"name":"original user entry","other":"keep"});
        cfg.installed.push(Installation {
            id: c.alias.clone(),
            previous: Some(previous.clone()),
            expected: Some(old.clone()),
        });
        let before = json!({"id":"before","apiKey":"keep"});
        let after = json!({"id":"after","extra":7});
        config::write_json(
            &path,
            &json!({"otherField":true,"models":[before,old,after]}),
        )
        .unwrap();
        let bridge = Bridge::new(cfg, dir.path().into()).unwrap();
        bridge
            .port
            .store(18080, std::sync::atomic::Ordering::Relaxed);
        assert_eq!(bridge.configure_workbuddy(&path).unwrap(), 1);
        let document = Document::read(&path).unwrap();
        let entries = document.entries().unwrap();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0], before);
        assert_eq!(entries[2], after);
        assert_eq!(entries[1]["id"], "opencode/deepseek-v4.1-flash");
        assert_eq!(entries[1]["name"], "ds · opencode");
        assert_eq!(entries[1]["url"], url);
        assert_eq!(entries[1]["reasoning"], old["reasoning"]);
        assert_eq!(
            bridge.config.lock().unwrap().installed[0].previous,
            Some(previous.clone())
        );
        let bytes = fs::read(&path).unwrap();
        bridge.configure_workbuddy(&path).unwrap();
        assert_eq!(fs::read(&path).unwrap(), bytes);
        bridge.config.lock().unwrap().providers[0].name = "renamed".into();
        bridge.configure_workbuddy(&path).unwrap();
        assert_eq!(
            Document::read(&path).unwrap().entries().unwrap()[1]["id"],
            "renamed/deepseek-v4.1-flash"
        );
        bridge.restore_workbuddy(&path).unwrap();
        assert_eq!(
            Document::read(&path).unwrap().root,
            json!({"otherField":true,"models":[before,previous,after]})
        );
    }
    #[test]
    fn legacy_snapshotless_sync_and_user_collisions_are_safe() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("models.json");
        let mut cfg = config::Config::default();
        cfg.providers[0].name = "opencode".into();
        cfg.providers[0].upstream = "https://example.com/v1".into();
        cfg.providers[0].api_key = "fixture-key".into();
        cfg.providers[0].set_models("same", "default").unwrap();
        cfg.providers[0].models[0].alias = "same".into();
        let c = cfg.connections().remove(0);
        let mut old = entry(&c, "http://127.0.0.1:18080/v1", &cfg.local_key);
        old.as_object_mut().unwrap().remove("_workbuddyProxy");
        cfg.installed.push(Installation {
            id: c.alias,
            previous: None,
            expected: None,
        });
        let user = json!({"id":"opencode/same","name":"user-owned"});
        config::write_json(&path, &json!([old, user])).unwrap();
        config::write_json(&dir.path().join("config.json"), &cfg).unwrap();
        let bridge = Bridge::new(cfg, dir.path().into()).unwrap();
        bridge
            .port
            .store(18080, std::sync::atomic::Ordering::Relaxed);
        let bytes = fs::read(&path).unwrap();
        let config_bytes = fs::read(dir.path().join("config.json")).unwrap();
        assert!(bridge.configure_workbuddy(&path).is_err());
        assert_eq!(fs::read(&path).unwrap(), bytes);
        assert_eq!(
            fs::read(dir.path().join("config.json")).unwrap(),
            config_bytes
        );
        config::write_json(&path, &json!([old])).unwrap();
        bridge.configure_workbuddy(&path).unwrap();
        assert_eq!(
            Document::read(&path).unwrap().entries().unwrap()[0]["id"],
            "opencode/same"
        );
        bridge.restore_workbuddy(&path).unwrap();
        assert!(Document::read(&path).unwrap().entries().unwrap().is_empty());
    }
    #[test]
    fn sync_multiple_preserves_unrelated_and_restores() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("models.json");
        let old = json!({"otherField":true,"models":[{"id":"other","apiKey":"keep"}]});
        config::write_json(&p, &old).unwrap();
        let mut c = config::Config::default();
        c.providers[0].upstream = "https://example.com/v1".into();
        c.providers[0].api_key = "test-upstream".into();
        c.providers[0]
            .set_models(
                "deepseek-v4.1-flash | First\ndeepseek-v4.1-flash | Second",
                "default",
            )
            .unwrap();
        let a = Bridge::new(c, dir.path().into()).unwrap();
        a.port.store(18080, std::sync::atomic::Ordering::Relaxed);
        assert_eq!(a.configure_workbuddy(&p).unwrap(), 2);
        let d = Document::read(&p).unwrap();
        assert_eq!(d.entries().unwrap().len(), 3);
        assert_eq!(d.entries().unwrap()[1]["supportsReasoning"], true);
        assert_ne!(d.entries().unwrap()[1]["id"], d.entries().unwrap()[2]["id"]);
        assert_ne!(
            d.entries().unwrap()[1]["url"],
            d.entries().unwrap()[2]["url"]
        );
        a.config.lock().unwrap().providers[0].models.remove(0);
        assert_eq!(a.configure_workbuddy(&p).unwrap(), 1);
        assert_eq!(Document::read(&p).unwrap().entries().unwrap().len(), 2);
        a.restore_workbuddy(&p).unwrap();
        assert_eq!(Document::read(&p).unwrap().root, old);
        a.configure_workbuddy(&p).unwrap();
        let mut d = Document::read(&p).unwrap();
        d.entries_mut()[1]["name"] = json!("user edited");
        config::write_json(&p, &d.root).unwrap();
        a.configure_workbuddy(&p).unwrap();
        let mut d = Document::read(&p).unwrap();
        assert_ne!(d.entries().unwrap()[1]["name"], "user edited");
        d.entries_mut()[1]["name"] = json!("edited again");
        config::write_json(&p, &d.root).unwrap();
        a.restore_workbuddy(&p).unwrap();
        assert_eq!(Document::read(&p).unwrap().root, old);
        a.restore_workbuddy(&p).unwrap();
        assert_eq!(Document::read(&p).unwrap().root, old);
    }
    fn configured() -> config::Config {
        let mut cfg = config::Config::default();
        cfg.providers[0].name = "supplier".into();
        cfg.providers[0].upstream = "https://example.com/v1".into();
        cfg.providers[0].api_key = "fixture-key".into();
        cfg.providers[0].set_models("model", "default").unwrap();
        cfg
    }
    #[test]
    fn edited_duplicate_and_missing_entries_are_authoritative() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("models.json");
        let bridge = Bridge::new(configured(), dir.path().into()).unwrap();
        bridge.configure_workbuddy(&path).unwrap();
        let expected = Document::read(&path).unwrap().entries().unwrap()[0].clone();
        let mut edited = expected.clone();
        // Older entries without a marker are still owned by their recorded ID.
        edited.as_object_mut().unwrap().remove("_workbuddyProxy");
        edited["name"] = json!("manual name");
        edited["url"] = json!("https://edited.example/v1");
        edited["apiKey"] = json!("manual-key");
        let unrelated = json!({"id":"other","name":"keep","extra":7});
        config::write_json(&path, &json!([unrelated, edited, edited])).unwrap();
        bridge.configure_workbuddy(&path).unwrap();
        assert_eq!(
            Document::read(&path).unwrap().root,
            json!([unrelated, expected])
        );
        config::write_json(&path, &json!([unrelated])).unwrap();
        bridge.configure_workbuddy(&path).unwrap();
        assert_eq!(
            Document::read(&path).unwrap().root,
            json!([unrelated, expected])
        );
        config::write_json(&path, &json!([unrelated, edited, edited])).unwrap();
        bridge.restore_workbuddy(&path).unwrap();
        assert_eq!(Document::read(&path).unwrap().root, json!([unrelated]));
        bridge.configure_workbuddy(&path).unwrap();
        config::write_json(&path, &json!([unrelated])).unwrap();
        bridge.restore_workbuddy(&path).unwrap();
        assert_eq!(Document::read(&path).unwrap().root, json!([unrelated]));
        assert!(bridge.config.lock().unwrap().installed.is_empty());
        bridge.configure_workbuddy(&path).unwrap();
        fs::remove_file(&path).unwrap();
        bridge.restore_workbuddy(&path).unwrap();
        bridge.restore_workbuddy(&path).unwrap();
        assert!(!path.exists());
        assert!(bridge.config.lock().unwrap().installed.is_empty());
    }
    #[test]
    fn reinstall_adopts_legacy_entries_and_removes_stale_routes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("models.json");
        let old = configured();
        let c = old.workbuddy_connections().unwrap().remove(0);
        let mut legacy = entry(
            &c,
            &format!("http://127.0.0.1:18080/r/{}/v1", c.id),
            &old.local_key,
        );
        legacy.as_object_mut().unwrap().remove("_workbuddyProxy");
        legacy["name"] = json!("manual legacy name");
        let mut stale = legacy.clone();
        stale["id"] = json!("deleted-supplier/old-model");
        stale["url"] = json!("http://localhost:19090/v1");
        let unrelated = json!({"id":"manual","vendor":"Custom","url":"http://127.0.0.1:18080/v1","apiKey":"user-key"});
        let cfg = configured();
        assert_ne!(cfg.local_key, old.local_key);
        assert_ne!(cfg.connections()[0].id, c.id);
        let bridge = Bridge::new(cfg, dir.path().into()).unwrap();
        config::write_json(
            &path,
            &json!({"extra":true,"models":[unrelated,legacy,legacy,stale]}),
        )
        .unwrap();
        bridge.configure_workbuddy(&path).unwrap();
        let d = Document::read(&path).unwrap();
        assert_eq!(d.root["extra"], true);
        assert_eq!(d.entries().unwrap().len(), 2);
        assert_eq!(d.entries().unwrap()[0], unrelated);
        assert_eq!(d.entries().unwrap()[1]["id"], "supplier/model");
        assert_eq!(
            d.entries().unwrap()[1]["apiKey"],
            bridge.config.lock().unwrap().local_key
        );
        let bytes = fs::read(&path).unwrap();
        bridge.configure_workbuddy(&path).unwrap();
        assert_eq!(fs::read(&path).unwrap(), bytes);
        // Cancel directly after another reinstall, with no installation history.
        let fresh = Bridge::new(configured(), dir.path().into()).unwrap();
        fresh.restore_workbuddy(&path).unwrap();
        assert_eq!(
            Document::read(&path).unwrap().root,
            json!({"extra":true,"models":[unrelated]})
        );
        config::write_json(&path, &json!([unrelated, legacy, stale])).unwrap();
        fresh.restore_workbuddy(&path).unwrap();
        assert_eq!(Document::read(&path).unwrap().root, json!([unrelated]));
    }
    #[test]
    fn marker_survives_reinstall_and_edits_to_id_url_and_key() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("models.json");
        let old = Bridge::new(configured(), dir.path().into()).unwrap();
        old.configure_workbuddy(&path).unwrap();
        let mut edited = Document::read(&path).unwrap().entries().unwrap()[0].clone();
        edited["id"] = json!("manually-renamed");
        edited["url"] = json!("https://edited.example/v1");
        edited["apiKey"] = json!("changed-key");
        edited["supportsToolCall"] = json!(false);
        let bridge = Bridge::new(configured(), dir.path().into()).unwrap();
        config::write_json(&path, &json!([edited])).unwrap();
        bridge.configure_workbuddy(&path).unwrap();
        assert_eq!(Document::read(&path).unwrap().entries().unwrap().len(), 1);
        assert_eq!(
            Document::read(&path).unwrap().entries().unwrap()[0]["id"],
            "supplier/model"
        );
        config::write_json(&path, &json!([edited, edited])).unwrap();
        bridge.restore_workbuddy(&path).unwrap();
        assert!(Document::read(&path).unwrap().entries().unwrap().is_empty());
    }
    #[test]
    fn invalid_config_is_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("models.json");
        fs::write(&p, "bad json").unwrap();
        assert!(Document::read(&p).is_err());
        assert_eq!(fs::read_to_string(p).unwrap(), "bad json");
    }
}
