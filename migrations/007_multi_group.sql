-- NekoTab 迁移 007：一条书签可属于多个分组
--
-- 早期约定是"一条书签归属至多一个分组"（`group_id`）。多分组上线后，完整归属
-- 存在 `group_ids`（逗号分隔的 id 串，与 `tags` 同一套存储习惯）；`group_id`
-- 保留为"主分组"（第一个），让 Netscape 导出、快捷添加这些只认识单分组的路径
-- 不必同时维护两套语义。
--
-- 默认空串 + 把已有的 `group_id` 回填进 `group_ids`，于是这次迁移对既有数据
-- 是零行为变化：单分组书签的 `group_id_list()` 读到的仍是原来那一组。
ALTER TABLE bookmarks ADD COLUMN group_ids TEXT NOT NULL DEFAULT '';

UPDATE bookmarks
SET group_ids = CAST(group_id AS TEXT)
WHERE group_id IS NOT NULL AND group_ids = '';
