//! Historical attribution classification for the WorkBuddy importer.
//!
//! The "did this installation write this WorkBuddy entry" test is **not**
//! re-implemented here. It is the very same shared predicate the proxy sync path
//! uses ([`crate::workbuddy::managed_signature`]), so the crate has exactly one
//! detector and no second one guessing from a display name, an alias or a bare
//! `localhost` string.
//!
//! Everything this module reads is non-sensitive: a boolean marker, the vendor
//! literal, the *shape* of the credential, and the host/path shape of the URL. No key,
//! URL, content or identifier is returned or stored.

use crate::workbuddy::route;
use serde_json::Value;

/// True when a WorkBuddy entry carries any non-sensitive proxy signature.
///
/// Delegates to the shared predicates; it adds no rule of its own.
pub fn carries_proxy_signature(value: &Value, local_key: &str) -> bool {
    crate::workbuddy::managed_signature(value, local_key)
}

/// The proxy route id of an entry, when it is one of ours and has a route.
pub fn proxy_route(value: &Value, local_key: &str) -> Option<String> {
    if carries_proxy_signature(value, local_key) {
        route(value)
    } else {
        None
    }
}

/// What the current WorkBuddy configuration proves, right now.
///
/// Deliberately tiny and non-sensitive: a model id, whether the entry is one of
/// ours, and its route. The credential and the URL are **never** copied out of
/// the configuration, not even to be compared later.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigObservation {
    /// True when the entry currently carries a proxy signature.
    pub managed: bool,
    /// The model id the entry declares, when it is one of ours.
    pub model_id: Option<String>,
    /// The proxy route id, when it has one.
    pub route_id: Option<String>,
    /// When the observation was taken.
    pub observed_at_ms: i64,
}

impl ConfigObservation {
    /// The half-open interval this observation can support: `[observed, ∞)`.
    ///
    /// It always starts at the observation instant. An observation of *today's*
    /// configuration can never say anything about earlier records, so no
    /// interval is ever back-dated from it.
    pub fn interval(&self) -> (i64, Option<i64>) {
        (self.observed_at_ms, None)
    }

    /// True when the observation is strong enough to classify anything.
    ///
    /// A direct (non-proxy) configuration proves nothing about the past, and a
    /// proxy entry without a model id cannot attribute a record to a model.
    pub fn is_usable(&self) -> bool {
        self.managed && self.model_id.is_some()
    }

    /// True when this observation covers an instant.
    pub fn covers(&self, at_ms: i64) -> bool {
        self.is_usable() && at_ms >= self.observed_at_ms
    }
}

/// The model id an entry declares, without copying anything else out.
fn current_model_id(value: &Value) -> Option<String> {
    // models.json stores the custom alias in `id`; JSONL prefixes that alias.
    // The provider's underlying model name is not this routing identity.
    if let Some(id) = value
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
    {
        let id = if id.starts_with("custom-local:") {
            id.to_owned()
        } else {
            format!("custom-local:{id}")
        };
        return (id.chars().count() <= crate::stats::dto::MAX_ID_CHARS
            && !id.chars().any(char::is_control))
        .then_some(id);
    }
    if let Some(model) = value.get("model").and_then(Value::as_str)
        && !model.is_empty()
    {
        return Some(model.to_owned());
    }
    for entry in value.get("models")?.as_array()? {
        if let Some(model) = entry.as_str()
            && !model.is_empty()
        {
            return Some(model.to_owned());
        }
        if let Some(model) = entry.get("id").and_then(Value::as_str)
            && !model.is_empty()
        {
            return Some(model.to_owned());
        }
    }
    None
}

/// Observes the current configuration at one instant.
///
/// Reads the signature, the route and the model id only. A direct
/// configuration yields `managed = false` and therefore proves nothing about
/// history; an unprovable older interval is left to `pending_classification`
/// rather than being guessed from today's settings.
pub fn observe_current(value: &Value, local_key: &str, now_ms: i64) -> ConfigObservation {
    let managed = carries_proxy_signature(value, local_key);
    ConfigObservation {
        managed,
        model_id: if managed {
            current_model_id(value)
        } else {
            None
        },
        route_id: if managed { route(value) } else { None },
        observed_at_ms: now_ms,
    }
}

