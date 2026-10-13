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
pub extern "C" fn wb_value(name: *const c_char) -> *mut c_char {
    output(
        catch_unwind(AssertUnwindSafe(|| {
            let Some(ctx) = CONTEXT.get() else {
                return String::new();
            };
            let c = ctx.bridge.settings();
            match input(name).as_str() {
                "model" => c.model,
                "upstream" => c.upstream,
                "url" => ctx.bridge.address(),
                "local_key" => c.local_key,
                "status" => ctx.bridge.status.lock().unwrap().clone(),
                "has_key" => {
                    if c.api_key.is_empty() {
                        "0".into()
                    } else {
                        "1".into()
                    }
                }
                "version" => env!("CARGO_PKG_VERSION").into(),
                _ => String::new(),
            }
        }))
        .unwrap_or_else(|_| "Internal error".into()),
    )
}
#[unsafe(no_mangle)]
pub extern "C" fn wb_save(
    key: *const c_char,
    model: *const c_char,
    upstream: *const c_char,
) -> *mut c_char {
    ffi_result(|| {
        let ctx = CONTEXT.get().ok_or("Service is not ready")?;
        #[cfg(any(windows, target_os = "macos"))]
        if let Some(remote) = &ctx.remote {
            let message = ctx.handle.block_on(remote.execute(
                &ctx.bridge.client,
                "save",
                &input(key),
                &input(model),
                &input(upstream),
            ))?;
            *ctx.bridge.config.lock().unwrap() = crate::config::Config::load(&ctx.bridge.dir)?;
            return Ok(message);
        }
        let mut cfg = ctx.bridge.config.lock().unwrap();
        let mut next = cfg.clone();
        let key = input(key).trim().to_owned();
        if !key.is_empty() {
            next.api_key = key;
        }
        next.model = input(model).trim().to_owned();
        next.upstream = input(upstream).trim().to_owned();
        next.validate()?;
        if next.api_key.is_empty() {
            return Err("Enter your upstream API Key / 请填写上游 API Key".into());
        }
        crate::config::write_json(&ctx.bridge.dir.join("config.json"), &next)?;
        *cfg = next;
        Ok("Saved / 已保存（Key 留空可保留原值）".into())
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn wb_command(name: *const c_char) -> *mut c_char {
    ffi_result(|| {
        let ctx = CONTEXT.get().ok_or("Service is not ready")?;
        #[cfg(any(windows, target_os = "macos"))]
        if let Some(remote) = &ctx.remote {
            return ctx.handle.block_on(remote.execute(
                &ctx.bridge.client,
                &input(name),
                "",
                "",
                "",
            ));
        }
        match input(name).as_str() {
            "test" => ctx.handle.block_on(ctx.bridge.test()),
            "install" => {
                ctx.bridge.configure_workbuddy(&crate::workbuddy::path()?)?;
                Ok("WorkBuddy configured. Reopen model settings or restart WorkBuddy. / 已备份并配置，重新打开模型设置或重启 WorkBuddy。".into())
            }
            "restore" => {
                ctx.bridge.restore_workbuddy(&crate::workbuddy::path()?)?;
                Ok("Previous model restored / 已恢复原模型配置".into())
            }
            "copy" => {
                let c = ctx.bridge.settings();
                Ok(format!(
                    "URL: {}\nAPI Key: {}\nModel: {}",
                    ctx.bridge.address(),
                    c.local_key,
                    c.model
                ))
            }
            "quit" => {
                if let Some(stop) = ctx.bridge.stop.lock().unwrap().take() {
                    let _ = stop.send(());
                }
                Ok("Stopping / 正在退出".into())
            }
            _ => Err("Unknown command".into()),
        }
    })
}
fn ffi_result(f: impl FnOnce() -> Result<String, String>) -> *mut c_char {
    output(match catch_unwind(AssertUnwindSafe(f)) {
        Ok(Ok(message)) => format!("OK {message}"),
        Ok(Err(message)) => format!("ERROR {message}"),
        Err(_) => "ERROR Internal error".into(),
    })
}
