//! Shared native Studio V2 tokens (Figma 51:2 / implementation contract 71:364).
//! Existing Skin palettes still own accents and saved appearance preferences.
//!
//! Business values (pagination sizes, retention days, token thresholds,
//! request limits and similar) must never be added here.
#![allow(dead_code)]

use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::*;
use std::sync::Arc;
mod skin;
pub use skin::{Skin, current_skin, load_skin, palette, save_skin};

pub fn apply_skin(skin: Skin, cx: &mut App) {
    skin::select(skin);
    install(cx);
}

/// A monochrome accent must remain visible on the midnight navigation surface.
pub fn sidebar_accent() -> u32 {
    if current_skin() == Skin::Monochrome {
        color::SIDEBAR_TEXT
    } else {
        palette().primary
    }
}

pub fn sidebar_selection() -> u32 {
    if current_skin() == Skin::Monochrome {
        0x52525b
    } else {
        palette().primary
    }
}

/// Design colours from DESIGN_SPEC §1.1 and the component states in §3.
pub mod color {
    pub const SUCCESS: u32 = 0x11976f;
    pub const SUCCESS_TINT: u32 = 0xe9f8f1;

    pub const TEXT_1: u32 = 0x161c2c;
    pub const TEXT_2: u32 = 0x4c566d;
    pub const TEXT_3: u32 = 0x778196;

    pub const BORDER: u32 = 0xe7eaf2;
    pub const BORDER_INPUT: u32 = 0xd8dde9;
    pub const DIVIDER: u32 = 0xeef0f3;
    pub const CHECKBOX_BORDER: u32 = 0xc7ccd6;

    pub const BG_PAGE: u32 = 0xf7f8fc;
    pub const BG_BAR: u32 = 0xffffff;
    pub const BG_TABLE_HEAD: u32 = 0xf3f5fa;
    pub const BG_NOTICE: u32 = 0xf4f6ff;
    pub const SIDEBAR: u32 = 0x111525;
    pub const SIDEBAR_TEXT: u32 = 0xf0f3fe;
    pub const SIDEBAR_MUTED: u32 = 0x929bb0;
    pub const SIDEBAR_LINE: u32 = 0x2c3246;
    pub const SIDEBAR_ACTIVE: u32 = 0x2d3251;
    pub const SIDEBAR_HOVER: u32 = 0x20263a;
    pub const DESTRUCTIVE: u32 = 0xc94f5e;
    pub const DESTRUCTIVE_TINT: u32 = 0xfff0f0;
    pub const DESTRUCTIVE_HOVER: u32 = 0xb74351;
    pub const SIDEBAR_DANGER: u32 = DESTRUCTIVE;
    pub const TERMINAL: u32 = 0x171c2c;
    pub const TERMINAL_LINE: u32 = 0x2d3548;
    pub const TERMINAL_TEXT: u32 = 0xadb8ca;
    pub const BG_SEG: u32 = 0xedeff3;
    pub const BG_DISABLED: u32 = 0xf3f4f6;
    pub const BG_MENU_SELECTED: u32 = 0xf3f5f8;
    pub const BG_TAG: u32 = 0xf0f2f5;

    pub const DANGER: u32 = 0xc2410c;
    pub const DANGER_BG: u32 = 0xfff3ee;
    pub const DANGER_BORDER: u32 = 0xf5c6b3;
    pub const WARNING: u32 = 0xb4690e;

    /// Shared Studio overlay: rgba(18, 23, 43, .49), from delete frames
    /// 78:1080 / 80:864, encoded as RGBA for Kit's native modal layer.
    pub const SCRIM_RGBA: u32 = 0x12172b7d;

    pub const CHART_INPUT: u32 = 0x6c61f8;
    pub const CHART_OUTPUT: u32 = 0x2e9e6b;
    pub const CHART_CACHE_READ: u32 = 0x28b7a6;
    pub const CHART_CACHE_WRITE: u32 = 0xb6bcc8;
    pub const CHART_COST: u32 = 0xc2185b;

