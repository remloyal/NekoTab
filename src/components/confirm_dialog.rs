use dioxus::prelude::*;

use crate::components::Modal;

/// 通用二次确认对话框，复用 [`Modal`] 外壳。
#[component]
pub fn ConfirmDialog(
    title: String,
    message: String,
    confirm_label: String,
    on_confirm: EventHandler<()>,
    on_cancel: EventHandler<()>,
) -> Element {
    rsx! {
        Modal { title, on_close: on_cancel,
            p { class: "confirm-msg", "{message}" }
            div { class: "form-actions",
                button {
                    class: "btn-ghost",
                    r#type: "button",
                    onclick: move |_| on_cancel.call(()),
                    "取消"
                }
                button {
                    class: "btn-danger",
                    r#type: "button",
                    onclick: move |_| on_confirm.call(()),
                    "{confirm_label}"
                }
            }
        }
    }
}
