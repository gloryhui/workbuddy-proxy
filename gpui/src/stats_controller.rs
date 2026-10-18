//! Page state shared by the real statistics view.
//!
//! This module deliberately holds no GPUI types: the view maps its inputs and
//! selects onto these values, so the state machine can be tested without a
//! window. Everything here is driven by `statistics.rs`; it is not a parallel
//! implementation kept only for tests.

use crate::stats_dto::{Currency, GroupBy, InputMode, PriceRuleDto, SortMetric, SourceScope};
use serde_json::{Value, json};

/// Reject invalid drafts without modifying the last range applied to reports.
pub fn validate_range(from: Option<i64>, to: Option<i64>) -> Result<(i64, i64), &'static str> {
    let (Some(from), Some(to)) = (from, to) else {
        return Err("invalid_date");
    };
    if from >= to {
        return Err("reversed_date");
    }
    if to
        .checked_sub(from)
        .is_none_or(|span| span > 366 * 86_400_000)
    {
        return Err("range_too_long");
    }
    Ok((from, to))
}

pub fn may_refresh(visible: bool, active: bool, modal: bool, valid_range: bool) -> bool {
    visible && active && !modal && valid_range
}

/// Which listing a read belongs to. The overview is a single resource and is
/// tracked separately, so only the two listings need an identity here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatsTab {
    Rows,
    Groups,
}

/// Identity of one statistics read.
///
/// A reply is only allowed to touch page state while its identity is still the
/// current one. Changing a filter invalidates the old identities *immediately*,
/// not when the debounce finally issues the next request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueryIdentity {
    pub filter_generation: u64,
    pub tab: StatsTab,
    pub group_by: GroupBy,
    pub sort: SortMetric,
    pub cursor: Option<String>,
    pub seq: u64,
}

impl QueryIdentity {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        filter_generation: u64,
        tab: StatsTab,
        group_by: GroupBy,
        sort: SortMetric,
        cursor: Option<String>,
        seq: u64,
    ) -> Self {
        Self {
            filter_generation,
            tab,
            group_by,
            sort,
            cursor,
            seq,
        }
    }
}

/// Latest-issued read per resource.
///
/// Every request records the identity it was issued with; a reply is accepted
/// only when it still matches. Stale replies cannot clear a newer pending flag
/// or overwrite newer data.
#[derive(Default)]
pub struct ReplyGate {
    identity: Option<QueryIdentity>,
    seq: u64,
    /// Shared with the import panel: bounds how often a running scan is polled.
    scan_poll: ScanPollBudget,
}

/// Bounds the polling a running scan may cause.
///
/// A scan is asynchronous, so a page has to ask again to see it finish. The
/// budget makes that finite: the read gate spends one poll per issued read, and
/// once the budget is gone the page keeps showing the last known state instead
/// of turning a slow scan into an endless stream of requests.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScanPollBudget {
    remaining: u32,
}

impl ScanPollBudget {
    /// The most polls one scan may cause.
    pub const MAX_POLLS: u32 = 60;

    /// A fresh budget.
    pub fn new() -> Self {
        Self {
            remaining: Self::MAX_POLLS,
        }
    }

    /// True when another poll may be issued; consumes one.
    pub fn next_poll(&mut self) -> bool {
        if self.is_exhausted() {
            return false;
        }
        self.remaining -= 1;
        true
    }

    /// True when the budget is spent.
    pub fn is_exhausted(&self) -> bool {
        self.remaining == 0
    }
}

impl Default for ScanPollBudget {
    fn default() -> Self {
        Self::new()
    }
}

/// The capture-method choices in the advanced filter (U05).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CaptureChoice {
    /// Every capture method.
    #[default]
    All,
    /// Requests this proxy relayed.
    Proxy,
    /// Records read out of a WorkBuddy log.
    WorkbuddyImport,
}

impl CaptureChoice {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Proxy => "proxy",
            Self::WorkbuddyImport => "workbuddy",
        }
    }
}

/// The advanced capture filter, kept consistent with the source scope.
///
/// The two controls constrain each other: under the built-in source the capture
/// method can only be "WorkBuddy import", so the dropdown is shown that way and
/// disabled, and a proxy value that the source hides is **cleared** rather than
/// kept in secret. Switching back does not restore it — the hidden condition is
/// gone for good, which is what the spec asks for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CaptureFilter {
    choice: CaptureChoice,
    include_tests: bool,
}

impl CaptureFilter {
    /// True when test connections may be included at all.
    ///
    /// They are only ever meaningful for the proxy's own traffic, so the switch
    /// is unavailable under the built-in source and under the import capture.
    pub fn include_tests_available(&self, scope: SourceScope) -> bool {
        scope != SourceScope::Builtin && self.displayed(scope) != CaptureChoice::WorkbuddyImport
    }

