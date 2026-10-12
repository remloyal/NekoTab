# mtab-lite 需求文档

> 版本：v1.0
> 日期：2026-10-12
> 状态：待评审
> 项目目录：`E:\project\NekoTab`

---

## 1. 项目概述

### 1.1 背景

浏览器自带的书签管理在跨设备、跨浏览器场景下体验割裂，而主流书签服务（如 Raindrop、Pinboard）功能繁杂、需要注册账号或依赖第三方托管。本项目希望提供一个**极简、可自部署**的书签导航页：部署在自己的服务器上，打开主页就能看到全部书签，支持实时搜索。

### 1.2 目标

用最小可用的功能集，交付一个单二进制部署的书签导航应用：

- 一个页面（主页）承载全部交互；
- 两项核心能力：**书签集中管理** + **快速搜索**；
- 单文件 SQLite 存储，无外部依赖；
- Docker 一条命令启动。

### 1.3 非目标（本期不做）

明确排除，避免范围蔓延：

| 项目 | 说明 |
|------|------|
| 用户系统 / 登录鉴权 | 单用户自部署，默认部署在内网或加反代 Basic Auth |
| 编辑书签 | 本期只做增、查、删，不做改 |
| 标签分类树 / 多级目录 | 标签仅作逗号分隔的字符串存储与匹配 |
| 分页 / 虚拟滚动 | 书签量级预期 < 5000，一次性返回 |
| 导入导出（浏览器书签 HTML） | 后续版本 |
| 收藏夹 / 置顶 / 排序切换 | 固定按 `created_at` 倒序 |
| 国际化 | 界面文案中文为主 |

---

## 2. 技术栈

| 层次 | 选型 | 说明 |
|------|------|------|
| 全栈框架 | Dioxus 0.7.10（`fullstack` feature） | SSR + Hydration 同构 |
| 路由 | `dioxus` 的 `router` feature（即 dioxus-router 0.7.10） | `#[derive(Routable)]`，本期仅 `/` |
| 服务端 | Axum 0.8（Dioxus 内置 server 集成） | 由 `dioxus::serve` 托管 |
| 数据库 | SQLite + sqlx 0.9（异步连接池） | WAL 模式 |
| 工具链 | Rust ≥ 1.85（实机 1.98） | dioxus 0.7.10 的 MSRV 为 1.83 |
| 序列化 | serde / serde_json | Server Function 传输 |
| 运行时 | tokio | 多线程 |
| 配置 | dotenvy | 读取 `.env` |
| Feature 隔离 | `web` / `server` | 客户端与服务端依赖分离 |
| 构建工具 | dioxus-cli (`dx`) | `dx serve` / `dx build` |

### 2.1 Feature 设计

```toml
[features]
default = []
web    = ["dioxus/web"]
server = ["dioxus/server", "dep:sqlx", "dep:tokio", "dep:dotenvy", "dep:anyhow"]
```

约束：`sqlx`、`tokio`、`dotenvy`、`anyhow` 均为 `optional = true`，**只在 `server` feature 下编译进产物**，保证 WASM 客户端包体不含数据库驱动。

---

## 3. 功能需求

### FR-1 主页展示书签列表（必须）

- 访问 `/` 时展示全部书签。
- 排序：`created_at DESC`（最新在前）；`created_at` 相同时以 `id DESC` 兜底保证稳定顺序。
- 每条书签卡片展示：标题、描述（为空则不渲染该行）、标签（逗号分隔渲染为 chip）。
- 列表为空时展示占位提示（区分"暂无书签"与"搜索无结果"两种文案）。

### FR-2 关键词实时搜索（必须）

> ⚠️ **本节已被第 24 节推翻。** 使用反馈"输入搜索词会筛选下面的"，
> 搜索框已改为纯粹搜网页，不再筛选书签；分类标签页的筛选保留。
> 服务端的搜索能力与单测仍在（接口可传 `query`），只是前端不再调用。

- 顶部搜索框，输入即触发服务端搜索，**无提交按钮**。
- 实现方式：`use_signal` 保存 query，`use_resource` 追踪该 signal，输入变化自动重新请求。
- 多关键词：按**空格**分隔，多个关键词之间为 **AND** 关系。
- 匹配字段：`title`、`url`、`description`、`tags` 四个字段。
- 匹配方式：对每个关键词，四个字段中**任一**命中即为该关键词命中（字段间 OR）；所有关键词都命中才返回该行（关键词间 AND）。
- 即 SQL 形态：`AND ( title LIKE ? OR url LIKE ? OR description LIKE ? OR tags LIKE ? )` 重复 N 次，参数为 `%word%`。
- 搜索词需做转义，避免 `%` / `_` 被当作通配符（见 4.2）。
- 输入防抖：建议 200ms（见 NFR-3），避免每敲一个字符打一次库。

### FR-3 添加书签（必须）

- **入口**：页面空白处右键 → 右键菜单「添加卡片」→ 二级菜单「网址导航」→
  弹出模态表单（不使用常驻表单面板）。
- 表单字段：

| 字段 | 必填 | 类型 | 校验 |
|------|------|------|------|
| 标题 | 是 | 文本 | trim 后非空 |
| URL | 是 | 文本 | trim 后非空；建议校验以 `http://` 或 `https://` 开头 |
| 描述 | 否 | 多行文本 | 允许空 |
| 标签 | 否 | 文本 | 逗号分隔，存储前 trim 每个标签并丢弃空项 |

- 提交后调用服务端保存，成功后：
  1. 清空表单；
  2. 触发 `on_saved` 回调，使主页刷新列表。
- 提交中禁用按钮，防止重复提交。

### FR-4 删除书签（建议实现）

- 卡片上提供删除入口。
- 删除前需二次确认。实现为跨平台的两步确认（首点「删除」进入待确认态，
  出现「确认删除 / 取消」），而非 `window.confirm`——后者需引入 web-sys 且
  是平台相关代码。
- 删除成功后刷新列表。

### FR-5 界面风格（必须）

参照浏览器新标签页（iTab 类），自上而下：

1. 大字实时时钟 + 日期（客户端渲染，避免 SSR 时间不一致）；
2. 药丸搜索框（左侧放大镜图标，半透明毛玻璃）；
3. 分类标签页（由 tags 派生，"全部"在前，选中项带下划线）；
4. 图标网格：每个书签为「彩色方块图标 + 名称」，末位固定一个虚线
   「添加卡片」块；
5. 纯 CSS 多层渐变壁纸（不依赖外部图片，离线可用）。

交互：点击图标新窗口打开；空白处右键出菜单；卡片上右键出卡片菜单；
添加/删除均走弹窗。响应式：窄屏缩小时钟与图标尺寸、标签栏自动换行。

---

## 4. 数据库设计

### 4.1 表结构 `bookmarks`

> 下面是**当前的基线**（第 25 节已把 001~006 合并为单一文件）。
> `sort_order` 与索引相对最初设计有调整，详见第 25 节。

```sql
CREATE TABLE IF NOT EXISTS bookmarks (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    title       TEXT NOT NULL,
    url         TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    tags        TEXT NOT NULL DEFAULT '',
    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
    sort_order  INTEGER NOT NULL DEFAULT 0   -- 显示顺序：ORDER BY sort_order ASC, id ASC
);

-- 排序用索引（原设计是 created_at DESC，但显示顺序早已改为按 sort_order）
CREATE INDEX IF NOT EXISTS idx_bookmarks_order
    ON bookmarks (sort_order, id);
```

迁移文件：`migrations/001_init.sql`（单一基线，含全部 4 张表）。

### 4.2 关键约束

- **WAL 模式**：建池时通过 `SqliteConnectOptions::journal_mode(Wal)` 设置。
- **连接池**：`OnceLock<SqlitePool>` 全局初始化一次；`max_connections(5)`。
  建池是异步的，而 `OnceLock::get_or_init` 只接受同步闭包，因此实际做法是
  在 `main` 里先 `await init_pool()`，再 `set` 进 `OnceLock`。
- **迁移**：启动时执行 `sqlx::migrate!("./migrations").run(&pool)`，光有
  SQL 文件不会建表。
