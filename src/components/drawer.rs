use dioxus::prelude::*;

/// 右侧抽屉容器：遮罩 + 从右滑入的面板。内容由 `children` 提供。
///
/// `wide` 给设置面板用——它现在是"左菜单 + 右内容"两栏，窄抽屉塞不下。
#[component]
pub fn Drawer(
    title: String,
    #[props(default = false)] wide: bool,
    on_close: EventHandler<()>,
    children: Element,
) -> Element {
    rsx! {
        div {
            class: "drawer-backdrop",
            onclick: move |_| on_close.call(()),
        }
        aside {
            class: if wide { "drawer wide" } else { "drawer" },
            // 吃掉冒泡：避免点抽屉内部时触发遮罩的关闭
            onclick: move |evt| evt.stop_propagation(),
            div { class: "drawer-head",
                h2 { class: "drawer-title", "{title}" }
                button {
                    class: "drawer-close",
                    r#type: "button",
                    title: "关闭",
                    onclick: move |_| on_close.call(()),
                    "✕"
                }
            }
            div { class: "drawer-body", {children} }
        }
    }
}
