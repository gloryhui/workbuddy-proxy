//! Kit DataTable owns virtualization, cells, headers and scrolling.
//! Application selection retains Shift/Ctrl/Command semantics.
use crate::{state::Selection, ui::Proxy};
use gpui_kit::component::{
    ActiveTheme,
    table::{Column, TableDelegate, TableState},
};
use gpui_kit::*;
use serde_json::Value;

pub struct Models {
    pub owner: WeakEntity<Proxy>,
    pub rows: Vec<Value>,
    pub selection: Selection,
    pub zh: bool,
    pub column_width: Pixels,
}
impl TableDelegate for Models {
    fn columns_count(&self, _: &App) -> usize {
        2
    }
    fn rows_count(&self, _: &App) -> usize {
        self.rows.len()
    }
    fn column(&self, i: usize, _: &App) -> Column {
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
        _: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let row = &self.rows[i];
        let id = row["model"].as_str().unwrap_or("");
        let value = if col == 0 {
            row["name"].as_str().filter(|s| !s.is_empty()).unwrap_or(id)
        } else {
            id
        };
        gpui_kit::component::label::Label::new(value.to_owned())
            .w_full()
            .text_ellipsis()
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
