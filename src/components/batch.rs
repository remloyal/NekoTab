use dioxus::prelude::*;

use crate::models::Group;

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
            p { class: "confirm-msg", "把选中的 {count} 个书签移到（会替换它们现有的分组）：" }

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
