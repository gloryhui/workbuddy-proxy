//! 原生更新弹窗及其 Core 命令投影。
//!
//! GPUI 只负责展示检查结果、发送 `check_update`/`update_start`/`update_apply`/
//! `update_cancel` 等命令，并轮询 `update_status`。网络、SHA-256、暂存、等待活动
//! 请求和 helper 启动都属于 Core；关闭弹窗不等于取消 Core 中正在运行的下载。
use super::*;
use crate::update_dto::{Phase, Progress};
use gpui_kit::component::Disableable;

fn accessible_label(value: impl Into<SharedString>) -> Stateful<Div> {
    let value = value.into();
    div()
        .id(value.clone())
        .role(Role::Label)
        .aria_label(value.clone())
        .child(value.to_string())
}

#[derive(Clone, Debug, PartialEq)]
struct UpdateInfo {
    /// 检查接口的展示快照。`available` 与 `online_supported` 分开：发现新版本不代表
    /// 当前平台一定能自动在线安装。
    available: bool,
    tag: String,
    current: String,
    name: String,
    body: String,
    published_at: String,
    platform: String,
    online_supported: bool,
    online_error: Option<String>,
}
fn valid_version(value: &str) -> bool {
    // UI 只渲染 Core 返回的保守三段数字版本；非法 tag 不作为可拼接的网页路径使用。
    let parts: Vec<_> = value
        .strip_prefix('v')
        .unwrap_or(value)
        .split('.')
        .collect();
    value.len() <= 32
        && parts.len() == 3
        && parts.iter().all(|p| {
            !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()) && p.parse::<u32>().is_ok()
        })
}
fn parse_result(text: &str) -> Result<UpdateInfo, ()> {
    // 先把字符串解析成动态 JSON 树 Value，再逐字段检查并组装 UpdateInfo。展示字段缺失/`null`
    // 可降级为空字符串，但 available、tag、current 必须有正确类型；旧结果若没有
    // online_supported，则按 `false` 处理，避免把未知能力当成可在线安装。
    let value: Value = serde_json::from_str(text).map_err(|_| ())?;
    let label = |name: &str| {
        value[name]
            .as_str()
            .filter(|v| valid_version(v))
            .map(str::to_owned)
            .ok_or(())
    };
    let field = |name: &str, limit: usize| -> Result<String, ()> {
        match value.get(name) {
            None | Some(Value::Null) => Ok(String::new()),
            Some(Value::String(s)) if s.len() <= limit => Ok(s.clone()),
            _ => Err(()),
        }
    };
    Ok(UpdateInfo {
        available: value["available"].as_bool().ok_or(())?,
        tag: label("tag")?,
        current: label("current")?,
        name: field("name", 512)?,
        body: field("body", 64 * 1024)?,
        published_at: field("published_at", 64)?,
        platform: field("platform", 128)?,
        online_supported: value
            .get("online_supported")
            .map(Value::as_bool)
            .unwrap_or(Some(false))
            .ok_or(())?,
        online_error: value["online_error"].as_str().map(str::to_owned),
    })
}

