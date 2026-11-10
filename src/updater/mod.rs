//! 自动更新的下载、验证、暂存和安装协调器。
//!
//! 一次自动更新包含三个不同确认点：
//!
//! 1. `start` 创建下载会话并启动标准下载线程，命令返回的是启动时的状态快照；
//! 2. 下载线程完成精确大小/SHA-256 检查并调用 `install::prepare`，成功后进入
//!    `Ready`，表示本地已有可供确认的安装计划；
//! 3. `apply` 在用户确认后暂停新代理请求、等待现有请求释放 permit，再启动 helper。
//!
//! `Arc<Updater>` 在各调用点共享同一实例，`Mutex<Inner>` 串行保护可变进度和计划，
//! `AtomicBool` 只承载取消/安装 gate 标志。下载和 apply 各自使用标准线程，并在
//! 该线程内创建 current-thread Tokio runtime，不占用代理的现有 Tokio runtime。
pub mod dto;
mod install;

use dto::{Phase, Progress};
use futures_util::StreamExt;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

const REPOSITORY: &str = "https://github.com/gloryhui/workbuddy-proxy";
/// 单个安装包上限；下载前 metadata 和下载中的累计字节都会检查此值。
const MAX_PACKAGE: u64 = 512 * 1024 * 1024;

pub fn version(tag: &str) -> Option<[u32; 3]> {
    // 与检查接口使用同一保守版本格式：可选 `v`、恰好三段 `u32`、不接受预发布后缀。
    if tag.len() > 32 {
        return None;
    }
    let parts: Vec<_> = tag.strip_prefix('v').unwrap_or(tag).split('.').collect();
    if parts.len() != 3 {
        return None;
    }
    let mut result = [0; 3];
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        result[i] = part.parse().ok()?;
    }
    Some(result)
}

pub(super) fn asset_name(tag: &str, os: &str, arch: &str) -> Result<String, String> {
    // platform/architecture 在编译时来自目标系统，tag 只用于拼固定命名；版本格式
    // 先被拒绝，避免把命令输入直接拼进下载路径。当前仅开放两个 macOS 架构和
    // Windows x64 的自动安装 asset。
    version(tag).ok_or("Invalid stable release version / 无效的正式版本号")?;
    let suffix = match (os, arch) {
        ("macos", "aarch64") => "macos-arm64.dmg",
        ("macos", "x86_64") => "macos-x64.dmg",
        ("windows", "x86_64") => "windows-x64-setup.exe",
        _ => return Err("Automatic updates require a supported Mac or Windows installation / 当前平台不支持自动安装更新".into()),
    };
    Ok(format!(
        "workbuddy-proxy-{}-{suffix}",
        tag.strip_prefix('v').unwrap_or(tag)
    ))
}

pub fn availability(data: &Value) -> Result<(), String> {
    // 检查阶段也验证“当前运行形态是否可替换”和 asset 元数据，但不会创建目录、
    // 下载文件或取得安装 gate。Linux、开发态目录和缺少精确 asset 的 Release 会在
    // 这里返回不可在线安装的原因。
    install::target()?;
    let tag = data["tag_name"].as_str().ok_or("Invalid release version")?;
    let name = asset_name(tag, std::env::consts::OS, std::env::consts::ARCH)?;
    package_metadata(data, tag, &name)?;
    Ok(())
}