    /// Applies a new source scope, clearing whatever that scope hides.
    ///
    /// Returns true when something changed, which means the page must
    /// re-request. The hidden value is **dropped**, not remembered: switching
    /// back does not restore it.
    pub fn reconcile(&mut self, scope: SourceScope) -> bool {
        let mut changed = false;
        if scope == SourceScope::Builtin && self.choice == CaptureChoice::Proxy {
            self.choice = CaptureChoice::All;
            changed = true;
        }
        if !self.include_tests_available(scope) && self.include_tests {
            self.include_tests = false;
            changed = true;
        }
        changed
    }

    /// The choice sent to the core: a locked dropdown always sends `all` and
    /// lets the source scope do the restricting.
    pub fn effective(&self, scope: SourceScope) -> CaptureChoice {
        if self.is_locked(scope) {
            CaptureChoice::All
        } else {
            self.choice
        }
    }
}

/// The source-confirmation flow for one model.
///
/// The dialog lives here rather than in the view because a filter change
/// invalidates it: a preview describes an interval and a rule revision, and both
/// stop being meaningful the moment the page moves on.
#[derive(Clone, Debug, Default)]
pub struct MappingFlow {
    dialog: Option<crate::workbuddy_import::MappingDialog>,
}

impl MappingFlow {
    /// True while a dialog is open.
    pub fn is_open(&self) -> bool {
        self.dialog.is_some()
    }

    /// The open dialog, if any.
    pub fn dialog(&self) -> Option<&crate::workbuddy_import::MappingDialog> {
        self.dialog.as_ref()
    }

    /// Opens the dialog for a model.
    pub fn open(&mut self, model_id: &str) -> &mut crate::workbuddy_import::MappingDialog {
        self.dialog = Some(crate::workbuddy_import::MappingDialog::new(model_id));
        self.dialog.as_mut().expect("just set")
    }

    /// Closes the dialog. Any preview it held goes with it.
    pub fn close(&mut self) {
        if self.is_open() {
            self.dialog = None;
        }
    }
}

/// What a filter change implies for the page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilterChange {
    /// Outstanding reads were invalidated and pagination was reset; the page
    /// may now issue exactly one request. A click on the option that was already
    /// selected produces **no** change at all, so no request follows.
    Invalidated,
}

/// Applies filter changes in the only order that keeps a page consistent.
///
/// A change first invalidates every outstanding reply, then resets pagination,
/// and only then allows **one** new request. Doing it in any other order is how
/// a slow reply for the previous filters ends up painted over the new ones, or
/// how a single click produces a burst of requests.
#[derive(Clone, Debug, Default)]
pub struct FilterCoordinator {
    generation: u64,
    request_pending: bool,
    /// The source-confirmation flow, closed whenever the page moves on.
    mapping: MappingFlow,
}

impl FilterCoordinator {
    /// A coordinator that has not seen a change yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// The current filter generation.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Records a change and says what the caller must do.
    pub fn change(&mut self) -> FilterChange {
        self.generation += 1;
        self.request_pending = true;
        // A preview belongs to the page it was taken on, so moving on closes it.
        self.mapping.close();
        FilterChange::Invalidated
    }

    /// Compatibility model used by startup checks.
    pub fn mapping(&mut self) -> &mut MappingFlow {
        &mut self.mapping
    }

    /// Takes the one request owed for the last change.
    pub fn take_request(&mut self) -> bool {
        std::mem::take(&mut self.request_pending)
    }
}

impl CaptureFilter {
    /// A filter with every capture method and no test rows.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the choice without going through the UI rules.
    ///
    /// Used by the startup checks and tests to reach a state the user could
    /// also reach; the interactive path goes through `reconcile`.
    #[cfg(test)]
    pub fn set_choice_for_test(&mut self, choice: CaptureChoice) {
        self.choice = choice;
    }

    pub fn select(&mut self, choice: CaptureChoice, scope: SourceScope) -> bool {
        if self.is_locked(scope) || self.choice == choice {
            return false;
        }
        self.choice = choice;
        self.reconcile(scope);
        true
    }

    pub fn set_include_tests(&mut self, enabled: bool, scope: SourceScope) {
        self.include_tests = enabled && self.include_tests_available(scope);
    }

    /// True when test connections are included.
    pub fn include_tests(&self) -> bool {
        self.include_tests
    }

    /// True when the dropdown must be shown disabled.
    pub fn is_locked(&self, scope: SourceScope) -> bool {
        scope == SourceScope::Builtin
    }

    /// The choice actually shown: under the built-in source the only sensible
    /// answer is the import itself.
    pub fn displayed(&self, scope: SourceScope) -> CaptureChoice {
        if self.is_locked(scope) {
            CaptureChoice::WorkbuddyImport
        } else {
            self.choice
        }
    }
}

