-- NekoTab 迁移 011：分组回归单值
--
-- 多分组（迁移 007 加的 `group_ids`）在三个地方都是别扭的：
--   * 界面：只有"改书签表单"能设成多个，任何一次"移到分组"都会塌回一个
--     （菜单上写「会替换」），两套语义并存；
--   * 计数：多分组书签会在每个标签页各计一次，「全部 N」与各标签页之和对不上；
--   * 导出：Netscape 格式只写得下一个分组，多分组的书签导出后会掉组。
--
-- 分组在这里是**平铺的分类标签页**，不是文件夹树，"主分组 + 全部分组"两层
-- 语义本来就不该存在。于是收回成单值：一条书签一个分组，`group_id` 就是全部。
--
-- `group_ids` 这一列**不能删**：007 已经在发布过的镜像里应用过，动已部署的
-- 迁移会撞上 sqlx 的校验和检查（`migration N was previously applied but has
-- been modified`）。它从此不再被读写，留着的唯一理由是"别去动已部署的迁移"。
--
-- 归一：主分组缺失时从 `group_ids` 的第一个补上（正常不会发生——写入路径
-- 一直把第一个同步进 `group_id`），然后把这一列清空。
UPDATE bookmarks
SET group_id = CAST(group_ids AS INTEGER)
WHERE group_id IS NULL AND group_ids <> '';

UPDATE bookmarks SET group_ids = '';
