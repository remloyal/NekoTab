-- NekoTab 迁移 003：书签自定义图标
--
-- 图标原来只能由服务端按 host 代取，取不到就只剩首字色块——用户没有任何办法修正
-- 某一张卡片。加一个字段让单条书签可以指定图标，取值与分组图标同一套约定：
--   ''        自动（按 host 代取，取不到回退首字）
--   'i:code'  内置图标，见 src/components/icons.rs
--   'http…'   图片地址
--   '/…'      本站路径（例如上传的壁纸、/icon/xxx 这类）
ALTER TABLE bookmarks ADD COLUMN icon TEXT NOT NULL DEFAULT '';
