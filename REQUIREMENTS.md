# NekoTab 需求文档

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
| `ICON_DIR` | 否 | `data/icon` | 站点图标缓存目录（图标字节落盘，库表只留元数据） |
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
NekoTab/
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
  `Content-Disposition: attachment; filename="nekotab-bookmarks.json"` 正确，
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

---

## 26. 首屏慢的定位与响应压缩（v2.1）

使用反馈两条：①「每个链接的图片是前端获取的，在新的地方访问会重新获取」；
②「第一次进来很慢」。

### 26.1 结论先行

- **②的原因是开发构建**：wasm 63.5 MB，正式构建只有 1.01 MB，差 63 倍；
- **①其实已经做了**：图标存在服务端的 `favicons` 表里，实测第二轮请求
  0.002s 返回且 `fetched_at` 不变（没有再去请求对方站点）；
- 顺带发现**服务端完全不做响应压缩**，已加上 gzip。

### 26.2 实测数据

| | `dx serve`（开发） | `dx build --release` |
|---|---|---|
| wasm | 63.46 MB | 1.01 MB |
| 首屏总下载 | 63.5 MB | 1.08 MB |
| 卡片出现（本机） | 1653 ms | 318 ms |

本机 localhost 带宽无限，63MB 只要 0.5s 所以不明显；**走网络时才是几十秒与
一两秒的差别**——这正是"在新的地方访问很慢"的全部原因。

### 26.3 图标缓存：已实现，且实测证明

`favicons` 表就是缓存。为排除浏览器自身缓存的干扰，用**唯一 query 绕过浏览器
缓存**（`/icon/{host}?nocache=<随机>`）连打两遍：

| | 第一遍（库缓存空） | 第二遍（库缓存已填） |
|---|---|---|
| 每个图标耗时 | 0.18 ~ 1.04 s（真去对方站点） | **0.0015 ~ 0.012 s** |
| `fetched_at` | 写入 | **未变（没有重新抓取）** |

即：新访客的额外成本是 N 次本地 SQLite 查询（毫秒级），不是 N 次外网请求。
另外图标响应带 `cache-control: public, max-age=604800`，**同一浏览器 7 天内
根本不发图标请求**。

### 26.4 为什么列表接口不返回图片（刻意如此）

使用者提出"请求链接列表的接口没有返回图片"。这是有意设计的，把图标塞进列表
响应会有三个问题：

1. **体积**：单个图标实测最大 209 KB，500 条书签可能是几十 MB 的单个响应；
   而列表在每次增/删/改/排序后都会重新拉取，等于每次都重传全部图标；
2. **base64 再膨胀 33%**；
3. **失去独立缓存**：现在浏览器能按图片单独缓存 7 天，内联进 JSON 后
   整个列表响应一起失效。

现在列表 JSON 只有 1.4 KB，图标是独立可缓存资源。

### 26.5 加上的压缩

dioxus-server 自身不做压缩（实测带 `Accept-Encoding: gzip, br` 也无
`content-encoding`）。在 `dioxus::serve` 的 router 上挂了一层
`tower_http::compression::CompressionLayer`：

```rust
let router = dioxus::server::router(App)
    .route(..).route(..)
    .layer(tower_http::compression::CompressionLayer::new());
```

**只开 gzip**：brotli 只多省约 15%，却要引入整个 `brotli` crate。
并纠正我先前的一个错误判断——我原以为 `tower-http` 已在依赖树里所以
"零新增依赖"，实际查锁文件后确认 gzip 后端（`flate2` / `miniz_oxide` /
`adler2` / `crc32fast`）都不在树里，是**新增 4 个纯 Rust 小 crate**。

### 26.6 已实测（v2.1）

- wasm：1,058,520 B → 427,980 B（**40%**），响应带
  `content-encoding: gzip` 与 `vary: accept-encoding`；
- SSR HTML：2884 B → 1233 B；
- 浏览器回归：10 张卡片、10 个图标、0 破图、时钟/一言/背景/分类计数均正常；
- 压缩前后两个 feature 编译零告警，20 个单测通过。

### 26.7 遗留

- `dx build --release` 的 `public/assets/` 里会留下**未被 manifest 引用的旧产物**
  （本次约 1.07 MB）。不影响访问，但会白占镜像体积；打包前清一下更干净。
