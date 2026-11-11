use dioxus::prelude::*;

/// 弹窗内部容器的 id：打开时把焦点送进去。
const MODAL_BODY_ID: &str = "nekotab-modal-body";

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
            // 无障碍 / 焦点：读屏知道这是一层独立的对话框
            role: "dialog",
            "aria-modal": "true",
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
            div {
                class: "modal-body",
                id: MODAL_BODY_ID,
                // 焦点得主动送进来：`autofocus` 写在运行时插入的节点上不生效，
                // 否则"打开弹窗后直接打字"没反应，Tab 也还在背后的页面上走。
                tabindex: "-1",
                onmounted: move |_| crate::dom::focus_first_input(MODAL_BODY_ID),
                {children}
            }
        }
    }
}
