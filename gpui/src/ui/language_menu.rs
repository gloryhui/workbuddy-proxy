//! Native language list; the original set_language handler owns all effects.
use super::*;
use gpui_kit::{
    base::{DeferredPopover, GlobalState, Popup},
    component::{
        IndexPath, StyledExt as _,
        list::{List, ListDelegate, ListState},
    },
};

gpui_kit::actions!(language_menu, [Close, ConfirmSpace]);

pub(super) fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("escape", Close, Some("StudioLanguagePopup")),
        KeyBinding::new("space", ConfirmSpace, Some("StudioLanguageList")),
    ]);
}

pub(super) struct State {
    open: bool,
    previous_focus: Option<FocusHandle>,
    deferred: Option<DeferredPopover>,
    list: Entity<ListState<LanguageRows>>,
}
impl State {
    pub(super) fn new(zh: bool, window: &mut Window, cx: &mut Context<Proxy>) -> Self {
        let owner = cx.entity().downgrade();
        let list = cx.new(|cx| {
            ListState::new(
                LanguageRows {
                    items: choices(zh, true),
                    checked: String::new(),
                    cursor: None,
                    owner,
                },
                window,
                cx,
            )
            .searchable(false)
        });
        Self {
            open: false,
            previous_focus: None,
            deferred: None,
            list,
        }
    }
}

impl Proxy {
    pub(super) fn language_menu(&self, cx: &mut Context<Self>) -> AnyElement {
        let was_open = self.language_menu.open;
        crate::ui_components::system_button(
            "language",
            "studio/language.svg",
            self.t("切换界面语言", "Change interface language"),
            self.t("界面语言", "Interface language"),
            self.busy,
        )
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _, window, cx| {
                // Outside dismissal may have closed it on this same press.
                if !this.busy && this.language_menu.open == was_open {
                    this.toggle_language_surface(window, cx);
                }
                cx.stop_propagation();
            }),
        )
        .on_click(cx.listener(|this, event, window, cx| {
            if !this.busy && matches!(event, ClickEvent::Keyboard(_) | ClickEvent::Touch(_)) {
                this.toggle_language_surface(window, cx);
            }
        }))
        .into_any_element()
    }

    fn toggle_language_surface(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.language_menu.open {
            self.close_language_surface(window, cx);
            return;
        }
        self.language_menu.previous_focus = window.focused(cx).or_else(|| Some(self.focus.clone()));
        let items = choices(self.zh, true);
        let checked = self.cfg.language.clone();
        let index = items
            .iter()
            .position(|item| item.code == checked)
            .unwrap_or(0);
        self.language_menu.list.update(cx, |list, cx| {
            list.delegate_mut().items = items;
            list.delegate_mut().checked = checked;
            list.set_selected_index(Some(IndexPath::new(index)), window, cx);
            cx.notify();
        });
        self.language_menu.deferred = Some(GlobalState::register_deferred_popover(cx));
        self.language_menu.open = true;
        self.language_menu.list.focus_handle(cx).focus(window, cx);
        cx.notify();
    }

    fn close_language_surface(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.language_menu.open {
            return;
        }
        self.language_menu.open = false;
        self.language_menu.deferred = None;
        if let Some(focus) = self.language_menu.previous_focus.take() {
            focus.focus(window, cx);
        }
        cx.notify();
    }

    fn confirm_language(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let list = self.language_menu.list.read(cx);
        let code = list
            .selected_index()
            .and_then(|ix| list.delegate().items.get(ix.row))
            .map(|item| item.code.clone());
        if let Some(code) = code {
            self.close_language_surface(window, cx);
            self.set_language(&code, window, cx);
        }
    }

    pub(super) fn language_toolbar(
        &self,
        toolbar: AnyElement,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        // The design aligns to the full toolbar: window8+height36+gap71=115,
        // right32. Popup owns only native anchoring and pointer occlusion; it
        // adds no Enter/Space handler around the other six toolbar buttons.
        Popup::new("studio-language-popup", toolbar)
            .anchor(Anchor::TopRight)
            .offset(px(71.))
            .w(px(crate::theme::size::SYSTEM_TOOLBAR_WIDTH))
            .h(px(crate::theme::size::TOP_ICON_ROW_HEIGHT))
            .when(self.language_menu.open, |popup| {
                popup.content(self.language_panel(cx))
            })
            .into_any_element()
    }

    fn language_panel(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .id("studio-language-panel")
            .role(Role::Dialog)
            .aria_label(self.t("界面语言", "Interface language"))
            .key_context("StudioLanguagePopup")
            .tab_group()
            .w(px(241.))
            .h(px(184.))
            .flex()
            .flex_col()
            .gap_0()
            .border_1()
            .border_color(rgb(crate::theme::color::BORDER))
            .rounded(px(12.))
            .bg(rgb(crate::theme::color::BG_BAR))
            .shadow(vec![BoxShadow {
                color: rgba(0x121c3d21).into(),
                offset: point(px(0.), px(10.)),
                blur_radius: px(24.),
                spread_radius: px(0.),
                inset: false,
            }])
            .on_action(cx.listener(|this, _: &Close, window, cx| {
                this.close_language_surface(window, cx);
                cx.stop_propagation();
            }))
            .on_mouse_down_out(
                cx.listener(|this, _, window, cx| this.close_language_surface(window, cx)),
            )
            .child(
                div()
                    .flex()
                    .h(px(42.))
                    .flex_none()
                    .items_start()
                    .justify_between()
                    .pl(px(15.))
                    .pr(px(5.))
                    .child(
                        Label::new(self.t("界面语言", "Interface language"))
                            .w(px(181.))
                            .mt(px(12.))
                            .text_size(px(14.))
                            .line_height(px(20.))
                            .font_weight(FontWeight::BOLD)
                            .text_color(rgb(INK)),
                    )
                    .child(
                        crate::ui_components::action_button("close-language-menu")
                            .ghost()
                            .w(px(27.))
                            .h(px(27.))
                            .p_0()
                            .border_0()
                            .mt(px(7.))
                            .flex_none()
                            .rounded(px(7.))
                            .bg(rgb(crate::theme::color::BG_TABLE_HEAD))
                            .accessibility_label(self.t("关闭语言菜单", "Close language menu"))
                            .child(
                                Label::new("×")
                                    .text_size(px(16.))
                                    .line_height(px(22.))
                                    .text_color(rgb(MUTED)),
                            )
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.close_language_surface(window, cx)
                            })),
                    ),
            )
            .child(
                div()
                    .id("language-list-space")
                    .key_context("StudioLanguageList")
                    .h(px(135.))
                    .flex_none()
                    .pl(px(8.))
                    .on_action(cx.listener(|this, _: &ConfirmSpace, window, cx| {
                        this.confirm_language(window, cx);
                        cx.stop_propagation();
                    }))
                    .child(
                        div().w(px(221.)).h_full().child(
                            List::new(&self.language_menu.list).paddings(Edges::all(px(0.))),
                        ),
                    ),
            )
            .into_any_element()
    }
}

