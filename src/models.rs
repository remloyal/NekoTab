use serde::{Deserialize, Serialize};

/// 一条书签记录。前后端共享，因此不依赖 sqlx；
/// `FromRow` 仅在服务端编译时派生。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "server", derive(sqlx::FromRow))]
pub struct Bookmark {
    pub id: i64,
    pub title: String,
    pub url: String,
    pub description: String,
    /// 逗号分隔的标签串，如 `"rust,web,工具"`
    pub tags: String,
    /// UTC 时间字符串，形如 `2026-10-12 08:30:00`
    pub created_at: String,
}

/// 新增书签的入参。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct NewBookmark {
    pub title: String,
    pub url: String,
    pub description: String,
    pub tags: String,
}

/// 一个搜索引擎。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "server", derive(sqlx::FromRow))]
pub struct Engine {
    pub id: i64,
    pub name: String,
    /// 搜索地址模板，`%s` 代表搜索词
    pub url: String,
    /// 空字符串表示回退到名称首字图标
    pub icon: String,
    /// 是否当前使用
    pub active: bool,
}

/// 新增搜索引擎的入参。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct NewEngine {
    pub name: String,
    pub url: String,
    pub icon: String,
}

/// 一条「一言」。来源可能是作者，也可能是作品名，故 `source` 已拼好。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Quote {
    pub text: String,
    pub source: String,
}

/// 主页设置。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    /// 背景图片地址；空字符串表示用内置渐变壁纸
    pub background_url: String,
    /// 背景图亮度遮罩百分比，0-80，越大越暗；仅在设了背景图时生效
    pub background_dim: i64,
    /// 每次打开页面换一张：给背景地址追加随机参数，绕开图片接口的长缓存
    pub background_random: bool,
    pub show_clock: bool,
    pub show_quote: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            background_url: String::new(),
            background_dim: 35,
            background_random: false,
            show_clock: true,
            show_quote: true,
        }
    }
}

/// 导入 / 导出的书签形状：不含 id，便于跨库搬运。
///
/// 只在服务端做解析与序列化，客户端只传文本、收回执，因此限定在 server 下。
#[cfg(feature = "server")]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ImportedBookmark {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub tags: String,
}

#[cfg(feature = "server")]
impl From<Bookmark> for ImportedBookmark {
    fn from(bookmark: Bookmark) -> Self {
        Self {
            title: bookmark.title,
            url: bookmark.url,
            description: bookmark.description,
            tags: bookmark.tags,
        }
    }
}

/// 导入结果回执。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ImportReport {
    /// 实际写入的条数
    pub imported: usize,
    /// 因 URL 重复或非法而跳过的条数
    pub skipped: usize,
}
