//! The resident tray controller never links GPUI. Only its on-demand child does.
use std::{
    io::{Read, Write},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::Mutex,
    time::{Duration, Instant},
};

static SETTINGS: Mutex<Option<Child>> = Mutex::new(None);
unsafe extern "C" {
    fn wb_gpui_host_run();
    fn wb_gpui_host_quit();
}
fn executable() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        let path = std::env::current_exe()
            .ok()?
            .parent()?
            .join("../Helpers/WorkBuddy Proxy Settings.app/Contents/MacOS/workbuddy-proxy-gpui");
        if path.is_file() {
            return Some(path);
        }
    }
    Some(
        std::env::current_exe()
            .ok()?
            .parent()?
            .join(if cfg!(windows) {
                "workbuddy-proxy-gpui.exe"
            } else {
                "workbuddy-proxy-gpui"
            }),
    )
}
pub fn available() -> bool {
    executable().is_some_and(|p| p.is_file())
}
pub fn run() {
    if let Some(ctx) = crate::native::CONTEXT.get() {
        let _ = std::fs::remove_file(ctx.bridge.dir.join("gpui-exit-host"));
    }
    unsafe { wb_gpui_host_run() };
    wb_gpui_stop();
}
pub fn quit() {
    unsafe { wb_gpui_host_quit() }
}
#[unsafe(no_mangle)]
pub extern "C" fn wb_gpui_open() {
    let Some(ctx) = crate::native::CONTEXT.get() else {
        return;
    };
    let mut child = SETTINGS.lock().unwrap();
    if child
        .as_mut()
        .is_some_and(|p| matches!(p.try_wait(), Ok(None)))
    {
        let _ = crate::config::write_json(
            &ctx.bridge.dir.join("gpui-wake.json"),
            &crate::journal::now_ms(),
        );
        return;
    }
    let Some(exe) = executable() else { return };
    let mut cmd = Command::new(exe);
    cmd.arg("--config-dir")
        .arg(&ctx.bridge.dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
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
    match cmd.spawn() {
        Ok(mut p) => {
            if let Some(mut stderr) = p.stderr.take() {
                let path = ctx.bridge.dir.join("gpui-startup.log");
                let _ = std::thread::Builder::new()
                    .name("settings-diagnostics".into())
                    .stack_size(128 * 1024)
                    .spawn(move || {
                        let mut options = std::fs::OpenOptions::new();
                        options.create(true).write(true).truncate(true);
                        #[cfg(unix)]
                        {
                            use std::os::unix::fs::OpenOptionsExt;
                            options.mode(0o600);
                        }
                        let mut file = options.open(path).ok();
                        let mut buffer = [0u8; 2048];
                        let mut written = 0;
                        while let Ok(size) = stderr.read(&mut buffer) {
                            if size == 0 {
                                break;
                            }
                            let count = size.min(65536usize.saturating_sub(written));
                            if let Some(f) = file.as_mut() {
                                let _ = f.write_all(&buffer[..count]);
                            }
                            written += count;
                        }
                    });
            }
            *child = Some(p);
        }
        Err(_) => crate::native::error("Cannot open GPUI settings / 无法打开 GPUI 设置窗口"),
    }
}
#[unsafe(no_mangle)]
pub extern "C" fn wb_gpui_stop() {
    let Some(ctx) = crate::native::CONTEXT.get() else {
        return;
    };
    let _ = crate::config::write_json(&ctx.bridge.dir.join("gpui-stop.json"), &true);
    let mut child = SETTINGS.lock().unwrap();
    if let Some(p) = child.as_mut() {
        let deadline = Instant::now() + Duration::from_secs(3);
        while matches!(p.try_wait(), Ok(None)) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(25));
        }
        // A failed renderer must not leave a detached, invisible settings process.
        if matches!(p.try_wait(), Ok(None)) {
            let _ = p.kill();
            let _ = p.wait();
        }
    }
    *child = None;
}
#[unsafe(no_mangle)]
pub extern "C" fn wb_gpui_exit_requested() -> i32 {
    // Reap a closed settings child promptly rather than retaining a zombie or
    // a Windows process handle until the user next opens the window.
    if let Ok(mut child) = SETTINGS.try_lock()
        && child
            .as_mut()
            .is_some_and(|p| matches!(p.try_wait(), Ok(Some(_))))
    {
        *child = None;
    }
    crate::native::CONTEXT
        .get()
        .is_some_and(|c| c.bridge.dir.join("gpui-exit-host").exists()) as i32
}
