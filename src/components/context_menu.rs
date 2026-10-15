use dioxus::prelude::*;

use crate::models::{Bookmark, Group, WIDGET_KINDS};

/// 自定义右键菜单。
///
/// `target` 为 `None` 表示在空白处点开，菜单只提供"添加卡片 / 刷新"；
/// 为 `Some` 时表示在某张卡片上点开，额外提供"打开 / 编辑 / 移到分组 /
/// 批量选择 / 删除"——其中"打开"与"移到分组"只对书签有意义（组件没有网址、
/// 也不进分组），所以按 `is_widget` 收起来，而不是留两个点了没反应的项。
#[component]
pub fn ContextMenu(
    x: f64,
    y: f64,
    target: Option<Bookmark>,
    groups: Vec<Group>,
    on_close: EventHandler<()>,
    on_refresh: EventHandler<()>,
    /// 打开「添加卡片」弹窗，停在网址导航那一页
    on_add: EventHandler<()>,
    /// 打开「添加卡片」弹窗并直接选好这个组件类型，参数是它的 kind
    on_add_widget: EventHandler<String>,
    on_edit: EventHandler<Bookmark>,
    on_delete: EventHandler<Bookmark>,
    /// (书签, 目标分组)；`None` 表示移到「未分组」
    on_move: EventHandler<(Bookmark, Option<i64>)>,
    /// 以这条书签为起点进入批量选择
    on_batch: EventHandler<Bookmark>,
) -> Element {
    // 贴边时把菜单拉回视口内，否则在右下角的卡片上点右键会看到半截菜单
    let (viewport_w, viewport_h) = crate::dom::viewport_size();
    let left = x.min((viewport_w - 200.0).max(8.0)).max(8.0);
    let top = y.min((viewport_h - 260.0).max(8.0)).max(8.0);

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
            style: "left: {left}px; top: {top}px;",

            if let Some(card) = target {
                if !card.is_widget() {
                    li { class: "menu-item",
                        a {
                            class: "menu-link",
                            href: "{card.url}",
                            target: "_blank",
                            rel: "noopener noreferrer",
                            onclick: move |_| on_close.call(()),
                            "在新标签页打开"
                        }
                    }
                }
                li {
                    class: "menu-item",
                    onclick: {
                        let selected = card.clone();
                        move |_| on_edit.call(selected.clone())
                    },
                    if card.is_widget() { "设置该组件" } else { "编辑该书签" }
                }

                if !card.is_widget() {
                    li { class: "menu-item has-sub",
                        span { class: "menu-row", "移到分组" }
                        ul { class: "submenu",
                            li {
                                class: "menu-item",
                                onclick: {
                                    let selected = card.clone();
                                    move |_| on_move.call((selected.clone(), None))
                                },
                                "未分组"
                            }
                            for group in groups {
                                li {
                                    key: "{group.id}",
                                    class: if card.group_id == Some(group.id) {
                                        "menu-item active"
                                    } else {
                                        "menu-item"
                                    },
                                    onclick: {
                                        let selected = card.clone();
                                        let id = group.id;
                                        move |_| on_move.call((selected.clone(), Some(id)))
                                    },
                                    "{group.name}"
                                }
                            }
                        }
                    }
                }

                li {
                    class: "menu-item",
                    onclick: {
                        let selected = card.clone();
                        move |_| on_batch.call(selected.clone())
                    },
                    "批量选择（从这条开始）"
                }

                li {
                    class: "menu-item danger",
                    onclick: {
                        let selected = card.clone();
                        move |_| on_delete.call(selected.clone())
                    },
                    if card.is_widget() { "删除该组件（可还原）" } else { "删除该书签（可还原）" }
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
                    // 组件：加一个就多一项，不必再写第二处
                    for (kind, label, _) in WIDGET_KINDS {
                        li {
                            key: "{kind}",
                            class: "menu-item",
                            onclick: move |_| on_add_widget.call(kind.to_string()),
                            "{label}"
                        }
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
