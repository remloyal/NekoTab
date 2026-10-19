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
    /// 卡片横向占几格，见 [`SPAN_MAX`]
    pub span_w: i64,
    /// 卡片纵向占几格
    pub span_h: i64,
    /// 卡片种类，见 [`is_widget_kind`]：空串＝网址导航，其余是组件。
    ///
    /// 组件与书签共用这张表、共用顺序与拖拽，只靠这一列分流，所以列的是
    /// `kind` 而不是把两类数据拆开——拆开就要重新回答"谁排在前面""两个网格的
    /// 卡片属性会不会撞车""跨类能不能拖"，而这些问题现在已经有答案了。
    #[serde(default)]
    pub kind: String,
    /// 组件的显示参数（JSON 串）。书签恒为空串。见 [`ClockConfig`]。
    #[serde(default)]
    pub config: String,
    /// 卡片的面（底色）。空串＝没设过，用该种类的默认；见 [`effective_surface`]。
    #[serde(default)]
    pub surface: String,
}

/// 卡片种类：网址导航（书签）。空串是它的取值，于是"老数据 / 没传"天然落在书签上。
pub const KIND_BOOKMARK: &str = "";

/// 数字时钟组件。
pub const KIND_CLOCK: &str = "clock";

/// 表盘时钟组件。
pub const KIND_DIAL: &str = "dial";

/// 记事本组件。
pub const KIND_NOTE: &str = "note";

/// 天气组件。
pub const KIND_WEATHER: &str = "weather";

/// 组件类型白名单：`(kind, 显示名, 内置图标 id)`。
///
/// 做成白名单而不是"任意字符串"：`kind` 决定了用哪个组件渲染、参数按哪套规则规范化，
/// 放任意值进来就等于把渲染层变成一个会收到未知输入的分发器。
///
/// 图标取 [`crate::components::icons::GROUP_ICONS`] 的 id，供「添加卡片」弹窗的选卡
/// 面板用——那一格要是没有图标，就得在渲染处按 kind 再写一次分支。
pub const WIDGET_KINDS: [(&str, &str, &str); 4] = [
    (KIND_CLOCK, "数字时钟", "digital"),
    (KIND_DIAL, "表盘时钟", "clock"),
    (KIND_NOTE, "记事本", "note"),
    (KIND_WEATHER, "天气", "cloud"),
];

/// 这个 kind 是不是（已知的）组件。
pub fn is_widget_kind(kind: &str) -> bool {
    WIDGET_KINDS.iter().any(|(known, _, _)| *known == kind)
}

/// 组件类型的显示名；书签或未知类型返回空串。
pub fn widget_label(kind: &str) -> &'static str {
    WIDGET_KINDS
        .iter()
        .find(|(known, _, _)| *known == kind)
        .map(|(_, label, _)| *label)
        .unwrap_or("")
}

/// 卡片的面（底色）：`(id, 显示名)`。**与 kind 无关**——书签卡将来也能用。
///
/// 空串不是"没有面"，而是"没设过，用该种类的默认"（见 [`effective_surface`]）。
/// 这样以后想改默认值，已有的卡片会一起跟上，不必写迁移回填。
pub const CARD_SURFACES: [(&str, &str); 4] =
    [("", "默认"), ("none", "无"), ("panel", "半透明"), ("solid", "实底")];

/// 没设过面时的默认：**组件卡要一层**（它的内容是文字与细线，压在壁纸上能不能看清
/// 全看壁纸是什么样），**书签卡不要**（内容是"彩色图标块 + 一行短标题"，图标块自带底色）。
pub fn default_surface(is_widget: bool) -> &'static str {
    if is_widget {
        "panel"
    } else {
        "none"
    }
}

/// 把库里存的面收敛到白名单；认不出的一律当成"没设过"。
///
/// 只有服务端落库前调它（表单那边给的是白名单里的固定几项），所以 web 侧编译时是死代码。
#[cfg_attr(not(feature = "server"), allow(dead_code))]
pub fn normalize_surface(value: &str) -> String {
    let value = value.trim();
    CARD_SURFACES
        .iter()
        .any(|(id, _)| !id.is_empty() && *id == value)
        .then(|| value.to_string())
        .unwrap_or_default()
}

/// 卡片实际用的面：把"没设过"解析成该种类的默认。
///
/// 返回值一定落在 [`CARD_SURFACES`] 的 id 里，渲染时直接当属性值用。
pub fn effective_surface(surface: &str, is_widget: bool) -> &'static str {
    let value = surface.trim();
    CARD_SURFACES
        .iter()
        .find(|(id, _)| !id.is_empty() && *id == value)
        .map(|(id, _)| *id)
        .unwrap_or_else(|| default_surface(is_widget))
}

