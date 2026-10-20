//! Local UI preferences, independent of provider configuration and report cursors.
use super::*;
use crate::stats_dto::{CaptureMethod, Outcome, SourceScope, UsageStatus};
use serde::{Deserialize, Serialize};
use std::path::Path;

const FILE: &str = "statistics-view.json";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum View {
    Overview,
    #[default]
    Details,
    Summary,
    Settings,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(super) struct Preferences {
    pub view: View,
    pub summary_tab: Tab,
    pub range: RangePreset,
    pub custom_range: (i64, i64),
    pub follow_now: bool,
    pub source: SourceScope,
    pub capture: CaptureMethod,
    pub provider: String,
    pub model: String,
    pub outcome: String,
    pub usage: String,
    pub record_kind: crate::stats_dto::RecordKind,
    pub include_tests: bool,
    pub more_filters: bool,
    pub refresh_seconds: u64,
    pub currency: Currency,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            view: View::Details,
            summary_tab: Tab::Providers,
            range: RangePreset::Week,
            custom_range: (0, 1),
            follow_now: false,
            source: SourceScope::All,
            capture: CaptureMethod::All,
            provider: String::new(),
            model: String::new(),
            outcome: String::new(),
            usage: String::new(),
            record_kind: crate::stats_dto::RecordKind::All,
            include_tests: false,
            more_filters: false,
            refresh_seconds: 5,
            currency: Currency::Usd,
        }
    }
}

impl Preferences {
    fn sanitize(mut self, now: i64) -> Self {
        for id in [&mut self.provider, &mut self.model] {
            if id.chars().count() > crate::stats_dto::MAX_ID_CHARS
                || id.chars().any(char::is_control)
            {
                id.clear();
            }
        }
        if !self.outcome.is_empty() && Outcome::parse(&self.outcome).is_none() {
            self.outcome.clear();
        }
        if !self.usage.is_empty() && UsageStatus::parse(&self.usage).is_none() {
            self.usage.clear();
        }
        if self.summary_tab == Tab::Requests {
            self.summary_tab = Tab::Providers;
        }
        if self.range == RangePreset::Custom {
            let (from, to) = resolve_custom_range(self.custom_range, self.follow_now, now);
            if crate::stats_controller::validate_range(Some(from), Some(to)).is_err() {
                self.range = RangePreset::Week;
                self.follow_now = false;
            }
        }
        if self.source == SourceScope::Builtin && self.capture == CaptureMethod::Proxy {
            self.capture = CaptureMethod::All;
        }
        if self.source == SourceScope::Builtin || self.capture == CaptureMethod::Workbuddy {
            self.include_tests = false;
        }
        if ![0, 5, 10, 30, 60].contains(&self.refresh_seconds) {
            self.refresh_seconds = 5;
        }
        self
    }

    pub fn load(dir: &Path) -> Self {
        let path = dir.join(FILE);
        std::fs::metadata(&path)
            .ok()
            .filter(|meta| meta.len() <= 16 * 1024)
            .and_then(|_| std::fs::read(&path).ok())
            .and_then(|data| serde_json::from_slice::<Self>(&data).ok())
            .unwrap_or_default()
            .sanitize(now_ms())
    }

    fn save(&self, dir: &Path) -> Result<(), String> {
        crate::config::write_json(&dir.join(FILE), self)
    }
}

impl Statistics {
    pub(super) fn restore_preferences(
        &mut self,
        saved: Preferences,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.view = saved.view;
        self.summary_tab = saved.summary_tab;
        self.tab = if self.view == View::Summary {
            self.summary_tab
        } else {
            Tab::Requests
        };
        self.range = saved.range;
        self.custom_range = saved.custom_range;
        self.custom_follow_now = saved.follow_now;
        self.source.select(saved.source);
        let capture = match saved.capture {
            CaptureMethod::Proxy => crate::stats_controller::CaptureChoice::Proxy,
            CaptureMethod::Workbuddy => crate::stats_controller::CaptureChoice::WorkbuddyImport,
            _ => crate::stats_controller::CaptureChoice::All,
        };
        self.capture.select(capture, saved.source);
        self.capture
            .set_include_tests(saved.include_tests, saved.source);
        self.include_tests = self.capture.include_tests();
        self.provider = saved.provider;
        self.model = saved.model;
        self.outcome = saved.outcome;
        self.usage = saved.usage;
        self.record_kind = saved.record_kind;
        self.more_filters_open = saved.more_filters;
        self.refresh_interval = saved.refresh_seconds;
        self.currency = saved.currency;
        self.rebuild_choices(window, cx);
        for (select, value) in [
            (&self.provider_select, self.provider.clone()),
            (&self.model_select, self.model.clone()),
            (&self.outcome_select, self.outcome.clone()),
            (&self.usage_select, self.usage.clone()),
            (&self.currency_select, self.currency.as_str().to_owned()),
        ] {
            select.update(cx, |state, cx| state.set_selected_value(&value, window, cx));
        }
    }

