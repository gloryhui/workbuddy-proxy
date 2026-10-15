#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
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
mod proxy;
mod session;
mod update;
mod workbuddy;
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

// Bump when native controllers require new management commands or semantics.
// Release versions alone cannot distinguish successive local review builds.
const CONTROL_PROTOCOL: u32 = 4;

#[derive(Serialize, Deserialize)]
struct Instance {
    #[serde(default)]
    pid: u32,
    port: u16,
    management_key: String,
}
fn main() {
    if let Err(e) = run() {
        let message = format!("WorkBuddy Proxy startup failed\n{e}");
        if std::env::args().any(|arg| matches!(arg.as_str(), "--headless" | "--autostart")) {
            eprintln!("{message}");
        } else {
            platform::error(&message);
        }
        std::process::exit(1);
    }
}
fn run() -> Result<(), String> {
    let mut headless = false;
    let mut autostart = false;
    let mut override_port = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
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
        headless = true;
    }
    let dir = config::directory()?;
    config::make_private_dir(&dir)?;
    #[cfg(any(windows, target_os = "macos"))]
    if !headless {
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
    a.port.store(port, Ordering::Relaxed);
    a.set_status(format!(
        "Local service started: {} / 本地服务已启动：{}",
        a.address(),
        a.address()
    ));
    let (port_tx, port_rx) = tokio::sync::mpsc::channel(1);
    *a.port_changes.lock().unwrap() = Some(port_tx);
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
    let worker=std::thread::Builder::new().name("proxy-io".into()).stack_size(512*1024).spawn(move||{
        let runtime=tokio::runtime::Builder::new_current_thread().enable_all().max_blocking_threads(2).thread_keep_alive(Duration::from_secs(10)).thread_stack_size(512*1024).build().unwrap();
        ready_tx.send(runtime.handle().clone()).unwrap();
        runtime.block_on(async{
            let metadata_bridge = thread_a.clone();
            tokio::spawn(async move {
                loop {
                    metadata_bridge.warm_metadata();
                    tokio::time::sleep(Duration::from_secs(3600)).await;
                }
            });
            let listener=tokio::net::TcpListener::from_std(listener).unwrap();let listener=port::SwitchingListener::new(listener,port_rx,thread_a.clone()).tap_io(|stream| { if let Err(e)=stream.set_nodelay(true) { eprintln!("Cannot disable local TCP buffering: {e}"); } });let server=axum::serve(listener,thread_a.routes());
            tokio::select!{result=server=>{if let Err(e)=result{eprintln!("Server stopped: {e}");}},_=stop_rx=>{},_=tokio::signal::ctrl_c()=>{}}
        });platform::quit();
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
        if let Some(stop) = a.stop.lock().unwrap().take() {
            let _ = stop.send(());
        }
    }
    worker.join().map_err(|_| "Network worker crashed")?;
    let _ = fs::remove_file(dir.join("instance.json"));
    drop(lock);
    Ok(())
}
