-- mtab-lite 迁移 002：分组、软删除、引擎前缀，图标缓存改为落盘
--
-- 约定（见 001 的说明）：已应用过的迁移文件不可再修改，表结构变更一律新增文件。

-- ---------------------------------------------------------------------------
-- 分组
-- ---------------------------------------------------------------------------
-- 早期版本用书签的 tags 去重派生分类标签页，代价是无法给分类排序、配图标，
-- 且一条书签带多个标签就会在多个标签页里重复出现。这里改为显式分组实体：
-- 一条书签归属至多一个分组（NULL 即"未分组"，由前端合成，不占一行）。
CREATE TABLE IF NOT EXISTS groups (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    name       TEXT NOT NULL,
    -- 可选的图标地址；留空则用名称首字
    icon       TEXT NOT NULL DEFAULT '',
    -- 标签页顺序
    sort_order INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- ---------------------------------------------------------------------------
-- 书签：归属分组 + 软删除
-- ---------------------------------------------------------------------------
-- 不声明外键：删除分组时的归位（group_id 置 NULL）在 delete_group 里显式执行，
-- 比依赖 ON DELETE SET NULL 更好读，也免得连接池的 foreign_keys 开关影响行为。
ALTER TABLE bookmarks ADD COLUMN group_id INTEGER;

-- NULL = 正常；非 NULL = 已进回收站的时间（UTC 字符串）
ALTER TABLE bookmarks ADD COLUMN deleted_at TEXT;

-- 分组内的排序；列表查询是 (deleted_at IS NULL) + (group_id) + 排序
CREATE INDEX IF NOT EXISTS idx_bookmarks_group ON bookmarks (group_id, sort_order, id);
CREATE INDEX IF NOT EXISTS idx_bookmarks_deleted ON bookmarks (deleted_at);

-- ---------------------------------------------------------------------------
-- 搜索引擎：前缀
-- ---------------------------------------------------------------------------
-- 在搜索框里输入「前缀 + 空格 + 关键词」可临时指定该引擎，如 `gh rust`。
ALTER TABLE engines ADD COLUMN prefix TEXT NOT NULL DEFAULT '';

-- ---------------------------------------------------------------------------
-- 站点图标缓存：字节改存文件
-- ---------------------------------------------------------------------------
-- 图标是二进制大对象，塞在库里会让 mtab.db 迅速膨胀（一个图标几十 KB，
-- 几百个书签就是几十 MB），备份和 WAL 都跟着变重。改为字节写在 ICON_DIR
-- （默认 data/icon）下，库表只留元数据。
--
-- 这是纯缓存，丢了会自动重取，所以直接重建表比"迁移数据"干净——顺带把
-- 旧的 BLOB 一并释放掉（VACUUM 后才会真正缩小文件）。
DROP TABLE IF EXISTS favicons;
CREATE TABLE favicons (
    host         TEXT PRIMARY KEY,
    content_type TEXT NOT NULL DEFAULT '',
    -- 图标在 ICON_DIR 下的文件名；空表示这条是"已确认取不到"的负缓存
    file_name    TEXT NOT NULL DEFAULT '',
    fetched_at   TEXT NOT NULL DEFAULT (datetime('now'))
);
