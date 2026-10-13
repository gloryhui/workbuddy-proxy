//! Native UI is an on-demand controller. The child core keeps serving when
//! the settings window closes; all UI allocations disappear with this PID.
use crate::{Instance, config, native, proxy::Bridge};
use fs2::FileExt;
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Arc, atomic::Ordering, mpsc},
    time::Duration,
};

pub struct Remote {
    pub port: u16,
    pub key: String,
}
impl Remote {
    pub async fn execute(
        &self,
        client: &reqwest::Client,
        command: &str,
        key: &str,
        model: &str,
        upstream: &str,
    ) -> Result<String, String> {
        let res = client
            .post(format!("http://127.0.0.1:{}/_control/action", self.port))
            .bearer_auth(&self.key)
            .json_body(command, key, model, upstream)
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
            Ok(message)
        } else {
            Err(message)
        }
    }
}
// Keep reqwest's JSON convenience feature disabled: serialize these tiny control
// payloads explicitly, leaving the forwarding hot path byte-identical.
trait ControlBody {
    fn json_body(self, command: &str, key: &str, model: &str, upstream: &str) -> Self;
}
impl ControlBody for reqwest::RequestBuilder {
    fn json_body(self, command: &str, key: &str, model: &str, upstream: &str) -> Self {
        self.header("Content-Type", "application/json").body(
            json!({"command":command,"key":key,"model":model,"upstream":upstream}).to_string(),
        )
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
pub fn run(dir: PathBuf, port: Option<u16>) -> Result<(), String> {
    let frontend_lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(dir.join("frontend.lock"))
        .map_err(|e| e.to_string())?;
    if frontend_lock.try_lock_exclusive().is_err() {
        native::show();
        return Ok(());
    }
    let existing = instance(&dir).filter(alive);
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
            remote: Some(Remote {
                port: running.port,
                key: running.management_key,
            }),
        })
        .map_err(|_| "Already initialized")?;
    native::run();
    let _ = stop_tx.send(());
    let _ = worker.join();
    drop(frontend_lock);
    Ok(())
}