- 未做 brotli / zstd；未做预压缩静态文件（tower-http 的 `ServeDir` 支持
  `precompressed_*`，但 dioxus-server 内部构造的 ServeDir 没有开，且那要改上游）。
  **这两条已在 §27 处理，其中"dioxus-server 不开预压缩"的判断是错的。**
- 开发构建体积大是 dioxus 的固有行为，未尝试通过 `[profile.dev]` 调参缓解。

## 27. 首屏直出列表 + 构建期 brotli（v2.2）

### 27.1 结论先行

「第一次进来要等好几秒」的根因**不是 wasm 体积，而是首屏内容依赖 wasm**。
列表（和由它派生的标签栏）用的是 `use_resource`，SSR 阶段它不执行，所以服务端只
渲出一个 `加载中…`；用户看到卡片的时间 = JS 下载 + wasm 下载 + hydrate + 再拉一次
列表。在跨境链路上这就是 6~9 秒的白等。

两处改动：

1. 列表改 `use_server_future`，卡片和标签栏直接进 SSR 的 HTML；
2. wasm 改为**构建期 brotli（q11）**，比 gzip 再省 24%。

### 27.2 列表直出：为什么用两个 hook 而不是一个

最直觉的写法是 `use_server_future(move || async move { let _ = refresh(); ... })?`，
但这样每次增删改排序都会闪回「加载中」。原因在 dioxus 的实现里：

- `use_server_future` 靠外层 `?` 表达 suspend，而它的判定是
  `resource.state() == Pending` 就返回 `RenderError::Suspended`；
- `use_resource` 在依赖变化重跑时，`cb` 只把状态置回 `Pending`，**不清空 value**
  （`value.write().take()` 只在手动 `clear()` 里，见 `use_resource.rs`）；
- 而 Suspense 边界在子组件一 suspend 时就会把已渲染内容移到后台 DOM、改显示
  fallback（`dioxus-core/src/suspense/component.rs` 的 `(None, true)` 分支）。

所以一个带 `refresh` 依赖的 `use_server_future` = 每次保存都整块闪一下。改成两个
hook 分工：

```rust
// 只在 SSR 跑一次，结果随 HTML 下发，hydrate 直接复用；刻意不带 refresh 依赖
let initial = use_server_future(move || async move { list_bookmarks(String::new()).await })?;

// 客户端的增量重查仍由 use_resource 负责；它保留旧值，重查期间不闪
let bookmarks = use_resource(move || async move {
    if refresh() == 0 {
        // 首帧直接用服务端带来的数据，避免首屏多打一次接口
        if let Some(v) = (&*initial.peek()).clone() { return v; }
    }
    list_bookmarks(String::new()).await
});

// 首屏优先用服务端直出的数据，客户端资源就绪后换成它
let snapshot = match (&*bookmarks.read()).clone() {
    Some(v) => Some(v),
    None => (&*initial.read()).clone(),
};
```

时钟与一言仍不 SSR：时钟依赖"现在"，硬 SSR 会造成 hydration 失配（渲染空串是刻意
的）；一言要让服务端出网，会把第三方延迟直接加到自己的 TTFB 上。

### 27.3 brotli：构建期压一次，而不是实时压

先量了各质量档（同一份 1,058,172 B 的 wasm，本机 `brotli` CLI）：

| 质量 | 体积 | 编码耗时 |
|---|---|---|
| gzip -9 | 423,297 B | — |
| br 4 | 408,491 B | 245 ms |
| br 6 | 371,831 B | 110 ms |
| br 9 | 368,902 B | 160 ms |
| br 11 | **325,928 B** | **2,073 ms** |

两个结论：**tower-http 的实时 brotli 默认质量是 4**（沿用 nginx 的取舍），相对 gzip
只省 3.6%，基本等于没用；而质量 11 要 2 秒 CPU/次，不能实时压。

于是改用构建期预压缩。查 dioxus-server 源码发现 `serve_dir_cached` 在 release 下
会把 `public/` 下**每个文件单独注册成一条路由**，且用的是
`ServeFile::new(&path).precompressed_br()`——也就是说**它本来就支持预压缩的 `.br`**，
只是磁盘上没有（§26.7 说"dioxus-server 内部构造的 ServeDir 没有开"，只对了一半：
目录是递归展开成逐文件路由的，逐文件路由开了）。

