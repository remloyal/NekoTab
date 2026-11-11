use dioxus::prelude::*;

use crate::components::tick::use_tick;
use crate::timefmt::{format_clock, weekday_name};

/// 首页大时钟。
///
/// SSR 阶段无法得知"现在"，因此首屏渲染空串；挂载后由 [`use_tick`] 每秒刷新，
/// 这样服务端与客户端首次渲染结果一致，不会 hydration 失配。
///
/// **格式化必须在渲染里做，不能塞进 `use_future`。** `use_future` 只跑一次，
/// 闭包捕获的是挂载那一刻的 props；把 `format_clock` 之类的调用放进去的话，
/// 设置里改了 12 小时制 / 显示秒 / 农历，大时钟要刷新页面才变（网格里的时钟
/// 组件反而是对的——它一直按这条约定写）。
#[component]
pub fn Clock(show_lunar: bool, show_day: bool, hour12: bool, show_seconds: bool) -> Element {
    let tick = use_tick();

    // 还没打上第一拍（SSR、或客户端刚挂载）时全部空串，两边不会失配
    let (time, date, meta) = match tick() {
        Some(now) => {
            let time = format_clock(now.hours, now.minutes, now.seconds, hour12, show_seconds);
            let date = format!(
                "{}年{}月{}日 {}",
                now.year,
                now.month,
                now.day,
                weekday_name(now.weekday)
            );

            // 农历与"第几天/第几周"按需拼接，都是纯计算，不额外发请求
            let mut parts: Vec<String> = Vec::new();
            if show_lunar {
                if let Some(lunar) = crate::lunar::solar_to_lunar(now.year, now.month, now.day) {
                    parts.push(lunar.full_name());
                }
            }
            if show_day {
                parts.push(format!(
                    "第 {} 天 · 第 {} 周",
                    crate::lunar::day_of_year(now.year, now.month, now.day),
                    crate::lunar::iso_week(now.year, now.month, now.day)
                ));
            }

            (time, date, parts.join(" · "))
        }
        None => (String::new(), String::new(), String::new()),
    };

    rsx! {
        header { class: "clock",
            div { class: "clock-time", "{time}" }
            div { class: "clock-date", "{date}" }
            div { class: "clock-meta", "{meta}" }
        }
    }
}
