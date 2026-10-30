//! Native sync confirmations. The existing save/install/restore jobs own behavior.
use super::*;
use gpui_kit::component::Disableable;

pub(super) fn estimated_height(zh: bool, remove: bool) -> Pixels {
    // At 526px the English sync notice adds one 17px line; removal stays 318px.
    px(if !zh && !remove { 335. } else { 318. })
}

fn paragraph(text: &'static str, size: f32, line: f32, color: u32) -> Label {
    Label::new(text)
        .text_size(px(size))
        .line_height(px(line))
        .text_color(rgb(color))
        .whitespace_normal()
        .h_auto()
        .flex_none()
}

impl Proxy {
    pub(super) fn sync_confirmation(&self, cx: &mut Context<Self>) -> AnyElement {
        let remove = self.unsync_open;
        let studio = crate::theme::current_skin() == crate::theme::Skin::StudioIndigo;
        let palette = crate::theme::palette();
        let main = if remove {
            self.t(
                "移除代理管理的模型，并恢复已备份的原始条目。",
                "Remove proxy-managed models and restore backed-up original entries.",
            )
        } else {
            self.t(
                "将所有已保存供应商的模型配置写入 WorkBuddy。",
                "Update models for all saved providers in WorkBuddy.",
            )
        };
        let detail = if remove {
            self.t(
                "此操作不会清除供应商配置。重启前请先保存正在进行的工作。",
                "Provider settings are kept. Save any work before restarting WorkBuddy.",
            )
        } else {
            self.t(
                "同步前会保存当前供应商修改；重启会中断正在执行的任务。",
                "Sync first saves current-provider changes. Restart interrupts running tasks.",
            )
        };
        let notice = if remove {
            self.t(
                "移除后，旧会话可能仍引用先前的模型。",
                "Existing conversations may still reference the removed models.",
            )
        } else {
            self.t(
                "重启后，已有会话才能读取新的地址和 Key",
                "Reopen WorkBuddy so existing conversations can load new addresses and keys.",
            )
        };
        let hint = if remove {
            self.t(
                "只有移除成功才会执行所选重启操作。",
                "Restart runs only after the models have been removed successfully.",
            )
        } else {
            self.t(
                "建议先保存 WorkBuddy 中正在进行的工作",
                "Save any work in progress in WorkBuddy before restarting.",
            )
        };
        div().flex().flex_col().w_full().gap_0()
            .child(div().flex().items_center().gap(px(14.)).h(px(45.)).flex_none().pr(px(20.))
                .when(studio, |header| header.items_start())
                .child(div().flex().items_center().justify_center().size(px(45.)).flex_none().rounded(px(14.))
                    .bg(rgb(if studio { 0xeeedff } else { palette.tint }))
                    .when(studio, |badge| badge.child(Icon::new(IconName::RefreshCw)
                        .with_size(px(25.)).text_color(rgb(palette.dark))))
                    .when(!studio, |badge| badge.child(Icon::default().path("studio/updates.svg")
                        .with_size(px(25.)).text_color(rgb(palette.dark)))))
                .child(Label::new(if remove { self.t("取消同步 WorkBuddy", "Unsync WorkBuddy") } else { self.t("同步到 WorkBuddy", "Sync to WorkBuddy") })
                    .text_size(px(21.)).font_weight(FontWeight::BOLD).text_color(rgb(INK))
                    .when(studio, |title| title.line_height(px(29.)).mt(px(-1.)))))
            .child(div().flex().flex_col().mt(px(18.)).gap(px(7.)).flex_none()
                .child(paragraph(main, 14., 20., crate::theme::color::TEXT_2)
                    .when(studio, |text| text.font_weight(FontWeight::MEDIUM)))
                .child(paragraph(detail, 12., 17., MUTED)))
            .child(div().flex().flex_col().mt(px(20.)).px(px(14.)).py(px(12.)).gap(px(3.)).rounded(px(10.)).flex_none()
                .bg(rgb(crate::theme::color::BG_NOTICE))
                .child(div().flex().items_start().gap(px(6.))
                    .child(Icon::new(IconName::Info).with_size(px(14.)).mt(px(1.)).flex_none().text_color(rgb(crate::theme::color::TEXT_2)))
                    .child(paragraph(notice, 12., 17., crate::theme::color::TEXT_2)
                        .when(studio, |text| text.font_weight(FontWeight::MEDIUM)).flex_1().min_w_0()))
                .child(paragraph(hint, 11., 15., MUTED).ml(px(20.)))
                .when(!cfg!(target_os = "macos"), |notice| notice.child(paragraph(self.t(
                    "此平台暂不支持自动重启；请使用不重启选项，再手动重开 WorkBuddy。",
                    "Automatic restart is unavailable on this platform. Use the option without restart, then reopen WorkBuddy manually."), 11., 15., MUTED).ml(px(20.)))))
            .child(div().flex().items_center().justify_end().gap(px(10.)).mt(px(23.)).pt(px(22.)).border_t_1()
                .border_color(rgb(crate::theme::color::BORDER)).flex_none()
                .child(crate::ui_components::action_button(if remove { "unsync-cancel" } else { "sync-cancel" })
                    .outline().h(px(36.)).min_w(px(83.)).rounded(px(9.)).label(self.t("取消", "Cancel"))
                    .on_click(cx.listener(|this, _, window, cx| this.dismiss(&Dismiss, window, cx))))
                .child(crate::ui_components::action_button(if remove { "unsync-only" } else { "sync-only" })
                    .outline().h(px(36.)).min_w(px(92.)).rounded(px(9.)).disabled(self.busy)
                    .label(if remove { self.t("仅移除", "Remove only") } else { self.t("仅同步", "Sync only") })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if remove { this.unsync(false, cx) } else { this.sync(false, cx) }
                    })))
                .child(crate::ui_components::action_button(if remove { "unsync-restart" } else { "sync-restart" })
                    .primary().h(px(36.)).min_w(px(126.)).rounded(px(9.))
                    .disabled(self.busy || !cfg!(target_os = "macos"))
                    .label(if remove { self.t("移除并重启", "Remove and restart") } else { self.t("同步并重启", "Sync and restart") })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if remove { this.unsync(true, cx) } else { this.sync(true, cx) }
                    }))))
            .into_any_element()
    }
}
