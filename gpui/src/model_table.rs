//! Kit DataTable owns virtualization, cells, headers and scrolling.
//! Application selection retains Shift/Ctrl/Command semantics.
use crate::{state::Selection, ui::Proxy};
use gpui_kit::component::{
    ActiveTheme, Disableable, Icon, Sizable,
    button::{Button, ButtonVariants},
    label::Label,
    table::{Column, TableDelegate, TableState},
    tooltip::Tooltip,
};
use gpui_kit::prelude::{FluentBuilder as _, StatefulInteractiveElement as _};
use gpui_kit::*;
use serde_json::Value;

pub const EDIT_COLUMN_WIDTH: Pixels = px(64.);

pub struct Models {
    pub owner: WeakEntity<Proxy>,
    pub rows: Vec<Value>,
    pub selection: Selection,
    pub zh: bool,
    pub busy: bool,
    pub column_width: Pixels,
}
impl TableDelegate for Models {
    fn columns_count(&self, _: &App) -> usize {
        3
    }
    fn rows_count(&self, _: &App) -> usize {
        self.rows.len()
    }
    fn column(&self, i: usize, _: &App) -> Column {
        if i == 2 {
            return Column::new("edit", if self.zh { "操作" } else { "Actions" })
                .width(EDIT_COLUMN_WIDTH)
                .min_width(EDIT_COLUMN_WIDTH)
                .max_width(EDIT_COLUMN_WIDTH)
                .resizable(false)
                .selectable(false);
        }
        Column::new(
            if i == 0 { "name" } else { "model" },
            if i == 0 {
                if self.zh { "显示名" } else { "Display name" }
            } else {
                if self.zh {
                    "上游模型 ID"
                } else {
                    "Upstream model ID"
                }
            },
        )
        .width(self.column_width)
        .min_width(px(120.))
    }
    fn render_header(
        &mut self,
        _: &mut Window,
        _: &mut Context<TableState<Self>>,
    ) -> Stateful<Div> {
        div()
            .id("model-header")
            .role(Role::Row)
            .a11y_synthetic_children(|tree| tree.parent_node().set_row_index(0))
    }
    fn render_th(
        &mut self,
        col: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let name = self.column(col, cx).name.clone();
        div()
            .id(("model-header-cell", col))
            .role(Role::ColumnHeader)
            .aria_label(name.clone())
            .size_full()
            .a11y_synthetic_children(move |tree| tree.parent_node().set_column_index(col))
            .child(name)
    }
    fn render_tr(
        &mut self,
        i: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> Stateful<Div> {
        let owner = self.owner.clone();
        let selected = self.selection.rows.contains(&i);
        let studio = crate::theme::current_skin() == crate::theme::Skin::StudioIndigo;
        div()
            .id(("model", i))
            .aria_selected(selected)
            .accessibility_id(format!("model-{i}"))
            .when(self.selection.cursor == Some(i), |row| {
                row.aria_active_descendant()
            })
            .a11y_synthetic_children(move |tree| {
                // Kit writes its single-row selection after render_tr returns.
                // This supported post-prepaint hook updates that same native
                // row node, retaining cells, buttons and virtualization.
                let row = tree.parent_node();
                row.set_selected(selected);
                row.set_row_index(i + 1); // Header occupies row zero.
            })
            .bg(if selected {
                cx.theme().table_active
            } else if studio || crate::theme::current_skin() == crate::theme::Skin::Monochrome {
                // Figma 62:308 / 80:646 keep the white / soft-blue row rhythm.
                // Application selection wins; other saved palettes keep their rows.
                rgb([crate::theme::color::WHITE, crate::theme::color::BG_NOTICE][i % 2]).into()
            } else {
                cx.theme().background
            })
            .when(selected && studio, |row| {
                // Studio's selected fill is also the alternating soft blue.
                // Native borders retain a visible selection cue without moving columns.
                row.border_t_1()
                    .border_b_1()
                    .border_color(rgb(crate::theme::palette().primary))
            })
            .on_click(move |event, window, cx| {
                cx.stop_propagation();
                let _ = owner.update(cx, |this, cx| this.click_model(i, event, window, cx));
            })
    }
    fn render_td(
        &mut self,
        i: usize,
        col: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        if col == 2 {
            let owner = self.owner.clone();
            let label = if self.zh {
                "编辑模型"
            } else {
                "Edit model"
            };
            let model = &self.rows[i];
            let name = model["name"]
                .as_str()
                .filter(|value| !value.is_empty())
                .or_else(|| model["model"].as_str())
                .unwrap_or("");
            return div()
                .id(("model-action-cell", i))
                .role(Role::Cell)
                .a11y_synthetic_children(|tree| tree.parent_node().set_column_index(2))
                .child(
                    Button::new(("model-edit", i))
                        .ghost()
                        .with_size(px(28.))
                        .size(px(28.))
                        .p_0()
                        .child(
                            Icon::default()
                                .path("studio/model-edit.svg")
                                .with_size(px(15.)),
                        )
                        .tooltip(label)
                        .accessibility_label(format!("{label}: {name}"))
                        .disabled(self.busy)
                        .on_click(move |_, window, cx| {
                            cx.stop_propagation();
                            let _ = owner.update(cx, |this, cx| this.edit_model_row(i, window, cx));
                        }),
                )
                .into_any_element();
        }
        let row = &self.rows[i];
        let id = row["model"].as_str().unwrap_or("");
        let value = if col == 0 {
            row["name"].as_str().filter(|s| !s.is_empty()).unwrap_or(id)
        } else {
            id
        };
        // Long display names and model IDs are ellipsised inside their column.
        // The full value stays reachable on hover, so trimming the display never
        // hides what was actually configured (FX03), and the model ID keeps the
        // design's monospace code treatment (DESIGN_SPEC §1.2).
        let full = value.to_owned();
        let text = if col == 1 {
            Label::new(value.to_owned())
                .w_full()
                .text_ellipsis()
                .font_family(cx.theme().mono_font_family.clone())
        } else {
            Label::new(value.to_owned()).w_full().text_ellipsis()
        };
        div()
            .id(("model-cell", i * 2 + col))
            .role(Role::Cell)
            .aria_label(value.to_owned())
            .a11y_synthetic_children(move |tree| tree.parent_node().set_column_index(col))
            .w_full()
            .min_w_0()
            .overflow_hidden()
            .tooltip(move |window, cx| Tooltip::new(full.clone()).build(window, cx))
            .child(text.text_size(px(crate::theme::font::BODY)))
            .into_any_element()
    }
    // The three explicit columns fill the container without Kit's 12px trailing cell.
    fn render_last_empty_col(
        &mut self,
        _: &mut Window,
        _: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        div().w(px(0.)).flex_shrink_0()
    }
    fn render_empty(
        &mut self,
        _: &mut Window,
        _: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .px(px(22.))
            .gap(px(8.))
            .rounded(px(10.))
            .bg(rgb(crate::theme::color::MODEL_EMPTY))
            .justify_center()
            .child(
                gpui_kit::component::label::Label::new(if self.zh {
                    "暂无模型"
                } else {
                    "No models yet"
                })
                .text_size(px(13.))
                .font_weight(gpui::FontWeight::SEMIBOLD),
            )
            .child(
                gpui_kit::component::label::Label::new(if self.zh {
                    "先填写供应商名称、API 地址和 Key，然后获取或手动添加模型。"
                } else {
                    "Enter a provider name, API URL and key, then fetch or add models."
                })
                .text_size(px(11.))
                .text_color(rgb(crate::theme::color::TEXT_3))
                .whitespace_normal()
                .h_auto(),
            )
    }
}
