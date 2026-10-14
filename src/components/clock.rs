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
pub fn Clock(show_lunar: bool, show_day: bool, hour12: bool, show_seconds: bool) -> Element {
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

            let hours = now.get_hours();
            let minutes = now.get_minutes();
            let seconds = now.get_seconds();

            // 12 小时制里 0 点要显示成 12 点；「上午 / 下午」放在最前面，
            // 中文界面下比尾随的 AM/PM 顺眼
            let clock = if hour12 {
                let hour = match hours % 12 {
                    0 => 12,
                    h => h,
                };
                let half = if hours < 12 { "上午" } else { "下午" };
                if show_seconds {
                    format!("{half} {hour:02}:{minutes:02}:{seconds:02}")
                } else {
                    format!("{half} {hour:02}:{minutes:02}")
                }
            } else if show_seconds {
                format!("{hours:02}:{minutes:02}:{seconds:02}")
            } else {
                format!("{hours:02}:{minutes:02}")
            };
            time.set(clock);

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
