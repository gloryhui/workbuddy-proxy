#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
//! WorkBuddy Proxy 可执行程序入口。
//!
//! 本模块解析启动模式，完成单实例锁、本地端口绑定、`Bridge` 初始化以及网络运行时和
//! 统计生命周期的接线。桌面常规启动先进入 `frontend::run` 的托盘控制进程；该进程复用
//! 或启动 `--headless` core 子进程，设置窗口再由 `gpui_host` 按需启动。真正承接模型
//! 请求的 core 运行在 `proxy-io` 系统线程中的单线程 Tokio runtime 上。
//!
//! 数据所有权随执行单元分开：核心和托盘 controller 各自构造自己的 `Bridge`，仅在
//! 各自进程内由 `Arc` 共享；GPUI 进程通过本机管理接口协作，不共享这份 Rust 内存。
//! 监听器、Axum 服务和请求任务归网络线程所有；统计 writer/reader 是另外的系统线程；
//! 图形控制进程持有 `frontend.lock`，headless core 持有 `instance.lock`。启动顺序与
//! 退出边界直接影响“客户端取消”和“关机中断”的统计归类。
mod autostart;
mod capabilities;
mod catalog;
mod config;
#[allow(dead_code)] // Shared diagnostic rendering is used by the settings binary.
mod diagnostic;
#[cfg(any(windows, target_os = "macos"))]
mod frontend;
#[cfg(any(windows, target_os = "macos"))]
mod gpui_host;
#[allow(dead_code)] // Shared localization is used by the settings binary.
mod i18n;
mod journal;
mod platform;
mod port;
mod price_catalog;
mod protocol;
mod proxy;
mod session;
mod stats;
mod update;
mod updater;
#[path = "protocol/config.rs"]
pub mod upstream_config;
mod workbuddy;
mod workbuddy_status;
use axum::serve::ListenerExt;
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    net::TcpListener,
    sync::{Arc, atomic::Ordering, mpsc},
    time::Duration,
};

// Native controller 与 core 必须同时理解控制命令语义；版本号只在二者均匹配时
// 才可复用已有核心。发布版本号相同也不能证明两次本地构建的管理协议兼容。
// Protocol 13 包含已确认的在线更新、取消、进度和恢复语义。
// 另一条自动更新开发分支也使用过 Protocol 9，因此不能只看数字外观推断兼容。
const CONTROL_PROTOCOL: u32 = 13;

/// `instance.json` 的磁盘记录，供第二个进程唤醒已有窗口或迁移旧核心。
///
/// `port` 是本次实际绑定的 TCP 端口；命令行传入 0 时它可能是系统分配的临时端口。
/// `management_key` 只允许本机控制端点使用，不能当作模型 API 的 `local_key`。
#[derive(Serialize, Deserialize)]
struct Instance {
    #[serde(default)]
    pid: u32,
    port: u16,
    management_key: String,
}

/// 顶层错误出口：后台/更新/自启动模式写 stderr，普通图形启动交给平台错误界面。
fn main() {
    if let Err(e) = run() {
        let message = format!("WorkBuddy Proxy startup failed\n{e}");
        if std::env::args().any(|arg| {
            matches!(
                arg.as_str(),
                "--headless" | "--autostart" | "--apply-update"
            )
        }) {
            eprintln!("{message}");
        } else {
            platform::error(&message);
        }
        std::process::exit(1);
    }
}

