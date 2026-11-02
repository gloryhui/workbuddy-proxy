//! The copied helper outlives the running app and waits for all profile locks.
//! No shell interpolation, elevated privileges, or user-data deletion is used.
use super::{
    dto::{Phase, Progress},
    version,
};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

const APP_ID: &str = "io.github.gloryhui.workbuddy-proxy";

#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum Target {
    Mac(PathBuf),
    Windows(PathBuf),
}
impl Target {
    fn path(&self) -> &Path {
        match self {
            Self::Mac(path) | Self::Windows(path) => path,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Plan {
    target: Target,
    staged: PathBuf,
    backup: PathBuf,
    stage_root: PathBuf,
    session: PathBuf,
    config_dir: PathBuf,
    tag: String,
    package: PathBuf,
    package_size: u64,
    package_digest: String,
    data_backup: PathBuf,
}

fn checked(command: &mut Command, message: &str) -> Result<(), String> {
    let output = command.output().map_err(|e| format!("{message}: {e}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "{message}: {}",
            String::from_utf8_lossy(&output.stderr)
                .chars()
                .take(1200)
                .collect::<String>()
        ))
    }
}

fn plist(bundle: &Path, key: &str) -> Result<String, String> {
    let output = Command::new("/usr/libexec/PlistBuddy")
        .arg("-c")
        .arg(format!("Print :{key}"))
        .arg(bundle.join("Contents/Info.plist"))
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err("Invalid application metadata / 应用信息无效".into());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn validate_bundle(bundle: &Path, tag: Option<&str>) -> Result<(), String> {
    if fs::symlink_metadata(bundle)
        .map_err(|e| e.to_string())?
        .file_type()
        .is_symlink()
        || plist(bundle, "CFBundleIdentifier")? != APP_ID
    {
        return Err("Application identity mismatch / 应用标识不匹配".into());
    }
    let core = bundle.join("Contents/MacOS/workbuddy-proxy");
    let settings = bundle.join("Contents/Helpers/WorkBuddy Proxy Settings.app");
    for path in [
        core.clone(),
        settings.clone(),
        settings.join("Contents/MacOS/workbuddy-proxy-gpui"),
        bundle.join("Contents/Info.plist"),
    ] {
        if fs::canonicalize(&path).map_err(|e| e.to_string())? != path {
            return Err(
                "Application paths contain symbolic links / 应用程序路径包含符号链接，请手动更新"
                    .into(),
            );
        }
    }
    if !core.is_file()
        || !settings
            .join("Contents/MacOS/workbuddy-proxy-gpui")
            .is_file()
    {
        return Err("Installer is missing application executables / 安装包缺少程序文件".into());
    }
    if let Some(tag) = tag {
        let expected = tag.strip_prefix('v').unwrap_or(tag);
        if plist(bundle, "CFBundleVersion")? != expected
            || plist(&settings, "CFBundleVersion")? != expected
        {
            return Err("Downloaded application version mismatch / 下载的应用版本不匹配".into());
        }
        checked(
            Command::new("/usr/bin/codesign")
                .args(["--verify", "--strict"])
                .arg(bundle),
            "Application signature verification failed / 应用签名校验失败",
        )?;
        checked(
            Command::new("/usr/bin/codesign")
                .args(["--verify", "--strict"])
                .arg(&settings),
            "Settings signature verification failed / 设置程序签名校验失败",
        )?;
        let output = Command::new(&core)
            .arg("--version")
            .output()
            .map_err(|e| e.to_string())?;
        if !output.status.success() || String::from_utf8_lossy(&output.stdout).trim() != expected {
            return Err("Downloaded executable version mismatch / 下载的程序版本不匹配".into());
        }
    }
    Ok(())
}

pub fn target() -> Result<Target, String> {
    let exe = std::env::current_exe()
        .and_then(fs::canonicalize)
        .map_err(|e| e.to_string())?;
    if cfg!(target_os = "macos") {
        let app = exe
            .ancestors()
            .nth(3)
            .ok_or("Cannot locate application bundle")?
            .to_path_buf();
        if app.extension().and_then(|s| s.to_str()) != Some("app")
            || exe != app.join("Contents/MacOS/workbuddy-proxy")
        {
            return Err("Install the application from the DMG before updating / 请先将 DMG 内的应用安装到可写目录，再自动更新".into());
        }
        validate_bundle(&app, None)?;
        Ok(Target::Mac(app))
    } else if cfg!(windows) {
        let parent = exe.parent().ok_or("Cannot locate installation directory")?;
        if !exe.file_name().is_some_and(|s| {
            s.to_string_lossy()
                .eq_ignore_ascii_case("workbuddy-proxy.exe")
        }) || !parent.join("internal/workbuddy-settings.exe").is_file()
        {
            return Err("Automatic updates require the installed desktop application / 请使用已安装的桌面版进行自动更新".into());
        }
        Ok(Target::Windows(parent.to_path_buf()))
    } else {
        Err("Automatic installation is supported on Mac and Windows / 仅 Mac 和 Windows 支持自动安装".into())
    }
}

fn copy_tree(from: &Path, to: &Path) -> Result<(), String> {
    fs::create_dir_all(to).map_err(|e| e.to_string())?;
    for item in fs::read_dir(from).map_err(|e| e.to_string())? {
        let item = item.map_err(|e| e.to_string())?;
        let kind = item.file_type().map_err(|e| e.to_string())?;
        let dest = to.join(item.file_name());
        if fs::symlink_metadata(&dest).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err("Refusing to overwrite a symbolic link / 拒绝覆盖符号链接".into());
        }
        if kind.is_symlink() {
            return Err("Cannot update an installation containing symbolic links / 安装目录包含符号链接，无法自动更新".into());
        }
        if kind.is_dir() {
            copy_tree(&item.path(), &dest)?;
        } else if kind.is_file() {
            fs::copy(item.path(), dest).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

pub struct VerifiedPackage<'a> {
    pub path: &'a Path,
    pub size: u64,
    pub digest: &'a str,
}

pub fn prepare(
    config_dir: &Path,
    session: &Path,
    token: &str,
    tag: &str,
    package: VerifiedPackage<'_>,
    target: Target,
) -> Result<Plan, String> {
    let config_dir = fs::canonicalize(config_dir).map_err(|e| e.to_string())?;
    if config_dir.starts_with(target.path()) {
        return Err("Configuration must be outside the installation directory / 配置目录必须位于安装目录之外".into());
    }
    let parent = target
        .path()
        .parent()
        .ok_or("Invalid installation directory")?;
    let stage_root = parent.join(format!(".workbuddy-update-{token}"));
    // This probes write access before the user is asked to leave the running app.
    fs::create_dir(&stage_root).map_err(|e|format!("Installation directory is not writable; current version is unchanged / 安装目录不可写，当前版本未改动：{e}"))?;
    crate::config::make_private_dir(&stage_root)?;
    let staged = match &target {
        Target::Mac(_) => stage_root.join("new.app"),
        Target::Windows(_) => package.path.to_path_buf(),
    };
    let backup = stage_root.join("previous");
    let plan = Plan {
        target,
        staged,
        backup,
        stage_root,
        session: session.to_path_buf(),
        config_dir,
        tag: tag.to_owned(),
        package: package.path.to_path_buf(),
        package_size: package.size,
        package_digest: package.digest.to_owned(),
        data_backup: session.join("profile-backup"),
    };
    let result = (|| {
        match &plan.target {
            Target::Mac(_) => {
                let mount = session.join("mount");
                fs::create_dir(&mount).map_err(|e| e.to_string())?;
                checked(
                    Command::new("/usr/bin/hdiutil")
                        .args([
                            "attach",
                            "-readonly",
                            "-nobrowse",
                            "-noautoopen",
                            "-mountpoint",
                        ])
                        .arg(&mount)
                        .arg(package.path),
                    "Cannot mount update DMG / 无法挂载更新安装包",
                )?;
                let copy = (|| {
                    let app = mount.join("WorkBuddy Proxy.app");
                    validate_bundle(&app, Some(tag))?;
                    checked(
                        Command::new("/usr/bin/ditto").arg(&app).arg(&plan.staged),
                        "Cannot stage update / 无法准备新版程序",
                    )?;
                    validate_bundle(&plan.staged, Some(tag))
                })();
                let detached = checked(
                    Command::new("/usr/bin/hdiutil").arg("detach").arg(&mount),
                    "Cannot detach update DMG / 无法卸载更新安装包",
                );
                copy?;
                detached?;
            }
            Target::Windows(path) => {
                copy_tree(path, &plan.backup)?;
            }
        }
        let helper = session.join(if cfg!(windows) {
            "update-helper.exe"
        } else {
            "update-helper"
        });
        fs::copy(std::env::current_exe().map_err(|e| e.to_string())?, &helper)
            .map_err(|e| e.to_string())?;
        let ui = match &plan.target {
            Target::Mac(app) => app.join(
                "Contents/Helpers/WorkBuddy Proxy Settings.app/Contents/MacOS/workbuddy-proxy-gpui",
            ),
            Target::Windows(dir) => dir.join("internal/workbuddy-settings.exe"),
        };
        fs::copy(
            ui,
            session.join(if cfg!(windows) {
                "update-ui.exe"
            } else {
                "update-ui"
            }),
        )
        .map_err(|e| e.to_string())?;
        crate::config::write_json(&session.join("plan.json"), &plan)?;
        Ok::<_, String>(())
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&plan.stage_root);
    }
    result.map(|()| plan)
}

pub fn launch_helper(plan: &Plan) -> Result<(), String> {
    let helper = plan.session.join(if cfg!(windows) {
        "update-helper.exe"
    } else {
        "update-helper"
    });
    let log = File::create(plan.session.join("install.log")).map_err(|e| e.to_string())?;
    let mut command = Command::new(helper);
    command
        .current_dir(&plan.session)
        .arg("--apply-update")
        .arg(plan.session.join("plan.json"))
        .stdin(Stdio::null())
        .stdout(Stdio::from(log.try_clone().map_err(|e| e.to_string())?))
        .stderr(Stdio::from(log));
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000 | 0x00000200);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command.spawn().map_err(|e| e.to_string())?;
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if plan.session.join("helper-ready").is_file() {
            return Ok(());
        }
        if child.try_wait().map_err(|e| e.to_string())?.is_some() {
            return Err("Update helper could not validate its installation plan; current app is unchanged / 更新进程验证失败，当前程序未改动".into());
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    let _ = child.kill();
    let _ = child.wait();
    Err("Update helper did not become ready; current app is unchanged / 更新进程未就绪，当前程序未改动".into())
}

fn wait_for_exit(dir: &Path, timeout: Duration) -> Result<Vec<File>, String> {
    wait_for_exit_inner(dir, timeout, None)
}
fn wait_for_exit_inner(
    dir: &Path,
    timeout: Duration,
    abort: Option<&Path>,
) -> Result<Vec<File>, String> {
    let started = Instant::now();
    loop {
        if abort.is_some_and(Path::exists) {
            return Err("Installation aborted before replacement / 安装已在替换前中止".into());
        }
        let mut locks = Vec::new();
        for name in ["instance.lock", "frontend.lock", "gpui.lock"] {
            let file = fs::OpenOptions::new()
                .create(true)
                .truncate(false)
                .read(true)
                .write(true)
                .open(dir.join(name))
                .map_err(|e| e.to_string())?;
            if file.try_lock_exclusive().is_err() {
                break;
            }
            locks.push(file);
        }
        if locks.len() == 3 {
            return Ok(locks);
        }
        if started.elapsed() >= timeout {
            return Err("The running application did not exit; nothing was replaced / 当前程序未退出，未替换任何文件".into());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn start_app(plan: &Plan) -> Result<(), String> {
    let mut command = match &plan.target {
        Target::Mac(app) => Command::new(app.join("Contents/MacOS/workbuddy-proxy")),
        Target::Windows(dir) => Command::new(dir.join("workbuddy-proxy.exe")),
    };
    command
        .arg("--config-dir")
        .arg(&plan.config_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    command.spawn().map_err(|e| e.to_string())?;
    Ok(())
}

fn healthy(plan: &Plan, expected: &str) -> Result<(), String> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    runtime.block_on(async {
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(1))
            .build()
            .map_err(|e| e.to_string())?;
        let deadline = Instant::now() + Duration::from_secs(45);
        while Instant::now() < deadline {
            if let Ok(bytes) = fs::read(plan.config_dir.join("instance.json"))
                && let Ok(data) = serde_json::from_slice::<serde_json::Value>(&bytes)
                && let Some(port) = data["port"].as_u64().filter(|p| *p > 0 && *p <= 65535)
                && let Ok(response) = client
                    .get(format!("http://127.0.0.1:{port}/health"))
                    .send()
                    .await
                && let Ok(bytes) = response.bytes().await
                && bytes.len() < 65536
                && let Ok(health) = serde_json::from_slice::<serde_json::Value>(&bytes)
                && health["app"] == "workbuddy-proxy"
                && health["ready"] == true
                && health["version"] == expected
                && let Some(key) = data["management_key"].as_str()
                && let Ok(reply) = client
                    .post(format!("http://127.0.0.1:{port}/_control/action"))
                    .bearer_auth(key)
                    .header("Content-Type", "application/json")
                    .body(serde_json::json!({"command":"stats_health"}).to_string())
                    .send()
                    .await
                && let Ok(bytes) = reply.bytes().await
                && bytes.len() < 65536
                && let Ok(outer) = serde_json::from_slice::<serde_json::Value>(&bytes)
                && let Some(message) = outer["message"].as_str()
                && let Ok(stats) = serde_json::from_str::<serde_json::Value>(message)
                && stats["state"] == "ready"
            {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        Err("Updated service did not become ready / 新版服务未能正常启动".into())
    })
}

pub fn stop_current(plan: &Plan) -> Result<(), String> {
    (|| {
        let data: serde_json::Value = serde_json::from_slice(
            &fs::read(plan.config_dir.join("instance.json")).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        let port = data["port"]
            .as_u64()
            .filter(|p| *p > 0 && *p <= 65535)
            .ok_or("Invalid port")?;
        let key = data["management_key"].as_str().ok_or("Missing key")?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| e.to_string())?;
        runtime.block_on(async {
            let client = reqwest::Client::builder()
                .no_proxy()
                .timeout(Duration::from_secs(3))
                .build()
                .map_err(|e| e.to_string())?;
            client
                .post(format!("http://127.0.0.1:{port}/_control/stop"))
                .bearer_auth(key)
                .send()
                .await
                .map_err(|e| e.to_string())?
                .error_for_status()
                .map_err(|_| "Cannot stop current service / 无法正常停止代理")?;
            crate::config::write_json(&plan.config_dir.join("gpui-exit-host"), &true)?;
            crate::config::write_json(&plan.config_dir.join("gpui-stop.json"), &true)?;
            Ok::<_, String>(())
        })
    })()
}

fn replace(plan: &Plan) -> Result<(), String> {
    match &plan.target {
        Target::Mac(target) => {
            fs::rename(target, &plan.backup).map_err(|e| e.to_string())?;
            if let Err(error) = fs::rename(&plan.staged, target) {
                fs::rename(&plan.backup, target).map_err(|rollback| {
                    format!("Update failed ({error}); rollback failed ({rollback})")
                })?;
                return Err(error.to_string());
            }
            Ok(())
        }
        Target::Windows(target) => {
            let status = Command::new(&plan.staged)
                .args([
                    "/VERYSILENT",
                    "/SUPPRESSMSGBOXES",
                    "/SP-",
                    "/NORESTART",
                    "/CLOSEAPPLICATIONS",
                ])
                .arg(format!("/DIR={}", target.display()))
                .arg(format!("/LOG={}", plan.session.join("setup.log").display()))
                .status()
                .map_err(|e| e.to_string())?;
            if status.success() {
                Ok(())
            } else {
                Err(format!("Installer failed ({status}) / 安装程序执行失败"))
            }
        }
    }
}

fn rollback(plan: &Plan) -> Result<(), String> {
    let target = plan.target.path();
    if !plan.backup.is_dir() {
        return Err("No rollback copy is available / 没有可用的旧版备份".into());
    }
    if matches!(plan.target, Target::Windows(_)) {
        return copy_tree(&plan.backup, target);
    }
    if target.exists() {
        fs::rename(target, plan.stage_root.join("failed")).map_err(|e| e.to_string())?;
    }
    fs::rename(&plan.backup, target).map_err(|e| e.to_string())
}

pub fn run_helper(path: &Path) -> Result<(), String> {
    let bytes = fs::read(path).map_err(|e| e.to_string())?;
    if bytes.len() > 65536 {
        return Err("Update plan too large".into());
    }
    let plan: Plan = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    let session =
        fs::canonicalize(path.parent().ok_or("Invalid update plan")?).map_err(|e| e.to_string())?;
    if fs::canonicalize(&plan.session).map_err(|e| e.to_string())? != session
        || version(&plan.tag).is_none()
        || plan.stage_root.parent() != plan.target.path().parent()
        || !plan
            .stage_root
            .file_name()
            .is_some_and(|s| s.to_string_lossy().starts_with(".workbuddy-update-"))
        || plan.backup != plan.stage_root.join("previous")
        || plan.config_dir.starts_with(plan.target.path())
    {
        return Err("Invalid update installation plan".into());
    }
    validate_paths(&plan, path)?;
    let helper_lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(plan.config_dir.join("update-helper.lock"))
        .map_err(|e| e.to_string())?;
    helper_lock
        .try_lock_exclusive()
        .map_err(|_| "Another update helper is running / 已有更新进程运行")?;
    if plan.package.parent() != Some(plan.session.as_path())
        || plan.package_size > super::MAX_PACKAGE
        || plan.package_digest.len() != 64
    {
        return Err("Invalid update package plan".into());
    }
    {
        use std::io::Read;
        let mut package = File::open(&plan.package).map_err(|e| e.to_string())?;
        if package.metadata().map_err(|e| e.to_string())?.len() != plan.package_size {
            return Err(
                "Update package changed after download / 下载后的安装包大小发生变化".into(),
            );
        }
        let mut hash = Sha256::new();
        let mut buffer = [0u8; 65536];
        loop {
            let count = package.read(&mut buffer).map_err(|e| e.to_string())?;
            if count == 0 {
                break;
            }
            hash.update(&buffer[..count]);
        }
        if format!("{:x}", hash.finalize()) != plan.package_digest {
            return Err(
                "Update package changed after verification / 校验后的安装包发生变化".into(),
            );
        }
    }
    match &plan.target {
        Target::Mac(target) => {
            if plan.staged != plan.stage_root.join("new.app") {
                return Err("Invalid staged application".into());
            }
            validate_bundle(target, None)?;
            validate_bundle(&plan.staged, Some(&plan.tag))?;
        }
        Target::Windows(_) => {
            if plan.staged.parent() != Some(plan.session.as_path())
                || plan.staged.extension().and_then(|s| s.to_str()) != Some("exe")
            {
                return Err("Invalid staged installer".into());
            }
        }
    }
    fs::write(plan.session.join("helper-ready"), b"ready").map_err(|e| e.to_string())?;
    write_progress(
        &plan,
        Phase::Installing,
        "安装进程已就绪 / Update helper ready",
        None,
        None,
    )?;
    let mut replaced = false;
    let mut exited = false;
    let mut maintenance_owned = false;
    let result = (|| {
        let abort = plan.session.join("abort");
        let locks = wait_for_exit_inner(&plan.config_dir, Duration::from_secs(60), Some(&abort))?;
        exited = true;
        let mut marker = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(plan.config_dir.join("update-maintenance"))
            .map_err(|e| format!("Cannot claim update maintenance state / 无法确认更新锁：{e}"))?;
        use std::io::Write;
        marker
            .write_all(
                plan.session
                    .file_name()
                    .ok_or("Invalid update session")?
                    .to_string_lossy()
                    .as_bytes(),
            )
            .map_err(|e| e.to_string())?;
        marker.sync_all().map_err(|e| e.to_string())?;
        maintenance_owned = true;
        let ui = plan.session.join(if cfg!(windows) {
            "update-ui.exe"
        } else {
            "update-ui"
        });
        if ui.is_file() {
            let mut cmd = Command::new(ui);
            cmd.arg("--update-progress")
                .arg(plan.config_dir.join("update-install-result.json"))
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                cmd.creation_flags(0x08000000);
            }
            #[cfg(unix)]
            {
                use std::os::unix::process::CommandExt;
                cmd.process_group(0);
            }
            let _ = cmd.spawn();
        }
        backup_profile(&plan)?;
        write_progress(
            &plan,
            Phase::Installing,
            &format!(
                "配置与用量数据库备份完成 / Profile and database backed up: {}",
                plan.data_backup.display()
            ),
            None,
            Some(plan.data_backup.display().to_string()),
        )?;
        replaced = matches!(plan.target, Target::Windows(_));
        if let Err(error) = replace(&plan) {
            replaced = replaced || plan.backup.is_dir();
            return Err(error);
        }
        replaced = true;
        drop(locks);
        start_app(&plan)?;
        healthy(&plan, plan.tag.strip_prefix('v').unwrap_or(&plan.tag))?;
        clear_maintenance(&plan)
    })();
    match result {
        Ok(()) => {
            write_progress(
                &plan,
                Phase::Complete,
                "新版版本及服务核对成功 / Updated version and service verified",
                None,
                Some(plan.data_backup.display().to_string()),
            )?;
            // Keep both application and data backups for explicit recovery.
        }
        Err(error) => {
            let mut recovery_error = None;
            if replaced {
                let _ = stop_current(&plan);
                let restored =
                    wait_for_exit(&plan.config_dir, Duration::from_secs(30)).and_then(|locks| {
                        let result = rollback(&plan).and_then(|()| restore_profile(&plan));
                        drop(locks);
                        result
                    });
                if let Err(failure) = restored {
                    recovery_error = Some(failure);
                }
            }
            if exited
                && recovery_error.is_none()
                && let Err(failure) =
                    start_app(&plan).and_then(|()| healthy(&plan, env!("CARGO_PKG_VERSION")))
            {
                recovery_error = Some(failure);
            }
            if exited && recovery_error.is_none() {
                if maintenance_owned {
                    if let Err(error) = clear_maintenance(&plan) {
                        recovery_error = Some(error);
                    }
                } else if plan.config_dir.join("update-maintenance").exists() {
                    recovery_error =
                        Some("Update maintenance state is unverified / 更新锁状态尚未确认".into());
                }
            }
            let (phase, message) = match recovery_error {
                Some(failure) => (
                    Phase::RecoveryFailed,
                    format!("{error}; recovery failed / 自动恢复未完成：{failure}"),
                ),
                None if exited => (
                    Phase::Failed,
                    format!(
                        "{error}; previous version and service verified / 已确认旧版本和代理恢复运行"
                    ),
                ),
                None => (
                    Phase::Failed,
                    format!("{error}; no application files replaced / 未替换程序文件"),
                ),
            };
            write_progress(
                &plan,
                phase,
                &message,
                Some(message.clone()),
                Some(plan.data_backup.display().to_string()),
            )?;
        }
    }
    Ok(())
}

fn clear_maintenance(plan: &Plan) -> Result<(), String> {
    let path = plan.config_dir.join("update-maintenance");
    let expected = plan
        .session
        .file_name()
        .ok_or("Invalid update session")?
        .to_string_lossy();
    if fs::symlink_metadata(&path)
        .map_err(|e| e.to_string())?
        .file_type()
        .is_symlink()
        || fs::read(&path).map_err(|e| e.to_string())? != expected.as_bytes()
    {
        return Err("Update maintenance owner changed / 更新锁归属发生变化".into());
    }
    fs::remove_file(path).map_err(|e| e.to_string())
}
fn write_progress(
    plan: &Plan,
    phase: Phase,
    message: &str,
    error: Option<String>,
    backup_path: Option<String>,
) -> Result<(), String> {
    let path = plan.config_dir.join("update-install-result.json");
    let mut value = fs::read(&path)
        .ok()
        .filter(|b| b.len() <= 65536)
        .and_then(|b| serde_json::from_slice::<Progress>(&b).ok())
        .unwrap_or_default();
    value.phase = phase;
    value.tag = plan.tag.clone();
    value.error = error;
    value.backup_path = backup_path;
    if value.logs.len() >= 80 {
        value.logs.remove(0);
    }
    value.logs.push(format!(
        "{}  {}",
        chrono::Local::now().format("%H:%M:%S"),
        message.chars().take(500).collect::<String>()
    ));
    while value.logs.len() > 1
        && serde_json::to_vec(&value).map_err(|e| e.to_string())?.len() > 60 * 1024
    {
        value.logs.remove(0);
    }
    crate::config::write_json(&path, &value)
}

fn backup_profile(plan: &Plan) -> Result<(), String> {
    // All three application locks are held; copy SQLite DB + WAL together.
    fs::create_dir(&plan.data_backup).map_err(|e| e.to_string())?;
    crate::config::make_private_dir(&plan.data_backup)?;
    for entry in fs::read_dir(&plan.config_dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name();
        let name_text = name.to_string_lossy();
        if name_text == "updates"
            || name_text.ends_with(".lock")
            || [
                "instance.json",
                "gpui.json",
                "gpui-stop.json",
                "gpui-exit-host",
                "update-install-result.json",
                "update-maintenance",
            ]
            .contains(&name_text.as_ref())
        {
            continue;
        }
        let kind = entry.file_type().map_err(|e| e.to_string())?;
        if kind.is_symlink() {
            return Err("Cannot back up a profile containing symbolic links / 配置目录含符号链接，请手动更新".into());
        }
        if kind.is_dir() {
            copy_tree(&entry.path(), &plan.data_backup.join(name))?;
        } else if kind.is_file() {
            fs::copy(entry.path(), plan.data_backup.join(name)).map_err(|e| e.to_string())?;
        }
    }
    let db = plan.data_backup.join("usage.sqlite3");
    if db.is_file() {
        let connection =
            rusqlite::Connection::open_with_flags(db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
                .map_err(|e| e.to_string())?;
        connection
            .busy_timeout(Duration::from_secs(2))
            .map_err(|e| e.to_string())?;
        let valid: String = connection
            .query_row("PRAGMA quick_check(1)", [], |row| row.get(0))
            .map_err(|e| e.to_string())?;
        if valid != "ok" {
            return Err("Database backup verification failed / 用量数据库备份校验失败".into());
        }
    }
    Ok(())
}
fn restore_profile(plan: &Plan) -> Result<(), String> {
    if !plan.data_backup.is_dir() {
        return Err("Profile backup is unavailable / 配置备份不可用".into());
    }
    for name in ["usage.sqlite3-wal", "usage.sqlite3-shm"] {
        let path = plan.config_dir.join(name);
        if path.exists() && !plan.data_backup.join(name).exists() {
            if fs::symlink_metadata(&path)
                .map_err(|e| e.to_string())?
                .file_type()
                .is_symlink()
            {
                return Err("Unsafe SQLite sidecar".into());
            }
            fs::remove_file(path).map_err(|e| e.to_string())?;
        }
    }
    copy_tree(&plan.data_backup, &plan.config_dir)
}

pub fn discard(plan: &Plan) -> Result<(), String> {
    // Only a stage created by prepare(), adjacent to this specific app.
    validate_paths(plan, &plan.session.join("plan.json"))?;
    fs::remove_dir_all(&plan.stage_root).map_err(|e| e.to_string())?;
    // The verified download/helper and data backup remain for diagnosis.
    Ok(())
}

pub fn helper_active(dir: &Path) -> bool {
    let path = dir.join("update-helper.lock");
    if !path.exists() {
        return false;
    }
    if fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink()) {
        return true;
    }
    fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .map_or(true, |file| file.try_lock_exclusive().is_err())
}
pub fn abort(plan: &Plan) {
    let _ = fs::write(plan.session.join("abort"), b"abort");
}
fn validate_paths(plan: &Plan, path: &Path) -> Result<(), String> {
    if !matches!(&plan.target,Target::Mac(_) if cfg!(target_os="macos"))
        && !matches!(&plan.target,Target::Windows(_) if cfg!(windows))
    {
        return Err("Update plan platform mismatch".into());
    }
    if let Target::Windows(target) = &plan.target
        && (!target.join("workbuddy-proxy.exe").is_file()
            || !target.join("internal/workbuddy-settings.exe").is_file())
    {
        return Err("Invalid Windows installation target".into());
    }
    for path in [path, plan.package.as_path(), plan.backup.as_path()] {
        if fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(
                "Update plan contains symbolic links / 更新计划包含不安全的符号链接".into(),
            );
        }
    }
    let token = plan
        .session
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("Invalid update session")?;
    if token.len() != 48
        || !token.bytes().all(|c| c.is_ascii_hexdigit())
        || plan.session != plan.config_dir.join("updates").join(token)
        || path != plan.session.join("plan.json")
        || plan.stage_root
            != plan
                .target
                .path()
                .parent()
                .ok_or("Invalid target")?
                .join(format!(".workbuddy-update-{token}"))
        || plan.data_backup != plan.session.join("profile-backup")
        || fs::canonicalize(&plan.session).map_err(|e| e.to_string())? != plan.session
        || fs::canonicalize(&plan.config_dir).map_err(|e| e.to_string())? != plan.config_dir
        || fs::canonicalize(&plan.stage_root).map_err(|e| e.to_string())? != plan.stage_root
        || fs::canonicalize(plan.target.path()).map_err(|e| e.to_string())? != plan.target.path()
    {
        return Err("Unsafe update installation paths / 更新安装路径不安全".into());
    }
    if plan.target.path() == Path::new("/") || plan.target.path().parent().is_none() {
        return Err("Unsafe installation target".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn held_profile_locks_prevent_replacement() {
        let dir = tempfile::tempdir().unwrap();
        let lock = File::create(dir.path().join("gpui.lock")).unwrap();
        lock.lock_exclusive().unwrap();
        assert!(wait_for_exit(dir.path(), Duration::from_millis(20)).is_err());
        drop(lock);
        assert_eq!(
            wait_for_exit(dir.path(), Duration::from_secs(1))
                .unwrap()
                .len(),
            3
        );
    }
    #[test]
    fn failed_staging_and_startup_preserve_a_recoverable_directory() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("app");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("version"), "old").unwrap();
        let stage = dir.path().join(".workbuddy-update-test");
        fs::create_dir(&stage).unwrap();
        let plan = Plan {
            target: Target::Mac(target.clone()),
            staged: stage.join("new.app"),
            backup: stage.join("previous"),
            stage_root: stage.clone(),
            session: dir.path().join("session"),
            config_dir: dir.path().join("profile"),
            tag: "v0.5.6".into(),
            package: dir.path().join("package"),
            package_size: 0,
            package_digest: String::new(),
            data_backup: dir.path().join("profile-backup"),
        };
        assert!(replace(&plan).is_err());
        assert_eq!(fs::read_to_string(target.join("version")).unwrap(), "old");
        fs::create_dir(&plan.staged).unwrap();
        fs::write(plan.staged.join("version"), "new").unwrap();
        replace(&plan).unwrap();
        assert_eq!(fs::read_to_string(target.join("version")).unwrap(), "new");
        rollback(&plan).unwrap();
        assert_eq!(fs::read_to_string(target.join("version")).unwrap(), "old");
    }
    fn fixture_plan(root: &Path) -> Plan {
        let canonical_root = fs::canonicalize(root).unwrap();
        let root = canonical_root.as_path();
        let config_dir = root.join("profile");
        fs::create_dir_all(&config_dir).unwrap();
        let token = "ab".repeat(24);
        let session = config_dir.join("updates").join(&token);
        fs::create_dir_all(&session).unwrap();
        let stage_root = root.join(format!(".workbuddy-update-{token}"));
        fs::create_dir(&stage_root).unwrap();
        let target = root.join("application.app");
        fs::create_dir(&target).unwrap();
        Plan {
            target: Target::Mac(target),
            staged: stage_root.join("new.app"),
            backup: stage_root.join("previous"),
            data_backup: session.join("profile-backup"),
            package: session.join("fixture.dmg"),
            package_size: 1,
            package_digest: "ab".repeat(32),
            config_dir,
            stage_root,
            session,
            tag: "v99.0.0".into(),
        }
    }
    #[test]
    fn cold_profile_backup_keeps_wal_and_rollback_restores_data() {
        let temp = tempfile::tempdir().unwrap();
        let plan = fixture_plan(temp.path());
        fs::write(
            plan.config_dir.join("config.json"),
            b"synthetic-old-settings",
        )
        .unwrap();
        fs::write(plan.config_dir.join("gpui-stop.json"), b"true").unwrap();
        fs::write(
            plan.config_dir.join("update-maintenance"),
            b"synthetic-owner",
        )
        .unwrap();
        let db = plan.config_dir.join("usage.sqlite3");
        let connection = rusqlite::Connection::open(&db).unwrap();
        connection.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE preserved(value); INSERT INTO preserved VALUES(42);").unwrap();
        assert!(
            fs::metadata(plan.config_dir.join("usage.sqlite3-wal"))
                .unwrap()
                .len()
                > 0
        );
        backup_profile(&plan).unwrap();
        assert!(!plan.data_backup.join("update-maintenance").exists());
        assert!(!plan.data_backup.join("updates").exists());
        assert!(!plan.data_backup.join("gpui-stop.json").exists());
        drop(connection);
        let connection = rusqlite::Connection::open(&db).unwrap();
        connection
            .execute("UPDATE preserved SET value=99", [])
            .unwrap();
        drop(connection);
        fs::write(
            plan.config_dir.join("config.json"),
            b"synthetic-new-settings",
        )
        .unwrap();
        restore_profile(&plan).unwrap();
        assert_eq!(
            fs::read(plan.config_dir.join("config.json")).unwrap(),
            b"synthetic-old-settings"
        );
        let restored = rusqlite::Connection::open(&db).unwrap();
        assert_eq!(
            restored
                .query_row("SELECT value FROM preserved", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            42
        );
    }
    #[test]
    fn a_live_helper_blocks_a_second_update_and_abort_prevents_replacement() {
        let temp = tempfile::tempdir().unwrap();
        let plan = fixture_plan(temp.path());
        let lock = File::create(plan.config_dir.join("update-helper.lock")).unwrap();
        lock.lock_exclusive().unwrap();
        assert!(helper_active(&plan.config_dir));
        drop(lock);
        assert!(!helper_active(&plan.config_dir));
        abort(&plan);
        assert!(
            wait_for_exit_inner(
                &plan.config_dir,
                Duration::from_secs(1),
                Some(&plan.session.join("abort"))
            )
            .is_err()
        );
    }
    #[cfg(target_os = "macos")]
    #[test]
    fn helper_paths_are_bound_to_exact_session_and_staging_directory() {
        let temp = tempfile::tempdir().unwrap();
        let mut plan = fixture_plan(temp.path());
        let path = plan.session.join("plan.json");
        assert!(validate_paths(&plan, &path).is_ok());
        plan.stage_root = temp.path().join("unrelated");
        fs::create_dir(&plan.stage_root).unwrap();
        assert!(validate_paths(&plan, &path).is_err());
    }
}