fn public_client() -> Result<reqwest::Client, String> {
    // 更新 HTTP 客户端只允许跳到 HTTPS:443、无 userinfo 且 host 在 GitHub 固定名单
    // 中的地址，跳转次数也有上限。初始 URL 同样由本模块的固定来源构造。
    // 连接/读取超时和重定向策略共同限制网络阶段，但不构成对远端内容真实性独立于
    // GitHub/Release 发布链的证明。
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            let url = attempt.url();
            if attempt.previous().len() >= 5 {
                return attempt.error("too many update redirects");
            }
            if url.scheme() == "https"
                && url.username().is_empty()
                && url.password().is_none()
                && url.port_or_known_default() == Some(443)
                && matches!(
                    url.host_str(),
                    Some(
                        "github.com"
                            | "api.github.com"
                            | "release-assets.githubusercontent.com"
                            | "objects.githubusercontent.com"
                            | "github-releases.githubusercontent.com"
                    )
                )
            {
                attempt.follow()
            } else {
                attempt.error("untrusted update redirect")
            }
        }))
        .user_agent(concat!("WorkBuddy-Proxy/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| e.to_string())
}

async fn cancellable<T>(
    future: impl std::future::Future<Output = Result<T, String>>,
    cancelled: &AtomicBool,
) -> Result<T, String> {
    // 将“等待整个 future”与 100ms 定时器 select；每次定时醒来检查取消原子量。
    // 这依赖 future 能被 poll/丢弃来实现协作式取消，不能强行中止已经阻塞在内核
    // I/O 中的单次调用，也不承诺固定毫秒内结束。
    tokio::pin!(future);
    loop {
        if cancelled.load(Ordering::Acquire) {
            return Err("Update cancelled / 已取消更新".into());
        }
        tokio::select! {result=&mut future=>return result, _=tokio::time::sleep(Duration::from_millis(100))=>{}}
    }
}
async fn small_response(
    client: &reqwest::Client,
    url: &str,
    limit: usize,
) -> Result<Vec<u8>, String> {
    // release metadata 和 SHA256SUMS 都走此函数：HTTP 成功后逐块累计，任一时刻
    // 超过调用方给出的字节上限即失败。返回的是完整小响应内存副本，因此上限必须
    // 很小；大体量安装包必须走下面的流式 `download`。
    let response = client
        .get(url)
        .header("Accept", "application/vnd.github+json")
        .timeout(Duration::from_secs(30))
        .send()
        .await
        .map_err(|_| "Update download failed; check network / 更新下载失败，请检查网络")?;
    if !response.status().is_success() {
        return Err(format!(
            "Update download HTTP {} / 更新下载失败",
            response.status().as_u16()
        ));
    }
    let mut bytes = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| "Update metadata download interrupted / 更新信息下载中断")?;
        if bytes.len() + chunk.len() > limit {
            return Err("Update metadata is too large / 更新信息超过大小限制".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn package_metadata(data: &Value, tag: &str, name: &str) -> Result<u64, String> {
    // metadata 必须同时满足：tag/draft/prerelease 与选定正式版一致；asset 列表里
    // 恰好一个目标文件名且 URL 精确等于固定仓库/版本/文件名；size 在 1 字节到
    // `MAX_PACKAGE` 之间；SHA256SUMS.txt 这个 asset 名恰好出现一次。
    //
    // 这里验证的是 Release API 声明的 URL 和长度。真实字节数、固定 SHA 文件内容、
    // 下载后的累计长度与摘要仍由 `checksum`/`download` 再验证。
    if data["tag_name"] != tag || data["draft"] != false || data["prerelease"] != false {
        return Err("Expected the selected stable release / 正式版本信息不匹配".into());
    }
    let assets = data["assets"]
        .as_array()
        .ok_or("Missing release assets / 缺少安装包")?;
    let expected_url = format!("{REPOSITORY}/releases/download/{tag}/{name}");
    let mut matching = assets.iter().filter(|item| item["name"] == name);
    let asset = matching
        .next()
        .ok_or("This release has no compatible installer / 此版本没有适用于本机的安装包")?;
    if matching.next().is_some() || asset["browser_download_url"] != expected_url {
        return Err("Invalid installer download address / 安装包下载地址无效".into());
    }
    let size = asset["size"]
        .as_u64()
        .ok_or("Missing installer size / 缺少安装包大小")?;
    if size == 0 || size > MAX_PACKAGE {
        return Err("Installer size is outside the allowed range / 安装包大小超过限制".into());
    }
    if assets
        .iter()
        .filter(|a| a["name"] == "SHA256SUMS.txt")
        .count()
        != 1
    {
        return Err("Missing or ambiguous release checksum / 缺少唯一的安装包校验文件".into());
    }
    Ok(size)
}

fn checksum(text: &[u8], name: &str) -> Result<String, String> {
    // 接受常见 `hex  filename` 或 `hex *filename` 两列行，但目标文件名只能出现
    // 一次且摘要必须恰好 64 个十六进制字符。其它文件名可被忽略；找不到目标行也
    // 失败。该摘要是 GitHub Release 中 SHA256SUMS 文件给出的完整性校验值。
    let text = std::str::from_utf8(text).map_err(|_| "Invalid checksum file / 校验文件无效")?;
    let mut found = None;
    for line in text.lines() {
        let columns: Vec<_> = line.split_whitespace().collect();
        if columns.len() == 2 && columns[1].trim_start_matches('*') == name {
            let digest = columns[0];
            if found.is_some()
                || digest.len() != 64
                || !digest.bytes().all(|b| b.is_ascii_hexdigit())
            {
                return Err("Invalid installer checksum / 安装包校验值无效".into());
            }
            found = Some(digest.to_ascii_lowercase());
        }
    }
    found.ok_or_else(|| "No checksum for this installer / 没有找到此安装包的校验值".into())
}

async fn download(
    client: &reqwest::Client,
    url: &str,
    path: &Path,
    size: u64,
    digest: &str,
    mut progress: impl FnMut(u64),
    cancelled: &AtomicBool,
) -> Result<(), String> {
    // 下载阶段把响应流直接写入磁盘，同时累计长度和 SHA-256：
    //
    // * `create_new(true)` 保证不覆盖同名已有文件；若创建失败，`created` 仍为
    //   false，下面的清理不会删除任何原先存在的文件；
    // * 若创建成功，后续网络、取消、写入、`sync_all`、长度或摘要任一步失败，都只
    //   尝试删除本次创建的包；删除失败被忽略，不能保证磁盘上没有残留；
    // * Content-Length 存在时必须等于 metadata size，最终收到字节数也必须相等；
    // * SHA-256 必须等于 SHA256SUMS 中的目标值。该哈希不是独立代码签名；
    //   macOS prepare 另查 bundle 身份、版本和 codesign，Windows 不走该 bundle 校验。
    let mut created = false;
    let result = async {
        let response = cancellable(async {client.get(url).timeout(Duration::from_secs(600)).send().await.map_err(|_| "Installer download interrupted / 安装包下载中断".to_owned())},cancelled).await?;
        if !response.status().is_success() { return Err(format!("Installer download HTTP {} / 安装包下载失败", response.status().as_u16())); }
        if response.content_length().is_some_and(|length| length != size) { return Err("Installer length mismatch / 安装包大小不匹配".into()); }
        let mut file = fs::OpenOptions::new().create_new(true).write(true).open(path).map_err(|e| e.to_string())?;
        created = true;
        let mut stream = response.bytes_stream();
        let mut hash = Sha256::new();
        let mut received = 0u64;
        loop {
            if cancelled.load(Ordering::Acquire) { return Err("Update cancelled / 已取消更新".into()); }
            let next = tokio::select! { value = stream.next() => value, _ = tokio::time::sleep(Duration::from_millis(100)) => continue };
            let Some(chunk) = next else { break };
            let chunk = chunk.map_err(|_| "Installer download interrupted / 安装包下载中断")?;
            received = received.checked_add(chunk.len() as u64).ok_or("Installer is too large")?;
            if received > size || received > MAX_PACKAGE { return Err("Installer exceeds expected size / 安装包超过预期大小".into()); }
            file.write_all(&chunk).map_err(|e| e.to_string())?;
            hash.update(&chunk);
            progress(received);
        }
        file.sync_all().map_err(|e| e.to_string())?;
        if received != size || format!("{:x}", hash.finalize()) != digest {
            return Err("Installer checksum mismatch; current version was not changed / 安装包校验失败，当前版本未改动".into());
        }
        Ok(())
    }.await;
    if result.is_err() && created {
        let _ = fs::remove_file(path);
    }
    result
}

#[derive(Default)]
struct Inner {
    /// 受同一 Mutex 保护的展示进度和已准备计划；二者会在阶段切换时一起读取。
    progress: Progress,
    /// 准备成功后持有的计划，在 Waiting 和 Installing 交接前也仍保留。
    /// apply 要求 Ready；命令回复的 Progress 快照不包含这份计划。
    prepared: Option<install::Plan>,
}
/// 进程内自动更新状态机。
///
/// `Mutex` 串行化状态转换，`cancelled` 让下载线程在不长期持锁的情况下观察取消；
/// `result_path` 既保存本进程进度，也允许启动后的管理端读取 helper 留下的终态。
/// 这些文件是跨进程通信手段，不保证每次写盘都成功，也不是事务数据库。
pub struct Updater {
    inner: Mutex<Inner>,
    result_path: std::path::PathBuf,
    cancelled: AtomicBool,
}
impl Updater {
    pub fn new(dir: &Path) -> Arc<Self> {
        // 启动时先读取有界 result 文件。除 `Installing` 外的 busy 阶段说明旧进程在
        // 下载/准备中退出，因此降为 Failed；`Installing` 或 maintenance marker 还要
        // 看 helper 锁：helper 已不在时无法确认替换结果，进入 RecoveryFailed。
        //
        // `Arc` 让管理命令、下载线程和 apply 线程共享这一状态机；它不会复制底层
        // `Plan` 或自动产生跨进程锁。
        let mut progress: Progress = fs::read(dir.join("update-install-result.json"))
            .ok()
            .filter(|bytes| bytes.len() <= 65536)
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        if progress.busy() && progress.phase != Phase::Installing {
            progress.phase = Phase::Failed;
            progress.error = Some(
                "Previous update was interrupted; check again / 上次更新已中断，请重新检查".into(),
            );
        } else if (progress.phase == Phase::Installing || dir.join("update-maintenance").exists())
            && !install::helper_active(dir)
        {
            progress.phase = Phase::RecoveryFailed;
            progress.error=Some("Previous installation outcome is unknown; use the recovery guide / 无法确认上次安装结果，请核对并按指引恢复".into());
        }
        Arc::new(Self {
            inner: Mutex::new(Inner {
                progress,
                prepared: None,
            }),
            result_path: dir.join("update-install-result.json"),
            cancelled: AtomicBool::new(false),
        })
    }
    fn save(&self, progress: &Progress) {
        // 进度写盘是尽力而为；I/O 错误被忽略，调用方不能把一次命令返回当作每次
        // 阶段都已可靠持久化。
        let _ = crate::config::write_json(&self.result_path, progress);
    }
    pub fn note(&self, message: &str) {
        // 日志只保留最近 80 行，单条截到 240 个字符；Inner 锁串行保护本进程的
        // 内存修改及此次保存，不能据此推断多次文件写入或 helper 写入构成联合事务。
        let mut inner = self.inner.lock().unwrap();
        if inner.progress.logs.len() >= 80 {
            inner.progress.logs.remove(0);
        }
        inner.progress.logs.push(format!(
            "{}  {}",
            chrono::Local::now().format("%H:%M:%S"),
            message.chars().take(240).collect::<String>()
        ));
        self.save(&inner.progress);
    }
    pub fn status(&self) -> Progress {
        // 返回 clone 而不是借用内部对象，调用方拿到的是某一时刻快照。若没有内存
        // prepared plan 且阶段为 Idle/Installing，会再试读 helper result 文件；
        // 这不是持续监听，也不等待下一次写盘。
        let mut inner = self.inner.lock().unwrap();
        if inner.prepared.is_none()
            && matches!(inner.progress.phase, Phase::Idle | Phase::Installing)
            && let Ok(bytes) = fs::read(&self.result_path)
            && bytes.len() <= 65536
            && let Ok(progress) = serde_json::from_slice::<Progress>(&bytes)
        {
            inner.progress = progress;
        }
        inner.progress.clone()
    }
    pub fn acknowledge(&self) -> Result<(), String> {
        // 任意 busy 阶段都拒绝确认，包括 `Ready`。非 busy 时保留日志并重置普通
        // Complete/Failed/Cancelled 状态。RecoveryFailed 且 maintenance marker 仍存在
        // 时返回 Ok 但不改 phase，因此“关闭/ACK 提示”不能抹掉未解决维护警告。
        let mut inner = self.inner.lock().unwrap();
        if inner.progress.busy() {
            return Err("An update is running / 更新正在进行".into());
        }
        // Dismissing the dialog cannot erase an unresolved maintenance warning.
        if inner.progress.phase == Phase::RecoveryFailed
            && self
                .result_path
                .with_file_name("update-maintenance")
                .exists()
        {
            return Ok(());
        }
        let logs = std::mem::take(&mut inner.progress.logs);
        inner.progress = Progress {
            logs,
            ..Default::default()
        };
        self.save(&inner.progress);
        Ok(())
    }
    fn phase(&self, phase: Phase) {
        let mut inner = self.inner.lock().unwrap();
        inner.progress.phase = phase;
        self.save(&inner.progress);
    }
    fn failed(&self, error: String) {
        self.note(&error);
        let mut inner = self.inner.lock().unwrap();
        inner.progress.phase = Phase::Failed;
        inner.progress.error = Some(error);
        self.save(&inner.progress);
    }
    pub fn cancel(&self) -> Result<Progress, String> {
        // 取消边界由当前 phase 决定：
        //
        // * Installing 已越过可撤销点，直接拒绝；
        // * Download/Verify/Prepare/Waiting 只置 cooperative cancel 标志，命令返回的
        //   仍是当时状态，后台线程稍后收敛；
        // * Ready 先取走 plan，再同步 discard；成功才转 Cancelled，失败直接返回 Err。
        //   下载包、helper 和诊断的保留范围由 `install::discard` 决定；
        // * Waiting 的 plan 不从内存移除，后台观察到取消后回到 Ready，可再次 apply。
        let mut inner = self.inner.lock().unwrap();
        match inner.progress.phase {
            Phase::Installing => {
                return Err("Installation is already running / 安装已经开始，无法取消".into());
            }
            Phase::Waiting | Phase::Downloading | Phase::Verifying | Phase::Preparing => {
                self.cancelled.store(true, Ordering::Release);
            }
            Phase::Ready => {
                if let Some(plan) = inner.prepared.take() {
                    install::discard(&plan)?;
                }
                inner.progress.phase = Phase::Cancelled;
                self.save(&inner.progress);
            }
            _ => {}
        }
        Ok(inner.progress.clone())
    }
    pub fn start(self: &Arc<Self>, dir: &Path, tag: &str) -> Result<Progress, String> {
        // `start` 是“创建下载任务”，不是等待下载完成。先拒绝不高于当前版本的 tag、
        // 未解决维护状态、仍活跃的 helper 和不支持的目标平台；随后检查
        // `Progress::busy()`，其中也包括 Ready，所以准备中的更新不能重复 start。
        //
        // 在配置目录下创建一次性 session，把状态切到 Downloading，再启动名为
        // `release-download` 的标准线程。下载线程重新取 Release metadata、SHA256SUMS
        // 和安装包，完成 prepare 后才把 plan 放进 Inner 并设为 Ready。函数返回的
        // Progress 只是线程启动附近的快照，不能据此断言下载或校验已经成功。
        if version(tag).ok_or("Invalid release version / 版本号无效")?
            <= version(env!("CARGO_PKG_VERSION")).unwrap()
        {
            return Err("The selected release is not newer / 所选版本不高于当前版本".into());
        }
        if dir.join("update-maintenance").exists() {
            return Err(
                "Previous installation requires recovery verification / 上次安装需要先完成恢复核对"
                    .into(),
            );
        }
        if install::helper_active(dir) {
            return Err("An update helper is still running / 更新进程仍在运行".into());
        }
        let target = install::target()?;
        let name = asset_name(tag, std::env::consts::OS, std::env::consts::ARCH)?;
        let mut inner = self.inner.lock().unwrap();
        if inner.progress.busy() {
            return Err("An update is already running / 正在更新，请勿重复操作".into());
        }
        let token = crate::config::token();
        let dir = fs::canonicalize(dir).map_err(|e| e.to_string())?;
        let updates = dir.join("updates");
        if fs::symlink_metadata(&updates).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err("Unsafe update directory / 更新目录不安全".into());
        }
        let session = updates.join(&token);
        crate::config::make_private_dir(&session)?;
        self.cancelled.store(false, Ordering::Release);
        let logs = std::mem::take(&mut inner.progress.logs);
        inner.progress = Progress {
            phase: Phase::Downloading,
            tag: tag.into(),
            logs,
            ..Default::default()
        };
        inner.prepared = None;
        self.save(&inner.progress);
        drop(inner);
        self.note("开始下载官方更新包 / Downloading official update");
        let worker = self.clone();
        // 线程句柄没有保存；它通过共享 Updater 回写状态。每个线程独立创建
        // current-thread runtime，代理 I/O runtime 不承担下载任务。
        let dir = dir.to_path_buf();
        let tag = tag.to_owned();
        let spawn = std::thread::Builder::new().name("release-download".into()).stack_size(512*1024).spawn(move || {
            let result = (|| {
                let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|e| e.to_string())?;
                runtime.block_on(async {
                    let client = public_client()?;
                    let data: Value = serde_json::from_slice(&cancellable(small_response(&client, &format!("https://api.github.com/repos/gloryhui/workbuddy-proxy/releases/tags/{tag}"), 128*1024), &worker.cancelled).await?).map_err(|_| "Invalid release metadata / 发布信息无效")?;
                    let size = package_metadata(&data, &tag, &name)?;
                    worker.inner.lock().unwrap().progress.total = size;
                    let sums = cancellable(small_response(&client, &format!("{REPOSITORY}/releases/download/{tag}/SHA256SUMS.txt"), 16384), &worker.cancelled).await?;
                    let digest = checksum(&sums, &name)?;
                    let package = session.join(&name);
                    download(&client, &format!("{REPOSITORY}/releases/download/{tag}/{name}"), &package,size,&digest,|bytes|{
                        let mut inner=worker.inner.lock().unwrap();inner.progress.downloaded=bytes;
                        if bytes==size {inner.progress.phase=Phase::Verifying;}
                    },&worker.cancelled).await?;
                    worker.note("下载及 SHA-256 校验完成 / Download and SHA-256 verified");
                    if worker.cancelled.load(Ordering::Acquire) { return Err("Update cancelled / 已取消更新".into()); }
                    worker.phase(Phase::Preparing);
                    let plan=install::prepare(&dir,&session,&token,&tag,install::VerifiedPackage {path:&package,size,digest:&digest},target)?;
                    let mut inner=worker.inner.lock().unwrap();
                    if worker.cancelled.load(Ordering::Acquire) {drop(inner);install::discard(&plan)?;return Err("Update cancelled / 已取消更新".into());}
                    inner.prepared=Some(plan);inner.progress.phase=Phase::Ready;worker.save(&inner.progress);drop(inner);
                    worker.note("更新已准备就绪，等待安装确认 / Ready; waiting for installation confirmation");
                    Ok::<_,String>(())
                })
            })();
            if let Err(error)=result {
                let cancelled=worker.cancelled.load(Ordering::Acquire);
                worker.failed(error);
                if cancelled {worker.phase(Phase::Cancelled);}
                // Never recurse into a DMG whose detach failed.
                if session.join("mount").read_dir().map_or(true, |mut entries| entries.next().is_none()) {let _=fs::remove_dir_all(&session);}
            }
        });
        if let Err(error) = spawn {
            self.failed(error.to_string());
            return Err(error.to_string());
        }
        Ok(self.status())
    }
    pub fn apply(
        self: &Arc<Self>,
        tag: &str,
        permits: Arc<tokio::sync::Semaphore>,
        gate: Arc<AtomicBool>,
    ) -> Result<Progress, String> {
        // apply 只接受仍为 Ready 且 tag 完全一致的 prepared plan。先在锁内克隆计划，
        // 再用 `gate.swap(true)` 取得进程级安装准入；成功后立即保存 Waiting 并启动
        // `release-apply` 线程。回复是调用 status 时的快照，线程可能已经推进阶段，
        // 不能保证一定返回 Waiting；进入 Waiting 本身也不表示现有请求已经结束。
        //
        // 线程申请全部 `MAX_CONCURRENT` 个代理 permit。gate 阻止新请求进入，permit
        // 则要等已有请求的 guard 随响应流结束才释放；这里不会强制切断现有请求。
        // Waiting 阶段仍可协作取消，取消时保留 plan 并回到 Ready。全部 permit 到手
        // 后在锁内再次检查取消并进入 Installing，此后 `cancel` 拒绝撤销。
        //
        // `launch_helper` 成功只代表 helper 进程完成了启动握手；本函数无法据此宣称
        // 安装完成、新版已启动，更不能保证通知一定送达 UI。helper 后续结果由
        // result/maintenance 文件和管理端轮询收敛。
        let mut inner = self.inner.lock().unwrap();
        if inner.progress.phase != Phase::Ready || inner.progress.tag != tag {
            return Err("No verified update is ready / 尚无已校验完成的更新".into());
        }
        let plan = inner
            .prepared
            .as_ref()
            .ok_or("Missing update plan")?
            .clone();
        if gate.swap(true, Ordering::AcqRel) {
            return Err("Installation is already running / 已在准备安装".into());
        }
        self.cancelled.store(false, Ordering::Release);
        inner.progress.phase = Phase::Waiting;
        self.save(&inner.progress);
        drop(inner);
        self.note(
            "等待现有请求结束，新请求暂不接入 / Waiting for active requests; new requests paused",
        );
        let worker = self.clone();
        let reset_gate = gate.clone();
        let spawn=std::thread::Builder::new().name("release-apply".into()).stack_size(512*1024).spawn(move || {
            let result=(||{
                let rt=tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|e|e.to_string())?;
                rt.block_on(async {
                    let drain=permits.acquire_many_owned(crate::proxy::MAX_CONCURRENT as u32);tokio::pin!(drain);
                    let all=loop {
                        if worker.cancelled.load(Ordering::Acquire) {worker.phase(Phase::Ready);gate.store(false,Ordering::Release);return Ok(());}
                        tokio::select! {value=&mut drain=>break value.map_err(|_|"Relay is shutting down")?,_=tokio::time::sleep(Duration::from_millis(100))=>{}}
                    };
                    {
                        let mut inner=worker.inner.lock().unwrap();
                        if worker.cancelled.load(Ordering::Acquire) {inner.progress.phase=Phase::Ready;worker.save(&inner.progress);gate.store(false,Ordering::Release);return Ok(());}
                        inner.progress.phase=Phase::Installing;worker.save(&inner.progress);
                    }
                    worker.note("正在启动独立更新进程 / Starting update helper");
                    install::launch_helper(&plan)?;
                    worker.inner.lock().unwrap().prepared=None;
                    if let Err(error)=install::stop_current(&plan) {install::abort(&plan);return Err(error);}
                    drop(all);Ok::<_,String>(())
                })
            })();
            if let Err(error)=result {gate.store(false,Ordering::Release);worker.failed(error);}
        });
        if let Err(error) = spawn {
            reset_gate.store(false, Ordering::Release);
            self.failed(error.to_string());
            return Err(error.to_string());
        }
        Ok(self.status())
    }
}