/// 完成命令行解析和一次核心启动，直到负责接收请求的网络线程退出。
///
/// 关键顺序如下：
///
/// 1. 解析启动模式，确定配置目录和可选端口覆盖；
/// 2. 桌面普通启动进入托盘的 `frontend::run`，由其复用或启动 headless core；
/// 3. headless core 获取 `instance.lock`，保证同一配置目录只有一个核心；
/// 4. 加载配置、绑定监听器并构造 `Bridge`，然后发布 `instance.json`；
/// 5. 在独立 `proxy-io` 线程内建立 Tokio runtime、Axum 服务和后台维护任务；
/// 6. 网络任务结束后先标记统计 closing，再 drop 网络 runtime；worker join 后 flush。
///
/// 普通 headless core 的 `!headless` 分支不会执行：它不进入 `platform::run()`，
/// 会直接等待网络 worker 退出。返回值只描述正常退出或启动失败，不能代替单个请求的
/// 业务结果。
fn run() -> Result<(), String> {
    let mut headless = false;
    let mut autostart = false;
    let mut override_port = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            // 更新 helper 是短生命周期进程：它不启动代理服务，也不初始化实例文件。
            "--apply-update" => {
                let path = args.next().ok_or("Missing update installation plan")?;
                return updater::run_helper(std::path::Path::new(&path));
            }
            "--headless" => headless = true,
            "--autostart" => autostart = true,
            "--port" => {
                override_port = Some(
                    args.next()
                        .ok_or("Missing port")?
                        .parse::<u16>()
                        .map_err(|_| "Invalid port")?,
                );
            }
            "--config-dir" => {
                let p = args.next().ok_or("Missing configuration directory")?;
                unsafe {
                    std::env::set_var("WORKBUDDY_PROXY_CONFIG_DIR", p);
                }
            }
            "--help" | "-h" => {
                println!(
                    "workbuddy-proxy [--headless | --autostart] [--port PORT] [--config-dir DIR]"
                );
                return Ok(());
            }
            "--version" => {
                println!("{}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            _ => return Err(format!("Unknown option: {arg}")),
        }
    }
    if cfg!(not(any(windows, target_os = "macos"))) {
        // Linux/其他平台没有本项目的内置设置窗口，固定使用 headless 路径。
        headless = true;
    }
    let dir = config::directory()?;
    config::make_private_dir(&dir)?;
    #[cfg(any(windows, target_os = "macos"))]
    if !headless {
        // 托盘控制进程会在确认核心可用后，把设置请求转发给 headless 子进程。
        return frontend::run(dir, override_port, autostart);
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    let _ = autostart;
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(dir.join("instance.lock"))
        .map_err(|e| e.to_string())?;
    // 文件锁由操作系统随进程退出释放，因此无需把 PID 判断当作唯一仲裁依据。
    // 锁被占用时，当前进程只尝试用旧实例记录中的管理凭据唤醒已有窗口。
    if lock.try_lock_exclusive().is_err() {
        let bytes = fs::read(dir.join("instance.json"))
            .map_err(|_| "Another instance is starting; try again in a moment")?;
        let old: Instance =
            serde_json::from_slice(&bytes).map_err(|_| "Invalid instance record")?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| e.to_string())?;
        return runtime.block_on(async {
            let client = reqwest::Client::builder()
                .no_proxy()
                .timeout(Duration::from_secs(3))
                .build()
                .map_err(|e| e.to_string())?;
            let r = client
                .post(format!("http://127.0.0.1:{}/_control/ping", old.port))
                .bearer_auth(old.management_key)
                .send()
                .await
                .map_err(|_| "Existing instance is not responding")?;
            if r.status().is_success() {
                Ok(())
            } else {
                Err("Existing instance could not open settings".into())
            }
        });
    }
    let mut cfg = config::Config::load(&dir)?;
    if let Some(port) = override_port {
        cfg.port = port;
    }
    let listener = TcpListener::bind(("127.0.0.1", cfg.port)).map_err(|e| {
        format!(
            "Cannot bind port {}: {e}. Close another proxy using this port.",
            cfg.port
        )
    })?;
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let a = Arc::new(proxy::Bridge::new(cfg, dir.clone())?);
    a.refresh_stats_providers();
    a.port.store(port, Ordering::Relaxed);
    a.set_status(format!(
        "Local service started: {} / 本地服务已启动：{}",
        a.address(),
        a.address()
    ));
    let (port_tx, port_rx) = tokio::sync::mpsc::channel(1);
    *a.port_changes.lock().unwrap() = Some(port_tx);
    // 监听器绑定成功后才发布实例记录；否则外部控制器可能把“有记录”误判成
    // 核心已准备好。`AtomicU16` 保存当前监听端口，供 Host 校验和端口热切换使用。
    config::write_json(
        &dir.join("instance.json"),
        &Instance {
            pid: std::process::id(),
            port,
            management_key: a.management_key.clone(),
        },
    )?;
    // Only startup metadata is logged; no body, credentials, management token or session ID.
    let mut log = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("startup.log"))
        .map_err(|e| e.to_string())?;
    if log.metadata().map_err(|e| e.to_string())?.len() > 65536 {
        log.set_len(0).map_err(|e| e.to_string())?;
    }
    let _ = writeln!(
        log,
        "{} {}-{} listening on 127.0.0.1:{}",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
        port
    );
    eprintln!(
        "WorkBuddy Proxy {} · 127.0.0.1:{}",
        env!("CARGO_PKG_VERSION"),
        port
    );
    let (ready_tx, ready_rx) = mpsc::sync_channel(1);
    let thread_a = a.clone();
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel();
    *a.stop.lock().unwrap() = Some(stop_tx);
    let stats_for_shutdown = a.stats.clone();
    // `proxy-io` 是显式命名的系统线程，线程内再创建 current-thread Tokio runtime。
    // 共享一个 `Arc<Bridge>` 不代表所有字段都可并行写；内部仍用 Mutex、RwLock、
    // 原子量和有界队列分别约束访问。
    let worker=std::thread::Builder::new().name("proxy-io".into()).stack_size(512*1024).spawn(move||{
        let runtime=tokio::runtime::Builder::new_current_thread().enable_all().max_blocking_threads(2).thread_keep_alive(Duration::from_secs(10)).thread_stack_size(512*1024).build().unwrap();
        ready_tx.send(runtime.handle().clone()).unwrap();
        runtime.block_on(async{
            let metadata_bridge = thread_a.clone();
            let price_bridge = thread_a.clone();
            tokio::spawn(async move {
                loop {
                    if let Err(error) = price_bridge.sync_prices(false).await {
                        eprintln!("Price catalogue update: {error}");
                    }
                    tokio::time::sleep(Duration::from_secs(60)).await;
                }
            });
            tokio::spawn(async move {
                loop {
                    metadata_bridge.warm_metadata();
                    tokio::time::sleep(Duration::from_secs(3600)).await;
                }
            });
            let listener=tokio::net::TcpListener::from_std(listener).unwrap();let listener=port::SwitchingListener::new(listener,port_rx,thread_a.clone()).tap_io(|stream| { if let Err(e)=stream.set_nodelay(true) { eprintln!("Cannot disable local TCP buffering: {e}"); } });let server=axum::serve(listener,thread_a.routes());
            // 三个 future 同时等待：服务自行结束、管理接口请求停止、或 Ctrl-C。
            // 这里没有 `biased`，所以多个分支同时就绪时不能声称某一分支优先；
            // 退出后的统一清理由下面代码完成。
            tokio::select!{result=server=>{if let Err(e)=result{eprintln!("Server stopped: {e}");}},_=stop_rx=>{},_=tokio::signal::ctrl_c()=>{}}
        });
        // Mark the process as closing *inside* the network thread, before the
        // runtime is dropped: guards released by teardown then record
        // `interrupted/shutdown` instead of a fictitious client cancellation.
        // Doing this from the main thread would fire at startup in headless
        // mode, where there is no UI loop to wait for.
        // 必须在这里、runtime 被 drop 之前标记 closing。仍在流中的 RequestGuard 随
        // 服务 teardown 被释放时，才会记录为 shutdown 中断而非虚构的客户端取消。
        stats_for_shutdown.mark_closing();
        platform::quit();
    }).map_err(|e|e.to_string())?;
    let handle = ready_rx
        .recv()
        .map_err(|_| "Network runtime failed to start")?;
    platform::CONTEXT
        .set(platform::Context {
            bridge: a.clone(),
            handle,
            #[cfg(any(windows, target_os = "macos"))]
            remote: None,
        })
        .map_err(|_| "Already initialized")?;
    if !headless {
        platform::run();
        // The network thread marks closing itself once its server stops, so a
        // slow stream dropped during teardown is recorded as
        // `interrupted/shutdown` rather than as a client cancellation.
        if let Some(stop) = a.stop.lock().unwrap().take() {
            let _ = stop.send(());
        }
    }
    // Wait for the network thread first: it marks closing from inside, before
    // its runtime drops, so guards released by teardown already published
    // `interrupted/shutdown`. Marking here (before the join) would run at
    // startup in headless mode and mislabel every later cancellation.
    worker.join().map_err(|_| "Network worker crashed")?;
    a.stats.mark_closing();
    // The network runtime is gone, so every open statistics guard has already
    // published its terminal event. Flush them before the process exits.
    a.stats.shutdown();
    // 只有网络 worker 已退出且 `StatsHandle::shutdown` 返回后才删除实例记录。shutdown
    // 返回表示队列已关闭或已到达有界等待期限，不保证所有统计事件都已持久化。
    // 删除失败不影响已经完成的正常退出，因此这里只忽略错误。
    let _ = fs::remove_file(dir.join("instance.json"));
    drop(lock);
    Ok(())
}