/// How one historical record is attributed.
///
/// The legacy attribution vocabulary is retained for ledger compatibility. `Proxy` is
/// `excluded_proxy` with no fact row, `Builtin` and `Direct` each produce one
/// `usage_requests` row, and `Pending` becomes `pending_classification` with an
/// allowlisted candidate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Attribution {
    /// The record went through this proxy.
    Proxy,
    /// A verified built-in format: recognised by evidence, never by its name.
    Builtin,
    /// A direct route confirmed for this record's interval.
    Direct,
    /// Nothing proves it either way: never guessed.
    Pending,
}

impl Attribution {
    /// Every outcome, in the fixed decision order.
    pub const ALL: &'static [Attribution] = &[
        Attribution::Proxy,
        Attribution::Builtin,
        Attribution::Direct,
        Attribution::Pending,
    ];

    /// True when this outcome writes a usage fact row.
    pub fn is_imported(self) -> bool {
        matches!(self, Attribution::Builtin | Attribution::Direct)
    }

    /// Stable ledger state name.
    pub fn as_str(self) -> &'static str {
        match self {
            Attribution::Proxy => "excluded_proxy",
            Attribution::Builtin | Attribution::Direct => "imported",
            Attribution::Pending => "pending_classification",
        }
    }
}

/// The evidence available for one record.
///
/// Every field is a *fact already established elsewhere* — a durable mapping
/// interval, an existing proxy fact, a verified built-in format, a confirmed
/// direct interval. Nothing here is derived from a model name, a route name or
/// a host, so a model called `custom-local:...` is not proxy by definition.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AttributionInput {
    /// A verified proxy mapping covers this record's model and interval.
    pub proxy_mapping: bool,
    /// A proxy fact already exists for this record's identity.
    pub proxy_fact: bool,
    /// The model matches a verified built-in format.
    pub builtin_format: bool,
    /// A confirmed direct interval covers this record's model and interval.
    pub confirmed_direct: bool,
}

/// Classifies one record with the fixed order of DATA-API section 6.
///
/// `proxy` first — and a known proxy model is excluded even when the proxy has
/// no matching fact, because a missing record is not evidence of a direct call.
/// Then the verified built-in formats, then an explicitly confirmed direct
/// interval, and only then `pending`. There is no fifth branch and no name
/// matching: a record with no evidence is never assumed to be either route.
pub fn attribute(input: &AttributionInput) -> Attribution {
    if input.proxy_mapping || input.proxy_fact {
        return Attribution::Proxy;
    }
    if input.builtin_format {
        return Attribution::Builtin;
    }
    if input.confirmed_direct {
        return Attribution::Direct;
    }
    Attribution::Pending
}

/// One historical interval of one model's attribution.
///
/// Intervals are half-open `[valid_from_ms, valid_to_ms)`: adjacent intervals
/// are legal, and the instant exactly at `valid_to_ms` belongs to the next one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MappingInterval {
    pub mapping_id: String,
    /// The model id this interval was established for, matched exactly.
    pub model_id: String,
    /// `Proxy` or `Direct`; the other two outcomes are not intervals.
    pub classification: Attribution,
    pub valid_from_ms: i64,
    /// Exclusive upper bound; `None` means "still in force".
    pub valid_to_ms: Option<i64>,
}

impl MappingInterval {
    /// True when this interval covers an instant.
    pub fn covers(&self, at_ms: i64) -> bool {
        self.valid_from_ms <= at_ms && self.valid_to_ms.is_none_or(|to| at_ms < to)
    }
}

/// The outcome of looking up the intervals of one model.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IntervalMatch {
    /// No interval covers the instant.
    None,
    /// Exactly one interval covers it.
    One(Box<MappingInterval>),
    /// More than one covers it: never resolved by picking the newest.
    Conflict,
}

/// Looks up the interval covering one model at one instant.
///
/// Matching is by **exact model id and time only**. A model that is called
/// something else today, or a route that has been renamed or deleted since,
/// cannot change what an earlier interval says: the past is never rewritten
/// from the present.
pub fn interval_for(intervals: &[MappingInterval], model_id: &str, at_ms: i64) -> IntervalMatch {
    let mut found = intervals
        .iter()
        .filter(|interval| interval.model_id == model_id && interval.covers(at_ms));
    let Some(first) = found.next() else {
        return IntervalMatch::None;
    };
    if found.next().is_some() {
        return IntervalMatch::Conflict;
    }
    IntervalMatch::One(Box::new(first.clone()))
}

