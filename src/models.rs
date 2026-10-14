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
    /// 逗号分隔的标签串，如 `"rust,web,工具"`。标签不再是分类（分类见 `Group`），
    /// 只作为检索词，服务端搜索与客户端筛选都会匹配它。
    pub tags: String,
    /// 归属分组；`None` 即"未分组"（不占一行，由前端合成）
    pub group_id: Option<i64>,
    /// 自定义图标。空串表示自动（按 host 代取），其余取值与分组图标同一套约定，
    /// 见 [`crate::components::icons`]：`i:xxx` 内置图标 / `http…` 图片地址 / `/…` 本站路径。
    pub icon: String,
    /// UTC 时间字符串，形如 `2026-10-12 08:30:00`
    pub created_at: String,
}

/// 新增/修改书签的入参。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct NewBookmark {
    pub title: String,
    pub url: String,
    pub description: String,
    pub tags: String,
    /// 留空表示"未分组"
    #[serde(default)]
    pub group_id: Option<i64>,
    /// 留空表示自动取站点图标
    #[serde(default)]
    pub icon: String,
}

/// 新增书签的结果。
///
/// `created == false` 表示这个 URL 已经存在，本次**没有**写入——去重是按 URL
/// 精确匹配做的（口径与导入一致），`in_trash` 说明命中的那条在回收站里。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AddOutcome {
    /// 新记录的 id，或命中的已有记录的 id
    pub id: i64,
    pub created: bool,
    #[serde(default)]
    pub in_trash: bool,
}

/// 一个分组（分类标签页）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "server", derive(sqlx::FromRow))]
pub struct Group {
    pub id: i64,
    pub name: String,
    /// 空字符串表示回退到名称首字图标
    pub icon: String,
}

/// 新增分组的入参。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct NewGroup {
    pub name: String,
    pub icon: String,
}

/// 回收站里的一条书签。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "server", derive(sqlx::FromRow))]
pub struct TrashItem {
    pub id: i64,
    pub title: String,
    pub url: String,
    /// 进回收站的时间（UTC 字符串）
    pub deleted_at: String,
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
    /// 搜索框前缀，如 `gh`：输入 `gh rust` 即用该引擎搜 rust（空表示不用前缀）
    pub prefix: String,
}

/// 新增搜索引擎的入参。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct NewEngine {
    pub name: String,
    pub url: String,
    pub icon: String,
    pub prefix: String,
}

/// 一条「一言」。来源可能是作者，也可能是作品名，故 `source` 已拼好。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Quote {
    pub text: String,
    pub source: String,
}

/// 内置壁纸预设。与 `main.css` 里的 `.bg.preset-*` 一一对应。
pub const BACKGROUND_PRESETS: [&str; 6] =
    ["aurora", "nebula", "forest", "ocean", "sunset", "mono"];

/// 主题：跟随系统 / 深色 / 浅色。
pub const THEMES: [&str; 3] = ["system", "dark", "light"];

/// 主页设置。纯键值存储，加字段不需要迁移（见 `to_pairs` / `from_pairs`）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    // ---- 外观 ----
    /// 背景图片地址；空字符串表示用内置渐变（`background_preset` 或默认渐变）
    pub background_url: String,
    /// 背景图亮度遮罩百分比，0-80，越大越暗；仅在设了背景图时生效
    pub background_dim: i64,
    /// 每次打开页面换一张：给背景地址追加随机参数，绕开图片接口的长缓存
    pub background_random: bool,
    /// 内置壁纸预设名，见 [`BACKGROUND_PRESETS`]；`background_url` 非空时被忽略
    pub background_preset: String,
    /// 主题，见 [`THEMES`]
    pub theme: String,

    // ---- 网格布局 ----
    /// 卡片宽度（px），56–160。图标块与字号按它等比缩放。
    pub tile_size: i64,
    /// 横向间距（px）
    pub tile_gap_x: i64,
    /// 纵向间距（px）
    pub tile_gap_y: i64,
    /// 每行**最大**列数；0 表示不限（完全按宽度自适应）
    pub grid_columns: i64,

    // ---- 分页 ----
    /// 每页卡片数；0 表示不分页（一屏到底滚动）
    pub page_size: i64,
    /// 分页时用鼠标滚轮翻页（会拦掉页面滚动，故默认关闭）
    pub page_wheel: bool,
    /// 分页时翻到最后一页后回到第一页
    pub page_loop: bool,

    // ---- 时间 ----
    pub show_clock: bool,
    pub show_quote: bool,
    /// 时钟下显示农历（如「二〇二六年九月初四」）
    pub clock_show_lunar: bool,
    /// 时钟下显示「第 N 天 / 第 N 周」
    pub clock_show_day: bool,
    /// 12 小时制（前面带「上午 / 下午」）；关闭为 24 小时制
    pub clock_hour12: bool,
    /// 时钟显示秒
    pub clock_show_seconds: bool,

    // ---- 行为 ----
    /// 打开书签时开新标签页；关掉则当前页跳转
    pub open_in_new_tab: bool,
    /// 简约模式：只留时钟与搜索框，整个网格隐藏
    pub hide_grid: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            background_url: String::new(),
            background_dim: 35,
            background_random: false,
            background_preset: String::new(),
            theme: "dark".to_string(),

            tile_size: 96,
            tile_gap_x: 8,
            tile_gap_y: 20,
            grid_columns: 0,

            page_size: 0,
            page_wheel: false,
            page_loop: false,

            show_clock: true,
            show_quote: true,
            clock_show_lunar: true,
            clock_show_day: false,
            clock_hour12: false,
            clock_show_seconds: true,

            open_in_new_tab: true,
            hide_grid: false,
        }
    }
}

