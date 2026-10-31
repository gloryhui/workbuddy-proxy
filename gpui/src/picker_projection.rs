//! Exact-input cache for the discovery filter and checked counter.
//!
//! Text-input caret paints must not lowercase and rescan every model ID. The
//! snapshot is bounded by the discovery response and released when it closes.
use std::collections::BTreeSet;

#[derive(Default)]
pub(crate) struct ProjectionCache {
    ids: Vec<String>,
    existing: Vec<String>,
    picked: BTreeSet<usize>,
    query: String,
    only_picked: bool,
    initialized: bool,
    indices: Vec<usize>,
    checked: usize,
    #[cfg(test)]
    computations: usize,
}

impl ProjectionCache {
    /// `query` has the same case-only normalization as the input's old filter;
    /// whitespace remains significant and membership is still case-sensitive.
    pub(crate) fn update<'a>(
        &mut self,
        ids: &[String],
        existing: impl Iterator<Item = &'a str> + Clone,
        picked: &BTreeSet<usize>,
        query: &str,
        only_picked: bool,
    ) {
        if self.initialized
            && self.ids == ids
            && self.picked == *picked
            && self.query == query
            && self.only_picked == only_picked
            && self
                .existing
                .iter()
                .map(String::as_str)
                .eq(existing.clone())
        {
            return;
        }
        self.ids = ids.to_vec();
        self.existing = existing.map(str::to_owned).collect();
        self.picked.clone_from(picked);
        self.query = query.to_owned();
        self.only_picked = only_picked;
        self.initialized = true;
        let existing = self
            .existing
            .iter()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        self.checked = 0;
        self.indices.clear();
        for (index, id) in ids.iter().enumerate() {
            let checked = existing.contains(id.as_str()) || picked.contains(&index);
            self.checked += usize::from(checked);
            if (query.is_empty() || id.to_lowercase().contains(query)) && (!only_picked || checked)
            {
                self.indices.push(index);
            }
        }
        #[cfg(test)]
        {
            self.computations += 1;
        }
    }

    pub(crate) fn indices(&self) -> &[usize] {
        &self.indices
    }

    pub(crate) fn checked(&self) -> usize {
        self.checked
    }

    pub(crate) fn clear(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(values: &[&str]) -> Vec<String> {
        values.iter().map(|id| (*id).to_owned()).collect()
    }

    #[test]
    fn unchanged_discovery_does_not_recompute_on_caret_paints() {
        let values = (0..4096).map(|n| format!("model-{n}")).collect::<Vec<_>>();
        let picked = BTreeSet::from([1, 9]);
        let mut cache = ProjectionCache::default();
        for _ in 0..1000 {
            cache.update(&values, ["model-0"].into_iter(), &picked, "", false);
        }
        assert_eq!(cache.computations, 1);
        assert_eq!(cache.indices().len(), 4096);
        assert_eq!(cache.checked(), 3);
    }

    #[test]
    fn same_count_selection_and_membership_changes_refresh_the_projection() {
        let values = ids(&["A", "B", "C"]);
        let mut cache = ProjectionCache::default();
        cache.update(&values, ["A"].into_iter(), &BTreeSet::from([1]), "", true);
        assert_eq!(cache.indices(), [0, 1]);
        cache.update(&values, ["A"].into_iter(), &BTreeSet::from([2]), "", true);
        assert_eq!(cache.indices(), [0, 2]);
        cache.update(&values, ["B"].into_iter(), &BTreeSet::from([2]), "", true);
        assert_eq!(cache.indices(), [1, 2]);
        assert_eq!(cache.checked(), 2);
        assert_eq!(cache.computations, 3);
    }

    #[test]
    fn same_length_replacement_and_query_changes_never_reuse_stale_rows() {
        let mut cache = ProjectionCache::default();
        let picked = BTreeSet::new();
        cache.update(&ids(&["AA", "B"]), [].into_iter(), &picked, "aa", false);
        assert_eq!(cache.indices(), [0]);
        cache.update(&ids(&["CC", "B"]), [].into_iter(), &picked, "aa", false);
        assert!(cache.indices().is_empty());
        cache.update(&ids(&["CC", "B"]), [].into_iter(), &picked, "b", false);
        assert_eq!(cache.indices(), [1]);
    }

    #[test]
    fn unicode_search_keeps_whitespace_and_exact_membership_semantics() {
        let mut cache = ProjectionCache::default();
        let values = ids(&["模型A", "İD", "ALPHA", "alpha"]);
        cache.update(
            &values,
            ["ALPHA"].into_iter(),
            &BTreeSet::new(),
            "模型a",
            false,
        );
        assert_eq!(cache.indices(), [0]);
        cache.update(
            &values,
            ["ALPHA"].into_iter(),
            &BTreeSet::new(),
            "i\u{307}",
            false,
        );
        assert_eq!(cache.indices(), [1]);
        cache.update(
            &values,
            ["ALPHA"].into_iter(),
            &BTreeSet::new(),
            "alpha",
            true,
        );
        assert_eq!(cache.indices(), [2]);
        cache.update(
            &values,
            ["ALPHA"].into_iter(),
            &BTreeSet::new(),
            " alpha",
            false,
        );
        assert!(cache.indices().is_empty());
    }

    #[test]
    fn closing_releases_the_snapshot_and_reopening_starts_fresh() {
        let mut cache = ProjectionCache::default();
        let values = ids(&["A"]);
        cache.update(&values, ["A"].into_iter(), &BTreeSet::new(), "", false);
        cache.clear();
        assert!(cache.ids.is_empty() && cache.existing.is_empty() && cache.picked.is_empty());
        assert!(cache.indices().is_empty());
        assert_eq!(cache.checked(), 0);
        cache.update(&values, [].into_iter(), &BTreeSet::new(), "", false);
        assert_eq!(cache.indices(), [0]);
        assert_eq!(cache.checked(), 0);
        assert_eq!(cache.computations, 1);
    }
}