- **sqlx 用法**：使用运行时 `sqlx::query()` / `query_as()`，不用编译期检查的
  `sqlx::query!`（后者要求构建时能连库并提供 `DATABASE_URL`，Docker 多阶段
  构建下很痛苦）。sqlx 0.9 对动态拼接的 SQL 有 `SqlSafeStr` 约束，本项目的
  模板拼接已手工审计（文本不含用户输入、值全部 bind），用 `AssertSqlSafe` 放行。
- **`created_at`** 使用 `datetime('now')`，UTC 时间，格式 `YYYY-MM-DD HH:MM:SS`。
- **LIKE 转义**：用户输入中的 `%`、`_`、`\` 需转义，查询时配合 `ESCAPE '\'`，防止通配符注入导致结果异常。
- **SQL 注入防护**：所有动态条件必须用 sqlx 绑定参数，禁止字符串拼接值。

### 4.3 环境变量

| 变量 | 必填 | 默认 | 说明 |
|------|------|------|------|
| `DATABASE_URL` | 是 | — | 如 `sqlite:///data/mtab.db?mode=rwc` |
| `IP` | 否 | `127.0.0.1`（Docker 中 `0.0.0.0`） | 监听地址（变量名是 `IP`，不是 `HOST`） |
| `PORT` | 否 | `8080` | 监听端口 |

---

## 5. Server Functions 接口契约

定义于 `src/backend.rs`，均使用 `#[server]` 标注。

### 5.1 `list_bookmarks`

```rust
#[server]
pub async fn list_bookmarks(query: String) -> Result<Vec<Bookmark>, ServerFnError>
```

- 入参 `query`：原始搜索串（未拆分）。
- 行为：拆分关键词 → 构造 AND/OR 条件 → 按 `created_at DESC, id DESC` 返回。
- 空 `query` 返回全部。

### 5.2 `add_bookmark`

```rust
#[server]
pub async fn add_bookmark(input: NewBookmark) -> Result<i64, ServerFnError>
```

- 返回新插入行的 `id`。
- 服务端需再次校验必填字段（不信任前端）。

### 5.3 `delete_bookmark`

```rust
#[server]
pub async fn delete_bookmark(id: i64) -> Result<(), ServerFnError>
```

- 幂等：删除不存在的 id 不报错。

### 5.4 传输模型（`src/models.rs`）

```rust
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct Bookmark {
    pub id: i64,
    pub title: String,
    pub url: String,
    pub description: String,
    pub tags: String,
    pub created_at: String,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug, Default)]
pub struct NewBookmark {
    pub title: String,
    pub url: String,
    pub description: String,
    pub tags: String,
}
```

> 注：`Bookmark` 与 `NewBookmark` 需在 `web` 与 `server` 两侧都可用，**不加 cfg**。

---

## 6. 项目结构

```
mtab-lite/
├── Cargo.toml
├── Dioxus.toml
├── Dockerfile
├── .env.example
├── README.md
├── migrations/
│   └── 001_init.sql
├── assets/
│   └── main.css
└── src/
    ├── main.rs
    ├── backend.rs          # #[server] 函数
    ├── db.rs               # 仅 server 编译
    ├── models.rs           # 共享模型
    ├── routes.rs           # Routable
    ├── components/
    │   ├── mod.rs
    │   ├── clock.rs
    │   ├── search_bar.rs
    │   ├── tag_tabs.rs
    │   ├── icon_tile.rs
    │   ├── bookmark_grid.rs
    │   ├── context_menu.rs
    │   ├── modal.rs
    │   ├── confirm_dialog.rs
    │   └── add_form.rs
    └── pages/
        ├── mod.rs
        └── home.rs
```

### 6.1 各文件职责

| 文件 | 职责 |
|------|------|
| `main.rs` | 入口；`#[cfg(feature="server")]` 走服务端启动（初始化 DB、启动 axum），`#[cfg(not)]` 走客户端 `launch` |
| `backend.rs` | 三个 server function；内部 `#[cfg(feature="server")]` 引入 `db`、`sqlx` |
| `db.rs` | `init_pool()` / `pool()`；`OnceLock<SqlitePool>`；WAL；整个文件 `#![cfg(feature = "server")]` 或模块级 cfg |
| `models.rs` | `Bookmark`、`NewBookmark` |
| `routes.rs` | `#[derive(Routable)]` 枚举，仅 `#[route("/")] Home {}` |
| `components/*` | 纯展示 + 事件回调，不直接持有全局状态 |
| `pages/home.rs` | 状态编排：`query` signal、`refresh` signal、`use_resource` 拉数据、组合各组件 |

---

## 7. 关键实现要求

### 7.1 编译期隔离

1. `db.rs` 仅 server 编译。
2. `main.rs` 双分支启动。
3. `Cargo.toml` 中 sqlx/tokio/dotenvy 为 optional，仅 server feature 启用。
4. WASM 产物中不得出现 sqlx 符号（可用 `cargo tree --features web` 验证）。

### 7.2 状态与刷新机制

- `Home` 持有 `query: Signal<String>` 与 `refresh: Signal<u32>`。
- `use_resource` 闭包内同时读取 `query()` 与 `refresh()`，任一变化触发重新请求
  （Dioxus 会在依赖变化时取消并重启该 future，因此不会出现响应乱序）。
- 添加/删除成功后 `refresh += 1`。

> **SSR 取舍**：`use_resource` 在 SSR 阶段不阻塞，首屏渲染"加载中"后由客户端
> 补齐数据；若要让首屏直出数据应改用 `use_server_future`，但它在重查时会重新
> suspend，搜索打字体验变差。本项目选择前者。若将来要兼顾，可用
> `use_server_future` 做首屏 + `use_loader`（0.7 新增，重跑不 re-suspend）做搜索。

### 7.5 服务端启动入口

服务端不能用 `dioxus::launch`，需走 `dioxus::serve` 才能在启动前初始化数据库：

```rust
#[cfg(feature = "server")]
dioxus::serve(|| async move {
    // 读 .env、建池、跑迁移
    Ok(dioxus::server::router(App))
});

#[cfg(not(feature = "server"))]
dioxus::launch(App);
```

### 7.3 组件接口

| 组件 | Props |
|------|-------|
| `Clock` | 无（仅 web 刷新，SSR 渲染空串） |
| `SearchBar` | `value: String`, `oninput: EventHandler<String>` |
| `TagTabs` | `tags: Vec<String>`, `active: String`, `on_select: EventHandler<String>` |
| `IconTile` | `bookmark: Bookmark`, `on_context: EventHandler<(f64, f64)>` |
| `BookmarkGrid` | `items: Vec<Bookmark>`, `on_add: EventHandler<()>`, `on_context: EventHandler<(Bookmark, f64, f64)>` |
| `ContextMenu` | `x`, `y`, `target: Option<Bookmark>`, `on_close`/`on_refresh`/`on_add`/`on_delete` |
| `Modal` | `title: String`, `on_close: EventHandler<()>`, `children: Element` |
| `ConfirmDialog` | `title`, `message`, `confirm_label`, `on_confirm`, `on_cancel` |
| `AddForm` | `on_saved: EventHandler<()>`, `on_cancel: EventHandler<()>` |

### 7.4 安全与健壮性

- 服务端二次校验入参。
- LIKE 通配符转义。
- URL 仅渲染为 `<a href>`，需防 `javascript:` 伪协议（渲染前校验 scheme，或统一 `rel="noopener noreferrer"` + `target="_blank"`）。
- 所有 DB 错误映射为 `ServerFnError`，不向客户端泄漏 SQL 细节。

---

## 8. 非功能需求

| 编号 | 需求 | 验收标准 |
|------|------|----------|
| NFR-1 | 可编译 | `cargo check --features web` 与 `cargo check --features server` 均通过，无 warning 级遗漏 use |
| NFR-2 | 可运行 | `dx serve` 本地起服务，浏览器可增/查/删 |
| NFR-3 | 搜索响应 | 输入后 200ms 防抖，服务端单次查询 < 50ms（5000 条量级） |
| NFR-4 | 部署 | `docker build` + `docker run -v ...:/data -p 8080:8080` 即可访问 |
| NFR-5 | 数据持久 | 容器重建后数据仍在（依赖 `/data` 卷） |
| NFR-6 | 首屏 | SSR 直出，未等 hydration 即可见列表 |

