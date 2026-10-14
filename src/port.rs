//! Rebind the accepting socket without restarting the core or active streams.
use crate::{Instance, config, proxy::Bridge, workbuddy};
use axum::serve::Listener;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    net::SocketAddr,
    path::PathBuf,
    sync::{Arc, atomic::Ordering},
    time::Duration,
};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::{mpsc, oneshot},
};

pub struct Request {
    pub port: u16,
    pub reply: oneshot::Sender<Result<Applied, String>>,
}
#[derive(Serialize, Deserialize)]
pub struct Applied {
    pub port: u16,
    pub updated: usize,
    pub changed: bool,
}
impl Applied {
    pub fn message(&self) -> String {
        if !self.changed {
            return format!(
                "Local port is already {} / 当前端口已为 {}",
                self.port, self.port
            );
        }
        format!(
            "Local port {}; updated {} WorkBuddy models / 本地端口已改为 {}，已更新 {} 个 WorkBuddy 模型地址",
            self.port, self.updated, self.port, self.updated
        )
    }
}
struct FileChange {
    path: PathBuf,
    before: Option<Vec<u8>>,
    after: Vec<u8>,
}
impl FileChange {
    fn json(path: PathBuf, value: &impl Serialize) -> Result<Self, String> {
        let before = match fs::read(&path) {
            Ok(bytes) => Some(bytes),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(e.to_string()),
        };
        let mut after = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
        after.push(b'\n');
        Ok(Self {
            path,
            before,
            after,
        })
    }
}
fn commit(files: &[FileChange]) -> Result<(), String> {
    for (index, file) in files.iter().enumerate() {
        if let Err(error) = config::write_bytes(&file.path, &file.after) {
            let mut rollback_error = None;
            for previous in files[..index].iter().rev() {
                let result = if let Some(bytes) = &previous.before {
                    config::write_bytes(&previous.path, bytes)
                } else {
                    fs::remove_file(&previous.path).map_err(|e| e.to_string())
                };
                if let Err(e) = result {
                    rollback_error = Some(e);
                }
            }
            return Err(if let Some(rollback) = rollback_error {
                format!("Port change failed: {error}; restoring previous files failed: {rollback}")
            } else {
                format!("Port change failed; previous settings retained: {error}")
            });
        }
    }
    Ok(())
}
pub struct SwitchingListener {
    listener: TcpListener,
    changes: mpsc::Receiver<Request>,
    bridge: Arc<Bridge>,
}
impl SwitchingListener {
    pub fn new(
        listener: TcpListener,
        changes: mpsc::Receiver<Request>,
        bridge: Arc<Bridge>,
    ) -> Self {
        Self {
            listener,
            changes,
            bridge,
        }
    }
    // No await points: bind, files, metadata and socket transition complete as
    // one runtime operation, even if the initiating HTTP client disconnects.
    fn apply(&mut self, port: u16) -> Result<Applied, String> {
        let old = self.bridge.port.load(Ordering::Relaxed);
        if old == port {
            return Ok(Applied {
                port,
                updated: 0,
                changed: false,
            });
        }
        let reserved = std::net::TcpListener::bind(("127.0.0.1", port)).map_err(|_| {
            format!("Port {port} is occupied or unavailable / 端口 {port} 已被占用或无法监听")
        })?;
        reserved.set_nonblocking(true).map_err(|e| e.to_string())?;
        let listener = TcpListener::from_std(reserved).map_err(|e| e.to_string())?;
        let mut cfg = self.bridge.config.lock().unwrap();
        let mut next = cfg.clone();
        next.port = port;
        let models_path = workbuddy::path()?;
        let plan = workbuddy::repoint_port(&mut next, &models_path, old, port)?;
        let mut files = vec![FileChange::json(
            self.bridge.dir.join("config.json"),
            &next,
        )?];
        let mut updated = 0;
        if let Some((models, count)) = plan {
            let file = FileChange::json(models_path.clone(), &models)?;
            if file.after.len() > 2 * 1024 * 1024 {
                return Err("WorkBuddy configuration exceeds 2 MiB".into());
            }
            workbuddy::backup(&models_path)?;
            files.push(file);
            updated = count;
        }
        files.push(FileChange::json(
            self.bridge.dir.join("instance.json"),
            &Instance {
                pid: std::process::id(),
                port,
                management_key: self.bridge.management_key.clone(),
            },
        )?);
        commit(&files)?;
        *cfg = next;
        self.listener = listener;
        self.bridge.port.store(port, Ordering::Relaxed);
        Ok(Applied {
            port,
            updated,
            changed: true,
        })
    }
}
impl Listener for SwitchingListener {
    type Io = TcpStream;
    type Addr = SocketAddr;
    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        let mut changes_open = true;
        loop {
            tokio::select! {
                request = self.changes.recv(), if changes_open => {
                    if let Some(request) = request {
                        let result = self.apply(request.port);
                        let _ = request.reply.send(result);
                    } else { changes_open = false; }
                }
                result = self.listener.accept() => match result {
                    Ok(connection) => return connection,
                    Err(error) => { eprintln!("Local accept failed: {error}"); tokio::time::sleep(Duration::from_secs(1)).await; }
                }
            }
        }
    }
    fn local_addr(&self) -> std::io::Result<SocketAddr> {
        self.listener.local_addr()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_file_batch_restores_exact_previous_bytes() {
        for existed in [false, true] {
            let folder = tempfile::tempdir().unwrap();
            let first = folder.path().join("first.json");
            let original = b"{ \"keep\": true }\n".to_vec();
            if existed {
                fs::write(&first, &original).unwrap();
            }
            let files = [
                FileChange {
                    path: first.clone(),
                    before: existed.then_some(original.clone()),
                    after: b"new".to_vec(),
                },
                FileChange {
                    path: folder.path().join("missing/second.json"),
                    before: None,
                    after: b"unwritable".to_vec(),
                },
            ];
            assert!(commit(&files).is_err());
            if existed {
                assert_eq!(fs::read(&first).unwrap(), original);
            } else {
                assert!(!first.exists());
            }
            assert!(
                folder
                    .path()
                    .read_dir()
                    .unwrap()
                    .all(|p| p.unwrap().path() == first)
            );
        }
    }
}
