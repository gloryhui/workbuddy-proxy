use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

pub const DEFAULT_MODEL: &str = "deepseek-v4.1-flash";
pub const DEFAULT_UPSTREAM: &str = "https://opencode.ai/zen/go/v1";
pub const DEFAULT_PORT: u16 = 18080;

pub const MAX_PROVIDERS: usize = 64;
pub const MAX_MODELS: usize = 256;
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub local_key: String,
    pub language: String,
    pub port: u16,
    pub providers: Vec<Provider>,
    pub installed: Vec<Installation>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Provider {
    pub id: String,
    pub name: String,
    pub upstream: String,
    pub api_key: String,
    pub models: Vec<Model>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Model {
    pub id: String,
    pub alias: String,
    pub name: String,
    pub model: String,
    pub reasoning_effort: String,
}
impl Default for Provider {
    fn default() -> Self {
        Self {
            id: token()[..12].into(),
            name: "Provider 1".into(),
            upstream: String::new(),
            api_key: String::new(),
            models: Vec::new(),
        }
    }
}
impl Default for Model {
    fn default() -> Self {
        let c = Connection::default();
        Self {
            id: c.id,
            alias: c.alias,
            name: DEFAULT_MODEL.into(),
            model: c.model,
            reasoning_effort: c.reasoning_effort,
        }
    }
}
impl Model {
    pub fn new(model: &str, name: &str) -> Self {
        let mut m = Self {
            model: model.into(),
            name: name.into(),
            ..Default::default()
        };
        m.refresh_alias();
        m
    }
    pub fn refresh_alias(&mut self) {
        self.alias = format!("wbp-{}", self.id);
    }
}
impl Provider {
    pub fn is_draft(&self) -> bool {
        self.upstream.is_empty() && self.api_key.is_empty() && self.models.is_empty()
    }
    pub fn set_model_rows(
        &mut self,
        rows: &[serde_json::Value],
        effort: &str,
    ) -> Result<(), String> {
        if !matches!(effort, "keep" | "default" | "none" | "low" | "high" | "max") {
            return Err("Invalid thinking effort".into());
        }
        if rows.len() > MAX_MODELS {
            return Err("At most 256 models".into());
        }
        let mut old = self.models.clone();
        let mut models = Vec::new();
        for row in rows {
            let id = row["model"].as_str().unwrap_or("").trim();
            let name = row["name"].as_str().unwrap_or("").trim();
            if id.is_empty() {
                return Err("Model ID is required".into());
            }
            let name = if name.is_empty() { id } else { name };
            let route = row["id"].as_str().unwrap_or("");
            let found = old
                .iter()
                .position(|m| m.id == route)
                .or_else(|| old.iter().position(|m| m.model == id && m.name == name))
                .or_else(|| old.iter().position(|m| m.model == id));
            let mut m = found.map_or_else(|| Model::new(id, name), |i| old.remove(i));
            m.name = name.into();
            m.model = id.into();
            if effort != "keep" {
                m.reasoning_effort = effort.into();
            }
            models.push(m);
        }
        self.models = models;
        Ok(())
    }
    pub fn set_models(&mut self, lines: &str, effort: &str) -> Result<(), String> {
        if !matches!(effort, "keep" | "default" | "none" | "low" | "high" | "max") {
            return Err("Invalid thinking effort".into());
        }
        let mut old = self.models.clone();
        let mut models = Vec::new();
        for line in lines.lines().map(str::trim).filter(|s| !s.is_empty()) {
            if models.len() >= MAX_MODELS {
                return Err("最多 256 个模型 / At most 256 models".into());
            }
            let (id, name) = line
                .split_once('|')
                .map_or((line, line), |(a, b)| (a.trim(), b.trim()));
            if id.is_empty() || name.is_empty() {
                return Err("每行填写模型 ID，可用 ID | 名称 / Invalid model line".into());
            }
            let found = old
                .iter()
                .position(|m| m.model == id && m.name == name)
                .or_else(|| old.iter().position(|m| m.model == id));
            let mut m = found.map_or_else(|| Model::new(id, name), |i| old.remove(i));
            m.name = name.into();
            if effort != "keep" {
                m.reasoning_effort = effort.into();
            }
            models.push(m);
        }
        self.models = models;
        Ok(())
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Connection {
    pub id: String,
    pub alias: String,
    pub name: String,
    pub provider: String,
    pub api_key: String,
    pub model: String,
    pub upstream: String,
    pub reasoning_effort: String,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Installation {
    pub id: String,
    pub previous: Option<serde_json::Value>,
    #[serde(default)]
    pub expected: Option<serde_json::Value>,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            local_key: token(),
            language: "auto".into(),
            port: DEFAULT_PORT,
            providers: vec![Provider::default()],
            installed: Vec::new(),
        }
    }
}
impl Default for Connection {
    fn default() -> Self {
        let id = token()[..12].to_owned();
        Self {
            alias: format!("wbp-{id}"),
            id,
            name: "连接 1".into(),
            provider: "OpenCode Go".into(),
            api_key: String::new(),
            model: DEFAULT_MODEL.into(),
            upstream: DEFAULT_UPSTREAM.into(),
            reasoning_effort: "default".into(),
        }
    }
}
impl Connection {
    pub fn label(&self) -> String {
        format!("{} · {}", self.name, self.provider)
    }
}
pub fn token() -> String {
    let mut bytes = [0u8; 24];
    rand::rng().fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
pub fn home() -> Result<PathBuf, String> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .map(PathBuf::from)
        .ok_or("Cannot find user home directory".into())
}
pub fn directory() -> Result<PathBuf, String> {
    if let Some(dir) = std::env::var_os("WORKBUDDY_PROXY_CONFIG_DIR") {
        return Ok(dir.into());
    }
    if cfg!(windows) {
        return std::env::var_os("APPDATA")
            .map(|p| PathBuf::from(p).join("WorkBuddyProxy"))
            .ok_or("APPDATA is missing".into());
    }
    if cfg!(target_os = "macos") {
        return Ok(home()?.join("Library/Application Support/WorkBuddyProxy"));
    }
    Ok(home()?.join(".config/workbuddy-proxy"))
}
pub fn make_private_dir(dir: &Path) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())?;
    }
    Ok(())
}
pub fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    atomic_write(path, |f| {
        serde_json::to_writer_pretty(&mut *f, value).map_err(|e| e.to_string())?;
        f.write_all(b"\n").map_err(|e| e.to_string())
    })
}
pub fn write_bytes(path: &Path, bytes: &[u8]) -> Result<(), String> {
    atomic_write(path, |f| f.write_all(bytes).map_err(|e| e.to_string()))
}
fn atomic_write(
    path: &Path,
    write: impl FnOnce(&mut fs::File) -> Result<(), String>,
) -> Result<(), String> {
    let tmp = path.with_extension(format!("tmp-{}", token()));
    let result = (|| {
        let mut opts = fs::OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut f = opts.open(&tmp).map_err(|e| e.to_string())?;
        write(&mut f)?;
        f.sync_all().map_err(|e| e.to_string())?;
        drop(f);
        replace_file(&tmp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(tmp);
    }
    result
}
#[cfg(not(windows))]
fn replace_file(src: &Path, dst: &Path) -> Result<(), String> {
    fs::rename(src, dst).map_err(|e| e.to_string())
}
#[cfg(windows)]
fn replace_file(src: &Path, dst: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn MoveFileExW(from: *const u16, to: *const u16, flags: u32) -> i32;
    }
    let from: Vec<u16> = src.as_os_str().encode_wide().chain(Some(0)).collect();
    let to: Vec<u16> = dst.as_os_str().encode_wide().chain(Some(0)).collect();
    for attempt in 0..6 {
        if unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), 1 | 8) } != 0 {
            return Ok(());
        }
        let error = std::io::Error::last_os_error();
        if attempt == 5 || !matches!(error.raw_os_error(), Some(5 | 32 | 33)) {
            return Err(error.to_string());
        }
        // Antivirus/file readers may briefly hold the destination during an
        // atomic replace. Keep retries bounded and outside the forwarding path.
        std::thread::sleep(std::time::Duration::from_millis(5));
    }

    Ok(())
}
impl Config {
    // Installation snapshots carry the stable route even after a supplier or
    // model is renamed. Older records can still be matched by their old alias.
    pub fn installed_connection_id(&self, record: &Installation) -> Option<String> {
        let route = record
            .expected
            .as_ref()
            .and_then(|e| e.get("url"))
            .and_then(serde_json::Value::as_str)
            .and_then(|url| reqwest::Url::parse(url).ok())
            .and_then(|url| {
                url.path()
                    .trim_end_matches('/')
                    .strip_prefix("/r/")
                    .and_then(|path| path.strip_suffix("/v1"))
                    .map(str::to_owned)
            });
        self.providers
            .iter()
            .flat_map(|p| &p.models)
            .find(|m| {
                route.as_ref().map_or_else(
                    || record.id == m.alias || record.id == format!("wbp-{}", m.id),
                    |id| id == &m.id,
                )
            })
            .map(|m| m.id.clone())
    }
    // Keep storage/legacy aliases intact; this ID is shared by WorkBuddy sync,
    // model discovery and routing. Reserve old aliases to avoid stealing routes.
    pub fn workbuddy_connections(&self) -> Result<Vec<Connection>, String> {
        use sha2::{Digest, Sha256};
        use std::collections::{HashMap, HashSet};
        let mut connections = self.connections();
        let bases: Vec<_> = connections
            .iter()
            .map(|c| format!("{}/{}", c.provider, c.model))
            .collect();
        let mut counts = HashMap::new();
        let mut owners: HashMap<String, HashSet<String>> = HashMap::new();
        for (c, base) in connections.iter().zip(&bases) {
            *counts.entry(base.clone()).or_insert(0usize) += 1;
            for alias in [c.alias.clone(), format!("wbp-{}", c.id), c.model.clone()] {
                owners.entry(alias).or_default().insert(c.id.clone());
            }
        }
        let mut installed = HashMap::new();
        for record in &self.installed {
            if let Some(id) = self.installed_connection_id(record) {
                owners
                    .entry(record.id.clone())
                    .or_default()
                    .insert(id.clone());
                installed.insert(id, record.id.as_str());
            }
        }
        let mut order: Vec<_> = (0..connections.len()).collect();
        order.sort_by(|&a, &b| connections[a].id.cmp(&connections[b].id));
        for i in order {
            let c = &mut connections[i];
            let base = &bases[i];
            let available = |alias: &str| {
                owners
                    .get(alias)
                    .is_none_or(|ids| ids.iter().all(|id| id == &c.id))
            };
            let digest = format!("{:x}", Sha256::digest(c.id.as_bytes()));
            // A suffix already synced for a duplicate remains fixed even if
            // its sibling is later removed. Renaming still changes the base.
            let previous = installed.get(&c.id).filter(|alias| {
                alias
                    .strip_prefix(&format!("{base}-"))
                    .is_some_and(|suffix| {
                        suffix.len() >= 8
                            && suffix.len() <= 64
                            && suffix.len() % 2 == 0
                            && digest.starts_with(suffix)
                    })
                    && !counts.contains_key(**alias)
                    && available(alias)
            });
            let alias = if let Some(alias) = previous {
                (*alias).to_owned()
            } else if counts[base] == 1 && available(base) {
                base.clone()
            } else {
                (8..=64)
                    .step_by(2)
                    .map(|length| format!("{base}-{}", &digest[..length]))
                    .find(|alias| !counts.contains_key(alias) && available(alias))
                    .ok_or("Cannot generate a unique WorkBuddy model ID")?
            };
            owners
                .entry(alias.clone())
                .or_default()
                .insert(c.id.clone());
            c.alias = alias;
        }
        Ok(connections)
    }
    pub fn load(dir: &Path) -> Result<Self, String> {
        let path = dir.join("config.json");
        if !path.exists() {
            let c = Self::default();
            write_json(&path, &c)?;
            return Ok(c);
        }
        let bytes = fs::read(&path).map_err(|e| e.to_string())?;
        if bytes.len() > 2 * 1024 * 1024 {
            return Err("Configuration is too large".into());
        }
        let value: serde_json::Value = serde_json::from_slice(&bytes)
            .map_err(|_| "Invalid config.json; no file was changed")?;
        if !value.is_object() {
            return Err("Invalid config.json object; no file was changed".into());
        }
        let legacy = value.get("providers").is_none();
        let mut c: Self = if legacy && value.get("connections").is_none() {
            #[derive(Deserialize)]
            #[serde(default)]
            struct Legacy {
                api_key: String,
                local_key: String,
                model: String,
                upstream: String,
                reasoning_effort: String,
                port: u16,
                installed: Option<Installation>,
            }
            impl Default for Legacy {
                fn default() -> Self {
                    Self {
                        api_key: String::new(),
                        local_key: String::new(),
                        model: DEFAULT_MODEL.into(),
                        upstream: DEFAULT_UPSTREAM.into(),
                        reasoning_effort: "default".into(),
                        port: DEFAULT_PORT,
                        installed: None,
                    }
                }
            }
            let old: Legacy = serde_json::from_value(value)
                .map_err(|_| "Invalid legacy config.json; no file was changed")?;
            let connection = Connection {
                alias: old.model.clone(),
                api_key: old.api_key,
                model: old.model,
                upstream: old.upstream,
                reasoning_effort: old.reasoning_effort,
                ..Default::default()
            };
            Self {
                local_key: old.local_key,
                language: "auto".into(),
                port: old.port,
                providers: Self::group(vec![connection]),
                installed: old.installed.into_iter().collect(),
            }
        } else if legacy {
            #[derive(Deserialize)]
            struct Flat {
                local_key: String,
                port: u16,
                connections: Vec<Connection>,
                #[serde(default)]
                installed: Vec<Installation>,
            }
            let old: Flat = serde_json::from_value(value)
                .map_err(|_| "Invalid v0.2 config.json; no file was changed")?;
            Self {
                local_key: old.local_key,
                language: "auto".into(),
                port: old.port,
                providers: Self::group(old.connections),
                installed: old.installed,
            }
        } else {
            serde_json::from_value(value).map_err(|_| "Invalid config.json; no file was changed")?
        };
        let missing_key = c.local_key.is_empty();
        if missing_key {
            c.local_key = token();
        }
        c.validate()?;
        if legacy {
            let backup = dir.join(format!("config.pre-v0.3-{}.bak", token()));
            fs::copy(&path, &backup).map_err(|e| e.to_string())?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(backup, fs::Permissions::from_mode(0o600))
                    .map_err(|e| e.to_string())?;
            }
        }
        if legacy || missing_key {
            write_json(&path, &c)?;
        }
        Ok(c)
    }
    fn group(connections: Vec<Connection>) -> Vec<Provider> {
        let mut providers: Vec<Provider> = Vec::new();
        for c in connections {
            let found = providers.iter().position(|p| {
                p.name == c.provider && p.upstream == c.upstream && p.api_key == c.api_key
            });
            let i = found.unwrap_or_else(|| {
                providers.push(Provider {
                    name: c.provider.clone(),
                    upstream: c.upstream.clone(),
                    api_key: c.api_key.clone(),
                    ..Default::default()
                });
                providers.len() - 1
            });
            providers[i].models.push(Model {
                id: c.id,
                alias: c.alias,
                name: c.name,
                model: c.model,
                reasoning_effort: c.reasoning_effort,
            });
        }
        let mut names = std::collections::HashSet::new();
        for p in &mut providers {
            let root = p.name.clone();
            let mut n = 2;
            while !names.insert(p.name.clone()) {
                p.name = format!("{root} {n}");
                n += 1;
            }
        }
        providers
    }
    pub fn connections(&self) -> Vec<Connection> {
        self.providers
            .iter()
            .flat_map(|p| {
                p.models.iter().map(move |m| Connection {
                    id: m.id.clone(),
                    alias: m.alias.clone(),
                    name: m.name.clone(),
                    model: m.model.clone(),
                    reasoning_effort: m.reasoning_effort.clone(),
                    provider: p.name.clone(),
                    upstream: p.upstream.clone(),
                    api_key: p.api_key.clone(),
                })
            })
            .collect()
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.providers.len() > MAX_PROVIDERS
            || self.providers.iter().map(|p| p.models.len()).sum::<usize>() > MAX_MODELS
        {
            return Err("最多 64 个供应商、合计 256 个模型 / Configuration limit exceeded".into());
        }
        let mut ids = std::collections::HashSet::new();
        let mut aliases = std::collections::HashSet::new();
        let mut names = std::collections::HashSet::new();
        for p in &self.providers {
            let c = Connection {
                id: p.id.clone(),
                name: p.name.clone(),
                provider: p.name.clone(),
                upstream: p.upstream.clone(),
                api_key: p.api_key.clone(),
                ..Default::default()
            };
            c.validate_fields(p.is_draft())?;
            if !ids.insert(p.id.clone()) || !names.insert(&p.name) {
                return Err("供应商名称和标识不可重复 / Duplicate provider".into());
            }
            let mut model_names = std::collections::HashSet::new();
            for m in &p.models {
                let c = Connection {
                    id: m.id.clone(),
                    alias: m.alias.clone(),
                    name: m.name.clone(),
                    model: m.model.clone(),
                    reasoning_effort: m.reasoning_effort.clone(),
                    ..c.clone()
                };
                c.validate()?;
                if !ids.insert(m.id.clone())
                    || !aliases.insert(m.alias.clone())
                    || (m.alias != format!("wbp-{}", m.id)
                        && !aliases.insert(format!("wbp-{}", m.id)))
                    || !model_names.insert(&m.name)
                {
                    return Err("模型标识和同供应商内的显示名称不可重复 / Duplicate model".into());
                }
            }
        }
        Ok(())
    }
    pub fn provider(&self, id: &str) -> Result<&Provider, String> {
        if id.is_empty() && self.providers.len() == 1 {
            return Ok(&self.providers[0]);
        }
        self.providers
            .iter()
            .find(|p| p.id == id || p.models.iter().any(|m| m.id == id))
            .ok_or("请选择供应商 / Select a provider".into())
    }
    pub fn connection(&self, id: &str) -> Result<Connection, String> {
        let p = self.provider(id)?;
        let m = p
            .models
            .iter()
            .find(|m| m.id == id)
            .or_else(|| p.models.first())
            .ok_or("请先添加模型 / Add a model first")?;
        Ok(Connection {
            id: m.id.clone(),
            alias: m.alias.clone(),
            name: m.name.clone(),
            model: m.model.clone(),
            reasoning_effort: m.reasoning_effort.clone(),
            provider: p.name.clone(),
            upstream: p.upstream.clone(),
            api_key: p.api_key.clone(),
        })
    }
}

