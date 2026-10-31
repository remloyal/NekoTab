-- NekoTab 迁移 008：搜索历史
--
-- 搜索历史存在服务端 SQLite 里，**不写浏览器存储**：换设备、清缓存都不该把
-- "最近搜过什么"带走，同时也不该占用 localStorage 那点容量。
--
-- 只存最终实际使用的搜索词和引擎 id，不存原始输入：
--   * 输入带前缀时（`gh rust`）存的是 `rust`，前缀只负责选出引擎；
--   * 点历史回放时按 `engine_id` 找回引擎，即使当前激活的引擎已经换了。
-- 引擎可能被删掉，这里不建外键：找不到就回落到当前引擎，历史本身仍然可读。
--
-- 去重与"最多 100 条"都在写入路径里做（同词删旧插新 → 新记录 id 最大，天然排在最前）。
CREATE TABLE IF NOT EXISTS search_history (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    query      TEXT NOT NULL,
    -- 0 表示没记录到引擎（历史数据或调用方没给）
    engine_id  INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- 写入前按词删旧记录，需要按 query 查
CREATE INDEX IF NOT EXISTS idx_search_history_query ON search_history (query);