所以只要 `Dioxus.toml` 里开 `[web] pre_compress = true`，dx 就会为每个产物写出
`<file>.br`（`BrotliEncoderParams::default()` 的质量正好是 11）。**不新增任何依赖，
运行时零 CPU**，拿到的是 q11 的体积。

第二层是 router 上的 `tower_http::compression::CompressionLayer`，**gzip 与 brotli
都开**，负责没有 `.br` 可发的动态响应（SSR 的 HTML、server function 的 JSON）。
它不会重复压第一层的产物——响应已带 `content-encoding`，压缩层会跳过，所以"实时
brotli 质量低"在这里不构成问题（这句话在 v2.1 曾是我"不开 br"的理由，加上预压缩
之后就不成立了）。质量显式设成 6：tower-http 给实时 brotli 的默认是 4（沿用 nginx
的取舍），抬到 6 多花的 CPU 可忽略，而 gzip 的默认本来就是 6、不受影响。

实测动态响应：SSR HTML 14,959 B → gzip 3,512 B / **br 3,185 B**。

### 27.4 实测（v2.2）

线上实测（本机 → tab.remloyal.cn，同一份产物分别走 gzip / br）：

| | 改动前 | 改动后 |
|---|---|---|
| wasm raw | 1,058,172 B | 1,147,577 B |
| wasm 实际传输 | gzip 433,159 B | **br 345,854 B**（省 24%） |
| js 实际传输 | gzip 14,221 B | br 12,017 B |
| SSR HTML | gzip 1,273 B（无列表） | **br 3,185 B**（gzip 3,512 B；含 10 卡片 + 标签栏） |

最终的压缩协商矩阵（起 release 服务实测，`-.--` 表示该编码不可用）：

| 请求的资源 | `br, gzip` | 只 `gzip` | `identity` |
|---|---|---|---|
| wasm（静态） | **br 345,854 B** | gzip 455,106 B | 1,147,577 B |
| SSR HTML（动态） | **br 3,185 B** | gzip 3,512 B | 14,959 B |

wasm 那一行走的是构建期的 `.br` 文件，压缩层没有重复压它（响应已带
`content-encoding`）；HTML 那一行才是压缩层实时压的。

首屏关键路径的字节数略有上升（HTML 因为带了列表涨了约 2.3 KB），但**可见内容不再
等 wasm**：HTML 一到就有卡片。

浏览器端验证（IAB / Chromium）：

- `window.hydration_callback` 已注册、`hydrate_queue` 已清空 → 已 hydrate；
- 加载期 `performance` 里 **`list_bookmarks` 请求数 = 0**（只有 list_engines /
  get_settings / get_quote 这三个仍走 `use_resource` 的）→ 首屏确实复用了服务端数据，
  没有重复请求；
- 点「git」标签，网格从 10 张筛到 1 张（GitHub）→ 事件处理器已挂上，hydration 生效。

服务端：`cargo test --no-default-features --features server` 20 个用例通过；server 与
web(wasm32) 两个 feature 的 `cargo check` 零告警。

### 27.5 代价与遗留

- **wasm 涨了约 89 KB（+8.4%）**：引入 `use_server_future` 会把 fullstack 的数据
  传输/序列化胶水带进客户端。br 后是 0.33 MB → 0.34 MB。相对"省掉整段等待"划算。
- 时钟、一言仍不直出（原因见 27.2）；背景图在非随机模式下仍要等 settings 到位
  （settings 还是 `use_resource`）。这两项可以按同样的"双 hook"套路再收一批。
- `pre_compress` 只覆盖 dx 生成、复制进 `public/assets/` 的文件。动态响应（SSR 的
  HTML、server function 的 JSON）没有 `.br`，靠压缩层实时压（br q6）。
- **开发构建下 brotli 不生效**：dx 里那个开关是 `pre_compress & release`，而且
  debug 模式下 dioxus-server 走 `ServeDir` 而不是逐文件 `precompressed_br` 路由。
  本地想看真实效果用 `dx serve -r`。