---

## 9. 交付物清单

必须交付以下文件，且每个文件内容完整、可直接编译：

- [x] `Cargo.toml`
- [x] `Dioxus.toml`
- [x] `Dockerfile`（草稿，产物路径待实测）
- [x] `migrations/001_init.sql`
- [x] `src/main.rs`
- [x] `src/backend.rs`
- [x] `src/db.rs`
- [x] `src/models.rs`
- [x] `src/routes.rs`
- [x] `src/components/mod.rs`
- [x] `src/components/search_bar.rs`
- [x] `src/components/bookmark_list.rs`
- [x] `src/components/bookmark_card.rs`
- [x] `src/components/add_form.rs`
- [x] `src/pages/mod.rs`
- [x] `src/pages/home.rs`
- [x] `assets/main.css`
- [x] `README.md`

额外补充：`.env.example`、`.gitignore`。

---

## 10. Docker 部署要求

- **多阶段构建**：
  - 构建阶段：`rust:1.85-slim`（≥ dioxus MSRV），`rustup target add wasm32-unknown-unknown`，安装 `dioxus-cli --version 0.7.10`（须与 dioxus 依赖同版本），执行 `dx build --release --platform web`。
  - 运行阶段：`debian:bookworm-slim`，仅复制 server 二进制。
- **环境变量**：`DATABASE_URL`、`IP=0.0.0.0`（注意不是 `HOST`）、`PORT=8080`。
- **端口**：`EXPOSE 8080`。
- **数据卷**：`VOLUME /data`。
- **注意**：运行阶段需安装 `ca-certificates`（sqlx/reqwest 依赖）与 `libsqlite3` 相关运行时（若使用 bundled 则不需要）。

---

## 11. 任务拆解与里程碑

| 阶段 | 任务 | 产出 | 预估 |
|------|------|------|------|
| M1 骨架 | Cargo.toml / Dioxus.toml / feature 配置 / main.rs 双分支 | 能 `cargo check` 通过 | 0.5d |
| M2 数据层 | migrations / db.rs / models.rs | 能建库、连接、WAL 生效 | 0.5d |
| M3 接口层 | backend.rs 三个 server fn | 接口可被前端调用 | 0.5d |
| M4 界面层 | routes / pages/home / 四个组件 / css | 主页可交互 | 1d |
| M5 联调 | 搜索 AND 逻辑、防抖、刷新、删除确认 | 功能全通 | 0.5d |
| M6 交付 | Dockerfile / README / .env.example | 一键部署 | 0.5d |

---

## 12. 验收测试用例

| 编号 | 前置 | 步骤 | 期望 |
|------|------|------|------|
| TC-1 | 空库 | 打开 `/` | 显示"暂无书签" |
| TC-2 | 空库 | 添加 "Rust / https://rust-lang.org / 官方站点 / 编程,rust" | 列表出现该条，表单清空 |
| TC-3 | 有 3 条 | 搜索框输入 `rust` | 仅 title/url/desc/tags 含 rust 的行出现 |
| TC-4 | 有数据 | 输入 `rust 编程` | 同时含两词的行才出现（AND） |
| TC-5 | 有数据 | 输入 `%` | 不返回全部（转义生效） |
| TC-6 | 有数据 | 清空搜索框 | 恢复全部，按时间倒序 |
| TC-7 | 有数据 | 点删除并确认 | 该条消失，刷新后仍不在 |
| TC-8 | 有数据 | 点标题 | 新窗口打开对应 URL |
| TC-9 | 运行中 | `docker restart` | 数据不丢 |
| TC-10 | 构建 | `cargo tree --features web` | 无 sqlx / tokio 依赖树 |

---

## 13. 风险与待确认

| 风险 | 影响 | 应对 |
|------|------|------|
| Dioxus 0.7 API 与示例代码不一致（`use_resource`、`#[server]` 签名随小版本变动） | 编译失败 | 以 `cargo doc` / 官方 0.7 示例为准，锁定具体 patch 版本 |
| `dx build` 在 slim 镜像缺 `pkg-config`/`libssl-dev` | Docker 构建失败 | 构建阶段装齐依赖 |
| `datetime('now')` 为 UTC，用户看到的时间与本地不符 | 体验 | 本期直接展示字符串，或前端转本地 |
| 无鉴权，公网暴露风险 | 安全 | README 明确提示配合反代鉴权 |
| 搜索无分页，数据量大时变慢 | 性能 | 本期接受，后续加 LIMIT |

### 待确认问题

1. 是否需要在卡片上展示 `created_at` 时间？
2. 标签是否点击后可直接作为搜索词？
3. 是否需要"编辑"能力的预留（表结构已支持，仅缺 UI）？
4. Docker 镜像构建时是否联网拉取 crates（离线环境需 vendor）？

---

## 14. 参考

- Dioxus 0.7 Fullstack: <https://dioxuslabs.com/learn/0.7/guide/fullstack>
- sqlx SQLite: <https://docs.rs/sqlx/latest/sqlx/sqlite/>
- SQLite WAL: <https://www.sqlite.org/wal.html>

---

## 15. 实现修订记录（v1.0 → 已交付）

本节记录实现过程中相对原始需求的偏差与补充，均为实测后修正。

### 15.1 版本修正

| 项 | 原需求 | 实际采用 | 原因 |
|----|--------|----------|------|
| Rust | Dockerfile 里写 1.82 | ≥ 1.85 | dioxus 0.7.10 的 MSRV 是 1.83，且 0.7 项目普遍用 edition 2024（需 1.85）；实机 1.98 |
| Dioxus | 0.7 | 0.7.10 | 最新 0.7.x 稳定版（0.8 尚为 alpha） |
| sqlx | 0.9（未指定） | 0.9.0 | 最新稳定版，需 `SqlSafeStr` 适配 |
| dioxus-router | 独立 crate | `dioxus` 的 `router` feature | 官方 0.7 写法，避免版本错配 |

### 15.2 服务端入口（对应需求第 3 条）

需求写"`#[cfg(feature = "server")]` 区分服务端启动"，方向正确，但服务端必须是
`dioxus::serve(|| async { Ok(dioxus::server::router(App)) })` 而不是 `launch`，
否则没有地方在启动前异步初始化数据库连接池。

### 15.3 连接池（对应需求"OnceLock 初始化一次"）

保留 `OnceLock<SqlitePool>`，但**建池在 `main` 中异步完成后再 `set`**。
`OnceLock::get_or_init` 只接受同步闭包，无法直接容纳 sqlx 的异步建池。

补充：sqlx 的 `create_if_missing` 只创建文件、不创建父目录，因此
`init_pool` 里加了 `ensure_parent_dir`，否则 `sqlite://data/mtab.db` 在
`data/` 不存在时会启动失败（实测踩到）。

### 15.4 数据加载 API（对应需求第 4 条）

保留 `use_resource`。需说明取舍：`use_resource` 在 SSR 阶段不阻塞，首屏
渲染"加载中"后由客户端补齐，**没有拿到 SSR 直出数据**；若改用
`use_server_future` 可首屏直出，但搜索重查会重新 suspend。当前选择偏向
搜索打字体验。若要兼顾，可首屏用 `use_server_future` + 搜索用
`use_loader`（0.7 新增，重跑不 re-suspend）。

### 15.5 sqlx 0.9 的 SQL 安全约束

sqlx 0.9 引入 `SqlSafeStr`，动态拼接的 SQL 文本不能直接传给 `query_as`。
本项目把搜索 SQL 的构造抽成 `build_search_sql`，文本仅由固定模板拼成
（不含用户输入），值全部 `bind`，故用 `AssertSqlSafe` 放行。同时把该函数
抽出来以便单测。

### 15.6 删除确认（对应 FR-4）

