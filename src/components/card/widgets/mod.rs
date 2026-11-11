//! 组件正文：一种组件一个文件。
//!
//! 加一种组件要动三处：在这里加一个模块，在 [`crate::models::WIDGET_KINDS`] 加一行，
//! 再在 [`super::tile`] 的分发（正文）与 [`super::widget_form`] 的分支（参数编辑器）
//! 各加一条——后两处是下一步要收拢的地方，让每个组件文件自己拥有这两样。

mod calendar;
mod clock;
mod dial;
mod hot_search;
mod note;
mod todo;
mod visits;
mod weather;

pub use calendar::CalendarWidget;
pub use clock::ClockWidget;
pub use dial::DialWidget;
pub use hot_search::HotSearchWidget;
pub use note::NoteWidget;
pub use todo::TodoWidget;
pub use visits::VisitsWidget;
pub use weather::WeatherWidget;

// "现在几点"挪到了 `components::tick`：首屏大时钟也要用它，而页面装饰不该
// 反向依赖卡片。这里保留一个转导出，卡片内部的 `use super::use_tick` 不必改。
pub(crate) use crate::components::tick::use_tick;