    pub(super) fn persist_preferences(&mut self) {
        let saved = Preferences {
            view: self.view,
            summary_tab: self.summary_tab,
            range: self.range,
            custom_range: self.custom_range,
            follow_now: self.custom_follow_now,
            source: self.source_scope(),
            capture: crate::stats_dto::CaptureMethod::parse(
                self.capture.effective(self.source_scope()).as_str(),
            )
            .unwrap_or_default(),
            provider: self.provider.clone(),
            model: self.model.clone(),
            outcome: self.outcome.clone(),
            usage: self.usage.clone(),
            record_kind: self.record_kind,
            include_tests: self.capture.include_tests(),
            more_filters: self.more_filters_open,
            refresh_seconds: self.refresh_interval,
            currency: self.currency,
        };
        if saved != self.saved_preferences && !self.preferences_error {
            match saved.save(&self.preferences_dir) {
                Ok(()) => self.saved_preferences = saved,
                // Keep the selection in memory and report the failure, without
                // hammering a read-only disk every timer tick.
                Err(_) => self.preferences_error = true,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{FILE, Preferences, View};
    use crate::{
        statistics::{RangePreset, Tab},
        stats_dto::{CaptureMethod, SourceScope},
    };

    #[test]
    fn reopens_view_and_filters_without_saving_cursors_or_provider_secrets() {
        let dir = tempfile::tempdir().unwrap();
        let saved = Preferences {
            view: View::Summary,
            summary_tab: Tab::Models,
            source: SourceScope::Builtin,
            range: RangePreset::Custom,
            custom_range: (1000, 2000),
            model: "deepseek".into(),
            provider: "workbuddy".into(),
            usage: "complete".into(),
            refresh_seconds: 30,
            ..Default::default()
        };
        saved.save(dir.path()).unwrap();
        assert_eq!(Preferences::load(dir.path()), saved);
        // An explicit earlier choice still wins over the new default range.
        let mut today = saved.clone();
        today.range = RangePreset::Today;
        today.save(dir.path()).unwrap();
        assert_eq!(Preferences::load(dir.path()), today);
        let json = std::fs::read_to_string(dir.path().join(FILE)).unwrap();
        for key in ["cursor", "api_key", "generation", "as_of"] {
            assert!(!json.contains(key));
        }
        // An atomic rewrite must also work on Windows when the file exists.
        saved.save(dir.path()).unwrap();
        assert_eq!(Preferences::load(dir.path()), saved);
    }

    #[test]
    fn corrupt_or_future_preferences_do_not_prevent_startup() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(Preferences::load(dir.path()).range, RangePreset::Week);
        for content in ["broken", "{\"view\":\"future_view\"}"] {
            std::fs::write(dir.path().join(FILE), content).unwrap();
            assert_eq!(Preferences::load(dir.path()), Preferences::default());
        }
        std::fs::write(dir.path().join(FILE), "x".repeat(17000)).unwrap();
        assert_eq!(Preferences::load(dir.path()), Preferences::default());
    }

    #[test]
    fn invalid_ranges_and_incompatible_hidden_conditions_are_reconciled() {
        let saved = Preferences {
            range: RangePreset::Custom,
            custom_range: (2000, 1000),
            follow_now: false,
            source: SourceScope::Builtin,
            capture: CaptureMethod::Proxy,
            include_tests: true,
            outcome: "bogus".into(),
            model: "bad\nmodel".into(),
            refresh_seconds: u64::MAX,
            ..Default::default()
        }
        .sanitize(3000);
        assert_eq!(saved.range, RangePreset::Week);
        assert_eq!(saved.capture, CaptureMethod::All);
        assert!(!saved.include_tests);
        assert!(saved.outcome.is_empty() && saved.model.is_empty());
        assert_eq!(saved.refresh_seconds, 5);
        let expired = Preferences {
            range: RangePreset::Custom,
            custom_range: (1, 10),
            follow_now: true,
            ..Default::default()
        }
        .sanitize(400 * 86_400_000);
        assert_eq!(expired.range, RangePreset::Week);
    }
}
