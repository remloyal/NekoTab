//! 农历换算与「第几天 / 第几周」。
//!
//! 农历是**纯数据表 + 算术**，所以没有引入第三方 crate：一张 1900–2100 年的
//! 编码表（每年的闰月与各月大小）加两个日期算术函数就够了。表本身是唯一容易
//! 出错的地方，因此单测直接拿历年春节（正月初一）的公历日期校验——春节日期
//! 由整张表的结构决定，任何一格抄错都会立刻让某一年的正月初一对不上。
//!
//! 只在客户端（`web`）调用：时钟本就只在挂载后才知道"现在"。

/// 1900–2100 年逐年的农历信息，每个 u32 按位编码：
///
/// | 位      | 含义                                                |
/// |---------|-----------------------------------------------------|
/// | 0–3     | 闰月月份，0 表示该年无闰月                            |
/// | 4–15    | 一至十二月的天数，1 为 30 天、0 为 29 天（bit 15 = 正月）|
/// | 16      | 闰月的天数，1 为 30 天、0 为 29 天；无闰月时无意义      |
const LUNAR_INFO: [u32; 201] = [
    0x04bd8, 0x04ae0, 0x0a570, 0x054d5, 0x0d260, 0x0d950, 0x16554, 0x056a0, 0x09ad0, 0x055d2,
    0x04ae0, 0x0a5b6, 0x0a4d0, 0x0d250, 0x1d255, 0x0b540, 0x0d6a0, 0x0ada2, 0x095b0, 0x14977,
    0x04970, 0x0a4b0, 0x0b4b5, 0x06a50, 0x06d40, 0x1ab54, 0x02b60, 0x09570, 0x052f2, 0x04970,
    0x06566, 0x0d4a0, 0x0ea50, 0x06e95, 0x05ad0, 0x02b60, 0x186e3, 0x092e0, 0x1c8d7, 0x0c950,
    0x0d4a0, 0x1d8a6, 0x0b550, 0x056a0, 0x1a5b4, 0x025d0, 0x092d0, 0x0d2b2, 0x0a950, 0x0b557,
    0x06ca0, 0x0b550, 0x15355, 0x04da0, 0x0a5b0, 0x14573, 0x052b0, 0x0a9a8, 0x0e950, 0x06aa0,
    0x0aea6, 0x0ab50, 0x04b60, 0x0aae4, 0x0a570, 0x05260, 0x0f263, 0x0d950, 0x05b57, 0x056a0,
    0x096d0, 0x04dd5, 0x04ad0, 0x0a4d0, 0x0d4d4, 0x0d250, 0x0d558, 0x0b540, 0x0b6a0, 0x195a6,
    0x095b0, 0x049b0, 0x0a974, 0x0a4b0, 0x0b27a, 0x06a50, 0x06d40, 0x0af46, 0x0ab60, 0x09570,
    0x04af5, 0x04970, 0x064b0, 0x074a3, 0x0ea50, 0x06b58, 0x055c0, 0x0ab60, 0x096d5, 0x092e0,
    0x0c960, 0x0d954, 0x0d4a0, 0x0da50, 0x07552, 0x056a0, 0x0abb7, 0x025d0, 0x092d0, 0x0cab5,
    0x0a950, 0x0b4a0, 0x0baa4, 0x0ad50, 0x055d9, 0x04ba0, 0x0a5b0, 0x15176, 0x052b0, 0x0a930,
    0x07954, 0x06aa0, 0x0ad50, 0x05b52, 0x04b60, 0x0a6e6, 0x0a4e0, 0x0d260, 0x0ea65, 0x0d530,
    0x05aa0, 0x076a3, 0x096d0, 0x04afb, 0x04ad0, 0x0a4d0, 0x1d0b6, 0x0d250, 0x0d520, 0x0dd45,
    0x0b5a0, 0x056d0, 0x055b2, 0x049b0, 0x0a577, 0x0a4b0, 0x0aa50, 0x1b255, 0x06d20, 0x0ada0,
    0x14b63, 0x09370, 0x049f8, 0x04970, 0x064b0, 0x168a6, 0x0ea50, 0x06b20, 0x1a6c4, 0x0aae0,
    0x0a2e0, 0x0d2e3, 0x0c960, 0x0d557, 0x0d4a0, 0x0da50, 0x05d55, 0x056a0, 0x0a6d0, 0x055d4,
    0x052d0, 0x0a9b8, 0x0a950, 0x0b4a0, 0x0b6a6, 0x0ad50, 0x055a0, 0x0aba4, 0x0a5b0, 0x052b0,
    0x0b273, 0x06930, 0x07337, 0x06aa0, 0x0ad50, 0x14b55, 0x04b60, 0x0a570, 0x054e4, 0x0d160,
    0x0e968, 0x0d520, 0x0daa0, 0x16aa6, 0x056d0, 0x04ae0, 0x0a9d4, 0x0a2d0, 0x0d150, 0x0f252,
    0x0d520,
];

