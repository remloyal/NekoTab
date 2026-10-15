use dioxus::prelude::*;

use super::use_tick;
use crate::models::ClockConfig;
use crate::timefmt::{format_clock, weekday_name};

/// 「数字时钟」组件的卡片正文。
///
/// 每秒的跳动与 SSR 空首屏的约定在 [`use_tick`] 里，两个时钟组件共用。
///
/// 排版不在这里算：字号、日期在矮格子里要不要显示，都交给 main.css 的容器查询，
/// 因为卡片能跨格，同一份标记得在 1×1 和 3×3 里都成立。
#[component]
pub fn ClockWidget(config: ClockConfig) -> Element {
    let tick = use_tick();

    // 还没打上第一拍（SSR、或客户端刚挂载）时两边都是空串，不会失配
    let (time, date) = match tick() {
        Some(now) => (
            format_clock(
                now.hours,
                now.minutes,
                now.seconds,
                config.hour12,
                config.show_seconds,
            ),
            format!("{}月{}日 {}", now.month, now.day, weekday_name(now.weekday)),
        ),
        None => (String::new(), String::new()),
    };

    // 12 小时制多出「上午 / 下午」，同样宽度下要小一档字号，否则时间会被卡片裁掉。
    // 这一点 CSS 自己看不出来，所以由这里挂一个类告诉它。
    let class = if config.hour12 {
        "widget widget-clock widget-clock--h12"
    } else {
        "widget widget-clock"
    };

    rsx! {
        div { class: "{class}",
            div { class: "widget-clock-time", "{time}" }
            div { class: "widget-clock-date", "{date}" }
        }
    }
}