    pub const WHITE: u32 = 0xffffff;
}

/// Typography sizes from DESIGN_SPEC §1.2. Sizes are logical pixels.
pub mod font {
    pub const PAGE_TITLE: f32 = 28.;
    pub const DIALOG_TITLE: f32 = 18.;
    /// The card-title range is 14–16; 15 is the shared middle value.
    pub const CARD_TITLE: f32 = 15.;
    pub const BODY: f32 = 13.;
    pub const LABEL: f32 = 12.;
    pub const SMALL: f32 = 12.;
    pub const KPI: f32 = 22.;
    pub const HERO: f32 = 44.;
    pub const MONO: f32 = 12.;
    pub const STATUS: f32 = 12.;
    pub const TAG: f32 = 11.;
}

/// Line heights paired with the sizes above.
pub mod line_height {
    pub const PAGE_TITLE: f32 = 41.;
    pub const DIALOG_TITLE: f32 = 24.;
    pub const BODY: f32 = 20.;
    pub const LABEL: f32 = 18.;
    pub const HERO: f32 = 52.;
    pub const TABLE_ROW: f32 = 20.;
}

/// Shared spacing values. These are visual gaps/padding, not business values.
pub mod space {
    pub const PAGE_TOP: f32 = 32.;
    pub const SYSTEM_TOOLBAR_TOP: f32 = 8.;
    pub const PAGE_X: f32 = 32.;
    pub const PAGE_BOTTOM: f32 = 16.;
    pub const SIDEBAR_X: f32 = 18.;
    pub const SIDEBAR_Y: f32 = 12.;
    pub const CARD_SM: f32 = 16.;
    pub const CARD_LG: f32 = 20.;
    pub const SECTION_GAP: f32 = 14.;
    pub const CARD_GAP: f32 = 12.;
    pub const ROW_GAP: f32 = 8.;
    pub const TIGHT: f32 = 4.;
    pub const SMALL: f32 = 6.;
    pub const DIALOG: f32 = 24.;
    pub const POPOVER: f32 = 6.;
    pub const SEGMENT: f32 = 3.;
    pub const TAG_X: f32 = 6.;
    pub const TAG_Y: f32 = 1.;
}

/// Corner radii from DESIGN_SPEC §1.3.
pub mod radius {
    pub const CONTROL: f32 = 9.;
    pub const TABLE: f32 = 10.;
    pub const CARD: f32 = 16.;
    pub const DIALOG: f32 = 18.;
    pub const SEGMENTED: f32 = 11.;
    pub const CHECKBOX: f32 = 4.;
    pub const MENU_OPTION: f32 = 7.;
    pub const TAG: f32 = 6.;
    pub const SIDEBAR: f32 = 12.;
}

