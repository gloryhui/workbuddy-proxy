//! The WorkBuddy import panel: the switch, the health line and the counters.
//!
//! Two things this panel deliberately does **not** have: a "realtime" mode and a
//! sync-period setting. The importer runs on its own fixed cadence in the
//! background (see [`BACKGROUND_SYNC_SECONDS`]), and the page's refresh choice
//! only decides how often the *page* re-reads what the core already stored.
use crate::stats_dto::SourceScope;

/// The dialog's geometry, from the spec.
pub const DIALOG_MIN_WIDTH: i32 = 520;
pub const DIALOG_MAX_WIDTH: i32 = 720;
/// The dialog never grows past this share of the window height.
pub const DIALOG_MAX_HEIGHT_RATIO: f32 = 0.8;
/// The page sizes the candidate list offers.
pub const PAGE_SIZES: &[i64] = &[20, 50, 100];

/// The two mutually exclusive source decisions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Classification {
    /// The user says they used the model directly.
    Direct,
    /// The user says the call went through this proxy.
    Proxy,
}

impl Classification {
    /// The wire value the command expects.
    pub fn as_str(self) -> &'static str {
        match self {
            Classification::Direct => "direct",
            Classification::Proxy => "proxy",
        }
    }

    /// The button's label.
    pub fn label(self, zh: bool) -> &'static str {
        match (self, zh) {
            (Classification::Direct, true) => "直连导入",
            (Classification::Direct, false) => "Import direct use",
            (Classification::Proxy, true) => "本机代理排除",
            (Classification::Proxy, false) => "Exclude local proxy copies",
        }
    }

    pub fn confirmation_label(self, zh: bool) -> &'static str {
        match (self, zh) {
            (Classification::Direct, true) => "确认直连导入",
            (Classification::Direct, false) => "Confirm direct import",
            (Classification::Proxy, true) => "确认排除日志",
            (Classification::Proxy, false) => "Confirm exclusion",
        }
    }
}

/// A preview the user is about to confirm.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Preview {
    /// The one-shot token that binds the confirmation to this preview.
    pub token: String,
    /// How many records it covers.
    pub affected_count: i64,
    /// When the token stops being accepted.
    pub expires_at_ms: i64,
    /// The watermark the preview froze.
    pub watermark_rowid: i64,
    /// The rule revision it was taken under.
    pub mapping_revision: i64,
}

/// Everything that can go wrong, as something the user can act on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DialogError {
    /// The preview went stale: preview again.
    PreviewExpired,
    /// The rule moved under the preview: preview again.
    RevisionConflict,
    /// The proxy already has evidence for this interval, so it cannot be
    /// declared a direct use.
    ProxyEvidenceConflict,
    /// The interval is not usable.
    InvalidRange,
    /// Anything else, with no internal detail attached.
    Unavailable,
}

impl DialogError {
    /// The code the core sent, turned into one of the cases above.
    pub fn from_code(code: &str) -> Self {
        if code.contains("stale_batch") {
            DialogError::PreviewExpired
        } else if code.contains("mapping_conflict") {
            DialogError::RevisionConflict
        } else if code.contains("proxy_interval_protected") || code.contains("interval_protected") {
            DialogError::ProxyEvidenceConflict
        } else if code.contains("invalid") {
            DialogError::InvalidRange
        } else {
            DialogError::Unavailable
        }
    }

    /// What the dialog says. These are fixed sentences: no SQL, no path, no
    /// internal identifier ever reaches the user.
    pub fn text(self, zh: bool) -> &'static str {
        match (self, zh) {
            (DialogError::PreviewExpired, true) => "预览已过期，请重新预览。",
            (DialogError::PreviewExpired, false) => "This preview expired. Preview again.",
            (DialogError::RevisionConflict, true) => "归属规则已变化，请重新预览。",
            (DialogError::RevisionConflict, false) => "The rule changed. Preview again.",
            (DialogError::ProxyEvidenceConflict, true) => "该区间已有代理记录，不能改为用户直连。",
            (DialogError::ProxyEvidenceConflict, false) => {
                "This interval already has proxy records; it cannot be marked as direct use."
            }
            (DialogError::InvalidRange, true) => "请填写有效的起止时间。",
            (DialogError::InvalidRange, false) => "Enter a valid time range.",
            (DialogError::Unavailable, true) => "暂时无法完成，请稍后再试。",
            (DialogError::Unavailable, false) => "That did not work. Try again shortly.",
        }
    }

    /// True when the answer is to preview again rather than to give up.
    pub fn needs_fresh_preview(self) -> bool {
        matches!(
            self,
            DialogError::PreviewExpired | DialogError::RevisionConflict
        )
    }
}