impl ReplyGate {
    /// Issues the next identity for a read, invalidating every older one.
    pub fn issue(
        &mut self,
        filter_generation: u64,
        tab: StatsTab,
        group_by: GroupBy,
        sort: SortMetric,
        cursor: Option<String>,
    ) -> QueryIdentity {
        self.seq += 1;
        // Every issued read spends one poll from the scan budget, so a page that
        // keeps asking about a running scan runs out instead of polling forever.
        self.scan_poll.next_poll();
        let identity = QueryIdentity::new(filter_generation, tab, group_by, sort, cursor, self.seq);
        self.identity = Some(identity.clone());
        identity
    }

    /// Invalidates outstanding reads without issuing a new one: used the moment
    /// a filter changes, so a reply that is already in flight cannot land.
    pub fn invalidate(&mut self) {
        self.seq += 1;
        self.identity = None;
    }

    #[cfg(test)]
    pub fn current(&self) -> Option<&QueryIdentity> {
        self.identity.as_ref()
    }

    /// Replies carry only their sequence number, so the page checks it here.
    pub fn may_apply_seq(&self, seq: u64) -> bool {
        self.identity
            .as_ref()
            .is_some_and(|current| current.seq == seq)
    }

    /// Adopts the sequence number the scheduler assigned to the read that was
    /// just queued, keeping one identity per outstanding reply.
    pub fn replace_seq(&mut self, tab: StatsTab, seq: u64) {
        if let Some(current) = self.identity.as_mut() {
            current.tab = tab;
            current.seq = seq;
        } else {
            self.seq = seq;
        }
    }
}

/// The listing a reply belongs to, as the scheduler reports it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReplyKind {
    Dashboard,
    Rows,
    Groups,
    /// Resources with independent reply identities, including price pages.
    Other,
}

/// What the page should do with a reply it just received.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReplyRoute {
    /// The reply describes what is on screen; apply it.
    Apply,
    /// The page has moved on. Apply nothing: not the data, not the error, and
    /// not the pending flag of the read that replaced it.
    Ignore,
}

/// Decides whether a reply may touch the page.
///
/// A reply must match the identity of its own listing *and* still belong to the
/// tab that is on screen: a rows reply that passes its gate while the user has
/// switched to providers must not be decoded as a grouping.
pub fn route_reply(
    kind: ReplyKind,
    seq: u64,
    dashboard_seq: u64,
    rows: &ReplyGate,
    groups: &ReplyGate,
    tab: StatsTab,
) -> ReplyRoute {
    match kind {
        ReplyKind::Dashboard => {
            if seq == dashboard_seq && seq != 0 {
                ReplyRoute::Apply
            } else {
                ReplyRoute::Ignore
            }
        }
        ReplyKind::Rows => {
            if rows.may_apply_seq(seq) && tab == StatsTab::Rows {
                ReplyRoute::Apply
            } else {
                ReplyRoute::Ignore
            }
        }
        ReplyKind::Groups => {
            if groups.may_apply_seq(seq) && tab == StatsTab::Groups {
                ReplyRoute::Apply
            } else {
                ReplyRoute::Ignore
            }
        }
        ReplyKind::Other => ReplyRoute::Apply,
    }
}

/// A price being created or edited.
///
/// It never borrows the report filters: editing a price must not change which
/// provider or model the page is reporting on, and cancelling must leave the
/// page exactly as it was.
#[derive(Clone, Debug, PartialEq)]
pub struct PriceDraft {
    /// `None` while creating.
    pub price_id: Option<String>,
    pub expected_revision: i64,
    pub provider_id: String,
    pub upstream_model: String,
    pub currency: Currency,
    pub input_mode: InputMode,
    pub input: String,
    pub output: String,
    pub cache_read: String,
    pub cache_write: String,
    pub label: String,
    /// True between submitting and the core's reply.
    pub pending: bool,
    pub error: Option<String>,
    /// Reply tag of the save currently in flight.
    pub tag: Option<String>,
    /// Identifies this draft so a late reply cannot close a newer one.
    pub generation: u64,
}

impl PriceDraft {
    /// A blank draft for a new price, owned by the given provider.
    pub fn blank(provider_id: String, generation: u64) -> Self {
        Self {
            price_id: None,
            expected_revision: 0,
            provider_id,
            upstream_model: String::new(),
            currency: Currency::Cny,
            input_mode: InputMode::Flat,
            input: String::new(),
            output: String::new(),
            cache_read: String::new(),
            cache_write: String::new(),
            label: String::new(),
            pending: false,
            error: None,
            tag: None,
            generation,
        }
    }

