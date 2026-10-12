-- mtab-lite 初始 schema（基线）
--
-- 这是合并后的单一基线：早期版本的 001~006 已并入此处。
-- 约定：本文件一旦被部署过就**不可再修改**——sqlx 会校验已应用迁移的校验和，
-- 改动会导致启动直接失败（`migration N was previously applied but has been
-- modified`）。今后任何表结构变更都必须**新增**迁移文件。

-- ---------------------------------------------------------------------------
-- 书签
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS bookmarks (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    title       TEXT NOT NULL,
    url         TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    -- 逗号分隔的标签串，如 "rust,web,工具"
    tags        TEXT NOT NULL DEFAULT '',
    -- UTC 时间字符串，形如 "2026-10-12 08:30:00"
    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
    -- 自定义排序位；显示顺序即 `ORDER BY sort_order ASC, id ASC`
    sort_order  INTEGER NOT NULL DEFAULT 0
);

-- 排序用索引。注意不是 created_at：显示顺序早已改为按 sort_order。
CREATE INDEX IF NOT EXISTS idx_bookmarks_order
    ON bookmarks (sort_order, id);

-- ---------------------------------------------------------------------------
-- 搜索引擎
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS engines (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    name       TEXT NOT NULL,
    -- 搜索地址模板，用 %s 代表搜索词
    url        TEXT NOT NULL,
    -- 图标：可为空（回退到名称首字），或一个 http(s) 图片地址
    icon       TEXT NOT NULL DEFAULT '',
    -- 是否当前使用；同一时刻只应有一行为 1
    active     INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

INSERT OR IGNORE INTO engines (id, name, url, icon, active) VALUES
    (1, '百度', 'https://www.baidu.com/s?wd=%s', '', 1),
    (2, '谷歌', 'https://www.google.com/search?q=%s', '', 0),
    (3, '必应', 'https://www.bing.com/search?q=%s', '', 0);

-- ---------------------------------------------------------------------------
-- 主页设置（键值）
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

INSERT OR IGNORE INTO settings (key, value) VALUES
    ('background_url', ''),    -- 空则用内置渐变壁纸
    ('background_dim', '35'),  -- 背景图亮度遮罩百分比 0-80，仅在设了背景图时生效
    ('background_random', '0'),-- 每次打开换一张：给背景地址追加随机参数绕开长缓存
    ('show_clock', '1'),
    ('show_quote', '1');

-- ---------------------------------------------------------------------------
-- 站点图标缓存
-- ---------------------------------------------------------------------------
-- 由服务端代取（浏览器不直接访问第三方站点），成功后落库。
-- data 为空表示"已确认取不到"的负缓存，配合 7 天 TTL 避免反复重试。
CREATE TABLE IF NOT EXISTS favicons (
    host         TEXT PRIMARY KEY,
    content_type TEXT NOT NULL DEFAULT '',
    data         BLOB NOT NULL,
    fetched_at   TEXT NOT NULL DEFAULT (datetime('now'))
);
