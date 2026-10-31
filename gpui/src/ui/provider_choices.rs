//! Native provider menus: Kit popup, input and virtual list with Studio geometry.
//! The existing SelectEvent contract continues to drive the provider handlers.
use super::*;
use gpui_kit::component::{
    Disableable, IndexPath, StyledExt as _,
    input::Input,
    list::{List, ListDelegate, ListState},
    popover::Popover,
    searchable_list::SearchableListDelegate,
};

gpui_kit::actions!(provider_menu, [Next, Previous, ConfirmChoice, CancelChoice]);

fn search_hint(zh: bool) -> &'static str {
    if zh {
        "搜索供应商、地区或套餐"
    } else {
        "Search providers, regions or plans"
    }
}

pub(super) fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("down", Next, Some("ProviderChoiceMenu")),
        KeyBinding::new("up", Previous, Some("ProviderChoiceMenu")),
        KeyBinding::new("enter", ConfirmChoice, Some("ProviderChoiceMenu")),
        KeyBinding::new("escape", CancelChoice, Some("ProviderChoiceMenu")),
    ]);
}

#[derive(Clone, Copy, PartialEq)]
pub(super) enum MenuKind {
    Protocol,
    Preset,
    Effort,
}

impl MenuKind {
    fn width(self) -> f32 {
        match self {
            Self::Protocol => 361.,
            Self::Preset => 436.,
            Self::Effort => 232.,
        }
    }
    fn height(self) -> f32 {
        match self {
            Self::Protocol => 233.,
            Self::Preset => 377.,
            Self::Effort => 258.,
        }
    }
    fn row_height(self) -> f32 {
        match self {
            Self::Protocol => 53.,
            Self::Preset => 36.,
            Self::Effort => 33.,
        }
    }
}

#[derive(Clone)]
pub(super) struct MenuChoices {
    items: Vec<Choice>,
    kind: MenuKind,
    zh: bool,
}
impl MenuChoices {
    pub(super) fn new(items: Vec<Choice>, kind: MenuKind, zh: bool) -> Self {
        Self { items, kind, zh }
    }
    fn hint(&self, code: &str) -> &'static str {
        match (code, self.zh) {
            ("openai_chat", true) => "标准 Chat 接口",
            ("openai_chat", false) => "Standard Chat API",
            ("openai_responses", true) => "Responses 接口",
            ("openai_responses", false) => "Responses API",
            ("anthropic_messages", true) => "Anthropic 官方协议",
            ("anthropic_messages", false) => "Native Anthropic protocol",
            ("gemini_generate_content", true) => "Google Gemini 接口",
            ("gemini_generate_content", false) => "Google Gemini API",
            _ => "",
        }
    }
    fn filtered(&self, query: &str) -> Vec<Choice> {
        self.items
            .iter()
            .filter(|item| item.matches(query))
            .cloned()
            .collect()
    }
}
// Retain the same typed SelectEvent<...> boundary used by the existing handlers.
impl SearchableListDelegate for MenuChoices {
    type Item = Choice;
    fn items_count(&self, _: usize) -> usize {
        self.items.len()
    }
    fn item(&self, ix: IndexPath) -> Option<&Choice> {
        self.items.get(ix.row)
    }
    fn position<V>(&self, value: &V) -> Option<IndexPath>
    where
        Choice: SelectItem<Value = V>,
        V: PartialEq,
    {
        self.items
            .iter()
            .position(|item| item.value() == value)
            .map(IndexPath::new)
    }
}

