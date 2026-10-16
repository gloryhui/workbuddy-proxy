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
#[allow(dead_code)]
#[path = "../../src/diagnostic.rs"]
mod diagnostic;
#[allow(dead_code)]
#[path = "../../src/i18n.rs"]
mod i18n;
mod model_options;
mod model_table;
mod presets;
mod protocol_options;
mod state;
mod statistics;
mod stats_backend;
mod stats_controller;
#[allow(dead_code)]
#[path = "../../src/stats/dto.rs"]
mod stats_dto;
mod ui;
#[allow(dead_code)]
#[path = "../../src/protocol/config.rs"]
mod upstream_config;
mod usage_chart;
mod usage_table;
mod workbuddy_restart;
use fs2::FileExt;
use gpui_kit::component::{Theme, ThemeMode, TitleBar};
use gpui_kit::*;
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
            Theme::change(ThemeMode::Light, None, cx);
            Theme::update(cx, |theme| {
                theme.radius = px(8.);
                theme.radius_lg = px(12.);
                theme.colors.foreground = rgb(0x19334d).into();
                theme.colors.muted_foreground = rgb(0x6b7c90).into();
                theme.colors.border = rgb(0xe2e9f1).into();
                theme.colors.sidebar = rgb(0xeaf2f8).into();
                theme.colors.sidebar_foreground = rgb(0x19334d).into();
                theme.colors.sidebar_accent = rgb(0xc5ebf4).into();
                theme.colors.sidebar_accent_foreground = rgb(0x006b83).into();
                theme.colors.table_active = rgb(0xdff3fa).into();
                theme.colors.table_hover = rgb(0xeef8fc).into();
                theme.colors.table_head = rgb(0xf7f9fc).into();
                theme.colors.table_head_foreground = rgb(0x6b7c90).into();
                theme.colors.table_row_border = rgb(0xe2e9f1).into();
                theme.colors.primary = rgb(0x008daa).into();
                theme.colors.primary_hover = rgb(0x007e99).into();
                theme.colors.primary_active = rgb(0x006e86).into();
                theme.colors.primary_foreground = rgb(0xffffff).into();
                theme.colors.button_primary = rgb(0x008daa).into();
                theme.colors.button_primary_hover = rgb(0x007e99).into();
                theme.colors.button_primary_active = rgb(0x006e86).into();
                theme.colors.button_primary_foreground = rgb(0xffffff).into();
            });
            cx.bind_keys([
                KeyBinding::new("cmd-s", ui::Save, Some("Proxy")),
                KeyBinding::new("ctrl-s", ui::Save, Some("Proxy")),
                KeyBinding::new("delete", ui::RemoveModels, Some("ModelList")),
                KeyBinding::new("backspace", ui::RemoveModels, Some("ModelList")),
                KeyBinding::new("cmd-a", ui::SelectModels, Some("ModelList")),
                KeyBinding::new("ctrl-a", ui::SelectModels, Some("ModelList")),
                KeyBinding::new("f2", ui::EditModel, Some("ModelList")),
                KeyBinding::new("enter", ui::EditModel, Some("ModelList")),
                KeyBinding::new("escape", ui::Dismiss, Some("Proxy")),
                KeyBinding::new("cmd-q", ui::Quit, None),
            ]);
            let bounds = Bounds::centered(None, size(px(1180.), px(780.)), cx);
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    window_min_size: Some(size(px(1100.), px(700.))),
                    ..TitleBar::window_options()
                },
                cx,
                move |window, cx| cx.new(|cx| ui::Proxy::new(dir, cfg, window, cx)),
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