impl Settings {
    /// 网格列模板：始终按卡片宽度自适应（`auto-fit`）。
    ///
    /// 每行的**最大**列数不写进列模板，而是交给 [`Self::grid_max_width`] 用
    /// `max-width` 收口。列数只封顶、不写死，于是窗口变窄放不下时自然换行，
    /// 不会像固定列数那样横向溢出。
    pub fn grid_template_columns(&self) -> String {
        format!("repeat(auto-fit, {}px)", self.tile_size)
    }

    /// 每行最大列数换算出的网格最大宽度，供 CSS `max-width` 使用。
    ///
    /// N 列加 (N-1) 个横向间距，正好是容纳 N 列的宽度；`0`（不限）时返回
    /// `none`，即不设上限。
    pub fn grid_max_width(&self) -> String {
        if self.grid_columns <= 0 {
            return "none".to_string();
        }
        format!(
            "calc({}px * {} + {}px * {})",
            self.tile_size,
            self.grid_columns,
            self.tile_gap_x,
            self.grid_columns - 1
        )
    }

    /// 把主题设置落到具体的一档（`"dark"` / `"light"`）。
    ///
    /// `theme == "system"` 时看系统偏好；`system_dark` 为 `None` 表示还没探到
    /// （SSR 阶段就是这样），按深色渲染，等客户端探到后再重渲染。
    pub fn effective_theme(&self, system_dark: Option<bool>) -> &'static str {
        match self.theme.as_str() {
            "light" => "light",
            "system" => {
                if system_dark.unwrap_or(true) {
                    "dark"
                } else {
                    "light"
                }
            }
            _ => "dark",
        }
    }
}

/// 读写 `settings` 表用的一对方法；只有服务端（`backend.rs`）会用到，
/// 客户端拿到的已经是解析好的 `Settings`。
#[cfg_attr(not(feature = "server"), allow(dead_code))]
impl Settings {
    /// 从 `settings` 表的键值对还原设置。未知键忽略，缺键回落默认值。
    ///
    /// 与 [`Settings::to_pairs`] 一一对应，两处写在一起，避免读写漂移。
    pub fn from_pairs<I: IntoIterator<Item = (String, String)>>(pairs: I) -> Self {
        let mut settings = Settings::default();

        for (key, value) in pairs {
            match key.as_str() {
                "background_url" => settings.background_url = value,
                "background_dim" => settings.background_dim = clamp_int(&value, 35, 0, 80),
                "background_random" => settings.background_random = parse_bool(&value),
                "background_preset" => {
                    settings.background_preset = pick_or(&value, &BACKGROUND_PRESETS, "")
                }
                // 空值不是合法主题，回落深色
                "theme" => settings.theme = pick_or(&value, &THEMES, "dark"),

                "tile_size" => settings.tile_size = clamp_int(&value, 96, 56, 160),
                "tile_gap_x" => settings.tile_gap_x = clamp_int(&value, 8, 0, 48),
                "tile_gap_y" => settings.tile_gap_y = clamp_int(&value, 20, 0, 64),
                "grid_columns" => settings.grid_columns = clamp_int(&value, 0, 0, 16),

                "page_size" => settings.page_size = clamp_int(&value, 0, 0, 200),
                "page_wheel" => settings.page_wheel = parse_bool(&value),
                "page_loop" => settings.page_loop = parse_bool(&value),

                "show_clock" => settings.show_clock = parse_bool(&value),
                "show_quote" => settings.show_quote = parse_bool(&value),
                "clock_show_lunar" => settings.clock_show_lunar = parse_bool(&value),
                "clock_show_day" => settings.clock_show_day = parse_bool(&value),
                "clock_hour12" => settings.clock_hour12 = parse_bool(&value),
                "clock_show_seconds" => settings.clock_show_seconds = parse_bool(&value),

                "open_in_new_tab" => settings.open_in_new_tab = parse_bool(&value),
                "hide_grid" => settings.hide_grid = parse_bool(&value),
                _ => {}
            }
        }

        settings
    }