/// Control, row and container dimensions from DESIGN_SPEC §1.3 and §2.
pub mod size {
    /// 256 keeps the three provider actions on one line: the row needs
    /// ~193px (widest label is the English "Delete"), plus footer padding,
    /// card border and both 18px sidebar insets.
    pub const SIDEBAR_WIDTH: f32 = 256.;
    pub const SIDEBAR_BRAND: f32 = 38.;
    pub const SIDEBAR_ICON: f32 = 18.;
    pub const SIDEBAR_STATUS: f32 = 104.;
    pub const SIDEBAR_STATUS_COMPACT: f32 = 76.;
    pub const RIGHT_RAIL_WIDTH: f32 = 300.;
    pub const TOP_ICON_ROW_HEIGHT: f32 = 36.;
    /// Native drag/control strip, visually continuous with sidebar and canvas.
    pub const NATIVE_TITLEBAR: f32 = 24.;
    /// Studio V2 SystemToolbar (Figma 103:442): seven 36px targets, six gaps.
    pub const SYSTEM_BUTTON: f32 = 36.;
    pub const SYSTEM_ICON: f32 = 20.;
    pub const SYSTEM_GAP: f32 = 8.;
    pub const SYSTEM_TOOLBAR_WIDTH: f32 = 300.;
    pub const STATUS_BAR_HEIGHT: f32 = 28.;
    pub const INPUT_HEIGHT: f32 = 40.;
    pub const SELECT_HEIGHT: f32 = 40.;
    pub const BUTTON_HEIGHT: f32 = 36.;
    /// DESIGN_SPEC §3 "按钮通用：内边距 0 14".
    pub const BUTTON_PADDING_X: f32 = 14.;
    /// Tighter horizontal padding for buttons that share a narrow rail.
    pub const BUTTON_X_TIGHT: f32 = 8.;
    pub const ICON_BUTTON: f32 = 32.;
    pub const DIALOG_ICON_BUTTON: f32 = 28.;
    /// Studio protocol editor (61:42): fixed monospace key column.
    pub const MAPPING_LABEL_WIDTH: f32 = 94.;
    /// Provider page: the thinking-effort select width in the design's action row.
    pub const EFFORT_SELECT_WIDTH: f32 = 200.;
    pub const LIST_ROW_HEIGHT: f32 = 36.;
    pub const SEGMENT_BUTTON_HEIGHT: f32 = 32.;
    pub const TABLE_HEADER_HEIGHT: f32 = 32.;
    pub const TABLE_ROW_HEIGHT: f32 = 56.;
    pub const NAV_ROW_HEIGHT: f32 = 43.;
    pub const PROVIDER_ROW_HEIGHT: f32 = 38.;
    pub const FOOTER_HEIGHT: f32 = 44.;
    pub const STATUS_DOT: f32 = 6.;
    pub const PROVIDER_DOT: f32 = 7.;
    pub const ICON: f32 = 14.;
    pub const ICON_LARGE: f32 = 16.;
    pub const PROGRESS_MIN_WIDTH: f32 = 44.;
    pub const PROGRESS_MAX_WIDTH: f32 = 110.;
    pub const PROGRESS_HEIGHT: f32 = 4.;
    pub const DIALOG_MIN_WIDTH: f32 = 520.;
    pub const DIALOG_MAX_WIDTH: f32 = 660.;
    /// Studio's four protocol variants (61:42/103/169/231) share a 620px shell.
    /// The model picker/editor keep their own independent dimensions.
    pub const DIALOG_PROTOCOL_WIDTH: f32 = 620.;
}

/// Border widths and focus treatment.
pub mod stroke {
    pub const BORDER: f32 = 1.;
    pub const CHECKBOX_BORDER: f32 = 1.5;
    pub const FOCUS_RING: f32 = 3.;
}

/// Shadow geometry/alpha from DESIGN_SPEC §1.3. Cards themselves have no shadow.
pub mod shadow {
    pub const DIALOG_Y: f32 = 20.;
    pub const DIALOG_BLUR: f32 = 60.;
    pub const DIALOG_ALPHA: f32 = 0.28;
    pub const POPOVER_Y: f32 = 10.;
    pub const POPOVER_BLUR: f32 = 30.;
    pub const POPOVER_ALPHA: f32 = 0.18;
    pub const TOOLBAR_Y: f32 = -8.;
    pub const TOOLBAR_BLUR: f32 = 12.;
    pub const TOOLBAR_SPREAD: f32 = -10.;
    pub const TOOLBAR_ALPHA: f32 = 0.18;
}