pub(super) struct MenuState {
    choices: MenuChoices,
    selected: Option<String>,
    open: bool,
    focus: FocusHandle,
    query: Entity<InputState>,
    list: Entity<ListState<MenuList>>,
    _query_subscription: Subscription,
}
impl EventEmitter<SelectEvent<MenuChoices>> for MenuState {}
impl MenuState {
    pub(super) fn new(choices: MenuChoices, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let query = cx.new(|cx| InputState::new(window, cx).placeholder(search_hint(choices.zh)));
        let owner = cx.entity().downgrade();
        let delegate = MenuList {
            choices: choices.clone(),
            visible: choices.items.clone(),
            checked: None,
            cursor: None,
            owner,
        };
        let list = cx.new(|cx| ListState::new(delegate, window, cx).searchable(false));
        let subscription = cx.subscribe_in(&query, window, |this, _, event, window, cx| {
            if matches!(event, InputEvent::Change) {
                this.filter(window, cx);
            }
        });
        Self {
            choices,
            selected: None,
            open: false,
            focus: cx.focus_handle(),
            query,
            list,
            _query_subscription: subscription,
        }
    }
    pub(super) fn selected_value(&self) -> Option<&String> {
        self.selected.as_ref()
    }
    pub(super) fn set_selected_value(
        &mut self,
        value: &String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.selected = self
            .choices
            .items
            .iter()
            .find(|item| &item.code == value)
            .map(|item| item.code.clone());
        self.filter(window, cx);
        cx.notify();
    }
    pub(super) fn set_items(
        &mut self,
        choices: MenuChoices,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.query.update(cx, |input, cx| {
            input.set_placeholder(search_hint(choices.zh), window, cx)
        });
        self.choices = choices;
        self.filter(window, cx);
        cx.notify();
    }
    fn filter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let visible = self.choices.filtered(self.query.read(cx).value().as_ref());
        let index = self
            .selected
            .as_ref()
            .and_then(|code| visible.iter().position(|item| &item.code == code));
        let index = index
            .or_else(|| (!visible.is_empty()).then_some(0))
            .map(IndexPath::new);
        self.list.update(cx, |list, cx| {
            let delegate = list.delegate_mut();
            delegate.choices = self.choices.clone();
            delegate.visible = visible;
            delegate.checked = self.selected.clone();
            list.set_selected_index(index, window, cx);
            list.scroll_to_selected_item(window, cx);
            cx.notify();
        });
    }
    fn set_open(&mut self, open: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.open = open;
        if open {
            self.query
                .update(cx, |input, cx| input.set_value("", window, cx));
            self.filter(window, cx);
        }
        cx.notify();
    }
    fn confirm(&mut self, value: String, window: &mut Window, cx: &mut Context<Self>) {
        self.selected = Some(value.clone());
        self.open = false;
        self.focus.focus(window, cx);
        cx.emit(SelectEvent::Confirm(Some(value)));
        cx.notify();
    }
    fn move_cursor(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        self.list.update(cx, |list, cx| {
            let count = list.delegate().visible.len();
            if count == 0 {
                return;
            }
            let row = list
                .selected_index()
                .map(|ix| ix.row as isize)
                .unwrap_or(if delta > 0 { -1 } else { count as isize });
            let next = (row + delta).clamp(0, count as isize - 1) as usize;
            list.set_selected_index(Some(IndexPath::new(next)), window, cx);
            list.scroll_to_selected_item(window, cx);
            cx.notify();
        });
    }
    fn confirm_cursor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let list = self.list.read(cx);
        let value = list
            .selected_index()
            .and_then(|ix| list.delegate().visible.get(ix.row))
            .map(|item| item.code.clone());
        if let Some(value) = value {
            self.confirm(value, window, cx);
        }
    }
    fn popup(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let kind = self.choices.kind;
        let zh = self.choices.zh;
        let height = px(kind.height()).min(window.viewport_size().height - px(16.));
        let panel = div()
            .id("provider-choice-content")
            .key_context("ProviderChoiceMenu")
            .flex()
            .flex_col()
            .w_full()
            .h(height - px(2.))
            .min_h_0()
            .on_action(cx.listener(|this, _: &Next, window, cx| this.move_cursor(1, window, cx)))
            .on_action(
                cx.listener(|this, _: &Previous, window, cx| this.move_cursor(-1, window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &ConfirmChoice, window, cx| this.confirm_cursor(window, cx)),
            )
            .on_action(cx.listener(|this, _: &CancelChoice, _, cx| {
                this.open = false;
                cx.notify();
            }))
            .when(kind == MenuKind::Protocol, |p| {
                p.pt(px(7.)).pb(px(4.)).pl(px(6.)).pr(px(8.))
            })
            .when(kind == MenuKind::Effort, |p| {
                p.pt(px(10.)).pb(px(11.)).pl(px(6.)).pr(px(8.))
            })
            .when(kind == MenuKind::Preset, |p| {
                p.pt(px(11.)).pb(px(12.)).pl(px(10.)).pr(px(12.))
            })
            .when(kind == MenuKind::Preset, |p| {
                p.child(
                    div()
                        .px(px(2.))
                        .pr(px(3.))
                        .mb(px(12.))
                        .flex_shrink_0()
                        .child(
                            Styled::h(Input::new(&self.query).large(), px(41.))
                                .bg(rgb(crate::theme::color::BG_TABLE_HEAD))
                                .prefix(Label::new("⌕").text_size(px(13.)))
                                .aria_label(if zh {
                                    "搜索供应商预设"
                                } else {
                                    "Search provider presets"
                                }),
                        ),
                )
            })
            .when(kind != MenuKind::Protocol, |p| {
                p.child(
                    div()
                        .pl(px(if kind == MenuKind::Preset { 2. } else { 6. }))
                        .mb(px(8.))
                        .h(px(17.))
                        .flex_shrink_0()
                        .child(
                            Label::new(match (kind, zh) {
                                (MenuKind::Preset, true) => "可用预设",
                                (MenuKind::Preset, false) => "Available presets",
                                (_, true) => "思考强度",
                                (_, false) => "Thinking effort",
                            })
                            .text_size(px(12.))
                            .line_height(px(17.))
                            .text_color(rgb(MUTED)),
                        ),
                )
            })
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .child(List::new(&self.list).paddings(Edges::all(px(0.)))),
            );
        panel.into_any_element()
    }
}

