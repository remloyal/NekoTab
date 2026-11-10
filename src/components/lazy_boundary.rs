use dioxus::prelude::*;

/// 懒加载组件的局部挂起边界。
///
/// `#[component(lazy)]` 展开后会先 `use_resource(...).suspend()?` 等分包 chunk 到位，
/// 首次打开必然挂起一次。本应用没有别处写 `SuspenseBoundary`，于是那次挂起会冒泡到
/// 框架在根节点自动插的默认边界——它的 fallback 是一个空 placeholder，效果是**整个
/// 页面先被拆掉、chunk 到了再整棵重建**：一眨眼的白屏，外加网格里所有站点图标重新
/// 加载（`<img>` 是新节点，`data-ok` 回到 0）。
///
/// 所以每个懒加载组件的使用处都得包一层，把挂起关在这一块里：打开设置抽屉、切到
/// 「背景」/「分组管理」时，闪烁与图标重载就只发生在该组件自己的位置上。
///
/// 边界本身不产生 DOM 节点，包与不包不影响布局。
#[component]
pub fn LazyBoundary(children: Element) -> Element {
    rsx! {
        SuspenseBoundary {
            fallback: |_| rsx! {
                div { class: "state state-loading", "加载中…" }
            },
            {children}
        }
    }
}
