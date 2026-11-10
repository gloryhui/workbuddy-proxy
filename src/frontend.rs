//! Lightweight tray controller; settings are a separate on-demand process.
//!
//! `frontend.lock` 仲裁托盘 controller；GPUI 设置进程另有自己的 `gpui.lock` 界面实例
//! 锁和 `gpui-wake.json` 唤醒路径，两者不是同一把“窗口锁”。controller 负责发现/升级
//! 旧核心，并通过 `_control/*` 管理接口转发操作。真正服务模型请求的是它启动的
//! `--headless` 子进程；这里的 `Bridge::for_controller` 不拥有统计 writer，也不创建
//! 统计数据库。
//!
//! `frontend.lock` 仲裁托盘控制进程，`instance.lock` 由核心持有。`Remote` 保存到核心的
//! 管理端口和随机管理 key；两者都只在同用户本地进程间使用。窗口 readiness 文件
//! `frontend.json` 必须配合仍在运行的 `frontend.lock` 解释，单看文件可能是崩溃遗留。
use crate::{Instance, config, platform, proxy::Bridge};
use fs2::FileExt;
use serde::Deserialize;
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicU16, Ordering},
        mpsc,
    },
    time::Duration,
};

pub struct Remote {
    /// 核心监听端口；端口热切换成功后由 `execute` 更新。
    pub port: AtomicU16,
    /// 当前核心实例管理 key，来自 `instance.json`，不是上游 API key。
    pub key: String,
}
impl Remote {
    /// 向 core 的 `_control/action` 发送 JSON 命令并取回统一 `{message}` 信封。
    ///
    /// 对 `set_port`，成功消息本身是序列化后的 `port::Applied`；只有解析成功才更新
    /// `Remote.port`。传输、HTTP 或解析失败都返回字符串错误，不在这里回滚 core。
    pub async fn execute(
        &self,
        client: &reqwest::Client,
        payload: &Value,
    ) -> Result<String, String> {
        let res = client
            .post(format!(
                "http://127.0.0.1:{}/_control/action",
                self.port.load(Ordering::Relaxed)
            ))
            .bearer_auth(&self.key)
            .header("Content-Type", "application/json")
            .body(payload.to_string())
            .send()
            .await
            .map_err(|_| "Core is not responding / 后台核心未响应")?;
        let status = res.status();
        let bytes = res.bytes().await.map_err(|_| "Invalid core response")?;
        let data: Value = serde_json::from_slice(&bytes).map_err(|_| "Invalid core response")?;
        let message = data
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("Control request failed")
            .to_owned();
        if status.is_success() {
            if payload["command"] == "set_port" {
                let applied: crate::port::Applied =
                    serde_json::from_str(&message).map_err(|_| "Invalid port response")?;
                self.port.store(applied.port, Ordering::Relaxed);
            }
            Ok(message)
        } else {
            Err(message)
        }
    }
}
/// 启动常规图形入口，而不是直接启动 headless core。
///
/// 输入是配置目录和希望核心使用的端口。新进程不会收到 `--headless`，因此会再次进入
/// 平台的 `frontend::run`，再复用或创建 core。成功只表示进程创建成功；失败来自当前
/// 可执行文件路径或 `spawn`。核心是否可用不在这里判断。
pub fn spawn(dir: &Path, port: u16) -> Result<(), String> {
    // 这里没有传 `--headless`，所以会重新进入当前平台的常规图形入口；该路径再由
    // `frontend::run` 复用或启动真正 headless core。`spawn` 成功只证明控制/窗口
    // 进程创建成功，调用方仍需等待 `instance.json` 和管理 ping 才能判断核心可用。
    let mut cmd = Command::new(std::env::current_exe().map_err(|e| e.to_string())?);
    cmd.arg("--config-dir")
        .arg(dir)
        .arg("--port")
        .arg(port.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    detach(&mut cmd);
    cmd.spawn().map_err(|e| e.to_string())?;
    Ok(())
}
fn detach(cmd: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // A launcher/terminal may clean up its process group when the window
        // closes. Give the core its own group; stdin/out/err are already null.
        cmd.process_group(0);
    }
}
fn instance(dir: &Path) -> Option<Instance> {
    serde_json::from_slice(&fs::read(dir.join("instance.json")).ok()?).ok()
}
fn alive(value: &Instance) -> bool {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let Ok(c) = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(1))
            .build()
        else {
            return false;
        };
        c.post(format!("http://127.0.0.1:{}/_control/ping", value.port))
            .bearer_auth(&value.management_key)
            .send()
            .await
            .is_ok_and(|r| r.status().is_success())
    })
}
fn compatible(value: &Instance) -> Result<bool, String> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    rt.block_on(async {
        let c = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(2))
            .build()
            .map_err(|e| e.to_string())?;
        let bytes = c
            .get(format!("http://127.0.0.1:{}/health", value.port))
            .send()
            .await
            .map_err(|e| e.to_string())?
            .bytes()
            .await
            .map_err(|e| e.to_string())?;
        let data: Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        Ok(data["version"] == env!("CARGO_PKG_VERSION")
            && data["control_protocol"].as_u64() == Some(crate::CONTROL_PROTOCOL.into()))
    })
}

