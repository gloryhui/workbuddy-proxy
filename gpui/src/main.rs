#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
// The statistics page builds large `IntoElement` trees; the default limit is
// not enough for the generated element types.
#![recursion_limit = "256"]
//! GPUI 设置窗口可执行程序的入口与模块装配。
//!
//! 本文件运行在独立的 `workbuddy-proxy-gpui` 进程中，负责解析设置进程参数、
//! 取得单实例锁、加载已存在的配置、注册快捷键并创建窗口。窗口内的
//! `ui::Proxy` 和 `statistics::Statistics` 归 GPUI 应用线程/上下文所有；
//! 供应商管理请求和统计查询分别交给它们自己创建的 `settings-control` 与
//! `statistics-control` 标准线程执行。后台线程只通过 channel 回传消息，
//! 不会直接修改 Entity。
//!
//! 桌面角色至少包括设置进程、后台 Core 与托盘控制进程；本文件只负责设置
//! 进程。设置进程相对 Core 和托盘控制器分别都是独立操作系统进程，不能把
//! 这三个角色简化成“总共两个进程”，也不能仅凭本入口推断另外两角色的具体
//! 进程数量。角色之间通过 `instance.json`、`gpui-*.json` 等状态文件及本机
//! 管理 HTTP 协作。
extern crate gpui_kit as gpui;
extern crate url as reqwest;
mod assets;
mod backend;
// `#[path]` 只让 Cargo 在编译 GPUI crate 时从另一个目录读取这些模块源码。
// 它会生成属于本 crate/本进程的类型与代码；`Config`、DTO 等对象不会因此在
// Core 与设置进程之间共享内存，运行时一致性仍由文件和本机管理 API 维持。
#[allow(dead_code)]
#[path = "../../src/capabilities.rs"]
mod capabilities;
#[allow(dead_code)]
#[path = "../../src/config.rs"]
mod config;
mod config_refresh;
#[allow(dead_code)]
#[path = "../../src/diagnostic.rs"]
mod diagnostic;
mod draft_discovery;
#[allow(dead_code)]
#[path = "../../src/i18n.rs"]
mod i18n;
mod model_options;
mod model_table;
mod picker_projection;
mod presets;
mod protocol_options;
mod provider_connection;
mod provider_create;
mod provider_filter;
mod source_filter;
mod state;
mod statistics;
mod stats_backend;
mod stats_controller;
#[allow(dead_code)]
#[path = "../../src/stats/dto.rs"]
mod stats_dto;
mod theme;
mod ui;
mod ui_components;
#[path = "../../src/updater/dto.rs"]
mod update_dto;
mod update_monitor;
#[allow(dead_code)]
#[path = "../../src/protocol/config.rs"]
mod upstream_config;
mod usage_chart;
mod usage_table;
mod workbuddy_import;
mod workbuddy_restart;
#[path = "../../src/workbuddy_status.rs"]
mod workbuddy_status;
use fs2::FileExt;
use gpui_kit::component::TitleBar;
use gpui_kit::*;

/// Keep unchanged page content separate from Kit's window-layer dialogs.
/// The pinned retained backend replays accessibility nodes and actions when
/// this sibling is reused; Proxy notifications and bounds changes invalidate it.
///
/// GPUI 中 `Entity<T>` 是应用上下文登记、可安全共享的实体句柄，不等同于
/// Spring 容器按类型注入的 Bean。`Proxy` 在窗口创建闭包里由 `cx.new`
/// 显式构造；这里保存的是句柄，渲染时 `.clone()` 只增加句柄所有权/引用，
/// 不会复制 `Proxy` 的字段、草稿或队列。若需观察 Entity 生命周期，可像
/// 本项目的其他位置一样使用 `WeakEntity` 的 `downgrade()`。
struct WindowContent {
    proxy: Entity<ui::Proxy>,
}

impl Render for WindowContent {
    // Fail to compile if the facade is switched back to a backend that does
    // not provide the retained accessibility replay this cache relies on.
    fn render(&mut self, _: &mut retained_gpui::Window, _: &mut Context<Self>) -> impl IntoElement {
        self.proxy
            .clone()
            .cached(StyleRefinement::default().size_full())
    }
}
pub(crate) fn default_window_size() -> Size<Pixels> {
    size(px(1440.), px(900.))
}