原写"浏览器原生 `confirm`"。实际改为**跨平台两步确认**（首点「删除」→
「确认删除/取消」），避免为此引入 web-sys 平台相关代码。

### 15.7 资产与 Dioxus.toml

- 0.7 用 `asset!("/assets/main.css")` 在代码中引入 CSS 并内嵌，不通过
  `Dioxus.toml` 配置。`Dioxus.toml` 精简为 `[application]` + `[web.app]`
  （原先手写的 `[web.resource]` 因缺 `dev` 字段会导致 `dx` 解析失败）。
- 实测：服务端运行时会读取可执行文件同级的 `public/` 目录。`dx` 会指好它，
  但**裸跑二进制需要 `public/`**，因此 Dockerfile 不能只拷二进制——已改为
  拷贝整个 `target/dx/<crate>/release/web/` 内容，并标注"待实测"。

### 15.8 已实测验证

- `cargo check`：`server` 与 `web`（wasm32）两个 feature 均通过，零告警。
- 单测：`cargo test --features server`，5 个基于内存 SQLite 的用例覆盖
  AND/OR 分组、LIKE 通配符转义、倒序。
- 端到端（`dx serve` + 浏览器）：
  - SSR 直出 HTML 含页面标记与 hydration 数据；
  - 表格添加 → 列表刷新、表单清空、标签按逗号拆分；
  - 搜索多关键词 AND、标签命中、无结果文案、清空恢复；
  - 两步删除确认 → 记录消失；
  - 重启进程后数据仍在（WAL）。
- 未验证：Docker 镜像构建与运行；`dx build --release` 的产物文件名。

### 15.9 已知遗留

- 搜索无防抖（每次按键一次查询）。
- `created_at` 为 UTC 字符串，界面直接展示，未转本地时区。
- 无鉴权、无编辑、无分页。
- `dx serve` 开发模式下页面顶部会有一个 devtools 覆盖层（"Your app is being
  rebuilt" / "Hot-patch success"），属 dev-only，不影响交互与生产。

---

## 16. 界面改版（v1.1）

按"参照浏览器新标签页"的要求重构了前端，需求侧变更如下。

### 16.1 交互变更

| 项 | v1.0 | v1.1 |
|----|------|------|
| 添加入口 | 页面常驻表单面板 | 空白处右键 → 添加卡片 ▸ 网址导航 → 弹窗 |
| 删除入口 | 卡片悬停显出的按钮 + 两步确认 | 卡片右键 → 删除该书签 → 确认弹窗 |
| 列表形态 | 信息卡片网格（标题/描述/标签） | 图标网格（彩色方块 + 名称），描述转入 tooltip |
| 分类 | 无 | 由 tags 派生的标签页，支持点选筛选 |
| 时钟 | 无 | 大字时钟 + 日期 |

### 16.2 新增/替换的组件

- 新增：`clock`、`tag_tabs`、`icon_tile`、`bookmark_grid`、`context_menu`、
  `modal`、`confirm_dialog`
- 替换：`bookmark_card` + `bookmark_list` → `icon_tile` + `bookmark_grid`
- 重写：`search_bar`（改为药丸 + SVG 图标）、`add_form`（改为弹窗内容，
  增加 `on_cancel`）

### 16.3 新增依赖（仅 web）

```toml
gloo-timers = { version = "0.3", features = ["futures"], optional = true }
js-sys = { version = "0.3", optional = true }
web = ["dioxus/web", "dep:gloo-timers", "dep:js-sys"]
```

两者都已在 dioxus-web 的依赖树中，作为直接依赖不引入新 crate。

### 16.4 需要留意的实现点

- **时钟的 SSR 一致性**：SSR 无法得知当前时间，若服务端与客户端各自取时间会
  导致 hydration 失配。做法是首屏渲染空串，挂载后由 `use_future` 每秒刷新；
  定时器与 `Date` 均置于 `#[cfg(feature = "web")]` 分支。
- **右键事件冒泡**：卡片 `contextmenu` 必须 `stop_propagation`，否则会继续冒泡到
  根节点的空白处菜单，把菜单目标覆盖成 `None`。
- **弹窗卸载后的 signal 写入**：提交成功后先把 `busy` 复位、再回调关闭弹窗，
  避免组件卸载后再写 signal。
- **二级菜单**：纯 CSS `:hover` 展开，不引入额外状态；子菜单在父 `li` 内部，
  因此鼠标从父项移入子项时父项仍处于 hover 态，点击不会中断。
- **IAB 测试环境注意**：Playwright 的 `fill("")` 在部分后端是空操作，清空搜索框
  需逐字符退格；`contextmenu` 需通过派发事件触发（CUA 不支持右键）。

### 16.5 已实测（v1.1）

`dx serve` + 浏览器逐项走通：时钟走动、右键菜单定位与二级菜单展开、
「网址导航」→弹窗→保存→网格刷新且分类标签自动新增、标签页筛选、
服务端搜索与多关键词 AND、卡片右键→删除确认→图标消失且空标签自动移除。
`cargo test --features server` 5 个用例仍全过，两个 feature 编译零告警。

---

## 17. 搜索引擎（v1.2）

搜索框从"只筛书签"升级为"搜索引擎框"，并支持自管理引擎。

### 17.1 交互

| 项 | v1.1 | v1.2 |
|----|------|------|
| 搜索框左侧 | 放大镜图标 | 当前引擎图标 + 下拉箭头 |
| 提交 | 无（仅实时筛选） | 回车或点 `→` 跳转到引擎结果页 |
| 引擎下拉 | 无 | 引擎列表（含选中标记）+「设置」 |
| 引擎管理 | 无 | 设置弹窗：新增 / 点选切换 / 悬停删除 |
| 书签筛选 | 实时筛选 | **保留**：打字仍实时筛选下方图标 |

即：**打字筛书签、回车搜网页**，两种用法并存，不牺牲原有功能。

### 17.2 数据与接口

新增迁移 `migrations/002_engines.sql`：

```sql
CREATE TABLE IF NOT EXISTS engines (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    url  TEXT NOT NULL,          -- 含 %s 占位
    icon TEXT NOT NULL DEFAULT '',
    active INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
-- 预置百度(active) / 谷歌 / 必应
```

新增 4 个 server function：`list_engines` / `add_engine` / `delete_engine` /
`set_active_engine`。约束：

- `url` 必须以 `http(s)://` 开头且**必须包含 `%s`**，否则服务端报错；
- 新增时若当前无任何引擎 active，新引擎自动生效；
- 删除当前引擎后若无 active，自动把列表第一个顶上（避免"没有引擎可用"）。

### 17.3 提交实现（关键点）

> ⚠️ **本节方案已被第 23 节推翻。** 下面"用原生表单 GET 把 `%s` 拼进 action"
> 的做法是错的：表单 GET 提交会**替换** action 里的查询串，导致搜索词被清空
> （实测拿到 `https://www.baidu.com/s?`）。保留原文仅为记录当时的判断。

搜索框渲染为原生表单：

```rust
form {
    action: "{build_search_url(&engine.url, &query)}",  // %s 已替换
    method: "GET",
    input { r#type: "search", /* 刻意不给 name */ }
    button { r#type: "submit", "→" }
}
```

- 输入框**不带 `name`**，因此 GET 提交不会往 action 上再拼参数，等价于纯导航；
- 由此**不需要任何 JS 跳转代码**，也就不需要为跳转引入 web-sys；
- `%s` 的替换值经 `src/engine.rs` 的百分号编码（空格→`%20`，多字节按 UTF-8
  字节转 `%XX`），该模块有 3 个单测。

### 17.4 新增文件

- `src/engine.rs` —— URL 编码与模板替换（含单测）
- `migrations/002_engines.sql`
- `src/components/engine_logo.rs` —— 引擎图标（图片地址或首字彩色圆点回退）
- `src/components/engine_settings.rs` —— 引擎设置弹窗内容
- 改写 `src/components/search_bar.rs`（引擎选择 + 输入 + 跳转）

### 17.5 顺带修掉的问题

