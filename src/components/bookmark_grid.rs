use dioxus::prelude::*;

use crate::components::IconTile;
use crate::models::Bookmark;

/// 图标网格。末尾固定一个「添加卡片」块。
///
/// 拖拽状态（正在拖谁）留在网格内部，对外只暴露「把 A 放到 B 的位置」这一个事件，
/// 调用方不必关心中间的 dragstart / drop 细节。
#[component]
#[allow(clippy::too_many_arguments)]
pub fn BookmarkGrid(
    items: Vec<Bookmark>,
    editing: bool,
    on_add: EventHandler<()>,
    on_context: EventHandler<(Bookmark, f64, f64)>,
    on_enter_edit: EventHandler<()>,
    on_edit: EventHandler<Bookmark>,
    on_delete: EventHandler<Bookmark>,
    on_reorder: EventHandler<(i64, i64)>,
) -> Element {
    let mut drag_id = use_signal(|| Option::<i64>::None);

    rsx! {
        div { class: if editing { "grid editing" } else { "grid" },
            for item in items {
                IconTile {
                    key: "{item.id}",
                    bookmark: item.clone(),
                    editing,
                    on_context: {
                        let target = item;
                        move |(x, y): (f64, f64)| on_context.call((target.clone(), x, y))
                    },
                    on_enter_edit,
                    on_edit,
                    on_delete,
                    on_drag_start: move |id: i64| drag_id.set(Some(id)),
                    on_drop_on: move |target: i64| {
                        if let Some(dragged) = drag_id() {
                            drag_id.set(None);
                            on_reorder.call((dragged, target));
                        }
                    },
                }
            }

            button {
                class: "tile tile-add",
                title: "添加卡片",
                onclick: move |_| on_add.call(()),
                div { class: "tile-icon tile-icon-add", "＋" }
                div { class: "tile-label", "添加卡片" }
            }
        }
    }
}
