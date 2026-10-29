//! Window-local feedback for a confirmed provider deletion, never persisted.
use super::Config;
use std::collections::BTreeSet;

#[derive(Default)]
pub(super) struct DeleteFeedback {
    submitted: Option<String>,
    survivor: Option<String>,
}

impl DeleteFeedback {
    pub(super) fn begin(&mut self, target: &str) {
        self.submitted = Some(target.to_owned());
        self.survivor = None;
    }

    pub(super) fn clear(&mut self) {
        self.submitted = None;
        self.survivor = None;
    }

    pub(super) fn is_for(&self, id: &str) -> bool {
        self.survivor.as_deref() == Some(id)
    }

    pub(super) fn committed(&mut self, before: &Config, after: &Config, survivor: &str) {
        let target = self.submitted.take();
        let before = before
            .providers
            .iter()
            .map(|p| p.id.as_str())
            .collect::<Vec<_>>();
        let after = after
            .providers
            .iter()
            .map(|p| p.id.as_str())
            .collect::<Vec<_>>();
        self.survivor = target
            .filter(|target| confirmed_removal(&before, &after, target, survivor))
            .map(|_| survivor.to_owned());
    }

    pub(super) fn reconcile(&mut self, cfg: &Config) {
        if self
            .survivor
            .as_ref()
            .is_some_and(|id| !cfg.providers.iter().any(|p| &p.id == id))
        {
            self.survivor = None;
        }
    }
}

fn confirmed_removal(before: &[&str], after: &[&str], target: &str, survivor: &str) -> bool {
    let before_set = before.iter().copied().collect::<BTreeSet<_>>();
    let after_set = after.iter().copied().collect::<BTreeSet<_>>();
    !target.is_empty()
        && before_set.len() == before.len()
        && after_set.len() == after.len()
        && before_set.contains(target)
        && !after_set.contains(target)
        && after_set.contains(survivor)
        && before_set.len() == after_set.len() + 1
        && before_set
            .into_iter()
            .filter(|id| *id != target)
            .eq(after_set)
}

#[cfg(test)]
mod tests {
    use super::{DeleteFeedback, confirmed_removal};

    #[test]
    fn only_the_submitted_target_may_disappear() {
        assert!(confirmed_removal(&["a", "b"], &["b"], "a", "b"));
        assert!(!confirmed_removal(&["a", "b"], &["a"], "a", "a"));
        assert!(!confirmed_removal(&["a", "b"], &["a", "b"], "a", "b"));
        assert!(!confirmed_removal(&["a", "b"], &["c"], "a", "c"));
        assert!(!confirmed_removal(&["a", "b", "c"], &["b"], "a", "b"));
    }

    #[test]
    fn feedback_requires_a_real_surviving_provider() {
        assert!(!confirmed_removal(&["a"], &[], "a", ""));
        assert!(!confirmed_removal(&["a", "b"], &["b"], "a", "a"));
        assert!(!confirmed_removal(&["a", "b"], &["b"], "missing", "b"));
        assert!(!confirmed_removal(&["a", "a", "b"], &["b"], "a", "b"));
        assert!(confirmed_removal(&["c", "a", "b"], &["b", "c"], "a", "c"));
    }

    #[test]
    fn another_attempt_or_context_clears_the_previous_receipt() {
        let mut feedback = DeleteFeedback {
            submitted: None,
            survivor: Some("b".into()),
        };
        assert!(feedback.is_for("b"));
        assert!(!feedback.is_for("a"));
        feedback.begin("c");
        assert!(!feedback.is_for("b"));
        assert_eq!(feedback.submitted.as_deref(), Some("c"));
        feedback.clear();
        assert!(feedback.submitted.is_none());
        assert!(!feedback.is_for("b"));
    }
}