`ServerFnError` 的 `Display` 会带上
`error running server function: … (details: None)` 前缀，之前直接 `to_string()`
展示给用户。新增 `backend::error_text()` 只取
`ServerFnError::ServerError { message, .. }` 里的原始消息，书签与引擎两处表单
都已改用。

### 17.6 已实测（v1.2）

`dx serve` + 浏览器逐项走通：

- 引擎下拉展开，百度带选中标记，谷/必各带首字图标，底部「设置」；
- 切到谷歌后：选择器图标变「谷」、表单 `action` 变为
  `https://www.google.com/search?q=`；
- 输入 `rust 中文` 后 `action` 为
  `https://www.google.com/search?q=rust%20%E4%B8%AD%E6%96%87`（编码正确）；
- 点提交按钮触发跳转（查询为空时被 Google 重定向到 `google.com.hk/webhp`）；
- 设置弹窗：标题正确、列出 3 个引擎、新增 DuckDuckGo 后列表变 4 条；
- 地址填 `not-a-url` 报「地址需以 http:// 或 https:// 开头」，
  填不含 `%s` 的地址报「地址必须包含 %s 作为搜索词占位」，列表均不变；
- 删除 DuckDuckGo 后回到 3 条；
- 错误文案已无 `ServerFnError` 包装前缀。

`cargo test --features server` 8 个用例全过（5 书签 + 3 引擎），
两个 feature 编译零告警。

### 17.7 未能验证 / 遗留

- **回车键提交未能自动化验证**：本环境的浏览器自动化键盘输入送不到页面
  （装了 `keydown` 监听器做诊断，事件从未触发），因此只能验证"点提交按钮会跳转"。
  表单是标准的「一个 submit 按钮 + GET」，回车走的是浏览器的隐式提交，实际浏览器
  中应正常；但这一点未取得实测证据，需要人工在真机确认一次。
- 未做 iTab 的「附加搜索 / 单搜」多引擎并行搜索。
- 跳转为整页跳转（当前标签页），非新标签页。

---

## 18. 卡片编辑态与拖拽排序（v1.3）

针对"卡片缺少功能"的补充：拖拽调整位置、长按进入编辑态、编辑态下删除与修改。

> **术语说明**：需求原文说的是"标签"。结合随附截图（卡片带 ✕ 角标与拖拽手柄、
> 底部有编辑栏），这里按**首页的图标卡片**实现。若本意是**分类标签页那一行**，
> 需要另加 `categories` 表来存顺序与显示名（当前分类是从书签 tags 派生的），
> 属于另一套改动。

### 18.1 交互

| 操作 | 行为 |
|------|------|
| 长按卡片（≈0.5s） | 进入编辑态 |
| 编辑态拖动卡片 | 调整顺序，落位后写库 |
| 编辑态点卡片 | 打开修改弹窗（字段预填） |
| 编辑态点 ✕ | 二次确认后删除 |
| 点底部「完成」 | 退出编辑态 |

### 18.2 数据层

迁移 `003_sort_order.sql`：

```sql
ALTER TABLE bookmarks ADD COLUMN sort_order INTEGER NOT NULL DEFAULT 0;
UPDATE bookmarks SET sort_order = -id;   -- 回填，保持原有序观感
```

- 显示顺序统一为 `ORDER BY sort_order ASC, id ASC`；
- 回填 `-id` 使升序等价于原来的 id 降序（最新在前），**默认顺序零变化**；
- 新增书签取 `MIN(sort_order) - 1`，落在最前，与原"最新在前"一致；
- 重排把全量 id 顺序写为 `0, 1, 2, …`。

新增 2 个 server function：

- `update_bookmark(id, input)` —— 改标题/URL/描述/标签，不动排序位；
- `reorder_bookmarks(ids)` —— 事务内按传入顺序重写 `sort_order`。

并把校验规范化抽成 `clean_bookmark()`，新增与修改共用，避免两处漂移。

### 18.3 实现要点

- **长按**：指针事件 + 计时器。`pointerdown` 记坐标并起 500ms 定时器，期间位移
  超过 8px 或抬手即作废（与点击、滑动区分）。定时器在 web 分支内，用
  `gloo_timers::future::TimeoutFuture`。
- **拖拽**：原生 HTML5 DnD；`dragstart` 顺带 `set_data`（部分浏览器要求拖拽必须
  带数据才有效）。拖拽态收在 `BookmarkGrid` 内，对外只暴露 `on_reorder`。
- **搜索时禁用拖拽**：搜索走服务端，客户端只有匹配子集，基于子集算序会破坏其他
  记录位置。标签筛选不受影响（客户端过滤，服务端返回全量）。故
  `can_drag = !searching`。
- **修改弹窗复用新增表单**：`AddForm` 改为 `BookmarkForm`，`bookmark: Option<Bookmark>`
  决定新增还是修改；调用处加 `key: "{id}"`，避免切换不同书签时沿用上一次的预填值。

### 18.4 修掉的一个交互 bug

最初实现了"编辑态点空白处退出"。它会**误触发**：点弹窗里的按钮时，弹窗在事件
分发过程中被卸载，`stop_propagation` 所在的节点已不在树上，事件照旧冒到根节点，
于是保存/删除后直接掉出编辑态（实测确认）。

修复：去掉根节点的退出处理，编辑态**只由「完成」按钮退出**。这也更可预期。

### 18.5 已实测（v1.3）

- 迁移 003 生效：库中 `sort_order` 为 `-1,-3,-4,-5,-6`，接口返回顺序与改动前完全一致；
- 长按 900ms 后确实进入编辑态（5 个 ✕ 角标、5 个手柄、底栏出现、`draggable="true"`）；
- 点卡片打开「修改网址导航」弹窗，字段预填正确，且未发生跳转（`prevent_default` 生效）；
- 改标题 + 改标签保存后：卡片名更新、分类栏自动新增标签「书」、排序位未变；
- ✕ → 确认删除后卡片消失，**且编辑态保持**（bug 修复后）；
- 拖拽末位卡片到首位：界面顺序变化，库中 `sort_order` 重写为 `0,1,2`，接口顺序一致；
- 输入搜索词后 `draggable` 变 `false`、手柄消失、提示文案切换为"搜索中不能调整顺序"；
- 点「完成」可退出编辑态。

`cargo test --features server` 9 个用例全过（新增 `custom_sort_order_is_respected`），
两个 feature 编译零告警。

### 18.6 遗留

- **拖拽重排只支持鼠标**：原生 HTML5 拖放触屏不触发。触屏上可长按进入编辑态，
  但调序需改用指针事件实现，未做。
- 未做截图里的「↑ 置顶」角标——拖拽已经覆盖了这个需求。
- 未做多选 / 批量删除（截图里的"批量编辑"只做了单选删除）。

---

## 19. 一言（v1.4）

对接「一言」接口（`https://v1.hitokoto.cn/`），默认 5 分钟换一条。

### 19.1 放置位置

时钟日期下方、搜索框上方。这是新标签页的常见位置（iTab 等同款），
视觉上从属于"时钟区"，不打断"搜索 → 分类 → 卡片"的主流程。

布局顺序：`时钟 → 一言 → 搜索框 → 分类标签页 → 图标网格`。

### 19.2 服务端代取，而非浏览器直连

接口本身允许跨域（响应头 `Access-Control-Allow-Origin: *`），HTTP 也可用，
所以浏览器直连是可行的。但仍选择**服务端代取**：

| | 浏览器直连 | 服务端代取（采用） |
|---|---|---|
| 依赖 | 需在 wasm 侧再加 HTTP 客户端 | 复用已在依赖树里的 reqwest |
| 网络 | 受用户浏览器网络限制 | 只用服务端出网 |
| 跨域 | 依赖对方 CORS 策略 | 不涉及 |
| 超时/兜底 | 分散在前端 | 集中在 server function |

请求加 8 秒超时，`reqwest::Client` 用 `OnceLock` 复用（避免重复 TLS 握手）。

### 19.3 数据结构

```rust
pub struct Quote {
    pub text: String,    // 一言正文
    pub source: String,  // 已拼好的出处
}
```

