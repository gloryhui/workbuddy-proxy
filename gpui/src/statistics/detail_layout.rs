//! Studio request detail, backed only by the selected record's own snapshot.
use super::*;
use gpui_kit::component::scroll::{ScrollableElement, ScrollableMask};

fn row(
    index: usize,
    name: impl Into<String>,
    value: String,
    color: u32,
    wrap: bool,
) -> Stateful<Div> {
    let name = name.into();
    let hint = value.clone();
    div()
        .id(("request-detail-row", index))
        .role(Role::Group)
        .aria_label(format!("{name}: {value}"))
        .flex()
        .items_center()
        .gap(px(9.))
        .h(px(36.))
        .flex_none()
        .when(wrap, |row| {
            row.h_auto().min_h(px(36.)).items_start().py(px(8.))
        })
        .tooltip(move |window, cx| {
            gpui_kit::component::tooltip::Tooltip::new(hint.clone())
                .max_w(px(560.))
                .build(window, cx)
        })
        .child(
            Label::new(name)
                .w(px(186.))
                .flex_none()
                .text_size(px(12.))
                .text_color(rgb(MUTED)),
        )
        .child(
            Label::new(value)
                .flex_1()
                .min_w_0()
                .when(wrap, |label| label.whitespace_normal())
                .when(!wrap, |label| label.text_ellipsis())
                .text_size(px(12.))
                .text_color(rgb(color)),
        )
}

fn cost_text(item: &RequestRowDto, zh: bool) -> String {
    match item.cost.as_ref() {
        Some(cost)
            if matches!(
                cost.status,
                CostStatus::Priced | CostStatus::DefaultZero | CostStatus::NotApplicable
            ) =>
        {
            format!(
                "{} {}",
                cost.currency.map_or("", |currency| currency.as_str()),
                usage_table::amount(cost.total_cost_nanos.as_deref(), cost.currency)
            )
        }
        Some(cost) => usage_table::cost_hint(cost.status, zh),
        None => usage_table::cost_label(CostStatus::Unpriced, zh),
    }
}

fn snapshot_text(snapshot: &crate::stats_dto::PriceSnapshotDto, zh: bool) -> String {
    let text = if snapshot.price_source == crate::stats_dto::PriceSource::DefaultZero {
        if zh {
            "未配置价格，按 0 计算"
        } else {
            "No price configured · counted as 0"
        }
        .to_owned()
    } else {
        [
            snapshot.input_rate_nanos.as_deref(),
            snapshot.output_rate_nanos.as_deref(),
            snapshot.cache_read_rate_nanos.as_deref(),
            snapshot.cache_write_rate_nanos.as_deref(),
        ]
        .iter()
        .enumerate()
        .filter_map(|(index, rate)| {
            rate.map(|value| {
                format!(
                    "{} {}",
                    ["in", "out", "read", "write"][index],
                    usage_table::amount(Some(value), None)
                )
            })
        })
        .collect::<Vec<_>>()
        .join(" · ")
    };
    let source = snapshot
        .source_price_id
        .as_deref()
        .map(|id| {
            format!(
                " · {} {id}@{}",
                if zh { "来源" } else { "source" },
                snapshot
                    .source_price_revision
                    .map_or_else(|| "—".to_owned(), |revision| revision.to_string())
            )
        })
        .unwrap_or_default();
    format!("{text}{source}")
}

impl Statistics {
    pub(crate) fn open_detail(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(item) = self.rows.get(index) {
            self.open_detail_id(item.request_id.clone(), window, cx);
        }
    }

    pub(super) fn open_detail_id(
        &mut self,
        request_id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.detail_request_id = Some(request_id);
        self.detail_scroll.set_offset(point(px(0.), px(0.)));
        self.detail_metadata_scroll
            .set_offset(point(px(0.), px(0.)));
        self.detail_open = true;
        self.request_detail(cx);
        self.open_detail_dialog(window, cx);
    }

    fn request_detail(&mut self, cx: &mut Context<Self>) {
        let Some(request_id) = self.detail_request_id.clone() else {
            return;
        };
        let tag = format!("detail-{}", self.generation);
        let request = crate::stats_backend::command(
            "stats_request_detail",
            &tag,
            json!({"request_id":request_id}),
        );
        self.detail_seq = self
            .backend
            .submit_read(JobKey::Detail(request_id), tag, request);
        self.details = None;
        self.detail_error = None;
        cx.notify();
    }

    fn clear_detail(&mut self, cx: &mut Context<Self>) {
        self.detail_seq = 0;
        self.detail_open = false;
        self.detail_request_id = None;
        self.detail_error = None;
        self.details = None;
        cx.notify();
    }

