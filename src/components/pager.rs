use dioxus::prelude::*;

/// 底部分页条。只有一页时不渲染。
///
/// 书签上百条后一屏到底会滚很久，翻页把可见范围切开；拖拽排序仍然作用在
/// 全量顺序上（见 `home.rs` 里的合并逻辑），所以翻页不影响排序语义。
#[component]
pub fn Pager(
    page: usize,
    page_count: usize,
    /// 每页条数，用于"第 3 / 8 页"这类提示
    page_size: usize,
    total: usize,
    on_jump: EventHandler<usize>,
) -> Element {
    if page_count <= 1 {
        return rsx! {};
    }

    let current = page + 1;
    let range_start = page * page_size + 1;
    let range_end = (page * page_size + page_size).min(total);

    rsx! {
        nav { class: "pager",
            button {
                class: "pager-btn",
                r#type: "button",
                title: "上一页",
                // 拖拽卡片到这个按钮上会翻页（跨页拖拽），见 CardGrid
                "data-page-flip": "prev",
                onclick: move |_| on_jump.call(page.saturating_sub(1)),
                "‹"
            }

            div { class: "pager-dots",
                for index in 0..page_count {
                    button {
                        key: "{index}",
                        class: if index == page { "pager-dot active" } else { "pager-dot" },
                        r#type: "button",
                        title: "第 {index + 1} 页",
                        onclick: move |_| on_jump.call(index),
                    }
                }
            }

            button {
                class: "pager-btn",
                r#type: "button",
                title: "下一页",
                "data-page-flip": "next",
                onclick: move |_| on_jump.call((page + 1).min(page_count - 1)),
                "›"
            }

            span { class: "pager-info", "{range_start}–{range_end} / {total}　第 {current} / {page_count} 页" }
        }
    }
}
