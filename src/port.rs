//! Rebind the accepting socket without restarting the core or active streams.
//!
//! `set_port` 管理命令从 `proxy::Bridge::command_inner` 发送 `Request`。Tokio mpsc
//! 的 receiver 归 `SwitchingListener` 所有，而 listener 正是 Axum accept 循环的一部分；
//! 因此换端口不需要停止现有连接或重建网络线程。`oneshot` reply 把同一次切换的结果
//! 送回控制命令。
//!
//! 切换先绑定新监听器，再在配置锁内构造三份文件内容，最后按顺序提交。文件批次不是
//! 跨文件原子事务：失败时只回滚已经写入的前序文件，当前失败文件由 `atomic_write`
//! 自身保证不留下半份内容。若回滚也失败，原始失败与回滚失败都会返回给调用方。
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
    /// 目标是完整 TCP 端口；管理命令已拒绝 0。
    pub port: u16,
    /// listener 完成内存/文件切换后发送结果；receiver 被 HTTP 命令 future 持有。
    pub reply: oneshot::Sender<Result<Applied, String>>,
}
#[derive(Serialize, Deserialize)]
pub struct Applied {
    pub port: u16,
    /// 实际改写的 WorkBuddy 模型条目数；端口相同或尚未安装时为 0。
    pub updated: usize,
    /// false 表示目标端口与当前端口相同，未发生文件或监听器切换。
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
    /// 写入前字节；`None` 表示文件原本不存在，回滚时应删除它。
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
/// 按数组顺序逐份写入预先构造的文件内容。
///
/// 成功返回时所有 `after` 都已由 `config::write_bytes` 报告写入成功。某一份失败时，
/// 只尝试恢复它之前已经成功写入的项；原本存在的文件写回原始字节，原本不存在则删除。
/// 回滚会遍历全部前序项并保留最后一个回滚错误。该函数不提供跨文件原子性。
fn commit(files: &[FileChange]) -> Result<(), String> {
    // 逐文件替换。失败点是 index，因此只恢复 `[..index]`，不恢复尚未写入的后续项；
    // 失败点目标交给 `atomic_write` 的临时文件语义处理。反向恢复会继续尝试其他
    // 前序文件，并只保留最后一次回滚错误用于报告。
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
    /// 在正在使用的 Axum listener 内执行一次端口切换。
    ///
    /// 输入来自管理命令队列。输出 `Applied` 只有在配置副本、可选 WorkBuddy 配置和
    /// `instance.json` 都成功写入、随后内存 listener/config/port 才被替换时返回。失败
    /// 可能来自绑定端口、读取 WorkBuddy 文件、2 MiB 上限、备份或批量写入；批量写入
    /// 失败时只做 `commit` 所描述的尽力回滚。
    fn apply(&mut self, port: u16) -> Result<Applied, String> {
        // 顺序很重要：先占用新端口；再持配置锁基于 `next` 快照生成文件；只有文件
        // 全部提交后才替换内存监听器和 AtomicU16。任何早退都会释放新端口并保留旧
        // listener/配置。`commit` 的跨文件失败只保证尽力恢复，不是强事务。
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
    /// 同时等待端口切换命令和新 TCP 连接。
    ///
    /// 该 future 不结束直到 connection 成功返回；accept 错误会等待 1 秒后继续。
    /// `select!` 没有 `biased`，多个 ready 分支之间没有代码层面的固定优先顺序。
    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        // 这是 Tokio `select!`，没有 `biased`。换端口请求和新连接同时就绪时，二者
        // 都可能先被处理；业务语义不依赖固定优先级。`apply` 内没有 await，因此开始
        // 后会运行到返回，即使发起控制请求的客户端随后断开。
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
