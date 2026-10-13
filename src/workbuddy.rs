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
impl Bridge {
    pub fn configure_workbuddy(&self, path: &Path) -> Result<(), String> {
        let mut cfg = self.config.lock().unwrap();
        if cfg.api_key.is_empty() {
            return Err("Save and test upstream key first / 请先保存并测试 Key".into());
        }
        if cfg.installed.as_ref().is_some_and(|i| i.id != cfg.model) {
            return Err(
                "Restore the previous model before changing model ID / 更换模型前请恢复原配置"
                    .into(),
            );
        }
        let mut d = Document::read(path)?;
        let matches: Vec<usize> = d
            .entries()
            .unwrap()
            .iter()
            .enumerate()
            .filter(|(_, v)| v.get("id").and_then(Value::as_str) == Some(&cfg.model))
            .map(|(i, _)| i)
            .collect();
        if matches.len() > 1 {
            return Err("Duplicate model IDs; refusing to overwrite".into());
        }
        let mut next = cfg.clone();
        if next.installed.is_none() {
            next.installed = Some(Installation {
                id: cfg.model.clone(),
                previous: matches.first().map(|i| d.entries().unwrap()[*i].clone()),
            })
        }
        let entry = json!({"id":cfg.model,"name":format!("WorkBuddy Proxy · {}",cfg.model),"vendor":"Custom","url":self.address(),"apiKey":cfg.local_key,"supportsToolCall":true,"supportsImages":false,"supportsReasoning":false,"useCustomProtocol":false});
        let list = d.entries_mut();
        if let Some(i) = matches.first() {
            list[*i] = entry
        } else {
            list.push(entry)
        }
        config::make_private_dir(path.parent().ok_or("Invalid configuration path")?)?;
        backup(path)?;
        config::write_json(&self.dir.join("config.json"), &next)?;
        *cfg = next;
        config::write_json(path, &d.root)
    }
    pub fn restore_workbuddy(&self, path: &Path) -> Result<(), String> {
        let mut cfg = self.config.lock().unwrap();
        let installation = cfg
            .installed
            .as_ref()
            .ok_or("No WorkBuddy configuration to restore / 无需恢复")?;
        let mut d = Document::read(path)?;
        let indices: Vec<usize> = d
            .entries()
            .unwrap()
            .iter()
            .enumerate()
            .filter(|(_, v)| v.get("id").and_then(Value::as_str) == Some(&installation.id))
            .map(|(i, _)| i)
            .collect();
        if indices.len() != 1 {
            return Err("Model was removed or duplicated; restore from backup manually".into());
        }
        let i = indices[0];
        let current = &d.entries().unwrap()[i];
        if current.get("url").and_then(Value::as_str) != Some(self.address().as_str())
            || current.get("apiKey").and_then(Value::as_str) != Some(&cfg.local_key)
        {
            return Err("This model has been edited elsewhere; refusing to overwrite / 模型已被其他操作修改，请使用备份恢复".into());
        }
        if let Some(previous) = &installation.previous {
            d.entries_mut()[i] = previous.clone()
        } else {
            d.entries_mut().remove(i);
        }
        backup(path)?;
        config::write_json(path, &d.root)?;
        let mut next = cfg.clone();
        next.installed = None;
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
    fn preserve_unrelated_changes_and_restore() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("models.json");
        let old = json!({"otherField":true,"models":[{"id":"other","apiKey":"keep"},{"id":"deepseek-v4.1-flash","url":"https://opencode.ai/zen/go/v1","apiKey":"original"}]});
        config::write_json(&p, &old).unwrap();
        let c = config::Config {
            api_key: "test-upstream".into(),
            ..Default::default()
        };
        let a = Bridge::new(c, dir.path().into()).unwrap();
        a.port.store(18080, std::sync::atomic::Ordering::Relaxed);
        a.configure_workbuddy(&p).unwrap();
        let mut d = Document::read(&p).unwrap();
        d.entries_mut().push(json!({"id":"new","apiKey":"new"}));
        config::write_json(&p, &d.root).unwrap();
        a.restore_workbuddy(&p).unwrap();
        let restored = Document::read(&p).unwrap();
        assert_eq!(restored.entries().unwrap()[1], old["models"][1]);
        assert_eq!(restored.entries().unwrap().len(), 3);
        assert_eq!(restored.root["otherField"], true);
        a.configure_workbuddy(&p).unwrap();
        let mut d = Document::read(&p).unwrap();
        d.entries_mut()[1]["url"] = json!("https://changed.example");
        config::write_json(&p, &d.root).unwrap();
        assert!(a.restore_workbuddy(&p).is_err());
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
