use crate::proxy::Bridge;
use std::{
    ffi::{CStr, CString, c_char},
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc, OnceLock,
        atomic::{AtomicUsize, Ordering},
    },
};

pub struct Context {
    pub bridge: Arc<Bridge>,
    pub handle: tokio::runtime::Handle,
    pub selected: AtomicUsize,
    #[cfg(any(windows, target_os = "macos"))]
    pub remote: Option<crate::frontend::Remote>,
}
pub static CONTEXT: OnceLock<Context> = OnceLock::new();
#[cfg(any(windows, target_os = "macos"))]
unsafe extern "C" {
    fn wb_native_run();
    fn wb_native_show();
    fn wb_native_quit();
    fn wb_native_error(message: *const c_char);
}
pub fn run() {
    #[cfg(any(windows, target_os = "macos"))]
    unsafe {
        wb_native_run()
    }
}
pub fn show() {
    #[cfg(any(windows, target_os = "macos"))]
    if let Some(ctx) = CONTEXT.get()
        && ctx.remote.is_none()
    {
        let _ = crate::frontend::spawn(
            &ctx.bridge.dir,
            ctx.bridge.port.load(std::sync::atomic::Ordering::Relaxed),
        );
        return;
    }
    #[cfg(any(windows, target_os = "macos"))]
    unsafe {
        wb_native_show()
    }
}
pub fn quit() {
    #[cfg(any(windows, target_os = "macos"))]
    unsafe {
        wb_native_quit()
    }
}
pub fn error(message: &str) {
    eprintln!("{message}");
    #[cfg(any(windows, target_os = "macos"))]
    {
        let m = CString::new(message.replace('\0', " ")).unwrap();
        unsafe { wb_native_error(m.as_ptr()) }
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
        unsafe {
            drop(CString::from_raw(p));
        }
    }
}
#[unsafe(no_mangle)]
pub extern "C" fn wb_select(index: i32) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        if let Some(ctx) = CONTEXT.get()
            && index >= 0
            && (index as usize) < ctx.bridge.config.lock().unwrap().connections.len()
        {
            ctx.selected.store(index as usize, Ordering::Relaxed);
        }
    }));
}
#[unsafe(no_mangle)]
pub extern "C" fn wb_value(name: *const c_char) -> *mut c_char {
    output(
        catch_unwind(AssertUnwindSafe(|| {
            let Some(ctx) = CONTEXT.get() else {
                return String::new();
            };
            let cfg = ctx.bridge.config.lock().unwrap();
            let index = ctx
                .selected
                .load(Ordering::Relaxed)
                .min(cfg.connections.len().saturating_sub(1));
            let c = cfg.connections.get(index).cloned().unwrap_or_default();
            match input(name).as_str() {
                "connections" => cfg
                    .connections
                    .iter()
                    .map(|c| c.label())
                    .collect::<Vec<_>>()
                    .join("\n"),
                "selected" => index.to_string(),
                "connection_count" => cfg.connections.len().to_string(),
                "name" => c.name,
                "provider" => c.provider,
                "model" => c.model,
                "upstream" => c.upstream,
                "reasoning_effort" => c.reasoning_effort,
                "url" => ctx.bridge.connection_address(&c.id),
                "local_key" => cfg.local_key.clone(),
                "status" => ctx.bridge.status.lock().unwrap().clone(),
                "has_key" => if c.api_key.is_empty() { "0" } else { "1" }.into(),
                "version" => env!("CARGO_PKG_VERSION").into(),
                _ => String::new(),
            }
        }))
        .unwrap_or_else(|_| "Internal error".into()),
    )
}
fn execute(mut p: serde_json::Value) -> Result<String, String> {
    let ctx = CONTEXT.get().ok_or("Service is not ready")?;
    {
        let cfg = ctx.bridge.config.lock().unwrap();
        if let Some(c) = cfg.connections.get(ctx.selected.load(Ordering::Relaxed)) {
            p["connection_id"] = serde_json::json!(c.id);
        }
    }
    #[cfg(any(windows, target_os = "macos"))]
    let result = if let Some(remote) = &ctx.remote {
        ctx.handle.block_on(remote.execute(&ctx.bridge.client, &p))
    } else {
        ctx.handle.block_on(ctx.bridge.command(&p))
    };
    #[cfg(not(any(windows, target_os = "macos")))]
    let result = ctx.handle.block_on(ctx.bridge.command(&p));
    if result.is_ok() {
        *ctx.bridge.config.lock().unwrap() = crate::config::Config::load(&ctx.bridge.dir)?;
        let count = ctx.bridge.config.lock().unwrap().connections.len();
        let index = if matches!(p["command"].as_str(), Some("add" | "duplicate")) {
            count.saturating_sub(1)
        } else {
            ctx.selected
                .load(Ordering::Relaxed)
                .min(count.saturating_sub(1))
        };
        ctx.selected.store(index, Ordering::Relaxed);
    }
    result
}
#[unsafe(no_mangle)]
pub extern "C" fn wb_save(
    name: *const c_char,
    provider: *const c_char,
    key: *const c_char,
    model: *const c_char,
    upstream: *const c_char,
    effort: *const c_char,
) -> *mut c_char {
    ffi_result(|| {
        execute(
            serde_json::json!({"command":"save","name":input(name),"provider":input(provider),"key":input(key),"model":input(model),"upstream":input(upstream),"reasoning_effort":input(effort)}),
        )
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn wb_command(name: *const c_char) -> *mut c_char {
    ffi_result(|| execute(serde_json::json!({"command":input(name)})))
}
fn ffi_result(f: impl FnOnce() -> Result<String, String>) -> *mut c_char {
    output(match catch_unwind(AssertUnwindSafe(f)) {
        Ok(Ok(message)) => format!("OK {message}"),
        Ok(Err(message)) => format!("ERROR {message}"),
        Err(_) => "ERROR Internal error".into(),
    })
}
