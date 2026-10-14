//! Small tray adapter; all settings widgets live in the shared GPUI application.
use crate::proxy::Bridge;
use std::{
    ffi::{CStr, CString, c_char},
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{Arc, OnceLock},
};

pub struct Context {
    pub bridge: Arc<Bridge>,
    pub handle: tokio::runtime::Handle,
    #[cfg(any(windows, target_os = "macos"))]
    pub remote: Option<crate::frontend::Remote>,
}
pub static CONTEXT: OnceLock<Context> = OnceLock::new();
#[cfg(any(windows, target_os = "macos"))]
unsafe extern "C" {
    fn wb_platform_show_process(pid: u32) -> i32;
    fn wb_platform_error(message: *const c_char);
}
pub fn run() {
    #[cfg(any(windows, target_os = "macos"))]
    crate::gpui_host::run();
}
pub fn show() {
    #[cfg(any(windows, target_os = "macos"))]
    if let Some(ctx) = CONTEXT.get() {
        if ctx.remote.is_none() {
            let _ = crate::frontend::spawn(
                &ctx.bridge.dir,
                ctx.bridge.port.load(std::sync::atomic::Ordering::Relaxed),
            );
        } else {
            crate::gpui_host::wb_gpui_open();
        }
    }
}
#[cfg(any(windows, target_os = "macos"))]
pub fn show_process(pid: u32) -> bool {
    pid != 0 && unsafe { wb_platform_show_process(pid) != 0 }
}
#[unsafe(no_mangle)]
pub extern "C" fn wb_frontend_ready() {
    #[cfg(any(windows, target_os = "macos"))]
    crate::frontend::ready();
}
#[unsafe(no_mangle)]
pub extern "C" fn wb_frontend_closing() {
    #[cfg(any(windows, target_os = "macos"))]
    crate::frontend::closing();
}
pub fn quit() {
    #[cfg(any(windows, target_os = "macos"))]
    crate::gpui_host::quit();
}
pub fn error(message: &str) {
    eprintln!("{message}");
    #[cfg(any(windows, target_os = "macos"))]
    {
        let m = CString::new(message.replace('\0', " ")).unwrap();
        unsafe { wb_platform_error(m.as_ptr()) }
    }
}
fn output(text: String) -> *mut c_char {
    CString::new(text.replace('\0', " ")).unwrap().into_raw()
}
fn input(p: *const c_char) -> String {
    if p.is_null() {
        return String::new();
    }
    unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
}
#[unsafe(no_mangle)]
pub extern "C" fn wb_free(p: *mut c_char) {
    if !p.is_null() {
        unsafe { drop(CString::from_raw(p)) }
    }
}
#[unsafe(no_mangle)]
pub extern "C" fn wb_value(name: *const c_char) -> *mut c_char {
    output(
        catch_unwind(AssertUnwindSafe(|| {
            CONTEXT.get().map_or_else(String::new, |ctx| {
                if input(name) == "language" {
                    ctx.bridge.config.lock().unwrap().language.clone()
                } else {
                    String::new()
                }
            })
        }))
        .unwrap_or_default(),
    )
}
#[unsafe(no_mangle)]
pub extern "C" fn wb_command(name: *const c_char) -> *mut c_char {
    output(
        match catch_unwind(AssertUnwindSafe(|| {
            let ctx = CONTEXT.get().ok_or("Service is not ready")?;
            let payload = serde_json::json!({"command":input(name),"ui":true});
            #[cfg(any(windows, target_os = "macos"))]
            if let Some(remote) = &ctx.remote {
                return ctx
                    .handle
                    .block_on(remote.execute(&ctx.bridge.client, &payload));
            }
            ctx.handle.block_on(ctx.bridge.command(&payload))
        })) {
            Ok(Ok(s)) => format!("OK {s}"),
            Ok(Err(e)) => format!("ERROR {e}"),
            Err(_) => "ERROR Internal error".into(),
        },
    )
}