#[cfg(test)]
mod compatibility_tests {
    use super::*;
    use std::io::{Read, Write};

    #[test]
    fn same_version_protocol_nine_is_incompatible_with_api_key_provider_settings() {
        for (version, protocol, expected) in [
            (env!("CARGO_PKG_VERSION"), None, false),
            (env!("CARGO_PKG_VERSION"), Some(9), false),
            (env!("CARGO_PKG_VERSION"), Some(10), false),
            (env!("CARGO_PKG_VERSION"), Some(11), false),
            (env!("CARGO_PKG_VERSION"), Some(12), false),
            (env!("CARGO_PKG_VERSION"), Some(13), true),
            ("0.3.0", Some(10), false),
        ] {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let port = listener.local_addr().unwrap().port();
            let server = std::thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let mut request = Vec::new();
                let mut byte = [0];
                while !request.ends_with(b"\r\n\r\n") {
                    stream.read_exact(&mut byte).unwrap();
                    request.push(byte[0]);
                    assert!(request.len() < 8192);
                }
                let body =
                    serde_json::json!({"version":version,"control_protocol":protocol}).to_string();
                write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            });
            let result = compatible(&Instance {
                pid: 0,
                port,
                management_key: "fixture-management".into(),
            });
            server.join().unwrap();
            assert_eq!(result.unwrap(), expected, "{version} protocol {protocol:?}");
        }
    }
}

fn stop_old(value: &Instance, dir: &Path) -> Result<(), String> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    rt.block_on(async {
        let c = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(3))
            .build()
            .map_err(|e| e.to_string())?;
        c.post(format!("http://127.0.0.1:{}/_control/stop", value.port))
            .bearer_auth(&value.management_key)
            .send()
            .await
            .map_err(|e| e.to_string())?
            .error_for_status()
            .map_err(|e| e.to_string())?;
        Ok::<_, String>(())
    })?;
    for _ in 0..100 {
        if !dir.join("instance.json").exists() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(30));
    }
    Err("Previous core did not stop / 旧版核心尚未退出".into())
}
#[derive(Deserialize)]
struct FrontendInstance {
    pid: u32,
}