fn main() {
    // 参数由 Core/托盘控制进程转交。`--config-dir` 在加载配置前写入环境变量，
    // 让共享的 `config::directory()` 解析到同一实例目录；更新进度模式则完全
    // 不创建设置窗口。
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--config-dir" => {
                if let Some(path) = args.next() {
                    unsafe {
                        std::env::set_var("WORKBUDDY_PROXY_CONFIG_DIR", path);
                    }
                }
            }
            "--update-progress" => {
                let Some(path) = args.next() else {
                    return;
                };
                update_monitor::run(std::path::PathBuf::from(path));
                return;
            }
            "--version" => {
                println!("{}", env!("CARGO_PKG_VERSION"));
                return;
            }
            _ => {}
        }
    }
    let dir = config::directory().expect("Cannot find configuration directory");
    config::make_private_dir(&dir).expect("Cannot create configuration directory");
    // `gpui.lock` 是本设置进程的跨进程单实例门闩。文件句柄由本函数持有到
    // 应用退出；第二个设置进程拿不到排他锁时只写入唤醒标记，然后结束。
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(dir.join("gpui.lock"))
        .expect("Cannot open settings lock");
    if lock.try_lock_exclusive().is_err() {
        let _ = config::write_json(
            &dir.join("gpui-wake.json"),
            &chrono::Local::now().timestamp_millis(),
        );
        return;
    }
    if !dir.join("instance.json").exists() {
        eprintln!("Start workbuddy-proxy first / 请先启动 WorkBuddy Proxy");
        return;
    }
    // Core 已创建 `instance.json` 才允许打开设置页。该文件同时含管理端口和
    // 管理令牌，真正读取并发送请求发生在后台 worker，而不是 UI 渲染循环。
    let _ = std::fs::remove_file(dir.join("gpui-stop.json"));
    let cfg = config::Config::load(&dir).expect("Cannot load configuration");
    gpui_kit::application()
        .with_assets(assets::Assets)
        // `move` 让外层闭包取得 `dir` 等值的所有权，避免应用启动后继续借用
        // `main` 中即将结束的栈帧。嵌套窗口闭包再次使用 `move` 把剩余所有权
        // 交给窗口创建逻辑；这不是 Spring 的作用域代理，也没有自动注入。
        .run(move |cx| {
            gpui_kit::init(cx);
            theme::apply_skin(theme::load_skin(&dir), cx);
            cx.bind_keys([
                KeyBinding::new("cmd-s", ui::Save, Some("Proxy")),
                KeyBinding::new("ctrl-s", ui::Save, Some("Proxy")),
                KeyBinding::new("delete", ui::RemoveModels, Some("ModelList")),
                KeyBinding::new("backspace", ui::RemoveModels, Some("ModelList")),
                KeyBinding::new("cmd-a", ui::SelectModels, Some("ModelList")),
                KeyBinding::new("ctrl-a", ui::SelectModels, Some("ModelList")),
                KeyBinding::new("f2", ui::EditModel, Some("ModelList")),
                KeyBinding::new("enter", ui::EditModel, Some("ModelList")),
                KeyBinding::new("enter", ui::ActivateProtocolTab, Some("ProtocolTab")),
                KeyBinding::new("space", ui::ActivateProtocolTab, Some("ProtocolTab")),
                KeyBinding::new("escape", ui::Dismiss, Some("ProxyModal")),
                KeyBinding::new("escape", ui::DismissModelInput, Some("ModelEditor > Input")),
                KeyBinding::new(
                    "enter",
                    ui::ActivateProviderProfileControl,
                    Some("ProviderProfileControl"),
                ),
                KeyBinding::new(
                    "space",
                    ui::ActivateProviderProfileControl,
                    Some("ProviderProfileControl"),
                ),
                KeyBinding::new(
                    "enter",
                    ui::ActivateProviderDeleteControl,
                    Some("ProviderDeleteControl"),
                ),
                KeyBinding::new(
                    "space",
                    ui::ActivateProviderDeleteControl,
                    Some("ProviderDeleteControl"),
                ),
                KeyBinding::new("escape", ui::Dismiss, Some("Proxy")),
                KeyBinding::new("cmd-q", ui::Quit, None),
            ]);
            cx.bind_keys(ui::model_navigation_bindings());
            let bounds = Bounds::centered(None, default_window_size(), cx);
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    window_min_size: Some(size(px(1100.), px(700.))),
                    ..TitleBar::window_options()
                },
                cx,
                // `cx.new` 显式创建两个受 GPUI Context 管理的 Entity：
                // `Proxy` 持有页面业务状态和两个后台 worker 的句柄，
                // `WindowContent` 只持有 Proxy 实体句柄。后续事件通过
                // `Context<Self>` 访问/更新它们，并调用 `cx.notify()` 标记重绘。
                move |window, cx| {
                    let proxy = cx.new(|cx| ui::Proxy::new(dir, cfg, window, cx));
                    cx.new(|_| WindowContent { proxy })
                },
            )
            .expect("Cannot open WorkBuddy Proxy window");
            cx.on_window_closed(|cx, _| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            // `detach()` 把订阅交给应用运行时持续持有；这里没有另存 `Task`，
            // 但仍允许回调在后续窗口关闭事件发生时执行。
            .detach();
            cx.activate(true);
        });
    drop(lock);
}

#[cfg(test)]
mod ui_contract_tests;