/// Classifies a record from its intervals, falling back to the other evidence.
///
/// A covered interval decides the route; an interval conflict is a conflict, not
/// a coin toss; and with no interval the remaining facts (`proxy_fact`,
/// `builtin_format`, `confirmed_direct`) still apply in the fixed order.
pub fn attribute_with_intervals(
    intervals: &[MappingInterval],
    model_id: Option<&str>,
    at_ms: i64,
    input: &AttributionInput,
) -> Result<Attribution, AttributionConflict> {
    if let Some(model_id) = model_id {
        match interval_for(intervals, model_id, at_ms) {
            IntervalMatch::One(interval) => return Ok(interval.classification),
            IntervalMatch::Conflict => return Err(AttributionConflict::OverlappingIntervals),
            IntervalMatch::None => {}
        }
    }
    Ok(attribute(input))
}

/// Why an attribution could not be decided.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttributionConflict {
    /// Two verified intervals cover the same model at the same instant.
    OverlappingIntervals,
}

impl AttributionConflict {
    /// Stable error code.
    pub fn as_str(self) -> &'static str {
        match self {
            AttributionConflict::OverlappingIntervals => "mapping_conflict",
        }
    }
}

/// Startup contract check for the classification predicates.
///
/// Pure and offline: it asserts that the shared detector behaves as documented
/// and that this module did not grow a second one.
pub fn classify_contract_check() -> Result<usize, &'static str> {
    let marked = serde_json::json!({"_workbuddyProxy": {"managed": true}});
    if !carries_proxy_signature(&marked, "") {
        return Err("workbuddy_import_classify_marker_refused");
    }
    let local_key = "0".repeat(48);
    let legacy = serde_json::json!({
        "vendor": "Custom",
        "apiKey": local_key,
        "url": "http://127.0.0.1:1/r/route-one/v1",
        "model": "probe-model",
    });
    if !carries_proxy_signature(&legacy, &local_key) {
        return Err("workbuddy_import_classify_legacy_refused");
    }
    if proxy_route(&legacy, &local_key).as_deref() != Some("route-one") {
        return Err("workbuddy_import_classify_route_drifted");
    }
    // A display name, an alias or a bare loopback URL is never enough.
    let guessed = serde_json::json!({
        "name": "WorkBuddy Proxy",
        "id": "workbuddy",
        "url": "http://localhost:1/v1",
    });
    if carries_proxy_signature(&guessed, &local_key) {
        return Err("workbuddy_import_classify_guess_accepted");
    }
    let foreign_key = serde_json::json!({
        "vendor": "Custom",
        "apiKey": "not-a-local-key",
        "url": "http://127.0.0.1:1/v1",
    });
    if carries_proxy_signature(&foreign_key, &local_key) {
        return Err("workbuddy_import_classify_foreign_key_accepted");
    }
    // The observation must be usable for a proxy entry and must never be
    // back-dated; a direct configuration proves nothing.
    let observed = observe_current(&legacy, &local_key, 5_000);
    if !observed.is_usable() || observed.interval() != (5_000, None) {
        return Err("workbuddy_import_classify_observation_drifted");
    }
    if !observed.covers(5_000) || observed.covers(4_999) {
        return Err("workbuddy_import_classify_observation_backdated");
    }
    let direct = observe_current(&guessed, &local_key, 5_000);
    if direct.is_usable() || direct.interval() != (5_000, None) {
        return Err("workbuddy_import_classify_direct_usable");
    }
    // The decision order is part of the contract: proxy wins over everything,
    // a known proxy model stays excluded without a matching fact, and a record
    // with no evidence is pending rather than guessed.
    if attribute(&AttributionInput {
        proxy_mapping: true,
        builtin_format: true,
        confirmed_direct: true,
        ..AttributionInput::default()
    }) != Attribution::Proxy
    {
        return Err("workbuddy_import_classify_order_drifted");
    }
    if attribute(&AttributionInput {
        proxy_mapping: true,
        ..AttributionInput::default()
    }) != Attribution::Proxy
    {
        return Err("workbuddy_import_classify_missing_fact_imported");
    }
    if attribute(&AttributionInput::default()) != Attribution::Pending {
        return Err("workbuddy_import_classify_unknown_guessed");
    }
    if Attribution::ALL.len() != 4 || !Attribution::Direct.is_imported() {
        return Err("workbuddy_import_classify_outcomes_drifted");
    }
    if Attribution::Proxy.as_str() != "excluded_proxy"
        || Attribution::Pending.as_str() != "pending_classification"
        || Attribution::Builtin.as_str() != "imported"
    {
        return Err("workbuddy_import_classify_state_names_drifted");
    }
    // Interval lookup is exact and half-open, and a conflict is never resolved
    // by preference.
    let intervals = [
        MappingInterval {
            mapping_id: "m1".into(),
            model_id: "custom-local:x".into(),
            classification: Attribution::Direct,
            valid_from_ms: 0,
            valid_to_ms: Some(100),
        },
        MappingInterval {
            mapping_id: "m2".into(),
            model_id: "custom-local:x".into(),
            classification: Attribution::Proxy,
            valid_from_ms: 100,
            valid_to_ms: None,
        },
    ];
    if interval_for(&intervals, "custom-local:x", 99)
        != IntervalMatch::One(Box::new(intervals[0].clone()))
        || interval_for(&intervals, "custom-local:x", 100)
            != IntervalMatch::One(Box::new(intervals[1].clone()))
    {
        return Err("workbuddy_import_classify_interval_boundary_drifted");
    }
    if attribute_with_intervals(
        &intervals,
        Some("custom-local:x"),
        50,
        &AttributionInput::default(),
    ) != Ok(Attribution::Direct)
    {
        return Err("workbuddy_import_classify_interval_drifted");
    }
    if AttributionConflict::OverlappingIntervals.as_str() != "mapping_conflict" {
        return Err("workbuddy_import_classify_conflict_code_drifted");
    }
    // This module must not carry a second detector: no URL parsing, no host
    // test and no key-shape test of its own, only delegation to the shared
    // predicates. The needles are assembled from fragments so this check cannot
    // match its own text, and the test module is excluded because it builds
    // sample URLs on purpose.
    let source = include_str!("classify.rs");
    let production = source.split("#[cfg(test)]").next().unwrap_or(source);
    // The credential key name is not a needle: the contract check itself has to
    // build sample entries, so that name legitimately appears in this file.
    let needles = [
        ["reqwest", "::Url"].concat(),
        ["host", "_str"].concat(),
        ["is_ascii", "_hexdigit"].concat(),
    ];
    for needle in needles {
        if production.contains(&needle) {
            return Err("workbuddy_import_classify_second_detector");
        }
    }
    Ok(1)
}