    /// 展开为逐键写入用的键值对。取值范围在这里已经夹好，落库即是可信值。
    pub fn to_pairs(&self) -> Vec<(&'static str, String)> {
        vec![
            ("background_url", self.background_url.trim().to_string()),
            ("background_dim", self.background_dim.clamp(0, 80).to_string()),
            ("background_random", bool_str(self.background_random)),
            (
                "background_preset",
                pick_or(&self.background_preset, &BACKGROUND_PRESETS, ""),
            ),
            ("theme", pick_or(&self.theme, &THEMES, "dark")),
            ("tile_size", self.tile_size.clamp(56, 160).to_string()),
            ("tile_gap_x", self.tile_gap_x.clamp(0, 48).to_string()),
            ("tile_gap_y", self.tile_gap_y.clamp(0, 64).to_string()),
            ("grid_columns", self.grid_columns.clamp(0, 16).to_string()),
            ("page_size", self.page_size.clamp(0, 200).to_string()),
            ("page_wheel", bool_str(self.page_wheel)),
            ("page_loop", bool_str(self.page_loop)),
            ("show_clock", bool_str(self.show_clock)),
            ("show_quote", bool_str(self.show_quote)),
            ("clock_show_lunar", bool_str(self.clock_show_lunar)),
            ("clock_show_day", bool_str(self.clock_show_day)),
            ("clock_hour12", bool_str(self.clock_hour12)),
            ("clock_show_seconds", bool_str(self.clock_show_seconds)),
            ("open_in_new_tab", bool_str(self.open_in_new_tab)),
            ("hide_grid", bool_str(self.hide_grid)),
        ]
    }
}

#[cfg_attr(not(feature = "server"), allow(dead_code))]
fn parse_bool(value: &str) -> bool {
    value != "0"
}

#[cfg_attr(not(feature = "server"), allow(dead_code))]
fn bool_str(value: bool) -> String {
    if value { "1" } else { "0" }.to_string()
}

#[cfg_attr(not(feature = "server"), allow(dead_code))]
fn clamp_int(value: &str, default: i64, min: i64, max: i64) -> i64 {
    value
        .trim()
        .parse::<i64>()
        .unwrap_or(default)
        .clamp(min, max)
}

