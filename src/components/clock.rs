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
pub fn Clock(show_lunar: bool, show_day: bool) -> Element {
    // 服务端分支里没有写入，`mut` 只在 web 下需要
    #[cfg_attr(feature = "server", allow(unused_mut))]
    let mut time = use_signal(String::new);
    #[cfg_attr(feature = "server", allow(unused_mut))]
    let mut date = use_signal(String::new);
    #[cfg_attr(feature = "server", allow(unused_mut))]
    let mut meta = use_signal(String::new);

    use_future(move || async move {
        #[cfg(feature = "web")]
        loop {
            let now = js_sys::Date::new_0();
            let (year, month, day) = (
                now.get_full_year(),
                now.get_month() + 1,
                now.get_date(),
            );

            time.set(format!(
                "{:02}:{:02}:{:02}",
                now.get_hours(),
                now.get_minutes(),
                now.get_seconds()
            ));

            date.set(format!(
                "{year}年{month}月{day}日 {}",
                WEEKDAYS[now.get_day() as usize]
            ));

            // 农历与"第几天/第几周"按需拼接，都是纯计算，不额外发请求
            let mut parts: Vec<String> = Vec::new();
            if show_lunar {
                if let Some(lunar) = crate::lunar::solar_to_lunar(year, month, day) {
                    parts.push(lunar.full_name());
                }
            }
            if show_day {
                parts.push(format!(
                    "第 {} 天 · 第 {} 周",
                    crate::lunar::day_of_year(year, month, day),
                    crate::lunar::iso_week(year, month, day)
                ));
            }
            meta.set(parts.join(" · "));

            gloo_timers::future::TimeoutFuture::new(1000).await;
        }
    });

    rsx! {
        header { class: "clock",
            div { class: "clock-time", "{time}" }
            div { class: "clock-date", "{date}" }
            div { class: "clock-meta", "{meta}" }
        }
    }
}
