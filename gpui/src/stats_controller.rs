//! Page state shared by the real statistics view.
//!
//! This module deliberately holds no GPUI types: the view maps its inputs and
//! selects onto these values, so the state machine can be tested without a
//! window. Everything here is driven by `statistics.rs`; it is not a parallel
//! implementation kept only for tests.

use crate::stats_dto::{Currency, GroupBy, InputMode, PriceRuleDto, SortMetric};
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
    /// Health, prices, settings and the rest are not paginated.
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
