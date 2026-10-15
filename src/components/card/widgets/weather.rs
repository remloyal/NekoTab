use dioxus::prelude::*;

// 只在客户端调：SSR 阶段不去请求第三方接口
#[cfg(feature = "web")]
use crate::backend::{error_text, get_weather};
use crate::models::{weather_label, Weather, WeatherConfig};

/// 隔多久去取一次（秒）。天气变得慢，十分钟一次足够，也让上游少被敲。
#[cfg(feature = "web")]
const REFRESH_SECS: u64 = 600;

/// 「天气」组件的卡片正文。
///
/// 取数在**客户端**发起、由服务端代取（浏览器直连会被 CORS 挡）。SSR 阶段不去请求
/// 第三方，首屏渲染成占位，于是服务端与客户端首次渲染一致、不会 hydration 失配
/// ——与「一言」同一条约定。
///
/// 与「一言」的区别：**取不到要说出来**。一言取不到就保持空白无所谓，天气卡片空着
/// 只会被当成坏了，所以这里保留了失败文案那一档；但只在还没取到过任何数据时才显示，
/// 已经有数据时留着旧的（出口抖一下不该让卡片变空）。
///
/// **调用方必须按 `config` 挂 `key`**：`use_future` 只跑一次、闭包捕获的是挂载那一刻
/// 的参数，改了地点若不重挂就会一直显示旧城市。见 `card::tile` 的分发。
#[component]
pub fn WeatherWidget(config: WeatherConfig) -> Element {
    // 服务端分支里没有写入，`mut` 只在 web 下需要
    #[cfg_attr(feature = "server", allow(unused_mut))]
    let mut state = use_signal(|| Option::<Result<Weather, String>>::None);

    let location = config.location.clone();
    // 服务端不跑取数循环（下面整个 future 都编译掉了），这个值在那侧没有用武之地
    #[cfg_attr(not(feature = "web"), allow(unused_variables))]
    let fahrenheit = config.fahrenheit;
    // 没填地点是"还没配置"，不是"取不到"——两者在卡片上说不同的话
    let configured = !location.trim().is_empty();

    // 整个循环只在客户端编译：SSR 阶段起一个"每分钟去请求第三方"的 future 既没意义，
    // 也会让"首屏不去碰外部接口"这条约定变得靠不住。
    #[cfg(feature = "web")]
    use_future(move || {
        let location = location.clone();
        async move {
            loop {
                if !location.trim().is_empty() {
                    match get_weather(location.clone(), fahrenheit).await {
                        Ok(fresh) => state.set(Some(Ok(fresh))),
                        Err(e) => {
                            // peek 而不是读信号：这个循环不该订阅自己写的值
                            if state.peek().is_none() {
                                state.set(Some(Err(error_text(&e))));
                            }
                        }
                    }
                }

                let wait = REFRESH_SECS.saturating_mul(1000).min(u32::MAX as u64) as u32;
                gloo_timers::future::TimeoutFuture::new(wait).await;
            }
        }
    });

    rsx! {
        div { class: "widget widget-weather",
            if !configured {
                span { class: "widget-weather-note", "去设置里填个地点" }
            } else {
                match state() {
                    Some(Ok(weather)) => {
                        let (label, icon) = weather_label(weather.code);
                        rsx! {
                            div { class: "widget-weather-row",
                                span { class: "widget-weather-icon", "{icon}" }
                                span { class: "widget-weather-temp", "{weather.temperature.round()}°" }
                            }
                            div { class: "widget-weather-place", "{weather.place}" }
                            div { class: "widget-weather-meta",
                                "{label} {weather.high.round()}° / {weather.low.round()}°"
                            }
                        }
                    }
                    Some(Err(message)) => rsx! {
                        span { class: "widget-weather-note", "{message}" }
                    },
                    // 还没取到（SSR、或首次请求在路上）：占位，不是空白
                    None => rsx! {
                        span { class: "widget-weather-note", "…" }
                    },
                }
            }
        }
    }
}
