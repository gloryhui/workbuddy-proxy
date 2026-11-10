//! 配置模型、磁盘位置和原子文件写入。
//!
//! `Config` 是代理路由与设置界面的共享磁盘契约；加载后的一份内存快照由
//! `proxy::Bridge::config` 的 `Mutex` 保护，`Routes` 再从它构建不可变路由索引。
//! 普通配置写入会先写同目录临时文件、`fsync`，再替换目标文件。这个保证是单文件级别
//! 的，不是跨多个文件的数据库事务，也不保证目录项已经同步到持久介质。
//!
//! 上游是设置控制命令和 `config.json`；下游是路由、WorkBuddy 模型列表、目录发现和
//! 统计定价。`Option::None` 通常表示“未显式配置”或“未知”，不能统一当作数值 0。
use crate::capabilities::ModelCapabilities;
use crate::upstream_config::{ProtocolSettings, UpstreamProtocol};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

/// 新连接草稿使用的默认模型 ID，不保证每个供应商都支持。
pub const DEFAULT_MODEL: &str = "deepseek-v4.1-flash";
pub const DEFAULT_UPSTREAM: &str = "https://opencode.ai/zen/go/v1";
/// 默认监听端口；实际值还可能被 `--port` 或端口热切换覆盖。
pub const DEFAULT_PORT: u16 = 18080;

