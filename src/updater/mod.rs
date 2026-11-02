//! Public GitHub downloads run on a dedicated thread, never on the proxy I/O loop.
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
const MAX_PACKAGE: u64 = 512 * 1024 * 1024;

pub fn version(tag: &str) -> Option<[u32; 3]> {
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
    install::target()?;
    let tag = data["tag_name"].as_str().ok_or("Invalid release version")?;
    let name = asset_name(tag, std::env::consts::OS, std::env::consts::ARCH)?;
    package_metadata(data, tag, &name)?;
    Ok(())
}

fn public_client() -> Result<reqwest::Client, String> {
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
    progress: Progress,
    prepared: Option<install::Plan>,
}
pub struct Updater {
    inner: Mutex<Inner>,
    result_path: std::path::PathBuf,
    cancelled: AtomicBool,
}
impl Updater {
    pub fn new(dir: &Path) -> Arc<Self> {
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
        let _ = crate::config::write_json(&self.result_path, progress);
    }
    pub fn note(&self, message: &str) {
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