    /// Fills every field from an existing price. The provider and model are
    /// copied too, so saving cannot silently move the price elsewhere.
    pub fn editing(rule: &PriceRuleDto, generation: u64) -> Self {
        let amount = |nanos: &str| crate::usage_table::rate_input(nanos);
        Self {
            price_id: Some(rule.price_id.clone()),
            expected_revision: rule.revision,
            provider_id: rule.provider_id.clone(),
            upstream_model: rule.upstream_model.clone(),
            currency: rule.currency,
            input_mode: rule.input_mode,
            input: amount(&rule.input_rate_nanos),
            output: amount(&rule.output_rate_nanos),
            cache_read: rule
                .cache_read_rate_nanos
                .as_deref()
                .map(amount)
                .unwrap_or_default(),
            cache_write: rule
                .cache_write_rate_nanos
                .as_deref()
                .map(amount)
                .unwrap_or_default(),
            label: rule.label.clone(),
            pending: false,
            error: None,
            tag: None,
            generation,
        }
    }

    pub fn is_editing(&self) -> bool {
        self.price_id.is_some()
    }

    /// The identifying pair cannot be edited: changing it would either fail at
    /// the core or, worse, look like an edit while the list kept the old key.
    pub fn identity_locked(&self) -> bool {
        self.is_editing()
    }

    /// Records that the draft was submitted and builds the payload.
    ///
    /// The boxes hold per-million amounts a human typed; the API stores integer
    /// nano units, so the conversion happens here — exactly once.
    pub fn submit(&mut self, tag: String) -> Result<Value, String> {
        // A blank box is the documented zero default.
        let rate = |label: &str, value: &str| -> Result<Value, String> {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                return Ok(Value::String("0".to_owned()));
            }
            crate::stats_dto::parse_human_amount(trimmed)
                .map(|nanos| Value::String(nanos.to_string()))
                .map_err(|detail| format!("{label}: {detail}"))
        };
        let optional_rate = |label: &str, value: &str| -> Result<Value, String> {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                return Ok(Value::String("0".to_owned()));
            }
            crate::stats_dto::parse_human_amount(trimmed)
                .map(|nanos| Value::String(nanos.to_string()))
                .map_err(|detail| format!("{label}: {detail}"))
        };
        let input = match rate("input", &self.input) {
            Ok(value) => value,
            Err(detail) => {
                self.error = Some(detail.clone());
                return Err(detail);
            }
        };
        let output = match rate("output", &self.output) {
            Ok(value) => value,
            Err(detail) => {
                self.error = Some(detail.clone());
                return Err(detail);
            }
        };
        let cache_read = match optional_rate("cache read", &self.cache_read) {
            Ok(value) => value,
            Err(detail) => {
                self.error = Some(detail.clone());
                return Err(detail);
            }
        };
        let cache_write = match optional_rate("cache write", &self.cache_write) {
            Ok(value) => value,
            Err(detail) => {
                self.error = Some(detail.clone());
                return Err(detail);
            }
        };
        self.pending = true;
        self.error = None;
        self.tag = Some(tag.clone());
        let mut payload = json!({
            "command": "stats_price_save",
            "request_tag": tag,
            "provider_id": self.provider_id,
            "upstream_model": self.upstream_model,
            "currency": self.currency.as_str(),
            "input_mode": self.input_mode.as_str(),
            "input_rate_nanos": input,
            "output_rate_nanos": output,
            "cache_read_rate_nanos": cache_read,
            "cache_write_rate_nanos": cache_write,
            "label": self.label.trim(),
            "expected_revision": self.expected_revision,
        });
        if let Some(price_id) = &self.price_id {
            payload["price_id"] = json!(price_id);
        }
        Ok(payload)
    }

    /// Applies a reply. Returns true when the save really landed and the dialog
    /// may close. A reply for an older draft is ignored entirely.
    pub fn apply_reply(&mut self, tag: &str, result: Result<(), String>) -> bool {
        if self.tag.as_deref() != Some(tag) {
            // A reply for a draft the user has already replaced.
            return false;
        }
        self.pending = false;
        self.tag = None;
        match result {
            Ok(()) => true,
            Err(message) => {
                // Keep every typed value so the failure can be corrected.
                self.error = Some(message);
                false
            }
        }
    }

    /// True when the draft still has to be sent before it can be trusted.
    pub fn is_complete(&self) -> bool {
        !self.provider_id.trim().is_empty() && !self.upstream_model.trim().is_empty()
    }
}

/// Cursor stack for one listing: the cursors that produced the pages before the
/// current one, so "previous" can re-issue them and "next" can advance.
#[derive(Clone, Debug, PartialEq)]
pub struct CursorStack<T> {
    /// Cursor used to fetch the current page (`None` for the first page).
    pub current: Option<T>,
    /// Cursor returned by the current page, if the core offered one.
    pub next: Option<T>,
    /// Cursors of the pages before the current one.
    history: Vec<Option<T>>,
}

impl<T> Default for CursorStack<T> {
    fn default() -> Self {
        Self {
            current: None,
            next: None,
            history: Vec::new(),
        }
    }
}

impl<T: Clone> CursorStack<T> {
    pub fn reset(&mut self) {
        self.current = None;
        self.next = None;
        self.history.clear();
    }