- **dx 的客户端构建在本机偶发崩溃**（`wasm_opt.rs` 复制临时文件时报
  `Os { code: 3, NotFound }`）。同一命令重跑即可通过，11:15 / 11:19 / 11:43 三次都是
  重试成功；疑似 Windows 上临时文件被实时防护短暂占用，未深究。

## 28. 已否决：wasm-split 代码分割（v2.2 实验）

为了评估"以后功能变多、包变大"的应对手段，实测了 `dx build --release --platform web
--wasm-split`（`dioxus/wasm-split` feature + 组件标 `#[component(lazy)]`）。
**结论是负收益，不采用。** 记录在此以免重复踩。

| | 基线（单包） | 只开 wasm-split | 再给 7 个非关键组件加 lazy |
|---|---|---|---|
| main wasm | 1,058,172 B | 768,857 B | 774,027 B |
| 路由 chunk | — | 787,230 B | 717,945 B |
| 懒加载 chunk ×7 | — | — | 1,254,176 B |
| 首屏关键路径 | 438 KB gzip / **1 跳** | 533 KB gzip / 2 跳 | 511 KB gzip / 2 跳 |
| 全量合计 | 438 KB gzip | 533 KB gzip | **958 KB gzip（2.2 倍）** |

三条原因：

1. **强制保留符号名**。dx 里 `keep_names` 被 `wasm_split` 强制打开，名字段占每个
   chunk 的 18~44%（基线因为 wasm-opt 把它们删了，只有 5 B）。
2. **必须关掉 `strip`**。拆分器要用 `--emit-relocs` 的重定位段建调用图，而
   `[profile.release]` 的 `strip = true` 会删掉它，报
   `Module does not contain the reloc section`；dx 自己不管 `strip`，只警告 `debug`。
3. **共享代码/数据被复制进多个 chunk**：各包 code 相加 833 KB → 1,407 KB，
   data 204 KB → 611 KB。

另外它**不稳定**：完全相同的配置，第一次构建成功、第二次失败在同一处。

对单路由应用最致命的一点：路由级拆分是**无条件的**（`route.rs` 对每个
`RouteType::Leaf` 都展开懒加载包装，没有 per-route 开关），所以唯一的路由 `/` 会被
切成一个独立 chunk，首屏变成"先下主包、再下 Home chunk"两跳串行，只会更慢。

顺带验证了"拆小并行会不会更快"：在同一台服务器上把 1 MB 拆成 4 份并行 Range 请求，
墙钟 **13~16 s**，而单次请求是 **4~8 s**——这条链路是带宽受限而非连接数受限，
多连接反而更慢。（带宽瓶颈下墙钟时间 ∝ 总字节，与切成几份无关。）

---

## 29. 背景池（v2.3）

针对三个连续的追问：「背景图片地址能不能添加多个」「或者选择保存图片」「是不是加个
壁纸管理」。

### 29.1 结论先行

三个诉求其实是同一个模型的三半：

1. **多张** → 背景从"一个地址"升级成一个**池**（列表），轮换从池里取；
2. **从保存的图里选** → 池里外链与"上传到服务器的图"混在一起，点缩略图即用；
3. **保存图片** → 外链条目多一个「存到本地」，抓回自己服务器，之后不再依赖对方；
4. **壁纸管理** → 不新开页面：设置里单开一节「背景」，并在池下面加一条
   「本机还有 N 张没在池里」的兜底条带。

### 29.2 数据模型：只加一个键，且不需要迁移

新增 `settings.background_urls`（**一行一个地址**，既可以是 `https://…`，也可以是
本地上传的 `/wallpaper/xxxx.png`）。`background_url` 保留，语义收窄为"当前生效的
那一条"（池非空时必定是池里的一条）。

几个当时的取舍：

- **池只存地址，不存文件名/元数据。** 本机图在池里就是 `/wallpaper/xxx.png` 这个
  地址，于是轮换、缩略图、删除都只处理字符串，不必分两套数据。反过来说：不把
  "列举目录"当作池——**首页渲染时只有 `Settings` 可用**（随首屏直出），列目录要多
  一次请求且赶不上首屏；而池随设置一起到达。