struct LanguageRows {
    items: Vec<Choice>,
    checked: String,
    cursor: Option<IndexPath>,
    owner: WeakEntity<Proxy>,
}
impl ListDelegate for LanguageRows {
    type Item = LanguageRow;
    fn items_count(&self, _: usize, _: &App) -> usize {
        self.items.len()
    }
    fn render_item(
        &mut self,
        ix: IndexPath,
        _: &mut Window,
        _: &mut Context<ListState<Self>>,
    ) -> Option<LanguageRow> {
        let item = self.items.get(ix.row)?;
        Some(LanguageRow {
            title: item.title.clone(),
            checked: item.code == self.checked,
            cursor: false,
        })
    }
    fn set_selected_index(
        &mut self,
        index: Option<IndexPath>,
        _: &mut Window,
        _: &mut Context<ListState<Self>>,
    ) {
        self.cursor = index;
    }
    fn confirm(&mut self, _: bool, window: &mut Window, cx: &mut Context<ListState<Self>>) {
        let Some(code) = self
            .cursor
            .and_then(|ix| self.items.get(ix.row))
            .map(|item| item.code.clone())
        else {
            return;
        };
        let owner = self.owner.clone();
        cx.defer_in(window, move |_, window, cx| {
            let _ = owner.update(cx, |this, cx| {
                this.close_language_surface(window, cx);
                this.set_language(&code, window, cx);
            });
        });
    }
    fn cancel(&mut self, window: &mut Window, cx: &mut Context<ListState<Self>>) {
        let owner = self.owner.clone();
        cx.defer_in(window, move |_, window, cx| {
            let _ = owner.update(cx, |this, cx| this.close_language_surface(window, cx));
        });
    }
}
#[derive(IntoElement)]
struct LanguageRow {
    title: SharedString,
    checked: bool,
    cursor: bool,
}
impl Selectable for LanguageRow {
    fn selected(mut self, selected: bool) -> Self {
        self.cursor = selected;
        self
    }
    fn is_selected(&self) -> bool {
        self.cursor
    }
}
impl RenderOnce for LanguageRow {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let palette = crate::theme::palette();
        let color = if self.checked {
            palette.dark
        } else {
            crate::theme::color::TEXT_2
        };
        div().w_full().h(px(45.)).pb(px(5.)).child(
            div()
                .w_full()
                .h(px(40.))
                .rounded(px(9.))
                .bg(rgb(if self.checked || self.cursor {
                    palette.tint
                } else {
                    crate::theme::color::BG_BAR
                }))
                .hover(|row| row.bg(rgb(palette.tint)))
                .child(
                    div()
                        .flex()
                        .w_full()
                        .h_full()
                        .items_start()
                        .pl(px(13.))
                        .pr(px(12.))
                        .pt(px(10.))
                        .gap(px(2.))
                        .child(
                            Label::new(self.title)
                                .w(px(177.))
                                .h(px(20.))
                                .mt(px(2.))
                                .text_size(px(13.))
                                .line_height(px(18.))
                                .text_color(rgb(color))
                                .font_weight(if self.checked {
                                    FontWeight::BOLD
                                } else {
                                    FontWeight::NORMAL
                                }),
                        )
                        .child(
                            Label::new(if self.checked { "✓" } else { "" })
                                .w(px(17.))
                                .h(px(20.))
                                .text_size(px(12.))
                                .line_height(px(17.))
                                .text_color(rgb(color)),
                        ),
                ),
        )
    }
}
