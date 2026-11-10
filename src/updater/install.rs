//! The copied helper outlives the running app and waits for all profile locks.
//! No shell interpolation or elevated privileges are used; recovery never
//! clears the profile directory wholesale.
//!
//! 中文导览：本模块实现一次更新的准备、独立 helper 进程、停机、冷备份、程序替换、
//! 新版本健康确认以及失败恢复。更新开始时，当前 app 进程先创建并校验暂存目录，把计划、
//! 新包、helper 可执行文件和进度 UI 放进本次 session；随后启动同一个可执行文件附加
//! `--apply-update` 参数，让它作为短生命周期 helper 进程继续工作。helper 进程内不运行
//! 代理 HTTP 服务；替换完成并释放 profile 锁后由 helper 另行拉起应用。helper 不从父进程
//! 内存继承安装状态，而是重新读取磁盘上的 `plan.json`。
//!
//! Java/Spring 读者可以把它理解为“显式序列化的后台安装 Job”，但这里没有 Spring 容器、
//! 自动注入、消息队列或隐式事务。`Plan` 是跨进程 DTO；普通 JSON、标记文件、日志和独占
//! 文件锁就是进程间协议。helper 与 app 是两个操作系统进程，Rust 的借用、`move` 闭包和
//! 内存所有权都只在一个进程内有效，无法替另一个进程保证路径可信，所以 helper 必须重新
//! 规范化路径、核对 token、重新计算包摘要并检查 bundle。
//!
//! 所有外部命令都通过 `Command` 的独立参数传递，没有拼接 shell 字符串；流程不申请管理员
//! 权限，也不删除旧 profile。程序替换和数据库备份都不是跨 macOS/Windows 的原子事务，
//! 因此失败标记、旧程序备份、profile 备份和健康检查共同构成恢复边界。
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
    // mac 路径指向当前 `.app` bundle；Windows 路径指向安装目录，而不是单个 exe。
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
    // 这些字段会由准备进程序列化到 `session/plan.json`，再由 helper 反序列化。
    // JSON 只保存路径、版本和摘要等值，不保存 File、锁句柄、进程句柄或 Rust 引用；
    // 因而 helper 能独立读取，但对路径安全、包内容和当前磁盘状态的信任必须重新建立。
    // `target`：要替换的已安装 app bundle 或 Windows 安装目录。
    // `staged`：已校验的新 `.app`，或 Windows 下载的安装器 exe。
    // `backup`：旧程序备份位置；mac 在替换时把原 bundle 移到这里，Windows 在 prepare
    // 阶段先复制原安装目录。
    // `stage_root`：与安装目标同一父目录的临时替换区，保证 mac rename 不跨文件系统。
    // `session`：config/updates/48 位 token 下的本次下载、日志、握手和计划目录。
    // `config_dir`：本次运行使用的配置目录；它必须位于安装目标之外，备份和恢复都围绕此目录。
    // `tag`、`package`、`package_size`、`package_digest`：期望版本和已下载制品身份；
    // digest 是内容摘要，不是数字签名。
    // `data_backup`：本次 session 中的冷 profile 备份，包含业务配置文件及 SQLite 数据库。
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
    // mac bundle 先拒绝根路径为符号链接，再核对固定 bundle id。核心 app、设置 app、
    // 设置程序可执行文件及其 plist 都必须 canonicalize 后仍是原路径，防止更新包通过
    // 符号链接把后续校验或复制引向安装目录之外。缺失主程序或设置程序会直接失败。
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
        // 下载包要求版本 tag（去掉前导 v）同时匹配两个 bundle 的 CFBundleVersion；
        // 随后分别校验主 app 和设置 app 的 codesign，再执行新核心的 --version 核对。
        // codesign 检查代码签名及 bundle 完整性，不等于 Apple 公证；这里没有
        // notarization/stapling 检查。SHA-256 只核对包与发布清单的一致性，不是独立签名。
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
    // 调用方已经在下载阶段核对过 `VerifiedPackage` 的字节数和 SHA-256；本函数继续做
    // 安装级校验。首先规范化 profile，并拒绝把它放进待替换 app/目录内部，否则替换可能
    // 连带覆盖用户数据。stage_root 使用 48 位随机 session token 命名，`create_dir`
    // 不覆盖既有目录，同时提前证明安装父目录可写，使用户确认安装前就能发现权限问题。
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
    // 这个闭包把所有可能失败的准备步骤集中起来；只要任一步返回 Err，下层 helper 或 UI
    // 尚未启动，函数会尝试删除本次 stage_root。失败清理不会删除 profile、下载 session 或用户
    // 备份。Rust 闭包借用 `plan` 并在返回 Result 后结束，不跨进程传递内存引用。
    let result = (|| {
        match &plan.target {
            Target::Mac(_) => {
                // DMG 只读、不自动打开，并挂载到本次 session 的专用 mount 目录。
                // 先把 DMG 内 app 校验一次，再用 ditto 复制到 stage_root/new.app，
                // 最后再次校验暂存副本；这样磁盘上的最终替换源不依赖稍后的挂载状态。
                // detach 无论 copy 闭包成功与否都会先执行，再传播 copy 的原始错误。
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
                // Windows 安装器会就地改写目标目录，所以 prepare 阶段先复制完整旧目录。
                // 此复制发生在任何 profile 锁或更新维护标记之前，只准备程序回滚副本，
                // 不是业务数据冷备份。
                copy_tree(path, &plan.backup)?;
            }
        }
        // helper 和进度 UI 都复制到本次 session，避免启动后继续依赖正在被关闭的旧 app。
        // plan.json 是跨进程序列化边界；等到写完它，prepare 才返回 Ready Plan。
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
    // 父 app 只负责启动 helper，并通过文件完成最小就绪握手。helper 的标准输出/错误写入
    // install.log；Unix 放入独立进程组，Windows 使用无窗口标志，避免关闭主窗口时连带终止。
    // `launch_helper` 成功只表示 helper 已创建 `helper-ready`，不表示程序已替换或更新成功。
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
    // run_helper 只有在 Plan 路径、token、安装包摘要、bundle、helper 独占锁等前置校验
    // 都通过后才写 helper-ready。10 秒内进程提前退出或文件未出现都会返回错误；超时路径
    // 会尽力 kill/wait，避免留下失控准备进程。此时尚未替换程序。
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
    // 三个 profile 锁分别覆盖实例、前端和 GPUI 三个应用角色；它们的用途是确认更新期间
    // 没有这些进程继续使用 profile，并使替换、冷备份和回滚获得静止时段。每次尝试按固定
    // 顺序打开 instance/frontend/gpui，任一独占锁失败就释放本轮已取得的锁并重试。只有三把
    // 全部取得才把 File 向量返回；调用方通过持有这个 Vec 延续锁。它不同于
    // `update-helper.lock`：profile 锁保护 app 退出，helper 锁防止两个更新 helper 并发。
    //
    // abort 在每个轮询开始前检查，因此父进程在 helper 进入替换阶段的等待窗口内仍可中止。
    // 超时或 abort 只会报错，不会在这里删除或替换程序。
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
    // 启动的新/旧 app 都显式接收同一个 config_dir，并以 detached/无窗口方式运行。
    // spawn 成功只代表操作系统创建了进程，不能证明版本正确或服务可用；健康状态必须由
    // healthy 再确认。
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
    // 健康检查使用 45 秒总预算，每轮请求超时均为 1 秒。先读取本次 profile 的
    // instance.json，限定端口范围并用 `as_str` 取管理密钥，没有额外的密钥格式校验；
    // 随后连续验证三项：GET /health 的 app/ready/
    // version 均正确；用 Bearer token 调用 stats_health；其 message 字符串再次解析后
    // state 必须为 ready。任何一项缺失都继续重试，直到截止时间。
    // 只有这三个条件同时满足，调用方才 clear_maintenance。这里确认运行版本和服务就绪，
    // 不复验公证状态，也不把 HTTP 200 单独当成成功。
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
    // 先从 instance.json 取当前代理端口与管理密钥，调用带鉴权的 `_control/stop`，
    // 再写 GPUI 的退出提示文件。HTTP 调用只表达停止请求；真正能否安全替换仍由随后
    // 三把 profile 独占锁确认。
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
            // mac 先用 rename 把旧 bundle 移到 backup，再把已校验 staged 移到原位置。
            // 第二次 rename 失败时会立即尝试把 backup 移回；两步都是目录重命名，但不构成
            // 跨进程事务，最终是否需要 run_helper 的恢复分支由 backup 是否仍存在和
            // replaced 状态共同判断。
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
            // Windows 运行同步安装器并等待其退出；安装器返回非零即让本函数失败。
            // 此时目标目录可能已被部分改写，所以 run_helper 在调用 replace 前就把
            // Windows 视为“可能已替换”，失败后继续用 prepare 阶段的程序备份回滚。
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
    // 回滚必须有完整旧程序备份。Windows 把 backup 树复制回安装目录；mac 先把当前
    // 失败版本移到 stage_root/failed，再把 backup 重命名回原 target。后者避免直接覆盖
    // 仍存在的目录，也保留失败程序供诊断。回滚只恢复程序目录，profile 由
    // restore_profile 单独处理。
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
    // 本函数是 main 收到 `--apply-update` 后的短生命周期 helper 入口。它读取并重新验证
    // 已序列化的 Plan，不信任父进程仍存活，也不假设父进程内存中的校验结果仍然有效。
    // 注意返回类型有两层含义：前置校验/读取失败直接返回 Err；进入安装状态机后的业务失败
    // 通常先写 Failed 或 RecoveryFailed 结果文件，最后仍可返回 Ok(())。后者表示 helper
    // 已把失败和恢复结论落盘，绝不代表更新成功。UI、监控方必须读取 phase。
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
    // helper 先取得独立 `update-helper.lock`，并在本进程余下生命周期持有它。这把锁阻止
    // 第二个 helper 同时处理更新，和等待 app 退出的 instance/frontend/gpui 三把 profile
    // 锁职责不同。随后重新核对包路径、大小、SHA-256、mac 新旧 bundle，以及 Windows
    // staged 安装器边界；任一项变化都在写 helper-ready 前失败。
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
    // helper-ready 是父进程等待的就绪信号；它只覆盖“计划与制品已验证、helper 已持锁”，
    // 不覆盖安装结果。接着写 Installing 进度，并建立只在当前闭包内推导的三种状态：
    // `replaced` 表示可能需要回滚程序；`exited` 表示已取得三把 profile 锁；
    // `maintenance_owned` 表示本 helper 成功创建并持有了 update-maintenance 标记。
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
        // 三把 profile 锁必须全部持有后才进入维护态和冷备份阶段。marker 内容写本次
        // session 文件名（48 位 token），既标记“更新尚未完成”，也证明 marker 的 owner；
        // clear_maintenance 会核对同一 token，防止误删另一个 helper 的标记。
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
        // 此时 app 已退出且三把锁仍全部持有，backup_profile 才复制配置、SQLite DB、
        // WAL 和 SHM 等文件，形成一致的冷备份。备份成功后执行程序替换；替换窗口结束
        // 前锁不会被释放，新 app 也尚未启动。
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
            // mac 的 replace 已尝试即时 rename 回滚；若 backup 仍在磁盘上，说明状态仍
            // 足以让外层恢复分支再尝试。Windows 无论安装器返回什么都在前面先标为
            // replaced，因为失败安装器可能已经部分改写目标目录。
            replaced = replaced || plan.backup.is_dir();
            return Err(error);
        }
        replaced = true;
        // 新程序替换完成后必须释放等待 app 退出的三把 profile 锁，否则新 app 无法启动。
        // helper 锁仍然持有。启动后只有版本 /health 和鉴权 stats_health 全部 ready，
        // clear_maintenance 才会删除维护标记；健康失败会进入下方回滚分支。
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
            // 成功路径保留程序 backup 和 profile data_backup，供用户显式恢复/诊断。
            // helper 返回 Ok 表示已经写完 Complete 结果，不表示备份已被自动删除。
        }
        Err(error) => {
            let mut recovery_error = None;
            if replaced {
                // 只要程序“可能已替换”，就先请求停止新版，再重新取得三把 profile 锁，
                // 回滚程序并恢复冷 profile。这里的两个 Err 用 and_then 串联：程序回滚
                // 没成功就不会继续把 profile 当成已恢复。locks 在恢复完成后才释放。
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
                // 未替换程序，或程序/profile 已成功回滚时，还要启动旧版并确认当前编译版本、
                // 健康接口和鉴权统计接口都 ready。仅 spawn 成功不算恢复成功。
                recovery_error = Some(failure);
            }
            if exited && recovery_error.is_none() {
                if maintenance_owned {
                    // 旧版已恢复健康后才清除 marker；marker 内容不匹配或删除失败会让本次
                    // 结果升级为 RecoveryFailed，而不是掩盖成普通 Failed。
                    if let Err(error) = clear_maintenance(&plan) {
                        recovery_error = Some(error);
                    }
                } else if plan.config_dir.join("update-maintenance").exists() {
                    recovery_error =
                        Some("Update maintenance state is unverified / 更新锁状态尚未确认".into());
                }
            }
            let (phase, message) = match recovery_error {
                // 回滚或启动旧版失败：终端状态是 RecoveryFailed，需要人工核对。
                Some(failure) => (
                    Phase::RecoveryFailed,
                    format!("{error}; recovery failed / 自动恢复未完成：{failure}"),
                ),
                // 旧版与代理已确认恢复：安装失败，但当前程序仍可用。
                None if exited => (
                    Phase::Failed,
                    format!(
                        "{error}; previous version and service verified / 已确认旧版本和代理恢复运行"
                    ),
                ),
                // 尚未取得 profile 锁、未替换任何 app 文件：普通失败。
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
    // 走到这里可能是 Complete、Failed 或 RecoveryFailed。后两种业务失败已经写入结果
    // 文件并尽力恢复；Complete 也只有在结果文件成功写入后才继续。因此返回 Ok(()) 只表示
    // helper 完成状态机并成功写出终态，不能当作“更新成功”；前置校验或结果写入错误仍会 Err。
    Ok(())
}

fn clear_maintenance(plan: &Plan) -> Result<(), String> {
    // maintenance 文件不是普通布尔开关：其内容必须等于本次 session 名，且路径不能是
    // 符号链接。只有 owner 仍匹配时才删除，避免一个迟到的 helper 清掉后来更新的标记。
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
    // 调用时三把 profile 锁全部由 run_helper 持有，app 已退出；因此普通文件、SQLite
    // 主库和 WAL/SHM 是按静止快照一起复制。备份目录新建且拒绝覆盖，复制树拒绝符号链接。
    // 不属于业务 profile 的更新 session、锁文件、实例/GPUI 临时信号和 marker 被排除。
    // 若存在主数据库，再对备份副本做 `PRAGMA quick_check(1)`；这是 cold backup 的一致
    // 性检查，不是完整数据库迁移验证，也不声称 SQLite 层面的全局原子备份。
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
    // 恢复前确认本次 data_backup 存在。若当前目录有 SQLite WAL/SHM、但备份中没有对应
    // sidecar，先删除当前 sidecar；随后把备份树复制回 profile。这个操作是“用备份覆盖
    // 同名文件”，不是镜像同步：当前目录中备份未包含的普通业务文件不会被递归删除。
    // 因而回滚关注恢复已备份状态，而不会把 profile 当成可整体清空的临时目录。
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
    // 只删除由本次 prepare 精确创建的 stage_root；已验证下载、helper、日志和
    // profile data_backup 位于 session，继续保留供诊断或显式恢复。
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
