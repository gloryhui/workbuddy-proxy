#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
// The statistics page builds large `IntoElement` trees; the default limit is
// not enough for the generated element types.
#![recursion_limit = "256"]
extern crate gpui_kit as gpui;
extern crate url as reqwest;
mod assets;
mod backend;
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
mod studio_fonts;
mod theme;
mod ui;
mod ui_components;
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
            "--version" => {
                println!("{}", env!("CARGO_PKG_VERSION"));
                return;
            }
            _ => {}
        }
    }
    let dir = config::directory().expect("Cannot find configuration directory");
    config::make_private_dir(&dir).expect("Cannot create configuration directory");
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
    let _ = std::fs::remove_file(dir.join("gpui-stop.json"));
    let cfg = config::Config::load(&dir).expect("Cannot load configuration");
    gpui_kit::application()
        .with_assets(assets::Assets)
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
            .detach();
            cx.activate(true);
        });
    drop(lock);
}

#[cfg(test)]
mod ui_contract_tests;