/// 「数字时钟」组件的显示参数。
///
/// 与 `Settings` 里那套 `clock_*` 是两回事：那是首屏大时钟的全局开关，这是**单张
/// 组件卡片**自己的参数。同一种组件将来可以有多个实例、各自不同，所以参数必须跟着
/// 卡片走，而不是读全局设置。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClockConfig {
    /// 12 小时制（带「上午 / 下午」前缀）
    #[serde(default)]
    pub hour12: bool,
    /// 显示秒
    #[serde(default = "default_true")]
    pub show_seconds: bool,
}

/// serde 的字段默认值：`show_seconds` 缺省为真，所以没有这一项的老 JSON 也是显示秒的。
fn default_true() -> bool {
    true
}

impl Default for ClockConfig {
    fn default() -> Self {
        Self {
            hour12: false,
            show_seconds: true,
        }
    }
}

impl ClockConfig {
    /// 从库里的 JSON 串还原；解析不了就回落默认值。
    ///
    /// 刻意不报错：参数只影响一张卡片怎么画，一条脏数据不该让整页渲染不出来。
    /// 这与 `Settings::from_pairs` 对非法值的姿态一致。
    pub fn parse(raw: &str) -> Self {
        serde_json::from_str(raw).unwrap_or_default()
    }

    /// 规范化成落库用的 JSON 串。服务端在写入前调它，于是库里存的永远是
    /// "这一版认识的字段"，而不是调用方塞进来的任意 JSON。
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
}

/// 「表盘时钟」组件的显示参数。
///
/// 与数字时钟分开一份配置，而不是共用 [`ClockConfig`]：两者听起来像同一个东西，实际
/// 参数几乎没有交集——这边是"画不画笔"，那边是"12 / 24 小时制"。硬合成一个结构，
/// 每个字段都要带一句"哪一种时钟才有意义"。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DialConfig {
    /// 画秒针
    #[serde(default = "default_true")]
    pub show_seconds: bool,
    /// 刻度写成数字（1–12）而不是短线
    #[serde(default)]
    pub show_numbers: bool,
}

impl Default for DialConfig {
    fn default() -> Self {
        Self {
            show_seconds: true,
            show_numbers: false,
        }
    }
}

impl DialConfig {
    /// 从库里的 JSON 串还原；解析不了就回落默认值（与另外两种组件同一条姿态）。
    pub fn parse(raw: &str) -> Self {
        serde_json::from_str(raw).unwrap_or_default()
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
}

/// 记事本能存多少字符。///
/// 上限是必要的：内容是自由文本，一次粘贴就可能是一整篇文档，而它会进列表接口的每一
/// 次响应（`config` 跟着卡片一起返回）。2000 字够写待办与备忘，也保证一屏卡片的数据
/// 量级不至于失控。
pub const NOTE_MAX_CHARS: usize = 2000;

/// 「记事本」组件的内容。
///
/// 目前只有一个字段，仍然走 JSON 而不是把正文直接塞进 `config`：一来别的组件已经是
/// JSON，两边不一致会让规范化那条路要分叉；二来字号、对齐这类开关迟早要加，那时不必
/// 再迁一次数据。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct NoteConfig {
    #[serde(default)]
    pub text: String,
}

impl NoteConfig {
    /// 从库里的 JSON 串还原；解析不了就回落空笔记（不是报错）。
    ///
    /// 与 [`ClockConfig::parse`] 同一条姿态：参数只影响一张卡片怎么画，
    /// 一条脏数据不该让整页渲染不出来。
    pub fn parse(raw: &str) -> Self {
        serde_json::from_str(raw).unwrap_or_default()
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    /// 收到合法范围内的正文：按字符数截断、去掉首尾空白。    ///
    /// 按**字符**而不是字节截断——中文一个字三字节，按字节切会切出半个字。
    ///
    /// 只有服务端落库前调它（表单那边用 `maxlength` 拦一道，但那是体验、不是保证），
    /// 所以 web 侧编译时它是死代码。
    #[cfg_attr(not(feature = "server"), allow(dead_code))]
    pub fn clean(text: &str) -> String {
        text.trim()
            .chars()
            .take(NOTE_MAX_CHARS)
            .collect::<String>()
            // 截断可能正好切在末尾空白上，再收一次
            .trim_end()
            .to_string()
    }
}

/// 地点名能有多长。
///
/// 它会被拼进地理编码接口的查询串，长度上限既是防手滑贴进一整段地址，
/// 也是给上游留个限度。
pub const WEATHER_MAX_LOCATION_CHARS: usize = 60;

/// 「天气」组件的参数。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct WeatherConfig {
    /// 地点名（如「北京」「Shanghai」）。交给数据源的地理编码接口查坐标，
    /// 所以填人类看得懂的地名即可，不用填经纬度。空串表示还没设过。
    #[serde(default)]
    pub location: String,
    /// 用华氏度（默认摄氏度）
    #[serde(default)]
    pub fahrenheit: bool,
}

