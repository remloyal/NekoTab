//! 时间的显示格式化。
//!
//! 单独一个模块，是因为用它的是**两个不同层次**的地方：页面上的大时钟
//! （[`crate::components::clock`]）和网格里的「数字时钟」组件
//! （[`crate::components::card::widgets`]）。放在任何一边都会造出一条反向依赖——
//! 要么卡片去 import 页面装饰，要么页面装饰去 import 卡片。这里是纯计算、没有 UI，
//! 和 `lunar.rs` 属于同一类东西，所以放在 crate 根。
//!
//! 全部只在客户端被调用（"现在几点"服务端不知道），因此服务端编译时整份是死代码。

#[cfg_attr(not(feature = "web"), allow(dead_code))]
const WEEKDAYS: [&str; 7] = [
    "星期日", "星期一", "星期二", "星期三", "星期四", "星期五", "星期六",
];

/// 星期几的中文名。`day` 取 `Date::get_day()` 的口径（0 = 星期日）。
///
/// 越界一律回落到星期日而不是 panic：这个值来自浏览器，宁可显示错一天，
/// 也不能让整页崩掉。
#[cfg_attr(not(feature = "web"), allow(dead_code))]
pub fn weekday_name(day: u32) -> &'static str {
    WEEKDAYS.get(day as usize).copied().unwrap_or(WEEKDAYS[0])
}

/// 把一天中的时刻拼成数字时钟串。
///
/// 12 小时制里 0 点要显示成 12 点；「上午 / 下午」放在最前面，
/// 中文界面下比尾随的 AM/PM 顺眼。
#[cfg_attr(not(feature = "web"), allow(dead_code))]
pub fn format_clock(
    hours: u32,
    minutes: u32,
    seconds: u32,
    hour12: bool,
    show_seconds: bool,
) -> String {
    if hour12 {
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
    }
}

#[cfg(test)]
mod tests {
    use super::format_clock;

    #[test]
    fn twenty_four_hour_keeps_leading_zero() {
        assert_eq!(format_clock(0, 5, 9, false, true), "00:05:09");
        assert_eq!(format_clock(13, 4, 7, false, false), "13:04");
    }

    #[test]
    fn twelve_hour_maps_midnight_and_noon_to_twelve() {
        // 0 点与 12 点都显示 12，前缀区分上下午
        assert_eq!(format_clock(0, 30, 0, true, false), "上午 12:30");
        assert_eq!(format_clock(12, 30, 0, true, false), "下午 12:30");
        assert_eq!(format_clock(13, 5, 0, true, true), "下午 01:05:00");
    }
}