/// 只接受白名单里的值，其余回落 `fallback`。
///
/// `fallback` 与"白名单第一项"分开指定：`background_preset` 的合法默认值是
/// **空串**（表示不用预设），若按第一项回落，没设过壁纸的用户会莫名多出一张预设。
#[cfg_attr(not(feature = "server"), allow(dead_code))]
fn pick_or(value: &str, allowed: &[&str], fallback: &str) -> String {
    let value = value.trim();
    if allowed.contains(&value) {
        value.to_string()
    } else {
        fallback.to_string()
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
    /// 分组名（导出时写成名字而非 id，跨库导入才能对上）
    #[serde(default)]
    pub group: String,
    /// 自定义图标（与 `Bookmark::icon` 同一套约定）
    #[serde(default)]
    pub icon: String,
    /// 浏览器导出文件里内联的图标（`ICON="data:…"`）。
    ///
    /// 只在导入方向有意义：Firefox 一类浏览器会把已缓存的站点图标直接内联进
    /// HTML，那是内网站点唯一能拿到图标的来源。导入时会被写进图标缓存，
    /// 因此**不进 JSON 备份**（备份里带的是 `icon` 字段）。
    #[serde(default, skip_serializing)]
    pub inline_icon: Option<String>,
}

#[cfg(feature = "server")]
impl ImportedBookmark {
    pub fn from_bookmark(bookmark: &Bookmark, group_name: &str) -> Self {
        Self {
            title: bookmark.title.clone(),
            url: bookmark.url.clone(),
            description: bookmark.description.clone(),
            tags: bookmark.tags.clone(),
            group: group_name.to_string(),
            icon: bookmark.icon.clone(),
            inline_icon: None,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip_is_stable() {
        let original = Settings {
            background_url: "https://example.com/a.jpg".to_string(),
            background_dim: 55,
            background_random: true,
            background_preset: "forest".to_string(),
            theme: "light".to_string(),
            tile_size: 120,
            tile_gap_x: 16,
            tile_gap_y: 32,
            grid_columns: 6,
            page_size: 24,
            page_wheel: true,
            page_loop: true,
            show_clock: false,
            show_quote: true,
            clock_show_lunar: false,
            clock_show_day: true,
            clock_hour12: true,
            clock_show_seconds: false,
            open_in_new_tab: false,
            hide_grid: true,
        };

        let pairs: Vec<(String, String)> = original
            .to_pairs()
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect();

        assert_eq!(Settings::from_pairs(pairs), original);
    }

    #[test]
    fn settings_fall_back_to_defaults() {
        // 缺键、非法值、非法枚举都回落默认
        let pairs = vec![
            ("background_dim".to_string(), "abc".to_string()),
            ("background_dim_over".to_string(), "1".to_string()),
            ("theme".to_string(), "rainbow".to_string()),
            ("tile_size".to_string(), "9999".to_string()),
        ];
        let settings = Settings::from_pairs(pairs);

        assert_eq!(settings.background_dim, Settings::default().background_dim);
        assert_eq!(settings.theme, "dark");
        assert_eq!(settings.tile_size, 160, "超上限夹到 160");
    }

    /// 空串是 `background_preset` 的合法取值（表示不用预设），
    /// 不能因为"不在白名单里"就回落到第一张预设。
    #[test]
    fn empty_wallpaper_preset_stays_empty() {
        let settings = Settings::from_pairs(vec![
            ("background_preset".to_string(), String::new()),
            ("background_preset".to_string(), "乱写".to_string()),
        ]);
        assert_eq!(settings.background_preset, "");

        // 往返之后仍然是空
        let pairs: Vec<(String, String)> = settings
            .to_pairs()
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect();
        assert_eq!(Settings::from_pairs(pairs).background_preset, "");

        // 已知预设照常保留
        let known = Settings::from_pairs(vec![(
            "background_preset".to_string(),
            "ocean".to_string(),
        )]);
        assert_eq!(known.background_preset, "ocean");
    }

    #[test]
    fn default_settings_round_trip_to_clean_defaults() {
        let original = Settings::default();
        let pairs: Vec<(String, String)> = original
            .to_pairs()
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect();
        assert_eq!(
            Settings::from_pairs(pairs),
            original,
            "默认值序列化再读回来必须还是同一份默认值"
        );
    }

    #[test]
    fn grid_columns_css() {
        let mut settings = Settings::default();
        assert_eq!(settings.grid_template_columns(), "repeat(auto-fit, 96px)");
        assert_eq!(settings.grid_max_width(), "none", "列数 0 表示不限，不加上限");

        settings.grid_columns = 5;
        settings.tile_size = 80;
        settings.tile_gap_x = 10;
        // 列模板不随列数变——列数只在宽度上封顶，不写死轨道数
        assert_eq!(settings.grid_template_columns(), "repeat(auto-fit, 80px)");
        assert_eq!(settings.grid_max_width(), "calc(80px * 5 + 10px * 4)");
    }

    #[test]
    fn system_theme_resolves() {
        let mut settings = Settings::default();

        settings.theme = "light".to_string();
        assert_eq!(settings.effective_theme(Some(true)), "light", "定死浅色不看系统");

        settings.theme = "system".to_string();
        assert_eq!(settings.effective_theme(Some(true)), "dark");
        assert_eq!(settings.effective_theme(Some(false)), "light");
        assert_eq!(settings.effective_theme(None), "dark", "探不到时按深色");
    }
}
