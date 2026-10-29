use dioxus::prelude::*;

use crate::backend::update_widget_config;
use crate::models::TodoConfig;

/// 「待办」组件的卡片正文。
///
/// 卡片上可以直接勾选：`span` 不是表单控件，放在 `<a>` 里不会产生嵌套交互元素，
/// 勾选时 `prevent_default` + `stop_propagation`，既不让外层的 `<a href="#">`
/// 把页面滚回顶部，也不触发卡片的长按/打开设置。
///
/// 新增/删除条目仍走卡片的设置弹窗（点卡片打开）。这样卡片上只保留"一眼看完"的
/// 信息与最常用的勾选动作，输入框与拖拽手势不会互相打架。
#[component]
pub fn TodoWidget(
    card_id: i64,
    config: TodoConfig,
    /// 保存成功后通知父级刷新列表：否则随后点开设置弹窗时，表单读到的还是
    /// 勾选前的旧 `bookmark.config`，一保存就把刚才的勾选覆盖回去。
    on_changed: EventHandler<()>,
) -> Element {
    // 本地状态让勾选立刻有反馈；服务端保存失败也不回滚——它只是待办，
    // 下一次成功保存或刷新页面就会对齐，不值得为此弹错误。
    let mut state = use_signal(|| config);
    // 连续快速勾选时只让最后一次落库：每次勾选递增令牌，300ms 后令牌没变才写。
    let mut save_token = use_signal(|| 0u32);
    let snapshot = state();
    let (done, total) = snapshot.progress();
    let items: Vec<_> = snapshot.items.iter().cloned().enumerate().take(8).collect();
    let more = total.saturating_sub(8);

    let mut toggle = move |index: usize| {
        let mut next = state();
        if let Some(item) = next.items.get_mut(index) {
            item.done = !item.done;
        }
        state.set(next.clone());

        let token = save_token() + 1;
        save_token.set(token);
        spawn(async move {
            #[cfg(feature = "web")]
            {
                gloo_timers::future::TimeoutFuture::new(300).await;
                // 期间又勾了一次：交给更新的那个任务保存
                if save_token() != token {
                    return;
                }
            }
            let latest = state();
            if update_widget_config(card_id, latest.to_json())
                .await
                .is_ok()
            {
                on_changed.call(());
            }
        });
    };

    rsx! {
        div { class: "widget widget-todo",
            div { class: "todo-head",
                span { class: "todo-title", "待办" }
                span { class: "todo-progress", "{done}/{total}" }
            }
            if items.is_empty() {
                span { class: "widget-note-empty", "点卡片添加待办" }
            } else {
                div { class: "todo-list",
                    for (index, item) in items {
                        div {
                            key: "{index}",
                            class: if item.done { "todo-item done" } else { "todo-item" },
                            span {
                                class: "todo-check",
                                role: "checkbox",
                                "aria-checked": if item.done { "true" } else { "false" },
                                title: if item.done { "标记为未完成" } else { "标记为完成" },
                                onclick: move |evt| {
                                    evt.prevent_default();
                                    evt.stop_propagation();
                                    toggle(index);
                                },
                                onpointerdown: move |evt| evt.stop_propagation(),
                                if item.done { "✓" }
                            }
                            span { class: "todo-text", "{item.text}" }
                        }
                    }
                }
                if total > 8 {
                    span { class: "todo-more", "还有 {more} 条" }
                }
            }
        }
    }
}
