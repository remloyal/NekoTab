-- NekoTab 迁移 010：链接健康状态
--
-- 书签会烂掉：站点改名、域名过期、文章下线。收藏夹里躺着一堆点不开的链接，
-- 久了就没人愿意一页页点过去试。
--
-- 存两个字段而不是一张新表：状态是**每条书签当前的一个属性**，不是流水。
-- 检测是一次性动作，结果直接盖在书签行上（`link_status` + 检查时间），
-- 于是卡片上能直接显示"这条可能坏了"，不必每次渲染都去探测。
--
-- `link_status` 取值：
--   ''            没查过（默认；刚建的书签、新导入的书签都是这个）
--   'ok'          探测时正常响应
--   'broken'      有响应但状态码是错误（404 / 500 …）——这一类才算"坏链"
--   'unreachable' 连不上 / 超时 / DNS 失败——**不等于坏链**：内网地址、
--                需要登录的站点、被墙的域名都会落到这一类，所以界面上要与
--                'broken' 分开显示，不提供一键清理。
ALTER TABLE bookmarks ADD COLUMN link_status TEXT NOT NULL DEFAULT '';

ALTER TABLE bookmarks ADD COLUMN link_checked_at TEXT NOT NULL DEFAULT '';
