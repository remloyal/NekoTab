-- NekoTab 迁移 009：访问统计
--
-- 每次从卡片打开书签时记一条，用来喂「最常访问」组件。
--
-- 单独一张表，而不是给 `bookmarks` 加两列（次数 / 最后访问时间）：
--   * 访问记录是**会一直长大的流水**，混进书签表意味着每次列表查询都要多带
--     两列与排序无关的数据，而书签本身（去重、导出、图标缓存）完全不需要
--     知道自己被打开过几次；
--   * "最近 N 天"这类范围统计在流水表上是顺手的一条 SQL，在计数列上
--     则做不到——次数是累计的，砍不出时间窗。
--
-- 不建外键：书签被彻底删除后这些行就是孤儿，写入路径与统计查询都会
-- 过滤掉它们（统计按 `bookmarks` 表 join，孤儿自然不在结果里）。
--
-- 表会长，但增长很慢（一次点击一行），且写入路径会定期修剪（见
-- `backend::record_visit`）：只保留最近的一批记录。
CREATE TABLE IF NOT EXISTS bookmark_visits (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    bookmark_id INTEGER NOT NULL,
    -- UTC 时间字符串，和 `bookmarks.created_at` 同一套口径
    visited_at  TEXT NOT NULL DEFAULT (datetime('now'))
);

-- 按书签聚合（"这个被打开过几次"）与按时间取窗口（"最近 7 天"）各一个索引
CREATE INDEX IF NOT EXISTS idx_bookmark_visits_bookmark ON bookmark_visits (bookmark_id);
CREATE INDEX IF NOT EXISTS idx_bookmark_visits_time ON bookmark_visits (visited_at);