pub const MAX_PROVIDERS: usize = 64;
pub const MAX_MODELS: usize = 256;
/// 持久化设置根对象。
///
/// `providers` 是当前版本的主存储；顶层没有 `providers`/`connections` 的旧 JSON
/// 会先经过迁移。配置对象被克隆用于“构造下一版快照”，但 `Clone` 只表示 Rust
/// 复制语义，不能据名称推断所有嵌套资源都会深复制。
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// 客户端访问本地模型端点使用的 bearer key，不是控制管理 key。
    pub local_key: String,
    pub language: String,
    pub port: u16,
    pub autostart: bool,
    /// 同一供应商共享地址、协议、设置和凭据，其下各模型分别形成路由。
    pub providers: Vec<Provider>,
    /// WorkBuddy 已安装条目的审计/恢复快照，用于保留旧路由和回滚同步。
    pub installed: Vec<Installation>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Provider {
    /// 供应商稳定标识；路由作用域和统计 provider_id 都引用它。
    pub id: String,
    pub name: String,
    pub upstream: String,
    pub protocol: UpstreamProtocol,
    pub protocol_settings: ProtocolSettings,
    pub api_key: String,
    pub models: Vec<Model>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Model {
    /// 模型路由稳定标识，也是 `/r/{id}` 中的 id。
    pub id: String,
    /// 持久化模型条目的内部兼容 alias。`workbuddy_connections` 另行生成
    /// `Connection` 投影的可读 alias，不会用投影值重写这里的磁盘字段。
    pub alias: String,
    pub name: String,
    pub model: String,
    pub reasoning_effort: String,
    /// `None` 表示没有逐模型覆盖；目录元数据仍可能提供自动能力。
    pub capabilities: Option<ModelCapabilities>,
}
impl Default for Provider {
    fn default() -> Self {
        Self {
            id: token()[..12].into(),
            name: "Provider 1".into(),
            upstream: String::new(),
            protocol: Default::default(),
            protocol_settings: Default::default(),
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
            capabilities: c.capabilities,
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
        // 这是内部兼容别名，不代表所有 WorkBuddy 版本最终显示的稳定 ID。
        self.alias = format!("wbp-{}", self.id);
    }
}
impl Provider {
    /// 尚未录入上游/凭据且没有模型时，草稿允许暂时通过部分字段校验。
    pub fn is_draft(&self) -> bool {
        self.upstream.is_empty() && self.api_key.is_empty() && self.models.is_empty()
    }
    /// 用设置界面提交的模型行替换当前模型列表。
    ///
    /// 输入：`rows` 是有上限的 JSON 行；`effort` 是 `keep/default/none/low/high/max`
    /// 之一。输出：成功时整体替换 `self.models`，失败时原列表不变。复用顺序是
    /// route id、`(model, name)`、上游 model，因此改名时尽量保留稳定路由 id。
    ///
    /// 失败来源包括未知思考强度、模型超过上限、缺少 model、无效 capabilities 或
    /// capabilities 校验失败。本函数不写磁盘；调用方还要校验整份 `Config` 后提交。
    pub fn set_model_rows(
        &mut self,
        rows: &[serde_json::Value],
        effort: &str,
    ) -> Result<(), String> {
        // 行式输入主要来自新版设置窗口。优先按已有路由 id 复用模型，其次按
        // `(model, name)`、最后按上游 model 复用，使编辑显示名时尽量保留稳定 id。
        // 解析只写局部 `models`，全部成功后才覆盖 `self.models`；中途错误不提交
        // 半份列表。
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
            if let Some(value) = row.get("capabilities") {
                m.capabilities = if value.is_null() {
                    None
                } else {
                    let capabilities: ModelCapabilities = serde_json::from_value(value.clone())
                        .map_err(|_| "Invalid model capabilities / 模型能力配置无效")?;
                    capabilities.validate()?;
                    Some(capabilities)
                };
            }
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
    /// 路由级视图：由 provider 与 model 合并而来。
    pub id: String,
    pub alias: String,
    pub name: String,
    pub provider: String,
    pub api_key: String,
    pub model: String,
    pub upstream: String,
    pub protocol: UpstreamProtocol,
    pub protocol_settings: ProtocolSettings,
    pub reasoning_effort: String,
    /// `None` 表示未知/未覆盖，不表示“所有能力都为 0”。
    pub capabilities: Option<ModelCapabilities>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Installation {
    /// WorkBuddy 旧/当前模型条目 ID；恢复时用它定位要删除或恢复的行。
    pub id: String,
    /// 安装前的完整 JSON 值；`None` 表示该条目原本不存在。
    pub previous: Option<serde_json::Value>,
    /// 期望的安装结果，可用于把旧别名关联回稳定路由 id。
    #[serde(default)]
    pub expected: Option<serde_json::Value>,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            local_key: token(),
            language: "auto".into(),
            port: DEFAULT_PORT,
            autostart: false,
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
            protocol: Default::default(),
            protocol_settings: Default::default(),
            reasoning_effort: "default".into(),
            capabilities: None,
        }
    }
}
impl Connection {
    pub fn label(&self) -> String {
        format!("{} · {}", self.name, self.provider)
    }
}
pub fn token() -> String {
    // 192 位随机值，返回 48 个小写十六进制字符。部分调用点会按用途截断；
    // 这不是密码哈希，也没有额外 KDF，随机性来自操作系统随机源。
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
    // 环境变量优先，主要服务 `--config-dir`、测试隔离和打包后的控制器。
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
    // 临时文件与目标文件同目录，避免跨文件系统 rename。Unix 以 0600 创建，
    // 写入后 `sync_all`，再替换目标；失败时尽力删除本函数创建的临时文件。
    // 单次目标替换之外没有跨文件事务或目录 fsync 保证。
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
    /// 计算向 WorkBuddy 投影的 `Connection` 列表及其可读 alias。
    ///
    /// 输入是当前 provider/model 快照和 `installed` 审计记录；输出是每个模型一份
    /// `Connection`。这里的 alias 是投影值，不回写 `Provider.models[].alias`。当
    /// `provider/model` 基名唯一时直接使用，冲突时追加由稳定 route id 得到的哈希
    /// 后缀；已同步过的同基名后缀和已安装旧 ID 会被尝试保留。
    ///
    /// 失败发生在无法生成不冲突的 alias 时。本函数只做纯内存计算，不修改
    /// WorkBuddy 文件；文件同步与回滚属于调用方。
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
    /// 从配置目录读取当前格式，或迁移受支持的旧格式。
    ///
    /// 输入缺失时创建默认配置；已有文件限制为 2 MiB。输出是已校验的 `Config`。
    /// 解析或校验失败会返回简短原因，并在覆盖 `config.json` 之前结束。旧格式成功时
    /// 先复制 `config.pre-v0.3-*.bak`，缺少 `local_key` 时补齐，再原子写回新格式。
    ///
    /// 这是单文件加载/迁移流程，不是多个配置文件的联合事务。
    pub fn load(dir: &Path) -> Result<Self, String> {
        // 加载分三步：解析旧格式、校验并按需补 `local_key`、最后只在确实迁移/补 key
        // 时回写。旧格式先复制备份，再原子写新格式；解析或校验失败都发生在覆盖
        // `config.json` 之前。
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
                autostart: false,
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
                autostart: false,
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
                p.name == c.provider
                    && p.upstream == c.upstream
                    && p.api_key == c.api_key
                    && p.protocol == c.protocol
                    && p.protocol_settings == c.protocol_settings
            });
            let i = found.unwrap_or_else(|| {
                providers.push(Provider {
                    name: c.provider.clone(),
                    upstream: c.upstream.clone(),
                    api_key: c.api_key.clone(),
                    protocol: c.protocol,
                    protocol_settings: c.protocol_settings.clone(),
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
                capabilities: c.capabilities,
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
                    capabilities: m.capabilities.clone(),
                    provider: p.name.clone(),
                    upstream: p.upstream.clone(),
                    api_key: p.api_key.clone(),
                    protocol: p.protocol,
                    protocol_settings: p.protocol_settings.clone(),
                })
            })
            .collect()
    }
    /// 校验数量上限、跨条目唯一性和每个连接的字段/协议约束。
    ///
    /// 成功返回 `Ok(())`；失败返回第一条被发现的问题。校验只读内存，不写磁盘。模型
    /// 显示名只要求在同一供应商内唯一，provider id、model id 和 alias 则跨全局检查。
    pub fn validate(&self) -> Result<(), String> {
        // 全局唯一性检查覆盖 supplier/model id 和 alias；兼容别名 `wbp-{id}` 也会
        // 预留，避免新旧客户端用不同写法规避同名冲突。
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
                protocol: p.protocol,
                protocol_settings: p.protocol_settings.clone(),
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
                    capabilities: m.capabilities.clone(),
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
            capabilities: m.capabilities.clone(),
            provider: p.name.clone(),
            upstream: p.upstream.clone(),
            api_key: p.api_key.clone(),
            protocol: p.protocol,
            protocol_settings: p.protocol_settings.clone(),
        })
    }
}

