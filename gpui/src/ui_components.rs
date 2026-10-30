//! Thin visual wrappers over the existing GPUI Kit controls.
//!
//! T02 owns only presentation: variants, sizes, disabled/busy wiring, dialog
//! chrome and pager/tag/segment appearance. Every wrapper returns the Kit
//! control, so callers keep their own `InputState`, `SelectState`, event
//! subscriptions, focus, keyboard/IME behavior and business callbacks.
#![allow(dead_code)]

use crate::theme::{color, font, radius, size};
use gpui_kit::component::{
    ActiveTheme, Disableable, ElementExt, FocusableExt, Icon, IconName, Sizable,
    button::{Button, ButtonCustomVariant, ButtonVariants},
    checkbox::Checkbox,
    dialog::{DialogContent, DialogFooter},
    input::{Input, InputContentType, InputState, Textarea, TextareaState},
    label::Label,
    searchable_list::{SearchableListDelegate, SearchableListItem},
    select::{Select, SelectState},
    table::{DataTable, TableDelegate, TableState},
    tag::Tag,
    tooltip::Tooltip,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

/// Keep complete values inside the native tooltip, including unbroken IDs.
pub fn full_value_tooltip(value: String, window: &mut Window, cx: &mut App) -> AnyView {
    Tooltip::element(move |_, cx| {
        Label::new(value.clone())
            .max_w(px(520.))
            .whitespace_normal()
            .text_color(cx.theme().popover_foreground)
    })
    .max_w(px(560.))
    .build(window, cx)
}

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

/// Shared logical-pixel action profile. Custom size keeps button text at
/// the theme body size instead of the rem-relative Small text variant.
pub fn action_button(id: impl Into<ElementId>) -> Button {
    Button::new(id)
        .with_size(px(size::BUTTON_HEIGHT))
        .h(px(size::BUTTON_HEIGHT))
        .px(px(size::BUTTON_PADDING_X))
        .rounded(px(radius::CONTROL))
}

/// Kit computes its interaction colours after generic builder hover styles.
/// Use the native custom variant for controls on the dark navigation surface.
pub fn sidebar_variant(active: bool, cx: &App) -> ButtonCustomVariant {
    ButtonCustomVariant::new(cx)
        .color(
            rgb(if active {
                color::SIDEBAR_ACTIVE
            } else {
                color::SIDEBAR
            })
            .into(),
        )
        .foreground(rgb(color::SIDEBAR_TEXT).into())
        .hover(rgb(color::SIDEBAR_HOVER).into())
        .active(rgb(color::SIDEBAR_ACTIVE).into())
}

/// Add icons as content: Button::icon resizes its icon during rendering,
/// overriding even an explicitly sized Icon in gpui-component 0.7.3.
pub fn action_icon(icon: impl Into<Icon>) -> Icon {
    icon.into().with_size(px(16.))
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
    let button = action_button(id)
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
        .size(px(button_size))
        .p_0()
        .child(action_icon(icon))
        .tooltip(tooltip)
        .accessibility_label(accessibility_label)
        .disabled(disabled)
}

/// Studio V2's exported vectors at their native 20px size, inside Kit buttons.
/// Keep Kit focus, tooltips and disabled semantics for all seven system actions.
pub fn system_button(
    id: impl Into<ElementId>,
    asset: &'static str,
    tooltip: impl Into<SharedString>,
    accessibility_label: impl Into<SharedString>,
    disabled: bool,
) -> Button {
    system_button_base(id, asset, accessibility_label, disabled).tooltip(tooltip)
}

/// Same system control, for callers supplying a live native tooltip view.
pub fn system_button_base(
    id: impl Into<ElementId>,
    asset: &'static str,
    accessibility_label: impl Into<SharedString>,
    disabled: bool,
) -> Button {
    Button::new(id)
        // Kit's soft theme ring is nearly the canvas color in Studio. Use
        // the native keyboard-focus state with a solid semantic outline;
        // keep other controls' existing focus/selection palettes intact.
        .focus_ring(false)
        .focus_visible(|style| {
            style
                .border_1()
                .border_color(rgb(crate::theme::focus_color(crate::theme::current_skin())))
        })
        .with_size(px(size::SYSTEM_BUTTON))
        .w(px(size::SYSTEM_BUTTON))
        .h(px(size::SYSTEM_BUTTON))
        .p_0()
        .ghost()
        .rounded(px(radius::CONTROL))
        .child(
            Icon::default()
                .path(asset)
                .with_size(px(size::SYSTEM_ICON))
                .flex_shrink_0(),
        )
        .text_color(rgb(color::TEXT_3))
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
        .size(px(button_size))
        .p_0()
        .child(action_icon(icon))
        .tooltip(tooltip)
        .accessibility_label(accessibility_label)
        .disabled(disabled)
}

/// Text input at Studio V2's 40px field height. The caller's `InputState` is
/// passed through untouched.
pub fn text_input(state: &Entity<InputState>, disabled: bool) -> Input {
    // Input::h is an inherent multi-line-only API in Kit 0.7.3. Use Styled::h
    // so the single-line field's final style overrides its rem-based size.
    Styled::h(Input::new(state).large(), px(size::INPUT_HEIGHT))
        .text_size(px(font::BODY))
        .disabled(disabled)
}

/// Masked input (API keys) with the same field metrics and mask toggle.
pub fn masked_input(state: &Entity<InputState>, disabled: bool, toggle: Button) -> Input {
    text_input(state, disabled)
        .content_type(InputContentType::Password)
        .suffix(toggle)
}

/// Filter/search input with compact text and the shared 36px hit area.
pub fn compact_input(state: &Entity<InputState>, disabled: bool) -> Input {
    text_input(state, disabled)
}

/// The sidebar uses the same native input and editing engine, with a local
/// paint projection. Changing the global theme here would also recolor the
/// bright workspace and refresh every window on each input render.
#[derive(IntoElement)]
pub struct SidebarSearch {
    state: Entity<InputState>,
    label: SharedString,
    clear_label: SharedString,
}

pub fn sidebar_search(
    state: &Entity<InputState>,
    label: impl Into<SharedString>,
    clear_label: impl Into<SharedString>,
) -> SidebarSearch {
    SidebarSearch {
        state: state.clone(),
        label: label.into(),
        clear_label: clear_label.into(),
    }
}

impl RenderOnce for SidebarSearch {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let clear_state = self.state.clone();
        let has_text = !self.state.read(cx).value().is_empty();
        let clear_style = ButtonCustomVariant::new(cx)
            .color(rgb(color::SIDEBAR_HOVER).into())
            .foreground(rgb(color::SIDEBAR_TEXT).into())
            .hover(rgb(color::SIDEBAR_ACTIVE).into())
            .active(rgb(color::SIDEBAR_ACTIVE).into());
        let clear = Button::new("provider-search-clear")
            .custom(clear_style)
            .with_size(px(24.))
            .size(px(24.))
            .p_0()
            .accessibility_label(self.clear_label.clone())
            .tooltip(self.clear_label)
            .child(
                Icon::new(IconName::Close)
                    .with_size(px(12.))
                    .text_color(rgb(color::SIDEBAR_MUTED)),
            )
            .on_click(move |_, window, cx| {
                clear_state.update(cx, |state, cx| state.set_value("", window, cx));
                clear_state.read(cx).focus_handle(cx).focus(window, cx);
            });
        let input = RenderOnce::render(
            Styled::h(
                Input::new(&self.state)
                    .large()
                    .when(has_text, |input| input.suffix(clear))
                    .aria_label(self.label)
                    .prefix(
                        Icon::default()
                            .path("studio/provider-search.svg")
                            .with_size(px(17.))
                            .text_color(rgb(color::SIDEBAR_MUTED)),
                    )
                    .text_color(rgb(color::SIDEBAR_TEXT))
                    .bg(rgb(color::SIDEBAR_HOVER))
                    .border_color(rgb(color::SIDEBAR_HOVER))
                    .rounded(px(radius::CONTROL)),
                px(size::PROVIDER_ROW_HEIGHT),
            ),
            window,
            cx,
        )
        .into_any_element();
        // Kit installs its global editor palette while building the input.
        // Project only this single-line state's paint colours after that,
        // before the existing input entity performs layout and paint.
        self.state.update(cx, |state, _| {
            state.set_editor_style(gpui_kit::base::input::InputEditorStyle {
                foreground: rgb(color::SIDEBAR_TEXT).into(),
                muted_foreground: rgb(color::SIDEBAR_MUTED).into(),
                background: rgb(color::SIDEBAR_HOVER).into(),
                border: rgb(color::SIDEBAR_LINE).into(),
                selection: rgb(crate::theme::sidebar_selection()).into(),
                caret: rgb(color::SIDEBAR_TEXT).into(),
                ..Default::default()
            });
        });
        input
    }
}

