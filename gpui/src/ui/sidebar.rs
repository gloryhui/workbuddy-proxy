//! Studio V2's fixed navigation and independently scrolling supplier rail.
use super::*;
use crate::theme::{color, size};
use gpui_kit::component::Disableable;
use gpui_kit::component::button::ButtonCustomVariant;

fn navigation(
    id: &'static str,
    icon: &'static str,
    label: &'static str,
    active: bool,
    cx: &App,
) -> Button {
    Button::new(id)
        .custom(crate::ui_components::sidebar_variant(active, cx))
        .with_size(px(size::NAV_ROW_HEIGHT))
        .w_full()
        .h(px(size::NAV_ROW_HEIGHT))
        .p_0()
        .rounded(px(11.))
        .toggled(active)
        .accessibility_label(label)
        .child(
            div()
                .relative()
                .flex()
                .items_center()
                .w_full()
                .h_full()
                .px(px(14.))
                .gap(px(14.))
                .when(active, |row| {
                    row.child(
                        div()
                            .absolute()
                            .left(px(1.))
                            .top(px(11.))
                            .w(px(3.))
                            .h(px(21.))
                            .rounded(px(2.))
                            .bg(rgb(crate::theme::sidebar_accent())),
                    )
                })
                .child(
                    Icon::default()
                        .path(icon)
                        .with_size(px(size::SIDEBAR_ICON))
                        .text_color(rgb(if active {
                            color::SIDEBAR_TEXT
                        } else {
                            color::SIDEBAR_MUTED
                        })),
                )
                .child(
                    Label::new(label)
                        .text_size(px(13.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(if active {
                            color::SIDEBAR_TEXT
                        } else {
                            color::SIDEBAR_MUTED
                        })),
                ),
        )
}

enum SupplierAction {
    Add,
    Copy,
    Delete,
}

fn action(kind: SupplierAction, label: &'static str, disabled: bool, cx: &App) -> Button {
    let (id, icon, width) = match kind {
        SupplierAction::Add => ("provider-add", "studio/supplier-add.svg", 79.),
        SupplierAction::Copy => ("provider-copy", "studio/supplier-copy.svg", 65.),
        SupplierAction::Delete => ("provider-delete", "studio/supplier-delete.svg", 68.),
    };
    let ink = if matches!(kind, SupplierAction::Delete) {
        color::SIDEBAR_DANGER
    } else {
        color::SIDEBAR_TEXT
    };
    let style = ButtonCustomVariant::new(cx)
        .color(rgb(color::SIDEBAR_HOVER).into())
        .foreground(rgb(ink).into())
        .hover(rgb(color::SIDEBAR_ACTIVE).into())
        .active(rgb(color::SIDEBAR_ACTIVE).into());
    Button::new(id)
        .with_size(px(size::BUTTON_HEIGHT))
        .w(px(width))
        .h(px(size::BUTTON_HEIGHT))
        .p_0()
        .rounded(px(8.))
        .custom(style)
        .when(matches!(kind, SupplierAction::Add), |button| {
            button.primary()
        })
        .disabled(disabled)
        .when(disabled, |button| {
            button
                .bg(rgb(color::SIDEBAR_HOVER))
                .text_color(rgb(ink))
                .opacity(0.45)
        })
        .child(
            div()
                .flex()
                .items_center()
                .justify_center()
                .gap(px(4.))
                .child(
                    Icon::default()
                        .path(icon)
                        .with_size(px(16.))
                        .text_color(rgb(ink)),
                )
                .child(Label::new(label).text_size(px(12.)).text_color(rgb(ink))),
        )
}

impl Proxy {
    pub(super) fn sidebar(&mut self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let compact = window.viewport_size().height < px(800.);
        let selected = self.selected();
        let query = self.provider_search.read(cx).value();
        let visible = crate::provider_filter::filtered(&self.cfg.providers, &self.drafts, &query);
        let count = visible.len();
        let settings = self.statistics.read(cx).is_settings_view();
        let rows = visible
            .into_iter()
            .map(|provider| {
                let id = provider.id.clone();
                let active = self.page == Page::Providers && id == selected;
                let name = crate::provider_filter::display_name(provider, &self.drafts).to_owned();
                let models = self
                    .drafts
                    .providers
                    .get(&id)
                    .map_or(provider.models.len(), |draft| draft.models.len());
                Button::new(SharedString::from(format!("provider-{id}")))
                    .custom(crate::ui_components::sidebar_variant(active, cx))
                    .with_size(px(size::PROVIDER_ROW_HEIGHT))
                    .h(px(size::PROVIDER_ROW_HEIGHT))
                    .w_full()
                    .p_0()
                    .rounded(px(crate::theme::radius::CONTROL))
                    .toggled(active)
                    .disabled(self.busy)
                    .when(self.busy, |button| button.opacity(0.45))
                    .accessibility_label(name.clone())
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .w_full()
                            .min_w_0()
                            .pl(px(13.))
                            .pr(px(9.))
                            .gap(px(9.))
                            .child(
                                Icon::default()
                                    .path(if active {
                                        "studio/supplier-selected.svg"
                                    } else {
                                        "studio/supplier-default.svg"
                                    })
                                    .with_size(px(size::PROVIDER_DOT))
                                    .flex_shrink_0()
                                    .text_color(rgb(if active {
                                        color::SUCCESS
                                    } else {
                                        color::SIDEBAR_MUTED
                                    })),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_ellipsis()
                                    .text_size(px(13.))
                                    .font_weight(if active {
                                        FontWeight::SEMIBOLD
                                    } else {
                                        FontWeight::MEDIUM
                                    })
                                    .text_color(rgb(if active {
                                        color::SIDEBAR_TEXT
                                    } else {
                                        color::SIDEBAR_MUTED
                                    }))
                                    .child(name),
                            )
                            .child(
                                Label::new(models.to_string())
                                    .text_size(px(12.))
                                    .text_color(rgb(color::SIDEBAR_MUTED)),
                            ),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if this.busy {
                            return;
                        }
                        this.switch_page(Page::Providers, window, cx);
                        this.switch_provider(id.clone(), window, cx);
                    }))
                    .into_any_element()
            })
            .collect::<Vec<_>>();

        let brand = div()
            .flex()
            .items_center()
            .flex_shrink_0()
            .h(px(size::SIDEBAR_BRAND))
            .mx(px(8.))
            .gap(px(11.))
            .child(
                img("studio/brand.png")
                    .size(px(size::SIDEBAR_BRAND))
                    .rounded(px(11.))
                    .flex_shrink_0(),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .min_w_0()
                    .gap(px(2.))
                    .child(
                        Label::new("WorkBuddy")
                            .text_color(rgb(color::SIDEBAR_TEXT))
                            .text_size(px(17.))
                            .font_weight(FontWeight::BOLD),
                    )
                    .child(
                        Label::new("PROXY · DESKTOP")
                            .text_size(px(10.))
                            .text_color(rgb(color::SIDEBAR_MUTED)),
                    ),
            );

        let nav = div()
            .flex()
            .flex_col()
            .flex_shrink_0()
            .gap(px(5.))
            .mt(px(6.))
            .child(
                navigation(
                    "nav-statistics",
                    "studio/nav-chart.svg",
                    self.t("数据统计", "Statistics"),
                    self.page == Page::Statistics && !settings,
                    cx,
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    this.switch_page(Page::Statistics, window, cx);
                    this.statistics
                        .update(cx, |page, cx| page.open_workspace(false, cx));
                })),
            )
            .child(
                navigation(
                    "nav-providers",
                    "studio/nav-server.svg",
                    self.t("供应商管理", "Providers"),
                    self.page == Page::Providers,
                    cx,
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    this.providers_expanded = true;
                    this.switch_page(Page::Providers, window, cx);
                    cx.notify();
                })),
            )
            .child(
                navigation(
                    "nav-statistics-settings",
                    "studio/nav-settings.svg",
                    self.t("统计设置", "Statistics settings"),
                    self.page == Page::Statistics && settings,
                    cx,
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    this.switch_page(Page::Statistics, window, cx);
                    this.statistics
                        .update(cx, |page, cx| page.open_workspace(true, cx));
                })),
            );

        let providers = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .mt(px(4.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .flex_shrink_0()
                    .h(px(18.))
                    .px(px(9.))
                    .child(
                        Button::new("toggle-providers")
                            .custom(crate::ui_components::sidebar_variant(false, cx))
                            .with_size(px(18.))
                            .h(px(18.))
                            .p_0()
                            .accessibility_label(
                                self.t("折叠或展开供应商列表", "Collapse or expand providers"),
                            )
                            .tooltip(self.t("折叠或展开供应商列表", "Collapse or expand providers"))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(4.))
                                    .child(
                                        Label::new(self.t("供应商", "Providers"))
                                            .text_color(rgb(color::SIDEBAR_TEXT))
                                            .text_size(px(13.))
                                            .font_weight(FontWeight::SEMIBOLD),
                                    )
                                    .child(
                                        Icon::new(if self.providers_expanded {
                                            IconName::ChevronDown
                                        } else {
                                            IconName::ChevronRight
                                        })
                                        .with_size(px(12.))
                                        .text_color(rgb(color::SIDEBAR_MUTED)),
                                    ),
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.providers_expanded = !this.providers_expanded;
                                cx.notify();
                            })),
                    )
                    .child(
                        Label::new(if query.trim().is_empty() {
                            format!("{} / {}", self.cfg.providers.len(), config::MAX_PROVIDERS)
                        } else {
                            format!(
                                "{} {} / {}",
                                self.t("匹配", "Matches"),
                                count,
                                self.cfg.providers.len()
                            )
                        })
                        .text_size(px(12.))
                        .text_color(rgb(color::SIDEBAR_MUTED)),
                    ),
            )
            .when(self.providers_expanded, |rail| {
                rail.child(div().flex_shrink_0().mt(px(14.)).mx(px(4.)).child(
                    crate::ui_components::sidebar_search(
                        &self.provider_search,
                        self.t("搜索供应商", "Search providers"),
                        self.t("清除供应商搜索", "Clear provider search"),
                    ),
                ))
                .child(
                    div()
                        .id("provider-list")
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_h_0()
                        .mt(px(13.))
                        .mx(px(4.))
                        .gap(px(4.))
                        .overflow_y_scroll()
                        .children(rows)
                        .when(count == 0, |list| {
                            list.child(
                                Label::new(if self.cfg.providers.is_empty() {
                                    self.t("还没有供应商", "No providers yet")
                                } else {
                                    self.t("未找到供应商", "No providers found")
                                })
                                .text_size(px(12.))
                                .text_color(rgb(color::SIDEBAR_MUTED)),
                            )
                        }),
                )
            })
            .when(!self.providers_expanded, |rail| rail.child(div().flex_1()))
            .child(
                div()
                    .h(px(1.))
                    .flex_shrink_0()
                    .mx(px(4.))
                    .mt(px(7.))
                    .bg(rgb(color::SIDEBAR_LINE)),
            )
            .child(self.provider_actions(cx));

        div()
            .flex()
            .flex_col()
            .flex_shrink_0()
            .w(px(size::SIDEBAR_WIDTH))
            .h_full()
            .px(px(16.))
            .pt(px(41.))
            .pb(px(if compact { 16. } else { 22. }))
            .bg(rgb(color::SIDEBAR))
            .text_color(rgb(color::SIDEBAR_TEXT))
            .child(brand)
            .child(
                div()
                    .flex_shrink_0()
                    .h(px(1.))
                    .mx(px(8.))
                    .mt(px(21.))
                    .bg(rgb(color::SIDEBAR_LINE)),
            )
            .child(
                Label::new("WORKSPACE")
                    .text_size(px(10.))
                    .text_color(rgb(color::SIDEBAR_MUTED))
                    .mt(px(24.))
                    .ml(px(10.))
                    .h(px(18.)),
            )
            .child(nav)
            .child(providers)
            .child(self.sidebar_relay(compact, cx))
            .child(
                Label::new(format!("v{}  ·  RUST / GPUI", env!("CARGO_PKG_VERSION")))
                    .text_size(px(10.))
                    .text_color(rgb(color::SIDEBAR_MUTED))
                    .flex_shrink_0()
                    .h(px(20.))
                    .mt(px(if compact { 12. } else { 26. }))
                    .ml(px(8.)),
            )
            .into_any_element()
    }

    fn provider_actions(&self, cx: &mut Context<Self>) -> AnyElement {
        let at_capacity = self.cfg.providers.len() >= config::MAX_PROVIDERS;
        let selected = self.selected();
        let has_selection = self
            .cfg
            .providers
            .iter()
            .any(|provider| provider.id == selected);
        div()
            .flex()
            .items_center()
            .flex_shrink_0()
            .h(px(size::FOOTER_HEIGHT))
            .mt(px(4.))
            .px(px(3.))
            .gap(px(3.))
            .rounded(px(10.))
            .bg(rgb(color::SIDEBAR_HOVER))
            .child(
                action(
                    SupplierAction::Add,
                    self.t("新增", "Add"),
                    self.busy || at_capacity,
                    cx,
                )
                .tooltip(self.t("新增供应商", "Add provider"))
                .accessibility_label(self.t("新增供应商", "Add provider"))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.capture(cx);
                    this.send("add", vec![json!({"command":"add"})], cx);
                })),
            )
            .child(
                action(
                    SupplierAction::Copy,
                    self.t("复制", "Copy"),
                    self.busy || at_capacity || !has_selection,
                    cx,
                )
                .tooltip(self.t("复制供应商", "Duplicate provider"))
                .accessibility_label(self.t("复制供应商", "Duplicate provider"))
                .on_click(cx.listener(|this, _, _, cx| {
                    let mut steps = this.dirty_steps(cx);
                    steps.push(json!({"command":"duplicate","provider_id":this.selected()}));
                    this.send("duplicate", steps, cx);
                })),
            )
            .child(
                action(
                    SupplierAction::Delete,
                    self.t("删除", "Delete"),
                    self.busy || !has_selection,
                    cx,
                )
                .tooltip(self.t("删除供应商", "Delete provider"))
                .accessibility_label(self.t("删除供应商", "Delete provider"))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.delete_open = true;
                    cx.notify();
                })),
            )
            .into_any_element()
    }

    fn sidebar_relay(&self, compact: bool, cx: &mut Context<Self>) -> AnyElement {
        let (dot, status, _) = self.core_indicator();
        let address = if self.proxy_address.is_empty() {
            "—"
        } else {
            self.proxy_address
                .strip_prefix("http://")
                .unwrap_or(&self.proxy_address)
                .trim_end_matches("/v1")
        };
        let row_style = ButtonCustomVariant::new(cx)
            .color(rgb(color::SIDEBAR_HOVER).into())
            .foreground(rgb(color::SIDEBAR_TEXT).into())
            .hover(rgb(color::SIDEBAR_ACTIVE).into())
            .active(rgb(color::SIDEBAR_ACTIVE).into());
        div()
            .flex()
            .flex_col()
            .flex_shrink_0()
            .mx(px(2.))
            .mt(px(10.))
            .p(px(14.))
            .h(px(if compact {
                size::SIDEBAR_STATUS_COMPACT
            } else {
                size::SIDEBAR_STATUS
            }))
            .gap(px(if compact { 4. } else { 10. }))
            .rounded(px(13.))
            .bg(rgb(color::SIDEBAR_HOVER))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .h(px(25.))
                    .flex_shrink_0()
                    .child(
                        Icon::default()
                            .path("studio/relay-activity.svg")
                            .with_size(px(size::SIDEBAR_ICON))
                            .text_color(rgb(dot)),
                    )
                    .child(
                        Label::new(self.t("本地代理", "Local relay"))
                            .text_color(rgb(color::SIDEBAR_TEXT))
                            .text_size(px(12.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .flex_1()
                            .min_w_0()
                            .text_ellipsis(),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .h(px(25.))
                            .px(px(8.))
                            .rounded_full()
                            .bg(rgb(color::SIDEBAR_ACTIVE))
                            .child(
                                Label::new(status)
                                    .text_size(px(11.))
                                    .text_color(rgb(color::SIDEBAR_TEXT)),
                            ),
                    ),
            )
            .child(
                Button::new("sidebar-copy-address")
                    .custom(row_style)
                    .with_size(px(24.))
                    .h(px(24.))
                    .w_full()
                    .p_0()
                    .disabled(self.proxy_address.is_empty())
                    .when(self.proxy_address.is_empty(), |button| {
                        button.bg(rgb(color::SIDEBAR_HOVER)).opacity(0.55)
                    })
                    .accessibility_label(self.t("复制代理地址", "Copy relay address"))
                    .tooltip(self.t("复制代理地址", "Copy relay address"))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(8.))
                            .w_full()
                            .min_w_0()
                            .child(
                                Label::new(address.to_owned())
                                    .text_color(rgb(color::SIDEBAR_TEXT))
                                    .text_size(px(13.))
                                    .flex_1()
                                    .min_w_0()
                                    .text_ellipsis(),
                            )
                            .child(
                                Icon::default()
                                    .path("studio/relay-link.svg")
                                    .with_size(px(15.))
                                    .text_color(rgb(color::SIDEBAR_MUTED)),
                            ),
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(this.proxy_address.clone()))
                    })),
            )
            .into_any_element()
    }
}
