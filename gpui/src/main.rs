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
pub(crate) fn default_window_size() -> Size<Pixels> {
    size(px(1440.), px(900.))
}

/// The source group's geometry is part of the layout contract: three buttons
/// fill the labelled track, including its fixed inset.
fn check_source_group_geometry() {
    assert_eq!(
        source_filter::GROUP_WIDTH,
        source_filter::ITEM_WIDTHS.iter().sum::<i32>()
            + 2 * source_filter::GROUP_INSET
            + 2 * source_filter::GROUP_GAP,
        "three options must fill the group"
    );
    assert_eq!(
        source_filter::GROUP_HEIGHT,
        source_filter::ITEM_HEIGHT + 2 * source_filter::GROUP_INSET
    );
    assert_eq!(source_filter::ICON_SIZE, 16);
}

/// The source group has exactly one answer at all times, and the default is
/// "all sources".
fn check_source_filter() {
    use crate::stats_dto::SourceScope;
    use source_filter::{SelectionOutcome, SourceFilter};

    let mut filter = SourceFilter::new();
    assert_eq!(
        filter.selected(),
        SourceScope::All,
        "the default is all sources"
    );
    assert_eq!(
        filter.select(SourceScope::All),
        SelectionOutcome::Unchanged,
        "clicking the current option is free"
    );
    assert_eq!(
        filter.move_selection(1),
        SelectionOutcome::Changed(SourceScope::Builtin)
    );
    assert_eq!(source_filter::OPTIONS.len(), 3, "every option is listed");
    let selected = source_filter::OPTIONS
        .iter()
        .filter(|option| filter.is_selected(option.scope))
        .count();
    assert_eq!(selected, 1, "exactly one option is selected");

    // Hover and focus are owned states, so the view can style them and show a
    // tooltip for both.
    filter.set_focused(true);
    assert!(filter.is_focused());
    filter.set_hovered(Some(SourceScope::Supplier));
    assert_eq!(filter.hovered(), Some(SourceScope::Supplier));
    filter.set_focused(false);
    assert_eq!(
        filter.hovered(),
        None,
        "leaving focus clears the hover state"
    );

    // A disabled group never changes its answer.
    filter.set_disabled(true);
    assert!(filter.is_disabled());
    assert_eq!(
        filter.move_selection(-1),
        SelectionOutcome::Unchanged,
        "a disabled group stays put"
    );
    filter.set_disabled(false);

    // Accessibility is state, not colour: a radio group of radio buttons, each
    // with the full label rather than the abbreviated one.
    assert_eq!(filter.group_role(), gpui_kit::Role::RadioGroup);
    assert_eq!(SourceFilter::option_role(), gpui_kit::Role::RadioButton);
    assert!(!filter.group_label().is_empty());
    for option in source_filter::OPTIONS {
        let label = SourceFilter::option_label(option.scope);
        assert!(label.contains('/'), "the label carries both languages");
    }
}

/// The import panel's contract: off by default, a failed switch rolls back, a
/// pause keeps the history, and the background cadence is fixed.
fn check_import_panel() {
    use workbuddy_import::{BACKGROUND_SYNC_SECONDS, ImportPanel, SwitchState};

    assert_eq!(BACKGROUND_SYNC_SECONDS, 5, "the cadence is a constant");
    let mut panel = ImportPanel::new();
    assert!(!panel.is_enabled(), "import is off by default");
    assert_eq!(panel.request_enable(), Some(true));
    panel.apply(Err("stats_unavailable".to_owned()));
    assert!(!panel.is_enabled(), "a refused switch rolls back");
    assert_eq!(panel.state(), SwitchState::Failed);
    // A plain refresh never reads the files.
    assert!(!panel.refresh_reads_jsonl());
    assert_eq!(
        panel.background_note(),
        workbuddy_import::BACKGROUND_SYNC_NOTE
    );

    // The six counters are readable by index, and unresolved rows keep the
    // panel honest about completion.
    assert_eq!(workbuddy_import::ImportCounters::ALL.len(), 6);
    let counters = workbuddy_import::ImportCounters {
        imported: 1,
        excluded_proxy: 2,
        pending: 3,
        conflict: 4,
        skipped: 5,
        deleted: 6,
        files_seen: 7,
    };
    for (index, expected) in [1, 2, 3, 4, 5, 6].iter().enumerate() {
        assert_eq!(counters.value(index), *expected);
    }
    assert!(counters.needs_attention());
    panel.apply(Ok(true));
    panel.set_counters(counters, crate::stats_dto::SourceScope::All);
    assert!(!panel.is_complete(), "pending rows are not 'synced'");
    assert_eq!(panel.scope(), crate::stats_dto::SourceScope::All);
    assert_eq!(panel.health_text(true), "已开启");
    assert!(panel.last_error().is_none(), "a success clears the error");

    // Pausing keeps every counter.
    assert_eq!(panel.request_disable(), Some(false));
    panel.apply(Ok(false));
    assert_eq!(panel.counters(), &counters, "a pause deletes nothing");
    assert_eq!(panel.health_text(false), "Off");
}

