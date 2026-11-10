//! 专用 WorkBuddy import 线程的源 I/O 与命令提交入口。
//!
//! `scheduler::Worker` 创建一个只读 `rusqlite::Connection`，在单一线程内依次完成
//! 发现、选择文件、读取一个有界片和等待 ACK。源文件正文只在本线程打开；持久化
//! 修改交给统计 writer，当前通道实现只在成功提交后发送 Ok ACK，失败也会尝试发送
//! 错误结果。Rust 所有权不保证 ACK 必达；这里没有异步 runtime，也没有让文件读取
//! 线程直接写数据库。
use super::*;

/// 由 namespace 与相对路径导出一个稳定文件身份。
///
/// 每个输入部分先写 u32 大端长度再写内容，避免简单拼接产生歧义。namespace
/// 改变会得到不同的 `file_key`，因此不同导入配置不能误用同一份 checkpoint。
fn file_key(namespace: &str, relative: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hash = Sha256::new();
    for part in [
        b"wbp-wb-file-v1".as_slice(),
        namespace.as_bytes(),
        relative.as_bytes(),
    ] {
        hash.update((part.len() as u32).to_be_bytes());
        hash.update(part);
    }
    format!("{:x}", hash.finalize())
}

pub(super) fn submit(
    inner: &Inner,
    sender: &SyncSender<Envelope>,
    shared: &Shared,
    work: Work,
) -> Result<(), &'static str> {
    // 先检查运行时是否仍启用，再构造一次性 reply 通道。`telemetry::offer` 用
    // try_send 把 Envelope 放入有界通道；队列满时立即返回 scan_busy，不在源线程
    // 无限堆积文件批次。
    //
    // 发送成功后循环等待本次结果，并周期性唤醒统计 writer。ACK 中的 Ok 表示
    // writer 已提交该事务，Err 表示没有走出成功提交路径；reply 通道断开表示调用方
    // 没有收到确认，不能解释为成功，也不能单独证明事务没有提交。这些是当前同步
    // 通道的保证，不是 Rust 所有权本身提供的回调保证。调度器收到可重试错误时会
    // 把受预算限制的 pending 文件放回队首；持久 backlog 则重新读取 checkpoint
    // 后再选文件。
    if !shared.active() {
        return Err("stats_workbuddy_import_disabled");
    }
    let (reply, ack) = mpsc::sync_channel(1);
    telemetry::offer(shared, sender, Envelope { work, reply })?;
    inner.queue.wake();
    loop {
        match ack.recv_timeout(Duration::from_millis(100)) {
            Ok(result) => return result,
            Err(mpsc::RecvTimeoutError::Disconnected) => return Err("stats_unavailable"),
            Err(mpsc::RecvTimeoutError::Timeout) if !shared.active() => {
                return Err("stats_workbuddy_import_disabled");
            }
            Err(mpsc::RecvTimeoutError::Timeout) => inner.queue.wake(),
        }
    }
}

pub(super) fn run_worker(
    inner: &Inner,
    root: &reader::ReaderRoot,
    sender: &SyncSender<Envelope>,
    shared: &Shared,
) -> Result<(), &'static str> {
    // 每次 reader 线程启动时创建一个调度器和自己的只读连接。调度器内部顺序运行，
    // 因此同一时刻至多有一个文件片等待 writer；这正是 Envelope ACK 控制源读取
    // 进度的位置。
    scheduler::Worker::new(inner, root, sender, shared)?.run()
}

mod discovery;
mod priority;
mod scheduler;

fn safe_source(root: &Path, relative: &str) -> Result<std::path::PathBuf, &'static str> {
    // 相对路径先经过白名单规则，再逐级用 symlink_metadata 拒绝符号链接。最后对
    // 文件与根目录执行 canonicalize，并确认真实文件仍位于真实根目录下。普通
    // `.jsonl` 文件本身是允许读取的对象；这里避免的是路径穿越和越过根目录边界。
    if !reader::is_allowed_relative(relative) {
        return Err("stats_workbuddy_invalid_path");
    }
    let mut component = root.to_path_buf();
    for part in relative.split('/') {
        component.push(part);
        let metadata = std::fs::symlink_metadata(&component)
            .map_err(|_| "stats_workbuddy_source_read_failed")?;
        if metadata.file_type().is_symlink() {
            return Err("stats_workbuddy_invalid_path");
        }
    }
    let path = root.join(relative);
    let metadata =
        std::fs::symlink_metadata(&path).map_err(|_| "stats_workbuddy_source_read_failed")?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err("stats_workbuddy_invalid_path");
    }
    let canonical_root = root
        .canonicalize()
        .map_err(|_| "stats_workbuddy_root_missing")?;
    let canonical = path
        .canonicalize()
        .map_err(|_| "stats_workbuddy_source_read_failed")?;
    if !canonical.starts_with(canonical_root) {
        return Err("stats_workbuddy_invalid_path");
    }
    Ok(path)
}

fn modified_ms(metadata: &std::fs::Metadata) -> Result<i64, &'static str> {
    // 修改时间统一为 Unix epoch 毫秒，并检查能否放进数据库使用的 i64。普通增量
    // 在 size 与 mtime 都未变且上次成功时会走不重读正文的快捷路径。prefix/anchor
    // fingerprint 只覆盖文件头和已提交位置尾部，既不能保证发现所有同大小改写，
    // 也不检查文件中段；full reread 才会重新解析中间内容并发现这类变化。
    let time = metadata
        .modified()
        .map_err(|_| "stats_workbuddy_source_read_failed")?
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| "stats_workbuddy_source_read_failed")?;
    i64::try_from(time.as_millis()).map_err(|_| "stats_workbuddy_unsupported_format")
}

mod file;
use file::read_file;
