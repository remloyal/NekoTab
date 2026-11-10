-- NekoTab 迁移 012：修掉 011 归一时留下的坏分组归属
--
-- 011 用 `CAST(group_ids AS INTEGER)` 把多分组里的第一个 id 取出来当 `group_id`。
-- 但 `group_ids` 存的是逗号分隔的串：`",2"`（第一段是空的）或 `"abc"`（脏数据）
-- 都会被 CAST 成 0，于是造出一个指向**不存在**的分组（id 从 1 开始）的归属。
-- 那条书签于是"在任何分类下都看不见"——标签栏里没有 0 这个组，而它又不是未分组。
--
-- `group_ids` 早在 011 里就被清空了，所以这里救不回多分组那部分信息（那部分
-- 本来也已经随"多分组收回单值"一起作废了）。能做的、也该做的是把这类坏归属
-- 归位成"未分组"。
--
-- 顺带把"指向已不存在的分组"的其它情况也一并归位：分组被并发删掉时可能留下
-- 同样的悬空 id。读列表那侧本来就会把它们当作未分组处理，这里是把库本身也
-- 修干净。

-- id = 0 一定是坏值（自增 id 从 1 开始）
UPDATE bookmarks SET group_id = NULL WHERE group_id = 0;

-- 其余指向不存在分组的悬空 id
UPDATE bookmarks
SET group_id = NULL
WHERE group_id IS NOT NULL
  AND group_id NOT IN (SELECT id FROM groups);