/// The source-confirmation dialog: nothing preselected, a fresh preview before
/// every confirmation, and no internal detail in what the user reads.
fn check_mapping_dialog() {
    use workbuddy_import::{
        Classification, DIALOG_MAX_HEIGHT_RATIO, DIALOG_MAX_WIDTH, DIALOG_MIN_WIDTH, DialogError,
        MappingDialog, PAGE_SIZES, Preview,
    };

    assert_eq!((DIALOG_MIN_WIDTH, DIALOG_MAX_WIDTH), (520, 720));
    assert_eq!(DIALOG_MAX_HEIGHT_RATIO, 0.8);
    assert_eq!(PAGE_SIZES, &[20, 50, 100]);

    let mut dialog = MappingDialog::new("custom-local:probe");
    assert_eq!(dialog.model_id(), "custom-local:probe");
    assert!(
        dialog.classification().is_none(),
        "nothing is selected by default"
    );
    assert!(!dialog.can_confirm(0), "and nothing can be confirmed yet");
    assert!(
        dialog.preview_request(0).is_none(),
        "no interval, no preview"
    );

    dialog.set_range(0, 1_000);
    assert!(
        dialog.preview_request(0).is_none(),
        "an interval alone is not enough: a classification is required"
    );
    dialog.choose(Classification::Direct);
    let request = dialog.preview_request(0).expect("the form is ready");
    assert_eq!(request.3, Classification::Direct);
    assert_eq!(Classification::Direct.as_str(), "direct");
    assert_eq!(Classification::Direct.label(true), "直连导入");
    assert_eq!(
        Classification::Proxy.label(false),
        "Exclude local proxy copies"
    );

    dialog.apply_preview(Ok(Preview {
        token: "token-1".into(),
        affected_count: 3,
        expires_at_ms: 100,
        watermark_rowid: 5,
        mapping_revision: 1,
    }));
    assert!(dialog.can_confirm(50));
    assert_eq!(dialog.take_token(50).as_deref(), Some("token-1"));
    assert!(
        !dialog.can_confirm(100),
        "an expired preview cannot be confirmed"
    );

    // A stale answer drops the preview and asks for a fresh one.
    dialog.apply_confirmation(Err("stats_workbuddy_stale_batch".into()));
    assert_eq!(dialog.error(), Some(DialogError::PreviewExpired));
    assert!(dialog.error().expect("set").needs_fresh_preview());
    assert!(dialog.preview().is_none());

    // Proxy evidence blocks a direct claim outright.
    dialog.choose(Classification::Direct);
    dialog.apply_preview(Err("stats_workbuddy_proxy_interval_protected".into()));
    assert_eq!(dialog.error(), Some(DialogError::ProxyEvidenceConflict));
    assert!(
        dialog.classification().is_none(),
        "the direct choice is cleared, not left standing"
    );

    // Paging only accepts the offered sizes.
    assert!(dialog.set_page(1, 50));
    assert_eq!((dialog.page(), dialog.page_size()), (1, 50));
    assert!(!dialog.set_page(1, 7), "7 is not an offered page size");

    // Nothing the user reads carries an internal code.
    for error in [
        DialogError::PreviewExpired,
        DialogError::RevisionConflict,
        DialogError::ProxyEvidenceConflict,
        DialogError::InvalidRange,
        DialogError::Unavailable,
    ] {
        for zh in [true, false] {
            let text = error.text(zh);
            assert!(!text.contains("stats_"), "no code leaks: {text}");
            assert!(!text.contains('/'), "no path leaks: {text}");
            assert!(!text.contains("SELECT"), "no SQL leaks: {text}");
        }
    }
    assert_eq!(dialog.confirm_label(true), "确认归属");
    assert_eq!(dialog.confirm_label(false), "Confirm source");

    // The flow that owns the dialog closes it the moment the page moves on: a
    // preview describes one page and stops meaning anything after that.
    let mut coordinator = stats_controller::FilterCoordinator::new();
    assert!(!coordinator.mapping().is_open());
    coordinator.mapping().open("custom-local:probe");
    assert!(coordinator.mapping().is_open());
    assert_eq!(
        coordinator
            .mapping()
            .dialog()
            .map(|dialog| dialog.model_id()),
        Some("custom-local:probe")
    );
    coordinator.change();
    assert!(
        !coordinator.mapping().is_open(),
        "a filter change closes it"
    );
}

fn main() {
    check_source_group_geometry();
    check_import_panel();
    check_mapping_dialog();
    check_source_filter();
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
            let bounds = Bounds::centered(None, default_window_size(), cx);
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

#[cfg(test)]
mod startup_contract_tests {
    #[test]
    fn startup_contract_matches_the_rendered_model() {
        super::check_mapping_dialog();
    }
}
