use dioxus::prelude::*;

use crate::models::Group;

/// 批量打标签 / 去标签的表单（弹窗内容）。
#[component]
pub fn BatchTagForm(
    count: usize,
    on_submit: EventHandler<(String, String)>,
    on_cancel: EventHandler<()>,
) -> Element {
    let mut tags = use_signal(String::new);

    let can_submit = !tags().trim().is_empty();

    rsx! {
        form {
            class: "form",
            onsubmit: move |evt| {
                evt.prevent_default();
                if !tags().trim().is_empty() {
                    on_submit.call((tags(), "add".to_string()));
                }
            },

            p { class: "confirm-msg", "对选中的 {count} 个书签打标签。已有标签不受影响。" }

            label { class: "field",
                span { class: "field-label", "标签" }
                input {
                    class: "input",
                    autofocus: true,
                    placeholder: "逗号分隔，如：常用, 工具",
                    value: "{tags}",
                    oninput: move |e| tags.set(e.value()),
                }
                p { class: "field-hint", "「添加」会把标签并进去；「移除」只删这几个标签。" }
            }

            div { class: "form-actions",
                button {
                    class: "btn-ghost",
                    r#type: "button",
                    onclick: move |_| on_cancel.call(()),
                    "取消"
                }
                button {
                    class: "btn-ghost",
                    r#type: "button",
                    disabled: !can_submit,
                    onclick: {
                        let value = tags();
                        move |_| {
                            if !value.trim().is_empty() {
                                on_submit.call((value.clone(), "remove".to_string()));
                            }
                        }
                    },
                    "移除"
                }
                button {
                    class: "btn-primary",
                    r#type: "submit",
                    disabled: !can_submit,
                    "添加"
                }
            }
        }
    }
}

/// 批量移动到分组（弹窗内容）：点一下即移动，不再确认。
#[component]
pub fn BatchMoveList(
    groups: Vec<Group>,
    count: usize,
    on_pick: EventHandler<Option<i64>>,
    on_cancel: EventHandler<()>,
) -> Element {
    rsx! {
        div { class: "form",
            p { class: "confirm-msg", "把选中的 {count} 个书签移到：" }

            div { class: "move-list",
                button {
                    class: "move-item",
                    r#type: "button",
                    onclick: move |_| on_pick.call(None),
                    span { class: "group-icon group-icon-sm", "—" }
                    span { class: "group-name", "未分组" }
                }
                for group in groups {
                    button {
                        key: "{group.id}",
                        class: "move-item",
                        r#type: "button",
                        onclick: {
                            let id = group.id;
                            move |_| on_pick.call(Some(id))
                        },
                        {super::icons::group_icon(&group.icon, &group.name, "group-icon-sm")}
                        span { class: "group-name", "{group.name}" }
                    }
                }
            }

            div { class: "form-actions",
                button {
                    class: "btn-ghost",
                    r#type: "button",
                    onclick: move |_| on_cancel.call(()),
                    "取消"
                }
            }
        }
    }
}