    /// Records the cursor the current page was fetched with.
    pub fn begin_page(&mut self, cursor: Option<T>) {
        self.current = cursor;
    }

    /// Stores the cursor the core offered for the following page.
    pub fn finish_page(&mut self, next: Option<T>) {
        self.next = next;
    }

    pub fn can_go_next(&self) -> bool {
        self.next.is_some()
    }

    pub fn can_go_back(&self) -> bool {
        !self.history.is_empty()
    }

    /// Moves forward; returns the cursor to request.
    pub fn forward(&mut self) -> Option<Option<T>> {
        let next = self.next.clone()?;
        self.history.push(self.current.clone());
        Some(Some(next))
    }

    /// Moves back; returns the cursor to request.
    pub fn back(&mut self) -> Option<Option<T>> {
        let previous = self.history.pop()?;
        self.next = self.current.clone();
        Some(previous)
    }

    pub fn page_number(&self) -> usize {
        self.history.len() + 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stats_dto::PriceRuleDto;

    fn rule(price_id: &str, provider: &str, model: &str) -> PriceRuleDto {
        PriceRuleDto {
            price_id: price_id.to_owned(),
            rule_id: price_id.to_owned(),
            provider_id: provider.to_owned(),
            upstream_model: model.to_owned(),
            currency: Currency::Cny,
            input_rate_nanos: "2000000000".to_owned(),
            output_rate_nanos: "8000000000".to_owned(),
            cache_read_rate_nanos: Some("200000000".to_owned()),
            cache_write_rate_nanos: None,
            input_mode: InputMode::Disjoint,
            label: "fixture".to_owned(),
            created_at_ms: 1,
            revision: 4,
            referenced: false,
        }
    }

    #[test]
    fn invalid_date_drafts_cannot_replace_an_applied_range() {
        let applied = (1000, 2000);
        for draft in [
            (None, Some(2000)),
            (Some(3000), Some(2000)),
            (Some(0), Some(367 * 86_400_000)),
        ] {
            assert!(validate_range(draft.0, draft.1).is_err());
            assert_eq!(validate_range(draft.0, draft.1).unwrap_or(applied), applied);
        }
        assert_eq!(
            validate_range(Some(0), Some(366 * 86_400_000)),
            Ok((0, 366 * 86_400_000))
        );
        assert!(!may_refresh(true, true, true, true));
        assert!(!may_refresh(false, true, false, true));
        assert!(!may_refresh(true, false, false, true));
        assert!(!may_refresh(true, true, false, false));
        assert!(may_refresh(true, true, false, true));
    }

    #[test]
    fn a_reply_is_only_applied_while_its_own_tab_is_on_screen() {
        let mut rows = ReplyGate::default();
        let mut groups = ReplyGate::default();
        let rows_seq = rows
            .issue(
                1,
                StatsTab::Rows,
                GroupBy::Model,
                SortMetric::TotalTokens,
                None,
            )
            .seq;
        assert_eq!(
            route_reply(ReplyKind::Rows, rows_seq, 0, &rows, &groups, StatsTab::Rows),
            ReplyRoute::Apply
        );
        // The user switched to providers before the reply landed: decoding it
        // as a grouping would put request rows into the group table.
        assert_eq!(
            route_reply(
                ReplyKind::Rows,
                rows_seq,
                0,
                &rows,
                &groups,
                StatsTab::Groups
            ),
            ReplyRoute::Ignore
        );
        let groups_seq = groups
            .issue(
                1,
                StatsTab::Groups,
                GroupBy::Provider,
                SortMetric::TotalTokens,
                None,
            )
            .seq;
        assert_eq!(
            route_reply(
                ReplyKind::Groups,
                groups_seq,
                0,
                &rows,
                &groups,
                StatsTab::Groups
            ),
            ReplyRoute::Apply
        );
        assert_eq!(
            route_reply(
                ReplyKind::Groups,
                groups_seq,
                0,
                &rows,
                &groups,
                StatsTab::Rows
            ),
            ReplyRoute::Ignore
        );
    }

    #[test]
    fn a_dashboard_reply_from_the_previous_filter_is_ignored() {
        let rows = ReplyGate::default();
        let groups = ReplyGate::default();
        // The filter changed and the debounce has not fired yet: the old
        // dashboard must not paint the previous window.
        assert_eq!(
            route_reply(ReplyKind::Dashboard, 4, 0, &rows, &groups, StatsTab::Rows),
            ReplyRoute::Ignore
        );
        assert_eq!(
            route_reply(ReplyKind::Dashboard, 5, 5, &rows, &groups, StatsTab::Rows),
            ReplyRoute::Apply
        );
    }

    #[test]
    fn editing_a_price_fills_every_field_and_keeps_its_identity() {
        let draft = PriceDraft::editing(&rule("price-1", "provider-a", "model-x"), 7);
        assert_eq!(draft.price_id.as_deref(), Some("price-1"));
        assert_eq!(draft.expected_revision, 4);
        assert_eq!(draft.provider_id, "provider-a");
        assert_eq!(draft.upstream_model, "model-x");
        assert_eq!(draft.currency, Currency::Cny);
        assert_eq!(draft.input_mode, InputMode::Disjoint);
        // Stored nano rates are rendered as human amounts exactly once.
        assert_eq!(draft.input, "2");
        assert_eq!(draft.output, "8");
        assert_eq!(draft.cache_read, "0.2");
        assert_eq!(draft.cache_write, "");
        assert_eq!(draft.label, "fixture");
        assert!(draft.identity_locked());
    }

    #[test]
    fn alternating_edits_never_leak_between_drafts() {
        let first = PriceDraft::editing(&rule("price-cny", "p-cny", "m-cny"), 1);
        let mut second = PriceDraft::blank("p-usd".to_owned(), 2);
        second.upstream_model = "m-usd".to_owned();
        second.currency = Currency::Usd;
        second.input_mode = InputMode::Flat;
        second.input = "3".to_owned();
        second.output = "9".to_owned();
        assert_eq!(first.currency, Currency::Cny);
        assert_eq!(first.input_mode, InputMode::Disjoint);
        assert_eq!(second.currency, Currency::Usd);
        assert_eq!(second.input_mode, InputMode::Flat);
        // Reopening the first one restores its own values, not the second's.
        let reopened = PriceDraft::editing(&rule("price-cny", "p-cny", "m-cny"), 3);
        assert_eq!(reopened.currency, Currency::Cny);
        assert_eq!(reopened.cache_read, "0.2");
    }

    #[test]
    fn a_label_only_edit_still_carries_the_original_rates_and_revision() {
        let mut draft = PriceDraft::editing(&rule("price-1", "p", "m"), 1);
        draft.label = "renamed".to_owned();
        let payload = draft.submit("tag-1".to_owned()).unwrap();
        assert_eq!(payload["price_id"], "price-1");
        assert_eq!(payload["expected_revision"], 4);
        // $2/M is stored as 2000000000 nano units.
        assert_eq!(payload["input_rate_nanos"], "2000000000");
        assert_eq!(payload["cache_read_rate_nanos"], "200000000");
        assert_eq!(payload["label"], "renamed");
    }

    #[test]
    fn a_blank_rate_is_sent_as_the_documented_zero() {
        let mut draft = PriceDraft::blank("p".to_owned(), 1);
        draft.upstream_model = "m".to_owned();
        let payload = draft.submit("tag".to_owned()).unwrap();
        assert_eq!(payload["input_rate_nanos"], "0");
        assert_eq!(payload["output_rate_nanos"], "0");
        assert_eq!(payload["cache_read_rate_nanos"], "0");
        // Creating is revision 0 and carries no identifier.
        assert_eq!(payload["expected_revision"], 0);
        assert!(payload.get("price_id").is_none());
    }

    #[test]
    fn a_failed_save_keeps_the_typed_values_for_correction() {
        let mut draft = PriceDraft::blank("p".to_owned(), 1);
        draft.upstream_model = "m".to_owned();
        draft.input = "99".to_owned();
        draft.submit("tag-1".to_owned()).unwrap();
        assert!(draft.pending);
        let closed = draft.apply_reply(
            "tag-1",
            Err("stats_revision_conflict: someone else changed it".to_owned()),
        );
        assert!(!closed, "a failure must not close the dialog");
        assert!(!draft.pending);
        assert_eq!(draft.input, "99", "the typed value must survive");
        assert!(
            draft
                .error
                .as_deref()
                .unwrap_or("")
                .contains("revision_conflict"),
            "{:?}",
            draft.error
        );
    }

    #[test]
    fn a_successful_save_closes_the_draft() {
        let mut draft = PriceDraft::editing(&rule("price-1", "p", "m"), 1);
        draft.submit("tag-1".to_owned()).unwrap();
        assert!(draft.apply_reply("tag-1", Ok(())));
        assert!(!draft.pending);
        assert!(draft.error.is_none());
    }

    #[test]
    fn a_reply_for_an_older_draft_cannot_close_the_newer_one() {
        let mut draft = PriceDraft::blank("p".to_owned(), 1);
        draft.upstream_model = "m".to_owned();
        draft.submit("tag-old".to_owned()).unwrap();
        // The user cancelled and opened a fresh draft before the reply arrived.
        let mut newer = PriceDraft::blank("p".to_owned(), 2);
        newer.upstream_model = "m2".to_owned();
        newer.input = "5".to_owned();
        newer.submit("tag-new".to_owned()).unwrap();
        let closed = newer.apply_reply("tag-old", Ok(()));
        assert!(!closed, "an old reply must not close the current draft");
        assert!(newer.pending, "the newer draft is still waiting");
        assert_eq!(newer.input, "5");
    }

    #[test]
    fn a_reply_that_changed_the_filters_is_not_applied() {
        let mut gate = ReplyGate::default();
        let issued = gate.issue(
            1,
            StatsTab::Rows,
            GroupBy::Model,
            SortMetric::TotalTokens,
            None,
        );
        assert!(gate.may_apply_seq(issued.seq));
        // The user changes the filter: every outstanding reply dies immediately,
        // before the debounce issues anything.
        gate.invalidate();
        assert!(!gate.may_apply_seq(issued.seq));
        assert!(gate.current().is_none());
    }

    #[test]
    fn switching_tabs_or_grouping_rejects_the_previous_reply() {
        let mut gate = ReplyGate::default();
        let rows = gate.issue(
            1,
            StatsTab::Rows,
            GroupBy::Model,
            SortMetric::TotalTokens,
            None,
        );
        let groups = gate.issue(
            1,
            StatsTab::Groups,
            GroupBy::Provider,
            SortMetric::TotalTokens,
            None,
        );
        assert!(
            !gate.may_apply_seq(rows.seq),
            "a rows reply must not fill the groups tab"
        );
        assert!(gate.may_apply_seq(groups.seq));
    }

    #[test]
    fn a_page_reply_that_arrived_after_paging_on_does_not_clear_the_newer_pending() {
        let mut gate = ReplyGate::default();
        let first = gate.issue(
            1,
            StatsTab::Rows,
            GroupBy::Model,
            SortMetric::TotalTokens,
            None,
        );
        let second = gate.issue(
            1,
            StatsTab::Rows,
            GroupBy::Model,
            SortMetric::TotalTokens,
            Some("cursor-2".to_owned()),
        );
        // The first page's reply landed late: it is no longer the current read,
        // so it cannot clear the newer pending flag.
        assert!(!gate.may_apply_seq(first.seq));
        assert!(gate.may_apply_seq(second.seq));
        assert_eq!(gate.current(), Some(&second));
    }

    #[test]
    fn cursor_history_walks_forward_and_back_through_three_pages() {
        let mut stack: CursorStack<String> = CursorStack::default();
        stack.begin_page(None);
        stack.finish_page(Some("c2".to_owned()));
        assert_eq!(stack.page_number(), 1);
        assert!(stack.can_go_next());
        assert!(!stack.can_go_back());

        let cursor = stack.forward().unwrap();
        assert_eq!(cursor, Some("c2".to_owned()));
        stack.begin_page(cursor);
        stack.finish_page(Some("c3".to_owned()));
        assert_eq!(stack.page_number(), 2);

        let cursor = stack.forward().unwrap();
        assert_eq!(cursor, Some("c3".to_owned()));
        stack.begin_page(cursor);
        stack.finish_page(None);
        assert_eq!(stack.page_number(), 3);
        assert!(!stack.can_go_next(), "the last page offers no next cursor");

        // 3 -> 2 -> 1 replays the very cursors that produced those pages.
        assert_eq!(stack.back(), Some(Some("c2".to_owned())));
        stack.begin_page(Some("c2".to_owned()));
        assert_eq!(stack.page_number(), 2);
        assert_eq!(stack.back(), Some(None));
        stack.begin_page(None);
        assert_eq!(stack.page_number(), 1);
        assert!(!stack.can_go_back());
    }

    #[test]
    fn resetting_the_stack_returns_to_the_first_page() {
        let mut stack: CursorStack<String> = CursorStack::default();
        stack.begin_page(None);
        stack.finish_page(Some("c2".to_owned()));
        let cursor = stack.forward().unwrap();
        stack.begin_page(cursor);
        stack.reset();
        assert_eq!(stack.page_number(), 1);
        assert!(!stack.can_go_back());
        assert!(!stack.can_go_next());
        assert_eq!(stack.current, None);
    }
}

#[cfg(test)]
mod scan_budget_tests {
    use super::*;