/// 表覆盖的首年
const FIRST_YEAR: u32 = 1900;
/// 表覆盖的末年
const LAST_YEAR: u32 = 2100;

/// 农历日期。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LunarDate {
    pub year: u32,
    /// 1–12
    pub month: u32,
    /// 1–30
    pub day: u32,
    /// 该月是否为闰月
    pub leap: bool,
}

impl LunarDate {
    /// 中文月份名，如 `正月` / `冬月` / `腊月`，闰月前缀「闰」。
    pub fn month_name(&self) -> String {
        const NAMES: [&str; 12] = [
            "正月", "二月", "三月", "四月", "五月", "六月", "七月", "八月", "九月", "十月",
            "冬月", "腊月",
        ];
        let name = NAMES[(self.month - 1) as usize];
        if self.leap {
            format!("闰{name}")
        } else {
            name.to_string()
        }
    }

    /// 中文日期名，如 `初一` / `廿三` / `三十`。
    pub fn day_name(&self) -> String {
        const NAMES: [&str; 30] = [
            "初一", "初二", "初三", "初四", "初五", "初六", "初七", "初八", "初九", "初十",
            "十一", "十二", "十三", "十四", "十五", "十六", "十七", "十八", "十九", "二十",
            "廿一", "廿二", "廿三", "廿四", "廿五", "廿六", "廿七", "廿八", "廿九", "三十",
        ];
        NAMES[(self.day - 1) as usize].to_string()
    }

    /// 「二〇二六年九月初四」这种整串写法。
    pub fn full_name(&self) -> String {
        format!(
            "{}年{}{}",
            chinese_year(self.year),
            self.month_name(),
            self.day_name()
        )
    }
}

/// 年份写成逐位的中文数字，如 2026 → `二〇二六`。
fn chinese_year(year: u32) -> String {
    const DIGITS: [&str; 10] = ["〇", "一", "二", "三", "四", "五", "六", "七", "八", "九"];
    year.to_string()
        .chars()
        .filter_map(|c| c.to_digit(10))
        .map(|d| DIGITS[d as usize])
        .collect()
}

/// Howard Hinnant 的 `days_from_civil`：公历转"距 1970-01-01 的天数"。
///
/// 自己算而不是引 chrono：用到的只有「两个日期相差几天」和「星期几」，
/// 一个函数就够，也避免为时钟功能多背一个日期库。
fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    let y = if month <= 2 { year - 1 } else { year } as i64;
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400; // [0, 399]
    let mp = ((month + 9) % 12) as i64; // 3 月为 0，2 月为 11
    let doy = (153 * mp + 2) / 5 + day as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// 星期几，1 = 周一 … 7 = 周日。
pub fn iso_weekday(year: u32, month: u32, day: u32) -> u32 {
    // 1970-01-01 是周四
    let sun0 = (days_from_civil(year as i32, month, day) + 4).rem_euclid(7);
    if sun0 == 0 {
        7
    } else {
        sun0 as u32
    }
}

/// 一年中的第几天（1 起）。
pub fn day_of_year(year: u32, month: u32, day: u32) -> u32 {
    let jan1 = days_from_civil(year as i32, 1, 1);
    (days_from_civil(year as i32, month, day) - jan1 + 1) as u32
}

/// ISO 8601 周号。跨年那一周归属哪一年由「该周的周四」决定。
pub fn iso_week(year: u32, month: u32, day: u32) -> u32 {
    let doy = day_of_year(year, month, day) as i64;
    let dow = iso_weekday(year, month, day) as i64;

    // 该周周四在当年内的第几天；<1 说明这一周属于上一年，>当年天数说明属于下一年
    let thursday_doy = doy + (4 - dow);
    let year_days = if is_leap_year(year) { 366 } else { 365 };

    if thursday_doy < 1 {
        // 落在上一年最后一周
        let prev = year - 1;
        let prev_days = if is_leap_year(prev) { 366 } else { 365 };
        return weeks_in_year(prev, prev_days);
    }
    if thursday_doy > year_days {
        return 1;
    }

    ((doy - dow + 10) / 7) as u32
}