接口返回 `hitokoto`（正文）、`from`（出处作品）、`from_who`（作者，可空）。
`source` 在服务端拼好，规则：

- 作者与作品都有 → `作者《作品》`
- 只有作者 → `作者`
- 否则 → 作品

### 19.4 刷新与容错

- 间隔：`components/quote.rs` 的 `REFRESH_SECS`，默认 `300`（5 分钟）。
  本地调试可临时调小，例如 `10`。
- 首次进入立即拉一次，之后按间隔循环。
- **拉取失败保留上一条**（`if let Ok(...)`），不显示错误、不清空、不打断页面；
  完全没拉到过就保持空白行（`min-height` 占位，避免布局跳动）。
- 只在 `#[cfg(feature = "web")]` 分支里拉取：SSR 阶段不发外部请求，首屏不被
  第三方接口拖慢，也保证服务端与客户端首次渲染一致（都是空行）。

### 19.5 新增依赖

```toml
reqwest = { version = "0.12", optional = true, default-features = false, features = ["json", "rustls-tls"] }
server = [..., "dep:reqwest"]
```

关键点：**不引入任何新 crate**。dioxus-fullstack 已经把 reqwest 以
`rustls-tls` 编进依赖树（锁定文件里只有 `rustls`/`ring`/`webpki-roots`，
没有 `native-tls`/`openssl-sys`），这里只是额外启用 `json` feature。
因此 Docker 的 slim 镜像也不需要装 OpenSSL。

### 19.6 已实测（v1.4）

- `get_quote` 端点直连可通：返回 `{"text":"...","source":"刘慈欣《三体II·黑暗森林》"}`，
  作者 + 作品拼接正确；只有出处的（如 `网络`）也正常；
- 浏览器官网页面：一言显示在时钟与搜索框之间（时钟 y=65、一言 y=185、搜索 y=229，
  顺序正确），样式与出处格式正确；
- 把 `REFRESH_SECS` 临时改为 10 后重启：服务端日志显示 `get_quote` 每 10~12 秒
  一次，页面文本在 22 秒内出现 3 条不同内容，刷新链路确认可用；
- 恢复 300 后：30 秒内 `get_quote` 无新增调用，5 分钟间隔确认。

`cargo test --features server` 9 个用例全过，两个 feature 编译零告警。

### 19.7 过程记录与遗留

- 中途一次"刷新不生效"是**测到了旧产物**：改完常量后 dx 的 watcher 没有重建
  （wasm mtime 早于源文件 mtime），重启 `dx serve` 后即正常。断言前先核对
  产物时间比反复调试代码更省事。
- 短间隔下连续几次拿到同一条，是接口 CDN 的短暂缓存（直连接口同样复现），
  非本项目问题；5 分钟间隔下不受影响。
- 未做：分类筛选（`?c=`）、缓存、离线兜底文案。

---

## 20. 导入导出 + 主页设置抽屉（v1.5）

承接"还缺什么"的结论，落地了两项：**① 浏览器书签导入/导出**；
**② 一处低成本体验修正**（右键「编辑」、搜索防抖、分类计数）；
并额外加了 **③ 可配置背景图 + 右上角设置抽屉**。

### 20.1 导入

- **格式**：浏览器导出的 Netscape 书签 HTML（自动识别），也支持本应用导出的 JSON。
- **入口**：设置抽屉 →「数据」→ 粘贴文本 →「开始导入」。
- **映射**：`<H3>` 文件夹路径（含嵌套，逗号连接）→ 标签；
  `<DD>` → 描述；HTML 实体（含数字实体）解码。
- **去重**：按 URL，含库内已有的；重复与非法（非 http/https）计入"跳过"。
- **顺序**：新记录追加在现有排序之后，并保持文件内顺序。

**为什么用粘贴而不是选文件**：Dioxus 读取文件内容的 API 不够清晰，且浏览器
自动化明确不支持文件选择器，那条路无法验证。导入的难点在解析而非取文本，
故选择零不确定性的粘贴（`transfer.rs` 整体 `#[cfg(feature = "server")]`，
客户端只传文本、收回执）。

**一个职责修正**：最初解析器会直接丢弃 `javascript:` 之类的链接，导致导入回执
"跳过 N 条"与源文件条数对不上（源文件 6 条，报 4 导入 1 跳过）。改为
**解析器忠实提取、可用性由导入环节判定**后，回执口径与源文件一致。

### 20.2 导出

走普通 axum 路由 `GET /export/bookmarks.json`，注册在 `dioxus::server::router` 上，
带 `Content-Disposition: attachment` 直接触发下载。这样客户端不需要任何
Blob/下载相关的 wasm 胶水，也能用 curl 直接验证。内容为无损 JSON（含标签与顺序），
与导入端对称，可原样搬回。

### 20.3 主页设置抽屉

右上角齿轮 → 右侧抽屉（`Drawer` + `SettingsPanel`），分两段：

| 段 | 项 |
|----|-----|
| 外观 | 背景图片地址、亮度遮罩（0-80%）、显示时钟、显示一言 |
| 数据 | 导入书签（textarea）、导出书签（下载链接） |

设置存 `settings` 键值表（迁移 004），新增 3 个 server function：
`get_settings` / `save_settings` / `import_bookmarks`（加上导出逻辑共 13 个）。

**亮度遮罩只在设了背景图时生效**：这样"没设背景图"的默认观感与没有该功能时
完全一致，避免用户一进来就发现页面莫名变暗。

### 20.4 顺带的体验修正

- **右键菜单加「编辑该书签」**：此前修改只能"长按进编辑态 → 点卡片"，桌面用户
  第一反应是右键，这是一处发现性缺口。
- **搜索防抖 200ms**：利用 `use_resource` 在依赖变化时重启 future 的语义，
  在查询前先 `TimeoutFuture::new(200).await`，打字期间的输入会取消上一个查询。
- **分类标签页显示条数**：`全部 12` / `rust 3`。

### 20.5 踩的坑：改了已应用的迁移

把 `004_settings.sql` 的默认值从 35 改成 20 后，服务端启动直接失败：

```
migration 4 was previously applied but has been modified
```

sqlx 会校验已应用迁移文件的校验和。**正确做法是新增一个迁移，而不是改历史。**
（本次因为该迁移只影响开发库里的种子值，选择把文件**逐字节还原**——并用
`sha384(文件) == _sqlx_migrations.checksum` 验证了还原正确，而不是靠记忆。）

### 20.6 已实测（v1.5）

- **导出**：`/export/bookmarks.json` 返回 200，`Content-Type` 与
  `Content-Disposition: attachment; filename="mtab-bookmarks.json"` 正确，
  正文按当前显示顺序输出无损 JSON；
- **导入（接口）**：粘贴含嵌套文件夹、实体、重复项与非 http 链接的样例，
  回执 `{"imported":4,"skipped":1}`；修正口径后另一组样例
  （1 新 + 1 重复 + 2 非法）回执 `{"imported":1,"skipped":3}`，与源文件一致；
  分层标签正确（`AI 工具` / `AI 工具,国产`）；
- **导入（界面）**：抽屉粘贴 → 回执「已导入 2 条，跳过 1 条」、文本框清空、
  网格自动刷新（8 → 10 条，含新导入的 Vite / esbuild）；
- **抽屉**：齿轮打开，标题/两段/字段/开关/导出链接齐全；设背景图 URL 与遮罩 60%
  后点保存 → `.bg` 得到 `background-image`、`.bg-dim` 得到 `rgba(0,0,0,0.6)`、
  显示「已保存」，且落库（实测后再还原为默认，避免留测试痕迹）；
- **分类计数**：标签栏显示 `全部 8` / `AI 工具 3` 等；
- **右键编辑**：菜单出现「编辑该书签」，点击打开「修改网址导航」且字段预填正确；
- **搜索防抖**：连续 5 次快速输入（间隔 60ms）服务端只收到 1 次
  `list_bookmarks`；再连打 4 次，计数同样只 +1。

`cargo test --features server` 17 个用例全过（新增 8 个导入解析用例），
两个 feature 编译零告警。

