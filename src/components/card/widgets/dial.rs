use dioxus::prelude::*;

use super::use_tick;
use crate::models::DialConfig;

/// 数字刻度离圆心的距离（viewBox 单位）。
const NUMBER_RADIUS: f64 = 37.0;

/// 「表盘时钟」组件的卡片正文。
///
/// 用 SVG 画，viewBox 固定成 0..100：卡片能跨格，让 SVG 自己等比缩放比在 CSS 里按格子
/// 算尺寸简单得多——CSS 那边只需决定这块画布在格子里占多大。
///
/// **与另外两个时钟不同，SSR 不渲染空白**：钟面（盘、刻度、指针）是静态形状，只有指针
/// 的**角度**依赖"现在"，所以首屏就画一个指向 12 点的完整表盘。两边画出来是同一个
/// 12 点整，同样不会 hydration 失配，但用户不必先看到一个空框。
#[component]
pub fn DialWidget(config: DialConfig) -> Element {
    let tick = use_tick();

    // 没有"现在"（SSR、或客户端刚挂载）时三根针都指向 12 点
    let (hour_angle, minute_angle, second_angle) = match tick() {
        Some(now) => (
            // 时针与分针带小数：时针 1 格 30°，跟着分钟慢慢走；分针跟着秒
            (now.hours % 12) as f64 * 30.0 + now.minutes as f64 * 0.5,
            now.minutes as f64 * 6.0 + now.seconds as f64 * 0.1,
            now.seconds as f64 * 6.0,
        ),
        None => (0.0, 0.0, 0.0),
    };

    // 三根针都是"从圆心指向上方、再绕圆心转"，所以旋转中心固定是 (50, 50)
    let rotate = |angle: f64| format!("rotate({angle:.2} 50 50)");
    let (hour_tf, minute_tf, second_tf) = (
        rotate(hour_angle),
        rotate(minute_angle),
        rotate(second_angle),
    );

    // 数字刻度的位置：12 个，按 30° 一步绕一圈。在这里算好，免得 rsx 里塞三角函数。
    // 不显示数字时整个跳过——省下的不只是几个 sin/cos，还有每秒一轮的字符串分配。
    let numbers: Vec<(String, String, u32)> = if config.show_numbers {
        (0..12)
            .map(|index| {
                let radians = index as f64 * 30.0 * std::f64::consts::PI / 180.0;
                // 0° 在正上方，顺时针走：所以 x 取 sin、y 取 -cos
                (
                    format!("{:.2}", 50.0 + NUMBER_RADIUS * radians.sin()),
                    format!("{:.2}", 50.0 - NUMBER_RADIUS * radians.cos()),
                    // 顶上那个位置是 12 点，不是 0 点
                    if index == 0 { 12 } else { index },
                )
            })
            .collect()
    } else {
        Vec::new()
    };

    rsx! {
        div { class: "widget widget-dial",
            svg { class: "dial", view_box: "0 0 100 100",
                circle { class: "dial-face", cx: "50", cy: "50", r: "46" }

                // 12 条刻度：画一条、转 30° 复制一圈，比手算 12 组坐标可读
                for index in 0..12 {
                    line {
                        key: "tick-{index}",
                        class: "dial-tick",
                        x1: "50",
                        y1: "6",
                        x2: "50",
                        y2: "11",
                        transform: "rotate({index * 30} 50 50)",
                    }
                }

                for (x, y, label) in numbers {
                    text {
                        key: "number-{label}",
                        class: "dial-number",
                        x: "{x}",
                        y: "{y}",
                        "{label}"
                    }
                }

                // 指针。y1/y2 决定长度：时针最短，秒针最长且带一小截尾
                line {
                    class: "dial-hand dial-hour",
                    x1: "50",
                    y1: "50",
                    x2: "50",
                    y2: "26",
                    transform: "{hour_tf}",
                }
                line {
                    class: "dial-hand dial-minute",
                    x1: "50",
                    y1: "50",
                    x2: "50",
                    y2: "15",
                    transform: "{minute_tf}",
                }
                if config.show_seconds {
                    line {
                        class: "dial-hand dial-second",
                        x1: "50",
                        y1: "57",
                        x2: "50",
                        y2: "14",
                        transform: "{second_tf}",
                    }
                }

                circle { class: "dial-pin", cx: "50", cy: "50", r: "2.6" }
            }
        }
    }
}