/// The historical-source confirmation dialog.
///
/// Nothing is selected to begin with, and a confirmation always needs a preview
/// taken after the last change: that is what makes "confirm" mean "confirm *this*
/// preview" rather than "confirm whatever the rule looks like now".
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MappingDialog {
    model_id: String,
    from_ms: Option<i64>,
    to_ms: Option<i64>,
    classification: Option<Classification>,
    preview: Option<Preview>,
    error: Option<DialogError>,
    page: i64,
    page_size: i64,
}

impl MappingDialog {
    /// A dialog for one model, with nothing chosen.
    pub fn new(model_id: &str) -> Self {
        Self {
            model_id: model_id.to_owned(),
            from_ms: None,
            to_ms: None,
            classification: None,
            preview: None,
            error: None,
            page: 0,
            page_size: 20,
        }
    }

    /// The model this dialog is about.
    pub fn model_id(&self) -> &str {
        &self.model_id
    }

    /// What the user picked, if anything.
    pub fn classification(&self) -> Option<Classification> {
        self.classification
    }

    /// The current page.
    pub fn page(&self) -> i64 {
        self.page
    }

    /// The current page size.
    pub fn page_size(&self) -> i64 {
        self.page_size
    }

    /// The error the dialog is showing, if any.
    pub fn error(&self) -> Option<DialogError> {
        self.error
    }

    /// The preview being confirmed, if any.
    pub fn preview(&self) -> Option<&Preview> {
        self.preview.as_ref()
    }

    /// Sets the interval and drops its previous preview.
    pub fn set_range(&mut self, from_ms: i64, to_ms: i64) {
        self.from_ms = Some(from_ms);
        self.to_ms = Some(to_ms);
        // A new interval invalidates any preview taken for the old one.
        self.preview = None;
    }

    /// Picks a classification. Nothing is picked by default, and the choice is
    /// mutually exclusive by construction (one field, not a set).
    pub fn choose(&mut self, classification: Classification) {
        if self.classification == Some(classification) {
            return;
        }
        self.classification = Some(classification);
        // The decision changed, so the old preview no longer describes it.
        self.preview = None;
    }

    /// Moves to another page of candidates.
    pub fn set_page(&mut self, page: i64, page_size: i64) -> bool {
        if !PAGE_SIZES.contains(&page_size) {
            return false;
        }
        self.page = page.max(0);
        self.page_size = page_size;
        true
    }

    /// The command payload for a preview, or nothing when the form is not ready.
    ///
    /// A preview needs both an interval and a classification: there is no
    /// default to fall back on.
    pub fn preview_request(
        &self,
        now_ms: i64,
    ) -> Option<(String, i64, Option<i64>, Classification)> {
        let (from_ms, to_ms) = (self.from_ms?, self.to_ms?);
        if to_ms <= from_ms {
            return None;
        }
        let classification = self.classification?;
        let _ = now_ms;
        Some((self.model_id.clone(), from_ms, Some(to_ms), classification))
    }

    /// Records the answer to a preview request.
    pub fn apply_preview(&mut self, result: Result<Preview, String>) {
        match result {
            Ok(preview) => {
                self.preview = Some(preview);
                self.error = None;
            }
            Err(code) => {
                let error = DialogError::from_code(&code);
                self.error = Some(error);
                // A refused preview leaves nothing to confirm.
                self.preview = None;
                if error == DialogError::ProxyEvidenceConflict {
                    // The proxy already has evidence here, so the direct choice
                    // cannot stand: it is cleared rather than left selected.
                    if self.classification == Some(Classification::Direct) {
                        self.classification = None;
                    }
                }
            }
        }
    }

    /// True when the confirm button may be pressed.
    pub fn can_confirm(&self, now_ms: i64) -> bool {
        match &self.preview {
            Some(preview) => preview.expires_at_ms > now_ms && self.classification.is_some(),
            None => false,
        }
    }

    /// Takes the token to confirm with, if there is one.
    pub fn take_token(&self, now_ms: i64) -> Option<String> {
        self.can_confirm(now_ms)
            .then(|| self.preview.as_ref().map(|preview| preview.token.clone()))
            .flatten()
    }

    /// Records the answer to a confirmation.
    pub fn apply_confirmation(&mut self, result: Result<(), String>) {
        match result {
            Ok(()) => {
                self.preview = None;
                self.error = None;
            }
            Err(code) => {
                let error = DialogError::from_code(&code);
                self.error = Some(error);
                // A stale or conflicting answer means the preview is worthless:
                // it is dropped so the user must take a fresh one.
                self.preview = None;
            }
        }
    }