impl Connection {
    pub fn validate(&self) -> Result<(), String> {
        self.validate_fields(false)
    }
    fn validate_fields(&self, draft: bool) -> Result<(), String> {
        if self.id.len() < 4
            || self.id.len() > 64
            || !self
                .id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        {
            return Err("Invalid connection ID".into());
        }
        for v in [&self.name, &self.provider] {
            if v.trim().is_empty() || v.len() > 240 || v.chars().any(char::is_control) {
                return Err("Enter a list name and provider / 请填写列表名称和供应商".into());
            }
        }
        if self.alias.is_empty()
            || self.alias.len() > 256
            || self.alias.chars().any(char::is_control)
        {
            return Err("Invalid model alias".into());
        }
        if self.model.trim().is_empty()
            || self.model.len() > 160
            || self.model.chars().any(char::is_control)
        {
            return Err("Invalid model name".into());
        }
        if !matches!(
            self.reasoning_effort.as_str(),
            "default" | "none" | "low" | "high" | "max"
        ) {
            return Err("Invalid thinking effort / 无效的思考强度".into());
        }
        if self.api_key.len() > 4096 || self.api_key.chars().any(char::is_control) {
            return Err("Invalid API key".into());
        }
        if self.upstream.len() > 2048 {
            return Err("Upstream URL is too long".into());
        }
        if draft {
            return Ok(());
        }
        let u = reqwest::Url::parse(&self.upstream).map_err(|_| "Invalid upstream URL")?;
        let loopback = matches!(
            u.host_str(),
            Some("127.0.0.1" | "localhost" | "[::1]" | "::1")
        );
        if u.scheme() != "https" && !(u.scheme() == "http" && loopback) {
            return Err("Upstream must use HTTPS (HTTP is allowed only on loopback)".into());
        }
        if !u.username().is_empty()
            || u.password().is_some()
            || u.query().is_some()
            || u.fragment().is_some()
        {
            return Err("URL must not contain credentials, query or fragment".into());
        }
        Ok(())
    }
    pub fn endpoint(&self) -> String {
        format!(
            "{}/chat/completions",
            self.upstream
                .trim_end_matches('/')
                .trim_end_matches("/chat/completions")
                .trim_end_matches('/')
        )
    }
    pub fn is_go(&self) -> bool {
        reqwest::Url::parse(&self.upstream)
            .is_ok_and(|u| u.host_str() == Some("opencode.ai") && u.path().starts_with("/zen/go"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn readable_ids_are_unique_and_duplicate_suffixes_are_stable() {
        let mut cfg = Config::default();
        cfg.providers[0].name = "opencode".into();
        cfg.providers[0]
            .set_models("same | First\nsame | Second\nother", "default")
            .unwrap();
        let connections = cfg.workbuddy_connections().unwrap();
        assert_eq!(connections[2].alias, "opencode/other");
        for c in &connections[..2] {
            let suffix = c.alias.strip_prefix("opencode/same-").unwrap();
            assert_eq!(suffix.len(), 8);
            assert!(suffix.bytes().all(|b| b.is_ascii_hexdigit()));
        }
        assert_ne!(connections[0].alias, connections[1].alias);
        cfg.providers[0].models.reverse();
        for c in cfg.workbuddy_connections().unwrap() {
            assert_eq!(
                c.alias,
                connections.iter().find(|old| old.id == c.id).unwrap().alias
            );
        }
        let c = &connections[0];
        cfg.installed.push(Installation {
            id: c.alias.clone(),
            previous: None,
            expected: Some(
                serde_json::json!({"url":format!("http://127.0.0.1:18080/r/{}/v1", c.id)}),
            ),
        });
        cfg.providers[0].models.retain(|m| m.id == c.id);
        assert_eq!(cfg.workbuddy_connections().unwrap()[0].alias, c.alias);
        cfg.providers[0].name = "中文供应商 / 日常".into();
        assert_eq!(
            cfg.workbuddy_connections().unwrap()[0].alias,
            "中文供应商 / 日常/same"
        );
    }
    #[test]
    fn readable_ids_reserve_other_bases_and_legacy_aliases() {
        let mut cfg = Config::default();
        cfg.providers[0].name = "provider".into();
        cfg.providers[0]
            .set_models("same | First\nsame | Second", "default")
            .unwrap();
        let original = cfg.workbuddy_connections().unwrap();
        let model = original[0].alias.strip_prefix("provider/").unwrap();
        cfg.providers[0].models.push(Model::new(model, "Collision"));
        let result = cfg.workbuddy_connections().unwrap();
        assert_eq!(result[2].alias, original[0].alias);
        assert_ne!(result[0].alias, result[2].alias);
        assert!(result[0].alias.starts_with(&original[0].alias));
        cfg.providers[0].models[2].alias = "provider/other".into();
        cfg.providers[0]
            .models
            .push(Model::new("other", "Legacy collision"));
        let result = cfg.workbuddy_connections().unwrap();
        assert!(result[3].alias.starts_with("provider/other-"));
        let mut ids = std::collections::HashSet::new();
        assert!(result.iter().all(|c| ids.insert(&c.alias)));
    }
    #[test]
    fn migration_groups_credentials_and_keeps_routes() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("config.json");
        let mut a = Connection {
            name: "one".into(),
            api_key: "fixture".into(),
            ..Default::default()
        };
        a.alias = format!("wbp-{}/{}", a.id, a.model);
        let mut b = a.clone();
        b.id = "second-route".into();
        b.name = "two".into();
        b.alias = format!("wbp-{}/{}", b.id, b.model);
        let mut c = b.clone();
        c.id = "third-route".into();
        c.api_key = "other-key".into();
        c.alias = format!("wbp-{}/{}", c.id, c.model);
        let old = serde_json::json!({"local_key":"keep","port":19000,"connections":[a,b,c],"installed":[{"id":"saved","previous":null}]});
        write_json(&p, &old).unwrap();
        let cfg = Config::load(dir.path()).unwrap();
        assert_eq!(cfg.providers.len(), 2);
        assert_eq!(cfg.providers[0].models.len(), 2);
        assert_eq!(cfg.connections()[0].alias, a.alias);
        assert_eq!(cfg.connections()[1].id, b.id);
        assert_eq!(cfg.local_key, "keep");
        assert_eq!(cfg.port, 19000);
        assert_eq!(cfg.installed.len(), 1);
        assert_eq!(
            Config::load(dir.path()).unwrap().providers[0].id,
            cfg.providers[0].id
        );
        assert!(fs::read_dir(dir.path()).unwrap().any(|e| {
            e.unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with("config.pre-v0.3-")
        }));
    }
    #[test]
    fn legacy_migration_and_model_edits_keep_aliases() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("config.json");
        write_json(
            &p,
            &serde_json::json!({"api_key":"fixture","local_key":"keep","model":DEFAULT_MODEL}),
        )
        .unwrap();
        let mut cfg = Config::load(dir.path()).unwrap();
        assert_eq!(cfg.connections()[0].alias, DEFAULT_MODEL);
        let p = &mut cfg.providers[0];
        let id = p.models[0].id.clone();
        p.set_models(&format!("{DEFAULT_MODEL} | Renamed\nother-model"), "keep")
            .unwrap();
        assert_eq!(p.models[0].id, id);
        assert_eq!(p.models[0].alias, DEFAULT_MODEL);
        assert_eq!(p.models.len(), 2);
        cfg.validate().unwrap();
    }
    #[test]
    fn empty_provider_drafts_have_no_example_url_and_cannot_route_models() {
        let mut cfg = Config::default();
        assert!(cfg.providers[0].is_draft());
        assert!(cfg.providers[0].upstream.is_empty());
        cfg.validate().unwrap();
        cfg.providers[0].name.clear();
        assert!(cfg.validate().is_err());
        cfg.providers[0].name = "New provider".into();
        cfg.providers[0]
            .models
            .push(Model::new("fixture-model", "Fixture"));
        assert!(cfg.validate().is_err());
    }
    #[test]
    fn limits_and_url_validation() {
        let mut cfg = Config::default();
        cfg.providers[0].upstream = "https://example.com/v1".into();
        cfg.providers[0]
            .set_models("same | One\nsame | Two\nother", "default")
            .unwrap();
        cfg.validate().unwrap();
        cfg.providers[0].upstream = "https://example.com/v1?key=secret".into();
        assert!(cfg.validate().is_err());
        cfg.providers[0].upstream = "http://127.0.0.1:1234/v1".into();
        cfg.validate().unwrap();
        cfg.providers[0].models[1].name = "One".into();
        assert!(cfg.validate().is_err());
    }
    #[test]
    fn atomic_replacement() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("config.json");
        write_json(&p, &vec![1]).unwrap();
        write_json(&p, &vec![2]).unwrap();
        assert_eq!(fs::read_to_string(p).unwrap().trim(), "[\n  2\n]");
    }
}
