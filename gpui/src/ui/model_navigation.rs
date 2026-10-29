//! Application multi-selection keys over the existing virtual Kit table.
use super::*;

pub(crate) fn bindings() -> Vec<KeyBinding> {
    let mut keys = Vec::new();
    // The descendant rule wins over Kit's single-row arrows if a pointer
    // interaction focuses its inner table. All keys remain model-list scoped.
    for context in ["ModelList", "ModelList > DataTable"] {
        keys.extend([
            KeyBinding::new("up", ModelPrevious, Some(context)),
            KeyBinding::new("down", ModelNext, Some(context)),
            KeyBinding::new("home", ModelFirst, Some(context)),
            KeyBinding::new("end", ModelLast, Some(context)),
            KeyBinding::new("shift-up", ExtendModelPrevious, Some(context)),
            KeyBinding::new("shift-down", ExtendModelNext, Some(context)),
            KeyBinding::new("shift-home", ExtendModelFirst, Some(context)),
            KeyBinding::new("shift-end", ExtendModelLast, Some(context)),
            KeyBinding::new("tab", ModelNextControl, Some(context)),
            KeyBinding::new("shift-tab", ModelPreviousControl, Some(context)),
        ]);
    }
    keys
}

impl Proxy {
    pub(super) fn move_models(
        &mut self,
        movement: ModelMove,
        extend: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.busy || self.editing.is_some() || self.picker.is_some() {
            return;
        }
        if let Some(row) = self.selection.navigate(movement, self.models.len(), extend) {
            self.model_table.update(cx, |table, _| {
                table.vertical_scroll_handle.scroll_to_item(
                    row,
                    match movement {
                        ModelMove::Previous | ModelMove::First => ScrollStrategy::Top,
                        ModelMove::Next | ModelMove::Last => ScrollStrategy::Bottom,
                    },
                );
            });
        }
        self.table_focus.focus(window, cx);
        cx.stop_propagation();
        cx.notify();
    }
}