pub fn run_helper(path: &Path) -> Result<(), String> {
    install::run_helper(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn installer_selection_is_architecture_specific_and_rejects_unsafe_tags() {
        assert_eq!(
            asset_name("v0.5.6", "macos", "aarch64").unwrap(),
            "workbuddy-proxy-0.5.6-macos-arm64.dmg"
        );
        assert!(
            asset_name("v0.5.6", "macos", "x86_64")
                .unwrap()
                .ends_with("macos-x64.dmg")
        );
        assert!(
            asset_name("v0.5.6", "windows", "x86_64")
                .unwrap()
                .ends_with("windows-x64-setup.exe")
        );
        for tag in ["../0.5.6", "v0.5.6-beta", "v0.5", "v4294967296.0.0"] {
            assert!(asset_name(tag, "macos", "aarch64").is_err());
        }
        assert!(asset_name("v0.5.6", "linux", "x86_64").is_err());
    }
    #[test]
    fn checksums_require_one_exact_filename_and_a_valid_digest() {
        let hash = "ab".repeat(32);
        assert_eq!(
            checksum(
                format!("{hash}  installer.exe\n{}  another.exe\n", "cd".repeat(32)).as_bytes(),
                "installer.exe"
            )
            .unwrap(),
            hash
        );
        for text in [
            format!("{hash}  another.exe"),
            format!("{hash}  installer.exe\n{hash}  installer.exe"),
            "bad  installer.exe".into(),
        ] {
            assert!(checksum(text.as_bytes(), "installer.exe").is_err());
        }
    }

    #[test]
    fn metadata_must_point_to_the_exact_public_release_asset() {
        let tag = "v0.5.6";
        let name = "workbuddy-proxy-0.5.6-macos-arm64.dmg";
        let mut data = serde_json::json!({"tag_name":tag,"draft":false,"prerelease":false,"assets":[{"name":name,"size":123,"browser_download_url":format!("{REPOSITORY}/releases/download/{tag}/{name}")},{"name":"SHA256SUMS.txt"}]});
        assert_eq!(package_metadata(&data, tag, name).unwrap(), 123);
        data["assets"][0]["browser_download_url"] =
            serde_json::json!("https://example.com/untrusted.dmg");
        assert!(package_metadata(&data, tag, name).is_err());
        data["assets"][0]["browser_download_url"] =
            serde_json::json!(format!("{REPOSITORY}/releases/download/{tag}/{name}"));
        data["prerelease"] = serde_json::json!(true);
        assert!(package_metadata(&data, tag, name).is_err());
    }

    #[test]
    fn helper_results_arriving_after_startup_are_visible_and_can_be_acknowledged() {
        let dir = tempfile::tempdir().unwrap();
        let updater = Updater::new(dir.path());
        assert_eq!(updater.status().phase, Phase::Idle);
        crate::config::write_json(
            &dir.path().join("update-install-result.json"),
            &Progress {
                phase: Phase::Complete,
                tag: "v0.5.6".into(),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(updater.status().phase, Phase::Complete);
        updater.acknowledge().unwrap();
        assert_eq!(updater.status().phase, Phase::Idle);
        assert_eq!(updater.status().phase, Phase::Idle);
        assert!(
            updater
                .apply(
                    "v0.5.6",
                    Arc::new(tokio::sync::Semaphore::new(4)),
                    Arc::new(AtomicBool::new(false))
                )
                .is_err()
        );
    }

    #[test]
    fn unresolved_maintenance_warning_survives_dialog_dismissal() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("update-maintenance"), b"interrupted").unwrap();
        let updater = Updater::new(dir.path());
        assert_eq!(updater.status().phase, Phase::RecoveryFailed);
        updater.acknowledge().unwrap();
        assert_eq!(updater.status().phase, Phase::RecoveryFailed);
        assert!(dir.path().join("update-maintenance").is_file());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn partial_or_corrupt_downloads_are_removed_and_existing_files_survive() {
        use std::{
            io::{Read, Write},
            net::TcpListener,
        };
        let dir = tempfile::tempdir().unwrap();
        let body = b"verified application fixture";
        let good_digest = format!("{:x}", Sha256::digest(body));
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        for (index, (digest, declared, preexisting, ok)) in [
            (good_digest.clone(), body.len(), false, true),
            ("00".repeat(32), body.len(), false, false),
            (good_digest.clone(), body.len() + 1, false, false),
            (good_digest.clone(), body.len(), true, false),
        ]
        .into_iter()
        .enumerate()
        {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let url = format!("http://{}/installer", listener.local_addr().unwrap());
            let worker = std::thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = [0u8; 4096];
                let n = stream.read(&mut request).unwrap();
                assert!(
                    !String::from_utf8_lossy(&request[..n])
                        .to_ascii_lowercase()
                        .contains("authorization:")
                );
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {declared}\r\nConnection: close\r\n\r\n"
                )
                .unwrap();
                stream.write_all(body).unwrap();
            });
            let path = dir.path().join(format!("installer-{index}"));
            if preexisting {
                fs::write(&path, b"keep existing").unwrap();
            }
            let result = download(
                &client,
                &url,
                &path,
                body.len() as u64,
                &digest,
                |_| {},
                &AtomicBool::new(false),
            )
            .await;
            assert_eq!(result.is_ok(), ok, "{result:?}");
            worker.join().unwrap();
            if preexisting {
                assert_eq!(fs::read(&path).unwrap(), b"keep existing");
            } else if ok {
                assert_eq!(fs::read(&path).unwrap(), body);
            } else {
                assert!(!path.exists());
            }
        }
    }
    #[tokio::test(flavor = "current_thread")]
    async fn cancellation_interrupts_a_server_that_never_sends_headers() {
        use std::io::Read;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/slow", listener.local_addr().unwrap());
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let mut buffer = [0; 1024];
            let _ = socket.read(&mut buffer);
            let _ = release_rx.recv_timeout(Duration::from_secs(2));
        });
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let cancelled = AtomicBool::new(false);
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("partial");
        let timer = async {
            tokio::time::sleep(Duration::from_millis(30)).await;
            cancelled.store(true, Ordering::Release);
        };
        let started = std::time::Instant::now();
        let digest = "ab".repeat(32);
        let (result, ()) = tokio::join!(
            download(&client, &url, &path, 1, &digest, |_| {}, &cancelled),
            timer
        );
        release_tx.send(()).unwrap();
        server.join().unwrap();
        assert!(result.is_err());
        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(!path.exists());
    }
    #[test]
    fn interrupted_staging_is_recoverable_instead_of_remaining_busy_forever() {
        let temp = tempfile::tempdir().unwrap();
        crate::config::write_json(
            &temp.path().join("update-install-result.json"),
            &Progress {
                phase: Phase::Ready,
                tag: "v99.0.0".into(),
                ..Default::default()
            },
        )
        .unwrap();
        let updater = Updater::new(temp.path());
        assert_eq!(updater.status().phase, Phase::Failed);
        assert!(!updater.status().busy());
        updater.acknowledge().unwrap();
        assert_eq!(updater.status().phase, Phase::Idle);
    }
    #[test]
    fn installation_waits_for_active_requests_and_can_be_deferred_without_launching_helper() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let plan:install::Plan=serde_json::from_value(serde_json::json!({
            "target":{"Mac":root.join("test.app")},"staged":root.join("staged"),"backup":root.join("backup"),"stage_root":root.join("stage"),"session":root.join("session"),"config_dir":root,"tag":"v99.0.0","package":root.join("package"),"package_size":1,"package_digest":"ab".repeat(32),"data_backup":root.join("profile-backup")
        })).unwrap();
        let updater = Updater::new(root);
        {
            let mut inner = updater.inner.lock().unwrap();
            inner.prepared = Some(plan);
            inner.progress.phase = Phase::Ready;
            inner.progress.tag = "v99.0.0".into();
        }
        let permits = Arc::new(tokio::sync::Semaphore::new(crate::proxy::MAX_CONCURRENT));
        let active = permits.clone().try_acquire_owned().unwrap();
        let gate = Arc::new(AtomicBool::new(false));
        assert_eq!(
            updater
                .apply("v99.0.0", permits.clone(), gate.clone())
                .unwrap()
                .phase,
            Phase::Waiting
        );
        std::thread::sleep(Duration::from_millis(25));
        assert_eq!(updater.status().phase, Phase::Waiting);
        assert!(gate.load(Ordering::Acquire));
        updater.cancel().unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while gate.load(Ordering::Acquire) && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(!gate.load(Ordering::Acquire));
        assert_eq!(updater.status().phase, Phase::Ready);
        drop(active);
        assert_eq!(permits.available_permits(), crate::proxy::MAX_CONCURRENT);
    }
}