### 20.7 遗留

- 导入没有文件选择器（见 20.1）；导出只有 JSON，没有 Netscape HTML
  （导出成 HTML 才能导回浏览器，多标签信息会退化）。
- 设置里没有壁纸上传，只能填图片地址。
- 仍未做：鉴权、分页、多选批量操作、触屏拖拽、favicon。

---

## 21. 随机壁纸（v1.6）

针对「能支持 URL 吗，比如 `https://www.dmoe.cc/random.php`」的追问。

### 21.1 结论

URL 本身**能用**——实测该接口直接返回 `200` + `image/jpeg`（1920×1080），
且**不校验 Referer**（带我们页面的 Referer 也正常返回），所以没有防盗链问题，
作为 CSS `background-image` 可以直接加载。

### 21.2 但直接填会有个坑

该接口返回：

```
Cache-Control: public, max-age=2592000
```

即 **30 天缓存**，而地址始终是同一个。浏览器会把第一张图缓存 30 天，
"随机壁纸"实际上变成"一张用 30 天"。这不是接口的问题，也不是代码问题，
是"随机图接口 + 静态地址"的固有矛盾。

### 21.3 处理

新增设置项 `background_random`（迁移 005，默认关）：

- 开启后，渲染前给地址追加一个随机参数：`?_=<毫秒时间戳>`，
  地址已有查询串时改用 `&`；
- 参数**只在客户端生成**。SSR 阶段设置本就还没加载，服务端与客户端首屏
  都是"没有背景图"，因此不会 hydration 失配；
- 参数就绪前**先不渲染背景**，避免先按旧地址请求一次再换掉（白下一次）。

设置面板「外观」段多了一个复选框「每次打开换一张」，并配了说明文案解释
为什么需要它。

### 21.4 已实测（v1.6）

- 迁移 005 生效，`settings` 表出现 `background_random`；
- 填入该接口地址并勾选随机后：`.bg` 的样式为
  `url("https://www.dmoe.cc/random.php?_=1791800697468")`；
- 刷新一次后随机参数变为 `?_=1791800712228`，**与上次不同**，说明每次都绕开了缓存；
- `performance.getEntriesByType("resource")` 中能查到对应请求（耗时约 2.2 秒、
  约 1MB；跨域无 `Timing-Allow-Origin`，故 `transferSize` 显示 0 属正常）；
- 截图确认壁纸确实换成了新的动漫图，遮罩按设定值生效。

### 21.5 遗留

- 随机模式下每次打开都要真下载一张图（约 1MB），且需等 hydration 之后才开始，
  首屏会有一小段没有壁纸；想避免就关掉该开关用固定图。
- 没有做壁纸预加载 / 上一张缓存，也没有"每次刷新换一张"之外的策略
  （如按小时换）。
- 壁纸只能填地址，没有上传；不支持本地文件。

---

## 22. 站点图标（v1.7）

针对「能不能获取到对应标签链接地址的 icon」的追问，落地真实站点图标。

### 22.1 先量后做

直接取站点自身 `/favicon.ico` 的命中率实测（12 个域名）：

| 结果 | 站点 |
|------|------|
| 200 + 图片 | developer.mozilla.org、doc.rust-lang.org、www.deepseek.com、www.dmoe.cc、www.baidu.com |
| 404（图标是 `<link rel="icon">` 声明的） | rust-lang.org、vitejs.dev、esbuild.github.io |
| **200 但是 `text/html`** | **claude.ai** |
| 连接失败 | github.com、news.ycombinator.com、chat.openai.com |

两个结论直接决定了设计：

1. 只试 `/favicon.ico` 会漏掉约 1/4 的站点 → 需要补一步"抓首页解析 `<link rel="icon">`"；
2. **必须校验 `Content-Type` 真的是图片** → 否则 `claude.ai` 这类"任意路径都返回
   200 + 网页"的站点会把整个 HTML 当图标存进库。

（连接失败的属于本机网络到不了目标域名，非实现问题；换浏览器 UA 后
github.com 实测能取到。）

### 22.2 实现

- **服务端代取，浏览器只请求本服务**：`GET /icon/{host}`（普通 axum 路由，
  与 `/export` 一样挂在 `dioxus::server::router` 上）。客户端不直接访问第三方站点。
- **取图顺序**：`{scheme}://{host}/favicon.ico` → 首页 HTML 里声明的图标
  （最多试 3 个候选，`apple-touch-icon` 优先，一般分辨率更高）。
  scheme 取自书签 URL 本身，所以内网的 `http://192.168.x.x` 也能正常取。
- **HTML 只读到 `</head>` 或 256KB**，不为一个图标下载整页
  （用 `reqwest` 的 `chunk()` 增量读，故启用了它的 `stream` feature）。
- **缓存**：`favicons` 表（迁移 006）存 host / content_type / data。
  取不到也写一条空数据的**负缓存**，7 天内不再重试，避免反复打扰对方站点。
- **安全**：`host_of()` 只允许字母数字与 `.-:_`，天然排除路径穿越字符；
  且 `/icon/{host}` **只服务已收藏站点**——要探测就得先加书签，把这个无鉴权
  接口被当作任意出站探测（SSRF）入口的风险压掉。
- **前端**：`<img src="/icon/{host}">` 盖在首字色块之上，`onerror` 时隐藏它、
  露出首字块，做到"有图标用图标、没有就用首字"。

### 22.3 视觉上的一个迭代

第一版让图标 `object-fit: cover` 铺满整个色块，结果 **16×16 的小图标被放大
3.5 倍，糊成一团**。改为：图标缩进到一块 **38px 亮色圆角底盘**里、`contain` 显示。

这样同时解决三件事：放大倍数降到约 2 倍（明显改善）；不透明底盘正好盖住下面的
首字，透明背景的图标不会透出字母；站点图标多是为浅色底设计的，浅底盘比彩色底更还原。

### 22.4 已实测（v1.7）

- 迁移 006 生效，`favicons` 表建立；
- 逐个打 `/icon/{host}`：**10 个书签站点全部 200 并落库**，
  其中 rust-lang.org / vitejs.dev / esbuild.github.io / claude.ai 是靠第二步
  解析 HTML 救回来的（`claude.ai` 同时验证了 CT 校验挡掉了 `text/html` 误判）；
- `dmoe.cc` / `baidu.com` 返回 404 —— **这是正确行为**，它们不是书签
  （dmoe 是背景图、baidu 只是预置搜索引擎），正是那道"只服务已收藏站点"的防护；
- 浏览器里 10 个图标全部 `naturalWidth > 0`、0 个破图；
- 回退路径：临时加一条指向不存在域名的书签，确认 `img` 被移除、显示首字块
  「无」，验证后已删除该测试数据；
- 负缓存：该不存在域名在 `favicons` 里留下一条空数据记录。

`cargo test --features server` 20 个用例全过（新增 3 个 host 解析用例），
两个 feature 编译零告警。

### 22.5 遗留

- 没有像 iTab 那样接第三方图标服务（更高分辨率、更全），因为那会把用户收藏的
  域名列表交给第三方，且这类服务在部分网络下不可达；代价是低分辨率图标偏软。
- 图标不随书签删除而清理（缓存表独立），也没有做容量上限与过期清理。
- 未解析 ICO/PNG 的实际尺寸，因此无法自动拒绝"太小注定会糊"的图标。
- 内网站点首次取图会等一个超时（最长 8 秒）才回退。

---

## 23. 修正：搜索跳转丢查询串（v1.8）

使用者反馈"搜索不起作用，三个引擎都不行，点搜索应该打开新页面"。
排查后确认是**两个问题叠在一起**，其中一个是真 bug。

### 23.1 真 bug：表单 GET 会清空 action 里的查询串

17.3 曾把"用原生表单 GET、把 `%s` 拼进 action"当作优点（"不需要任何 JS"）。
这个做法**根本上是错的**：按 HTML 规范，表单 GET 提交时浏览器会用表单数据
**替换** action URL 的查询串。我们的输入框刻意不带 `name`（当时为了避免
额外参数），于是查询串被替换成空 —— 实测点箭头打开的是
`https://www.baidu.com/s?`，**搜索词被丢掉了**。