    #[test]
    fn a_running_scan_cannot_be_polled_forever() {
        // AC062: the polls a scan may cause are bounded, so a slow scan cannot
        // turn into an endless stream of requests.
        let mut budget = ScanPollBudget::new();
        let mut polls = 0;
        while budget.next_poll() {
            polls += 1;
        }
        assert_eq!(polls, ScanPollBudget::MAX_POLLS as i32);
        assert!(budget.is_exhausted());
        assert!(
            !budget.next_poll(),
            "a spent budget never yields another poll"
        );
    }

    #[test]
    fn the_read_gate_spends_the_budget_it_owns() {
        // The budget is not decoration: every issued read consumes one poll.
        let mut gate = ReplyGate::default();
        assert_eq!(gate.scan_poll.remaining, ScanPollBudget::MAX_POLLS);
        for expected in 1..=3 {
            gate.issue(
                1,
                StatsTab::Rows,
                GroupBy::Model,
                SortMetric::TotalTokens,
                None,
            );
            assert_eq!(
                gate.scan_poll.remaining,
                ScanPollBudget::MAX_POLLS - expected,
                "each read spends exactly one poll"
            );
        }
    }

    #[test]
    fn a_stale_reply_cannot_overwrite_a_newer_read() {
        // AC062: switching the source (or any filter) issues a new identity, so
        // a reply that is still in flight for the old one is refused.
        let mut gate = ReplyGate::default();
        let old = gate.issue(
            1,
            StatsTab::Rows,
            GroupBy::Model,
            SortMetric::TotalTokens,
            None,
        );
        assert!(gate.may_apply_seq(old.seq));

        let new = gate.issue(
            2,
            StatsTab::Rows,
            GroupBy::Model,
            SortMetric::TotalTokens,
            None,
        );
        assert!(gate.may_apply_seq(new.seq));
        assert!(
            !gate.may_apply_seq(old.seq),
            "the slow reply for the previous source is dropped"
        );

        // Invalidating without issuing also drops everything in flight.
        gate.invalidate();
        assert!(!gate.may_apply_seq(new.seq));
    }
}

#[cfg(test)]
mod filter_change_tests {
    use super::*;

