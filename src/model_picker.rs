use serde_json::{Value, json};

// Discovery is bounded separately from the number of configured models.
pub const MAX_DISCOVERED_MODELS: usize = 4096;

#[derive(Default)]
pub struct ModelPicker {
    pub models: Vec<Candidate>,
}

pub struct Candidate {
    pub id: String,
    pub existing: bool,
    pub selected: bool,
}

impl ModelPicker {
    pub fn new(ids: Vec<String>, draft: &[Value]) -> Self {
        Self {
            models: ids
                .into_iter()
                .map(|id| {
                    let existing = draft.iter().any(|m| m["model"].as_str() == Some(&id));
                    Candidate {
                        id,
                        existing,
                        selected: existing,
                    }
                })
                .collect(),
        }
    }

    pub fn select(&mut self, row: usize, selected: bool) {
        if let Some(model) = self.models.get_mut(row)
            && !model.existing
        {
            model.selected = selected;
        }
    }

    // Validate the entire addition before mutating the draft, retaining names,
    // stable route IDs and any manually added models absent from discovery.
    pub fn apply(&self, draft: &mut Vec<Value>, other: usize) -> Result<usize, String> {
        let missing: Vec<_> = self
            .models
            .iter()
            .filter(|m| m.selected && !draft.iter().any(|row| row["model"].as_str() == Some(&m.id)))
            .collect();
        if draft.len() + missing.len() + other > crate::config::MAX_MODELS {
            return Err("At most 256 models".into());
        }
        for model in &missing {
            draft.push(json!({"name":model.id,"model":model.id}));
        }
        Ok(missing.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovery_and_cancel_leave_the_draft_untouched() {
        let draft = vec![
            json!({"id":"stable", "name":"Alias", "model":"saved"}),
            json!({"model":"manual"}),
        ];
        let original = draft.clone();
        let mut picker = ModelPicker::new(vec!["saved".into(), "new".into()], &draft);
        assert!(picker.models[0].existing && picker.models[0].selected);
        assert!(!picker.models[1].selected);
        picker.select(1, true);
        picker.select(0, false);
        assert!(picker.models[0].selected);
        drop(picker);
        assert_eq!(draft, original);
    }

    #[test]
    fn selection_across_filters_preserves_existing_rows_and_skips_duplicates() {
        let mut draft = vec![
            json!({"id":"stable", "name":"Alias", "model":"saved"}),
            json!({"model":"manual"}),
        ];
        let original = draft.clone();
        let mut picker = ModelPicker::new(
            vec!["saved".into(), "a".into(), "b".into(), "c".into()],
            &draft,
        );
        // Select all, invert a filtered subset, then clear a different subset.
        for row in 0..4 {
            picker.select(row, true);
        }
        for row in [0, 2] {
            picker.select(row, !picker.models[row].selected);
        }
        picker.select(3, false);
        assert_eq!(picker.apply(&mut draft, 0).unwrap(), 1);
        assert_eq!(&draft[..2], &original);
        assert_eq!(draft[2], json!({"name":"a","model":"a"}));
        assert_eq!(picker.apply(&mut draft, 0).unwrap(), 0);
    }

    #[test]
    fn large_catalog_can_select_a_subset_and_limit_failure_is_atomic() {
        let mut draft = vec![json!({"model":"saved"})];
        let mut picker = ModelPicker::new((0..300).map(|i| format!("m-{i}")).collect(), &draft);
        picker.select(290, true);
        picker.select(299, true);
        let before = draft.clone();
        assert!(picker.apply(&mut draft, 254).is_err());
        assert_eq!(draft, before);
        picker.select(299, false);
        assert_eq!(picker.apply(&mut draft, 254).unwrap(), 1);
        assert_eq!(draft[1]["model"], "m-290");
    }
}