/// Select wrapper; the state, items and keyboard behavior stay with the caller.
pub fn select<D>(state: &Entity<SelectState<D>>, disabled: bool) -> Select<D>
where
    D: SearchableListDelegate + 'static,
    <D::Item as SearchableListItem>::Value: PartialEq + Clone,
{
    Select::new(state)
        .large()
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
        .large()
        .text_size(px(font::BODY))
}

/// One button in a segmented control. The row helper below only supplies the
/// visual grouping; each button keeps its own callback.
pub fn segment_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    active: bool,
    disabled: bool,
) -> Button {
    action_button(id)
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

/// Table frame with the shared 32px header, 36px rows and both scrollbars visible.
pub fn table<D: TableDelegate>(state: &Entity<TableState<D>>) -> DataTable<D> {
    DataTable::new(state)
        .with_size(px(size::LIST_ROW_HEIGHT))
        .header_row_height(px(size::TABLE_HEADER_HEIGHT))
        .scrollbar_visible(true, true)
}

/// Request-log table: DESIGN_SPEC §4.1.5 fixes a 56px body row while the
/// header keeps the shared 32px height. Only the log uses this; the aggregate
/// tables stay on the shared 36px rows.
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
    action_button(id).ghost().label(label).disabled(disabled)
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
    tag.small()
        .text_size(px(font::TAG))
        .h(px(20.))
        .px(px(5.))
        .child(label.into().to_string())
}

