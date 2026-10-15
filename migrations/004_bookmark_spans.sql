-- NekoTab 迁移 004：卡片跨几格
--
-- 之前每张卡片固定占一格（一行一列的等宽格子），版面是"等宽图标阵列"。这里给书签
-- 加上宽 / 高跨度，卡片就能占 1×2 / 2×1 / 2×2 / 2×3 这样的多格，网格从等宽阵列变成
-- 可自由拼版的版面——也是后续往大格子里放预览图、备注之类内容的容器基础。
--
-- 取值由 clean_bookmark 夹到 1..=3；默认 1×1，所以旧数据的行为与升级前完全一致。
ALTER TABLE bookmarks ADD COLUMN span_w INTEGER NOT NULL DEFAULT 1;
ALTER TABLE bookmarks ADD COLUMN span_h INTEGER NOT NULL DEFAULT 1;
