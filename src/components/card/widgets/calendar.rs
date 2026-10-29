use dioxus::prelude::*;

use super::use_tick;
use crate::lunar;
use crate::models::CalendarConfig;

/// 闰年判定（公历）。
fn is_leap(year: u32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// 某个月有多少天。
pub(crate) fn days_in_month(year: u32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap(year) => 29,
        2 => 28,
        _ => 0,
    }
}

/// 某天是星期几：0 = 星期日 … 6 = 星期六（Sakamoto 算法）。
pub(crate) fn weekday_of(year: u32, month: u32, day: u32) -> u32 {
    let offsets = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let y = if month < 3 { year - 1 } else { year };
    (y + y / 4 - y / 100 + y / 400 + offsets[(month.max(1) - 1) as usize] + day) % 7
}

/// 月历格子：前面用 `None` 补空，使得 1 号落在正确的星期列。
///
/// `monday_start` 为真时一周从周一开始（周一…周日），否则从周日开始。
pub(crate) fn month_grid(year: u32, month: u32, monday_start: bool) -> Vec<Option<u32>> {
    let first = weekday_of(year, month, 1);
    let lead = if monday_start { (first + 6) % 7 } else { first };
    let mut cells = vec![None; lead as usize];
    for day in 1..=days_in_month(year, month) {
        cells.push(Some(day));
    }
    cells
}

/// 从某个月前后移动 `delta` 个月。
fn shift_month(year: u32, month: u32, delta: i32) -> (u32, u32) {
    let total = year as i64 * 12 + (month as i64 - 1) + delta as i64;
    let year = (total.div_euclid(12)).max(1) as u32;
    let month = (total.rem_euclid(12) + 1) as u32;
    (year, month)
}

/// 「日历」组件的卡片正文。
///
/// 当前日期与月历只在客户端算（服务端不知道"现在"），SSR 阶段渲染一个空壳，
/// 挂载后再填上——与两个时钟组件同一条约定。
#[component]
pub fn CalendarWidget(config: CalendarConfig) -> Element {
    let tick = use_tick();
    // 相对当前月移动几个月；0 表示本月
    let mut offset = use_signal(|| 0i32);

    let Some(now) = tick() else {
        return rsx! { div { class: "widget widget-calendar" } };
    };

    let (year, month) = shift_month(now.year, now.month, offset());
    let cells = month_grid(year, month, config.week_start_monday);
    let today = offset() == 0;

    let labels: [&str; 7] = if config.week_start_monday {
        ["一", "二", "三", "四", "五", "六", "日"]
    } else {
        ["日", "一", "二", "三", "四", "五", "六"]
    };

    // 农历只对"本月"有意义：翻到别的月份时下面显示今天的农历会误导。
    let lunar_text = if config.show_lunar && today {
        lunar::solar_to_lunar(now.year, now.month, now.day)
            .map(|date| format!("今天 · {}{}", date.month_name(), date.day_name()))
            .unwrap_or_default()
    } else {
        String::new()
    };

    rsx! {
        div { class: "widget widget-calendar",
            div { class: "cal-head",
                span {
                    class: "cal-nav",
                    title: "上个月",
                    onclick: move |evt| {
                        evt.prevent_default();
                        evt.stop_propagation();
                        offset.set(offset() - 1);
                    },
                    onpointerdown: move |evt| evt.stop_propagation(),
                    "‹"
                }
                span { class: "cal-title",
                    "{year}年{month}月"
                    if !lunar_text.is_empty() {
                        span { class: "cal-lunar", "{lunar_text}" }
                    }
                }
                span {
                    class: "cal-nav",
                    title: "下个月",
                    onclick: move |evt| {
                        evt.prevent_default();
                        evt.stop_propagation();
                        offset.set(offset() + 1);
                    },
                    onpointerdown: move |evt| evt.stop_propagation(),
                    "›"
                }
            }
            div { class: "cal-week",
                for label in labels {
                    span { class: "cal-weekday", "{label}" }
                }
            }
            div { class: "cal-grid",
                for (index, cell) in cells.into_iter().enumerate() {
                    if let Some(day) = cell {
                        div {
                            key: "{index}",
                            class: if today && day == now.day { "cal-day today" } else { "cal-day" },
                            "{day}"
                        }
                    } else {
                        div { key: "{index}", class: "cal-day empty" }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn month_lengths() {
        assert_eq!(days_in_month(2024, 2), 29);
        assert_eq!(days_in_month(2023, 2), 28);
        assert_eq!(days_in_month(2026, 10), 31);
        assert_eq!(days_in_month(2026, 11), 30);
    }

    #[test]
    fn weekdays_match_known_dates() {
        // 2026-10-29 是星期四（0 = 周日）
        assert_eq!(weekday_of(2026, 10, 29), 4);
        // 2026-01-01 是星期四
        assert_eq!(weekday_of(2026, 1, 1), 4);
    }

    #[test]
    fn grid_aligns_first_day() {
        // 2026-10 的 1 号是星期四：周一起始时前面补 3 格
        let monday = month_grid(2026, 10, true);
        assert_eq!(monday.iter().take_while(|c| c.is_none()).count(), 3);
        assert_eq!(monday[3], Some(1));

        // 周日起始时前面补 4 格
        let sunday = month_grid(2026, 10, false);
        assert_eq!(sunday.iter().take_while(|c| c.is_none()).count(), 4);
        assert_eq!(sunday[4], Some(1));

        assert_eq!(monday.iter().filter(|c| c.is_some()).count(), 31);
    }

    #[test]
    fn month_shifts_across_year() {
        assert_eq!(shift_month(2026, 12, 1), (2027, 1));
        assert_eq!(shift_month(2026, 1, -1), (2025, 12));
        assert_eq!(shift_month(2026, 10, 0), (2026, 10));
    }
}