impl Connection {
    pub fn validate(&self) -> Result<(), String> {
        self.validate_fields(false)
    }
    /// 校验一个 `Connection` 的结构字段。
    ///
    /// `draft=true` 只跳过最终上游 URL 的可用性检查，名称、id、alias、model、思考
    /// 强度、API key 长度/控制字符和协议设置仍会检查。失败时不修改任何字段。
    fn validate_fields(&self, draft: bool) -> Result<(), String> {
        // `draft` 只放宽 URL 等尚未填写完成的字段；标识、名称、思考强度和凭据的
        // 基础结构仍会检查。
        self.protocol_settings.validate(self.protocol)?;
        if let Some(capabilities) = &self.capabilities {
            capabilities.validate()?;
        }
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
        self.upstream_endpoint(false)?;
        Ok(())
    }
    /// 根据连接协议、上游 base、模型名和 stream 标志构造最终 URL。
    ///
    /// 失败来自协议设置校验、URL/base 规则或 Gemini 模型名编码。函数只计算 URL，
    /// 不发起网络请求。
    pub fn upstream_endpoint(&self, stream: bool) -> Result<reqwest::Url, String> {
        // 协议设置决定最终路径；Gemini 流式请求还会加上 `alt=sse`。
        self.protocol_settings
            .endpoint(self.protocol, &self.upstream, &self.model, stream)
    }
    /// 返回用于展示/构建路由的上游端点字符串。
    ///
    /// native 和 Azure 使用协议感知的完整 URL；普通 OpenAI Chat 则去重后追加
    /// `/chat/completions`。该展示结果不替代实际发送前的严格 URL 校验。
    pub fn endpoint(&self) -> String {
        if self.protocol.is_native() || self.protocol_settings.is_azure() {
            return self
                .upstream_endpoint(false)
                .map(|u| u.to_string())
                .unwrap_or_default();
        }
        format!(
            "{}/chat/completions",
            self.upstream
                .trim_end_matches('/')
                .trim_end_matches("/chat/completions")
                .trim_end_matches('/')
        )
    }
    pub fn is_go(&self) -> bool {
        // 这里按实际 host/path 判断 OpenCode Go，而不是按模型名猜测。
        self.protocol == UpstreamProtocol::OpenaiChat
            && reqwest::Url::parse(&self.upstream).is_ok_and(|u| {
                u.host_str() == Some("opencode.ai") && u.path().starts_with("/zen/go")
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn protocol_config_roundtrips_without_changing_routes_and_legacy_defaults_to_chat() {
        let mut cfg = Config::default();
        let p = &mut cfg.providers[0];
        p.upstream = "https://example.test/gateway/v1".into();
        p.api_key = "fixture".into();
        p.protocol = UpstreamProtocol::OpenaiResponses;
        p.models.push(Model::new("native-test", "Native"));
        let id = p.models[0].id.clone();
        let alias = p.models[0].alias.clone();
        cfg.validate().unwrap();
        let c = cfg.connection(&id).unwrap();
        assert_eq!(c.protocol, UpstreamProtocol::OpenaiResponses);
        assert_eq!(c.id, id);
        assert_eq!(c.alias, alias);
        let mut value = serde_json::to_value(&cfg).unwrap();
        let decoded: Config = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(
            decoded.connections()[0].protocol,
            UpstreamProtocol::OpenaiResponses
        );
        let p = value["providers"][0].as_object_mut().unwrap();
        p.remove("protocol");
        p.remove("protocol_settings");
        let legacy: Config = serde_json::from_value(value).unwrap();
        legacy.validate().unwrap();
        assert_eq!(
            legacy.connections()[0].protocol,
            UpstreamProtocol::OpenaiChat
        );
        let mut first = c.clone();
        first.provider = "same".into();
        let mut other = first.clone();
        other.id = token()[..12].into();
        other.protocol = UpstreamProtocol::OpenaiChat;
        assert_eq!(Config::group(vec![first, other]).len(), 2);
    }
    #[test]
    fn per_model_capabilities_survive_edits_routes_and_legacy_clients() {
        let mut cfg = Config::default();
        let provider = &mut cfg.providers[0];
        provider.upstream = "https://example.com/v1".into();
        provider.api_key = "fixture".into();
        provider.set_models("vision | First", "default").unwrap();
        let id = provider.models[0].id.clone();
        provider
            .set_model_rows(
                &[serde_json::json!({
                    "id":id, "model":"vision", "name":"First",
                    "capabilities":{"supportsImages":true,"maxOutputTokens":8192}
                })],
                "keep",
            )
            .unwrap();
        provider
            .set_model_rows(
                &[serde_json::json!({"id":id,"model":"renamed-model","name":"Renamed"})],
                "keep",
            )
            .unwrap();
        cfg.validate().unwrap();
        for connection in [
            cfg.connections()[0].clone(),
            cfg.connection(&id).unwrap(),
            cfg.workbuddy_connections().unwrap()[0].clone(),
        ] {
            let value = connection.capabilities.unwrap();
            assert!(value.supports_images);
            assert_eq!(value.max_output_tokens, Some(8192));
        }
        let decoded: Config = serde_json::from_slice(&serde_json::to_vec(&cfg).unwrap()).unwrap();
        assert!(
            decoded.providers[0].models[0]
                .capabilities
                .as_ref()
                .unwrap()
                .supports_images
        );
        let before = cfg.providers[0].models[0].capabilities.clone();
        assert!(cfg.providers[0].set_model_rows(&[serde_json::json!({
            "id":id,"model":"renamed-model","name":"Renamed","capabilities":{"maxInputTokens":0}
        })], "keep").is_err());
        assert_eq!(cfg.providers[0].models[0].capabilities, before);
        cfg.providers[0]
            .set_model_rows(
                &[serde_json::json!({"id":id,"model":"renamed-model","capabilities":null})],
                "keep",
            )
            .unwrap();
        assert!(cfg.providers[0].models[0].capabilities.is_none());
    }
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
