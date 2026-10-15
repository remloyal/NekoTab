use dioxus::prelude::*;

use super::{bookmark_form::BookmarkForm, widget_form::WidgetForm};
use crate::components::icons;
use crate::models::{Bookmark, Group, WIDGET_KINDS};

/// 「添加卡片」弹窗的标签页。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddTab {
    Bookmark,
    Widget,
}

/// `(标签页, 菜单文字)`。顺序即标签的显示顺序。
const TABS: [(AddTab, &str); 2] = [(AddTab::Bookmark, "网址导航"), (AddTab::Widget, "小组件")];

/// 「添加卡片」弹窗：先选要加什么，再填对应的表单。
///
/// 原来"加书签"与"加组件"是两条各自独立的入口——网格末尾的 ＋ 只会加书签，组件藏在
/// 右键的二级菜单里。收成一个弹窗之后入口只剩一个，而种类是可枚举的：加第三类卡片
/// 只要往 [`TABS`] 和 [`crate::models::WIDGET_KINDS`] 里各加一行。
///
/// 已经存在的组件在面板上标成"已添加"，点进去直接是它的设置表单。**不是**把它置灰：
/// 置灰只会让人卡在"看到它、点不动、也不知道去哪改"上。服务端的去重仍然在（另一个
/// 标签页/另一次会话里仍可能撞上），那道提示是兜底，不是常规路径。
#[component]
pub fn AddCardDialog(
    /// 打开时停在哪个标签页
    initial_tab: AddTab,
    /// 打开时已经选中的组件类型（从右键菜单直接点某个组件进来时带着）
    initial_kind: Option<String>,
    groups: Vec<Group>,
    /// 新增书签的默认分组（跟着当前筛选走）
    default_group: Option<i64>,
    /// 已存在的组件卡片：面板据此显示"已添加"，并把点击导向它们的设置表单
    existing_widgets: Vec<Bookmark>,
    on_close: EventHandler<()>,
    on_saved: EventHandler<()>,
) -> Element {
    let mut tab = use_signal(|| initial_tab);
    // 选中的组件类型；`None` 表示还停在选卡面板上
    let mut kind = use_signal(|| initial_kind);

    rsx! {
        div { class: "card-tabs",
            for (value, label) in TABS {
                button {
                    key: "{label}",
                    class: if tab() == value { "tab active" } else { "tab" },
                    r#type: "button",
                    onclick: move |_| {
                        tab.set(value);
                        // 切走再回来时回到选卡面板，而不是停在上次那个组件的表单上
                        kind.set(None);
                    },
                    "{label}"
                }
            }
        }

        match tab() {
            AddTab::Bookmark => rsx! {
                BookmarkForm {
                    bookmark: None,
                    groups: groups.clone(),
                    default_group,
                    on_cancel: move |_| on_close.call(()),
                    on_saved: move |_| on_saved.call(()),
                }
            },

            AddTab::Widget => match kind() {
                // 选好了一种组件：填它的参数
                Some(chosen) => {
                    let label = crate::models::widget_label(&chosen);
                    let existing = existing_widgets
                        .iter()
                        .find(|card| card.kind == chosen)
                        .cloned();

                    rsx! {
                        // key 挂在外层：Dioxus 的 key 只能落在代码块的第一个节点上，
                        // 而需要重挂的其实是整个子树（返回行 + 表单）
                        div { key: "{chosen}", class: "widget-setup",
                            div { class: "picker-head",
                                button {
                                    class: "btn-ghost",
                                    r#type: "button",
                                    onclick: move |_| kind.set(None),
                                    "← 换一种"
                                }
                                span { class: "picker-head-label",
                                    if existing.is_some() {
                                        "{label}（已添加，这里是它的设置）"
                                    } else {
                                        "{label}"
                                    }
                                }
                            }
                            WidgetForm {
                                card: existing,
                                kind: chosen.clone(),
                                on_cancel: move |_| on_close.call(()),
                                on_saved: move |_| on_saved.call(()),
                            }
                        }
                    }
                }

                // 还没选：列出所有可加的组件
                None => rsx! {
                    div { class: "card-picker",
                        for (id, label, icon) in WIDGET_KINDS {
                            {
                                let added = existing_widgets.iter().any(|card| card.kind == id);
                                rsx! {
                                    button {
                                        key: "{id}",
                                        class: "picker-item",
                                        r#type: "button",
                                        title: if added { "已经有一个了，点开它的设置" } else { "添加{label}" },
                                        onclick: move |_| kind.set(Some(id.to_string())),
                                        span { class: "picker-icon",
                                            if let Some(svg) = icons::builtin_svg(icon) {
                                                {svg}
                                            }
                                        }
                                        span { class: "picker-label", "{label}" }
                                        if added {
                                            span { class: "picker-note", "已添加" }
                                        }
                                    }
                                }
                            }
                        }
                    }
                },
            },
        }
    }
}