/// Installs the light theme on the existing Kit theme state.
///
/// `Theme::change` remains the only mode switch; `Theme::update` carries every
/// visual value. No page, window, key binding or platform lifecycle is touched.
pub fn install(cx: &mut App) {
    Theme::change(ThemeMode::Light, None, cx);

    // Font families are resolved from the fonts GPUI actually sees. A failed
    // or missing name is not evidence that the family is unavailable; it only
    // means this candidate cannot be selected. The Kit-resolved system family
    // remains the fallback when no explicit candidate is found.
    let installed = cx.text_system().all_font_names();
    let sans_family = resolve_sans(&installed);
    let mono_family = resolve_mono(&installed);

    Theme::update(cx, move |theme| {
        if let Some(family) = sans_family {
            theme.font_family = family;
        }
        if let Some(family) = mono_family {
            theme.mono_font_family = family;
        }
        theme.font_size = px(font::BODY);
        theme.mono_font_size = px(font::MONO);
        theme.radius = px(radius::CONTROL);
        theme.radius_lg = px(radius::CARD);
        theme.shadow = true;

        let colors = &mut theme.colors;
        colors.accent = rgb(crate::theme::palette().tint).into();
        colors.accent_foreground = rgb(crate::theme::palette().dark).into();
        colors.accordion = rgb(color::WHITE).into();
        colors.background = rgb(color::BG_PAGE).into();
        colors.border = rgb(color::BORDER).into();
        colors.button = rgb(color::WHITE).into();
        colors.button_active = rgb(color::BG_PAGE).into();
        colors.button_foreground = rgb(color::TEXT_2).into();
        colors.button_hover = rgb(color::BG_TABLE_HEAD).into();
        colors.button_danger = rgb(color::DANGER_BG).into();
        colors.button_danger_active = rgb(color::DANGER_BORDER).into();
        colors.button_danger_foreground = rgb(color::DANGER).into();
        colors.button_danger_hover = rgb(color::DANGER_BORDER).into();
        colors.button_info = rgb(crate::theme::palette().tint).into();
        colors.button_info_active = rgb(crate::theme::palette().border).into();
        colors.button_info_foreground = rgb(crate::theme::palette().dark).into();
        colors.button_info_hover = rgb(crate::theme::palette().border).into();
        colors.button_primary = rgb(crate::theme::palette().primary).into();
        colors.button_primary_active = rgb(crate::theme::palette().dark).into();
        colors.button_primary_foreground = rgb(color::WHITE).into();
        colors.button_primary_hover = rgb(crate::theme::palette().dark).into();
        colors.button_secondary = rgb(crate::theme::palette().tint).into();
        colors.button_secondary_active = rgb(crate::theme::palette().border).into();
        colors.button_secondary_foreground = rgb(crate::theme::palette().dark).into();
        colors.button_secondary_hover = rgb(crate::theme::palette().border).into();
        colors.button_success = rgb(color::SUCCESS).into();
        colors.button_success_active = rgb(color::SUCCESS).into();
        colors.button_success_foreground = rgb(color::WHITE).into();
        colors.button_success_hover = rgb(color::SUCCESS).into();
        colors.button_warning = rgb(color::WARNING).into();
        colors.button_warning_active = rgb(color::WARNING).into();
        colors.button_warning_foreground = rgb(color::WHITE).into();
        colors.button_warning_hover = rgb(color::WARNING).into();
        colors.caret = rgb(crate::theme::palette().primary).into();
        colors.chart_1 = rgb(color::CHART_INPUT).into();
        colors.chart_2 = rgb(color::CHART_OUTPUT).into();
        colors.chart_3 = rgb(color::CHART_CACHE_READ).into();
        colors.chart_4 = rgb(color::CHART_CACHE_WRITE).into();
        colors.chart_5 = rgb(color::CHART_COST).into();
        colors.chart_bearish = rgb(color::DANGER).into();
        colors.chart_bullish = rgb(color::SUCCESS).into();
        colors.chart_grid = rgb(color::DIVIDER).into();
        colors.danger = rgb(color::DANGER).into();
        colors.danger_active = rgb(color::DANGER).into();
        colors.danger_foreground = rgb(color::WHITE).into();
        colors.danger_hover = rgb(color::DANGER).into();
        colors.description_list_label = rgb(color::BG_TABLE_HEAD).into();
        colors.description_list_label_foreground = rgb(color::TEXT_2).into();
        colors.drag_border = rgb(crate::theme::palette().border).into();
        colors.drop_target = rgb(crate::theme::palette().tint).into();
        colors.foreground = rgb(color::TEXT_1).into();
        colors.group_box = rgb(color::WHITE).into();
        colors.group_box_foreground = rgb(color::TEXT_1).into();
        colors.info = rgb(crate::theme::palette().tint).into();
        colors.info_active = rgb(crate::theme::palette().border).into();
        colors.info_foreground = rgb(crate::theme::palette().dark).into();
        colors.info_hover = rgb(crate::theme::palette().border).into();
        colors.input = rgb(color::BORDER_INPUT).into();
        colors.link = rgb(crate::theme::palette().dark).into();
        colors.link_active = rgb(crate::theme::palette().dark).into();
        colors.link_hover = rgb(crate::theme::palette().dark).into();
        colors.list = rgb(color::WHITE).into();
        colors.list_active = rgb(crate::theme::palette().tint).into();
        colors.list_active_border = rgb(crate::theme::palette().border).into();
        colors.list_even = rgb(color::BG_PAGE).into();
        colors.list_head = rgb(color::BG_TABLE_HEAD).into();
        colors.list_hover = rgb(color::BG_TABLE_HEAD).into();
        colors.muted = rgb(color::BG_DISABLED).into();
        colors.muted_foreground = rgb(color::TEXT_3).into();
        colors.overlay = rgba(color::SCRIM_RGBA).into();
        colors.popover = rgb(color::WHITE).into();
        colors.popover_foreground = rgb(color::TEXT_1).into();
        colors.primary = rgb(crate::theme::palette().primary).into();
        colors.primary_active = rgb(crate::theme::palette().dark).into();
        colors.primary_foreground = rgb(color::WHITE).into();
        colors.primary_hover = rgb(crate::theme::palette().dark).into();
        colors.progress_bar = rgb(crate::theme::palette().dark).into();
        colors.ring = rgb(crate::theme::palette().ring).into();
        colors.scrollbar = rgb(color::BG_PAGE).into();
        colors.scrollbar_thumb = rgb(color::BORDER).into();
        colors.scrollbar_thumb_hover = rgb(color::TEXT_3).into();
        colors.secondary = rgb(crate::theme::palette().tint).into();
        colors.secondary_active = rgb(crate::theme::palette().border).into();
        colors.secondary_foreground = rgb(crate::theme::palette().dark).into();
        colors.secondary_hover = rgb(crate::theme::palette().border).into();
        colors.selection = rgb(crate::theme::palette().ring).into();
        colors.sidebar = rgb(color::SIDEBAR).into();
        colors.sidebar_accent = rgb(color::SIDEBAR_ACTIVE).into();
        colors.sidebar_accent_foreground = rgb(color::SIDEBAR_TEXT).into();
        colors.sidebar_border = rgb(color::SIDEBAR_LINE).into();
        colors.sidebar_foreground = rgb(color::SIDEBAR_TEXT).into();
        colors.sidebar_primary = rgb(crate::theme::palette().primary).into();
        colors.sidebar_primary_foreground = rgb(color::WHITE).into();
        colors.skeleton = rgb(color::BG_DISABLED).into();
        colors.slider_bar = rgb(color::BORDER).into();
        colors.slider_thumb = rgb(crate::theme::palette().primary).into();
        colors.status_bar = rgb(color::BG_BAR).into();
        colors.status_bar_border = rgb(color::BORDER).into();
        colors.success = rgb(color::SUCCESS).into();
        colors.success_active = rgb(color::SUCCESS).into();
        colors.success_foreground = rgb(color::WHITE).into();
        colors.success_hover = rgb(color::SUCCESS).into();
        colors.switch = rgb(crate::theme::palette().primary).into();
        colors.switch_thumb = rgb(color::WHITE).into();
        colors.tab = rgb(color::WHITE).into();
        colors.tab_active = rgb(crate::theme::palette().tint).into();
        colors.tab_active_foreground = rgb(crate::theme::palette().dark).into();
        colors.tab_bar = rgb(color::WHITE).into();
        colors.tab_bar_segmented = rgb(color::BG_SEG).into();
        colors.tab_foreground = rgb(color::TEXT_2).into();
        colors.table = rgb(color::WHITE).into();
        colors.table_active = rgb(crate::theme::palette().tint).into();
        colors.table_active_border = rgb(crate::theme::palette().border).into();
        colors.table_even = rgb(color::WHITE).into();
        colors.table_foot = rgb(color::WHITE).into();
        colors.table_foot_foreground = rgb(color::TEXT_2).into();
        colors.table_head = rgb(color::BG_TABLE_HEAD).into();
        colors.table_head_foreground = rgb(color::TEXT_2).into();
        colors.table_hover = rgb(color::BG_TABLE_HEAD).into();
        colors.table_row_border = rgb(color::DIVIDER).into();
        colors.title_bar = rgb(color::BG_BAR).into();
        colors.title_bar_border = rgb(color::BORDER).into();
        colors.warning = rgb(color::WARNING).into();
        colors.warning_active = rgb(color::WARNING).into();
        colors.warning_foreground = rgb(color::WHITE).into();
        colors.warning_hover = rgb(color::WARNING).into();
        colors.window_border = rgb(color::BORDER).into();
        colors.red = rgb(color::DANGER).into();
        colors.red_light = rgb(color::DANGER_BG).into();
        colors.green = rgb(color::SUCCESS).into();
        colors.green_light = rgb(color::SUCCESS_TINT).into();
        colors.blue = rgb(0x3b6fe0).into();
        colors.blue_light = rgb(0xdbeafe).into();
        colors.yellow = rgb(color::WARNING).into();
        colors.yellow_light = rgb(color::BG_TABLE_HEAD).into();
    });
}

