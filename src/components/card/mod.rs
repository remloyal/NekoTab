//! 卡片：一张卡片是什么、怎么画、怎么改、怎么排列。
//!
//! 书签与组件是**同一种东西的两种 kind**——同一张表、同一套顺序、同一套拖拽与编辑态，
//! 只有正文和表单按 kind 分（见 [`crate::models::WIDGET_KINDS`]）。所以它们放一起，
//! 而不是拆成两个平行目录。
//!
//! 不在这里的东西，以及为什么：
//!
//! - `components::icons`：图标词汇表（内置图标 + 首字/色相回退）。分组那边也在用，
//!   不是卡片专属。
//! - `components::icon_picker`：图标选择器，书签图标与分组图标共用。
//! - `components::pager` / `batch` / `context_menu`：分页条、批量移动、右键菜单，
//!   主题是卡片但所有权在首页（`pages::home`）。
//! - `models` 里的 [`crate::models::WIDGET_KINDS`] 与各 `*Config`：服务端
//!   `clean_bookmark` 也要用它按 kind 规范化参数，是前后端共享的表，不是 UI。
//!
//! 加一种组件要动的地方见 `widgets` 的模块说明。

mod add_dialog;
mod bookmark_form;
mod grid;
mod tile;
mod widget_form;
mod widgets;

// 对外（`components`）只转出首页要用的这几个。`tile` 与 `widgets` 留在内部：
// `CardTile` 只被 `CardGrid` 用，组件正文只被 `CardTile` 用，同级之间按路径直接取，
// 不必再经一层转出。
pub use add_dialog::{AddCardDialog, AddTab};
pub use bookmark_form::BookmarkForm;
pub use grid::CardGrid;
pub use widget_form::WidgetForm;