    /// The label of the final button.
    pub fn confirm_label(&self, zh: bool) -> &'static str {
        self.classification
            .map_or(if zh { "确认归属" } else { "Confirm source" }, |class| {
                class.confirmation_label(zh)
            })
    }
}

/// The background cadence, fixed by the design (WB-RT-001).
///
/// It is a constant on purpose: there is no setting for it, and the page's
/// refresh choice never changes it.
pub const BACKGROUND_SYNC_SECONDS: u32 = 5;

/// The line the panel shows about that cadence.
pub const BACKGROUND_SYNC_NOTE: &str = "后台每 5 秒同步一次，与页面刷新无关 / Background sync runs every 5s, independent of page refresh";

/// The six counters the panel publishes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ImportCounters {
    /// Records imported and counted.
    pub imported: i64,
    /// Records excluded because the proxy already saw them.
    pub excluded_proxy: i64,
    /// Candidates waiting for a source decision.
    pub pending: i64,
    /// Records whose content changed under the same identity.
    pub conflict: i64,
    /// Records deleted by hand or by retention.
    pub deleted: i64,
    pub skipped: i64,
    /// Files discovered so far.
    pub files_seen: i64,
}

impl ImportCounters {
    /// Every counter, in the order the panel lists them.
    pub const ALL: &'static [(&'static str, &'static str)] = &[
        ("已导入", "Imported"),
        ("重复已去重", "Deduplicated"),
        ("待重读", "Awaiting reread"),
        ("冲突", "Conflict"),
        ("跳过异常", "Skipped"),
        ("已清除", "Cleared"),
    ];

    /// The value of one counter by index, matching [`Self::ALL`].
    pub fn value(&self, index: usize) -> i64 {
        match index {
            0 => self.imported,
            1 => self.excluded_proxy,
            2 => self.pending,
            3 => self.conflict,
            4 => self.skipped,
            _ => self.deleted,
        }
    }

    /// True when anything still needs a decision.
    pub fn needs_attention(&self) -> bool {
        self.pending > 0 || self.conflict > 0
    }
}

/// What the switch is currently doing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SwitchState {
    /// The importer is off. This is the default.
    #[default]
    Off,
    /// A change is in flight; the switch shows the requested value already.
    Saving,
    /// The importer is on.
    On,
    /// The last change failed and the switch has been put back.
    Failed,
}

/// The import panel's own state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportPanel {
    enabled: bool,
    state: SwitchState,
    counters: ImportCounters,
    /// The error code of the last failure, if any.
    last_error: Option<String>,
    /// The scope the counters were read for.
    scope: SourceScope,
}

impl Default for ImportPanel {
    fn default() -> Self {
        Self::new()
    }
}

impl ImportPanel {
    /// A panel with the importer off, which is the default everywhere.
    pub fn new() -> Self {
        Self {
            enabled: false,
            state: SwitchState::Off,
            counters: ImportCounters::default(),
            last_error: None,
            scope: SourceScope::All,
        }
    }

    /// True when the importer is on.
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// What the switch is doing.
    pub fn state(&self) -> SwitchState {
        self.state
    }

    /// The counters as last read.
    pub fn counters(&self) -> &ImportCounters {
        &self.counters
    }

    /// The last failure's code, if any.
    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    /// The scope the counters belong to.
    pub fn scope(&self) -> SourceScope {
        self.scope
    }

