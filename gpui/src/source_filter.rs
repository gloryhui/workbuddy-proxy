//! The source filter: exactly one of "all models", "built-in models" or
//! "custom models".
//!
//! It is its own component rather than three buttons glued into the filter bar,
//! because the group has rules the rest of the bar does not: it keeps its options together,
//! it never wraps, it never splits, and it can never end up with nothing
//! selected.
use crate::stats_dto::SourceScope;
use gpui_kit::{Role, SharedString};

/// Chinese labelled group, including its 4px track inset and two inter-option gaps.
pub const GROUP_WIDTH: i32 = 391;
pub const GROUP_HEIGHT: i32 = 41;
pub const ITEM_HEIGHT: i32 = 33;
pub const ITEM_WIDTHS: [i32; 3] = [120, 122, 129];
pub const ITEM_WIDTHS_EN: [i32; 3] = [120, 130, 121];
pub const GROUP_INSET: i32 = 4;
pub const GROUP_GAP: i32 = 6;
pub const ICON_SIZE: i32 = 16;

/// One selectable source, in the order the spec lists them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceOption {
    /// The scope this option selects.
    pub scope: SourceScope,
    /// The asset key the icon is loaded from.
    pub asset: &'static str,
    /// White counterpart used on the selected green button.
    pub selected_asset: &'static str,
    /// The full label, used for the tooltip and for accessibility.
    pub label: &'static str,
}

/// Every option, in order: all, built-in, supplier.
pub const OPTIONS: &[SourceOption] = &[
    SourceOption {
        scope: SourceScope::All,
        asset: "design/source-all.png",
        selected_asset: "design/source-all-selected.png",
        label: "全部模型 / All models",
    },
    SourceOption {
        scope: SourceScope::Builtin,
        asset: "design/source-builtin.png",
        selected_asset: "design/source-builtin-selected.png",
        label: "内置模型 / Built-in models",
    },
    SourceOption {
        scope: SourceScope::Supplier,
        asset: "design/source-custom.png",
        selected_asset: "design/source-custom-selected.png",
        label: "自定义模型 / Custom models",
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
                .unwrap_or("全部模型 / All models"),
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
    /// The source group. Kept together at the end of the primary row.
    Source,
    Provider,
    Model,
    Refresh,
    DateRange,
    RefreshButton,
}

impl FilterSlot {
    /// The width the spec reserves for this control.
    pub fn width(self) -> i32 {
        match self {
            FilterSlot::Source => GROUP_WIDTH,
            FilterSlot::Provider => 174,
            FilterSlot::Model => 156,
            FilterSlot::Refresh => 178,
            FilterSlot::DateRange => 108,
            FilterSlot::RefreshButton => 42,
        }
    }
}

/// The gap between two controls.
pub const GAP: i32 = 14;
/// Every slot, in order.
pub const SLOTS: &[FilterSlot] = &[
    FilterSlot::Model,
    FilterSlot::Provider,
    FilterSlot::DateRange,
    FilterSlot::Refresh,
    FilterSlot::RefreshButton,
    FilterSlot::Source,
];

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
    fn the_reserved_widths_match_the_layout_spec() {
        assert_eq!(FilterSlot::Source.width(), GROUP_WIDTH);
        assert_eq!(FilterSlot::Provider.width(), 174);
        assert_eq!(FilterSlot::Model.width(), 156);
        assert_eq!(FilterSlot::Refresh.width(), 178);
        assert_eq!(FilterSlot::DateRange.width(), 108);
        assert_eq!(FilterSlot::RefreshButton.width(), 42);
        assert_eq!(GAP, 14);
        assert_eq!(SLOTS.len(), 6);
    }

    #[test]
    fn the_group_geometry_matches_the_layout_spec() {
        assert_eq!(
            ITEM_WIDTHS.iter().sum::<i32>() + 2 * GROUP_INSET + 2 * GROUP_GAP,
            GROUP_WIDTH
        );
        assert_eq!(GROUP_HEIGHT, ITEM_HEIGHT + 2 * GROUP_INSET);
        assert_eq!(ICON_SIZE, 16);
        for option in OPTIONS {
            assert!(option.asset.starts_with("design/source-"));
            assert!(option.asset.ends_with(".png"));
            assert!(option.selected_asset.ends_with("-selected.png"));
        }
    }
}
