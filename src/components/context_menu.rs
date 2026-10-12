use dioxus::prelude::*;

use crate::models::Bookmark;

/// 自定义右键菜单。
///
/// `target` 为 `None` 表示在空白处点开，菜单只提供"添加卡片 / 刷新"；
/// 为 `Some` 时表示在某张卡片上点开，额外提供"打开 / 编辑 / 删除"。
#[component]
pub fn ContextMenu(
    x: f64,
    y: f64,
    target: Option<Bookmark>,
    on_close: EventHandler<()>,
    on_refresh: EventHandler<()>,
    on_add: EventHandler<()>,
    on_edit: EventHandler<Bookmark>,
    on_delete: EventHandler<Bookmark>,
) -> Element {
    rsx! {
        // 透明遮罩：点击或右键任意处关闭菜单
        div {
            class: "menu-backdrop",
            onclick: move |_| on_close.call(()),
            oncontextmenu: move |evt| {
                evt.prevent_default();
                on_close.call(());
            },
        }

        ul {
            class: "context-menu",
            style: "left: {x}px; top: {y}px;",

            if let Some(bookmark) = target {
                li { class: "menu-item",
                    a {
                        class: "menu-link",
                        href: "{bookmark.url}",
                        target: "_blank",
                        rel: "noopener noreferrer",
                        onclick: move |_| on_close.call(()),
                        "在新标签页打开"
                    }
                }
                li {
                    class: "menu-item",
                    onclick: {
                        let selected = bookmark.clone();
                        move |_| on_edit.call(selected.clone())
                    },
                    "编辑该书签"
                }
                li {
                    class: "menu-item danger",
                    onclick: {
                        let selected = bookmark.clone();
                        move |_| on_delete.call(selected.clone())
                    },
                    "删除该书签"
                }
                li { class: "menu-sep" }
            }

            li { class: "menu-item has-sub",
                span { class: "menu-row", "＋ 添加卡片" }
                ul { class: "submenu",
                    li {
                        class: "menu-item",
                        onclick: move |_| on_add.call(()),
                        "网址导航"
                    }
                }
            }

            li {
                class: "menu-item",
                onclick: move |_| on_refresh.call(()),
                "刷新列表"
            }
        }
    }
}
