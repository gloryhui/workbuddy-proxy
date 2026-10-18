//! The source filter: exactly one of "all sources", "built-in models" or
//! "supplier models".
//!
//! It is its own component rather than three buttons glued into the filter bar,
//! because the group has rules the rest of the bar does not: it is always first,
//! it never wraps, it never splits, and it can never end up with nothing
//! selected.
use crate::stats_dto::SourceScope;
use gpui_kit::{Role, SharedString};

/// The width the group occupies, fixed by the layout spec.
pub const GROUP_WIDTH: i32 = 108;
/// The height of the group and of every button in it.
pub const GROUP_HEIGHT: i32 = 32;
/// The width of one button: three of them fill the group exactly.
pub const ITEM_WIDTH: i32 = 36;
/// The icon inside a button.
pub const ICON_SIZE: i32 = 16;

/// One selectable source, in the order the spec lists them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceOption {
    /// The scope this option selects.
    pub scope: SourceScope,
    /// The asset key the icon is loaded from.
    pub asset: &'static str,
    /// Unselected icon colour from UI-SPEC U03.
    pub color: u32,
    /// The full label, used for the tooltip and for accessibility.
    pub label: &'static str,
    /// The shorter label a narrow layout may show.
    pub short_label: &'static str,
}

/// Every option, in order: all, built-in, supplier.
pub const OPTIONS: &[SourceOption] = &[
    SourceOption {
        scope: SourceScope::All,
        asset: "icons/stats-source-all.svg",
        color: 0x356fc0,
        label: "全部来源 / All sources",
        short_label: "全部",
    },
    SourceOption {
        scope: SourceScope::Builtin,
        asset: "icons/stats-source-builtin.svg",
        color: 0x268563,
        label: "内置模型 / Built-in models",
        short_label: "内置",
    },
    SourceOption {
        scope: SourceScope::Supplier,
        asset: "icons/stats-source-supplier.svg",
        color: 0xb64270,
        label: "自定义模型 / Custom models",
        short_label: "自定义",
    },
];

/// What a click or a key did to the selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectionOutcome {
    /// The selection changed; the caller should request the new scope.
    Changed(SourceScope),
    /// The option was already selected: nothing happens and **no request is
    /// made**, which is what makes clicking the current option free.
    Unchanged,
}

/// The three mutually exclusive source choices.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceFilter {
    selected: SourceScope,
    /// True while the group holds keyboard focus.
    focused: bool,
    /// The option the pointer is over, if any.
    hovered: Option<SourceScope>,
    /// True when the whole group is disabled (a page with no data yet).
    disabled: bool,
}

impl Default for SourceFilter {
    fn default() -> Self {
        Self::new()
    }
}

impl SourceFilter {
    /// A filter with "all sources" selected, which is the default.
    pub fn new() -> Self {
        Self {
            selected: SourceScope::All,
            focused: false,
            hovered: None,
            disabled: false,
        }
    }

    /// The scope currently selected.
    pub fn selected(&self) -> SourceScope {
        self.selected
    }

    /// True while the group has keyboard focus.
    pub fn is_focused(&self) -> bool {
        self.focused
    }

    /// The option under the pointer, if any.
    pub fn hovered(&self) -> Option<SourceScope> {
        self.hovered
    }

    /// True when the group is disabled.
    pub fn is_disabled(&self) -> bool {
        self.disabled
    }

    /// Enables or disables the whole group. A disabled group never changes its
    /// selection.
    pub fn set_disabled(&mut self, disabled: bool) {
        self.disabled = disabled;
    }

