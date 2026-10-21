//! Page navigation keeps cursor paging fast and seeks only unvisited pages.
use std::collections::BTreeMap;

pub(super) const PAGE_SIZES: [i64; 4] = [10, 20, 50, 100];

pub(super) fn pages(total: i64, size: i64) -> usize {
    ((total.max(0) / size) + i64::from(total.max(0) % size != 0)).max(1) as usize
}

pub(super) fn parse_page(text: &str, total_pages: usize) -> Option<usize> {
    text.trim()
        .parse::<usize>()
        .ok()
        .filter(|page| (1..=total_pages).contains(page))
}

#[derive(Clone)]
struct Start<C> {
    cursor: Option<C>,
    seek: Option<usize>,
}

pub(super) struct Pager<C> {
    page: usize,
    start: Start<C>,
    next: Option<C>,
    known: BTreeMap<usize, Start<C>>,
    pending: Option<(usize, Start<C>)>,
}

impl<C> Default for Pager<C> {
    fn default() -> Self {
        Self {
            page: 1,
            start: Start {
                cursor: None,
                seek: None,
            },
            next: None,
            known: BTreeMap::new(),
            pending: None,
        }
    }
}

impl<C: Clone> Pager<C> {
    pub fn reset(&mut self) {
        *self = Self::default();
    }
    pub fn page_number(&self) -> usize {
        self.page
    }
    pub fn can_go_back(&self) -> bool {
        self.page > 1
    }
    pub fn can_go_next(&self) -> bool {
        self.next.is_some()
    }
    fn requested(&self) -> &Start<C> {
        self.pending
            .as_ref()
            .map_or(&self.start, |(_, start)| start)
    }
    pub fn cursor(&self) -> Option<&C> {
        self.requested().cursor.as_ref()
    }
    pub fn seek_page(&self) -> Option<usize> {
        self.requested().seek
    }

    pub fn go(&mut self, page: usize) -> bool {
        if page == 0 || page == self.page || self.pending.is_some() {
            return false;
        }
        let snapshot = self.start.cursor.clone().or_else(|| self.next.clone());
        let start = if page == self.page + 1 && self.next.is_some() {
            Start {
                cursor: self.next.clone(),
                seek: None,
            }
        } else if let Some(start) = self.known.get(&page).filter(|start| start.cursor.is_some()) {
            start.clone()
        } else {
            Start {
                cursor: snapshot,
                seek: Some(page),
            }
        };
        self.pending = Some((page, start));
        true
    }

    pub fn finish_page(&mut self, next: Option<C>) {
        if let Some((page, start)) = self.pending.take() {
            self.page = page;
            self.start = start;
        }
        self.next = next;
        self.known.insert(self.page, self.start.clone());
        // Keep navigation memory bounded even when a user visits thousands of pages.
        while self.known.len() > 128 {
            let key = *self.known.keys().find(|key| **key != self.page).unwrap();
            self.known.remove(&key);
        }
    }

    pub fn fail(&mut self) {
        self.pending = None;
    }
}

impl super::Statistics {
    pub(super) fn request_listing(&mut self, cx: &mut gpui_kit::Context<Self>) {
        match self.tab {
            super::Tab::Requests => self.request_rows(cx),
            _ => self.request_groups(cx),
        }
    }
    pub(super) fn navigate_page(&mut self, page: usize, cx: &mut gpui_kit::Context<Self>) {
        if self.loading_rows {
            return;
        }
        let total = match self.tab {
            super::Tab::Requests => self.total_rows,
            _ => self.total_groups,
        };
        if page == 0
            || (self.tab == super::Tab::Requests || total <= crate::stats_dto::MAX_COUNTED_GROUPS)
                && page > pages(total, self.page_size)
        {
            return;
        }
        let changed = match self.tab {
            super::Tab::Requests => self.rows_cursors.go(page),
            _ => self.group_pages.go(page),
        };
        if changed {
            self.request_listing(cx);
        }
    }
    pub(super) fn jump_to_input(&mut self, cx: &mut gpui_kit::Context<Self>) {
        let total = match self.tab {
            super::Tab::Requests => self.total_rows,
            _ => self.total_groups,
        };
        let maximum =
            if self.tab != super::Tab::Requests && total > crate::stats_dto::MAX_COUNTED_GROUPS {
                (i64::MAX / self.page_size) as usize
            } else {
                pages(total, self.page_size)
            };
        if let Some(page) = parse_page(self.page_input.read(cx).value().as_ref(), maximum) {
            self.navigate_page(page, cx);
        } else {
            self.error = Some(
                self.t("请输入有效页码", "Enter a valid page number")
                    .to_owned(),
            );
            cx.notify();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn jump_keeps_snapshot_and_failed_navigation_keeps_visible_page() {
        let mut pager = Pager::<u64>::default();
        pager.finish_page(Some(10));
        assert!(pager.go(8));
        assert_eq!(pager.seek_page(), Some(8));
        assert_eq!(pager.cursor(), Some(&10));
        assert_eq!(pager.page_number(), 1);
        pager.fail();
        assert_eq!(pager.page_number(), 1);
        assert_eq!(pager.cursor(), None);
        assert!(pager.go(8));
        pager.finish_page(Some(80));
        assert_eq!(pager.page_number(), 8);
        assert!(pager.go(9));
        assert_eq!(pager.seek_page(), None);
        assert_eq!(pager.cursor(), Some(&80));
        pager.finish_page(None);
        assert!(pager.go(8));
        assert_eq!(pager.seek_page(), Some(8));
        pager.finish_page(Some(80));
        assert!(pager.go(1));
        assert_eq!(pager.seek_page(), Some(1));
        assert!(
            pager.cursor().is_some(),
            "returning to page one must retain its snapshot"
        );
        pager.finish_page(Some(10));
        pager.reset();
        assert_eq!(pager.page_number(), 1);
        assert!(pager.cursor().is_none());
    }
    #[test]
    fn page_input_rejects_zero_overflow_and_out_of_range() {
        for text in [
            "",
            "0",
            "-1",
            "1.5",
            "abc",
            "11",
            "999999999999999999999999",
        ] {
            assert_eq!(parse_page(text, 10), None);
        }
        assert_eq!(parse_page(" 10 ", 10), Some(10));
        assert_eq!(pages(21, 10), 3);
        assert_eq!(pages(0, 10), 1);
    }
}
