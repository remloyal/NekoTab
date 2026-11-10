# NekoTab

极简、可自部署的书签导航页，界面参照浏览器新标签页：大字时钟（带农历）+
搜索引擎框 + 分类标签页 + 图标网格。

用 [Dioxus 0.7](https://dioxuslabs.com/) fullstack 写成：同一份 Rust 代码编译出
服务端（Axum + SSR）和浏览器端（WASM + Hydration），数据存单文件 SQLite。

## 功能

- **首页**：大字时钟（可开农历、第 N 天 / 第 N 周、12 小时制）、一言、搜索框。
- **搜索**：网页 / 书签双模式；可切换与编辑搜索引擎，支持 `gh rust` 这类前缀；
  输入时带引擎联想词，空搜索框聚焦时显示最近搜索（服务端保存，最多 100 条）。
- **书签**：由分组驱动的标签页（一条书签归一个分组）、分页、跨格排版；
  站点图标自动代取，也可选内置图标、文字图标或自定义图片；新增时填完网址
  离开输入框，服务端会代取页面标题与描述预填名称（抓不到可手填）。
- **小组件**：数字时钟、表盘时钟、日历、待办、记事本、天气、热搜、最常访问，
  与书签共用网格，可拖拽、可调尺寸；可归到一个分组，不选就是全局（每个分组页都显示）。
- **整理**：长按进入编辑态拖拽排序、批量移动 / 删除、右键菜单、回收站还原；
  支持跨页拖拽；删除后底部提示条里可以一键撤销。
- **维护**：链接检查（服务端并发探测，点不开的会标在卡片上，可批量清理）、
  访问统计（记在服务端，换设备也带着）。
- **外观**：深色 / 浅色 / 跟随系统、6 套内置渐变、背景池（图片直链 / 上传 /
  抓回本地）、卡片宽度 / 间距 / 列数 / 跨格 / 底色。
- **数据**：导入浏览器书签 HTML 或本应用 JSON；导出 JSON / Netscape HTML /
  SQLite 快照；书签小工具与 `POST /api/bookmark` 快捷添加。
  一次导入的内容上限 8 MB（浏览器自己导出的书签文件通常在几百 KB 以内）。
- **其他**：可选整站访问密码；站点标题 / 描述 / 图标 / 主题色可配；
  `Ctrl/Cmd + K` 回到搜索框，`Esc` 逐层关闭。

详细说明见 [docs/FEATURES.md](docs/FEATURES.md)。

## 技术栈

| 层 | 选型 |
|----|------|
| 全栈框架 | Dioxus 0.7.10（`fullstack`；发布构建开启 wasm 分包） |
| 服务端 | Axum 0.8（由 `dioxus::serve` 托管） |
| 数据库 | SQLite + sqlx 0.9（异步连接池，WAL） |
| 序列化 | serde / serde_json |
| 配置 | dotenvy |
| 一言（仅 server） | reqwest（复用 dioxus 已有的 rustls，无系统 OpenSSL 依赖） |
| 响应压缩（仅 server） | tower-http（gzip + brotli） |
| 访问密码（仅 server） | ring（HMAC-SHA256，复用 rustls 已编进来的那份） |
| 时钟 / 农历 / 搜索跳转 / 指针拖拽 / 文件读取（仅 web） | js-sys、gloo-timers、web-sys、wasm-bindgen |

**没有第三方日期库。** 农历是「一张 1900–2100 年的编码表 + 几十行算术」，
自己写在 `src/lunar.rs` 里，配套单测拿历年春节（正月初一）的公历日期校验——
表里任何一格抄错都会让某一年的正月初一对不上，所以这组测试是有效护栏。

## 目录结构

```
.
├── Cargo.toml
├── Dioxus.toml
├── Dockerfile                    # 多阶段构建
├── .github/workflows/release.yml # 打 v* 标签：多架构镜像 + GitHub Release
├── .env.example
├── docs/
│   ├── FEATURES.md               # 功能详细说明（交互与边界）
│   └── DESIGN.md                 # 实现取舍与关键细节（面向维护者）
├── migrations/
│   ├── 001_init.sql              # 基线：bookmarks / engines / settings / favicons
│   ├── 002_groups_trash_and_icon_files.sql   # 分组表、软删除、引擎前缀、图标改落盘
│   ├── 003_bookmark_icon.sql     # 书签自定义图标（空 / i:内置 / t:文字 / http… / 本地路径）
│   ├── 004_bookmark_spans.sql    # 卡片跨几格（span_w / span_h）
│   ├── 005_card_kind_and_config.sql  # 卡片种类（kind）与组件参数（config）
│   ├── 006_card_surface.sql      # 卡片底色（surface）
│   ├── 007_multi_group.sql       # 曾支持多分组（group_ids），已由 011 收回
│   ├── 008_search_history.sql    # 搜索历史（服务端存储，最多 100 条）
│   ├── 009_visit_stats.sql       # 访问统计（打开书签的流水）
│   ├── 010_link_status.sql       # 链接健康状态（坏链标记）
│   ├── 011_single_group.sql      # 分组回归单值（清空 group_ids）
│   └── 012_fix_dangling_group.sql # 修掉 011 留下的坏分组归属（group_id 悬空）
├── assets/
│   ├── main.css                  # 经 asset! 内嵌进二进制
│   └── favicon.svg               # 站点图标
└── src/
    ├── main.rs                   # 入口：server 走 dioxus::serve，client 走 launch
    ├── backend.rs                # #[server] 函数 + 搜索/前后缀等纯逻辑单测
    ├── db.rs                     # 连接池（仅 server 编译）
    ├── dom.rs                    # 浏览器侧小工具（仅 web）：聚焦、指针捕获、落点命中、读文件
    ├── engine.rs                 # 搜索词 URL 编码与模板替换（含单测）
    ├── favicon.rs                # 站点图标：host 解析 + 服务端代取 + 落盘缓存（含单测）
    ├── lunar.rs                  # 农历换算 / 第几天 / ISO 周（含单测）
    ├── timefmt.rs                # 时间的显示格式化：大时钟与「数字时钟」组件共用（含单测）
    ├── models.rs                 # 前后端共享模型 + 设置项的键值编解码（含单测）
    ├── quickadd.rs               # 快捷添加：令牌存取 + /api/bookmark 两个处理器（仅 server，含单测）
    ├── transfer.rs               # 书签导入解析 / JSON 与 Netscape 导出（仅 server，含单测）
    ├── wallpaper.rs              # 壁纸：上传落盘 / 列表（带体积）/ 删除 / 回发 / 抓外链存本地（仅 server，含单测）
    ├── pages/home.rs             # 首页（唯一页面）
    ├── auth.rs                   # 可选访问密码：登录页 / 签名 Cookie / 守卫中间件（仅 server，含单测）
    ├── hot.rs                    # 热搜数据源：五个源地址 + 响应解析 + 十分钟缓存（仅 server，含单测）
    ├── suggest.rs                # 搜索联想：认引擎端点 + 解析各家响应（仅 server，含单测）
    ├── weather.rs                # 天气数据源：地址拼装 + 响应解析（仅 server，含单测）
    ├── page_meta.rs              # 抓网页标题 / 描述给新增书签预填名称：head 解析（仅 server，含单测）
    ├── linkcheck.rs              # 死链检测：并发探测地址 + 状态码分类（仅 server，含单测）
    ├── netguard.rs               # 出站判定：目标是不是内网 + 重定向逐跳放行（仅 server，含单测）
    ├── components/
    │   ├── card/                 # 卡片：框 / 网格 / 正文 / 表单 / 选卡弹窗（见 card/mod.rs 的说明）
    │   │   ├── tile.rs           # 单张卡片的框（书签与组件共用：编辑态、选择态、长按、指针事件）
    │   │   ├── grid.rs           # 卡片网格 + 拖拽判定 + 落点计算
    │   │   ├── add_dialog.rs     # 「添加卡片」弹窗：标签页（网址导航 / 小组件）+ 选卡面板
    │   │   ├── bookmark_form.rs  # 书签类卡片的表单：新增 / 修改共用
    │   │   ├── widget_form.rs    # 组件的参数表单（参数按 kind 分支）
    │   │   └── widgets/
    │   │       ├── clock.rs      # 「数字时钟」正文（仅 web 刷新，格式化见 src/timefmt.rs）
    │   │       ├── dial.rs       # 「表盘时钟」正文（SVG 指针表盘）
    │   │       ├── calendar.rs   # 「日历」正文（月历 + 农历；含日期算术单测）
    │   │       ├── todo.rs       # 「待办」正文（卡片上直接勾选，写回 config）
    │   │       ├── hot_search.rs # 「热搜」正文（服务端代取 + 十分钟刷新）
    │   │       ├── note.rs       # 「记事本」正文（只读展示，内容在弹窗里改）
    │   │       ├── visits.rs     # 「最常访问」正文（服务端聚合 + 一分钟刷新）
    │   │       └── weather.rs    # 「天气」正文（客户端取数 + 十分钟刷新 + 失败说明）
    │   ├── clock.rs              # 首屏大时钟（仅 web 刷新，农历在这里算）
    │   ├── quote.rs              # 一言（仅 web 定时拉取）
    │   ├── icons.rs              # 内置图标集 + 首字 / 色相回退 + 统一渲染入口
    │   ├── search_bar.rs         # 搜索框：双模式 + 引擎选择 + 前缀解析 + 历史面板
    │   ├── engine_logo.rs        # 引擎图标（图片或首字回退）
    │   ├── engine_settings.rs    # 搜索引擎设置（含前缀）
    │   ├── group_tabs.rs         # 分类标签页（全部 / 未分组 / 各分组，含条数）
    │   ├── icon_picker.rs        # 图标选择器（书签与分组共用：自动 / 内置 / 自定义地址）
    │   ├── pager.rs              # 分页条
    │   ├── quick_add_panel.rs    # 快捷添加（书签小工具 / 令牌，含单测）
    │   ├── batch.rs              # 批量移动到分组的弹窗内容
    │   ├── group_settings.rs     # 分组管理（行内改名、排序、删除）
    │   ├── link_check_panel.rs   # 链接检查（并发探测结果 / 批量移入回收站）
    │   ├── trash_panel.rs        # 回收站（还原 / 彻底删除 / 清空）
    │   ├── toast.rs              # 底部提示条（删除后可撤销）
    │   ├── context_menu.rs       # 自定义右键菜单（含二级菜单与贴边回拉）
    │   ├── drawer.rs             # 右侧抽屉外壳
    │   ├── settings_panel.rs     # 主页设置（外观 / 背景 / 布局 / 时间 / 行为 / 数据…）
    │   ├── background_panel.rs   # 背景池：上传 / 粘贴地址 / 存到本地 / 移出 / 删文件（HTTP 走 document::eval）
    │   ├── modal.rs              # 弹窗外壳
    │   └── confirm_dialog.rs     # 二次确认弹窗
    └── pages/
        └── home.rs               # 状态编排：筛选 / 分页 / 批量 / 快捷键 / 主题
```

## 设计文档

实现细节与关键取舍（feature 隔离、状态刷新、拖拽排序、导入导出、响应压缩等）
见 [docs/DESIGN.md](docs/DESIGN.md)。

## 环境变量

| 变量 | 必填 | 默认 | 说明 |
|------|------|------|------|
| `DATABASE_URL` | 否 | `sqlite://data/mtab.db?mode=rwc` | SQLite 连接串 |
| `ICON_DIR` | 否 | `data/icon` | 站点图标缓存目录（字节落盘，库表只留元数据） |
| `WALLPAPER_DIR` | 否 | `data/wallpaper` | 上传的壁纸目录（用户数据，不参与缓存清理） |
| `IP` | 否 | `127.0.0.1`（Docker 中 `0.0.0.0`） | 监听地址。注意是 `IP` 不是 `HOST`——由 `dioxus-cli-config` 读取 |
| `PORT` | 否 | `8080` | 监听端口 |
| `ACCESS_PASSWORD` | 否 | 空 | 整站访问密码。留空＝不设防；设置后未登录访问跳 `/login`，登录状态保持 **30 天** |
| `SESSION_SECRET` | 否 | 用密码派生 | 会话签名密钥。想轮换密码又不让所有设备退出时固定它；改了它＝全部会话失效 |

复制 `.env.example` 为 `.env` 后按需修改。

**访问密码。** 设置 `ACCESS_PASSWORD` 后：
- 页面、server function（`/api/*`）、导出、图标与壁纸都受保护；登录页、静态产物与
  带令牌的 `/api/bookmark` 放行（书签小工具从别的站点发起请求，没有本站 Cookie）。
- Cookie 是 `HttpOnly; SameSite=Lax`，HTTPS 下自动加 `Secure`（需要反代传
  `X-Forwarded-Proto: https`）；服务端不存会话表，签名里带过期时间，重启后仍有效。
  没设 `SESSION_SECRET` 时，服务端会生成一个随机密钥存进 `settings.session_secret`，
  再用它和密码派生签名密钥——只拿到 Cookie 的人无法离线快速枚举密码。
  **这个随机密钥拿不到时服务端会拒绝启动**（而不是悄悄退化成"用密码当密钥"），
  日志里会提示去设 `SESSION_SECRET`。
- 连续输错 8 次后，**错误的尝试**会被挡 60 秒（正确密码仍可进，避免匿名者把站长锁在门外）；
  公网部署仍建议在反向代理层再加 HTTPS 与限流。
- 「退出登录」会**作废所有已签发的登录状态**（不只是清掉当前浏览器这枚 Cookie），
  其它设备上的会话也会一起失效——版本号进了签名，所以复制走的 Cookie 也用不了。
- 纯 HTTP 下密码是明文传输的——内网也要清楚这一点，公网请务必上 HTTPS。

**为什么还有几处叫 `mtab`。** 应用名统一成了 NekoTab，但**数据所在位置**的名字有意
没跟着改：数据库文件仍是 `mtab.db`，`docker run` 示例里的卷名仍是 `mtab-data`。
改个名字容易，改数据位置会让人看着像"书签全没了"——沿用旧名字，现有部署升级后
照样读到原来的库。要把这些一并改成 `nekotab.*`，那是一次数据迁移（移动文件 +
改环境变量），不是重命名，得单独做。

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

```bash
cargo test --no-default-features --features server      # 单测
cargo check --no-default-features --features server
cargo check --no-default-features --features web --target wasm32-unknown-unknown
```

单测共 189 个。覆盖范围与关键断言见[设计文档](docs/DESIGN.md)的「测试覆盖」。

## 生产构建

```bash
# 三个参数缺一不可：
#   --features wasm-split 让 #[component(lazy)] 走懒加载（开发构建不开，dx serve 不受影响）
#   --wasm-split          让 dx 把懒加载组件拆成独立 .wasm chunk
#   --debug-symbols=false 拆完后再去掉调试信息（主包 br 528KB -> 469KB）
# 用 @client 限定只作用于客户端，服务端不必带 wasm-split。
dx build --release --platform web \
    @client --features wasm-split --wasm-split --debug-symbols=false
```

产物在 `target/dx/nekotab/release/web/`：客户端资源在 `public/`，
服务端二进制名为 `server`（Windows 下 `server.exe`）。

**务必用 release 构建对外提供服务。** 开发构建的 wasm 有 63 MB（不优化且带调试
信息）；正式构建首屏主包约 1.6 MB，brotli 后约 460 KB。设置面板、添加卡片弹窗、
搜索引擎设置都走懒加载，各自是 `public/assets/module_*.wasm`，点到时才下载：

- 设置面板外壳约 33 KB（br）；背景 / 分组管理 / 链接检查 / 回收站 / 快捷添加
  各是一个子 chunk，约 8–26 KB，切到对应标签页才下
- 添加卡片弹窗约 6 KB；搜索引擎设置约 11 KB

> Windows 上**先把正在运行的服务停掉再构建**：服务端二进制与 `public/` 里的产物
> 会被占用，`dx build` 写不进去，报的错是「另一个程序正在使用此文件」，
> 或 wasm-opt 阶段的「系统找不到指定的路径」——看着像路径问题，其实是文件锁。

## Docker

### 用已发布的多架构镜像（推荐）

`docker-compose.yml` 默认直接拉 `remloyal001/nekotab:latest`（amd64 + arm64 合在一个
manifest 里）：

```bash
docker compose up -d                        # 拉取并启动
docker compose pull && docker compose up -d  # 升级到新镜像
docker compose logs -f
```

数据用 bind mount 放在宿主目录，默认是仓库下的 `./data`（与本地开发的约定一致），
可用 `NEKOTAB_DATA` 指向别处、`NEKOTAB_PORT` 换宿主端口。
**要放在本地磁盘，别放 NFS / SMB 这类网络文件系统**——SQLite 的 WAL 依赖正常文件锁。

### 自己构建

```bash
docker build -t nekotab .
# 需要整站访问密码时，在下面的命令里加：
#   -e ACCESS_PASSWORD='换成你的密码' -e SESSION_SECRET='一串随机字符' \
docker run -d --name nekotab \
  -p 8080:8080 \
  -v mtab-data:/data \
  nekotab
```

镜像是多阶段的：构建阶段装工具链并跑一次带分包的
`dx build --release --platform web @client --features wasm-split --wasm-split --debug-symbols=false`，
运行阶段只留 `debian:bookworm-slim` + 产物。首次构建要下 Rust 镜像、`cargo install
dioxus-cli` 并全量编译，是十几分钟量级；之后由 Docker 层缓存接管。

**构建不需要访问 GitHub。** dx 收尾会想从 GitHub Releases 下 wasm-opt（binaryen），
在连不上 GitHub 的网络里整个构建会倒在最后一步、且报错像"编译失败"。所以
`Dockerfile` 里把发行版的 `binaryen` 直接放进 dx 的工具缓存目录，让它不去下载
（细节写在 Dockerfile 的注释里，含"dx 升级后要跟着改哪个数字"）。

要「本地编译一次、部署时秒构建」，用 `Dockerfile.prebuilt`：先在 **Linux 环境**
（WSL / CI / rust 容器）里跑 `bash scripts/pack-linux.sh` 把产物打到 `dist/<arch>/`，
再以那个目录为上下文构建运行镜像。产物**必须是 Linux 的**——Windows 上 dx 产出的是
`server-*.exe`，容器里跑不起来。

`scripts/publish.sh` 把上面两步连起来：本机按架构打包 → 各架构分别 build & push →
合成多架构 manifest。Git Bash / MSYS 下它会自动把自己丢进 WSL 重跑一遍，所以
**Windows 上直接 `bash scripts/publish.sh` 即可**。支持 `DRY_RUN=1`（只打印命令）、
`SKIP_BUILD=1`、`ARCHES=...`。

### 数据、升级与出网

数据落在 `/data/mtab.db`（连同 WAL 的 `-wal` / `-shm` 文件），图标落在 `/data/icon`，
上传的壁纸落在 `/data/wallpaper`：容器重建都不丢。镜像里 `ICON_DIR` 与
`WALLPAPER_DIR` 都已指向挂载卷。

**升级只需换镜像**：数据库迁移随二进制编译进去（`sqlx::migrate!`），启动时自动应用，
没有单独的迁移步骤。

**回退镜像前先看[设计文档](docs/DESIGN.md)的「迁移与回退」。** 拿旧镜像跑已经升级过的
库会直接启动失败（sqlx 不允许"库里已应用、当前二进制里没有"的迁移）；007 这类
`ALTER TABLE ... ADD COLUMN` 的迁移还有额外的列回退要求。

**「一言」与「天气」需要服务端能出网**（前者取一言接口，后者取 Open-Meteo，
都不需要密钥）。纯内网部署时这两处会保持空白 / 显示取不到，其余功能不受影响。

注意：服务端运行时会读取可执行文件同级的 `public/` 目录，因此镜像里要连同
`public/` 一起拷贝，不能只拷二进制。

## 发布

仓库只有一条工作流 `.github/workflows/release.yml`：**只在推 `v*` 标签时运行**，
日常 push / PR 不跑任何 CI。工作流内部会先跑一遍 `cargo test`，再构建与发布。
工具链固定 1.94，与 `Dockerfile` 的 `rust:1.94` 对齐；依赖里最高的 MSRV 是 1.89。

**版本号必须与 `Cargo.toml` 的 `version` 一致**，否则工作流会在校验版本那一步直接失败：

```bash
# 先把 Cargo.toml 的 version 改成 0.2.0 并提交
git tag -a v0.2.0 -m "NekoTab 0.2.0"
git push origin v0.2.0
```

发布流程做的事：

1. 用 `scripts/pack-linux.sh` 构建 **amd64 + arm64** 的 Linux 产物：客户端 wasm 只编
   一次，服务端在 amd64 上原生编、在 arm64 上交叉编（`gcc-aarch64-linux-gnu` +
   `libc6-dev-arm64-cross`），不走 QEMU 模拟编译。
2. 用 `Dockerfile.prebuilt` 打成多架构镜像并推送：
   - `ghcr.io/remloyal/nekotab`（用 `GITHUB_TOKEN`，无需额外配置）
   - `remloyal001/nekotab`（**可选**，见下方 secrets）
3. 推镜像前先用 amd64 镜像起一次容器、等首页返回 200，避免把坏镜像推成 `latest`。
4. 创建 GitHub Release，附两个架构的 `nekotab-<版本>-linux-{amd64,arm64}.tar.gz`
   和 `SHA256SUMS`。解压后 `server` 与 `public/` 同级，设置环境变量即可直接运行。

镜像标签规则：`<版本>`（如 `0.2.0`）；非预发布同时更新 `latest`。
标签里带 `-` 的（如 `v0.2.0-rc1`）按预发布处理：不碰 `latest`，Release 标为
pre-release。

**可选的 Docker Hub 推送。** 不配置就只发 GHCR。要用 Docker Hub，在仓库
Settings → Secrets and variables → Actions 里加：

| Secret | 内容 |
| --- | --- |
| `DOCKERHUB_USERNAME` | Docker Hub 用户名（填 `remloyal001` 才能与 `docker-compose.yml` 的默认镜像一致） |
| `DOCKERHUB_TOKEN` | Docker Hub 的 Access Token（不是登录密码） |

**GHCR 的包默认是私有的。** 想让别人直接 `docker pull`，到 GitHub 个人/组织设置的
Packages 里把 `nekotab` 的可见性改成 public。

## 已知限制

- **鉴权是可选的。** 不设置 `ACCESS_PASSWORD` 时整站不设防（单用户自部署的默认），
  公网暴露请设置它，或在反向代理上加鉴权。`/api/bookmark` 另有自己的令牌把关
  （见[功能说明](docs/FEATURES.md)）；反代若只做 Basic Auth，记得那条路径同样要覆盖到。
- **一条书签只属于一个分组。** 早先支持多分组（一条书签同时出现在多个标签页下），
  但界面里只有"改书签表单"能设成多个，任何一次「移到分组」都会塌回一个，导出还再丢一次——
  三套语义互相打岔，已经收回成单值（迁移 011）。数据库里的 `tags` 列和已有的值都还在，
  它仍参与搜索，只是不再是分类。
  归组后的条数在各个标签页上一致：各分组之和正好等于「全部」。
- **没有做「文件夹卡片」**（把一组链接折叠成一张卡片）。分组已经承担了归类职责，
  再叠一层文件夹会让「分类标签页 + 卡片网格」两套导航互相打岔。
- **搜索联想只认已知引擎。** 联想接口是各家的私有端点、格式互不相同，所以只有一张
  写死的表：百度 / 必应 / 谷歌 / 360 / 搜狗。自己加的引擎（比如自建 SearXNG）没有
  联想词，面板就不出现。服务端拿不到响应时同样静默为空，搜索本身照常用。
  另外，联想是把**你正在输入的词**发给对应搜索引擎换回来的——介意这一点就不要用
  它（输入完直接回车照样搜）。
- **滚轮翻页默认关闭**：开启后滚轮会被拦下来翻页、不再滚动页面。
- **跨格卡片会留洞，且 Netscape 导出丢跨度。** 网格用默认的稀疏排布（不做 dense
  回填），因为回填会让视觉顺序与数据顺序分家，拖拽落位和分页立刻变得不可预期；
  代价是同一行放不下的大卡片会被挤到下一行，中间留洞。另外跨度只有 JSON
  备份能原样搬回，`/export/bookmarks.html`（Netscape 格式）没有这个概念，
  导回浏览器再导回来一律变成 1×1。
- **书签卡做成纵向长条时上下会留白。** 图标块按格子的**短边**缩放（这样跨格时不会
  中间一个小图标、四周一圈空），所以 1×3 / 2×3 这种"窄而高"的书签卡，图标仍受宽度
  约束，纵向空间就空着。组件卡会把它用满——记事本、天气、两个时钟都按格子的宽高
  自适应排版。
- **拖拽排序作用在全量顺序上。** 筛选或分页时，被拖的卡片落到**目标在全量里的
  位置**，中间那些书签（包括被筛掉、当前看不见的）依次让位。所以看到的落位与
  最终顺序一致；代价是一次跨页拖动会连带挪动中间的书签——这是「顺序唯一」的必然
  结果，不做「只重排可见项」那种会让人下次打开时莫名其妙的分裂语义。
- **触屏拖拽要先进入编辑态**（长按卡片）：编辑态下卡片带 `touch-action: none`，
  从卡片上起手不会滚动页面——这是让拖拽可用的代价。
- **一言需要服务端能出网。** 拉取失败时该行保持空白（不影响其他功能）。
  换一条的间隔改 `src/components/quote.rs` 的 `REFRESH_SECS`。
- **背景图有两种来源**：填任意图片直链（由浏览器直接加载，需要对方允许跨站引用——
  CSS 背景图不受 CORS 限制，但可能被防盗链拦），或者**上传到服务器 / 用「存到本地」
  抓回服务器**（见上，都不受防盗链影响）。轮换或随机图模式下每次打开都会真下载
  一张图（实测约 1MB / 2 秒），且要在 hydration 之后才开始，首屏会有一小段没有
  壁纸——这也是内置壁纸预设存在的理由。
- **「存到本地」是一次显式动作，不做自动。** 外链图只在用户点「存本地」时才抓回
  自己服务器（否则会静默把磁盘堆满）；抓回来之后池里那条就是本机地址，与其他
  本机图一样可删、可移出池。
- **本机壁纸不参与任何自动清理。** 池里移出只是不再参与轮换，文件仍在
  `WALLPAPER_DIR`；要真的释放磁盘得点「删除」（不可恢复）。所以"本机文件"条带
  会一直列出不在池里的那些，避免它们变成看不见的占用。
- **农历只覆盖 1900–2100 年**，超出范围不显示。
- **图标是缓存，不是数据，取不到会回退首字色块。** `/icon/{host}` 返回 404
  表示"这个站点没有可取到的图标"，不是故障：卡片会露出名称首字 + 哈希取色的色块。
  实际上确实有不少站点取不到——`/favicon.ico` 返回 HTML 且 head 里也不声明图标
  （实测 `pre.friggatech.com` 这类就是），或者站点从服务器所在网络不可达。
  内网/本机地址（`172.16.x.x`、`localhost:8000`）在容器里通常也取不到，
  除非容器确实能访问那些服务。取不到的原因会打进服务端日志，也可以在设置里
  「清空图标缓存」强制全部重新代取。低分辨率图标（16×16）在大尺寸卡片上会偏软，
  这是源文件本身的限制。
- **服务端不会替第三方站点去访问内网。** 被收藏的页面可以声明自己的图标地址、
  也可以用 302 把请求引到别处——这些地址是**对方站点给的**。所以除"与书签
  同一个地址"外，代取一律要求目标解析到公网 IP；每一跳重定向也重新判一次。
  唯一例外是用户自己收藏的内网地址（那本来就是用户要访问的）。
  反过来说：**别指望让服务端代取一个内网站点的图标**——那正是被挡掉的那类。
- **自动抓标题同样需要服务端能访问目标站点。** 它和站点图标一样由服务端出网，
  不带你浏览器里的 Cookie，所以需要登录、在内网或对服务器不可达的页面抓不到标题
  （名称保持空白，手填即可，不影响保存）。标题按 UTF-8 解析，少数仍在用 GBK 的
  老站点可能抓到乱码，改名一次即可。
- **链接检查看到的是"服务器看到的那个链接"，不是"你浏览器看到的"。** 内网地址、
  要登录的站点、被墙的域名会算成"连不上"而不是"坏链"，所以只有真正返回错误
  状态码的才会被标红、才提供一键清理——宁可漏掉几个坏链，也不误删。
  检测结果是**当时**的快照，站点修好后要重新检测才会变。
- **访问统计只记服务端收到的那一次。** 同标签页打开书签时页面正在跳转，
  那个记访问的请求可能被浏览器掐掉，这一次就不计入；开新标签页（默认）则不受影响。
- **组件不进书签导出。** `/export/bookmarks.json` 与 `/export/bookmarks.html` 都只
  导书签：组件的网址是空的，写进给浏览器导入的书签文件只会留一条打不开的链接。
  要连带组件一起备份，用 `/export/db.sqlite`（整库快照）。
- **组件不参与关键词筛选、分组计数与分页**，且固定显示在书签之前。分组只决定它出现
  在哪些分组页：不选是全局，选了只在那组和「全部」下出现。它不是"多、要筛、要翻页"
  的东西；被关键词筛掉或被翻到第二页都只会让版面突然缺一块。拖拽也只在同类之间
  生效——跨类落位没有可解释的结果，会直接忽略。
- **记事本的正文是"弹窗里改"，不是"卡片上直接敲"。** 在卡片上放可输入的文本域要和三条
  既有行为抢指针——长按进编辑态、从卡片任意处起手拖拽、编辑态下的 `touch-action: none`
  ——再叠一层防抖自动保存，以及"父级刷新把没保存的本地文本冲掉"的保护。那是独立的一件事，
  没混进这一版。卡面因此是只读的，点一下打开编辑器（一次点击，不需要右键找菜单）。
- **天气需要服务端能出网**，数据源是 [Open-Meteo](https://open-meteo.com)（选它的唯一理由
  是**不需要 API key**：要注册账号、要配密钥的数据源会把"部署完就能看天气"变成"先去申请
  一个密钥"）。地图名先在服务端换成经纬度再取预报，两次请求都走同一个 8 秒超时的客户端。
  取不到时**卡片上直接写明原因**（"找不到「xx」这个地方"／连接失败的原话），而不是留一片
  空白——这与「一言」的取舍相反，一言取不到就保持空白无所谓，天气卡片空着只会被当成坏了。
  已经取到过数据时，后续刷新失败会保留旧值。
  客户端每 **10 分钟**刷新一次，服务端**不做缓存**：一个开着的页面一小时四次请求，
  自己用足够了；要多开几个标签页的话，考虑给它加一层缓存。换数据源只需改
  `src/weather.rs` 里的两个地址与两个解析函数。
- **热搜依赖服务端出网。** 五个源都是各家公开接口，可能改格式、限流或需要特定 UA；
  取不到时卡片上会写明原因，换一个源即可。服务端按源缓存十分钟，客户端每十分钟刷新。
- `created_at` 存 UTC 时间字符串，界面未展示；回收站里的删除时间按 UTC 显示。

## 许可证

[MIT](LICENSE) © 2026 remloyal
