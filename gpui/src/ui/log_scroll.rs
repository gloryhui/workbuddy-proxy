//! Retain a visible log entry when the bounded history drops old rows.
use gpui_kit::component::{RopeExt, input::TextareaState};
use gpui_kit::{Context, Pixels, Point, point, px};

pub(super) fn restore_anchor(
    state: &mut TextareaState,
    byte: usize,
    y: Pixels,
    offset: Point<Pixels>,
    cx: &mut Context<TextareaState>,
) -> bool {
    let row = state.text().offset_to_point(byte).row;
    if !state
        .visible_row_range()
        .is_some_and(|rows| rows.contains(&row))
    {
        return false;
    }
    let Some(bounds) = state.range_to_bounds(&(byte..byte)) else {
        return false;
    };
    // range_to_bounds already includes the editor's scroll offset.
    let delta = y - bounds.top();
    if delta > px(0.5) || delta < px(-0.5) {
        state.set_scroll_offset(offset + point(px(0.), delta), cx);
    }
    true
}

pub(super) fn retained_anchor(old: &str, new: &str, row: usize) -> Option<(usize, usize)> {
    let old_rows: Vec<_> = old.split('\n').collect();
    let new_rows: Vec<_> = new.split('\n').collect();
    if new_rows.starts_with(&old_rows) {
        return None;
    }
    for removed in 1..old_rows.len() {
        let retained = old_rows.len() - removed;
        if new_rows.len() >= retained && old_rows[removed..] == new_rows[..retained] {
            if row < removed || row >= old_rows.len() {
                return None;
            }
            let offset =
                |text: &str, count| text.split_inclusive('\n').take(count).map(str::len).sum();
            return Some((offset(old, row), offset(new, row - removed)));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    #[test]
    fn retained_history_uses_utf8_byte_offsets_after_multiple_rows_expire() {
        let old = "旧记录\n较早的一条\n正在阅读 🦀\n最后一条";
        let new = "正在阅读 🦀\n最后一条\n新增一\n新增二";
        assert_eq!(
            super::retained_anchor(old, new, 2),
            Some(("旧记录\n较早的一条\n".len(), 0))
        );
        assert_eq!(super::retained_anchor(old, new, 0), None);
    }

    #[test]
    fn appended_or_replaced_history_needs_no_retained_row_correction() {
        assert_eq!(super::retained_anchor("a\nb", "a\nb\nc", 1), None);
        assert_eq!(super::retained_anchor("a\na", "a\na\nnew", 1), None);
        assert_eq!(super::retained_anchor("a\nb", "new history", 1), None);
        assert_eq!(
            super::retained_anchor("same\nsame\ntail", "same\ntail\nnew", 2),
            Some((10, 5))
        );
    }
}