    /// Moves the keyboard focus into or out of the group.
    pub fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
        if !focused {
            self.hovered = None;
        }
    }

    /// Records which option the pointer is over.
    pub fn set_hovered(&mut self, scope: Option<SourceScope>) {
        self.hovered = scope;
    }

    /// Selects a scope.
    ///
    /// The three options are mutually exclusive by construction — there is one
    /// field, not a set — and the current option can never be cleared, so the
    /// group always has exactly one selection.
    pub fn select(&mut self, scope: SourceScope) -> SelectionOutcome {
        if self.disabled || self.selected == scope {
            return SelectionOutcome::Unchanged;
        }
        self.selected = scope;
        SelectionOutcome::Changed(scope)
    }

    /// Moves the selection by keyboard: `-1` left, `+1` right, wrapping.
    ///
    /// Wrapping keeps the group usable from the keyboard alone: three presses in
    /// either direction always come back to where they started.
    pub fn move_selection(&mut self, step: i32) -> SelectionOutcome {
        if self.disabled {
            return SelectionOutcome::Unchanged;
        }
        let count = OPTIONS.len() as i32;
        let current = OPTIONS
            .iter()
            .position(|option| option.scope == self.selected)
            .unwrap_or(0) as i32;
        let next = (current + step).rem_euclid(count);
        self.select(OPTIONS[next as usize].scope)
    }

    /// The accessibility label of the whole group.
    pub fn group_label(&self) -> SharedString {
        SharedString::from("来源 / Source")
    }

    /// The accessibility role of the group: a set of radio buttons.
    pub fn group_role(&self) -> Role {
        Role::RadioGroup
    }

    /// The accessibility label of one option.
    ///
    /// The **full** label is used even when the visible text is abbreviated, so
    /// a screen reader never receives the short form.
    pub fn option_label(scope: SourceScope) -> SharedString {
        SharedString::from(
            OPTIONS
                .iter()
                .find(|option| option.scope == scope)
                .map(|option| option.label)
                .unwrap_or("全部来源 / All sources"),
        )
    }

    /// The accessibility role of one option.
    pub fn option_role() -> Role {
        Role::RadioButton
    }

    /// True when the option reads as selected to assistive technology. This is
    /// state, not only colour.
    pub fn is_selected(&self, scope: SourceScope) -> bool {
        self.selected == scope
    }
}

/// One control in the filter bar, in the order the spec fixes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilterSlot {
    /// The source group. Always first, never split.
    Source,
    Provider,
    Model,
    Status,
    Refresh,
    DateRange,
    RefreshButton,
    More,
}

impl FilterSlot {
    /// The width the spec reserves for this control.
    pub fn width(self) -> i32 {
        match self {
            FilterSlot::Source => GROUP_WIDTH,
            FilterSlot::Provider => 136,
            FilterSlot::Model => 180,
            FilterSlot::Status => 112,
            FilterSlot::Refresh => 88,
            FilterSlot::DateRange => 168,
            FilterSlot::RefreshButton | FilterSlot::More => 32,
        }
    }
}

/// The gap between two controls.
pub const GAP: i32 = 8;
/// The padding inside the filter card.
pub const PADDING: i32 = 12;
/// The width at which everything fits on one line, from the spec.
pub const SINGLE_ROW_WIDTH: i32 = 1012;
/// Below this the bar still keeps a whole source group but wraps the rest.
pub const NARROW_WIDTH: i32 = 560;

/// Every slot, in order.
pub const SLOTS: &[FilterSlot] = &[
    FilterSlot::Source,
    FilterSlot::Provider,
    FilterSlot::Model,
    FilterSlot::Status,
    FilterSlot::Refresh,
    FilterSlot::DateRange,
    FilterSlot::RefreshButton,
    FilterSlot::More,
];

/// How the filter bar is laid out for a given available width.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FilterLayout {
    /// The slots of each row, in order.
    pub rows: Vec<Vec<FilterSlot>>,
}

impl FilterLayout {
    /// True when everything fits on one line.
    #[cfg(test)]
    pub fn is_single_row(&self) -> bool {
        self.rows.len() == 1
    }
}

