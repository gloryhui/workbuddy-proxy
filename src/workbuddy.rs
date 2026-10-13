use crate::{
    config::{self, Installation},
    proxy::Bridge,
};
use serde_json::{Value, json};
use std::{
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
fn backup(path: &Path) -> Result<(), String> {
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
fn matching(d: &Document, id: &str) -> Result<Option<usize>, String> {
    let indices: Vec<_> = d
        .entries()
        .unwrap()
        .iter()
        .enumerate()
        .filter(|(_, v)| v.get("id").and_then(Value::as_str) == Some(id))
        .map(|(i, _)| i)
        .collect();
    if indices.len() > 1 {
        return Err("Duplicate model IDs; refusing to overwrite".into());
    }
    Ok(indices.first().copied())
}
fn owned(current: &Value, record: &Installation, address: &str, key: &str) -> bool {
    if let Some(expected) = &record.expected {
        return current == expected;
    }
    current.get("url").and_then(Value::as_str) == Some(address)
        && current.get("apiKey").and_then(Value::as_str) == Some(key)
}
fn entry(c: &config::Connection, url: &str, key: &str) -> Value {
    let mut e = json!({"id":c.alias,"name":c.label(),"vendor":"Custom","url":url,"apiKey":key,"supportsToolCall":true,"supportsImages":false,"useCustomProtocol":false});
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
impl Bridge {
    pub fn configure_workbuddy(&self, path: &Path) -> Result<usize, String> {
        let mut cfg = self.config.lock().unwrap();
        let mut d = Document::read(path)?;
        let mut connections = cfg.connections();
        for c in &mut connections {
            c.alias = format!("wbp-{}", c.id);
        }
        let active: Vec<_> = connections
            .iter()
            .filter(|c| !c.api_key.is_empty())
            .collect();
        if active.is_empty() && cfg.installed.is_empty() {
            return Err("Save at least one connection first / 请先保存至少一个连接".into());
        }
        // Preflight the complete batch before touching any file or model.
        for record in &cfg.installed {
            if let Some(i) = matching(&d, &record.id)?
                && !owned(
                    &d.entries().unwrap()[i],
                    record,
                    &self.address(),
                    &cfg.local_key,
                )
            {
                return Err("A proxy model was edited elsewhere; restore from backup / 列表项已被其他操作修改，请使用备份处理".into());
            }
        }
        let mut next = cfg.clone();
        next.installed.clear();
        for record in &cfg.installed {
            if !active.iter().any(|c| c.alias == record.id)
                && let Some(i) = matching(&d, &record.id)?
            {
                if let Some(previous) = &record.previous {
                    d.entries_mut()[i] = previous.clone();
                } else {
                    d.entries_mut().remove(i);
                }
            }
        }
        for c in &active {
            let i = matching(&d, &c.alias)?;
            let old = cfg.installed.iter().find(|r| r.id == c.alias);
            // Generated IDs must not overwrite a coincidental user-created ID.
            if i.is_some() && old.is_none() {
                return Err(
                    "Model alias already exists in WorkBuddy; refusing to overwrite".into(),
                );
            }
            let e = entry(c, &self.connection_address(&c.id), &cfg.local_key);
            next.installed.push(Installation {
                id: c.alias.clone(),
                previous: old.and_then(|r| r.previous.clone()),
                expected: Some(e.clone()),
            });
            if let Some(i) = i {
                d.entries_mut()[i] = e;
            } else {
                d.entries_mut().push(e);
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
        if cfg.installed.is_empty() {
            return Err("No WorkBuddy entries to restore / 无需恢复".into());
        }
        let mut d = Document::read(path)?;
        for record in &cfg.installed {
            let i = matching(&d, &record.id)?
                .ok_or("Model was removed; restore manually from backup")?;
            if !owned(
                &d.entries().unwrap()[i],
                record,
                &self.address(),
                &cfg.local_key,
            ) {
                return Err("This model has been edited elsewhere; refusing to overwrite / 列表项已被其他操作修改".into());
            }
        }
        for record in &cfg.installed {
            let i = matching(&d, &record.id)?.unwrap();
            if let Some(previous) = &record.previous {
                d.entries_mut()[i] = previous.clone();
            } else {
                d.entries_mut().remove(i);
            }
        }
        backup(path)?;
        config::write_json(path, &d.root)?;
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
    fn sync_multiple_preserves_unrelated_and_restores() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("models.json");
        let old = json!({"otherField":true,"models":[{"id":"other","apiKey":"keep"}]});
        config::write_json(&p, &old).unwrap();
        let mut c = config::Config::default();
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
        let bytes = fs::read(&p).unwrap();
        assert!(a.configure_workbuddy(&p).is_err());
        assert!(a.restore_workbuddy(&p).is_err());
        assert_eq!(fs::read(&p).unwrap(), bytes);
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