- **不建表。** 池是纯配置，没有 id / 时间戳 / 需要级联删除的关联；键值表够用，
  而且零迁移。等哪天每张图要挂自己的元数据（单独遮罩、裁切焦点、标题），再挪进表。
- **老库的隐含迁移放在读取路径。** 池为空而 `background_url` 非空时，把那条当成池
  里的唯一一条（`Settings::normalized_background`）。纯函数、幂等、**不写库**，
  于是 v1.6 及以前的数据打开设置就能看见自己那张图、也能顺势加别的。
- **归一只写一处。** 去空行 / 去非法形状 / 去重 / 截到 30 条 / "当前地址收进池内"
  都在 `normalize_pool` 与 `normalized_background` 里，读（`from_pairs`）与写
  （`to_pairs`）走同一条，两条路径不会漂移。
- **上限 30 条、单条 2048 字符。** 一是面板要能一眼扫完，二是 `Settings` 会进
  hydration 载荷，不能让一个键无限膨胀。

### 29.3 「每次打开换一张」的新语义

字段名不变（`background_random`），行为按池的大小分档：

| 池的大小 | 行为 |
|---|---|
| ≥ 2 | 每次打开在池里随机取一条 |
| ≤ 1 | 与 v1.6 完全一致：给地址追加 `?_=<时间戳>` 绕长缓存 |

- **随机只追加到外链上。** 本机的 `/wallpaper/xxx` 内容不会变、响应本来就带
  `immutable` 长缓存，追加参数只会白下一次请求。这是对 v1.6 行为的一处修正。
- **哪儿都不存东西。** 选中的那条不落库（否则每开一个标签页就要写一次 `settings`，
  多标签还会互相覆盖），也不写 `localStorage`——新标签页不该往浏览器里留痕迹。
  代价：设置里高亮的那张在轮换开启时只是"默认张"，文案里写明。
- **池里只有两张时，有 1/2 的概率连着两次给同一张**（每次打开都是一次独立抽取）。
  要"必然不重样"就得记状态（`localStorage` 或服务端游标），按上面的取舍不做。
- 随机源是那个 nonce（时间戳）的 FNV 散列，不是 `Math.random`：同一次页面加载里
  组件会重渲染很多次，每次都得算出同一张，否则界面一动背景就换。

### 29.4 界面：设置里单开一节，池是唯一可编辑入口

- **不新开页面、也不做弹窗。** 设置面板本来就是"左菜单 + 右内容"，`分组管理` /
  `回收站` 就是同类先例；加一节只花 `SECTIONS` 一项。弹窗则要再叠一层导航，而池
  本来就在设置里的一次"草稿 + 保存"流程里。原「外观」一节的六块内容也顺势拆开了。
- **取消孤立的"背景图片地址"输入框。** 以前"填地址"和"上传列表"是两条互不相干的
  路，同一个概念两个真相来源；现在地址也往池里加（粘贴 + 添加），源只有一处。
- **上传成功后自动入池并设为当前**（多选一张张传，最后一张设为当前）。v1.6 的流程
  是上传完再提示"点一下设为背景"，多一步。
- **空状态与失败要能看见**：缩略图 `onerror` 后换成一行"取不到"（面板只在抽屉打开
  时挂载，图片都是客户端渲染的，不需要图标那套 hydration 后补查）。
- **这一节的改动即时落库，不用点「保存设置」。** 起因是实测反馈：传完图、点完
  「设为当前」，背景却没变——因为池的改动还躺在草稿里等一次手动保存。上传、选图、
  移出是"动作"，遮罩与轮换开关是"看着调"的旋钮，都该当场看到结果。做法是一个
  effect 盯着草稿里背景那五项与"已落库那版"的差异，不一致就存；睡 400ms 再存
  （拖滑块会连发 input 事件），存完再看一眼，直到一致为止。其余设置项仍是手动保存。
- **本机文件兜底条带**是这次新增里最不显眼但最必要的一块：本机文件 ≠ 池里条目
  （旧版本上传的、手动丢进 `WALLPAPER_DIR` 的、被移出池的都在这一档）。没有它，
  这些文件会从界面上消失却仍占着磁盘，而壁纸目录**不参与任何自动清理**。
  「移出池」（非破坏、可逆）与「删除文件」（销毁数据）因此也分成了两个动作。