#[cfg(test)]
mod classify_tests {
    use super::*;

    const LOCAL_KEY: &str = "0123456789abcdef0123456789abcdef0123456789abcdef";

    fn entry(vendor: &str, key: &str, url: &str) -> Value {
        serde_json::json!({"vendor": vendor, "apiKey": key, "url": url})
    }

    #[test]
    fn the_marker_alone_is_enough() {
        let marked = serde_json::json!({"_workbuddyProxy": {"managed": true}});
        assert!(carries_proxy_signature(&marked, LOCAL_KEY));
        // A false marker is not a signature, and there is nothing else here.
        let unmarked = serde_json::json!({"_workbuddyProxy": {"managed": false}});
        assert!(!carries_proxy_signature(&unmarked, LOCAL_KEY));
    }

    #[test]
    fn the_legacy_shape_is_recognised_without_a_marker() {
        assert!(carries_proxy_signature(
            &entry("Custom", LOCAL_KEY, "http://127.0.0.1:1/v1"),
            LOCAL_KEY
        ));
        // A generated 48-hex key from an older installation counts too.
        let generated = "a".repeat(48);
        assert!(carries_proxy_signature(
            &entry("Custom", &generated, "http://[::1]:1/r/abc/v1"),
            LOCAL_KEY
        ));
        assert!(carries_proxy_signature(
            &entry(
                "Custom",
                &generated,
                "http://localhost:1/r/abc/v1/chat/completions"
            ),
            LOCAL_KEY
        ));
    }

    #[test]
    fn a_guessed_name_or_a_bare_loopback_url_is_never_enough() {
        // AC033: no detector may guess from a name, an alias or "it is local".
        for value in [
            serde_json::json!({"name": "WorkBuddy Proxy", "url": "http://127.0.0.1:1/v1"}),
            serde_json::json!({"id": "workbuddy", "url": "http://localhost:1/v1"}),
            serde_json::json!({"vendor": "Custom", "url": "http://127.0.0.1:1/v1"}),
            serde_json::json!({"vendor": "Custom", "apiKey": "short", "url": "http://127.0.0.1:1/v1"}),
            serde_json::json!({"vendor": "Other", "apiKey": "a".repeat(48), "url": "http://127.0.0.1:1/v1"}),
        ] {
            assert!(!carries_proxy_signature(&value, LOCAL_KEY), "{value}");
        }
    }

