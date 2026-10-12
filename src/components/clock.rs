use dioxus::prelude::*;

#[cfg(feature = "web")]
const WEEKDAYS: [&str; 7] = [
    "星期日", "星期一", "星期二", "星期三", "星期四", "星期五", "星期六",
];

/// 首页大时钟。
///
/// SSR 阶段无法得知"现在"，因此首屏渲染空串；`use_future` 只在客户端
/// 持续刷新时间，这样服务端与客户端首次渲染结果一致，不会 hydration 失配。
#[component]
pub fn Clock() -> Element {
    // 服务端分支里没有写入，`mut` 只在 web 下需要
    #[cfg_attr(feature = "server", allow(unused_mut))]
    let mut time = use_signal(String::new);
    #[cfg_attr(feature = "server", allow(unused_mut))]
    let mut date = use_signal(String::new);

    use_future(move || async move {
        #[cfg(feature = "web")]
        loop {
            let now = js_sys::Date::new_0();

            time.set(format!(
                "{:02}:{:02}:{:02}",
                now.get_hours(),
                now.get_minutes(),
                now.get_seconds()
            ));

            date.set(format!(
                "{}年{}月{}日 {}",
                now.get_full_year(),
                now.get_month() + 1,
                now.get_date(),
                WEEKDAYS[now.get_day() as usize]
            ));

            gloo_timers::future::TimeoutFuture::new(1000).await;
        }
    });

    rsx! {
        header { class: "clock",
            div { class: "clock-time", "{time}" }
            div { class: "clock-date", "{date}" }
        }
    }
}
