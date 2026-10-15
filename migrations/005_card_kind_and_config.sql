-- NekoTab 迁移 005：卡片种类与组件参数
--
-- 网格原本只装书签。004 给了它跨度、把它从"等宽图标阵列"变成可拼版的版面之后，
-- 下一个自然需求就是往大格子里放组件（数字时钟、天气、记事本这类）。组件和书签
-- **共用同一张表、同一套顺序、同一套拖拽与编辑态**，区别只有两点：没有网址，
-- 以及带自己的显示参数。所以这里加两列来区分与承载，而不是另起一张表——另起一张
-- 就要重新处理排序（谁在前）、id 空间（两个网格的 data 属性会撞车）和跨类拖拽，
-- 而那些正是现有网格已经做对的事情。
--
-- 取值：
--   kind   ''       网址导航（书签）。升级前的全部数据都是这个值，行为不变。
--          'clock'  数字时钟组件。白名单见 src/models.rs 的 WIDGET_KINDS。
--   config 组件自己的参数，JSON 串（见 models::ClockConfig）。书签恒为空串。
--
-- 两个默认值都刻意是"空"，于是这次迁移对既有数据是零行为变化。
ALTER TABLE bookmarks ADD COLUMN kind TEXT NOT NULL DEFAULT '';
ALTER TABLE bookmarks ADD COLUMN config TEXT NOT NULL DEFAULT '';

-- 两个查询按 kind 收口：导出要排除组件（组件不是书签，不能写进给浏览器用的
-- 书签文件），落库前要去重"同一种组件是否已经存在"。
CREATE INDEX IF NOT EXISTS idx_bookmarks_kind ON bookmarks (kind);
