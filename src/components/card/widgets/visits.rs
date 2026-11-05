use dioxus::prelude::*;

#[cfg(feature = "web")]
use crate::backend::{error_text, list_top_visited};
use crate::models::{VisitedItem, VisitsConfig};

/// 一分钟刷新一次。
///
/// 比热搜勤：访问次数是"我自己刚点过"的东西，用户很可能点完回来就想看到
/// 次数变了。而这条查询只是对本地 SQLite 做一次聚合，开销可以忽略。
#[cfg(feature = "web")]
const REFRESH_SECS: u32 = 60;

/// 一次最多列几条。卡片本身有高限制（矮格子里塞不下更多），列多了也是被截掉。
#[cfg(feature = "web")]
const MAX_ITEMS: i64 = 8;

/// 「最常访问」组件的卡片正文。
///
/// 数据由服务端聚合（见 `backend::list_top_visited`），这里只负责画与定时刷新。
/// 条目用 `span` 而不是 `<a>`：卡片本身是 `<a>`，嵌套链接会被浏览器拆开；
/// 点条目时走 `dom::open_url` 手动开新标签页。
#[component]
pub fn VisitsWidget(config: VisitsConfig) -> Element {
    let label = config.days_label();

    #[allow(unused_mut)]
    let mut items = use_signal(Vec::<VisitedItem>::new);
    #[allow(unused_mut)]
    let mut error = use_signal(|| Option::<String>::None);

    #[cfg(feature = "web")]
    {
        let days = config.days;
        use_future(move || {
            let days = days;
            async move {
                loop {
                    match list_top_visited(days, MAX_ITEMS).await {
                        Ok(list) => {
                            items.set(list);
                            error.set(None);
                        }
                        Err(e) => error.set(Some(error_text(&e))),
                    }
                    gloo_timers::future::TimeoutFuture::new(REFRESH_SECS * 1000).await;
                }
            }
        });
    }

    let snapshot = items();
    let message = error();
    let has_error = message.is_some();

    rsx! {
        div { class: "widget widget-visits",
            div { class: "visits-head",
                span { class: "visits-title", "最常访问" }
                span { class: "visits-range", "{label}" }
            }
            if let Some(message) = message {
                span { class: "widget-error", "{message}" }
            }
            if snapshot.is_empty() {
                if !has_error {
                    span { class: "widget-note-empty", "还没有访问记录" }
                }
            } else {
                div { class: "visits-list",
                    for (index, item) in snapshot.into_iter().enumerate() {
                        span {
                            key: "{index}",
                            class: "visits-item",
                            title: "{item.title}（{item.count} 次）",
                            onclick: move |evt| {
                                evt.prevent_default();
                                evt.stop_propagation();
                                crate::dom::open_url(&item.url, true);
                            },
                            onpointerdown: move |evt| evt.stop_propagation(),
                            span { class: "visits-rank", "{index + 1}" }
                            span { class: "visits-text", "{item.title}" }
                            span { class: "visits-count", "{item.count}" }
                        }
                    }
                }
            }
        }
    }
}
