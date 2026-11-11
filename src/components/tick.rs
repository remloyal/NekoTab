//! 每秒钟跳一次的"当前时刻"。
//!
//! 首屏大时钟与网格里的数字 / 表盘时钟要的是同一件事，放在这里而不是任一个
//! 组件文件里：卡片不该反向依赖页面装饰，页面装饰也不该依赖卡片。

use dioxus::prelude::*;

/// 每秒跳一次的"当前时刻"快照。
///
/// 抽成一个类型而不是元组：六个 `u32` 用元组在 `match` 里会变成一串位置参数，
/// 读的人得回去数第几个是月份。
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Tick {
    pub year: u32,
    pub hours: u32,
    pub minutes: u32,
    pub seconds: u32,
    pub month: u32,
    pub day: u32,
    /// `Date::get_day()` 的口径：0 = 星期日
    pub weekday: u32,
}

/// 订阅"现在几点"。首发大时钟与两个卡片时钟共用这一份。
///
/// 两条约定都写在这里，免得抄第二遍时漏掉：
///
/// - **只有"现在"进 future，格式化留在渲染里。** `use_future` 只跑一次，闭包捕获的是
///   挂载那一刻的参数；把格式化也塞进去的话，改完 12 / 24 小时制就得刷新页面才生效。
/// - **SSR 时是 `None`**，组件渲染成空白，客户端挂载后才填上——服务端不知道"现在"，
///   两边首次渲染必须一致，否则 hydration 失配。
pub(crate) fn use_tick() -> Signal<Option<Tick>> {
    // 服务端分支里没有写入，`mut` 只在 web 下需要
    #[cfg_attr(feature = "server", allow(unused_mut))]
    let mut tick = use_signal(|| Option::<Tick>::None);

    use_future(move || async move {
        #[cfg(feature = "web")]
        loop {
            let now = js_sys::Date::new_0();
            tick.set(Some(Tick {
                year: now.get_full_year(),
                hours: now.get_hours(),
                minutes: now.get_minutes(),
                seconds: now.get_seconds(),
                month: now.get_month() + 1,
                day: now.get_date(),
                weekday: now.get_day(),
            }));

            gloo_timers::future::TimeoutFuture::new(1000).await;
        }
    });

    tick
}