- 顺带把占用显示出来：`GET /wallpaper` 从 `["name"]` 变成 `[{name, bytes}]`（多一次
  `metadata`），标题行显示"本机壁纸共 N 张 · xx MB"——管理的核心诉求就是回收空间。

### 29.5 「存到本地」：server function，不是新路由

`POST /wallpaper` 那条路由是给上传用的（请求体就是字节），而"给我一个地址、我去取
回来"的输入输出都是 JSON——正是 `#[server]` 函数的形状，于是
`save_wallpaper_from_url` 落在 `backend.rs`（`wallpaper.rs` 整模块是
`#[cfg(feature = "server")]`，客户端调不到那里的函数；函数体里再 `use crate::wallpaper::…`）。

抓取与站点图标同源（`backend::http_client()`、带 UA、确认是图片才收），两点不同：

- **体积上限按壁纸算（16 MB），而且边读边卡**：图标是"全读进内存再看大小"，
  壁纸按那个写法，一个几百 MB 的响应就能把服务打爆；
- **失败原因要能读**（超时 / 不是图片 / 支持不了的类型 / 超限 / HTTP 状态），
  它会被摆到面板上。

落盘复用上传那条路（同一个 `store`：不透明文件名 + 由 MIME 推扩展名 + 白名单校验）。
抓到的地址直接替换池里那条；若它正是当前背景，当前地址一起换掉。

### 29.6 实施时踩到的两处 Dioxus 约束

1. **一个组件里五个处理器都要读池与当前地址，而 `move` 闭包一旦捕获值类型就把那份
   吃掉了**（第二个处理器就没得用；`async move` 还会把用到的东西统统移进去）。
   解法是外层把两项包进 `Rc` 各发一份句柄，进 `async` 前再克隆一次句柄；
   "是不是当前那张"这类判断在进 `async` 之前先算成 `bool`。v1.6 的旧面板在
   `for` 循环里踩过同一个坑，当时是靠"由调用处算好传进来"绕的。
2. **hook 不能开在循环里**，所以每张卡片拆成子组件（`PoolCard` / `LooseFileCard` /
   `Thumb`）——缩略图加载失败的状态是每张各自的。

### 29.7 已实测（v2.3）

跑法：`dx serve --release`，内置浏览器逐步操作。注意 dev 模式发出去的 wasm 是没优化
过的那份（这次实测 18.5 MB，首屏要等 20~30 秒才 hydrate，中途点按钮不会有反应），
所以下面的等待都按"时钟开始跳动"来判断页面就绪，而不是固定等几百毫秒。

1. **老数据零回归**：库里原本就是 v1.6 那条 `background_url =
   https://www.dmoe.cc/random.php` + `background_random = 1`（池还不存在）。打开设置
   → 背景，池里出现它这一张、标着「使用中」，带「存本地 / 移出池」两个动作；首页
   的 `.bg` 仍是 `url("https://www.dmoe.cc/random.php?_=1792163632161")`——单条地址
   的行为与 v1.6 逐字节一致。
2. **加第二张 → 轮换**：粘贴 `https://picsum.photos/1600/900` 并「添加」（池变两张、
   新的那张设为当前），保存后连开三次页面：

   | 打开 | `.bg` 里的地址 | localStorage |
   |---|---|---|
   | 1 | `dmoe.cc/random.php?_=…512859` | 空 |
   | 2 | `picsum.photos/1600/900?_=…535266` | 空 |
   | 3 | `picsum.photos/1600/900?_=…537455` | 空 |

   第一次落在 A、后两次落在 B——**同一条连着出现是允许的**（每次打开都是一次独立
   抽取，池里两张时各有 1/2），这正是"不留任何状态"的代价。三条都拿到了随机参数，
   而 localStorage 从头到尾是空的：轮换不写任何客户端存储。

   轮换本身的纯逻辑另有单测：池里不足两条返回 `None`（退回"设置里选中的那条"）、
   同一 nonce 永远算出同一下标（重渲染不会让背景乱跳）、下标不越界、不同 nonce 不会
   全落到同一条上。
