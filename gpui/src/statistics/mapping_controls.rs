//! Read-only import diagnostics. Model classification is automatic.
use super::*;
#[derive(Default)]
pub(super) struct MappingUi {
    pub(super) open: bool,
}
impl Statistics {
    pub(super) fn open_mapping_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.mapping_ui.open {
            return;
        }
        self.mapping_ui.open = true;
        let owner = cx.entity().downgrade();
        let text=self.t("模型自动分为内置／自定义，无需确认创建来源。同一次调用优先保留本机代理明细；日志按会话与消息标识精确去重。历史缺失数据会在后台重新读取。", "Models are classified as built-in or custom automatically. Exact session/message identities deduplicate calls, preferring native proxy details. Missing history is reread in the background.");
        window.open_dialog(cx, move |dialog, _, _| {
            dialog
                .w(px(520.))
                .close_button(true)
                .child(
                    div()
                        .id("automatic-import-diagnostics")
                        .role(Role::Group)
                        .aria_label(text)
                        .p_3()
                        .text_size(px(13.))
                        .child(text),
                )
                .on_close({
                    let owner = owner.clone();
                    move |_, _, cx| {
                        let _ = owner.update(cx, |this, cx| {
                            this.mapping_ui.open = false;
                            cx.notify();
                        });
                    }
                })
        });
    }
    pub(super) fn close_mapping_state(&mut self) {
        self.mapping_ui.open = false;
    }
    pub(super) fn expire_mapping_preview(&mut self, _: &mut Context<Self>) {}
    pub(super) fn invalidate_mapping_draft(&mut self, _: &mut Context<Self>) {}
    pub(super) fn handle_mapping_reply(
        &mut self,
        _: JobKind,
        _: u64,
        tag: &str,
        _: &Result<Value, String>,
        _: &mut Context<Self>,
    ) -> bool {
        tag.starts_with("mapping-")
    }
}