fn claim_or_wake_frontend(lock: &fs::File, dir: &Path) -> Result<bool, String> {
    // The lock can be held before the native window and its wake-up handler exist.
    for _ in 0..160 {
        // A window may be closing while another launch waits for readiness.
        if lock.try_lock_exclusive().is_ok() {
            return Ok(true);
        }
        if let Ok(bytes) = fs::read(dir.join("frontend.json"))
            && let Ok(frontend) = serde_json::from_slice::<FrontendInstance>(&bytes)
            && platform::show_process(frontend.pid)
        {
            return Ok(false);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    Err(
        "Existing settings window is still starting; try again / 已有设置窗口仍在启动，请稍后重试"
            .into(),
    )
}

pub fn ready() {
    if let Some(context) = platform::CONTEXT.get()
        && context.remote.is_some()
    {
        let _ = config::write_json(
            &context.bridge.dir.join("frontend.json"),
            &serde_json::json!({"pid": std::process::id()}),
        );
    }
}

pub fn closing() {
    if let Some(context) = platform::CONTEXT.get()
        && context.remote.is_some()
    {
        let _ = fs::remove_file(context.bridge.dir.join("frontend.json"));
    }
}

/// 运行桌面托盘控制进程，并确保它有一个可用的 headless core。
///
/// 输入是配置目录、可选端口和自启动标志。调用链为：获取 `frontend.lock`、检查已有
/// `instance.json` 的存活/版本/控制协议、必要时停止旧核心、复用或启动新核心，然后
/// 构造不拥有统计 writer 的 controller `Bridge` 并进入 `platform::run()` 托盘循环。
///
/// 返回错误可能来自缺少 GPUI helper、旧核心不兼容/无法停止、核心启动失败或 controller
/// runtime 初始化失败。等待核心使用固定 100 次轮询，但总 wall-clock 时间还包含每次
/// 管理请求耗时，因此没有由循环常数直接推出的严格上限。
pub fn run(dir: PathBuf, port: Option<u16>, autostart: bool) -> Result<(), String> {
    // 桌面常规启动链是“托盘控制进程 -> 复用/启动 headless core -> 按需 GPUI 设置进程”。
    // 这里先判定已有核心版本/控制协议兼容性，再决定复用、唤醒或停止重启。只有取得
    // `frontend.lock` 的进程可以清理陈旧 `frontend.json`。
    if !crate::gpui_host::available() {
        return Err("Settings program is missing; reinstall WorkBuddy Proxy / 设置程序缺失，请重新安装 WorkBuddy Proxy".into());
    }
    let frontend_lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(dir.join("frontend.lock"))
        .map_err(|e| e.to_string())?;
    let mut existing = instance(&dir).filter(alive);
    let outdated = if let Some(i) = &existing {
        !compatible(i)?
    } else {
        false
    };
    if frontend_lock.try_lock_exclusive().is_err() {
        if autostart {
            return Ok(());
        }
        if outdated {
            return Err("Quit the previous version from its tray/menu, then reopen / 请先从托盘或菜单栏退出旧版，再打开新版".into());
        }
        if !claim_or_wake_frontend(&frontend_lock, &dir)? {
            return Ok(());
        }
    }
    let ready_path = dir.join("frontend.json");
    // Only the lock owner removes stale readiness data left after a crash.
    let _ = fs::remove_file(&ready_path);
    if outdated {
        stop_old(existing.as_ref().unwrap(), &dir)?;
        existing = None;
    }
    let mut child = None;
    let running = if let Some(i) = existing {
        i
    } else {
        let mut cmd = Command::new(std::env::current_exe().map_err(|e| e.to_string())?);
        cmd.arg("--headless")
            .arg("--config-dir")
            .arg(&dir)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        if let Some(port) = port {
            cmd.arg("--port").arg(port.to_string());
        }
        detach(&mut cmd);
        child = Some(cmd.spawn().map_err(|e| e.to_string())?);
        let mut result = None;
        // 固定为最多轮询 100 次、两次 sleep 相隔 50 ms；总 wall-clock 时间还包含
        // `instance(...).filter(alive)` 中每次管理 ping 的阻塞时间，因此不能声称严格
        // 5 秒上限。超时不等于核心一定未启动，这里也不盲目杀掉可能仍在初始化、尚未
        // 发布 `instance.json` 的进程。
        for _ in 0..100 {
            std::thread::sleep(Duration::from_millis(50));
            if let Some(i) = instance(&dir).filter(alive) {
                result = Some(i);
                break;
            };
            if child
                .as_mut()
                .unwrap()
                .try_wait()
                .map_err(|e| e.to_string())?
                .is_some()
            {
                return Err("Core failed to start. Check startup.log and port availability. / 后台核心启动失败，请检查日志或端口占用。".into());
            }
        }
        result.ok_or("Core startup timed out")?
    };
    // Drop Child only detaches it; it does not terminate the core.
    drop(child);
    let cfg = config::Config::load(&dir)?;
    let mut controller = Bridge::for_controller(cfg, dir)?;
    // Never send local management credentials through a system/HTTP proxy.
    controller.client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(3))
        .timeout(Duration::from_secs(100))
        .build()
        .map_err(|e| e.to_string())?;
    let a = Arc::new(controller);
    a.port.store(running.port, Ordering::Relaxed);
    let (ready_tx, ready_rx) = mpsc::sync_channel(1);
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let worker = std::thread::Builder::new()
        .name("settings-io".into())
        .stack_size(512 * 1024)
        .spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .max_blocking_threads(2)
                .build()
                .unwrap();
            ready_tx.send(rt.handle().clone()).unwrap();
            rt.block_on(async {
                let _ = stop_rx.await;
            });
        })
        .map_err(|e| e.to_string())?;
    let handle = ready_rx.recv().map_err(|_| "Controller runtime failed")?;
    platform::CONTEXT
        .set(platform::Context {
            bridge: a,
            handle,
            remote: Some(Remote {
                port: AtomicU16::new(running.port),
                key: running.management_key,
            }),
        })
        .map_err(|_| "Already initialized")?;
    platform::run();
    // `platform::run()` 是托盘控制 host。单独关闭按需启动的 GPUI 设置窗口不会结束
    // 这个循环，也不会停止正在服务模型的 core。显式 quit/stop 或版本替换流程才向
    // core 发送停止；托盘 host 结束后这里关闭 controller runtime 并释放 frontend.lock。
    let _ = fs::remove_file(&ready_path);
    let _ = stop_tx.send(());
    let _ = worker.join();
    drop(frontend_lock);
    Ok(())
}