pub(super) fn control(
    state: &Entity<MenuState>,
    disabled: bool,
    label: &'static str,
    width: Option<f32>,
    cx: &App,
) -> AnyElement {
    let menu = state.read(cx);
    let studio_field = crate::theme::current_skin() == crate::theme::Skin::StudioIndigo
        && menu.choices.kind != MenuKind::Effort;
    let title = menu
        .selected
        .as_ref()
        .and_then(|code| menu.choices.items.iter().find(|item| &item.code == code))
        .map(|item| item.title.clone())
        .unwrap_or_default();
    let trigger = crate::ui_components::action_button(("provider-menu", state.entity_id()))
        .h(px(if menu.choices.kind == MenuKind::Effort {
            34.
        } else {
            crate::theme::size::INPUT_HEIGHT
        }))
        .w_full()
        .px(px(12.))
        .bg(rgb(crate::theme::color::BG_TABLE_HEAD))
        .border_color(rgb(crate::theme::color::BORDER))
        .when(studio_field, |button| {
            button.rounded(px(crate::theme::radius::PROVIDER_FIELD))
        })
        .track_focus(&menu.focus)
        .disabled(disabled)
        .accessibility_label(format!("{label}: {title}"))
        .child(
            div()
                .flex()
                .w_full()
                .min_w_0()
                .items_center()
                .justify_between()
                .gap(px(8.))
                .child(
                    Label::new(title)
                        .when(studio_field, |label| {
                            label.text_color(rgb(if disabled {
                                crate::theme::color::TEXT_4
                            } else {
                                crate::theme::color::TEXT_2
                            }))
                        })
                        .text_size(px(if menu.choices.kind == MenuKind::Effort {
                            12.
                        } else {
                            13.
                        }))
                        .line_height(px(if menu.choices.kind == MenuKind::Effort {
                            17.
                        } else {
                            19.
                        }))
                        .text_ellipsis(),
                )
                .child(
                    svg()
                        .path(if menu.choices.kind == MenuKind::Effort {
                            "studio/provider-effort-chevron.svg"
                        } else {
                            "studio/provider-chevron.svg"
                        })
                        .size(px(if menu.choices.kind == MenuKind::Effort {
                            14.
                        } else {
                            16.
                        }))
                        .text_color(rgb(crate::theme::color::CONTROL_CARET)),
                ),
        );
    if disabled {
        return div()
            .map(|p| {
                if let Some(w) = width {
                    p.w(px(w))
                } else {
                    p.w_full()
                }
            })
            .child(trigger)
            .into_any_element();
    }
    let owner = state.downgrade();
    let content_owner = owner.clone();
    let focus = if menu.choices.kind == MenuKind::Preset {
        menu.query.focus_handle(cx)
    } else {
        menu.list.focus_handle(cx)
    };
    Popover::new(("provider-menu-popup", state.entity_id()))
        .open(menu.open)
        .offset(px(
            if studio_field && menu.choices.kind == MenuKind::Protocol {
                44.
            } else {
                6.
            },
        ))
        .track_focus(&focus)
        .trigger_style(StyleRefinement {
            size: gpui_kit::SizeRefinement {
                width: Some(width.map(|w| px(w).into()).unwrap_or(relative(1.).into())),
                ..Default::default()
            },
            ..Default::default()
        })
        .trigger(trigger)
        .on_open_change(move |open, window, cx| {
            let _ = owner.update(cx, |menu, cx| menu.set_open(*open, window, cx));
        })
        .w(px(menu.choices.kind.width()))
        .p_0()
        .rounded(px(12.))
        .border_1()
        .border_color(rgb(crate::theme::color::BORDER))
        .bg(rgb(crate::theme::color::BG_BAR))
        .content(move |_, window, cx| {
            content_owner
                .update(cx, |menu, cx| menu.popup(window, cx))
                .unwrap_or_else(|_| div().into_any_element())
        })
        .into_any_element()
}