impl WeatherConfig {
    /// 从库里的 JSON 串还原；解析不了就回落默认值（与另外几种组件同一条姿态）。
    pub fn parse(raw: &str) -> Self {
        serde_json::from_str(raw).unwrap_or_default()
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    /// 收到合法范围内的地点名：去掉首尾空白、按字符数截断。
    ///
    /// 按**字符**截断而不是字节——地名多为中文，按字节切会切出半个字。
    /// 写库和发请求前各调一次，于是"库里存的"和"发出去的"是同一个值。
    #[cfg_attr(not(feature = "server"), allow(dead_code))]
    pub fn clean_location(text: &str) -> String {
        text.trim()
            .chars()
            .take(WEATHER_MAX_LOCATION_CHARS)
            .collect()
    }
}

/// 一次天气查询的结果。服务端填好，客户端只负责画。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Weather {
    /// 数据源回填的规范地名。可能和用户输入的不完全一样（「北京」→「北京市」），
    /// 显示它比显示用户输入更能说明"查的是哪儿"。
    pub place: String,
    /// 当前气温
    pub temperature: f64,
    /// 今日最高
    pub high: f64,
    /// 今日最低
    pub low: f64,
    /// WMO 天气代码，见 [`weather_label`]
    pub code: u32,
}

/// WMO 天气代码 → `(中文描述, 图示)`。
///
/// 分档按数据源文档给的 WMO 代码表归并：细到 51/53/55（小/中/大毛毛雨）那种程度的
/// 区分在一张小卡片上读不出来，同组共用一个说法反而更清楚。
///
/// 认不出的代码回落成「未知」而不是空串——卡片上显示一个空白比显示「未知」更像坏了。
pub fn weather_label(code: u32) -> (&'static str, &'static str) {
    match code {
        0 => ("晴", "☀️"),
        1 => ("晴间多云", "🌤️"),
        2 => ("多云", "⛅"),
        3 => ("阴", "☁️"),
        45 | 48 => ("雾", "🌫️"),
        51 | 53 | 55 | 56 | 57 => ("毛毛雨", "🌦️"),
        61 | 63 | 65 | 66 | 67 => ("雨", "🌧️"),
        71 | 73 | 75 | 77 => ("雪", "❄️"),
        80 | 81 | 82 => ("阵雨", "🌦️"),
        85 | 86 => ("阵雪", "🌨️"),
        95 | 96 | 99 => ("雷雨", "⛈️"),
        _ => ("未知", "❓"),
    }
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
    /// 卡片占几格（宽）。缺省 / 非法值由 [`clamp_span`] 收到合法范围，0 视作 1，
    /// 因此老备份与不传这个字段的调用方都退化成 1×1。
    #[serde(default)]
    pub span_w: i64,
    #[serde(default)]
    pub span_h: i64,
    /// 卡片种类；空串（缺省）＝网址导航，见 [`is_widget_kind`]
    #[serde(default)]
    pub kind: String,
    /// 组件参数（JSON 串）；书签留空。服务端会按 kind 规范化后再落库。
    #[serde(default)]
    pub config: String,
    /// 卡片的面（底色）；留空表示用该种类的默认，见 [`effective_surface`]。
    #[serde(default)]
    pub surface: String,
}

impl Bookmark {
    /// 这张卡片是不是组件。未知 kind 按书签处理——渲染层因此不会碰到没见过的分支。
    pub fn is_widget(&self) -> bool {
        is_widget_kind(&self.kind)
    }

    /// 这张卡片实际该用的面：把"没设过"解析成该种类的默认。
    ///
    /// 渲染层只调这一个方法，不必自己判断 kind 或处理空串。
    pub fn surface_class(&self) -> &'static str {
        effective_surface(&self.surface, self.is_widget())
    }
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

/// 背景池最多存几条。
///
/// 有上限是为了让面板能一眼扫完，也为了控制体积：`Settings` 会随首屏直出进
/// hydration 载荷，让一个键无限膨胀不合适。
pub const BACKGROUND_POOL_MAX: usize = 30;

/// 背景池里单条地址的长度上限。
pub const BACKGROUND_URL_MAX: usize = 2048;