/// Lays the filter bar out for the width the card actually has.
///
/// The width is the **card's** available width, not the window's, and the answer
/// is computed rather than guessed: nothing here subtracts a magic 108 from some
/// other number, which is what used to make the English labels overflow.
pub fn layout(available: i32) -> FilterLayout {
    if available >= SINGLE_ROW_WIDTH {
        return FilterLayout {
            rows: vec![SLOTS.to_vec()],
        };
    }
    if available >= NARROW_WIDTH {
        // Two rows, split exactly where the spec says: the identity controls
        // first, the timing controls second.
        return FilterLayout {
            rows: vec![
                vec![
                    FilterSlot::Source,
                    FilterSlot::Provider,
                    FilterSlot::Model,
                    FilterSlot::Status,
                ],
                vec![
                    FilterSlot::Refresh,
                    FilterSlot::DateRange,
                    FilterSlot::RefreshButton,
                    FilterSlot::More,
                ],
            ],
        };
    }
    // Very narrow: the source group stays whole on the first row and the rest
    // wraps in order, one control per row when it has to.
    let mut rows: Vec<Vec<FilterSlot>> = vec![vec![FilterSlot::Source]];
    let mut current: Vec<FilterSlot> = Vec::new();
    let mut used = GROUP_WIDTH + GAP;
    for slot in SLOTS.iter().copied().filter(|s| *s != FilterSlot::Source) {
        let needed = slot.width() + GAP;
        if used + needed > available && !current.is_empty() {
            rows.push(std::mem::take(&mut current));
            used = 0;
        }
        current.push(slot);
        used += needed;
    }
    if !current.is_empty() {
        rows.push(current);
    }
    FilterLayout { rows }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_is_all_sources_and_exactly_one_is_always_selected() {
        let filter = SourceFilter::new();
        assert_eq!(filter.selected(), SourceScope::All);
        let selected = OPTIONS
            .iter()
            .filter(|option| filter.is_selected(option.scope))
            .count();
        assert_eq!(selected, 1, "exactly one option is selected");
    }

    #[test]
    fn the_three_options_are_mutually_exclusive() {
        // Selecting one clears the others: there is one field, not a set.
        let mut filter = SourceFilter::new();
        assert_eq!(
            filter.select(SourceScope::Builtin),
            SelectionOutcome::Changed(SourceScope::Builtin)
        );
        assert_eq!(filter.selected(), SourceScope::Builtin);
        assert!(!filter.is_selected(SourceScope::All));
        assert!(!filter.is_selected(SourceScope::Supplier));

        assert_eq!(
            filter.select(SourceScope::Supplier),
            SelectionOutcome::Changed(SourceScope::Supplier)
        );
        assert!(!filter.is_selected(SourceScope::Builtin));
    }

    #[test]
    fn clicking_the_current_option_is_free() {
        // The spec is explicit: clicking the selected option makes no request.
        let mut filter = SourceFilter::new();
        assert_eq!(filter.select(SourceScope::All), SelectionOutcome::Unchanged);
        filter.select(SourceScope::Supplier);
        assert_eq!(
            filter.select(SourceScope::Supplier),
            SelectionOutcome::Unchanged
        );
        assert_eq!(filter.selected(), SourceScope::Supplier);
    }

    #[test]
    fn the_selection_can_never_be_cleared() {
        // There is no "select nothing" path, so the group always has one answer.
        let mut filter = SourceFilter::new();
        for _ in 0..5 {
            filter.move_selection(1);
            assert!(
                OPTIONS
                    .iter()
                    .any(|option| filter.is_selected(option.scope))
            );
        }
        filter.move_selection(-1);
        assert!(
            OPTIONS
                .iter()
                .any(|option| filter.is_selected(option.scope))
        );
    }

    #[test]
    fn the_keyboard_wraps_in_both_directions() {
        let mut filter = SourceFilter::new();
        assert_eq!(
            filter.move_selection(1),
            SelectionOutcome::Changed(SourceScope::Builtin)
        );
        assert_eq!(
            filter.move_selection(1),
            SelectionOutcome::Changed(SourceScope::Supplier)
        );
        assert_eq!(
            filter.move_selection(1),
            SelectionOutcome::Changed(SourceScope::All),
            "three steps to the right come back"
        );
        assert_eq!(
            filter.move_selection(-1),
            SelectionOutcome::Changed(SourceScope::Supplier),
            "and one step left wraps the other way"
        );
    }

    #[test]
    fn a_disabled_group_never_changes_its_selection() {
        let mut filter = SourceFilter::new();
        filter.set_disabled(true);
        assert_eq!(
            filter.select(SourceScope::Builtin),
            SelectionOutcome::Unchanged
        );
        assert_eq!(filter.move_selection(1), SelectionOutcome::Unchanged);
        assert_eq!(filter.selected(), SourceScope::All, "still one answer");
    }

    #[test]
    fn hover_and_focus_are_tracked_for_every_option() {
        // Hover and focus are states the component owns, so the view can style
        // them and a tooltip can be shown for both.
        let mut filter = SourceFilter::new();
        assert!(!filter.is_focused());
        assert_eq!(filter.hovered(), None);
        filter.set_focused(true);
        assert!(filter.is_focused());
        filter.set_hovered(Some(SourceScope::Supplier));
        assert_eq!(filter.hovered(), Some(SourceScope::Supplier));
        filter.set_focused(false);
        assert!(!filter.is_focused());
        assert_eq!(
            filter.hovered(),
            None,
            "leaving focus clears the hover state"
        );
    }

    #[test]
    fn every_option_exposes_a_role_a_full_label_and_a_selected_state() {
        // Accessibility is state, not colour: each option has a radio role, the
        // full (never abbreviated) label, and an explicit selected flag.
        let mut filter = SourceFilter::new();
        filter.select(SourceScope::Builtin);
        assert_eq!(filter.group_role(), Role::RadioGroup);
        assert_eq!(SourceFilter::option_role(), Role::RadioButton);
        for option in OPTIONS {
            let label = SourceFilter::option_label(option.scope);
            assert!(
                label.contains('/'),
                "the label carries both languages: {label}"
            );
            assert_eq!(
                filter.is_selected(option.scope),
                option.scope == SourceScope::Builtin
            );
        }
    }

    #[test]
    fn the_bar_is_one_row_at_1012_and_two_rows_at_1011() {
        // AC066: the boundary itself fits; one pixel less wraps.
        assert!(layout(1012).is_single_row());
        assert_eq!(layout(1012).rows.len(), 1);
        assert_eq!(layout(1011).rows.len(), 2);
        assert!(!layout(1011).is_single_row());
    }

    #[test]
    fn the_two_row_split_is_exactly_the_spec_order() {
        let budget = layout(1011);
        assert_eq!(
            budget.rows[0],
            vec![
                FilterSlot::Source,
                FilterSlot::Provider,
                FilterSlot::Model,
                FilterSlot::Status
            ],
            "identity controls first"
        );
        assert_eq!(
            budget.rows[1],
            vec![
                FilterSlot::Refresh,
                FilterSlot::DateRange,
                FilterSlot::RefreshButton,
                FilterSlot::More
            ],
            "timing and actions second"
        );
        // And the same split holds right down to the narrow boundary.
        assert_eq!(layout(560).rows, budget.rows);
    }

    #[test]
    fn the_source_group_is_always_first_and_never_split() {
        // AC066: whatever the width, the group leads and stays in one piece.
        for available in [0, 100, 559, 560, 800, 1011, 1012, 2000] {
            let budget = layout(available);
            assert_eq!(
                budget.rows[0].first(),
                Some(&FilterSlot::Source),
                "at {available} the source group must lead"
            );
            let rows_with_source = budget
                .rows
                .iter()
                .filter(|row| row.contains(&FilterSlot::Source))
                .count();
            assert_eq!(
                rows_with_source, 1,
                "at {available} the group must not split"
            );
        }
    }

    #[test]
    fn below_560_the_source_group_still_stays_whole_and_the_rest_wraps() {
        let budget = layout(559);
        assert_eq!(budget.rows[0], vec![FilterSlot::Source]);
        assert!(budget.rows.len() > 2, "the remaining controls wrap further");
        // Every control still appears, in order, exactly once.
        let mut seen: Vec<FilterSlot> = budget.rows.iter().flatten().copied().collect();
        assert_eq!(seen.len(), SLOTS.len());
        seen.sort_by_key(|slot| SLOTS.iter().position(|s| s == slot).unwrap());
        assert_eq!(seen, SLOTS.to_vec());
    }

    #[test]
    fn every_slot_appears_exactly_once_at_every_width() {
        for available in [0, 320, 559, 560, 1011, 1012, 4000] {
            let budget = layout(available);
            let mut seen: Vec<FilterSlot> = budget.rows.iter().flatten().copied().collect();
            assert_eq!(seen.len(), SLOTS.len(), "at {available} nothing is lost");
            seen.sort_by_key(|slot| SLOTS.iter().position(|s| s == slot).unwrap());
            assert_eq!(seen, SLOTS.to_vec(), "at {available} the order is kept");
        }
    }

    #[test]
    fn the_reserved_widths_match_the_layout_spec() {
        assert_eq!(FilterSlot::Source.width(), 108);
        assert_eq!(FilterSlot::Provider.width(), 136);
        assert_eq!(FilterSlot::Model.width(), 180);
        assert_eq!(FilterSlot::Status.width(), 112);
        assert_eq!(FilterSlot::Refresh.width(), 88);
        assert_eq!(FilterSlot::DateRange.width(), 168);
        assert_eq!(FilterSlot::RefreshButton.width(), 32);
        assert_eq!(FilterSlot::More.width(), 32);
        assert_eq!(GAP, 8);
        assert_eq!(PADDING, 12);
        assert_eq!(SLOTS.len(), 8);
    }

    #[test]
    fn the_group_geometry_matches_the_layout_spec() {
        assert_eq!(ITEM_WIDTH * OPTIONS.len() as i32, GROUP_WIDTH);
        assert_eq!(GROUP_HEIGHT, 32);
        assert_eq!(ICON_SIZE, 16);
        for option in OPTIONS {
            assert!(option.asset.starts_with("icons/stats-source-"));
            assert!(option.asset.ends_with(".svg"));
        }
    }
}