    #[test]
    fn a_change_invalidates_first_and_then_owes_exactly_one_request() {
        // AC067: the order is fixed — invalidate, reset pagination, then one
        // request. A second take finds nothing owed, so a single change cannot
        // turn into a burst.
        let mut coordinator = FilterCoordinator::new();
        assert_eq!(coordinator.generation(), 0);
        assert_eq!(coordinator.change(), FilterChange::Invalidated);
        assert_eq!(coordinator.generation(), 1);
        assert!(coordinator.take_request(), "the change owes one request");
        assert!(!coordinator.take_request(), "and only one");
        coordinator.change();
        assert_eq!(coordinator.generation(), 2);
        assert!(coordinator.take_request());
    }

    #[test]
    fn clicking_the_option_that_is_already_selected_changes_nothing() {
        // AC067: the component refuses the click, so no generation is opened and
        // no request can follow.
        use crate::stats_dto::SourceScope;
        let mut source = crate::source_filter::SourceFilter::new();
        let coordinator = FilterCoordinator::new();
        assert_eq!(
            source.select(SourceScope::All),
            crate::source_filter::SelectionOutcome::Unchanged
        );
        assert_eq!(coordinator.generation(), 0, "nothing was invalidated");
    }

    #[test]
    fn switching_to_builtin_drops_the_hidden_proxy_value_for_good() {
        // AC067: the value the new scope hides is cleared, and switching back
        // does not bring it back.
        let mut capture = CaptureFilter::new();
        capture.set_choice_for_test(CaptureChoice::Proxy);
        assert_eq!(capture.displayed(SourceScope::All), CaptureChoice::Proxy);

        assert!(
            capture.reconcile(SourceScope::Builtin),
            "the hidden value is cleared"
        );
        assert_eq!(
            capture.displayed(SourceScope::Builtin),
            CaptureChoice::WorkbuddyImport
        );
        assert!(capture.is_locked(SourceScope::Builtin));

        // Back to "all": the dropdown is usable again, but stays on the default.
        assert!(
            !capture.reconcile(SourceScope::All),
            "nothing left to clear"
        );
        assert_eq!(capture.displayed(SourceScope::All), CaptureChoice::All);
    }