/// 一个地址能不能当背景用。
///
/// 三种形状之外一律不收：`http://` / `https://`（外链，也可能是随机图接口）、
/// `/`（本站路径，本地上传的壁纸就是 `/wallpaper/xxxx.png`）。空串是合法的
/// "没设"，由调用方决定怎么解释。
pub fn is_allowed_background(url: &str) -> bool {
    let url = url.trim();
    url.is_empty()
        || url.starts_with("http://")
        || url.starts_with("https://")
        || url.starts_with('/')
}

/// 把一串候选地址收敛成可信的背景池：逐行 trim、丢空行、丢非法行与超长行、
/// 按字符串去重、截到 [`BACKGROUND_POOL_MAX`]。
///
/// 与 `to_pairs` 那套"取值范围在落库前夹好"是同一职责，所以读（老库）与写
/// （保存）都走它，两条路径不会漂移。做了过滤而不是报错：真走到这一步说明
/// 界面那道校验漏了，静默丢掉总比让整次保存失败好。
pub fn normalize_pool<I, S>(urls: I) -> Vec<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut pool: Vec<String> = Vec::new();

    for raw in urls {
        let url = raw.as_ref().trim();
        if url.is_empty() || !is_allowed_background(url) || url.len() > BACKGROUND_URL_MAX {
            continue;
        }
        if pool.iter().any(|known| known == url) {
            continue;
        }
        if pool.len() >= BACKGROUND_POOL_MAX {
            break;
        }
        pool.push(url.to_string());
    }

    pool
}

/// 主题：跟随系统 / 深色 / 浅色。
pub const THEMES: [&str; 3] = ["system", "dark", "light"];

/// 卡片跨度（占几格）的上下限。
pub const SPAN_MIN: i64 = 1;
pub const SPAN_MAX: i64 = 3;

/// 表单里可选的格子尺寸（宽 × 高）。宽最多 2 列、高最多 3 行，够拼出
/// 「一张大的 + 一圈小的」这种版面，又不至于一屏放不下几张卡。
pub const SPAN_PRESETS: [(i64, i64); 6] =
    [(1, 1), (2, 1), (1, 2), (2, 2), (1, 3), (2, 3)];

/// 把跨度夹到合法范围。0（老备份、表单没传）会被抬到 1，也就是默认的一格。
pub fn clamp_span(span: i64) -> i64 {
    span.clamp(SPAN_MIN, SPAN_MAX)
}

/// 按当前能排出的列数夹取横向跨度。
///
/// `columns` 为 `None` 表示网格宽度还没量到（SSR、或网格尚未挂载），此时不夹。
///
/// 为什么要夹：放不下时多出来的那一列是**隐式轨道**，宽度会塌成 0，卡片于是变成
/// 「一格 + 一个间距」宽（实测 1 列容器里 `span 2` 得到 104px，而正常一格是 96px）。
/// 不会溢出容器，但会比同列的其它卡片宽出一个间距、左右边缘对不齐。夹掉之后
/// 卡片始终落在整数格上。
pub fn clamp_span_w(span_w: i64, columns: Option<usize>) -> i64 {
    let span = clamp_span(span_w);
    match columns {
        Some(columns) => span.min(columns.max(1) as i64),
        None => span,
    }
}

/// 网格在给定宽度下实际能排出几列。
///
/// 与 CSS `repeat(auto-fit, Npx)` 的算法一致：n 列需要 `n * 卡片宽 + (n-1) * 横向间距`，
/// 反过来就是 `floor((宽 + 间距) / (卡片宽 + 间距))`。跨格卡片能不能放下要看这个数，
/// 而它只取决于容器宽度，跟设置里的"每行最大列数"无关，所以只能实测宽度后现算。
pub fn columns_for(width: f64, tile_size: i64, gap_x: i64) -> usize {
    if !(width > 0.0) || tile_size <= 0 {
        return 1;
    }

    let gap = gap_x.max(0) as f64;
    let step = tile_size as f64 + gap;
    (((width + gap) / step).floor() as i64).max(1) as usize
}

/// 主页设置。纯键值存储，加字段不需要迁移（见 `to_pairs` / `from_pairs`）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    // ---- 外观 ----
    /// 背景图片地址；空字符串表示用内置渐变（`background_preset` 或默认渐变）。
    ///
    /// 它是"**当前生效的那一条**"，背景池非空时必定是池里的一条（见
    /// [`Self::normalize_background`]）——渲染只看它，轮换选谁也只改它。
    pub background_url: String,
    /// 背景池：一行一个地址，既可以是外链 `https://…`，也可以是本地上传的
    /// `/wallpaper/xxxx.png`。
    ///
    /// "有哪些背景"以池为准，"这次用哪张"看 `background_url`。分成两个字段而不是
    /// 一个列表 + 下标：下标会在增删条目时整体错位，而地址字符串是稳定的。
    pub background_urls: Vec<String>,
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
            background_urls: Vec::new(),
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

