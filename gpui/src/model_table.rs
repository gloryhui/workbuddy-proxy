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
use gpui_kit::prelude::StatefulInteractiveElement as _;
use gpui_kit::*;
use serde_json::Value;

pub const EDIT_COLUMN_WIDTH: Pixels = px(48.);

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
                if self.zh { "模型 ID" } else { "Model ID" }
            },
        )
        .width(self.column_width)
        .min_width(px(120.))
    }
    fn render_tr(
        &mut self,
        i: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> Stateful<Div> {
        let owner = self.owner.clone();
        let selected = self.selection.rows.contains(&i);
        div()
            .id(("model", i))
            .aria_selected(selected)
            .accessibility_id(format!("model-{i}"))
            .bg(if selected {
                cx.theme().table_active
            } else {
                cx.theme().background
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
            return Button::new(("model-edit", i))
                .small()
                .ghost()
                .icon(Icon::default().path("icons/model-edit.svg"))
                .tooltip(label)
                .accessibility_label(label)
                .disabled(self.busy)
                .on_click(move |_, window, cx| {
                    cx.stop_propagation();
                    let _ = owner.update(cx, |this, cx| this.edit_model_row(i, window, cx));
                })
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
            .w_full()
            .min_w_0()
            .overflow_hidden()
            .tooltip(move |window, cx| Tooltip::new(full.clone()).build(window, cx))
            .child(text)
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
            .items_center()
            .justify_center()
            .child(gpui_kit::component::label::Label::new(if self.zh {
                "获取模型，或点击 + 手动添加"
            } else {
                "Fetch models or use + to add one"
            }))
    }
}