/// A failed lookup is only a missed candidate, never proof that a family is
/// absent. Comparison is case-insensitive because font registries differ.
fn first_installed(installed: &[String], candidates: &[&str]) -> Option<SharedString> {
    candidates.iter().find_map(|candidate| {
        installed
            .iter()
            .find(|name| name.eq_ignore_ascii_case(candidate))
            .cloned()
            .map(SharedString::from)
    })
}

/// Sans serif deliberately keeps the system-resolved family unless a platform
/// has an explicit candidate that GPUI reports as installed.
fn resolve_sans(installed: &[String]) -> Option<SharedString> {
    #[cfg(target_os = "windows")]
    {
        first_installed(
            installed,
            &["Segoe UI Variable", "Segoe UI", "Microsoft YaHei UI"],
        )
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = installed;
        None
    }
}

/// Monospace candidates. macOS and Windows were checked against the candidate
/// order in DESIGN_SPEC §1.2; the runtime list remains the authority, so an
/// unavailable name is skipped rather than assumed present.
fn resolve_mono(installed: &[String]) -> Option<SharedString> {
    #[cfg(target_os = "macos")]
    const CANDIDATES: &[&str] = &["SF Mono", "SFMono-Regular", "Menlo", "Monaco"];
    #[cfg(target_os = "windows")]
    const CANDIDATES: &[&str] = &["Consolas", "Cascadia Mono"];
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    const CANDIDATES: &[&str] = &["DejaVu Sans Mono", "Noto Sans Mono"];

    first_installed(installed, CANDIDATES)
}

/// OpenType `tnum`, identical to the Kit's own time-field figures. It is a
/// per-text feature; pages apply it when they migrate in T02–T08.
pub fn tabular_figures() -> FontFeatures {
    FontFeatures(Arc::new(vec![("tnum".into(), 1)]))
}