    #[test]
    fn a_remote_or_shaped_url_is_not_a_loopback_proxy_endpoint() {
        let key = "a".repeat(48);
        for url in [
            "https://127.0.0.1:1/v1",
            "http://example.com/v1",
            "http://127.0.0.1:1/other",
            "http://127.0.0.1:1/r//v1",
            "http://127.0.0.1:1/r/a/b/v1",
            "http://user:pass@127.0.0.1:1/v1",
            "http://127.0.0.1:1/v1?x=1",
            "not a url",
        ] {
            assert!(
                !carries_proxy_signature(&entry("Custom", &key, url), LOCAL_KEY),
                "{url}"
            );
        }
    }

    #[test]
    fn the_shared_predicate_is_the_sync_path_predicate() {
        // AC033: the importer and the sync path must agree on every sample, so
        // there cannot be two detectors with different opinions.
        let samples = [
            serde_json::json!({"_workbuddyProxy": {"managed": true}}),
            entry("Custom", LOCAL_KEY, "http://127.0.0.1:1/v1"),
            entry("Custom", &"b".repeat(48), "http://localhost:1/r/x/v1"),
            entry("Other", &"b".repeat(48), "http://localhost:1/v1"),
            serde_json::json!({"name": "proxy"}),
        ];
        for value in samples {
            assert_eq!(
                carries_proxy_signature(&value, LOCAL_KEY),
                crate::workbuddy::managed_signature(&value, LOCAL_KEY),
                "{value}"
            );
        }
    }

    #[test]
    fn a_route_is_only_reported_for_our_own_entries() {
        assert_eq!(
            proxy_route(
                &entry("Custom", LOCAL_KEY, "http://127.0.0.1:1/r/mine/v1"),
                LOCAL_KEY
            )
            .as_deref(),
            Some("mine")
        );
        assert_eq!(
            proxy_route(
                &entry("Other", LOCAL_KEY, "http://127.0.0.1:1/r/mine/v1"),
                LOCAL_KEY
            ),
            None,
            "a foreign entry has no proxy route for us"
        );
    }

    #[test]
    fn the_classify_contract_holds() {
        assert!(classify_contract_check().is_ok());
    }
}

#[cfg(test)]
mod observation_tests {
    use super::*;

    const LOCAL_KEY: &str = "0123456789abcdef0123456789abcdef0123456789abcdef";
    const NOW: i64 = 1_792_108_800_000;

    fn proxy_entry(model: &str) -> Value {
        serde_json::json!({
            "vendor": "Custom",
            "apiKey": LOCAL_KEY,
            "url": "http://127.0.0.1:1/r/route-one/v1",
            "_workbuddyProxy": {"managed": true, "route": "route-one"},
            "model": model,
        })
    }

    #[test]
    fn an_observation_starts_at_the_observation_instant() {
        // AC034: an observation can only speak about the future.
        let observed = observe_current(&proxy_entry("hy3"), LOCAL_KEY, NOW);
        assert_eq!(observed.observed_at_ms, NOW);
        assert_eq!(observed.interval(), (NOW, None), "never back-dated");
        assert!(observed.covers(NOW));
        assert!(observed.covers(NOW + 1));
        assert!(
            !observed.covers(NOW - 1),
            "an earlier instant is not covered"
        );
    }

    #[test]
    fn the_observation_carries_no_credential_and_no_url() {
        // AC034: only the model id, the marker and the route leave the config.
        let observed = observe_current(&proxy_entry("hy3"), LOCAL_KEY, NOW);
        let rendered = format!("{observed:?}");
        assert!(
            !rendered.contains(LOCAL_KEY),
            "the credential must not be copied"
        );
        assert!(
            !rendered.contains("127.0.0.1"),
            "the URL must not be copied"
        );
        assert!(!rendered.contains("http"), "not even the scheme");
        assert_eq!(observed.model_id.as_deref(), Some("hy3"));
        assert_eq!(observed.route_id.as_deref(), Some("route-one"));
        assert!(observed.managed);
    }