    #[test]
    fn the_test_switch_is_cleared_the_moment_it_becomes_unavailable() {
        // The switch is only meaningful for the proxy's own traffic, and it
        // never reads as on where it cannot be used.
        let capture = CaptureFilter::new();
        assert!(capture.include_tests_available(SourceScope::All));
        assert!(capture.include_tests_available(SourceScope::Supplier));
        assert!(
            !capture.include_tests_available(SourceScope::Builtin),
            "the built-in source has no test connections"
        );
        assert!(!capture.include_tests(), "and the switch starts off");

        let mut imported = CaptureFilter::new();
        imported.set_choice_for_test(CaptureChoice::WorkbuddyImport);
        assert!(!imported.include_tests_available(SourceScope::All));
        assert!(!imported.include_tests());
    }

    #[test]
    fn a_locked_dropdown_refuses_a_change() {
        let locked = CaptureFilter::new();
        assert!(locked.is_locked(SourceScope::Builtin));
        assert_eq!(locked.effective(SourceScope::Builtin), CaptureChoice::All);
    }

    #[test]
    fn the_effective_choice_lets_the_source_do_the_restricting() {
        let mut capture = CaptureFilter::new();
        capture.set_choice_for_test(CaptureChoice::Proxy);
        assert_eq!(capture.effective(SourceScope::All), CaptureChoice::Proxy);
        assert_eq!(
            capture.effective(SourceScope::Builtin),
            CaptureChoice::All,
            "a locked dropdown sends all and lets the scope restrict"
        );
    }
}
