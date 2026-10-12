use dioxus::prelude::*;

#[cfg(feature = "web")]
use crate::backend::get_quote;
use crate::models::Quote;

/// 换一言的间隔（秒）。默认 5 分钟；本地调试可临时调小（例如 10）。
#[cfg(feature = "web")]
const REFRESH_SECS: u64 = 300;

/// 时钟下方的一言行。
///
/// 只在客户端拉取：SSR 阶段不去请求第三方接口，免得首屏被外部依赖拖住，
/// 也保证服务端与客户端首次渲染一致（都是空行，不会 hydration 失配）。
#[component]
pub fn QuoteLine() -> Element {
    // 服务端分支里没有写入，`mut` 只在 web 下需要
    #[cfg_attr(feature = "server", allow(unused_mut))]
    let mut quote = use_signal(|| Option::<Quote>::None);

    use_future(move || async move {
        #[cfg(feature = "web")]
        loop {
            // 取失败就保留上一条：不显示错误、不打断页面
            if let Ok(fresh) = get_quote().await {
                quote.set(Some(fresh));
            }

            let wait_ms = REFRESH_SECS.saturating_mul(1000).min(u32::MAX as u64) as u32;
            gloo_timers::future::TimeoutFuture::new(wait_ms).await;
        }
    });

    rsx! {
        div { class: "quote",
            if let Some(current) = quote() {
                span { class: "quote-text", "「{current.text}」" }
                if !current.source.is_empty() {
                    span { class: "quote-source", "—— {current.source}" }
                }
            }
        }
    }
}
