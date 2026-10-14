use dioxus::prelude::*;

use super::icon_picker::IconPicker;
use super::icons::group_icon;
use crate::backend::{add_group, delete_group, error_text, reorder_groups, update_group};
use crate::models::{Group, NewGroup};

/// 分组名长度上限（与界面上的 `n / 20` 计数一致）。
const NAME_MAX: usize = 20;

/// 分组管理（设置面板的一节）。
///
/// 表单同时承担"新增"和"编辑"：点某一行的 ✎ 会把该组载入表单，按钮变成
/// 「保存修改」。这样图标选择器只需要写一份，用户也只需要认一套操作——
/// 和参考站点那种「新增/编辑后统一提交」的做法一致。
#[component]
pub fn GroupSettings(groups: Vec<Group>, on_changed: EventHandler<()>) -> Element {
    // 表单草稿
    let mut name = use_signal(String::new);
    let mut icon = use_signal(String::new);
    // 正在编辑的分组 id；None 表示"新增"
    let mut editing = use_signal(|| Option::<i64>::None);

    let mut busy = use_signal(|| false);
    let mut error = use_signal(|| Option::<String>::None);

    let ids: Vec<i64> = groups.iter().map(|group| group.id).collect();
    let name_len = name().chars().count();
    let is_edit = editing().is_some();

    let mut reset_form = move || {
        name.set(String::new());
        icon.set(String::new());
        editing.set(None);
        error.set(None);
    };

    let submit = move |_| {
        if busy() {
            return;
        }
        let input = NewGroup {
            name: name(),
            icon: icon(),
        };
        let target = editing();

        spawn(async move {
            busy.set(true);
            error.set(None);

            let result = match target {
                Some(id) => update_group(id, input).await,
                None => add_group(input).await.map(|_| ()),
            };

            match result {
                Ok(()) => {
                    busy.set(false);
                    name.set(String::new());
                    icon.set(String::new());
                    editing.set(None);
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
            p { class: "field-hint",
                "分组就是首页顶部的分类标签页。没有书签的分组不会显示在标签栏里；删除分组不会删书签（组内书签归到「未分组」）。"
            }

            // ---------- 新增 / 编辑表单 ----------
            div { class: "group-form",
                div { class: "field",
                    div { class: "field-label-row",
                        span { class: "field-label",
                            if is_edit { "编辑分组" } else { "新增分组" }
                        }
                        span {
                            class: if name_len > NAME_MAX { "char-count over" } else { "char-count" },
                            "{name_len} / {NAME_MAX}"
                        }
                    }
                    input {
                        class: "input",
                        maxlength: "{NAME_MAX}",
                        placeholder: "分组名称",
                        value: "{name}",
                        oninput: move |e| name.set(e.value()),
                    }
                }

                div { class: "field",
                    span { class: "field-label", "图标" }
                    IconPicker {
                        value: icon(),
                        on_change: move |v: String| icon.set(v),
                    }
                }

                p { class: "field-hint",
                    "图标显示在首页的分类标签页上。内置图标不依赖外网；自定义地址需要对方允许跨站引用。"
                }

                if let Some(message) = error() {
                    p { class: "form-error", "{message}" }
                }

                div { class: "form-actions",
                    if is_edit {
                        button {
                            class: "btn-ghost",
                            r#type: "button",
                            onclick: move |_| reset_form(),
                            "取消编辑"
                        }
                    }
                    button {
                        class: "btn-primary",
                        r#type: "button",
                        disabled: busy() || name().trim().is_empty(),
                        onclick: submit,
                        if busy() {
                            "保存中…"
                        } else if is_edit {
                            "保存修改"
                        } else {
                            "新增分组"
                        }
                    }
                }
            }

            // ---------- 已有分组 ----------
            if groups.is_empty() {
                p { class: "field-hint", "还没有分组。现在所有书签都在「未分组」下。" }
            } else {
                ul { class: "group-list",
                    for (index, group) in groups.iter().enumerate() {
                        GroupRow {
                            key: "{group.id}",
                            group: group.clone(),
                            index,
                            total: groups.len(),
                            ids: ids.clone(),
                            on_changed,
                            on_edit: move |target: Group| {
                                name.set(target.name);
                                icon.set(target.icon.clone());
                                editing.set(Some(target.id));
                                error.set(None);
                            },
                            on_deleted: move |deleted: i64| {
                                // 删掉的正好在表单里编辑时，把表单复位，免得按钮还写着"保存修改"
                                if editing() == Some(deleted) {
                                    reset_form();
                                }
                            },
                        }
                    }
                }
                p { class: "field-hint", "↑ ↓ 调整标签页顺序。" }
            }
        }
    }
}

/// 分组列表的一行。确认删除的状态留在行自己身上，父级只关心"变了"和"要编辑谁"。
#[component]
fn GroupRow(
    group: Group,
    index: usize,
    total: usize,
    ids: Vec<i64>,
    on_changed: EventHandler<()>,
    on_edit: EventHandler<Group>,
    on_deleted: EventHandler<i64>,
) -> Element {
    let mut confirming = use_signal(|| false);
    let id = group.id;

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

    // 两个方向各拿一份顺序：move 闭包会把捕获的值吃进去，不能共用
    let ids_up = ids.clone();
    let ids_down = ids;

    rsx! {
        li { class: "group-row",
            span { class: "group-main",
                {group_icon(&group.icon, &group.name, "group-icon-sm")}
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
                    title: "编辑",
                    onclick: {
                        let target = group.clone();
                        move |_| on_edit.call(target.clone())
                    },
                    "✎"
                }
                if confirming() {
                    button {
                        class: "icon-btn danger",
                        r#type: "button",
                        title: "确认删除",
                        onclick: move |_| {
                            confirming.set(false);
                            on_deleted.call(id);
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
                        title: "删除分组（书签保留）",
                        onclick: move |_| confirming.set(true),
                        "🗑"
                    }
                }
            }
        }
    }
}