    #[test]
    fn a_direct_configuration_proves_nothing_about_history() {
        let direct = serde_json::json!({
            "vendor": "DeepSeek",
            "apiKey": "a-foreign-key",
            "url": "https://api.example.com/v1",
            "model": "deepseek-chat",
        });
        let observed = observe_current(&direct, LOCAL_KEY, NOW);
        assert!(!observed.managed);
        assert!(
            !observed.is_usable(),
            "a direct configuration cannot classify"
        );
        assert_eq!(observed.model_id, None, "and it claims no model of ours");
        assert_eq!(observed.route_id, None);
        // The interval is still honest about when the observation happened.
        assert_eq!(observed.interval(), (NOW, None));
        assert!(
            !observed.covers(NOW),
            "an unusable observation covers nothing"
        );
    }

    #[test]
    fn a_proxy_entry_without_a_model_is_not_usable() {
        let mut entry = proxy_entry("hy3");
        entry.as_object_mut().unwrap().remove("model");
        let observed = observe_current(&entry, LOCAL_KEY, NOW);
        assert!(observed.managed);
        assert!(!observed.is_usable(), "no model id means no attribution");
    }

    #[test]
    fn the_model_id_is_read_from_either_entry_shape() {
        assert_eq!(
            observe_current(&proxy_entry("hy3"), LOCAL_KEY, NOW)
                .model_id
                .as_deref(),
            Some("hy3")
        );
        // A `models` array, of strings or of objects, is the other observed shape.
        let listed = serde_json::json!({
            "_workbuddyProxy": {"managed": true},
            "models": ["glm-5.2", "hy3"],
        });
        assert_eq!(
            observe_current(&listed, LOCAL_KEY, NOW).model_id.as_deref(),
            Some("glm-5.2")
        );
        let objects = serde_json::json!({
            "_workbuddyProxy": {"managed": true},
            "models": [{"id": "hy3"}, {"id": "glm-5.2"}],
        });
        assert_eq!(
            observe_current(&objects, LOCAL_KEY, NOW)
                .model_id
                .as_deref(),
            Some("hy3")
        );
        // An empty declaration is not a model id.
        let empty = serde_json::json!({"_workbuddyProxy": {"managed": true}, "model": ""});
        assert_eq!(observe_current(&empty, LOCAL_KEY, NOW).model_id, None);
    }

    #[test]
    fn an_older_record_is_outside_a_fresh_observation() {
        // AC034: a record that predates the observation is not covered by it, so
        // it must fall through to pending classification instead of being
        // back-filled from today's settings.
        let observed = observe_current(&proxy_entry("hy3"), LOCAL_KEY, NOW);
        let old_record_ms = NOW - 86_400_000;
        assert!(!observed.covers(old_record_ms));
        assert_eq!(observed.interval().0, NOW);
    }

    #[test]
    fn the_observation_contract_holds() {
        assert!(classify_contract_check().is_ok());
    }
}

#[cfg(test)]
mod attribution_tests {
    use super::*;

    fn input(
        proxy_mapping: bool,
        proxy_fact: bool,
        builtin_format: bool,
        confirmed_direct: bool,
    ) -> AttributionInput {
        AttributionInput {
            proxy_mapping,
            proxy_fact,
            builtin_format,
            confirmed_direct,
        }
    }

    #[test]
    fn the_full_table_of_evidence_combinations_is_fixed() {
        // Every combination of the four independent facts, in order. `m` =
        // proxy mapping, `f` = existing proxy fact, `b` = built-in format,
        // `d` = confirmed direct interval.
        let table: [(bool, bool, bool, bool, Attribution); 16] = [
            (false, false, false, false, Attribution::Pending),
            (false, false, false, true, Attribution::Direct),
            (false, false, true, false, Attribution::Builtin),
            (false, false, true, true, Attribution::Builtin),
            (false, true, false, false, Attribution::Proxy),
            (false, true, false, true, Attribution::Proxy),
            (false, true, true, false, Attribution::Proxy),
            (false, true, true, true, Attribution::Proxy),
            (true, false, false, false, Attribution::Proxy),
            (true, false, false, true, Attribution::Proxy),
            (true, false, true, false, Attribution::Proxy),
            (true, false, true, true, Attribution::Proxy),
            (true, true, false, false, Attribution::Proxy),
            (true, true, false, true, Attribution::Proxy),
            (true, true, true, false, Attribution::Proxy),
            (true, true, true, true, Attribution::Proxy),
        ];
        for (mapping, fact, builtin, direct, expected) in table {
            assert_eq!(
                attribute(&input(mapping, fact, builtin, direct)),
                expected,
                "mapping={mapping} fact={fact} builtin={builtin} direct={direct}"
            );
        }
    }