/// Use the Kit's existing calendar state, keyboard handling and date selection,
/// with explicit typography for all days, month/year pickers and navigation.
/// Styling only the facade's outer element cannot override its tiny day labels.
pub fn calendar(
    state: &Entity<gpui_kit::component::calendar::CalendarState>,
    zh: bool,
) -> impl IntoElement {
    use gpui_kit::base::{Calendar, CalendarItemKind as Kind};
    Calendar::new(("calendar", state.entity_id()), state)
        .first_day_of_week(chrono::Weekday::Mon)
        .label(move |kind, value| {
            match kind {
                Kind::Previous => "‹".to_owned(),
                Kind::Next => "›".to_owned(),
                Kind::MonthToggle | Kind::Month if zh => format!("{value}月"),
                Kind::MonthToggle | Kind::Month => [
                    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov",
                    "Dec",
                ]
                .get((value - 1) as usize)
                .unwrap_or(&"")
                .to_string(),
                Kind::Weekday => if zh {
                    ["日", "一", "二", "三", "四", "五", "六"]
                } else {
                    ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"]
                }
                .get(value as usize)
                .unwrap_or(&"")
                .to_string(),
                _ => value.to_string(),
            }
            .into()
        })
        .item(|item, state, _, _| {
            let heading = matches!(state.kind(), Kind::MonthToggle | Kind::YearToggle);
            let weekday = state.kind() == Kind::Weekday;
            item.size(px(36.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(radius::CONTROL))
                .text_size(px(if weekday { font::LABEL } else { font::BODY }))
                .text_color(rgb(color::TEXT_1))
                .when(heading, |item| {
                    item.w_auto().px(px(8.)).font_weight(FontWeight::SEMIBOLD)
                })
                .when(matches!(state.kind(), Kind::Month | Kind::Year), |item| {
                    item.w_full()
                })
                .when(weekday || state.is_muted(), |item| {
                    item.text_color(rgb(color::TEXT_3))
                })
                .when(state.is_disabled(), |item| item.opacity(0.5))
                .when(state.is_in_range() || state.is_today(), |item| {
                    item.bg(rgb(crate::theme::palette().tint))
                })
                .when(
                    !weekday && !state.is_disabled() && !state.is_active(),
                    |item| item.hover(|style| style.bg(rgb(crate::theme::palette().tint))),
                )
                .when(state.is_active(), |item| {
                    item.bg(rgb(crate::theme::palette().primary))
                        .text_color(rgb(color::WHITE))
                })
                .into_any_element()
        })
        .w(px(280.))
        .flex_shrink_0()
        .p(px(12.))
        .border_1()
        .border_color(rgb(color::BORDER))
        .rounded(px(radius::CARD))
}

/// A local paint projection over the existing read-only log editor. It keeps
/// selection, clipboard and scroll state in the original native entity.
#[derive(IntoElement)]
pub struct TerminalLogs {
    state: Entity<TextareaState>,
    mono: SharedString,
    label: SharedString,
}

pub fn terminal_logs(
    state: &Entity<TextareaState>,
    mono: SharedString,
    label: impl Into<SharedString>,
) -> TerminalLogs {
    TerminalLogs {
        state: state.clone(),
        mono,
        label: label.into(),
    }
}

impl RenderOnce for TerminalLogs {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let state = self.state.clone();
        div()
            .size_full()
            .child(
                Textarea::new(&self.state)
                    .readonly(true)
                    .bordered(false)
                    .aria_label(self.label)
                    .font_family(self.mono)
                    .text_size(px(12.))
                    .line_height(px(20.))
                    .bg(rgb(color::TERMINAL))
                    .text_color(rgb(color::TERMINAL_TEXT))
                    .h_full(),
            )
            // Textarea returns a deferred Kit Input, which installs the global
            // palette in its own render. Project after that child's prepaint,
            // before the editor paints; assigning during Textarea::render is
            // too early. This does not notify or schedule another frame.
            .on_prepaint(move |_, _, cx| {
                state.update(cx, |state, _| {
                    state.set_editor_style(gpui_kit::base::input::InputEditorStyle {
                        foreground: rgb(color::TERMINAL_TEXT).into(),
                        muted_foreground: rgb(color::SIDEBAR_MUTED).into(),
                        background: rgb(color::TERMINAL).into(),
                        border: rgb(color::TERMINAL_LINE).into(),
                        // A translucent accent keeps the light log glyphs
                        // readable across all seven palettes.
                        selection: Hsla::from(rgb(crate::theme::sidebar_selection())).opacity(0.35),
                        caret: rgb(color::SIDEBAR_TEXT).into(),
                        ..Default::default()
                    });
                });
            })
    }
}
