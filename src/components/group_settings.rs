use dioxus::prelude::*;

use crate::backend::{add_group, delete_group, error_text, reorder_groups, update_group};
use crate::models::{Group, NewGroup};

/// 分组管理（主页设置抽屉内容）。
///
/// 原来分类标签页由书签的 tags 去重派生，无法排序、配图标，也无法让一条书签
/// 只属于一个分类。改成显式分组后，这里负责增删改与顺序。
#[component]
pub fn GroupSettings(groups: Vec<Group>, on_changed: EventHandler<()>) -> Element {
    let mut name = use_signal(String::new);
    let mut icon = use_signal(String::new);
    let mut busy = use_signal(|| false);
    let mut error = use_signal(|| Option::<String>::None);

    // 上下移动要用完整的 id 顺序，所以在这里算一次发给每一行
    let ids: Vec<i64> = groups.iter().map(|group| group.id).collect();

    let add = move |_| {
        if busy() {
            return;
        }

        let input = NewGroup {
            name: name(),
            icon: icon(),
        };

        spawn(async move {
            busy.set(true);
            error.set(None);

            match add_group(input).await {
                Ok(_) => {
                    name.set(String::new());
                    icon.set(String::new());
                    busy.set(false);
                    on_changed.call(());
                }
                Err(e) => {
                    error.set(Some(error_text(&e)));
                    busy.set(false);
                }
            }
        });
    };

    rsx! {
        section { class: "panel-section",
            h3 { class: "panel-heading", "分组管理" }

            div { class: "field",
                span { class: "field-label", "新增分组" }
                div { class: "inline-form",
                    input {
                        class: "input",
                        placeholder: "分组名，如：开发",
                        value: "{name}",
                        oninput: move |e| name.set(e.value()),
                    }
                    input {
                        class: "input",
                        placeholder: "图标地址（可选）",
                        value: "{icon}",
                        oninput: move |e| icon.set(e.value()),
                    }
                    button {
                        class: "btn-primary",
                        r#type: "button",
                        disabled: busy(),
                        onclick: add,
                        "添加"
                    }
                }
                p { class: "field-hint",
                    "分组就是首页顶部的分类标签页。没有书签的分组不会显示在标签栏里。"
                }
            }

            if let Some(message) = error() {
                p { class: "form-error", "{message}" }
            }

            if groups.is_empty() {
                p { class: "field-hint", "还没有分组。现在所有书签都在「未分组」下。" }
            } else {
                ul { class: "group-list",
                    for (index, group) in groups.iter().enumerate() {
                        GroupRow {
                            key: "{group.id}",
                            group: group.clone(),
                            index,
                            ids: ids.clone(),
                            on_changed,
                        }
                    }
                }
                p { class: "field-hint",
                    "删除分组不会删书签：组内书签会归到「未分组」。↑ ↓ 调整标签页顺序。"
                }
            }
        }
    }
}

/// 分组列表的一行。行内编辑的状态留在行自己身上，父级只关心"变了"。
#[component]
fn GroupRow(group: Group, index: usize, ids: Vec<i64>, on_changed: EventHandler<()>) -> Element {
    let mut editing = use_signal(|| false);
    let mut draft_name = use_signal(|| group.name.clone());
    let mut draft_icon = use_signal(|| group.icon.clone());
    let mut confirming = use_signal(|| false);
    let mut error = use_signal(|| Option::<String>::None);

    let id = group.id;
    let total = ids.len();

    let save = move |_| {
        let input = NewGroup {
            name: draft_name(),
            icon: draft_icon(),
        };

        spawn(async move {
            match update_group(id, input).await {
                Ok(()) => {
                    editing.set(false);
                    error.set(None);
                    on_changed.call(());
                }
                Err(e) => error.set(Some(error_text(&e))),
            }
        });
    };

    let move_by = move |delta: isize, ids: Vec<i64>| {
        let Some(from) = ids.iter().position(|&x| x == id) else {
            return;
        };
        let to = from as isize + delta;
        if to < 0 || to as usize >= ids.len() {
            return;
        }

        let mut next = ids;
        next.swap(from, to as usize);

        spawn(async move {
            if reorder_groups(next).await.is_ok() {
                on_changed.call(());
            }
        });
    };

    // 两个按钮各拿一份 id 顺序：move 闭包会把捕获的值吃进去，不能共用同一个
    let ids_up = ids.clone();
    let ids_down = ids;

    rsx! {
        li { class: "group-row",
            if editing() {
                div { class: "inline-form",
                    input {
                        class: "input",
                        placeholder: "分组名",
                        value: "{draft_name}",
                        oninput: move |e| draft_name.set(e.value()),
                    }
                    input {
                        class: "input",
                        placeholder: "图标地址（可选）",
                        value: "{draft_icon}",
                        oninput: move |e| draft_icon.set(e.value()),
                    }
                    button {
                        class: "btn-primary",
                        r#type: "button",
                        onclick: save,
                        "保存"
                    }
                    button {
                        class: "btn-ghost",
                        r#type: "button",
                        onclick: move |_| {
                            // 取消时把草稿还原，免得下次进来看到上次没保存的内容
                            draft_name.set(group.name.clone());
                            draft_icon.set(group.icon.clone());
                            editing.set(false);
                        },
                        "取消"
                    }
                }
            } else {
                span { class: "group-main",
                    if group.icon.starts_with("http://") || group.icon.starts_with("https://") {
                        img { class: "group-icon", src: "{group.icon}", alt: "", loading: "lazy" }
                    } else {
                        span { class: "group-icon group-icon-text",
                            "{group.name.chars().next().unwrap_or('?')}"
                        }
                    }
                    span { class: "group-name", "{group.name}" }
                }

                div { class: "group-actions",
                    button {
                        class: "icon-btn",
                        r#type: "button",
                        title: "上移",
                        disabled: index == 0,
                        onclick: move |_| move_by(-1, ids_up.clone()),
                        "↑"
                    }
                    button {
                        class: "icon-btn",
                        r#type: "button",
                        title: "下移",
                        disabled: index + 1 == total,
                        onclick: move |_| move_by(1, ids_down.clone()),
                        "↓"
                    }
                    button {
                        class: "icon-btn",
                        r#type: "button",
                        title: "重命名 / 改图标",
                        onclick: move |_| editing.set(true),
                        "✎"
                    }
                    if confirming() {
                        button {
                            class: "icon-btn danger",
                            r#type: "button",
                            title: "确认删除该分组",
                            onclick: move |_| {
                                confirming.set(false);
                                spawn(async move {
                                    if delete_group(id).await.is_ok() {
                                        on_changed.call(());
                                    }
                                });
                            },
                            "确认删除"
                        }
                        button {
                            class: "icon-btn",
                            r#type: "button",
                            onclick: move |_| confirming.set(false),
                            "取消"
                        }
                    } else {
                        button {
                            class: "icon-btn",
                            r#type: "button",
                            title: "删除分组（书签保留，归到未分组）",
                            onclick: move |_| confirming.set(true),
                            "🗑"
                        }
                    }
                }
            }

            if let Some(message) = error() {
                p { class: "form-error", "{message}" }
            }
        }
    }
}