3. **存到本地**：点 dmoe 那张的「存本地」→ 服务端抓回 557 KB，落成
   `/wallpaper/31fd0c5835885f13.jpg`；卡片当场变成「本机 · 557 KB」，动作换成
   「设为当前 / 移出池 / 删除」，提示"已存到本机，池里那条已换成它"。随后把池里那
   条移出池，确认兜底条带出现「本机还有 1 张没在池里」，展开能看到体积与
   「加入池 / 删除」；「加入池」后条带消失、池恢复两张。
4. **混合池的轮换（本机地址不加随机参数）**：池 = [外链, 本机] 时连开四次：

   | 打开 | `.bg` | 加载耗时 |
   |---|---|---|
   | 1 | `picsum.photos/1600/900?_=…47833` | 662 ms |
   | 2 | `/wallpaper/31fd0c5835885f13.jpg`（**没有 `?_=`**） | 5 ms |
   | 3 | `picsum.photos/1600/900?_=…3851156` | 1238 ms |
   | 4 | `/wallpaper/31fd0c5835885f13.jpg` | 3 ms |

   本机那张走的是 `immutable` 长缓存，3~5 ms 命中——这正是"nonce 只加外链"这条
   修正要的结果（v1.6 会给它也加参数、每次都重新下载 557 KB）。
5. **取不到的地址**：往池里加 `https://example.com/`（返回 HTML 的地址），缩略图
   变成「取不到」占位；对它点「存本地」，服务端按 MIME 拦下并回
   `存到本地失败：不是图片（text/html; charset=utf-8）`——与图标那边的实测教训
   一致（有站点对任意路径都返回 200 + text/html）。
6. **删除本机文件**：把本机那张设为当前后删掉 → 磁盘文件消失（`GET /wallpaper`
   回到 `[]`）、卡片消失、提示"它正是当前背景，已换成池里的另一张"，当前背景确实
   落到剩下那条上，没有留下"指着一个不存在的文件"的状态。
7. `GET /wallpaper` 的新形状（`[{"name":"…","bytes":570736}]`）与 `/wallpaper/…`
   的回发（200、image/jpeg、570736 字节）都实测过；面板上的"本机壁纸共 1 张 ·
   557 KB"与之一致。

去掉 `localStorage` 之后，在**最终构建**上把这套面板动作又从头跑了一遍：添加
（池变两张并设为当前）→「存本地」（dmoe 抓回 781 KB 落成 `/wallpaper/bb4fa333….jpg`，
卡片换成本机、计数与体积跟上）→「移出池」（兜底条带出现「本机还有 1 张没在池里」）
→ 展开 →「加入池」（条带消失、池恢复两张）→「删除」（文件与池里那条一起消失、当前
背景落到剩下那条），结果与上面逐条一致；**全程 `localStorage` 都是空的**。

验证完把测试留下的图片删掉、设置改回原来那一条随机图接口——库里现在只有
`background_urls = ['https://www.dmoe.cc/random.php']`（正是隐含迁移该得到的形态），
`data/wallpaper/` 与改动前一样是空的。

### 29.7.1 追加实测：上传与"即时生效"（v2.3）

上一轮验证之后补了"背景改动即时落库"（见 29.4 最后一条），起因是实际使用反馈：
**上传了一张图，背景没变**——文件确实落盘了，但池的改动还在草稿里等一次手动保存。
这轮专门把"上传 → 立刻生效"跑通：

1. 由上传接口（与界面「上传图片」同一条路）传两张图，池里没它们，兜底条带显示
   「本机还有 3 张没在池里」（含用户先前上传的那张）；
2. 展开条带 → 对其中一张点「加入池」→ **什么都不点**，约 1 秒后保存条变成"已保存"，
   库里 `background_urls` / `background_url` 已经写进那张图——即时落库生效；
3. 真正走界面那条上传链路（把一张同源图片包成 `File` 塞进 `<input type="file">`
   再派发 `change`，等于替掉"点系统文件框选一个文件"这一步）：提示"已上传 1 张，
   并加入池"、池里新卡片标着"使用中"、保存条"已保存"——全程没碰「保存设置」；
