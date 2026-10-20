//! Thin visual wrappers over the existing GPUI Kit controls.
//!
//! T02 owns only presentation: variants, sizes, disabled/busy wiring, dialog
//! chrome and pager/tag/segment appearance. Every wrapper returns the Kit
//! control, so callers keep their own `InputState`, `SelectState`, event
//! subscriptions, focus, keyboard/IME behavior and business callbacks.
#![allow(dead_code)]

use crate::theme::{color, font, radius, size};
use gpui_kit::component::{
    Disableable, Icon, IconName, Sizable,
    button::{Button, ButtonVariants},
    checkbox::Checkbox,
    dialog::{DialogContent, DialogFooter},
    input::{Input, InputState},
    label::Label,
    searchable_list::{SearchableListDelegate, SearchableListItem},
    select::{Select, SelectState},
    table::{DataTable, TableDelegate, TableState},
    tag::Tag,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

/// Shared button kinds. `Outline` is the primary-outline style used by the
/// secondary actions in the provider page; `DangerOutline` is the pale danger
/// button from the design's destructive rows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonKind {
    Primary,
    Outline,
    Normal,
    Danger,
    DangerOutline,
    Ghost,
}

/// Standard labelled button. A busy button is both disabled and loading, so a
/// second click cannot reach the caller's handler while work is in flight.
///
/// The height and horizontal padding are absolute pixels from DESIGN_SPEC §3
/// ("按钮通用：高 32，内边距 0 14"). The Kit's `small()` shorthand is
/// rem-relative, and this profile installs a 13px base font size, which made
/// every labelled button render ~19.5px tall instead of the specified 32px.
pub fn button(
    id: impl Into<ElementId>,
    kind: ButtonKind,
    label: impl Into<SharedString>,
    disabled: bool,
    busy: bool,
) -> Button {
    let button = Button::new(id)
        .small()
        .h(px(size::BUTTON_HEIGHT))
        .px(px(size::BUTTON_PADDING_X))
        .label(label)
        .disabled(disabled || busy)
        .loading(busy);
    match kind {
        ButtonKind::Primary => button.primary(),
        ButtonKind::Outline => button.primary().outline(),
        ButtonKind::Normal => button,
        ButtonKind::Danger => button.danger(),
        ButtonKind::DangerOutline => button.danger().outline(),
        ButtonKind::Ghost => button.ghost(),
    }
}

/// Icon button with the design's default 32px hit area.
pub fn icon_button(
    id: impl Into<ElementId>,
    icon: Icon,
    tooltip: impl Into<SharedString>,
    accessibility_label: impl Into<SharedString>,
    disabled: bool,
) -> Button {
    icon_button_sized(
        id,
        size::ICON_BUTTON,
        icon,
        tooltip,
        accessibility_label,
        disabled,
    )
}

/// Icon button for callers that already own a size (for example the compact
/// application header while T03 has not rebuilt the skeleton yet).
pub fn icon_button_sized(
    id: impl Into<ElementId>,
    button_size: f32,
    icon: Icon,
    tooltip: impl Into<SharedString>,
    accessibility_label: impl Into<SharedString>,
    disabled: bool,
) -> Button {
    Button::new(id)
        .with_size(px(button_size))
        .ghost()
        .icon(icon)
        .tooltip(tooltip)
        .accessibility_label(accessibility_label)
        .disabled(disabled)
}

/// `IconName` variant of [`icon_button`].
pub fn named_icon_button(
    id: impl Into<ElementId>,
    icon: IconName,
    tooltip: impl Into<SharedString>,
    accessibility_label: impl Into<SharedString>,
    disabled: bool,
) -> Button {
    named_icon_button_sized(
        id,
        size::ICON_BUTTON,
        icon,
        tooltip,
        accessibility_label,
        disabled,
    )
}

/// `IconName` variant of [`icon_button_sized`].
pub fn named_icon_button_sized(
    id: impl Into<ElementId>,
    button_size: f32,
    icon: IconName,
    tooltip: impl Into<SharedString>,
    accessibility_label: impl Into<SharedString>,
    disabled: bool,
) -> Button {
    Button::new(id)
        .with_size(px(button_size))
        .ghost()
        .icon(icon)
        .tooltip(tooltip)
        .accessibility_label(accessibility_label)
        .disabled(disabled)
}

/// Text input at the design's 36px field height. The caller's `InputState` is
/// passed through untouched.
pub fn text_input(state: &Entity<InputState>, disabled: bool) -> Input {
    // Input::h is an inherent multi-line-only API in Kit 0.7.3. Use Styled::h
    // so the single-line field's final style overrides its rem-based size.
    Styled::h(Input::new(state), px(size::INPUT_HEIGHT)).disabled(disabled)
}

/// Masked input (API keys) with the same field metrics and mask toggle.
pub fn masked_input(state: &Entity<InputState>, disabled: bool) -> Input {
    text_input(state, disabled).mask_toggle()
}

/// Filter/search input with compact text and the shared 36px hit area.
pub fn compact_input(state: &Entity<InputState>, disabled: bool) -> Input {
    text_input(state, disabled).small()
}