    fn open_detail_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let owner = cx.entity().downgrade();
        let observer = cx.entity().clone();
        let body = cx.new(|cx| {
            cx.observe(&observer, |_, _, cx| cx.notify()).detach();
            DetailBody {
                owner: owner.clone(),
            }
        });
        window.open_dialog(cx, move |dialog, window, _| {
            let close_owner = owner.clone();
            let height = (window.viewport_size().height - px(64.)).min(px(770.));
            dialog
                .width(px(650.))
                .h(height)
                .p(px(27.))
                .pt(px(21.))
                .pb(px(10.))
                .bg(rgb(PANEL))
                .rounded(px(18.))
                .border_color(rgb(BORDER))
                .margin_top(((window.viewport_size().height - height) / 2. + px(7.)).max(px(16.)))
                .close_button(true)
                .overlay_closable(true)
                .content({
                    let body = body.clone();
                    move |content, _, _| content.gap_0().child(body.clone())
                })
                .on_close(move |_, _, cx| {
                    let _ = close_owner.update(cx, |this, cx| this.clear_detail(cx));
                })
                .on_ok(|_, _, _| true)
        });
    }

    fn detail_content(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        use crate::{stats_dto::Outcome, ui_components::TagKind};
        let height = (window.viewport_size().height - px(64.)).min(px(770.)) - px(33.);
        let body_height = (height - px(66. + 51. + 50.)).max(px(100.));
        let (kind, badge) = self
            .details
            .as_ref()
            .map(|detail| {
                let item = &detail.request;
                let recorded = item.recorded || item.outcome == Outcome::Recorded;
                let kind = if recorded {
                    TagKind::Secondary
                } else {
                    match item.outcome {
                        Outcome::Completed => TagKind::Success,
                        Outcome::InFlight => TagKind::Primary,
                        Outcome::Cancelled | Outcome::Interrupted => TagKind::Warning,
                        _ => TagKind::Danger,
                    }
                };
                let text = if recorded {
                    usage_table::status_text(true, item.outcome, self.zh).into()
                } else {
                    format!(
                        "{} · {}",
                        usage_table::http_text(item.http_status),
                        usage_table::outcome_label(item.outcome, item.error_kind, self.zh)
                    )
                };
                (kind, text)
            })
            .unwrap_or_else(|| {
                if self.detail_error.is_some() {
                    (TagKind::Danger, self.t("读取失败", "Read failed").into())
                } else {
                    (TagKind::Secondary, self.t("读取中…", "Loading…").into())
                }
            });
        let content = if let Some(detail) = &self.details {
            self.detail_rows(detail).into_any_element()
        } else if let Some(error) = &self.detail_error {
            div()
                .flex()
                .flex_col()
                .gap(px(14.))
                .p(px(20.))
                .child(
                    Label::new(self.t("无法读取请求详情", "Unable to read request details"))
                        .text_size(px(15.))
                        .text_color(rgb(0xc94f5e)),
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(rgb(0xc94f5e))
                        .child(error.clone()),
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(rgb(MUTED))
                        .child(self.detail_request_id.clone().unwrap_or_default()),
                )
                .child(
                    stats_button("detail-retry")
                        .outline()
                        .label(self.t("重试", "Retry"))
                        .on_click(cx.listener(|this, _, _, cx| this.request_detail(cx))),
                )
                .into_any_element()
        } else {
            div()
                .p(px(20.))
                .text_color(rgb(MUTED))
                .child(self.t("正在读取…", "Loading…"))
                .into_any_element()
        };
        let body = div()
            .relative()
            .h(body_height)
            .w_full()
            .flex_none()
            .child(
                div()
                    .id("usage-detail-scroll")
                    .h(body_height)
                    .overflow_y_scroll()
                    .track_scroll(&self.detail_scroll)
                    .pr(px(12.))
                    .child(content),
            )
            .child(ScrollableMask::new(Axis::Vertical, &self.detail_scroll))
            .vertical_scrollbar(&self.detail_scroll);
        div().flex().flex_col().w_full().h(height).gap_0()
            .child(div().flex().flex_col().h(px(66.)).flex_none().gap(px(8.))
                .child(div().flex().items_center().justify_between().gap(px(12.)).pr(px(16.))
                    .child(Label::new(self.t("请求详情", "Request detail")).text_size(px(21.))
                        .font_weight(FontWeight::BOLD).text_color(rgb(INK)))
                    .child(super::settings_capture::status_badge(kind, badge).h(px(27.)).min_w(px(137.))))
                .child(Label::new(self.t("仅保存统计摘要，不包含消息内容、工具参数、图片或密钥", "Summary only: no message content, tool arguments, images or keys"))
                    .text_size(px(12.)).text_color(rgb(MUTED)))
                .child(div().h(px(1.)).w_full().bg(rgb(BORDER))))
            .child(body)
            .child(div().flex().flex_col().gap(px(3.)).h(px(51.)).flex_none().p(px(10.))
                .rounded(px(10.)).bg(rgb(0xf4f6ff))
                .child(Label::new(self.t("缓存读写是输入的子集，不能与总 Token 简单相加。", "Cache reads/writes are input subsets; do not add them to total tokens."))
                    .text_size(px(12.)).text_color(rgb(0x4c566d)))
                .child(Label::new(self.t("历史费用按请求发生时的价格快照计算。", "Historical costs use each request's recorded price snapshot."))
                    .text_size(px(11.)).text_color(rgb(MUTED))))
            .child(div().flex().items_end().justify_between().h(px(50.)).flex_none()
                .child(self.detail_metadata_button(cx))
                .child(stats_button("detail-close").outline().h(px(36.)).min_w(px(98.)).rounded(px(9.))
                    .label(self.t("关闭", "Close"))
                    .on_click(cx.listener(|this, _, window, cx| { this.clear_detail(cx); window.close_dialog(cx); }))))
            .into_any_element()
    }

    fn detail_rows(&self, detail: &RequestDetailDto) -> Div {
        let item = &detail.request;
        let metadata = detail_metadata::fields(item, self.zh);
        let tokens = if item.usage_status == crate::stats_dto::UsageStatus::NotApplicable {
            self.t("不适用", "Not applicable").into()
        } else {
            format!(
                "↓{}  ↑{}  R{}  W{}  Σ{}",
                usage_table::exact(item.input_tokens),
                usage_table::exact(item.output_tokens),
                usage_table::exact(item.cache_read_tokens),
                usage_table::exact(item.cache_write_tokens),
                usage_table::exact(item.total_tokens)
            )
        };
        let imported = item.origin.is_imported();
        let cost = if imported {
            format!("{} {}", metadata[4].1, self.t("点", "credits"))
        } else {
            cost_text(item, self.zh)
        };
        let protocol = match item.upstream_protocol.as_deref() {
            Some("openai_responses") => "OpenAI Responses",
            Some("anthropic_messages") => "Anthropic Messages",
            Some("gemini_generate_content") => "Gemini GenerateContent",
            Some("openai_chat") => "OpenAI Chat Completions",
            _ => "—",
        };
        let cache = usage_table::cache_rate(item.cache_read_tokens, item.input_tokens);
        let values = [
            (self.t("请求标识", "Request ID"), item.request_id.clone()),
            (
                if item.time_basis == crate::stats_dto::TimeBasis::MessageTimestamp {
                    self.t("消息时间", "Message timestamp")
                } else {
                    self.t("请求时间", "Started")
                },
                usage_table::timestamp(item.started_at_ms, self.zh),
            ),
            (metadata[0].0, metadata[0].1.clone()),
            (self.t("供应商", "Provider"), item.provider_name.clone()),
            (self.t("上游协议", "Upstream protocol"), protocol.into()),
            (
                self.t("上游模型", "Upstream model"),
                usage_table::model_text(item, self.zh),
            ),
            (
                self.t("请求模型", "Requested model"),
                usage_table::requested_text(item, self.zh),
            ),
            (metadata[1].0, metadata[1].1.clone()),
            (metadata[5].0, metadata[5].1.clone()),
            (
                self.t("状态", "Outcome"),
                format!(
                    "HTTP {} · {}",
                    usage_table::http_text(item.http_status),
                    usage_table::outcome_label(item.outcome, item.error_kind, self.zh)
                ),
            ),
            (self.t("Token", "Tokens"), tokens),
            (
                self.t("缓存命中率", "Cache hit rate"),
                usage_table::percent(cache),
            ),
            (
                if imported {
                    self.t("WorkBuddy 点数", "WorkBuddy credits")
                } else {
                    self.t("代理费用", "Proxy cost")
                },
                cost,
            ),
            (
                self.t("用时", "Latency"),
                format!(
                    "{} {} · {} {} · TTFB {}",
                    self.t("总", "total"),
                    usage_table::duration(item.duration_ms),
                    self.t("首内容", "first content"),
                    usage_table::duration(item.first_content_ms),
                    usage_table::duration(item.ttfb_ms)
                ),
            ),
            (self.t("输出速度", "Output speed"), {
                let rate = usage_table::rate_text(item);
                if rate == "—" {
                    rate
                } else {
                    format!("{rate} tok/s")
                }
            }),
        ];
        div().flex().flex_col().pt(px(12.)).pb(px(12.)).children(
            values
                .into_iter()
                .enumerate()
                .map(|(index, (name, value))| {
                    row(
                        index,
                        name,
                        value,
                        if index == 11 && cache.is_some() {
                            OK
                        } else {
                            INK
                        },
                        false,
                    )
                    .when(index == 6 || index == 9, |row| {
                        row.border_b_1().border_color(rgb(BORDER))
                    })
                }),
        )
    }

    fn detail_metadata_button(&self, cx: &mut Context<Self>) -> AnyElement {
        let owner = cx.entity().downgrade();
        Popover::new("request-detail-metadata")
            .trigger(
                stats_button("detail-metadata")
                    .ghost()
                    .h(px(36.))
                    .label(self.t("更多详情", "More details"))
                    .disabled(self.details.is_none()),
            )
            .content(move |_, window, cx| {
                owner
                    .update(cx, |this, cx| this.detail_metadata_content(window, cx))
                    .unwrap_or_else(|_| div().into_any_element())
            })
            .into_any_element()
    }

    fn detail_metadata_content(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(detail) = &self.details else {
            return div().into_any_element();
        };
        let item = &detail.request;
        let mut fields = detail_metadata::fields(item, self.zh);
        fields.extend([
            (self.t("请求标识", "Request ID"), item.request_id.clone()),
            (
                self.t("请求模型", "Requested model"),
                usage_table::requested_text(item, self.zh),
            ),
            (
                self.t("上游模型", "Upstream model"),
                usage_table::model_text(item, self.zh),
            ),
            (
                self.t("供应商 / 路由", "Provider / route"),
                format!(
                    "{} · {}",
                    item.provider_name,
                    usage_table::route_text(item, self.zh)
                ),
            ),
            (
                self.t("响应声明模型", "Response model"),
                item.response_model.clone().unwrap_or_else(|| "—".into()),
            ),
            (
                self.t("用量", "Usage"),
                format!(
                    "{} · {}",
                    usage_table::usage_label(item.usage_status, self.zh).0,
                    item.usage_reason
                        .map_or_else(|| "—".into(), |reason| format!("{reason}"))
                ),
            ),
            (
                self.t("估算费用", "Estimated cost"),
                cost_text(item, self.zh),
            ),
            (
                self.t("输出速度口径", "Output speed basis"),
                usage_table::rate_hint(item.stream, self.zh).into(),
            ),
        ]);
        if let Some(snapshot) = &detail.price_snapshot {
            fields.push((
                self.t("本次单价", "Unit price used"),
                snapshot_text(snapshot, self.zh),
            ));
        }
        fields.push((
            self.t("记录时间", "Recorded"),
            format!(
                "{} → {}",
                usage_table::timestamp(detail.created_at_ms, self.zh),
                usage_table::timestamp(detail.updated_at_ms, self.zh)
            ),
        ));
        let height = (window.viewport_size().height - px(180.)).clamp(px(220.), px(520.));
        let body_height = height - px(76.);
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .w(px(594.))
            .max_w(window.viewport_size().width - px(64.))
            .h(height)
            .child(
                Label::new(self.t("统计元数据与价格快照", "Metadata & price snapshot"))
                    .text_size(px(16.))
                    .font_weight(FontWeight::BOLD)
                    .text_color(rgb(INK)),
            )
            .child(
                div()
                    .relative()
                    .w_full()
                    .h(body_height)
                    .flex_none()
                    .child(
                        div()
                            .id("detail-metadata-rows")
                            .flex()
                            .flex_col()
                            .h(body_height)
                            .pr(px(12.))
                            .overflow_y_scroll()
                            .track_scroll(&self.detail_metadata_scroll)
                            .children(fields.into_iter().enumerate().map(
                                |(index, (label, value))| row(index, label, value, INK, true),
                            )),
                    )
                    .child(ScrollableMask::new(
                        Axis::Vertical,
                        &self.detail_metadata_scroll,
                    ))
                    .vertical_scrollbar(&self.detail_metadata_scroll),
            )
            .child(
                stats_button("detail-copy-id")
                    .outline()
                    .h(px(32.))
                    .label(self.t("复制请求标识", "Copy request ID"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(detail) = &this.details {
                            cx.write_to_clipboard(ClipboardItem::new_string(
                                detail.request.request_id.clone(),
                            ));
                        }
                    })),
            )
            .into_any_element()
    }
}

struct DetailBody {
    owner: WeakEntity<Statistics>,
}
impl Render for DetailBody {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.owner
            .update(cx, |this, cx| this.detail_content(window, cx))
            .unwrap_or_else(|_| div().into_any_element())
    }
}