4. 收尾：把测试传的图删掉（文件与池里那条一起消失，只剩用户自己那张 400 KB），
   把「设为当前」点到用户那张上，并关掉「每次打开换一张」（于是它成为固定背景）。
   重载首页确认 `.bg` 就是 `url("/wallpaper/830f1407fb1e9293.jpg")`——没有随机参数，
   遮罩 25% 照旧。

#### 29.7.2 顺带查出来的真 bug：2 MB 请求体上限（v2.3）

用着用着报「上传失败：TypeError: Failed to fetch」。复现与定位：

- 本地造一张 3 MB 的图（真 JPEG 后面补零，浏览器照样能显示）POST `/wallpaper`
  → **413 `Failed to buffer the request body: length limit exceeded`**；
- 换成 71 KB 的同一张 → 200。分界线就是 axum 对 `Bytes` 这类提取器**默认 2 MB**
  的请求体上限。

为什么浏览器只看到 "Failed to fetch"：超限时服务端在**读**请求体这一步就拒绝并掐掉
连接，而我们那句"图片过大（上限 16 MB）"在更后面（读完之后才检查 `body.len()`），
根本没机会返回。之前所有测试用的都是 1 MB 以下的图（557 KB / 781 KB / 71 KB），
所以这条一直没暴露——**"单张 16 MB"这个承诺只在"存到本地"（服务端抓取，不经过
axum 的请求体）上成立，直传从来没成立过。**

修法两处：

1. `main.rs` 在 router 末尾挂 `DefaultBodyLimit::max(wallpaper::MAX_WALLPAPER_BYTES)`。
   `Router::layer` 作用于此前注册的全部路由，所以 dioxus 的 server function
   （`/api/*`）也在内——**书签导入同样吃这个上限**，一份 3 MB 的备份导出以前会以
   完全相同的方式失败（实测：给 `/api/import_bookmarks…` POST 一个 3 MB 的 JSON
   请求体，现在能正常走到业务逻辑，返回"没解析出任何书签"而不是 413）。
2. 前端 `UPLOAD_SCRIPT` 先按 `file.size` 过滤：超限的不发请求，直接回
   「图片过大（上限 16 MB）：huge.jpg 20.0 MB」；混选时只传能传的那些，超限的张数
   在提示里说明。

实测（修完、重启整构建）：curl 传 3 MB → `200 {"url":"/wallpaper/83cfec…jpg"}`；
curl 传 20 MB → 413（超上限本身就该拒，这次是"安静地拒绝"而不是掐断连接）；
浏览器里注入 3 MB 的 `File` 走界面上传 → "已上传 1 张，并加入池"、卡片标"使用中"、
保存条"已保存"（即时落库那条也在起作用）；注入 20 MB → 前端当场回上面那句提示，
池里条数不变（没发请求）。测试传的文件事后都删干净，磁盘只剩用户那张。

顺带记两个环境坑（都跟代码无关，但会让人以为代码坏了）：

- `dx serve --release` 的构建偶尔崩在 `wasm_opt.rs`，报
  `Os { code: 3, kind: NotFound }`。查下来是 `target/dx/<app>/release/web/public/wasm/`
  这个目录被删掉了（构建中途清产物留下的），而最后一步要把优化后的 wasm 拷进去，
  目标目录不存在就 unwrap 崩。**`mkdir` 回来再重建即可**；崩掉时服务会 500，
  但源码与产物都完好。
- 崩过一次之后 `dx serve` 的**增量**构建也会反复崩在同一处，重起一次做整构建就好。

### 29.8 遗留

- 池里的顺序就是添加顺序，**不做拖拽排序**：随机轮换不关心顺序。
- **池里不能放渐变预设**：渐变仍是一档互斥设置（池非空时被忽略），混进池会把
  "外链 / 本机"两类的判断复杂化。留了扩展位（`preset:` 之类的前缀）。
- **不做定时轮换**（按小时换等）：新标签页每次打开都是新页面，页面内的定时器只在
  页面活着时有效，收益很低。
- **不做预加载下一张、也不做"当前这张挂了自动退到池里下一条"**：属于后续增强。
- 外链缩略图是每张一个请求（`loading="lazy"`）：池很大且都是大图时，打开面板会真
  下载一批图。上限 30 条之下可以接受。