pub(super) struct State {
    /// `open` 只控制弹窗显示；下载/安装状态仍由 `progress` 和 Core 持有。
    pub(super) open: bool,
    checking: bool,
    info: Option<UpdateInfo>,
    error: Option<String>,
    progress: Progress,
    /// 单次 status 请求在途时置 true，避免 UI 每秒叠加多个查询。
    polling: bool,
    last_poll: Instant,
    first_status: bool,
    previous_focus: Option<FocusHandle>,
    scroll: ScrollHandle,
}
pub(super) fn init(_: &mut App) {}
impl State {
    pub(super) fn new(_: &mut App) -> Self {
        Self {
            open: false,
            checking: false,
            info: None,
            error: None,
            progress: Progress::default(),
            polling: false,
            last_poll: Instant::now() - Duration::from_secs(2),
            first_status: true,
            previous_focus: None,
            scroll: ScrollHandle::new(),
        }
    }
    fn locked(&self) -> bool {
        // 这里只锁住 Installing 的界面交互；Downloading/Ready 等阶段允许关闭弹窗，
        // 不等于向后端发送取消命令。
        self.progress.phase == Phase::Installing
    }
}
impl Proxy {
    pub(super) fn check_update_with_notice(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy || self.dialog_open {
            return;
        }
        self.update_notice.previous_focus = window.focused(cx);
        self.update_notice.open = true;
        if !matches!(
            self.update_notice.progress.phase,
            Phase::Idle | Phase::Cancelled
        ) {
            cx.notify();
            return;
        }
        self.start_update_check(cx);
    }
    fn start_update_check(&mut self, cx: &mut Context<Self>) {
        // 每次用户主动检查先尝试 ACK 普通旧结果，再发送 `force=true` 的检查命令。
        // checking/info/progress 在这里先清空，后续只由命令回调重新填充。
        self.update_notice.scroll.set_offset(point(px(0.), px(0.)));
        self.update_notice.checking = true;
        self.update_notice.error = None;
        self.update_notice.info = None;
        self.update_notice.progress = Progress::default();
        self.send(
            "check_update",
            vec![
                json!({"command":"update_ack"}),
                json!({"command":"check_update","force":true}),
            ],
            cx,
        );
        if !self.busy {
            self.finish_update_notice(Err(self.status.clone()), cx);
        }
    }
    pub(super) fn finish_update_notice(
        &mut self,
        result: Result<String, String>,
        cx: &mut Context<Self>,
    ) {
        // 这是 check_update 命令的最终回包：成功时还要通过 GPUI 自己的边界解析，
        // 失败或格式异常只更新错误展示，不启动下载。
        self.update_notice.scroll.set_offset(point(px(0.), px(0.)));
        self.update_notice.checking = false;
        match result {
            Ok(text) => match parse_result(&text) {
                Ok(info) => {
                    self.update_notice.info = Some(info);
                    self.update_notice.error = None;
                }
                Err(()) => {
                    self.update_notice.error = Some(
                        self.t("更新服务返回了无效结果", "Invalid update check response")
                            .into(),
                    )
                }
            },
            Err(error) => self.update_notice.error = Some(error),
        }
        cx.notify();
    }
    pub(super) fn poll_update(&mut self, _cx: &mut Context<Self>) {
        // `ui.rs` 的周期 pump 会调用本函数。距上次发起至少 500ms、当前没有 status
        // 请求在途，并且首次启动/弹窗打开/进度 busy 时，才用 try_send 投递一次
        // update_status。队列满时 try_send 失败，不会阻塞 UI；Ready 的 busy() 为真，
        // 因此仍会继续轮询。
        if !self.update_notice.polling
            && self.update_notice.last_poll.elapsed() >= Duration::from_millis(500)
            && (self.update_notice.first_status
                || self.update_notice.open
                || self.update_notice.progress.busy())
            && self
                .backend
                .jobs
                .try_send(Job {
                    tag: "update_status".into(),
                    steps: vec![json!({"command":"update_status"})],
                })
                .is_ok()
        {
            self.update_notice.polling = true;
            self.update_notice.last_poll = Instant::now();
        }
    }
    pub(super) fn finish_update_status(
        &mut self,
        result: Result<String, String>,
        cx: &mut Context<Self>,
    ) {
        // 成功解析的 Progress 才是 Core 状态快照。首个成功状态若是 Complete/Failed/
        // RecoveryFailed 且没有其他模态对话框，弹窗会主动打开，以便展示上次 helper 结果。
        // status 命令失败或 JSON 无效时只结束本次 polling，不把旧快照改成成功。
        self.update_notice.polling = false;
        let Ok(text) = result else {
            return;
        };
        let Ok(progress) = serde_json::from_str::<Progress>(&text) else {
            return;
        };
        let first = self.update_notice.first_status;
        self.update_notice.first_status = false;
        if first
            && matches!(
                progress.phase,
                Phase::Complete | Phase::Failed | Phase::RecoveryFailed
            )
            && !self.dialog_open
        {
            self.update_notice.open = true;
        }
        if progress != self.update_notice.progress {
            if progress.phase != self.update_notice.progress.phase {
                self.update_notice.scroll.set_offset(point(px(0.), px(0.)));
            }
            self.update_notice.progress = progress;
            cx.notify();
        }
    }
    pub(super) fn finish_update_action(
        &mut self,
        result: Result<String, String>,
        cx: &mut Context<Self>,
    ) {
        // update_start/update_apply 的返回值只是命令处理时的阶段快照；线程随后仍可
        // 推进到下一阶段。例如 apply 返回 Waiting，后续 status 才会变成 Installing。
        // 这里清掉的是 GPUI 的短命令 busy 标志，不表示 Core 下载或安装已经结束。
        self.busy = false;
        match result {
            Ok(text) => match serde_json::from_str::<Progress>(&text) {
                Ok(p) => {
                    if p.phase != self.update_notice.progress.phase {
                        self.update_notice.scroll.set_offset(point(px(0.), px(0.)));
                    }
                    self.update_notice.progress = p;
                    self.update_notice.error = None;
                }
                Err(_) => {
                    self.update_notice.error =
                        Some(self.t("更新状态无效", "Invalid update status").into())
                }
            },
            Err(error) => self.update_notice.error = Some(error),
        }
        cx.notify();
    }
    pub(super) fn close_update_notice(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Installing 不允许关闭；其他阶段只把 `open` 置 false。若 Core 仍 busy 或正在
        // check，不发送 update_ack，下载/准备会继续并由后台轮询。非 busy 时这里只
        // try_send ACK 并把本地快照重置为默认值，不等待 ACK 结果。
        //
        // Core 对未解决 maintenance marker 的 RecoveryFailed ACK 会保留 phase；
        // GPUI 的本地重置不会删除那个后端/文件状态，也不是维护警告的清除确认。
        if self.update_notice.locked() {
            return;
        }
        self.update_notice.open = false;
        if !self.update_notice.progress.busy() && !self.update_notice.checking {
            let _ = self.backend.jobs.try_send(Job {
                tag: "update_ack".into(),
                steps: vec![json!({"command":"update_ack"})],
            });
            self.update_notice.progress = Progress::default();
        }
        if let Some(focus) = self.update_notice.previous_focus.take() {
            focus.focus(window, cx);
        }
        cx.notify();
    }
    pub(super) fn subscribe_update_notice_bounds(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.observe_window_bounds(window, |this, _, cx| {
            if this.update_notice.open {
                cx.notify();
            }
        })
        .detach();
    }
    pub(super) fn update_toolbar(&self, toolbar: AnyElement, _: &mut Context<Self>) -> AnyElement {
        toolbar
    }
    fn update_primary(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy || self.update_notice.checking {
            return;
        }
        match self.update_notice.progress.phase {
            Phase::Ready=>{let tag=self.update_notice.progress.tag.clone();self.send("update_apply",vec![json!({"command":"update_apply","tag":tag})],cx);}
            Phase::Idle|Phase::Cancelled|Phase::Failed=>{
                if let Some(info)=&self.update_notice.info && info.available && info.online_supported {
                    let tag=info.tag.clone();self.update_notice.error=None;self.send("update_start",vec![json!({"command":"update_start","tag":tag})],cx);
                } else if self.update_notice.error.is_some() || self.update_notice.progress.phase==Phase::Failed {self.start_update_check(cx);}
                else {self.close_update_notice(window,cx);}
            }
            Phase::Complete=>self.close_update_notice(window,cx),
            Phase::RecoveryFailed=>cx.open_url("https://github.com/gloryhui/workbuddy-proxy/blob/main/docs/studio_v2/UPGRADE_ROLLBACK.md"),
            _=>{}
        }
    }
    fn update_cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Download/Verify/Prepare/Wait 只发协作取消命令。Waiting 时先关闭弹窗，
        // 因为 plan 会保留并在后台取消完成后回到 Ready。Ready 的“稍后安装”走普通
        // 关闭路径；Installing 的关闭按钮/取消按钮在此之前已被禁用。
        if matches!(
            self.update_notice.progress.phase,
            Phase::Downloading | Phase::Verifying | Phase::Preparing | Phase::Waiting
        ) {
            let waiting = self.update_notice.progress.phase == Phase::Waiting;
            self.send(
                "update_cancel",
                vec![json!({"command":"update_cancel"})],
                cx,
            );
            if waiting {
                self.update_notice.open = false;
            }
        } else {
            self.close_update_notice(window, cx);
        }
    }
    pub(super) fn update_dialog(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let u = &self.update_notice;
        let p = &u.progress;
        let info = u.info.as_ref();
        let (title, headline, description) = if u.checking {
            (
                self.t("检查软件更新", "Check software updates"),
                self.t("正在检查最新版本…", "Checking for the latest release…")
                    .to_owned(),
                self.t(
                    "正在连接 GitHub Releases，请稍候。",
                    "Connecting to GitHub Releases. Please wait.",
                )
                .to_owned(),
            )
        } else if let Some(error) = &u.error {
            (
                self.t("操作未完成", "Could not complete the update"),
                self.t("请检查错误详情后重试", "Review the error and retry")
                    .to_owned(),
                crate::i18n::message(error, self.zh),
            )
        } else {
            match p.phase {
                Phase::Downloading=>(self.t("正在下载更新","Downloading update"),format!("{} {}",self.t("正在下载","Downloading"),p.tag),self.t("下载期间可以继续使用本地代理。","The local relay remains available during the download.").into()),
                Phase::Verifying|Phase::Preparing=>(self.t("正在校验更新","Verifying update"),self.t("下载完成，正在校验文件","Download complete; verifying files").into(),self.t("校验文件、平台与应用版本，完成后再安装。","Checking the file, platform and application version before installation.").into()),
                Phase::Ready=>(self.t("更新已准备就绪","Update ready"),format!("{} {}",self.t("安装并重新打开","Install and reopen"),p.tag),self.t("安装将短暂中断本地代理。我们会先等待进行中的请求完成，再退出并安装。","Installation briefly interrupts the relay. Active requests finish before the application exits and installs.").into()),
                Phase::Waiting=>(self.t("正在准备安装","Preparing installation"),self.t("等待进行中的请求完成","Waiting for active requests").into(),self.t("新请求暂不接入。不会强制中断已有请求，也可以选择稍后安装。","New requests are paused. Active requests will not be interrupted; you can defer installation.").into()),
                Phase::Installing=>(self.t("正在安装更新","Installing update"),format!("{} {}",self.t("正在安装","Installing"),p.tag),self.t("更新程序将在安装完成后重新打开应用。","The updater will reopen the application after installation.").into()),
                Phase::Complete=>(self.t("更新完成","Update complete"),format!("{} {}",self.t("欢迎使用","Welcome to"),p.tag),self.t("已核对运行版本，本地代理已恢复。","The running version and local relay have been verified.").into()),
                Phase::Failed|Phase::RecoveryFailed=>(self.t("本次更新未完成","Update was not completed"),self.t("请根据下方原因处理","Review the reason below").into(),p.error.as_deref().map(|e|crate::i18n::message(e,self.zh)).unwrap_or_default()),
                Phase::Cancelled=>(self.t("已取消更新","Update cancelled"),self.t("当前版本未改变","The installed version is unchanged").into(),self.t("可以重新下载，或前往 GitHub 手动下载。","Retry the download or download manually from GitHub.").into()),
                _=>match info {
                    Some(i) if i.available=>(self.t("发现新版本","Update available"),if i.name.is_empty(){format!("WorkBuddy Proxy {}",i.tag)}else{i.name.clone()},String::new()),
                    Some(i)=>(self.t("已是最新版本","Already up to date"),format!("WorkBuddy Proxy {}",i.current),self.t("当前已是最新正式版本，暂无需要安装的更新。","You are running the latest stable release.").into()),
                    None=>(self.t("软件更新","Software updates"),self.t("检查最新正式版本","Check the latest stable release").into(),String::new()),
                }
            }
        };
        let updating = matches!(
            p.phase,
            Phase::Downloading
                | Phase::Verifying
                | Phase::Preparing
                | Phase::Waiting
                | Phase::Installing
        );
        let online_disabled = self.busy
            || u.checking
            || updating
            || info.is_some_and(|i| i.available && !i.online_supported) && p.phase == Phase::Idle;
        let primary = match p.phase {
            Phase::Ready => self.t("重启并安装", "Restart and install"),
            Phase::Waiting => self.t("等待中…", "Waiting…"),
            Phase::Installing => self.t("正在安装…", "Installing…"),
            Phase::Downloading => self.t("下载中…", "Downloading…"),
            Phase::Verifying | Phase::Preparing => self.t("校验中…", "Verifying…"),
            Phase::Complete => self.t("开始使用", "Continue"),
            Phase::RecoveryFailed => self.t("查看恢复指引", "Recovery guide"),
            Phase::Failed => self.t("重试更新", "Retry update"),
            _ if u.checking => self.t("正在检查…", "Checking…"),
            _ if u.error.is_some() => self.t("重新检查", "Retry check"),
            _ if info.is_some_and(|i| i.available) => self.t("立即更新", "Update now"),
            _ => self.t("完成", "Done"),
        };
        let no_update = !u.checking
            && u.error.is_none()
            && matches!(p.phase, Phase::Idle | Phase::Cancelled)
            && info.is_some_and(|i| !i.available);
        let cancel = match p.phase {
            Phase::Downloading | Phase::Verifying | Phase::Preparing => {
                self.t("取消下载", "Cancel download")
            }
            Phase::Ready | Phase::Waiting => self.t("稍后安装", "Install later"),
            _ => self.t("暂不更新", "Not now"),
        };
        let mut content = crate::ui_components::scrollable_form("update-release-scroll")
            .track_scroll(&u.scroll)
            .gap(px(12.));
        content = content.child(
            accessible_label(headline)
                .text_size(px(22.))
                .line_height(px(31.))
                .font_weight(FontWeight::BOLD)
                .whitespace_normal()
                .text_color(rgb(INK)),
        );
        if !description.is_empty() {
            content = content.child(
                accessible_label(description)
                    .text_size(px(13.))
                    .line_height(px(18.))
                    .whitespace_normal(),
            );
        }
        if let Some(i) = info {
            content = content.child(
                accessible_label(format!(
                    "{} v{}  →  {} {}",
                    self.t("当前版本", "Current"),
                    i.current.trim_start_matches('v'),
                    self.t("最新正式版", "Latest stable"),
                    i.tag
                ))
                .text_size(px(12.)),
            );
            let date = chrono::DateTime::parse_from_rfc3339(&i.published_at)
                .map(|d| {
                    d.with_timezone(&chrono::Local)
                        .format("%Y-%m-%d")
                        .to_string()
                })
                .unwrap_or_default();
            content = content.child(
                accessible_label(format!(
                    "{date} · {} · GitHub Releases",
                    match i.platform.as_str() {
                        "macos / aarch64" => "macOS · Apple Silicon",
                        "macos / x86_64" => "macOS · Intel",
                        "windows / x86_64" => "Windows · x64",
                        other => other,
                    }
                ))
                .text_size(px(12.)),
            );
            if !updating && p.phase != Phase::Complete {
                if !i.body.is_empty() {
                    content = content
                        .child(div().h(px(1.)).bg(rgb(crate::theme::color::BORDER)))
                        .child(
                            accessible_label(self.t("本次更新", "Release notes"))
                                .text_size(px(15.))
                                .font_weight(FontWeight::BOLD),
                        );
                    for (index, line) in i.body.lines().enumerate() {
                        if !line.trim().is_empty() {
                            content = content.child(
                                div()
                                    .id(("release-line", index))
                                    .role(Role::Label)
                                    .aria_label(line.trim_start_matches('#').trim().to_owned())
                                    .text_size(px(13.))
                                    .line_height(px(18.))
                                    .whitespace_normal()
                                    .child(line.trim_start_matches('#').trim().to_owned()),
                            );
                        }
                    }
                }
                if let Some(reason) = &i.online_error {
                    content = content.child(
                        accessible_label(crate::i18n::message(reason, self.zh))
                            .whitespace_normal()
                            .text_size(px(12.))
                            .text_color(rgb(MUTED)),
                    );
                }
            }
        }
        if p.phase == Phase::Downloading && p.total > 0 {
            let ratio = (p.downloaded as f32 / p.total as f32).clamp(0., 1.);
            content = content
                .child(accessible_label(format!(
                    "{:.0}% · {:.1} / {:.1} MiB",
                    ratio * 100.,
                    p.downloaded as f64 / 1048576.,
                    p.total as f64 / 1048576.
                )))
                .child(
                    div()
                        .w_full()
                        .h(px(6.))
                        .rounded(px(3.))
                        .bg(rgb(crate::theme::color::BG_TABLE_HEAD))
                        .child(
                            div()
                                .w(relative(ratio))
                                .h_full()
                                .rounded(px(3.))
                                .bg(rgb(crate::theme::palette().primary)),
                        ),
                );
        }
        if let Some(path) = &p.backup_path {
            content = content.child(
                accessible_label(format!("{} {path}", self.t("备份位置：", "Backup:")))
                    .whitespace_normal()
                    .text_size(px(12.)),
            );
        }
        let tag = info.map(|i| i.tag.as_str()).unwrap_or(&p.tag);
        let github = if valid_version(tag) {
            format!("https://github.com/gloryhui/workbuddy-proxy/releases/tag/{tag}")
        } else {
            "https://github.com/gloryhui/workbuddy-proxy/releases/latest".into()
        };
        div()
            .w_full()
            .h((window.viewport_size().height - px(112.)).min(px(512.)))
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(17.))
                            .line_height(px(24.))
                            .font_weight(FontWeight::BOLD)
                            .id("software-update-heading")
                            .role(Role::Label)
                            .aria_label(title)
                            .child(title),
                    )
                    .child(
                        crate::ui_components::action_button("close-software-update")
                            .ghost()
                            .size(px(24.))
                            .p_0()
                            .disabled(u.locked())
                            .accessibility_label(self.t("关闭更新提示", "Close update dialog"))
                            .child(
                                Icon::default()
                                    .path("studio/update-close.svg")
                                    .with_size(px(13.7)),
                            )
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.close_update_notice(window, cx)
                            })),
                    ),
            )
            .child(content)
            .child(
                div()
                    .h(px(1.))
                    .flex_none()
                    .bg(rgb(crate::theme::color::BORDER)),
            )
            .child(
                div().flex().items_center().justify_between().child(
                    accessible_label("GitHub Releases")
                        .text_size(px(12.))
                        .text_color(rgb(MUTED)),
                ),
            )
            .child(
                div()
                    .flex()
                    .justify_end()
                    .flex_none()
                    .gap(px(8.))
                    .when(!no_update, |row| {
                        row.child(
                            crate::ui_components::action_button("cancel-software-update")
                                .label(cancel)
                                .disabled(u.locked() || (self.busy && !u.checking))
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.update_cancel(window, cx)
                                })),
                        )
                    })
                    .child(
                        crate::ui_components::action_button("download-update-github")
                            .label(self.t("前往 GitHub", "Go to GitHub"))
                            .disabled(u.locked())
                            .on_click(move |_, _, cx| cx.open_url(&github)),
                    )
                    .child(
                        crate::ui_components::action_button("apply-software-update")
                            .primary()
                            .label(primary)
                            .disabled(online_disabled)
                            .on_click(
                                cx.listener(|this, _, window, cx| this.update_primary(window, cx)),
                            ),
                    ),
            )
            .into_any_element()
    }
}
#[cfg(test)]
mod tests {
    use super::parse_result;
    #[test]
    fn requires_typed_result_and_safe_versions_but_tolerates_older_metadata() {
        let i = parse_result(r#"{"available":true,"tag":"v0.6.0","current":"0.5.7"}"#).unwrap();
        assert!(i.available);
        assert!(!i.online_supported);
        for text in [
            "{}",
            "null",
            "not json",
            r#"{"available":"false","tag":"v0.6.0","current":"0.5.7"}"#,
            r#"{"available":true,"tag":"../bad","current":"0.5.7"}"#,
            r#"{"available":false,"tag":"v0.6.0\n","current":"0.5.7"}"#,
        ] {
            assert!(parse_result(text).is_err());
        }
    }
}