    #[test]
    fn a_custom_local_model_is_not_proxy_by_its_name() {
        // AC035: the classifier never sees a model name at all, so a name that
        // looks local cannot make a record proxy.
        let unnamed = input(false, false, false, false);
        assert_eq!(attribute(&unnamed), Attribution::Pending);
        // Even a confirmed direct interval is only direct, never proxy.
        assert_eq!(
            attribute(&input(false, false, false, true)),
            Attribution::Direct
        );
        // Only durable evidence flips it.
        assert_eq!(
            attribute(&input(true, false, false, false)),
            Attribution::Proxy
        );
    }

    #[test]
    fn the_same_response_model_on_two_routes_is_distinguishable() {
        // AC035: two records that share an upstream/response model must be told
        // apart by their own evidence, not by the model they share.
        let through_proxy = input(true, false, false, false);
        let direct_call = input(false, false, false, true);
        assert_eq!(attribute(&through_proxy), Attribution::Proxy);
        assert_eq!(attribute(&direct_call), Attribution::Direct);
        assert_ne!(attribute(&through_proxy), attribute(&direct_call));
        // And a built-in format on the same shared model is a third outcome.
        assert_eq!(
            attribute(&input(false, false, true, false)),
            Attribution::Builtin
        );
    }

    #[test]
    fn a_known_proxy_model_stays_excluded_without_a_matching_fact() {
        // AC035: the proxy having no record of a call is not evidence that the
        // call went direct, so it must not become an imported row.
        let known_without_fact = input(true, false, false, false);
        assert_eq!(attribute(&known_without_fact), Attribution::Proxy);
        assert!(!attribute(&known_without_fact).is_imported());
        // Even if a direct interval also covers it, proxy still wins.
        let both = input(true, false, false, true);
        assert_eq!(attribute(&both), Attribution::Proxy);
    }

    #[test]
    fn only_builtin_and_direct_write_a_fact_row() {
        assert!(!Attribution::Proxy.is_imported());
        assert!(Attribution::Builtin.is_imported());
        assert!(Attribution::Direct.is_imported());
        assert!(!Attribution::Pending.is_imported());
        assert_eq!(Attribution::Proxy.as_str(), "excluded_proxy");
        assert_eq!(Attribution::Builtin.as_str(), "imported");
        assert_eq!(Attribution::Direct.as_str(), "imported");
        assert_eq!(Attribution::Pending.as_str(), "pending_classification");
        assert_eq!(Attribution::ALL.len(), 4);
    }

    #[test]
    fn the_attribution_contract_holds() {
        assert!(classify_contract_check().is_ok());
    }
}

#[cfg(test)]
mod interval_tests {
    use super::*;

    fn interval(
        id: &str,
        model: &str,
        classification: Attribution,
        from: i64,
        to: Option<i64>,
    ) -> MappingInterval {
        MappingInterval {
            mapping_id: id.into(),
            model_id: model.into(),
            classification,
            valid_from_ms: from,
            valid_to_ms: to,
        }
    }

    #[test]
    fn the_same_alias_is_direct_before_and_proxy_after() {
        // AC036: one alias, two routes, told apart by time alone.
        let intervals = [
            interval(
                "m1",
                "custom-local:hy3",
                Attribution::Direct,
                0,
                Some(1_000),
            ),
            interval("m2", "custom-local:hy3", Attribution::Proxy, 1_000, None),
        ];
        assert_eq!(
            attribute_with_intervals(
                &intervals,
                Some("custom-local:hy3"),
                999,
                &AttributionInput::default()
            ),
            Ok(Attribution::Direct)
        );
        assert_eq!(
            attribute_with_intervals(
                &intervals,
                Some("custom-local:hy3"),
                1_000,
                &AttributionInput::default()
            ),
            Ok(Attribution::Proxy)
        );
        assert_eq!(
            attribute_with_intervals(
                &intervals,
                Some("custom-local:hy3"),
                5_000,
                &AttributionInput::default()
            ),
            Ok(Attribution::Proxy)
        );
    }

