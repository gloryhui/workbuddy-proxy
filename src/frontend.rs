//! Native UI is an on-demand controller. The child core keeps serving when
//! the settings window closes. Windows keeps its controller for the tray.
use crate::{Instance, config, native, proxy::Bridge};
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
    pub port: AtomicU16,
    pub key: String,
}
impl Remote {
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
pub fn spawn(dir: &Path, port: u16) -> Result<(), String> {
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
            && native::show_process(frontend.pid)
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
    if let Some(context) = native::CONTEXT.get()
        && context.remote.is_some()
    {
        let _ = config::write_json(
            &context.bridge.dir.join("frontend.json"),
            &serde_json::json!({"pid": std::process::id()}),
        );
    }
}

pub fn closing() {
    if let Some(context) = native::CONTEXT.get()
        && context.remote.is_some()
    {
        let _ = fs::remove_file(context.bridge.dir.join("frontend.json"));
    }
}

pub fn run(dir: PathBuf, port: Option<u16>) -> Result<(), String> {
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
    let mut controller = Bridge::new(cfg, dir)?;
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
    native::CONTEXT
        .set(native::Context {
            bridge: a,
            handle,
            selected: std::sync::atomic::AtomicUsize::new(0),
            remote: Some(Remote {
                port: AtomicU16::new(running.port),
                key: running.management_key,
            }),
        })
        .map_err(|_| "Already initialized")?;
    native::run();
    let _ = fs::remove_file(&ready_path);
    let _ = stop_tx.send(());
    let _ = worker.join();
    drop(frontend_lock);
    Ok(())
}