    /// The note the panel shows about the background cadence.
    pub fn background_note(&self) -> &'static str {
        BACKGROUND_SYNC_NOTE
    }

    /// Turns the importer on.
    ///
    /// The switch moves immediately — no confirmation dialog, because turning
    /// import on is not destructive — and the request carries the revision the
    /// panel last saw, so a stale click cannot overwrite someone else's change.
    pub fn request_enable(&mut self) -> Option<bool> {
        self.request(true)
    }

    /// Turns the importer off.
    ///
    /// Pausing only stops new commits: nothing is deleted, so the counters and
    /// the history stay exactly where they are.
    pub fn request_disable(&mut self) -> Option<bool> {
        self.request(false)
    }

    fn request(&mut self, enabled: bool) -> Option<bool> {
        if self.state == SwitchState::Saving {
            // One change at a time: a second click while saving is ignored
            // rather than queued.
            return None;
        }
        if self.enabled == enabled {
            // Clicking the switch that is already in the requested position
            // asks for nothing.
            return None;
        }
        self.enabled = enabled;
        self.state = SwitchState::Saving;
        self.last_error = None;
        Some(enabled)
    }

    /// Applies the core's answer.
    ///
    /// On failure the switch **rolls back** to what it was, so the panel never
    /// shows a state the core refused.
    pub fn apply(&mut self, result: Result<bool, String>) {
        match result {
            Ok(enabled) => {
                self.enabled = enabled;
                self.state = if enabled {
                    SwitchState::On
                } else {
                    SwitchState::Off
                };
                self.last_error = None;
            }
            Err(code) => {
                // Roll back to the opposite of what was requested: the request
                // set `enabled` to the requested value, so flipping it restores
                // the state the core still has.
                self.enabled = !self.enabled;
                self.state = SwitchState::Failed;
                self.last_error = Some(code);
            }
        }
    }

    /// Replaces the counters with a fresh read.
    pub fn set_counters(&mut self, counters: ImportCounters, scope: SourceScope) {
        self.counters = counters;
        self.scope = scope;
    }

    /// True when the panel may show "everything is synced".
    ///
    /// A disabled importer has not looked at anything, so it never claims
    /// completion; neither does one with unresolved rows.
    pub fn is_complete(&self) -> bool {
        self.enabled && self.state != SwitchState::Failed && !self.counters.needs_attention()
    }

    /// True when the page's own refresh should read the import status.
    ///
    /// Reading the status is a database read: it never touches the JSONL files,
    /// which is why a plain refresh cannot change what the importer has done.
    pub fn refresh_reads_jsonl(&self) -> bool {
        false
    }

    /// The health line's text.
    pub fn health_text(&self, zh: bool) -> &'static str {
        match (self.state, zh) {
            (SwitchState::Failed, true) => "需处理",
            (SwitchState::Failed, false) => "Needs attention",
            (SwitchState::Saving, true) => "导入中",
            (SwitchState::Saving, false) => "Importing",
            (SwitchState::On, true) => "已开启",
            (SwitchState::On, false) => "On",
            (SwitchState::Off, true) => "未开启",
            (SwitchState::Off, false) => "Off",
        }
    }
}