/// 卡区那一列（`.stage`）的基准宽度上限，单位 px。
///
/// 列数不限时页面就按这个宽度居中；设了每行最大列数后，这一列要跟着放宽到放得下
/// 那些列（见 [`Settings::stage_max_width`]），否则它自己的上限会先一步把列数卡死。
pub const STAGE_MAX_WIDTH: i64 = 940;

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

    /// 卡区那一列的最大宽度，供 `.stage` 的 `max-width` 使用。
    ///
    /// `.grid` 的 `width: 100%` 取的是这一列的**内容宽**，`max-width` 只能把它收窄、
    /// 不能让它超出父级。所以父级必须自己也放得下 N 列，[`Self::grid_max_width`] 才
    /// 真正生效；否则 940px 会先卡在 9 列，设置里 9 以上的值等于没设。
    ///
    /// 列数不限（`0`）时保持基准宽度 [`STAGE_MAX_WIDTH`]；有上限时取两者较大者——
    /// 窗口不够宽时 `width: 100%` 仍然优先，不会横向溢出。
    pub fn stage_max_width(&self) -> String {
        if self.grid_columns <= 0 {
            return format!("{STAGE_MAX_WIDTH}px");
        }
        format!("max({STAGE_MAX_WIDTH}px, {})", self.grid_max_width())
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
                "background_urls" => settings.background_urls = normalize_pool(value.split('\n')),
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

        // 池与当前地址是一对，读完一起收敛（老库只有 `background_url`，见
        // `normalized_background` 的说明）
        let (pool, url) = settings.normalized_background();
        settings.background_urls = pool;
        settings.background_url = url;
        settings
    }

    /// 背景归一：一次性算出"可信的池 + 当前地址"。
    ///
    /// * 池：逐行去空、去非法、去重、截到上限（[`normalize_pool`]）；
    /// * 池为空而 `background_url` 非空 → 把那条当成池里唯一的一条。老库里只有
    ///   这一个字段，这样"有哪些背景"始终只有池一个出处，而用户打开设置就能看见
    ///   自己那张图、也能顺势加别的——纯函数、幂等，不必为一个字段写迁移；
    /// * 池非空 → `background_url` 必是池里的一条，不在池里就取池首，免得留下
    ///   "既不在池里、界面上也看不见，却实际生效"的背景。
    ///
    /// 读（`from_pairs`）与写（`to_pairs`）都走它，两条路径不会漂移。
    pub fn normalized_background(&self) -> (Vec<String>, String) {
        let mut pool = normalize_pool(&self.background_urls);
        let url = self.background_url.trim().to_string();

        if pool.is_empty() && !url.is_empty() {
            pool = normalize_pool(std::slice::from_ref(&self.background_url));
        }

        let url = if pool.is_empty() || pool.iter().any(|known| *known == url) {
            url
        } else {
            pool[0].clone()
        };

        (pool, url)
    }

    /// 展开为逐键写入用的键值对。取值范围在这里已经夹好，落库即是可信值。
    pub fn to_pairs(&self) -> Vec<(&'static str, String)> {
        // 背景的那两项要一起收敛（池去非法行、当前地址收进池内），先算出来
        let (pool, background_url) = self.normalized_background();

        vec![
            ("background_url", background_url),
            ("background_urls", pool.join("\n")),
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
    /// 卡片占几格。Netscape HTML 表达不了这个，只有 JSON 备份能原样搬回来；
    /// 缺省 0 经 [`clamp_span`] 收成 1，所以老备份仍是 1×1。
    #[serde(default)]
    pub span_w: i64,
    #[serde(default)]
    pub span_h: i64,
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
            span_w: bookmark.span_w,
            span_h: bookmark.span_h,
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
            background_urls: vec![
                "https://example.com/a.jpg".to_string(),
                "/wallpaper/deadbeef.png".to_string(),
            ],
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

    /// 池读进来就收敛：去空行、去非法形状、去重、保序。
    #[test]
    fn pool_is_normalized_on_read() {
        let settings = Settings::from_pairs(vec![(
            "background_urls".to_string(),
            " https://a.test/1.jpg \n\njavascript:alert(1)\n/wallpaper/deadbeef.png\n\
             https://a.test/1.jpg \nftp://x/y.png"
                .to_string(),
        )]);

        assert_eq!(
            settings.background_urls,
            vec![
                "https://a.test/1.jpg".to_string(),
                "/wallpaper/deadbeef.png".to_string(),
            ]
        );
    }

    /// 老库里只有一条 `background_url`：读出来就是池里唯一的一条，
    /// 而当前地址仍是它——"有哪些背景"从此只有池一个出处。
    #[test]
    fn legacy_single_url_becomes_the_pool() {
        let settings = Settings::from_pairs(vec![(
            "background_url".to_string(),
            "https://a.test/only.jpg".to_string(),
        )]);

        assert_eq!(settings.background_urls, vec!["https://a.test/only.jpg"]);
        assert_eq!(settings.background_url, "https://a.test/only.jpg");
    }

    /// 池非空时，当前地址必须落在池里；不在（或压根没设）就取池首。
    /// 否则会留下一张既不在池里、面板上也看不见，却实际生效的背景。
    #[test]
    fn current_background_is_pulled_into_the_pool() {
        let mut settings = Settings::default();
        settings.background_urls = vec![
            "https://a.test/1.jpg".to_string(),
            "https://a.test/2.jpg".to_string(),
        ];

        // 没设当前地址 → 池首
        assert_eq!(
            settings.normalized_background().1,
            "https://a.test/1.jpg"
        );

        // 设了一条不在池里的 → 也是池首
        settings.background_url = "https://a.test/ghost.jpg".to_string();
        assert_eq!(
            settings.normalized_background().1,
            "https://a.test/1.jpg"
        );

        // 池里有的照原样保留
        settings.background_url = "https://a.test/2.jpg".to_string();
        assert_eq!(
            settings.normalized_background().1,
            "https://a.test/2.jpg"
        );
    }

    /// 池有条数上限，超出的截掉；单条超长的直接丢。
    #[test]
    fn pool_is_capped() {
        let many: Vec<String> = (0..BACKGROUND_POOL_MAX + 5)
            .map(|i| format!("https://a.test/{i}.jpg"))
            .collect();
        let pool = normalize_pool(&many);
        assert_eq!(pool.len(), BACKGROUND_POOL_MAX);
        assert_eq!(pool[0], "https://a.test/0.jpg");

        let long = format!("https://a.test/{}", "x".repeat(BACKGROUND_URL_MAX));
        assert!(normalize_pool([long.as_str()]).is_empty(), "超长地址不收");
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
        assert_eq!(settings.stage_max_width(), "940px", "列数不限时保持基准宽度");

        settings.grid_columns = 5;
        settings.tile_size = 80;
        settings.tile_gap_x = 10;
        // 列模板不随列数变——列数只在宽度上封顶，不写死轨道数
        assert_eq!(settings.grid_template_columns(), "repeat(auto-fit, 80px)");
        assert_eq!(settings.grid_max_width(), "calc(80px * 5 + 10px * 4)");
        // 外层列宽取"基准宽度"与"放得下 N 列"的较大者：5 列只要 440px，
        // 页面不该因此变窄，所以仍是 940px 起
        assert_eq!(
            settings.stage_max_width(),
            "max(940px, calc(80px * 5 + 10px * 4))"
        );
    }

    #[test]
    fn columns_match_what_auto_fit_actually_does() {
        // 前三个数是拿浏览器实测的网格对过的：容器 900px / 上限 4 列(408px) / 320px
        // 分别排出 8 / 4 / 3 列（见 README 的实测记录）。跨格卡片能放下几格就看它。
        assert_eq!(columns_for(900.0, 96, 8), 8);
        assert_eq!(columns_for(408.0, 96, 8), 4);
        assert_eq!(columns_for(320.0, 96, 8), 3);

        // 刚好放下一列，以及还没测到宽度 / 非法输入
        assert_eq!(columns_for(96.0, 96, 8), 1);
        assert_eq!(columns_for(0.0, 96, 8), 1, "还没测到宽度时至少算 1 列");
        assert_eq!(columns_for(-5.0, 96, 8), 1);
    }

    #[test]
    fn spans_are_clamped() {
        assert_eq!(clamp_span(0), 1, "没设过 / 老备份一律当一格");
        assert_eq!(clamp_span(-3), 1);
        assert_eq!(clamp_span(2), 2);
        assert_eq!(clamp_span(99), SPAN_MAX);
    }

    #[test]
    fn span_width_is_clamped_to_available_columns() {
        assert_eq!(clamp_span_w(2, Some(4)), 2, "放得下就不动");
        assert_eq!(
            clamp_span_w(2, Some(1)),
            1,
            "只排得下 1 列时必须收成 1，否则会撑出隐式列、横向溢出"
        );
        assert_eq!(clamp_span_w(2, None), 2, "还没测到宽度时不夹");
        assert_eq!(clamp_span_w(0, Some(3)), 1);
    }

    #[test]
    fn span_presets_are_within_range() {
        for (w, h) in SPAN_PRESETS {
            assert!((SPAN_MIN..=SPAN_MAX).contains(&w), "{w} 超出范围");
            assert!((SPAN_MIN..=SPAN_MAX).contains(&h), "{h} 超出范围");
        }
    }

    #[test]
    fn bookmark_kind_defaults_to_bookmark() {
        // 缺 kind 的 JSON（老备份、老接口调用方）必须落成书签，而不是未知类型
        let parsed: Bookmark = serde_json::from_str(
            r#"{"id":1,"title":"A","url":"https://a.com","description":"","tags":"",
                "group_id":null,"icon":"","created_at":"","span_w":1,"span_h":1}"#,
        )
        .unwrap();
        assert_eq!(parsed.kind, "");
        assert_eq!(parsed.config, "");
        assert!(!parsed.is_widget(), "空 kind 是书签");
    }

    #[test]
    fn widget_kind_is_recognised_and_labelled() {
        assert!(is_widget_kind(KIND_CLOCK));
        assert!(!is_widget_kind(KIND_BOOKMARK), "空串是书签，不是组件");
        assert!(!is_widget_kind("rainbow"), "白名单外的不算组件");
        assert_eq!(widget_label(KIND_CLOCK), "数字时钟");
        assert_eq!(widget_label("rainbow"), "");
        // 选卡面板直接拿白名单里的图标 id 去画，所以每一项都得指到一个真存在的图标
        for (kind, label, icon) in WIDGET_KINDS {
            assert!(!label.is_empty(), "{kind} 没有显示名");
            assert!(
                crate::components::icons::builtin_id(&format!("i:{icon}")).is_some(),
                "{kind} 引用了不存在的内置图标：{icon}"
            );
        }
    }

    #[test]
    fn clock_config_falls_back_to_defaults_on_junk() {
        let default = ClockConfig::default();
        assert!(!default.hour12, "默认 24 小时制");
        assert!(default.show_seconds, "默认显示秒");

        // 空串（书签那一侧的取值）、乱码、非对象 JSON 一律回落默认，不许 panic
        for junk in ["", "not json", "[]", "123", "null"] {
            assert_eq!(ClockConfig::parse(junk), default, "脏数据 {junk:?} 应回落默认");
        }
    }

    #[test]
    fn clock_config_round_trips_and_fills_missing_fields() {
        let config = ClockConfig {
            hour12: true,
            show_seconds: false,
        };
        assert_eq!(ClockConfig::parse(&config.to_json()), config);

        // 只写了 hour12：缺的 show_seconds 按"真"补，而不是按 bool 的默认假
        let partial = ClockConfig::parse(r#"{"hour12":true}"#);
        assert_eq!(
            partial,
            ClockConfig {
                hour12: true,
                show_seconds: true
            }
        );
    }

    #[test]
    fn weather_config_defaults_and_trims_location() {
        let default = WeatherConfig::default();
        assert_eq!(default.location, "");
        assert!(!default.fahrenheit, "默认摄氏度");

        for junk in ["", "not json", "[]", "123", "null"] {
            assert_eq!(WeatherConfig::parse(junk), default, "脏数据 {junk:?} 应回落默认");
        }

        assert_eq!(WeatherConfig::clean_location("  北京  "), "北京");
        // 按字符截断：中文一个字三字节，按字节切会切出半个字
        let long = "京".repeat(WEATHER_MAX_LOCATION_CHARS + 20);
        let cleaned = WeatherConfig::clean_location(&long);
        assert_eq!(cleaned.chars().count(), WEATHER_MAX_LOCATION_CHARS);
        assert!(cleaned.chars().all(|character| character == '京'));

        let config = WeatherConfig {
            location: "上海".to_string(),
            fahrenheit: true,
        };
        assert_eq!(WeatherConfig::parse(&config.to_json()), config);
    }

    #[test]
    fn weather_codes_have_a_label_and_an_icon() {
        // 每一档都要有说法和图示，且认不出的代码不能回落到空串
        for code in [0u32, 1, 2, 3, 45, 48, 51, 55, 61, 65, 71, 77, 82, 86, 95, 99] {
            let (label, icon) = weather_label(code);
            assert!(!label.is_empty(), "代码 {code} 没有描述");
            assert_ne!(label, "未知", "代码 {code} 应该是认得的");
            assert!(!icon.is_empty(), "代码 {code} 没有图示");
        }

        let (label, icon) = weather_label(9999);
        assert_eq!(label, "未知");
        assert!(!icon.is_empty(), "未知也要有个图示，不能是空白");
    }

    #[test]
    fn dial_config_falls_back_to_defaults_on_junk() {
        let default = DialConfig::default();
        assert!(default.show_seconds, "默认画秒针");
        assert!(!default.show_numbers, "默认用短线刻度");

        for junk in ["", "not json", "[]", "123", "null"] {
            assert_eq!(DialConfig::parse(junk), default, "脏数据 {junk:?} 应回落默认");
        }
    }

    #[test]
    fn dial_config_round_trips_and_fills_missing_fields() {
        let config = DialConfig {
            show_seconds: false,
            show_numbers: true,
        };
        assert_eq!(DialConfig::parse(&config.to_json()), config);

        // 只写了 show_numbers：缺的 show_seconds 按"真"补
        assert_eq!(
            DialConfig::parse(r#"{"show_numbers":true}"#),
            DialConfig {
                show_seconds: true,
                show_numbers: true
            }
        );
    }

    #[test]
    fn note_config_falls_back_to_empty_on_junk() {
        let empty = NoteConfig::default();
        for junk in ["", "not json", "[]", "123", "null"] {
            assert_eq!(NoteConfig::parse(junk), empty, "脏数据 {junk:?} 应回落空笔记");
        }
    }

    #[test]
    fn note_config_round_trips() {
        let note = NoteConfig {
            text: "第一行\n第二行".to_string(),
        };
        assert_eq!(NoteConfig::parse(&note.to_json()), note);
    }

    #[test]
    fn note_text_is_trimmed_and_capped() {
        assert_eq!(NoteConfig::clean("  你好  "), "你好", "首尾空白去掉");
        assert_eq!(NoteConfig::clean("a\n\nb"), "a\n\nb", "中间的换行与空行要保留");

        // 超长按**字符**截断：中文一个字三字节，按字节切会切出半个字
        let long = "字".repeat(NOTE_MAX_CHARS + 50);
        let cleaned = NoteConfig::clean(&long);
        assert_eq!(cleaned.chars().count(), NOTE_MAX_CHARS);
        assert!(cleaned.chars().all(|character| character == '字'));
    }

    #[test]
    fn card_surface_defaults_are_kind_dependent() {
        // 没设过（空串）时：组件卡有面、书签卡没有。这条是"迁移零行为变化"的依据——
        // 既有书签的 surface 都是空串，解析出来必须是"无面"。
        assert_eq!(effective_surface("", false), "none", "书签卡默认不要面");
        assert_eq!(effective_surface("", true), "panel", "组件卡默认要一层");

        // 明确设过的照用，不管是什么种类
        assert_eq!(effective_surface("solid", false), "solid");
        assert_eq!(effective_surface("none", true), "none");
        assert_eq!(effective_surface("panel", true), "panel");

        // 认不出的值当成"没设过"，回落该种类的默认，而不是把乱码当类名用
        assert_eq!(effective_surface("rainbow", false), "none");
        assert_eq!(effective_surface("rainbow", true), "panel");
    }

    #[test]
    fn card_surface_whitelist_round_trips() {
        for (id, label) in CARD_SURFACES {
            assert!(!label.is_empty(), "{id} 没有显示名");
            assert_eq!(normalize_surface(id), id, "{id} 应原样保留");
        }

        // 空串、乱码、大小写不同的一律收敛成空串（＝没设过）
        for junk in ["", "  ", "PANEEL", "PANEL", "Default", "面板"] {
            assert_eq!(normalize_surface(junk), "", "应把 {junk:?} 收敛成没设过");
        }

        // 白名单里的值都能"原样往返"：规范化再解析，结果不变
        for id in ["none", "panel", "solid"] {
            assert_eq!(normalize_surface(id), id);
            assert_eq!(effective_surface(&normalize_surface(id), true), id);
        }
    }

    #[test]
    fn bookmark_surface_class_uses_its_kind() {
        let mut card = Bookmark {
            id: 1,
            title: "A".to_string(),
            url: "https://a.com".to_string(),
            description: String::new(),
            tags: String::new(),
            group_id: None,
            icon: String::new(),
            created_at: String::new(),
            span_w: 1,
            span_h: 1,
            kind: KIND_BOOKMARK.to_string(),
            config: String::new(),
            surface: String::new(),
        };

        assert_eq!(card.surface_class(), "none", "书签卡不设面时是透明的");

        card.kind = KIND_NOTE.to_string();
        assert_eq!(card.surface_class(), "panel", "换成组件就自动有面");

        card.surface = "solid".to_string();
        assert_eq!(card.surface_class(), "solid", "明确设过的压过默认");
    }

    #[test]
    fn system_theme_resolves() {        let mut settings = Settings::default();

        settings.theme = "light".to_string();
        assert_eq!(settings.effective_theme(Some(true)), "light", "定死浅色不看系统");

        settings.theme = "system".to_string();
        assert_eq!(settings.effective_theme(Some(true)), "dark");
        assert_eq!(settings.effective_theme(Some(false)), "light");
        assert_eq!(settings.effective_theme(None), "dark", "探不到时按深色");
    }
}
