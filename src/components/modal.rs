use dioxus::prelude::*;

/// 弹窗外壳：半透明遮罩 + 居中面板，内容由 `children` 提供。
#[component]
pub fn Modal(title: String, on_close: EventHandler<()>, children: Element) -> Element {
    rsx! {
        div {
            class: "modal-backdrop",
            onclick: move |_| on_close.call(()),
        }
        div {
            class: "modal",
            // 吃掉冒泡：否则弹窗内的点击会传到根节点，
            // 触发「点空白处退出编辑态」，导致保存后就掉出编辑态
            onclick: move |evt| evt.stop_propagation(),
            div { class: "modal-head",
                h2 { class: "modal-title", "{title}" }
                button {
                    class: "modal-close",
                    r#type: "button",
                    title: "关闭",
                    onclick: move |_| on_close.call(()),
                    "✕"
                }
            }
            div { class: "modal-body", {children} }
        }
    }
}