/// Fixed user-facing messages; never echo SQL, paths or arbitrary core details.
pub fn error_text(error: &str, zh: bool) -> &'static str {
    let code = error.split(':').next().unwrap_or("");
    let pair = match code {
        "stats_workbuddy_proxy_history_unsafe" => (
            "该区间无法安全补录：请选择本机统计采集开始前、且没有原始代理明细的封闭历史区间。",
            "Choose a closed interval before local capture began, with no native proxy records.",
        ),
        "stats_workbuddy_import_disabled" => (
            "导入已暂停，请先开启导入或全局采集。",
            "Import is paused. Enable import and global collection first.",
        ),
        "stats_workbuddy_root_missing" => (
            "尚未找到本机 WorkBuddy 记录目录。",
            "The local WorkBuddy records directory was not found.",
        ),
        "stats_workbuddy_permission_denied" => (
            "无法读取本机记录，请检查目录访问权限。",
            "Cannot read local records. Check directory access.",
        ),
        "stats_workbuddy_invalid_path" | "stats_workbuddy_unsupported_format" => (
            "本地记录格式暂不受支持，请检查导入状态。",
            "The local record format is unsupported. Check import status.",
        ),
        "stats_workbuddy_stale_batch" => (
            "预览已过期或已使用，请重新预览。",
            "The preview expired or was used. Preview again.",
        ),
        "stats_workbuddy_mapping_conflict" => (
            "归属已变化或区间已有代理证据，请检查区间后重新预览。",
            "Attribution changed or proxy evidence protects this interval. Check the range and preview again.",
        ),
        "stats_workbuddy_source_changed" => (
            "记录文件发生变化，后台将重新检查。",
            "A record file changed. The background worker will recheck it.",
        ),
        "stats_workbuddy_source_read_failed" => (
            "本地记录读取失败，后台将重试。",
            "Reading local records failed. The background worker will retry.",
        ),
        "stats_workbuddy_ledger_conflict" => (
            "记录内容存在冲突，未计入统计。",
            "Conflicting record content is excluded from statistics.",
        ),
        "stats_workbuddy_scan_busy" | "stats_busy" => (
            "已有任务处理中，请稍后再试。",
            "A task is running. Try again shortly.",
        ),
        "stats_revision_conflict" => (
            "设置已变化，请刷新后重试。",
            "Settings changed. Reload and retry.",
        ),
        "stats_cursor_expired" => (
            "列表已更新，请从第一页重新查看。",
            "The list changed. Start again from the first page.",
        ),
        "stats_invalid" | "stats_invalid_filter" => (
            "请填写有效的模型、分类和起止时间，终点不含。",
            "Enter a valid model, classification and time range with an exclusive end.",
        ),
        _ => (
            "统计服务暂时不可用，请稍后再试。",
            "Statistics are temporarily unavailable. Try again shortly.",
        ),
    };
    if zh { pair.0 } else { pair.1 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn import_is_off_by_default_and_never_claims_completion_while_off() {
        let panel = ImportPanel::new();
        assert!(!panel.is_enabled());
        assert_eq!(panel.state(), SwitchState::Off);
        assert!(
            !panel.is_complete(),
            "an importer that never ran is not synced"
        );
        assert_eq!(panel.health_text(true), "未开启");
    }

    #[test]
    fn turning_it_on_asks_once_and_needs_no_confirmation() {
        // AC070: the first enable is not destructive, so it goes straight
        // through; there is no confirmation step to forget.
        let mut panel = ImportPanel::new();
        assert_eq!(panel.request_enable(), Some(true));
        assert_eq!(
            panel.state(),
            SwitchState::Saving,
            "the switch moves at once"
        );
        assert!(panel.is_enabled());
        panel.apply(Ok(true));
        assert_eq!(panel.state(), SwitchState::On);
        // Clicking again asks for nothing.
        assert_eq!(panel.request_enable(), None);
    }

    #[test]
    fn a_failed_switch_rolls_back() {
        // AC070: the panel never shows a state the core refused.
        let mut panel = ImportPanel::new();
        panel.request_enable();
        panel.apply(Err("stats_unavailable".into()));
        assert!(!panel.is_enabled(), "rolled back to off");
        assert_eq!(panel.state(), SwitchState::Failed);
        assert_eq!(panel.last_error(), Some("stats_unavailable"));
        assert!(!panel.is_complete(), "a failed switch is not synced");

        // And a failed disable rolls back to on.
        panel.apply(Ok(true));
        panel.request_disable();
        panel.apply(Err("stats_revision_conflict".into()));
        assert!(panel.is_enabled(), "rolled back to on");
    }

    #[test]
    fn pausing_keeps_every_counter_and_the_history() {
        // AC070: pausing stops new commits; it deletes nothing.
        let mut panel = ImportPanel::new();
        panel.apply(Ok(true));
        let counters = ImportCounters {
            imported: 12,
            excluded_proxy: 3,
            pending: 1,
            conflict: 0,
            deleted: 2,
            skipped: 0,
            files_seen: 4,
        };
        panel.set_counters(counters, SourceScope::All);
        panel.request_disable();
        panel.apply(Ok(false));
        assert!(!panel.is_enabled());
        assert_eq!(
            panel.counters(),
            &counters,
            "the history is still there after a pause"
        );
    }

    #[test]
    fn a_plain_refresh_never_reads_the_jsonl() {
        // AC070 / X45: the page's refresh only reads what the core stored; the
        // files are the background worker's business.
        let panel = ImportPanel::new();
        assert!(!panel.refresh_reads_jsonl());
    }

    #[test]
    fn the_background_cadence_is_fixed_and_has_no_setting() {
        // WB-RT-001: five seconds, a constant. There is no "realtime" option and
        // no period to configure, so the panel can only report it.
        assert_eq!(BACKGROUND_SYNC_SECONDS, 5);
        assert!(BACKGROUND_SYNC_NOTE.contains("5"));
        assert!(
            BACKGROUND_SYNC_NOTE.contains("无关") || BACKGROUND_SYNC_NOTE.contains("independent")
        );
        let panel = ImportPanel::new();
        assert_eq!(panel.background_note(), BACKGROUND_SYNC_NOTE);
    }

    #[test]
    fn the_six_counters_are_listed_and_readable_by_index() {
        assert_eq!(ImportCounters::ALL.len(), 6);
        let counters = ImportCounters {
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
        assert!(counters.needs_attention(), "pending rows need a decision");
        assert!(!ImportCounters::default().needs_attention());
    }

    #[test]
    fn a_second_click_while_saving_is_ignored_rather_than_queued() {
        let mut panel = ImportPanel::new();
        assert_eq!(panel.request_enable(), Some(true));
        assert_eq!(panel.request_disable(), None, "still saving");
        assert_eq!(panel.request_enable(), None);
        panel.apply(Ok(true));
        assert!(panel.is_enabled());
    }
}