    #[test]
    fn the_boundary_instant_belongs_to_the_later_interval() {
        let intervals = [
            interval("m1", "m", Attribution::Direct, 0, Some(100)),
            interval("m2", "m", Attribution::Proxy, 100, None),
        ];
        assert!(
            matches!(interval_for(&intervals, "m", 99), IntervalMatch::One(i) if i.mapping_id == "m1")
        );
        assert!(
            matches!(interval_for(&intervals, "m", 100), IntervalMatch::One(i) if i.mapping_id == "m2")
        );
        assert_eq!(interval_for(&intervals, "m", -1), IntervalMatch::None);
    }

    #[test]
    fn a_later_name_never_rewrites_an_earlier_interval() {
        // AC036: matching is by the id the interval was established for; what
        // the model is called today cannot change the past.
        let intervals = [interval("m1", "old-name", Attribution::Direct, 0, None)];
        assert_eq!(
            attribute_with_intervals(
                &intervals,
                Some("old-name"),
                50,
                &AttributionInput::default()
            ),
            Ok(Attribution::Direct)
        );
        // The same interval simply does not cover a differently named model.
        assert_eq!(
            attribute_with_intervals(
                &intervals,
                Some("new-name"),
                50,
                &AttributionInput::default()
            ),
            Ok(Attribution::Pending)
        );
    }

    #[test]
    fn a_route_rename_or_delete_does_not_touch_history() {
        // The interval carries its own mapping id and model id, so renaming or
        // deleting a route leaves the historical decision intact.
        let before = [interval("m1", "model-a", Attribution::Proxy, 0, None)];
        let renamed = before.clone();
        assert_eq!(
            attribute_with_intervals(&before, Some("model-a"), 10, &AttributionInput::default()),
            attribute_with_intervals(&renamed, Some("model-a"), 10, &AttributionInput::default())
        );
        // A revoked interval is represented by removing it; the record then
        // falls back to the other evidence rather than to a guess.
        assert_eq!(
            attribute_with_intervals(&[], Some("model-a"), 10, &AttributionInput::default()),
            Ok(Attribution::Pending)
        );
        assert_eq!(
            attribute_with_intervals(
                &[],
                Some("model-a"),
                10,
                &AttributionInput {
                    confirmed_direct: true,
                    ..AttributionInput::default()
                }
            ),
            Ok(Attribution::Direct)
        );
    }

    #[test]
    fn overlapping_intervals_are_a_conflict_not_a_preference() {
        let intervals = [
            interval("m1", "m", Attribution::Direct, 0, Some(100)),
            interval("m2", "m", Attribution::Proxy, 50, None),
        ];
        assert_eq!(interval_for(&intervals, "m", 75), IntervalMatch::Conflict);
        assert_eq!(
            attribute_with_intervals(&intervals, Some("m"), 75, &AttributionInput::default()),
            Err(AttributionConflict::OverlappingIntervals)
        );
        assert_eq!(
            AttributionConflict::OverlappingIntervals.as_str(),
            "mapping_conflict"
        );
        // Outside the overlap each interval still answers on its own.
        assert_eq!(
            attribute_with_intervals(&intervals, Some("m"), 25, &AttributionInput::default()),
            Ok(Attribution::Direct)
        );
        assert_eq!(
            attribute_with_intervals(&intervals, Some("m"), 150, &AttributionInput::default()),
            Ok(Attribution::Proxy)
        );
    }

    #[test]
    fn without_an_interval_the_other_evidence_still_applies_in_order() {
        assert_eq!(
            attribute_with_intervals(&[], Some("m"), 10, &AttributionInput::default()),
            Ok(Attribution::Pending)
        );
        assert_eq!(
            attribute_with_intervals(
                &[],
                None,
                10,
                &AttributionInput {
                    proxy_mapping: true,
                    ..AttributionInput::default()
                }
            ),
            Ok(Attribution::Proxy)
        );
        // A record without a model id can still be decided by a proxy fact.
        assert_eq!(
            attribute_with_intervals(
                &[],
                None,
                10,
                &AttributionInput {
                    proxy_fact: true,
                    ..AttributionInput::default()
                }
            ),
            Ok(Attribution::Proxy)
        );
    }

    #[test]
    fn the_interval_contract_holds() {
        assert!(classify_contract_check().is_ok());
    }
}
