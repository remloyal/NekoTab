-- NekoTab 迁移 013：书签缓存自己的 host
--
-- `/icon/{host}` 每次请求都要确认"这个 host 确实是本站收藏过的站点"，否则那个
-- 无鉴权接口就成了任意出站探测。原先的做法是把 `SELECT url FROM bookmarks`
-- 全表捞出来、在 Rust 里逐个 `host_of` 比对——连"图标已经缓存好了"的请求也要
-- 扫一遍。书签几千条时每个图标请求多花几毫秒，页面一开就是几十个图标。
--
-- 这里把 host 落成一列并建索引，判定就退化成一次走索引的等值查询。
-- 列由写入路径（新增 / 修改 / 导入 / 快捷添加）填好；老数据由启动时的一次
-- 回填写上（`favicon::backfill_hosts`）——SQL 里拆 URL 拼不出与 `host_of`
-- 完全一致的口径（userinfo、端口、大小写、非法 host），交给 Rust 做才不会漂。
ALTER TABLE bookmarks ADD COLUMN host TEXT NOT NULL DEFAULT '';

CREATE INDEX IF NOT EXISTS idx_bookmarks_host ON bookmarks (host);