**为什么之前"验证通过"了**：早先我只检查了"跳转到了搜索引擎域名"，
而那次查询词是空的，恰好掩盖了问题。教训是**验证要检查完整 URL，
而不是只看域名这类粗粒度特征**。

### 23.2 需求：结果要开在新标签页

原实现是整页跳转，会把当前书签页顶掉；使用者期望新标签页打开。

### 23.3 修复

原生表单无法通用地解决这两点：不但查询串会被替换，而且各引擎参数名不同
（百度 `wd`、谷歌/必应 `q`），还有把词放在路径里的模板。

改为拦掉原生提交、由 JS 精确打开：

```rust
onsubmit: move |evt| {
    evt.prevent_default();          // 原生提交会清空查询串
    window.open(url, "_blank");     // url 由已带单测的 build_search_url 算好
}
```

- `open_in_new_tab()` 用 `web-sys` 的 `open_with_url_and_target`，因此
  **web feature 新增 `web-sys` 依赖**（本来就已在依赖树里，只启用 `Window`）；
- 由用户手势（点击 / 回车隐式提交）触发，不会被弹窗拦截；
- 表单上保留 `action` / `target` 作为 JS 未加载时的静态兜底；
- `Dioxus` 的 `form` 不支持 `rel` 属性（编译报错），故未挂 `noopener` ——
  现代浏览器对 `target=_blank` 已默认 noopener。

### 23.4 已实测（v1.8）

- 百度：点箭头后新标签 URL 为
  `https://www.baidu.com/s?wd=rust%20%E6%95%99%E7%A8%8B`，标签标题
  "rust 教程_百度搜索"（说明搜索真的执行了）；**当前书签页 URL 未变**；
- 切换到谷歌（参数名 `q`）再测：新标签 URL 为
  `https://www.google.com/search?q=rust%20lang`，标题 "rust lang - Google 搜索"；
- 对照：修复前同一操作打开的是 `https://www.baidu.com/s?`（查询词丢失）。

`cargo test --features server` 20 个用例全过，两个 feature 编译零告警。

### 23.5 遗留

- 若 wasm 没加载起来，搜索框点了不会有反应（JS 依赖）。原生兜底的 URL
  因 23.1 的规范行为本身不可用，属于已知取舍。
- 未验证"回车"路径的自动化（本环境键盘事件送不进页面），但它与点击共用
  同一个 `onsubmit`，机制一致。

---

## 24. 搜索框不再筛选书签（v1.9）

使用反馈："现在输入搜索词，会筛选下面的"。

### 24.1 背景

17 节里我把搜索框做成了**混合**行为——打字筛书签、回车搜网页，当时明确说明
这是取舍，并留了话"想纯搜网页就说一声"。客服反馈就是这个：打字时下方图标
跟着变，干扰了"搜网页"的预期。

### 24.2 改动

- **搜索框只负责搜网页**，不再驱动书签列表；
- **分类标签页的筛选保留**（那是另一条线，反馈没提到它）；
- 由此派生出的简化：
  - 列表恒取全量（`list_bookmarks("")`），`query` 不再参与数据请求；
  - 去掉 200ms 搜索防抖——没有"打字触发查询"这回事了，`SEARCH_DEBOUNCE_MS`
    与 `gloo_timers` 在该处的用法一并删除（时钟/一言仍用它）；
  - **删掉 `can_drag` 概念**：它当初存在的唯一理由是"搜索态下客户端只有匹配
    子集、重排会算错"。列表恒为全量后拖拽永远安全，`IconTile` / `BookmarkGrid`
    的该参数与"搜索中不能调整顺序"的提示语都移除了；
  - 空状态提示简化为只有"还没有书签"一种（标签都是从现有书签派生的，
    按标签筛选不可能筛出空集）。

### 24.3 服务端搜索能力的去留

**保留**。理由：它是原始需求（FR-2）明确要求的能力，逻辑与单测都完好；
前端不调用不等于能力无意义——`POST /api/list_bookmarks` 传
`{"query":"rust 编程"}` 依然返回 AND/OR 模糊匹配结果。

代价是前端存在一条不走的路径。已在此处记明，若确认以后也不要书签搜索，
可以连同 `build_search_sql` 的关键词分支、`escape_like` / `like_pattern`
及 5 个相关单测一起删掉。

> 注：`build_search_sql(0)` 仍被导出接口复用（导出要按显示顺序取全量），
> 所以该函数本身不会变成死代码。

### 24.4 已实测（v1.9）

- 初始 10 个卡片；在搜索框输入 `rust` 后**仍是 10 个**（不再筛选），
  同时表单 action 正确变为 `https://www.bing.com/search?q=rust`；
- 点「AI 工具」标签 → 3 个（ChatGPT / Claude & Friends / DeepSeek）；
  点回「全部」→ 10 个，分类筛选未受影响；
- 编辑态拖拽手柄与提示语正常（不再有"搜索中不能调整顺序"分支）。

`cargo test --features server` 20 个用例全过，两个 feature 编译零告警。

---

## 25. 迁移合并为单一基线（v2.0）

### 25.1 做了什么

把 `001`~`006` 六个迁移文件合并为**一个** `001_init.sql`。合并后的内容是
4 张表的最终形态，不再有增量痕迹：

- `bookmarks` 的 `sort_order` 归位到建表语句内（原本是 `003` 的
  `ALTER TABLE ADD COLUMN` + 回填，对全新库毫无意义）；
- `settings` 的 5 个种子键并为一条 `INSERT`（原本拆在 `004` 和 `005`）；
- **索引换成了对真实查询有用的那个**：原来是 `(created_at DESC)`，但显示顺序
  早在第 18 节就改成了 `ORDER BY sort_order ASC, id ASC`，旧索引服务不了任何
  查询，属于遗留。现改为 `(sort_order, id)`。

合并前从库里 dump 出来的 `bookmarks` 建表语句里，`sort_order` 是以
`, sort_order ...` 这样的独立行挂在末尾的——那是 `ALTER` 的痕迹，合并后消失。

### 25.2 为什么现在做，以及前提

改动**已应用**的迁移文件会让 sqlx 校验和不匹配、服务端直接起不来，所以合并
等于**重建数据库**。因此它只在"还没有任何带数据的部署"时才划算：

- 本项目尚未发布，唯一的库是本地开发库 → 现在合并，**这是唯一的机会**；
- 一旦真正部署过、有了数据，就永远只能往上叠新迁移文件。

已与使用者确认无其它在跑实例后执行。

### 25.3 数据没有丢

执行前做了双保险备份：整库文件（`.db`，344KB）+ 逻辑导出（书签 JSON、
settings、engines）。重建后按顺序恢复：

1. 书签经 `import_bookmarks` 导回，**显示顺序与备份逐条一致**（已断言比对）；
2. 设置经 `save_settings` 恢复（随机壁纸 + 遮罩 25% + 随机开关全部保留）；
3. 当前引擎恢复为原先的「必应」；
4. 图标缓存**故意不恢复**（是缓存，重建后自动重新抓取，正好验证链路）。

### 25.4 已实测（v2.0）

- 重建后 `_sqlx_migrations` 只有 `1`；4 张表齐全；
- `bookmarks` 的 DDL 中 `sort_order` 在建表内、缩进正常，无 `ALTER` 痕迹；
- 索引为 `idx_bookmarks_order ON bookmarks (sort_order, id)`；
- 10 条书签恢复且顺序与备份**完全一致**；设置三项、当前引擎均与备份相同；
- 浏览器复核：10 张卡片、10 个站点图标**全部重新抓取成功**、0 破图、
  分类计数正常、随机壁纸生效。

### 25.5 遗留

- 重建期间服务不可用（本次约 1 分钟）。生产环境若将来需要类似操作，
  必须改用"新增迁移"而不是重建。
- 备份留存在 `/tmp/mtab-backup-premerge/`（含合并前的整库文件），确认无误后可删。
