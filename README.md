# mtab-lite

极简、可自部署的书签导航页，界面参照浏览器新标签页：大字时钟 + 搜索引擎框 +
分类标签页 + 图标网格。

用 [Dioxus 0.7](https://dioxuslabs.com/) fullstack 写成：同一份 Rust 代码编译出
服务端（Axum + SSR）和浏览器端（WASM + Hydration），数据存单文件 SQLite。

## 功能

- **时钟**：首页顶部实时时钟与日期。只在客户端渲染刷新（避免 SSR 时间不一致）。
- **一言**：时钟下方一句「一言」，默认每 5 分钟换一条。服务端代取接口，
  失败时保留上一条，不显示错误、不打断页面。
- **搜索（搜索引擎）**：左侧可选搜索引擎（预置百度 / 谷歌 / 必应，可自行增删），
  回车或点 `→` **在新标签页**用当前引擎搜索，当前书签页不受影响。
  搜索框只负责搜网页，**不会筛选下方书签**——筛选由分类标签页负责。
- **分类标签页**：由所有书签的 tags 去重生成，带条数（`全部 12` / `rust 3`），
  点选即筛选；某标签下没有书签时自动从标签栏消失并回落"全部"。
- **导入 / 导出**：设置抽屉里可粘贴浏览器导出的书签 HTML（Netscape 格式）批量导入，
  文件夹路径会变成标签，按 URL 自动去重；也可下载无损 JSON 备份并原样导入回来。
- **主页设置**：右上角齿轮打开右侧抽屉——自定义背景图片（支持任意图片地址，
  含随机图接口）、亮度遮罩、每次打开换一张、显示/隐藏时钟与一言，
  以及上述导入导出。
- **添加书签**：页面**空白处右键 → 添加卡片 → 网址导航**，弹窗填表；保存后网格刷新。
- **打开**：点图标在新窗口打开对应网址。
- **站点图标**：卡片上自动显示对应站点的真实图标（服务端代取并缓存）；
  取不到时回退成"名称首字 + 哈希取色"的彩色块。
- **卡片编辑态**：**长按卡片**（按住约 0.5 秒）进入编辑态——卡片显示 ✕ 删除角标与
  拖拽手柄，底部出现「完成」栏。此时：
  - **拖动**卡片可调整顺序（落位后写回数据库）；
  - **点卡片**打开修改弹窗（标题 / URL / 描述 / 标签，字段预填）；
  - **点 ✕** 弹窗二次确认后删除。
- **编辑书签**：卡片**右键 → 编辑该书签**，或长按进入编辑态后点卡片。
- **删除书签**：卡片**右键 → 删除该书签**，或在编辑态点 ✕，均弹窗二次确认。
- **搜索引擎设置**：搜索框下拉里的「设置」→ 新增（名称 / 地址含 `%s` / 可选图标）、
  点选切换、悬停删除。

## 技术栈

| 层 | 选型 |
|----|------|
| 全栈框架 | Dioxus 0.7.10（`fullstack` + `router`） |
| 服务端 | Axum 0.8（由 `dioxus::serve` 托管） |
| 数据库 | SQLite + sqlx 0.9（异步连接池，WAL） |
| 序列化 | serde / serde_json |
| 配置 | dotenvy |
| 一言（仅 server） | reqwest（复用 dioxus 已有的 rustls，无系统 OpenSSL 依赖） |
| 响应压缩（仅 server） | 静态产物构建期 brotli（dx `pre_compress`）+ 动态响应走 tower-http 的 gzip/brotli；详见"关键设计" |
| 时钟 / 搜索跳转（仅 web） | js-sys（`Date`）+ gloo-timers（1s 定时）+ web-sys（`window.open`） |

## 目录结构

```
.
├── Cargo.toml
├── Dioxus.toml
├── Dockerfile                    # 草稿，尚未实测
├── .env.example
├── migrations/
│   └── 001_init.sql              # 单一基线：bookmarks / engines / settings / favicons
├── assets/
│   └── main.css                  # 经 asset! 内嵌进二进制
└── src/
    ├── main.rs                   # 入口：server 走 dioxus::serve，client 走 launch
    ├── backend.rs                # 13 个 #[server] 函数 + 搜索逻辑单测
    ├── db.rs                     # 连接池（仅 server 编译）
    ├── engine.rs                 # 搜索词 URL 编码与模板替换（含单测）
    ├── favicon.rs                # 站点图标：host 解析 + 服务端代取/缓存（仅 server 取图，含单测）
    ├── transfer.rs               # 书签导入解析 / 导出序列化（仅 server，含单测）
    ├── models.rs                 # 前后端共享模型
    ├── routes.rs                 # Routable，仅 "/"
    ├── components/
    │   ├── clock.rs              # 大时钟（仅 web 刷新）
    │   ├── quote.rs              # 一言（仅 web 定时拉取）
    │   ├── search_bar.rs         # 搜索引擎框（引擎选择 + 输入 + 跳转）
    │   ├── engine_logo.rs        # 引擎图标（图片或首字回退）
    │   ├── engine_settings.rs    # 搜索引擎设置（弹窗内容）
    │   ├── tag_tabs.rs           # 分类标签页（带条数）
    │   ├── icon_tile.rs          # 单个书签卡片（含编辑态与拖拽）
    │   ├── bookmark_grid.rs      # 图标网格 + 「添加卡片」，内部管理拖拽态
    │   ├── context_menu.rs       # 自定义右键菜单（含二级菜单）
    │   ├── drawer.rs             # 右侧抽屉外壳
    │   ├── settings_panel.rs     # 主页设置（外观 + 数据导入导出）
    │   ├── modal.rs              # 弹窗外壳
    │   ├── confirm_dialog.rs     # 二次确认弹窗
    │   └── bookmark_form.rs      # 书签表单：新增 / 修改共用
    └── pages/
        └── home.rs               # 状态编排：搜索 / 分类 / 右键菜单 / 弹窗 / 编辑态 / 设置
```

## 关键设计

**Feature 隔离。** 依赖按编译目标切开，`sqlx` / `tokio` / `dotenvy` / `reqwest`
只在 `server` 下启用，`js-sys` / `gloo-timers` / `web-sys` 只在 `web` 下启用：

```toml
[features]
default = []
web    = ["dioxus/web", "dep:gloo-timers", "dep:js-sys", "dep:web-sys"]
server = ["dioxus/server", "dep:sqlx", "dep:tokio", "dep:dotenvy", "dep:anyhow", "dep:reqwest", "dep:tower-http"]
```

WASM 产物里不含数据库驱动与 HTTP 客户端；服务端二进制里不含浏览器 JS 绑定。

**连接池初始化。** 建池是异步的，而 `OnceLock::get_or_init` 只接受同步闭包，
所以在 `main` 里先 `await db::init_pool(...)`，再 `set` 进 `OnceLock`。建池时
开 `create_if_missing`、`journal_mode = WAL`，并执行 `sqlx::migrate!` 跑迁移；
同时会补建数据库文件的父目录（sqlx 不会建）。

**搜索提交必须走 JS，不能用原生表单。** 最初的做法是
`<form method="GET" action="{引擎地址，%s 已替换}">`，指望浏览器直接导航过去。
**这是错的**：按 HTML 规范，表单 GET 提交会**替换** action URL 里原有的查询串，
于是搜索词被清空，实测拿到的是 `https://www.baidu.com/s?`（查询词没了）。

而且各引擎参数名不同（百度 `wd`、谷歌 `q`、必应 `q`），还有把词放在路径里的
模板，原生表单无法通用地表达。所以改为拦掉原生提交、由 JS 精确打开那个 URL：

```rust
onsubmit: move |evt| {
    evt.prevent_default();                    // 原生提交会清空查询串
    window.open(url, "_blank");               // url 由 build_search_url 算好
}
```

`window.open` 由用户手势（点击 / 回车隐式提交）触发，不会被弹窗拦截。
表单上仍保留 `action` / `target` 作为"JS 没跑起来"时的静态兜底。
`%s` 的替换值走自写的百分号编码（`src/engine.rs`，有单测覆盖空格与多字节字符）。

**引擎存在数据库里。** `engines` 表带 `active` 标记，`set_active_engine` 先全置 0
再置 1；删除当前引擎时若没有任何行 active，会自动把列表第一个顶上。这样换浏览器
或换设备后设置仍在。

**一言由服务端代取。** 浏览器不直接访问 `v1.hitokoto.cn`，而是调用自己的
`get_quote` server function，由服务端发请求。这样：避开跨域（虽然该接口
`Access-Control-Allow-Origin: *`，但客户端直连要在 wasm 里再加一个 HTTP 客户端）；
统一做 8 秒超时；失败时前端保留上一条，不显示错误也不打断页面。
`reqwest::Client` 用 `OnceLock` 复用，避免每次请求重做 TLS 握手。

换一条的间隔是 `components/quote.rs` 里的 `REFRESH_SECS`（默认 300 秒），
本地调试可临时调小；和时钟一样，拉取只在 `#[cfg(feature = "web")]` 分支里进行，
SSR 阶段不发外部请求，首屏不会被第三方拖慢。

**时钟的 SSR 一致性。** SSR 阶段无从得知"现在"，因此时钟首屏渲染空串，
挂载后由 `use_future` 每秒刷新。服务端与客户端首次渲染结果一致，不会 hydration
失配。定时器与 `Date` 都只存在于 `#[cfg(feature = "web")]` 分支中。

**搜索 SQL 安全性。** 查询串只由固定模板拼接（按关键词个数重复占位符），
文本中不含用户输入，关键词全部走 `bind`；同时对 `%` `_` `\` 做 LIKE 转义并
配合 `ESCAPE '\'`。sqlx 0.9 的 `SqlSafeStr` 约束用 `AssertSqlSafe` 显式放行。

**错误文案。** `ServerFnError` 的 `Display` 会带上
`error running server function: … (details: None)` 前缀，界面上不该出现。
`backend::error_text()` 只取 `ServerError { message }` 里的原始消息。

**自定义排序。** `bookmarks.sort_order` 是唯一的排序依据，显示顺序为
`ORDER BY sort_order ASC, id ASC`。迁移 003 把存量数据回填成 `sort_order = -id`，
于是升序仍等价于原来的 id 降序（最新在前），默认观感不变；新增书签取
`MIN(sort_order) - 1`，同样落在最前。拖拽落位后前端把**完整 id 顺序**发给
`reorder_bookmarks`，服务端在事务里重写为 `0, 1, 2, …`。

**列表恒为全量。** 书签列表始终取全部（`list_bookmarks("")`），筛选只由分类标签页
在客户端按标签做。这样拖拽重排永远作用在全量顺序上，安全；代价是服务端那套
按关键词的 AND/OR 模糊搜索目前前端不再调用（能力与单测都还在，
`POST /api/list_bookmarks` 传 `{"query":"..."}` 即可用）。

**长按与拖拽的实现。** 长按用指针事件 + 计时器：`pointerdown` 记下坐标并起一个
500ms 定时器，这期间位移超过 8px 或抬手都会作废（避免和点击/滑动打架）。
拖拽用原生 HTML5 DnD（`dragstart` / `dragover` / `drop`），拖拽态（正在拖谁）
收在 `BookmarkGrid` 内部，对外只暴露一个 `on_reorder` 事件。

**编辑态只用「完成」退出。** 曾实现过"点空白处退出"，但会误触发：点弹窗按钮时
弹窗在事件分发过程中被卸载，`stop_propagation` 所在的节点已经不在树上，事件
照旧冒到根节点，导致保存/删除后掉出编辑态。改为只由「完成」按钮退出。

**导入解析在服务端、导入方式是粘贴。** 浏览器导出的书签是 Netscape HTML，
解析（文件夹栈 → 标签、实体解码、`<DD>` 描述）放在 `transfer.rs` 里做，纯函数、
有单测。**没有做文件选择器**：Dioxus 读取文件内容的 API 不够清晰，而导入的
难点在解析、不在取文本，所以走"粘贴文本"这条零不确定性的路（`transfer.rs`
整体只在 `server` 下编译，客户端只负责传文本和展示回执）。

**导出走普通 axum 路由而不是 server function。** `/export/bookmarks.json` 注册在
`dioxus::server::router` 上，浏览器直接以文件形式下载（带
`Content-Disposition: attachment`），客户端不需要任何 Blob/下载相关的 wasm 胶水，
也能用 curl 直接验证。导出为无损 JSON（含标签与顺序），和导入端对称，能原样搬回。

**解析器不判断 URL 是否可用。** 它只忠实提取；`javascript:` 之类的可用性判断
留给导入环节。这样"已导入 N 条 / 跳过 M 条"的口径才和源文件条数对得上
（早先解析器直接丢弃非法项，导致计数对不上，已修正）。

**亮度遮罩只在设了背景图时生效。** 这样"没设背景图"的默认观感与没有该功能时
完全一致，拖到多少都不会莫名其妙变暗；调色板式的默认渐变不受影响。

**随机壁纸要绕开长缓存。** 随机图接口（如 `dmoe.cc/random.php`）通常返回
`Cache-Control: public, max-age=2592000` 这类长缓存头，而地址又始终不变——
不处理的话浏览器会拿同一张缓存图 30 天，"随机"完全失效。所以「每次打开换一张」
开启时会在地址后追加一个随机参数（`?_=<毫秒时间戳>`，已有查询串则用 `&`）。
该参数只在客户端生成：SSR 阶段设置本就还没加载，两边都是空，因此不会
hydration 失配；参数就绪前先不渲染背景，避免先请求一次旧地址再换掉。

**压缩分两层，gzip 与 brotli 都开。**

第一层是**构建期预压缩**，专治体积最大的静态产物。wasm 是内容哈希命名、不可变的，
压一次就够，运行时零 CPU：`Dioxus.toml` 里开 `[web] pre_compress = true`，dx 会为
`public/assets/` 下每个产物写出 `<file>.br`（`BrotliEncoderParams::default()` 的质量
就是 11）；而 dioxus-server 的静态资源走 `ServeFile::precompressed_br()`，磁盘上有
`.br` 就直接发它。实测 wasm 1.15 MB → **br 0.34 MB**（同文件 gzip 是 0.44 MB，
br 再省 24%）。

第二层是 router 上的 `tower_http::compression::CompressionLayer`，负责**动态响应**：
SSR 的 HTML、server function 的 JSON，这些没有 `.br` 文件可发，只能实时压。
它不会重复压第一层的产物（响应已带 `content-encoding`，会跳过），所以实时 brotli
"质量低"这件事在这里不构成问题。质量显式设成 6（HTML 3508 → 3094 B）：
tower-http 给实时 brotli 的默认是 4（沿用 nginx 的取舍），抬到 6 多花的 CPU 可忽略，
而 gzip 的默认本来就是 6，不受影响。只认 gzip 的客户端自动退回 gzip。

如果部署时前面挂了 nginx/Caddy 做压缩，这两层都是冗余的（`content-encoding`
已经带上，规范上代理不该再压），可以去掉。

**站点图标由服务端代取并落库。** 浏览器只请求本服务的 `/icon/{host}`，
服务端按 `host` 命中缓存（`favicons` 表）就直接返回，否则去取并写入。
取图顺序：先试站点自带 `/favicon.ico`，不是图片再抓首页 HTML 解析
`<link rel="icon">`（`apple-touch-icon` 优先，一般分辨率更高）。
两步都必须校验 `Content-Type` 是 `image/`——实测 `claude.ai` 对任意路径都返回
`200 + text/html`，不校验就会把整个网页当成图标存下来。

这个接口**只服务本站已收藏的站点**：要探测就得先加书签，顺带把这个无鉴权接口
被当成任意出站探测（SSRF）入口的风险压掉了。取不到会写一条空数据的负缓存，
7 天内不再重试。前端 `onerror` 拿不到图标就隐藏 `<img>`，露出下面的首字块。

**右键菜单。** 非原生菜单，自己渲染 `position: fixed` 列表 + 透明遮罩。
二级菜单用纯 CSS `:hover` 展开。卡片上的 `contextmenu` 会 `stop_propagation`，
否则会连带触发背景菜单把目标覆盖成"空白处"。

## 环境变量

| 变量 | 必填 | 默认 | 说明 |
|------|------|------|------|
| `DATABASE_URL` | 否 | `sqlite://data/mtab.db?mode=rwc` | SQLite 连接串 |
| `IP` | 否 | `127.0.0.1`（Docker 中 `0.0.0.0`） | 监听地址。注意是 `IP` 不是 `HOST`——由 `dioxus-cli-config` 读取 |
| `PORT` | 否 | `8080` | 监听端口 |

复制 `.env.example` 为 `.env` 后按需修改。

## 本地开发

先装与依赖版本一致的 CLI：

```bash
cargo install dioxus-cli --version 0.7.10 --locked
```

然后：

```bash
cp .env.example .env
dx serve            # 开发模式，默认 http://127.0.0.1:8080
```

## 测试与检查

搜索 SQL 逻辑（AND/OR 分组、LIKE 转义、排序）与搜索词 URL 编码都有单测：

```bash
cargo test --no-default-features --features server

# 两侧类型检查
cargo check --no-default-features --features server
cargo check --no-default-features --features web --target wasm32-unknown-unknown
```

## 生产构建

```bash
dx build --release --platform web
```

产物在 `target/dx/mtab-lite/release/web/`：客户端资源在 `public/`，
服务端二进制名为 `server`（Windows 下 `server.exe`）。

**务必用 release 构建对外提供服务。** 开发构建与正式构建的客户端体积差得很远：

| | `dx serve`（开发） | `dx build --release` |
|---|---|---|
| wasm | 63.5 MB | 1.15 MB（br 后 **0.34 MB**） |
| 首屏总下载 | 63.5 MB | 1.24 MB（br 后约 **0.36 MB**） |

本地想看接近真实的速度，用 `dx serve -r`（release 模式 serve）。

## Docker

> `Dockerfile` 为草稿，产物路径与运行时依赖尚未实测，首次使用请先本地
> `dx build` 确认路径。

```bash
docker build -t mtab-lite .
docker run -d --name mtab-lite \
  -p 8080:8080 \
  -v mtab-data:/data \
  mtab-lite
```

数据落在 `/data/mtab.db`（连同 WAL 的 `-wal` / `-shm` 文件），容器重建不丢。

注意：服务端运行时会读取可执行文件同级的 `public/` 目录，因此镜像里要连同
`public/` 一起拷贝，不能只拷二进制。

## 已知限制

- **一言需要服务端能出网。** 纯内网部署时拉取会失败，此时该行保持空白
  （不影响其他功能）。换一条的间隔改 `src/components/quote.rs` 的 `REFRESH_SECS`。
- **背景图只能是图片直链**，没有上传功能；填的地址会由浏览器直接加载，
  因此需要对方允许跨站引用（CSS 背景图不受 CORS 限制，但可能被防盗链拦）。
- **随机模式下限流**：随机图接口每次打开都会真下载一张图（实测约 1MB / 2 秒），
  且要在 hydration 之后才开始，所以首屏会有一小段没有壁纸。
- **开发构建的客户端极大**：`dx serve` 的 wasm 有 63.5 MB（不优化且带调试信息），
  正式构建 1.15 MB（br 后 0.34 MB）。本地调试感觉慢是正常的，用 `dx serve -r`。
- **压缩没有 zstd**：静态产物是构建期 brotli（q11），动态响应是实时 gzip/brotli（q6）。
  要更省传输就交给反向代理。
- **无鉴权。** 单用户自部署定位，公网暴露请自行加反向代理鉴权。
- **拖拽重排只支持鼠标。** 用的是原生 HTML5 拖放，触屏浏览器不触发；触屏上
  长按仍可进入编辑态，但调整顺序需要指针事件版本（未实现）。
- **退出编辑态只能点「完成」**（见上文，点空白处退出已因误触发移除）。
- **导入只支持粘贴文本**，没有文件选择器（见上文）；几百条书签粘贴一次即可，
  但需要先用文本编辑器打开导出的 .html。
- **迁移是"基线 + 追加"**：`001_init.sql` 是合并后的单一基线（4 张表全在里面），
  但它**一旦被部署过就不能再改**——sqlx 会校验已应用迁移的校验和，改动会导致
  启动直接失败（`migration N was previously applied but has been modified`）。
  这条踩过两次。今后任何表结构变更都必须**新增**文件，比如 `002_xxx.sql`。
- **分类筛选在客户端**：标签栏反映的是当前结果集的标签集合。
- **无书签多选 / 批量删除 / 分页。**
- **首屏已直出列表与标签栏。** 列表走 `use_server_future`，SSR 阶段就把卡片和标签
  渲进 HTML，同一份数据也进 hydration 载荷，客户端**不会再请求一次**。重查（增删改
  排序）另走一个 `use_resource` 且保留旧值，所以不会闪回"加载中"。时钟与一言仍为空：
  时钟依赖"现在"，硬 SSR 会造成 hydration 失配；一言要让服务端出网，会把第三方
  延迟引到自己的 TTFB 上。
- **搜索引擎跳转在新标签页打开**（脚本 `window.open`），当前书签页不会被顶掉。
  代价是依赖 JS：如果 wasm 没加载起来，搜索框点了不会有反应。
- **引擎设置不支持 iTab 的"附加搜索 / 单搜"** 等高级项，只做了新增 / 切换 / 删除。
- `created_at` 存 UTC 时间字符串，界面未展示。
- 图标优先显示站点真实图标（服务端代取，见上文），取不到才回退"首字 + 哈希取色"。
  低分辨率图标（16×16）在大尺寸卡片上会偏软——这是源文件本身的限制。