struct MenuList {
    choices: MenuChoices,
    visible: Vec<Choice>,
    checked: Option<String>,
    cursor: Option<IndexPath>,
    owner: WeakEntity<MenuState>,
}
impl ListDelegate for MenuList {
    type Item = MenuRow;
    fn items_count(&self, _: usize, _: &App) -> usize {
        self.visible.len()
    }
    fn render_item(
        &mut self,
        ix: IndexPath,
        _: &mut Window,
        _: &mut Context<ListState<Self>>,
    ) -> Option<MenuRow> {
        let item = self.visible.get(ix.row)?.clone();
        Some(MenuRow {
            hint: self.choices.hint(&item.code),
            checked: self.checked.as_ref() == Some(&item.code),
            item,
            kind: self.choices.kind,
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
        let value = self
            .cursor
            .and_then(|ix| self.visible.get(ix.row))
            .map(|item| item.code.clone());
        if let Some(value) = value {
            let owner = self.owner.clone();
            cx.defer_in(window, move |_, window, cx| {
                let _ = owner.update(cx, |menu, cx| menu.confirm(value, window, cx));
            });
        }
    }
    fn cancel(&mut self, _: &mut Window, cx: &mut Context<ListState<Self>>) {
        let owner = self.owner.clone();
        cx.defer(move |cx| {
            let _ = owner.update(cx, |menu, cx| {
                menu.open = false;
                cx.notify();
            });
        });
    }
    fn render_empty(
        &mut self,
        _: &mut Window,
        _: &mut Context<ListState<Self>>,
    ) -> impl IntoElement {
        div()
            .p(px(12.))
            .text_color(rgb(MUTED))
            .child(if self.choices.zh {
                "没有匹配的预设"
            } else {
                "No matching presets"
            })
    }
}
#[derive(IntoElement)]
struct MenuRow {
    item: Choice,
    hint: &'static str,
    kind: MenuKind,
    checked: bool,
    cursor: bool,
}
impl Selectable for MenuRow {
    fn selected(mut self, selected: bool) -> Self {
        self.cursor = selected;
        self
    }
    fn is_selected(&self) -> bool {
        self.cursor
    }
}
impl RenderOnce for MenuRow {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let studio_protocol = self.kind == MenuKind::Protocol
            && crate::theme::current_skin() == crate::theme::Skin::StudioIndigo;
        let tint = if studio_protocol {
            crate::theme::color::ACCENT_TINT
        } else {
            crate::theme::palette().tint
        };
        let ink = if self.checked {
            crate::theme::palette().dark
        } else if self.kind == MenuKind::Effort {
            crate::theme::color::TEXT_2
        } else {
            INK
        };
        let text = div()
            .flex()
            .flex_col()
            .min_w_0()
            .flex_1()
            .gap(px(4.))
            .child(
                Label::new(self.item.title)
                    .text_size(px(if self.kind == MenuKind::Effort {
                        12.
                    } else {
                        13.
                    }))
                    .line_height(px(if self.kind == MenuKind::Effort {
                        17.
                    } else {
                        19.
                    }))
                    .font_weight(if self.checked {
                        FontWeight::BOLD
                    } else if studio_protocol {
                        FontWeight::MEDIUM
                    } else {
                        FontWeight::NORMAL
                    })
                    .text_color(rgb(ink)),
            )
            .when(self.kind == MenuKind::Protocol, |r| {
                r.child(
                    Label::new(self.hint)
                        .text_size(px(11.))
                        .line_height(px(16.))
                        .font_weight(FontWeight::NORMAL)
                        .text_color(rgb(MUTED)),
                )
            });
        let row = div()
            .flex()
            .items_center()
            .justify_between()
            .w_full()
            .h(px(self.kind.row_height()))
            .px(px(if self.kind == MenuKind::Preset {
                12.
            } else {
                13.
            }))
            .rounded(px(9.))
            .when(self.checked || self.cursor, |r| r.bg(rgb(tint)))
            .hover(|r| r.bg(rgb(tint)))
            .child(text)
            .when(self.checked, |r| {
                r.child(
                    Label::new("✓")
                        .w(px(20.))
                        .text_size(px(13.))
                        .line_height(px(20.))
                        .when(studio_protocol, |check| check.font_weight(FontWeight::BOLD))
                        .text_color(rgb(crate::theme::palette().dark)),
                )
            });
        div()
            .h(px(self.kind.row_height() + 2.))
            .pb(px(2.))
            .w_full()
            .child(row)
    }
}
