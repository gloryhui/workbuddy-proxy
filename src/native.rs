use crate::proxy::Bridge;
use std::{
    ffi::{CStr, CString, c_char},
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc, LazyLock, Mutex, OnceLock,
        atomic::{AtomicBool, AtomicUsize, Ordering},
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
    fn wb_native_show_process(pid: u32) -> i32;
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
#[cfg(any(windows, target_os = "macos"))]
pub fn show_process(pid: u32) -> bool {
    pid != 0 && unsafe { wb_native_show_process(pid) != 0 }
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
            && (index as usize) < ctx.bridge.config.lock().unwrap().providers.len()
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
                .min(cfg.providers.len().saturating_sub(1));
            let c = cfg.providers.get(index);
            match input(name).as_str() {
                "language" => cfg.language.clone(),
                "connections" => cfg
                    .providers
                    .iter()
                    .map(|c| {
                        if UI_ZH.load(Ordering::Relaxed) {
                            c.label()
                        } else {
                            format!("{} · {} models", c.name, c.models.len())
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
                "selected" => c.map_or_else(|| "-1".into(), |_| index.to_string()),
                "connection_count" => cfg.providers.len().to_string(),
                "has_provider" => if c.is_some() { "1" } else { "0" }.into(),
                "installed_count" => cfg.installed.len().to_string(),
                "port" => ctx.bridge.port.load(Ordering::Relaxed).to_string(),
                "name" => c
                    .filter(|p| !p.is_draft())
                    .map(|p| p.name.clone())
                    .unwrap_or_default(),
                "provider" => c.map(|p| p.name.clone()).unwrap_or_default(),
                "model_count" => cfg
                    .providers
                    .iter()
                    .map(|p| p.models.len())
                    .sum::<usize>()
                    .to_string(),
                "model" => c.map(|p| p.model_lines()).unwrap_or_default(),
                "upstream" => c.map(|p| p.upstream.clone()).unwrap_or_default(),
                "reasoning_effort" => c.map(|p| p.effort()).unwrap_or("keep").into(),
                "url" => c
                    .map(|p| ctx.bridge.connection_address(&p.id))
                    .unwrap_or_default(),
                "local_key" => cfg.local_key.clone(),
                "status" => ctx.bridge.status.lock().unwrap().clone(),
                "has_key" => if c.is_some_and(|p| !p.api_key.is_empty()) {
                    "1"
                } else {
                    "0"
                }
                .into(),
                "api_key" => c.map(|p| p.api_key.clone()).unwrap_or_default(),
                "version" => env!("CARGO_PKG_VERSION").into(),
                "update_url" => UPDATE.lock().unwrap()["url"].as_str().unwrap_or("").into(),
                "update_available" => if UPDATE.lock().unwrap()["available"] == true {
                    "1"
                } else {
                    "0"
                }
                .into(),
                "update_label" => {
                    let state = UPDATE.lock().unwrap();
                    let zh = UI_ZH.load(Ordering::Relaxed);
                    if state["available"] == true {
                        format!(
                            "{} {}",
                            if zh { "下载" } else { "Get" },
                            state["tag"].as_str().unwrap_or("")
                        )
                    } else if state["tag"].is_string() {
                        if zh { "已是最新" } else { "Up to date" }.into()
                    } else {
                        if zh { "检查更新" } else { "Check updates" }.into()
                    }
                }
                _ => String::new(),
            }
        }))
        .unwrap_or_else(|_| "Internal error".into()),
    )
}
fn execute(mut p: serde_json::Value) -> Result<String, String> {
    p["ui"] = serde_json::json!(true);
    let ctx = CONTEXT.get().ok_or("Service is not ready")?;
    {
        let cfg = ctx.bridge.config.lock().unwrap();
        if let Some(c) = cfg.providers.get(ctx.selected.load(Ordering::Relaxed)) {
            p["provider_id"] = serde_json::json!(c.id);
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
    if let Ok(message) = &result {
        if p["command"] == "set_port" {
            let applied: crate::port::Applied =
                serde_json::from_str(message).map_err(|_| "Invalid port response")?;
            ctx.bridge.port.store(applied.port, Ordering::Relaxed);
        }
        *ctx.bridge.config.lock().unwrap() = crate::config::Config::load(&ctx.bridge.dir)?;
        let count = ctx.bridge.config.lock().unwrap().providers.len();
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
    key: *const c_char,
    model: *const c_char,
    upstream: *const c_char,
    effort: *const c_char,
) -> *mut c_char {
    ffi_result(|| {
        execute(
            serde_json::json!({"command":"save","name":input(name),"key":input(key),"models":input(model),"upstream":input(upstream),"reasoning_effort":input(effort)}),
        )
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn wb_command(name: *const c_char) -> *mut c_char {
    let command = input(name);
    if command == "copy" {
        return output(
            match catch_unwind(AssertUnwindSafe(|| {
                execute(serde_json::json!({"command":"copy"}))
            })) {
                Ok(Ok(s)) => format!("OK {s}"),
                Ok(Err(e)) => format!(
                    "ERROR {}",
                    crate::i18n::message(&e, UI_ZH.load(Ordering::Relaxed))
                ),
                Err(_) => "ERROR Internal error".into(),
            },
        );
    }
    ffi_result(|| execute(serde_json::json!({"command":command})))
}
fn ffi_result(f: impl FnOnce() -> Result<String, String>) -> *mut c_char {
    output(match catch_unwind(AssertUnwindSafe(f)) {
        Ok(Ok(message)) => format!(
            "OK {}",
            crate::i18n::message(&message, UI_ZH.load(Ordering::Relaxed))
        ),
        Ok(Err(message)) => format!(
            "ERROR {}",
            crate::i18n::message(&message, UI_ZH.load(Ordering::Relaxed))
        ),
        Err(_) => "ERROR Internal error".into(),
    })
}

static UI_ZH: AtomicBool = AtomicBool::new(false);
#[unsafe(no_mangle)]
pub extern "C" fn wb_set_port(text: *const c_char) -> *mut c_char {
    ffi_result(|| {
        let port = input(text)
            .trim()
            .parse::<u16>()
            .ok()
            .filter(|n| *n > 0)
            .ok_or("Enter a port from 1 to 65535 / 请输入 1–65535 的端口号")?;
        let result = execute(serde_json::json!({"command":"set_port","port":port}))?;
        let applied: crate::port::Applied =
            serde_json::from_str(&result).map_err(|_| "Invalid port response")?;
        Ok(applied.message())
    })
}
static DRAFT: LazyLock<Mutex<Vec<serde_json::Value>>> = LazyLock::new(|| Mutex::new(Vec::new()));
#[unsafe(no_mangle)]
pub extern "C" fn wb_use_language(zh: i32) {
    UI_ZH.store(zh != 0, Ordering::Relaxed);
}
#[unsafe(no_mangle)]
pub extern "C" fn wb_ui_zh() -> i32 {
    i32::from(UI_ZH.load(Ordering::Relaxed))
}
#[unsafe(no_mangle)]
pub extern "C" fn wb_language(lang: *const c_char) -> *mut c_char {
    ffi_result(|| execute(serde_json::json!({"command":"language","language":input(lang)})))
}
#[unsafe(no_mangle)]
pub extern "C" fn wb_models_load() {
    if let Some(ctx) = CONTEXT.get() {
        let cfg = ctx.bridge.config.lock().unwrap();
        let rows = cfg
            .providers
            .get(ctx.selected.load(Ordering::Relaxed))
            .map(|p| {
                p.models
                    .iter()
                    .map(|m| serde_json::json!({"id":m.id,"name":m.name,"model":m.model}))
                    .collect()
            })
            .unwrap_or_default();
        *DRAFT.lock().unwrap() = rows;
    }
}
#[unsafe(no_mangle)]
pub extern "C" fn wb_models_count() -> i32 {
    DRAFT.lock().unwrap().len() as i32
}
#[unsafe(no_mangle)]
pub extern "C" fn wb_model(row: i32, column: i32) -> *mut c_char {
    let draft = DRAFT.lock().unwrap();
    output(
        draft
            .get(row as usize)
            .and_then(|m| m[if column == 0 { "name" } else { "model" }].as_str())
            .unwrap_or("")
            .into(),
    )
}
#[unsafe(no_mangle)]
pub extern "C" fn wb_model_set(row: i32, column: i32, text: *const c_char) {
    if let Some(m) = DRAFT.lock().unwrap().get_mut(row as usize) {
        let value = input(text);
        if column == 1 && (m["name"] == m["model"] || m["name"] == "") {
            m["name"] = serde_json::json!(value);
        }
        m[if column == 0 { "name" } else { "model" }] = serde_json::json!(value);
    }
}
#[unsafe(no_mangle)]
pub extern "C" fn wb_model_add() -> i32 {
    let mut rows = DRAFT.lock().unwrap();
    if rows.len() >= crate::config::MAX_MODELS {
        return -1;
    }
    rows.push(serde_json::json!({"name":"","model":""}));
    rows.len() as i32 - 1
}
#[unsafe(no_mangle)]
pub extern "C" fn wb_model_remove(row: i32) {
    let mut rows = DRAFT.lock().unwrap();
    if row >= 0 && (row as usize) < rows.len() {
        rows.remove(row as usize);
    }
}
#[unsafe(no_mangle)]
pub extern "C" fn wb_save_table(
    name: *const c_char,
    key: *const c_char,
    upstream: *const c_char,
    effort: *const c_char,
) -> *mut c_char {
    ffi_result(|| {
        execute(
            serde_json::json!({"command":"save","name":input(name),"key":input(key),"upstream":input(upstream),"reasoning_effort":input(effort),"models":DRAFT.lock().unwrap().clone()}),
        )
    })
}
fn remote_call(mut p: serde_json::Value) -> Result<String, String> {
    p["ui"] = serde_json::json!(true);
    let ctx = CONTEXT.get().ok_or("Service is not ready")?;
    if let Some(c) = ctx
        .bridge
        .config
        .lock()
        .unwrap()
        .providers
        .get(ctx.selected.load(Ordering::Relaxed))
    {
        p["provider_id"] = serde_json::json!(c.id);
    }
    #[cfg(any(windows, target_os = "macos"))]
    if let Some(remote) = &ctx.remote {
        return ctx.handle.block_on(remote.execute(&ctx.bridge.client, &p));
    }
    ctx.handle.block_on(ctx.bridge.command(&p))
}
#[unsafe(no_mangle)]
pub extern "C" fn wb_fetch(upstream: *const c_char, key: *const c_char) -> *mut c_char {
    ffi_result(|| {
        let result = remote_call(
            serde_json::json!({"command":"fetch","upstream":input(upstream),"key":input(key)}),
        )?;
        let models: Vec<String> =
            serde_json::from_str(&result).map_err(|_| "Invalid model list")?;
        let mut rows = DRAFT.lock().unwrap();
        let missing: Vec<_> = models
            .iter()
            .filter(|id| !rows.iter().any(|m| m["model"].as_str() == Some(id)))
            .collect();
        let other = CONTEXT
            .get()
            .map(|ctx| {
                let cfg = ctx.bridge.config.lock().unwrap();
                cfg.providers
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| *i != ctx.selected.load(Ordering::Relaxed))
                    .map(|(_, p)| p.models.len())
                    .sum::<usize>()
            })
            .unwrap_or(0);
        if rows.len() + missing.len() + other > crate::config::MAX_MODELS {
            return Err("At most 256 models".into());
        }
        let count = missing.len();
        for id in missing {
            rows.push(serde_json::json!({"name":id,"model":id}));
        }
        Ok(if UI_ZH.load(Ordering::Relaxed) {
            format!("获取 {} 个模型，新增 {count} 项；保存后生效", models.len())
        } else {
            format!(
                "Fetched {} models, {count} added; save to apply",
                models.len()
            )
        })
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn wb_snapshot() -> *mut c_char {
    let result = (|| -> Result<String, String> {
        let data = remote_call(serde_json::json!({"command":"snapshot"}))?;
        let v: serde_json::Value =
            serde_json::from_str(&data).map_err(|_| "Invalid core response")?;
        let zh = UI_ZH.load(Ordering::Relaxed);
        let mut text = if zh {
            format!(
                "本地服务\n{}\n\n供应商  {}    模型  {}\n累计请求  {}    运行  {} 秒\n\n",
                v["url"].as_str().unwrap_or(""),
                v["providers"],
                v["models"],
                v["requests"],
                v["uptime"]
            )
        } else {
            format!(
                "Local service\n{}\n\nProviders  {}    Models  {}\nRequests  {}    Uptime  {} s\n\n",
                v["url"].as_str().unwrap_or(""),
                v["providers"],
                v["models"],
                v["requests"],
                v["uptime"]
            )
        };
        let mut rows: Vec<(u64, String)> = v["logs"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|row| Some((row["time"].as_u64()?, row["message"].as_str()?.to_owned())))
            .collect();
        if rows.is_empty() {
            rows.push((
                crate::journal::now_ms(),
                v["status"].as_str().unwrap_or("Ready").into(),
            ));
        }
        text.push_str(&crate::journal::render(rows, zh));
        Ok(text)
    })();
    output(result.unwrap_or_else(|e| crate::i18n::message(&e, UI_ZH.load(Ordering::Relaxed))))
}

#[unsafe(no_mangle)]
pub extern "C" fn wb_record(text: *const c_char) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        if let Some(ctx) = CONTEXT.get() {
            let mut message = input(text);
            if let Some(s) = message.strip_prefix("OK ") {
                message = s.to_owned();
            }
            #[cfg(any(windows, target_os = "macos"))]
            if let Some(remote) = &ctx.remote {
                message = message.replace(&remote.key, "[redacted]");
            }
            let message = ctx.bridge.sanitize(message);
            let mut sender = RECORDS.lock().unwrap();
            let sender = sender.get_or_insert_with(|| {
                let (sender, mut receiver) = tokio::sync::mpsc::channel::<String>(100);
                ctx.handle.spawn(async move {
                    while let Some(message) = receiver.recv().await {
                        let payload =
                            serde_json::json!({"command":"record","message":message,"ui":true});
                        #[cfg(any(windows, target_os = "macos"))]
                        if let Some(remote) = &ctx.remote {
                            let _ = tokio::time::timeout(
                                std::time::Duration::from_secs(3),
                                remote.execute(&ctx.bridge.client, &payload),
                            )
                            .await;
                            continue;
                        }
                        let _ = ctx.bridge.command(&payload).await;
                    }
                });
                sender
            });
            let _ = sender.try_send(message);
        }
    }));
}
static RECORDS: LazyLock<Mutex<Option<tokio::sync::mpsc::Sender<String>>>> =
    LazyLock::new(|| Mutex::new(None));

#[unsafe(no_mangle)]
pub extern "C" fn wb_message(text: *const c_char) -> *mut c_char {
    output(
        input(text)
            .lines()
            .map(|line| crate::i18n::message(line, UI_ZH.load(Ordering::Relaxed)))
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

static UPDATE: LazyLock<Mutex<serde_json::Value>> =
    LazyLock::new(|| Mutex::new(serde_json::Value::Null));
#[unsafe(no_mangle)]
pub extern "C" fn wb_check_update(force: i32) -> *mut c_char {
    ffi_result(|| {
        let raw = remote_call(serde_json::json!({"command":"check_update","force":force!=0}))?;
        let state: serde_json::Value =
            serde_json::from_str(&raw).map_err(|_| "Invalid release response")?;
        let zh = UI_ZH.load(Ordering::Relaxed);
        let message = if state["available"] == true {
            format!(
                "{} {}",
                if zh {
                    "发现新版本"
                } else {
                    "New version available:"
                },
                state["tag"].as_str().unwrap_or("")
            )
        } else {
            if zh {
                "当前已是最新版本"
            } else {
                "Already up to date"
            }
            .into()
        };
        *UPDATE.lock().unwrap() = state;
        Ok(message)
    })
}
