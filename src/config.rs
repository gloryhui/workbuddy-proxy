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

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub api_key: String,
    pub local_key: String,
    pub model: String,
    pub upstream: String,
    pub port: u16,
    pub installed: Option<Installation>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Installation {
    pub id: String,
    pub previous: Option<serde_json::Value>,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            local_key: token(),
            model: DEFAULT_MODEL.into(),
            upstream: DEFAULT_UPSTREAM.into(),
            port: DEFAULT_PORT,
            installed: None,
        }
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
        let bytes = fs::read(path).map_err(|e| e.to_string())?;
        if bytes.len() > 65536 {
            return Err("Configuration is too large".into());
        }
        let mut c: Self = serde_json::from_slice(&bytes)
            .map_err(|_| "Invalid config.json; no file was changed")?;
        if c.local_key.is_empty() {
            c.local_key = token();
            write_json(&dir.join("config.json"), &c)?;
        }
        c.validate()?;
        Ok(c)
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.model.trim().is_empty()
            || self.model.len() > 160
            || self.model.chars().any(char::is_control)
        {
            return Err("Invalid model name".into());
        }
        if self.api_key.len() > 4096 || self.api_key.chars().any(char::is_control) {
            return Err("Invalid API key".into());
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
            c.endpoint(),
            "https://opencode.ai/zen/go/v1/chat/completions"
        );
        let mut d = c.clone();
        d.upstream = "https://evil.example/path?key=secret".into();
        assert!(d.validate().is_err());
        d.upstream = "http://127.0.0.1:1234/v1".into();
        assert!(d.validate().is_ok());
        d.upstream = "http://example.com/v1".into();
        assert!(d.validate().is_err());
        assert_eq!(c.local_key, Config::load(dir.path()).unwrap().local_key);
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
