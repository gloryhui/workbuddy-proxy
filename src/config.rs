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

pub const MAX_CONNECTIONS: usize = 64;
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub local_key: String,
    pub port: u16,
    pub connections: Vec<Connection>,
    pub installed: Vec<Installation>,
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
            port: DEFAULT_PORT,
            connections: vec![Connection::default()],
            installed: Vec::new(),
        }
    }
}
impl Default for Connection {
    fn default() -> Self {
        let id = token()[..12].to_owned();
        Self {
            alias: format!("wbp-{id}/{DEFAULT_MODEL}"),
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
        format!("{} · {} · {}", self.name, self.provider, self.model)
    }
    pub fn refresh_alias(&mut self) {
        self.alias = format!("wbp-{}/{}", self.id, self.model);
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
        serde_json::to_writer_pretty(&mut f, value).map_err(|e| e.to_string())?;
        f.write_all(b"\n").map_err(|e| e.to_string())?;
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
    if unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), 1 | 8) } == 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    Ok(())
}
impl Config {
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
        let legacy = value.get("connections").is_none();
        let mut c: Self = if legacy {
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
                port: old.port,
                connections: vec![connection],
                installed: old.installed.into_iter().collect(),
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
            let backup = dir.join(format!("config.pre-v0.2-{}.bak", token()));
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
    pub fn validate(&self) -> Result<(), String> {
        if self.connections.len() > MAX_CONNECTIONS {
            return Err("At most 64 connections / 最多 64 个连接".into());
        }
        let mut ids = std::collections::HashSet::new();
        let mut aliases = std::collections::HashSet::new();
        let mut names = std::collections::HashSet::new();
        for c in &self.connections {
            c.validate()?;
            if !ids.insert(&c.id) || !aliases.insert(&c.alias) || !names.insert(&c.name) {
                return Err(
                    "Connection IDs and list names must be unique / 连接标识和列表名称不可重复"
                        .into(),
                );
            }
        }
        Ok(())
    }
    pub fn connection(&self, id: &str) -> Result<&Connection, String> {
        if id.is_empty() && self.connections.len() == 1 {
            return Ok(&self.connections[0]);
        }
        self.connections
            .iter()
            .find(|c| c.id == id)
            .ok_or("Select a connection / 请选择连接".into())
    }
    pub fn resolve(&self, route: Option<&str>, model: &str) -> Result<Connection, String> {
        if let Some(route) = route {
            let c = self.connection(route)?;
            if model != c.alias && model != c.model {
                return Err("Model does not belong to this connection".into());
            }
            return Ok(c.clone());
        }
        if let Some(c) = self.connections.iter().find(|c| c.alias == model) {
            return Ok(c.clone());
        }
        let mut matching = self.connections.iter().filter(|c| c.model == model);
        let first = matching
            .next()
            .ok_or("Unknown model alias / 未知模型标识")?;
        if matching.next().is_some() {
            return Err("Ambiguous model ID; use the connection alias or its URL / 同名模型请使用独立列表项".into());
        }
        Ok(first.clone())
    }
}
impl Connection {
    pub fn validate(&self) -> Result<(), String> {
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
    fn url_rules_and_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let c = Config::load(dir.path()).unwrap();
        assert_eq!(
            c.connections[0].endpoint(),
            "https://opencode.ai/zen/go/v1/chat/completions"
        );
        let mut d = c.connections[0].clone();
        d.upstream = "https://evil.example/path?key=secret".into();
        assert!(d.validate().is_err());
        d.upstream = "http://127.0.0.1:1234/v1".into();
        assert!(d.validate().is_ok());
        d.upstream = "http://example.com/v1".into();
        assert!(d.validate().is_err());
        assert_eq!(c.connections[0].reasoning_effort, "default");
        d.upstream = DEFAULT_UPSTREAM.into();
        d.reasoning_effort = "ultra".into();
        assert!(d.validate().is_err());
        assert_eq!(c.local_key, Config::load(dir.path()).unwrap().local_key);
    }
    #[test]
    fn legacy_migrates_without_losing_key_or_installation() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("config.json");
        let old = serde_json::json!({"api_key":"secret-fixture","local_key":"keep-local","model":"deepseek-v4.1-flash","upstream":DEFAULT_UPSTREAM,"port":19000,"installed":{"id":"deepseek-v4.1-flash","previous":{"id":"old"}}});
        write_json(&p, &old).unwrap();
        let c = Config::load(dir.path()).unwrap();
        assert_eq!(c.local_key, "keep-local");
        assert_eq!(c.connections[0].api_key, "secret-fixture");
        assert_eq!(c.connections[0].alias, "deepseek-v4.1-flash");
        assert_eq!(c.port, 19000);
        assert_eq!(c.installed.len(), 1);
        let id = c.connections[0].id.clone();
        assert_eq!(Config::load(dir.path()).unwrap().connections[0].id, id);
        assert!(fs::read_dir(dir.path()).unwrap().any(|e| {
            e.unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with("config.pre-v0.2-")
        }));
    }
    #[test]
    fn every_provider_and_model_combination_routes_independently() {
        let mut cfg = Config {
            connections: Vec::new(),
            ..Default::default()
        };
        for (i, (provider, model)) in [
            ("A", "same"),
            ("A", "same"),
            ("A", "different"),
            ("B", "same"),
            ("B", "different"),
        ]
        .into_iter()
        .enumerate()
        {
            let mut c = Connection {
                name: format!("connection {i}"),
                provider: provider.into(),
                model: model.into(),
                ..Default::default()
            };
            c.refresh_alias();
            cfg.connections.push(c);
        }
        cfg.validate().unwrap();
        for c in &cfg.connections {
            assert_eq!(cfg.resolve(None, &c.alias).unwrap().id, c.id);
            assert_eq!(cfg.resolve(Some(&c.id), &c.model).unwrap().id, c.id);
        }
        assert!(cfg.resolve(None, "same").is_err());
        assert!(cfg.resolve(None, "different").is_err());
        assert!(
            cfg.resolve(Some(&cfg.connections[0].id), "different")
                .is_err()
        );
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
