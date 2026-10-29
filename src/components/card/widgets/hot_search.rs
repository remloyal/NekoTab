use dioxus::prelude::*;

#[cfg(feature = "web")]
use crate::backend::{error_text, get_hot_search};
use crate::models::{HotItem, HotSearchConfig, HOT_SOURCES};

/// 十分钟刷新一次（与服务端缓存同节奏）。
#[cfg(feature = "web")]
const REFRESH_SECS: u32 = 600;

/// 「热搜」组件的卡片正文。
///
/// 数据由服务端代取并解析（见 `src/hot.rs`），这里只负责画与定时刷新。
/// 条目用 `span` 而不是 `<a>`：卡片本身是 `<a>`，嵌套链接会被浏览器拆开；
/// 点条目时走 `dom::open_url` 手动开新标签页。
#[component]
pub fn HotSearchWidget(config: HotSearchConfig) -> Element {
    let source = config.source.clone();
    let label = HOT_SOURCES
        .iter()
        .find(|(id, _)| *id == source)
        .map(|(_, name)| *name)
        .unwrap_or("热搜");

    #[allow(unused_mut)]
    let mut items = use_signal(Vec::<HotItem>::new);
    #[allow(unused_mut)]
    let mut error = use_signal(|| Option::<String>::None);

    #[cfg(feature = "web")]
    {
        let source = source.clone();
        use_future(move || {
            let source = source.clone();
            async move {
                loop {
                    match get_hot_search(source.clone()).await {
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
        div { class: "widget widget-hot",
            div { class: "hot-head",
                span { class: "hot-title", "热搜" }
                span { class: "hot-source", "{label}" }
            }
            if let Some(message) = message {
                span { class: "widget-error", "{message}" }
            }
            if snapshot.is_empty() {
                if !has_error {
                    span { class: "widget-note-empty", "加载中…" }
                }
            } else {
                div { class: "hot-list",
                    for (index, item) in snapshot.into_iter().take(10).enumerate() {
                        span {
                            key: "{index}",
                            class: "hot-item",
                            title: "{item.title}",
                            onclick: move |evt| {
                                evt.prevent_default();
                                evt.stop_propagation();
                                crate::dom::open_url(&item.url, true);
                            },
                            onpointerdown: move |evt| evt.stop_propagation(),
                            span { class: "hot-rank", "{index + 1}" }
                            span { class: "hot-text", "{item.title}" }
                        }
                    }
                }
            }
        }
    }
}
