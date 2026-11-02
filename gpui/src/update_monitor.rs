//! Read-only progress window for the copied updater UI; it never opens the profile or its locks.
use crate::update_dto::{Phase, Progress};
use gpui_kit::component::Disableable;
use gpui_kit::component::{button::ButtonVariants, label::Label};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use std::{path::PathBuf, time::Duration};

struct Monitor {
    path: PathBuf,
    progress: Progress,
}
impl Monitor {
    fn new(path: PathBuf, window: &mut Window, cx: &mut Context<Self>) -> Self {
        cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(250))
                    .await;
                if this
                    .update_in(cx, |this, _, cx| {
                        let value = std::fs::read(&this.path)
                            .ok()
                            .filter(|b| b.len() <= 65536)
                            .and_then(|b| serde_json::from_slice::<Progress>(&b).ok());
                        if let Some(value) = value
                            && value != this.progress
                        {
                            this.progress = value;
                            cx.notify();
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        Self {
            path,
            progress: Progress {
                phase: Phase::Installing,
                ..Default::default()
            },
        }
    }
}
impl Render for Monitor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = &self.progress;
        let done = matches!(
            p.phase,
            Phase::Complete | Phase::Failed | Phase::RecoveryFailed
        );
        let heading = match p.phase {
            Phase::Complete => "更新完成 / Update complete",
            Phase::Failed => "更新未完成 / Update failed",
            Phase::RecoveryFailed => "需要手动恢复 / Manual recovery required",
            _ => "正在安装更新 / Installing update",
        };
        div().size_full().p(px(24.)).bg(rgb(crate::theme::color::BG_BAR)).text_color(rgb(crate::theme::color::TEXT_1)).flex().flex_col().gap(px(16.))
            .child(Label::new(heading).text_size(px(17.)).font_weight(FontWeight::BOLD))
            .child(Label::new(format!("WorkBuddy Proxy {}",p.tag)).text_size(px(22.)).font_weight(FontWeight::BOLD))
            .child(Label::new(if done {"请查看下方执行结果。 / Review the result below."} else {"安装完成后会重新打开应用，关闭此窗口不会取消安装。 / The application reopens after installation; closing this window does not cancel it."}).text_size(px(13.)).whitespace_normal())
            .child(crate::ui_components::scrollable_form("update-monitor-log")
                .when_some(p.error.clone(),|form,error|form.child(Label::new(error).whitespace_normal().text_color(rgb(crate::theme::color::DANGER))))
                .children(p.logs.iter().map(|line|Label::new(line.clone()).text_size(px(12.)).whitespace_normal()))
                .when_some(p.backup_path.clone(),|form,path|form.child(Label::new(format!("备份 / Backup: {path}")).text_size(px(12.)).whitespace_normal())))
            .child(div().flex().justify_end().gap(px(8.))
                .child(crate::ui_components::action_button("monitor-github").label("前往 GitHub / GitHub").disabled(!done).on_click(|_,_,cx|cx.open_url("https://github.com/gloryhui/workbuddy-proxy/releases/latest")))
                .child(crate::ui_components::action_button("monitor-finish").primary().label(if done {"关闭 / Close"} else {"正在安装… / Installing…"}).disabled(!done).on_click(cx.listener(|_,_,_,cx|cx.quit()))))
    }
}
pub fn run(path: PathBuf) {
    if path.file_name().and_then(|s| s.to_str()) != Some("update-install-result.json") {
        return;
    }
    gpui_kit::application()
        .with_assets(crate::assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);
            crate::theme::apply_skin(crate::theme::Skin::StudioIndigo, cx);
            let bounds = Bounds::centered(None, size(px(620.), px(560.)), cx);
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    window_min_size: Some(size(px(620.), px(460.))),
                    ..Default::default()
                },
                cx,
                move |window, cx| cx.new(|cx| Monitor::new(path, window, cx)),
            )
            .expect("Cannot open update progress");
            cx.on_window_closed(|cx, _| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();
            cx.activate(true);
        });
}