/// Select wrapper; the state, items and keyboard behavior stay with the caller.
pub fn select<D>(state: &Entity<SelectState<D>>, disabled: bool) -> Select<D>
where
    D: SearchableListDelegate + 'static,
    <D::Item as SearchableListItem>::Value: PartialEq + Clone,
{
    Select::new(state)
        .small()
        .h(px(size::SELECT_HEIGHT))
        .disabled(disabled)
}

/// Checkbox wrapper. The caller keeps ownership of `on_change`.
pub fn checkbox(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    checked: bool,
    disabled: bool,
) -> Checkbox {
    Checkbox::new(id)
        .label(label.into())
        .checked(checked)
        .disabled(disabled)
        .small()
}

/// One button in a segmented control. The row helper below only supplies the
/// visual grouping; each button keeps its own callback.
pub fn segment_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    active: bool,
    disabled: bool,
) -> Button {
    Button::new(id)
        .small()
        .label(label)
        .disabled(disabled)
        .when(active, |button| button.primary().outline())
        .when(!active, |button| button.ghost())
}

/// Visual container for a row of [`segment_button`]s.
pub fn segmented_row() -> Div {
    div().flex().items_center().gap_2()
}

/// Full-height surface used by the provider page (keeps the existing layout
/// contract of its old local `card` helper).
pub fn panel_card(id: impl Into<ElementId>) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .flex_col()
        .min_h_0()
        .h_full()
        .bg(rgb(color::WHITE))
        .border_1()
        .border_color(rgb(color::BORDER))
        .rounded(px(radius::CARD))
        .p_4()
        .gap_3()
}

/// Content-sized surface used by the statistics page (the local `stats_card`
/// contract: width follows content and only charts/tables set a height).
pub fn content_card(id: impl Into<ElementId>) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .flex_col()
        .w_full()
        .min_w_0()
        .bg(rgb(color::WHITE))
        .border_1()
        .border_color(rgb(color::BORDER))
        .rounded(px(radius::CARD))
        .p_3()
        .gap(px(6.))
}

/// Table frame with the shared 32px header and both scrollbars visible.
pub fn table<D: TableDelegate>(state: &Entity<TableState<D>>) -> DataTable<D> {
    DataTable::new(state)
        .small()
        .header_row_height(px(size::TABLE_HEADER_HEIGHT))
        .scrollbar_visible(true, true)
}

/// Request-log table: DESIGN_SPEC §4.1.5 fixes a 56px body row while the
/// header keeps the shared 32px height. Only the log uses this; the aggregate
/// tables stay on the default 30px rows.
pub fn log_table<D: TableDelegate>(state: &Entity<TableState<D>>) -> DataTable<D> {
    table(state).with_size(px(size::TABLE_ROW_HEIGHT))
}

/// Dialog body container: full width, shared gaps.
pub fn dialog_body() -> DialogContent {
    DialogContent::new().w_full().gap_3()
}

/// Dialog body constrained to the viewport with the same reserved/maximum
/// bounds the model and protocol dialogs already used.
pub fn dialog_body_sized(
    viewport_height: Pixels,
    reserved: f32,
    min: f32,
    max: f32,
) -> DialogContent {
    dialog_body().h((viewport_height - px(reserved)).clamp(px(min), px(max)))
}

/// Scrollable form area inside a dialog. It flexes and scrolls, so the footer
/// actions stay reachable on small windows and long forms.
pub fn scrollable_form(id: impl Into<ElementId>) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .flex_col()
        .gap_3()
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
}

/// Dialog title with the shared typography.
pub fn dialog_title(text: impl Into<SharedString>) -> Div {
    div()
        .text_size(px(font::DIALOG_TITLE))
        .font_weight(FontWeight::SEMIBOLD)
        .child(text.into().to_string())
}

/// Standard dialog footer container.
pub fn dialog_footer() -> DialogFooter {
    DialogFooter::new()
}

/// Pager button (`Previous` / page size / `Next`); callback stays with caller.
pub fn pager_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    disabled: bool,
) -> Button {
    Button::new(id)
        .small()
        .ghost()
        .label(label)
        .disabled(disabled)
}

/// Muted pager/status label.
pub fn pager_status(text: impl Into<SharedString>) -> Label {
    Label::new(text)
        .text_size(px(font::SMALL))
        .text_color(rgb(color::TEXT_3))
}

/// Tag variants used by the existing compact counters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TagKind {
    Primary,
    Secondary,
    Success,
    Warning,
    Danger,
}

/// Small tag with a caller-supplied label.
pub fn tag(kind: TagKind, label: impl Into<SharedString>) -> Tag {
    let tag = match kind {
        TagKind::Primary => Tag::primary(),
        TagKind::Secondary => Tag::secondary(),
        TagKind::Success => Tag::success(),
        TagKind::Warning => Tag::warning(),
        TagKind::Danger => Tag::danger(),
    };
    tag.small().child(label.into().to_string())
}
