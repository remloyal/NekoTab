use dioxus::prelude::*;

/// 底部浮出的短提示条。
///
/// 只用来放"刚做完某件事、可以反悔"这一类提示（比如删除后撤销）。它不是
/// 错误提示——错误走表单里的 `form-error`，不会自己消失。
#[component]
pub fn Toast(
    message: String,
    /// 动作按钮上的文字（如「撤销」）。空串则不显示动作按钮。
    action_label: String,
    on_action: EventHandler<()>,
    on_close: EventHandler<()>,
    /// 位置微调。底栏（编辑态 / 选择态）也占着底部时把它抬高一档。
    style: String,
) -> Element {
    rsx! {
        div {
            class: "toast",
            style: "{style}",
            // 读屏软件会把它当状态播报，不抢焦点
            role: "status",
            aria_live: "polite",
            span { class: "toast-msg", "{message}" }
            if !action_label.is_empty() {
                button {
                    class: "toast-action",
                    r#type: "button",
                    onclick: move |_| on_action.call(()),
                    "{action_label}"
                }
            }
            button {
                class: "toast-close",
                r#type: "button",
                title: "关闭",
                onclick: move |_| on_close.call(()),
                "✕"
            }
        }
    }
}