fn is_leap_year(year: u32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// 该年有 52 还是 53 个 ISO 周：1 月 1 日是周四，或闰年且 1 月 1 日是周三，则有 53 周。
fn weeks_in_year(year: u32, year_days: u32) -> u32 {
    let jan1_dow = iso_weekday(year, 1, 1);
    if jan1_dow == 4 || (year_days == 366 && jan1_dow == 3) {
        53
    } else {
        52
    }
}

/// 闰月月份，0 表示无闰月。
fn leap_month(info: u32) -> u32 {
    info & 0xf
}

/// 闰月天数；无闰月为 0。
fn leap_days(info: u32) -> u32 {
    if leap_month(info) == 0 {
        0
    } else if info & 0x10000 != 0 {
        30
    } else {
        29
    }
}

/// 某平月（1–12）的天数。
fn month_days(info: u32, month: u32) -> u32 {
    if info & (0x10000 >> month) != 0 {
        30
    } else {
        29
    }
}

/// 该农历年的总天数。
fn year_days(info: u32) -> u32 {
    (1..=12).map(|m| month_days(info, m)).sum::<u32>() + leap_days(info)
}

/// 公历转农历。超出 1900–2100 范围或日期非法时返回 `None`。
pub fn solar_to_lunar(year: u32, month: u32, day: u32) -> Option<LunarDate> {
    if !(FIRST_YEAR..=LAST_YEAR).contains(&year) || !(1..=12).contains(&month) {
        return None;
    }
    if day == 0 || day > 31 {
        return None;
    }

    // 已知锚点：1900-01-31 是农历 1900 年正月初一
    let anchor = days_from_civil(FIRST_YEAR as i32, 1, 31);
    let mut offset = days_from_civil(year as i32, month, day) - anchor;
    if offset < 0 {
        return None;
    }

    let mut lunar_year = FIRST_YEAR;
    loop {
        if lunar_year > LAST_YEAR {
            return None;
        }
        let days = year_days(LUNAR_INFO[(lunar_year - FIRST_YEAR) as usize]) as i64;
        if offset < days {
            break;
        }
        offset -= days;
        lunar_year += 1;
    }

    let info = LUNAR_INFO[(lunar_year - FIRST_YEAR) as usize];
    let leap = leap_month(info);

    // 逐月扣减，闰月紧跟在同号的平月之后
    for m in 1..=12u32 {
        let days = month_days(info, m) as i64;
        if offset < days {
            return Some(LunarDate {
                year: lunar_year,
                month: m,
                day: (offset + 1) as u32,
                leap: false,
            });
        }
        offset -= days;

        if leap == m {
            let days = leap_days(info) as i64;
            if offset < days {
                return Some(LunarDate {
                    year: lunar_year,
                    month: m,
                    day: (offset + 1) as u32,
                    leap: true,
                });
            }
            offset -= days;
        }
    }

    // 表数据自洽时不会走到这里（年的总天数 = 各月之和）
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 正月初一（春节）的公历日期。表里任何一格抄错都会让某一年对不上。
    #[test]
    fn spring_festival_matches_known_dates() {
        let cases: [(u32, u32, u32); 20] = [
            (1900, 1, 31),
            (1950, 2, 17),
            (1960, 1, 28),
            (1970, 2, 6),
            (1980, 2, 16),
            (1990, 1, 27),
            (2000, 2, 5),
            (2010, 2, 14),
            (2015, 2, 19),
            (2018, 2, 16),
            (2019, 2, 5),
            (2020, 1, 25),
            (2021, 2, 12),
            (2022, 2, 1),
            (2023, 1, 22),
            (2024, 2, 10),
            (2025, 1, 29),
            (2026, 2, 17),
            (2027, 2, 6),
            (2028, 1, 26),
        ];

        for (y, m, d) in cases {
            let lunar = solar_to_lunar(y, m, d).expect("换算失败");
            assert_eq!(
                (lunar.month, lunar.day, lunar.leap),
                (1, 1, false),
                "{y}-{m:02}-{d:02} 应为正月初一，实际 {lunar:?}"
            );
        }
    }

    /// 春节前一天必须是上一年的腊月最后一天（二十九或三十）。
    #[test]
    fn day_before_spring_festival_is_last_lunar_day() {
        for (y, m, d) in [(2025, 1, 29), (2026, 2, 17), (2027, 2, 6)] {
            let prev = solar_to_lunar(y, m, d).unwrap();
            let earlier = solar_to_lunar(y, m, d - 1).unwrap();
            assert_eq!(prev.month, 1, "{y} 春节应在正月");
            assert_eq!(earlier.month, 12, "{y} 春节前一天应在腊月");
            assert!(earlier.day >= 29, "腊月最后一天应为 29 或 30");
            assert_eq!(earlier.year, y - 1);
        }
    }

    /// 2026-10-13 是农历九月初四（与参考站点当天的显示一致）。
    #[test]
    fn converts_a_regular_date() {
        let lunar = solar_to_lunar(2026, 10, 13).unwrap();
        assert_eq!(lunar.year, 2026);
        assert_eq!(lunar.month, 9);
        assert_eq!(lunar.day, 4);
        assert!(!lunar.leap);
        assert_eq!(lunar.month_name(), "九月");
        assert_eq!(lunar.day_name(), "初四");
        assert_eq!(lunar.full_name(), "二〇二六年九月初四");
    }

    /// 闰月要落在正确的月份之后，且编号与平月同名。
    #[test]
    fn handles_leap_month() {
        // 2020 年是闰四月
        let before = solar_to_lunar(2020, 5, 22).unwrap();
        let first = solar_to_lunar(2020, 5, 23).unwrap();
        assert_eq!((before.month, before.leap), (4, false), "闰四月初一前一天");
        assert_eq!((first.month, first.leap), (4, true), "2020-05-23 应为闰四月初一");
        assert_eq!(first.month_name(), "闰四月");
    }

    /// 农历年的天数只能是 353–385 天，且与逐月天数自洽。
    #[test]
    fn year_lengths_are_consistent() {
        for year in FIRST_YEAR..=LAST_YEAR {
            let info = LUNAR_INFO[(year - FIRST_YEAR) as usize];
            let total = year_days(info);

            let months: u32 = (1..=12).map(|m| month_days(info, m)).sum();
            assert!((348..=360).contains(&months), "{year} 平月合计异常: {months}");
            assert!((353..=385).contains(&total), "{year} 全年天数异常: {total}");

            let leap = leap_days(info);
            assert!(leap == 0 || leap == 29 || leap == 30, "{year} 闰月天数异常");
            assert_eq!(total, months + leap);
        }
    }

    /// 连续日期的农历日必须连续递增（跨月回绕），用它检查逐月扣减的边界。
    #[test]
    fn consecutive_days_increment_by_one() {
        // days_from_civil 的逆运算，供测试逐日推进日期用
        fn civil_from_days(z: i64) -> (u32, u32, u32) {
            let z = z + 719_468;
            let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
            let doe = z - era * 146_097;
            let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
            let y = yoe + era * 400;
            let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
            let mp = (5 * doy + 2) / 153;
            let d = doy - (153 * mp + 2) / 5 + 1;
            let m = if mp < 10 { mp + 3 } else { mp - 9 };
            let year = if m <= 2 { y + 1 } else { y };
            (year as u32, m as u32, d as u32)
        }

        for (y, m, d) in [(2026, 2, 17), (2026, 10, 13), (2024, 2, 10), (2000, 2, 5)] {
            let mut expected = solar_to_lunar(y, m, d).unwrap();
            let start = days_from_civil(y as i32, m, d);

            for step in 1..=400i64 {
                let (yy, mm, dd) = civil_from_days(start + step);
                let lunar = solar_to_lunar(yy, mm, dd).expect("换算失败");

                let same_month = lunar.month == expected.month && lunar.leap == expected.leap;
                assert!(same_month || lunar.day == 1, "跨月必须落在初一: {lunar:?}");
                if same_month {
                    assert_eq!(lunar.day, expected.day + 1, "{yy}-{mm}-{dd} 不连续");
                }
                expected = lunar;
            }
        }
    }

    #[test]
    fn out_of_range_returns_none() {
        assert!(solar_to_lunar(1899, 12, 31).is_none());
        assert!(solar_to_lunar(2100, 12, 31).is_some() || solar_to_lunar(2101, 1, 1).is_none());
    }

    #[test]
    fn day_of_year_and_iso_week() {
        assert_eq!(day_of_year(2026, 10, 13), 286);
        assert_eq!(iso_week(2026, 10, 13), 42, "参考站点当天显示第 42 周");
        assert_eq!(day_of_year(2026, 1, 1), 1);
        assert_eq!(day_of_year(2024, 12, 31), 366, "2024 是闰年");

        // 2024-12-30 是周一，属于 2025 年第 1 周
        assert_eq!(iso_week(2024, 12, 30), 1);
        // 2021-01-01 是周五，属于 2020 年第 53 周
        assert_eq!(iso_week(2021, 1, 1), 53);
        assert_eq!(iso_weekday(2026, 10, 13), 2, "2026-10-13 是周二");
    }
}
