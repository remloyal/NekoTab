use dioxus::prelude::*;

/// 抽屉内部容器的 id：打开时把焦点送进去。
const DRAWER_BODY_ID: &str = "nekotab-drawer-body";

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
            // 无障碍 / 焦点：读屏知道这是一层独立的对话框
            role: "dialog",
            "aria-modal": "true",
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
            div {
                class: "drawer-body",
                id: DRAWER_BODY_ID,
                tabindex: "-1",
                // 同弹窗：把焦点送进去，否则 Tab 还在背后被遮住的页面上走
                onmounted: move |_| crate::dom::focus_first_input(DRAWER_BODY_ID),
                {children}
            }
        }
    }
}
