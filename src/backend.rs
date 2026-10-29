//! 服务端接口。`#[server]` 宏会把每个函数拆成：
//! - 客户端 stub（发 HTTP 请求）
//! - 服务端实现（直接执行函数体）
//!
//! 因此本模块在 web / server 两侧都会编译，函数体里对 `crate::db`、`sqlx`
//! 的引用只存在于服务端实现中，不会污染 wasm 产物。

use dioxus::prelude::*;

use crate::models::{
    AddOutcome, Bookmark, Engine, Group, HotItem, ImportReport, NewBookmark, NewEngine, NewGroup,
    Quote, Settings, TrashItem, Weather,
};
// 只在服务端的入库校验里用到（客户端的夹取在组件层）
#[cfg(feature = "server")]
use crate::models::clamp_span;

/// 把 `ServerFnError` 转成可直接展示给用户的文案。
///
/// `ServerFnError` 的 `Display` 会带上 "error running server function: … (details: None)"
/// 这类包装前缀，界面上只该显示服务端返回的原始消息。
pub fn error_text(error: &ServerFnError) -> String {
    match error {
        ServerFnError::ServerError { message, .. } => message.clone(),
        other => other.to_string(),
    }
}

/// 转义 LIKE 通配符，避免用户输入的 `%` `_` 被当作模式匹配。
#[cfg(feature = "server")]
fn escape_like(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        if matches!(ch, '\\' | '%' | '_') {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

/// 把一个关键词包成 `%word%` 形式的 LIKE 模式（已转义）。
#[cfg(feature = "server")]
fn like_pattern(word: &str) -> String {
    format!("%{}%", escape_like(word))
}

/// 按关键词个数拼出查询 SQL。
///
/// 结构固定：每个关键词对应一个括号分组，组内在 title / url / description /
/// tags 四个字段间 OR；组与组之间 AND。字段数固定为 4，所以每个关键词要
/// 绑定 4 个相同的模式参数。
///
/// 回收站里的条目（`deleted_at` 非空）永远不出现在这里。
///
/// `bookmarks_only` 为真时排除组件。**导出**走这一档：组件不是书签，写进给浏览器
/// 导入的书签文件只会得到一条打不开的 `widget:` 链接。列表查询则两样都要，前端再按
/// kind 分流（组件置顶、不参与筛选与分页）。
#[cfg(feature = "server")]
fn build_search_sql(keyword_count: usize, bookmarks_only: bool) -> String {
    const CLAUSE: &str = "(title LIKE ? ESCAPE '\\' \
                           OR url LIKE ? ESCAPE '\\' \
                           OR description LIKE ? ESCAPE '\\' \
                           OR tags LIKE ? ESCAPE '\\')";

    let mut sql = String::from(
        "SELECT id, title, url, description, tags, group_id, group_ids, icon, created_at, \
         span_w, span_h, kind, config, surface \
         FROM bookmarks WHERE deleted_at IS NULL",
    );

    if bookmarks_only {
        sql.push_str(" AND kind = ''");
    }

    if keyword_count > 0 {
        let clauses = vec![CLAUSE; keyword_count];
        sql.push_str(" AND ");
        sql.push_str(&clauses.join(" AND "));
    }

    sql.push_str(" ORDER BY sort_order ASC, id ASC");
    sql
}

/// 校验并规范化之后的入库字段。新增与修改共用，避免两处校验漂移。
#[cfg(feature = "server")]
pub(crate) struct CleanBookmark {
    pub title: String,
    pub url: String,
    pub description: String,
    pub tags: String,
    pub group_id: Option<i64>,
    /// 全部分组 id，逗号分隔；空串表示未分组。
    pub group_ids: String,
    pub icon: String,
    pub span_w: i64,
    pub span_h: i64,
    pub kind: String,
    pub config: String,
    /// 卡片的面。空串＝没设过，用该种类的默认（见 `models::effective_surface`）。
    pub surface: String,
}

/// 校验并规范化卡片入参。新增与修改共用，避免两处校验漂移。
///
/// 书签与组件分两条路走：书签必须有合法网址、可以进分组带标签；组件没有网址，
/// 也不属于任何分组（分组标签页算的是书签），参数按 kind 规范化后落库。
#[cfg(feature = "server")]
async fn clean_bookmark(input: &NewBookmark) -> Result<CleanBookmark, String> {
    use crate::models::{
        is_widget_kind, text_icon_parts, CalendarConfig, ClockConfig, DialConfig, HotSearchConfig,
        NoteConfig, TodoConfig, WeatherConfig, KIND_CALENDAR, KIND_CLOCK, KIND_DIAL, KIND_HOT,
        KIND_NOTE, KIND_TODO, KIND_WEATHER,
    };

    let title = input.title.trim().to_string();
    let kind = input.kind.trim().to_string();

    if !kind.is_empty() && !is_widget_kind(&kind) {
        return Err(format!("不认识的卡片类型：{kind}"));
    }

    if title.is_empty() {
        return Err("标题不能为空".to_string());
    }

    // 图标：空串=自动代取，其余与分组图标同一套约定。两类卡片共用这一条。
    let icon = input.icon.trim().to_string();
    let icon_ok = icon.is_empty()
        || icon.starts_with("i:")
        || text_icon_parts(&icon).is_some()
        || icon.starts_with("http://")
        || icon.starts_with("https://")
        || icon.starts_with('/');
    if !icon_ok {
        return Err("图标需为内置图标、文字图标、http(s) 地址或以 / 开头的本站路径".to_string());
    }

    // 跨度在这里夹好：0（老备份、调用方没传）会被抬到 1
    let span_w = clamp_span(input.span_w);
    let span_h = clamp_span(input.span_h);
    // 面收敛到白名单；认不出的当成"没设过"，由渲染时的默认接手
    let surface = crate::models::normalize_surface(&input.surface);

    // ---- 组件 ----
    if !kind.is_empty() {
        // 参数按类型规范化：只留这一版认识的字段，调用方塞进来的多余内容落不了库。
        // 解析失败回落默认值，不报错——参数只是显示开关，不值得为它拒收一张卡片。
        let config = match kind.as_str() {
            KIND_CLOCK => ClockConfig::parse(&input.config).to_json(),
            KIND_DIAL => DialConfig::parse(&input.config).to_json(),
            KIND_NOTE => {
                // 正文额外收一道：按字符数截断、去掉首尾空白。它是自由文本，一次粘贴
                // 就可能上千字，而 config 会跟着列表接口的每次响应一起回去。
                let text = NoteConfig::parse(&input.config).text;
                NoteConfig {
                    text: NoteConfig::clean(&text),
                }
                .to_json()
            }
            KIND_WEATHER => {
                // 地点名同样收一道：它会被拼进上游的查询串
                let config = WeatherConfig::parse(&input.config);
                WeatherConfig {
                    location: WeatherConfig::clean_location(&config.location),
                    fahrenheit: config.fahrenheit,
                }
                .to_json()
            }
            KIND_CALENDAR => CalendarConfig::parse(&input.config).to_json(),
            KIND_TODO => TodoConfig::parse(&input.config).clean().to_json(),
            KIND_HOT => {
                let config = HotSearchConfig::parse(&input.config);
                HotSearchConfig {
                    source: HotSearchConfig::clean_source(&config.source),
                }
                .to_json()
            }
            _ => String::new(),
        };

        return Ok(CleanBookmark {
            title,
            // 组件没有网址。留空而不是硬塞一个伪地址：伪地址会在"按网址去重"和
            // "导出成书签文件"这两条路上变成需要打补丁的特例。
            url: String::new(),
            description: input.description.trim().to_string(),
            tags: String::new(),
            group_id: None,
            group_ids: String::new(),
            icon,
            span_w,
            span_h,
            kind,
            config,
            surface,
        });
    }

    // ---- 书签 ----
    let url = input.url.trim().to_string();
    if url.is_empty() {
        return Err("URL 不能为空".to_string());
    }
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err("URL 需以 http:// 或 https:// 开头".to_string());
    }

    // 分组可能已被删掉，落库前确认一次；不存在就丢掉，免得留下指向空气的 id
    // 让条目在任何分组标签下都不显示。`group_ids` 是新字段，旧调用方只传
    // `group_id` 时退回单值。
    let mut candidates: Vec<i64> = input.group_ids.clone();
    if candidates.is_empty() {
        if let Some(id) = input.group_id {
            candidates.push(id);
        }
    }
    let known_groups: Vec<i64> = sqlx::query_scalar("SELECT id FROM groups")
        .fetch_all(crate::db::pool())
        .await
        .map_err(|e| format!("读取分组失败: {e}"))?;
    let mut group_ids: Vec<i64> = Vec::new();
    // 先过滤再截断：否则前 20 个里混着重复/已删的 id 时，后面合法的反而被丢掉。
    // 原始候选也设个上限，避免调用方塞一个超长数组来放大查询。
    for id in candidates.into_iter().take(100) {
        if id <= 0 || group_ids.contains(&id) || !known_groups.contains(&id) {
            continue;
        }
        group_ids.push(id);
        if group_ids.len() >= 20 {
            break;
        }
    }
    let group_id = group_ids.first().copied();

    Ok(CleanBookmark {
        title,
        url,
        description: input.description.trim().to_string(),
        tags: normalize_tags(&input.tags),
        group_id,
        group_ids: group_ids
            .iter()
            .map(|id| id.to_string())
            .collect::<Vec<_>>()
            .join(","),
        icon,
        span_w,
        span_h,
        kind,
        // 书签没有组件参数
        config: String::new(),
        surface,
    })
}

/// 规范化标签：按逗号拆分、去空白、丢空项、去重、重新用逗号连接。
#[cfg(feature = "server")]
fn normalize_tags(raw: &str) -> String {
    split_tags(raw).join(",")
}

/// 拆出标签列表（去空白、丢空项、保持首次出现的顺序、去重）。
#[cfg(feature = "server")]
fn split_tags(raw: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for tag in raw.split(',') {
        let tag = tag.trim();
        if !tag.is_empty() && !out.iter().any(|existing| existing == tag) {
            out.push(tag.to_string());
        }
    }
    out
}

// ---------------------------------------------------------------------------
// 书签
// ---------------------------------------------------------------------------

/// 列出书签。`query` 按空格拆成多个关键词，关键词之间 AND，
/// 每个关键词在 title / url / description / tags 四个字段间 OR。
///
/// 前端的即时筛选是纯客户端做的（列表本就是全量），这个带关键词的入口
/// 保留给接口调用方，逻辑与单测都在。
#[server]
pub async fn list_bookmarks(query: String) -> Result<Vec<Bookmark>, ServerFnError> {
    use crate::db;

    let keywords: Vec<String> = query.split_whitespace().map(str::to_string).collect();
    // 组件也要取回来：首页按 kind 分流（组件置顶、不参与筛选与分页），
    // 少了一边就得再发一次请求，还会让两边顺序对不上。
    let sql = build_search_sql(keywords.len(), false);

    // 该 SQL 仅由固定模板拼成，文本中不含任何用户输入；关键词一律 bind 传参。
    let mut q = sqlx::query_as::<_, Bookmark>(sqlx::AssertSqlSafe(sql));
    for keyword in &keywords {
        let pattern = like_pattern(keyword);
        for _ in 0..4 {
            q = q.bind(pattern.clone());
        }
    }

    q.fetch_all(db::pool())
        .await
        .map_err(|e| ServerFnError::new(format!("查询书签失败: {e}")))
}

/// 落库一张新卡片，写之前先去重。新记录排在列表最前
/// （`sort_order` 取当前最小值再减一）。
///
/// **去重口径按种类分**：书签比 URL 字符串（与导入一致），组件比 `kind`——组件没有
/// 网址，按空网址去重会让第二张组件被判成"已存在"而静默不写入；按 kind 比则正好
/// 表达"时钟只要一个"。两条都**把回收站里的算进去**，否则同一网址（或同一个组件）
/// 会一边躺在回收站、一边又出现在列表里。
/// 命中时返回已有记录的 id 且 `created == false`，由调用方决定怎么提示，
/// 这里不报错（"已存在"不是失败，静默再插一条才是问题）。
///
/// 收一个 `pool` 参数而不是直接用全局连接池，是为了能在单测里喂内存库。
#[cfg(feature = "server")]
pub(crate) async fn insert_new_bookmark(
    pool: &sqlx::SqlitePool,
    row: CleanBookmark,
) -> Result<AddOutcome, String> {
    // 同一条命中多个（历史遗留）时，优先命中正常条目，其次才是回收站里的
    let existing: Option<(i64, Option<String>)> = if row.kind.is_empty() {
        sqlx::query_as(
            "SELECT id, deleted_at FROM bookmarks WHERE kind = '' AND url = ? \
             ORDER BY (deleted_at IS NOT NULL) ASC, id ASC LIMIT 1",
        )
        .bind(&row.url)
        .fetch_optional(pool)
        .await
        .map_err(|e| format!("检查重复失败: {e}"))?
    } else {
        sqlx::query_as(
            "SELECT id, deleted_at FROM bookmarks WHERE kind = ? \
             ORDER BY (deleted_at IS NOT NULL) ASC, id ASC LIMIT 1",
        )
        .bind(&row.kind)
        .fetch_optional(pool)
        .await
        .map_err(|e| format!("检查重复失败: {e}"))?
    };

    if let Some((id, deleted_at)) = existing {
        return Ok(AddOutcome {
            id,
            created: false,
            in_trash: deleted_at.is_some(),
        });
    }

    let kind = row.kind;
    let id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO bookmarks \
         (title, url, description, tags, group_id, group_ids, icon, span_w, span_h, kind, config, \
          surface, sort_order) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, \
                 COALESCE((SELECT MIN(sort_order) FROM bookmarks), 0) - 1) \
         RETURNING id",
    )
    .bind(row.title)
    .bind(row.url)
    .bind(row.description)
    .bind(row.tags)
    .bind(row.group_id)
    .bind(row.group_ids)
    .bind(row.icon)
    .bind(row.span_w)
    .bind(row.span_h)
    .bind(&kind)
    .bind(row.config)
    .bind(row.surface)
    .fetch_one(pool)
    .await
    .map_err(|e| {
        if kind.is_empty() {
            format!("保存书签失败: {e}")
        } else {
            format!("保存卡片失败: {e}")
        }
    })?;

    Ok(AddOutcome {
        id,
        created: true,
        in_trash: false,
    })
}

/// 快捷键添加入口（书签小工具 / `POST /api/bookmark`）共用的落库路径。
///
/// 与表单提交的区别：只要一个 URL，标题缺省时用主机名兜底，一律进「未分组」。
#[cfg(feature = "server")]
pub(crate) async fn quick_add(
    pool: &sqlx::SqlitePool,
    url: &str,
    title: &str,
    description: &str,
) -> Result<AddOutcome, String> {
    let url = url.trim();
    if !crate::transfer::is_usable_url(url) {
        return Err("URL 需以 http:// 或 https:// 开头".to_string());
    }

    let title = title.trim();
    // 标题兜底用主机名：书签小工具通常带着页面标题来，但接口调用方未必
    let title = if title.is_empty() {
        crate::favicon::host_of(url).unwrap_or_else(|| url.to_string())
    } else {
        title.to_string()
    };

    insert_new_bookmark(
        pool,
        CleanBookmark {
            title,
            url: url.to_string(),
            description: description.trim().to_string(),
            tags: String::new(),
            group_id: None,
            group_ids: String::new(),
            icon: String::new(),
            // 快捷添加只给 URL，格子尺寸用默认的一格
            span_w: 1,
            span_h: 1,
            // 快捷添加只会加书签——它拿到的就是一个网址
            kind: crate::models::KIND_BOOKMARK.to_string(),
            config: String::new(),
            // 没设过面：书签卡的默认是"无面"，与加进来之前的样子一致
            surface: String::new(),
        },
    )
    .await
}

/// 改书签时确认这个 URL 没被别的条目占用。
///
/// 只在 URL **真的变了**时才查：去重是后加上的，历史数据里可能已经有重复，
/// 若一概拦下，那些书签连改个标题都改不动。
#[cfg(feature = "server")]
pub(crate) async fn ensure_url_not_taken(
    pool: &sqlx::SqlitePool,
    id: i64,
    url: &str,
) -> Result<(), String> {
    let previous: Option<String> = sqlx::query_scalar("SELECT url FROM bookmarks WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(|e| format!("读取原网址失败: {e}"))?;

    // 没改动（或这条已不存在）就不必查重
    if previous.as_deref() == Some(url) {
        return Ok(());
    }

    let taken: Option<i64> = sqlx::query_scalar(
        "SELECT id FROM bookmarks WHERE kind = '' AND url = ? AND id <> ? LIMIT 1",
    )
    .bind(url)
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(|e| format!("检查重复失败: {e}"))?;

    if taken.is_some() {
        return Err("该网址已被另一条书签使用".to_string());
    }

    Ok(())
}

/// 新增卡片，返回新记录 id（重复则不写入，见 [`insert_new_bookmark`]）。
///
/// 书签与组件共用这一个入口：入参里的 `kind` 决定它是哪一种。多一个"加组件"的
/// 接口只会让两边各写一遍校验与去重，而那正是最容易漂移的地方。
#[server]
pub async fn add_bookmark(input: NewBookmark) -> Result<AddOutcome, ServerFnError> {
    use crate::db;

    let row = clean_bookmark(&input).await.map_err(ServerFnError::new)?;

    insert_new_bookmark(db::pool(), row)
        .await
        .map_err(ServerFnError::new)
}

/// 修改已有卡片（标题 / URL / 描述 / 标签 / 分组 / 图标 / 格子尺寸 / 组件参数 / 底色），
/// 不改排序位与种类。
#[server]
pub async fn update_bookmark(id: i64, input: NewBookmark) -> Result<(), ServerFnError> {
    use crate::db;

    let row = clean_bookmark(&input).await.map_err(ServerFnError::new)?;

    // 网址查重只对书签有意义：组件的 url 是空串，且它的唯一性由 kind 去重保证
    if row.kind.is_empty() {
        ensure_url_not_taken(db::pool(), id, &row.url)
            .await
            .map_err(ServerFnError::new)?;
    }

    let affected = sqlx::query(
        "UPDATE bookmarks SET title = ?, url = ?, description = ?, tags = ?, group_id = ?, \
         group_ids = ?, icon = ?, span_w = ?, span_h = ?, config = ?, surface = ? WHERE id = ?",
    )
    .bind(row.title)
    .bind(row.url)
    .bind(row.description)
    .bind(row.tags)
    .bind(row.group_id)
    .bind(row.group_ids)
    .bind(row.icon)
    .bind(row.span_w)
    .bind(row.span_h)
    .bind(row.config)
    .bind(row.surface)
    .bind(id)
    .execute(db::pool())
    .await
    .map_err(|e| ServerFnError::new(format!("修改卡片失败: {e}")))?
    .rows_affected();

    if affected == 0 {
        return Err(ServerFnError::new("该卡片已不存在"));
    }

    Ok(())
}

/// 按传入的 id 顺序重写 `sort_order`（0, 1, 2, …）。
///
/// 前端传的是**全量**顺序（列表始终取全量），因此能整体覆盖。分页只影响显示，
/// 拖拽仍然作用在全量顺序上。
#[server]
pub async fn reorder_bookmarks(ids: Vec<i64>) -> Result<(), ServerFnError> {
    use crate::db;

    let mut tx = db::pool()
        .begin()
        .await
        .map_err(|e| ServerFnError::new(format!("开始事务失败: {e}")))?;

    for (index, id) in ids.iter().enumerate() {
        sqlx::query("UPDATE bookmarks SET sort_order = ? WHERE id = ?")
            .bind(index as i64)
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(|e| ServerFnError::new(format!("更新排序失败: {e}")))?;
    }

    tx.commit()
        .await
        .map_err(|e| ServerFnError::new(format!("提交排序失败: {e}")))?;

    Ok(())
}

/// 删除书签 —— 实际是软删除，进回收站，可在设置里还原。
///
/// 幂等：删除不存在的 id 不报错。
#[server]
pub async fn delete_bookmark(id: i64) -> Result<(), ServerFnError> {
    delete_bookmarks(vec![id]).await.map(|_| ())
}

/// 批量软删除，返回实际移入回收站的条数。
///
/// 逐条 UPDATE 而不是拼 `IN (?, ?, …)`：SQLite 没有数组绑定，动态拼占位符
/// 反而更容易出错；这些语句都在同一个事务里，几百条也只是一次提交。
#[server]
pub async fn delete_bookmarks(ids: Vec<i64>) -> Result<usize, ServerFnError> {
    use crate::db;

    if ids.is_empty() {
        return Ok(0);
    }

    let mut tx = db::pool()
        .begin()
        .await
        .map_err(|e| ServerFnError::new(format!("开始事务失败: {e}")))?;

    let mut affected = 0usize;
    for id in &ids {
        affected += sqlx::query(
            "UPDATE bookmarks SET deleted_at = datetime('now') \
             WHERE id = ? AND deleted_at IS NULL",
        )
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(|e| ServerFnError::new(format!("删除书签失败: {e}")))?
        .rows_affected() as usize;
    }

    tx.commit()
        .await
        .map_err(|e| ServerFnError::new(format!("提交删除失败: {e}")))?;

    Ok(affected)
}

/// 批量移动到某个分组；`group_id` 为 `None` 表示移到"未分组"。
#[server]
pub async fn move_bookmarks(ids: Vec<i64>, group_id: Option<i64>) -> Result<usize, ServerFnError> {
    use crate::db;

    if ids.is_empty() {
        return Ok(0);
    }

    let pool = db::pool();

    // 与 clean_bookmark 同理：分组不存在就归到"未分组"
    let group_id = match group_id {
        Some(id) => sqlx::query_scalar::<_, i64>("SELECT id FROM groups WHERE id = ?")
            .bind(id)
            .fetch_optional(pool)
            .await
            .map_err(|e| ServerFnError::new(format!("检查分组失败: {e}")))?,
        None => None,
    };

    let mut tx = pool
        .begin()
        .await
        .map_err(|e| ServerFnError::new(format!("开始事务失败: {e}")))?;

    let mut affected = 0usize;
    for id in &ids {
        // 组件不属于任何分组：即使调用方误传了组件 id 也写不进去。
        affected += sqlx::query(
            "UPDATE bookmarks SET group_id = ?, group_ids = ? WHERE id = ? AND kind = ''",
        )
        .bind(group_id)
        .bind(group_id.map(|id| id.to_string()).unwrap_or_default())
        .bind(id)
        .execute(&mut *tx)
            .await
            .map_err(|e| ServerFnError::new(format!("移动分组失败: {e}")))?
            .rows_affected() as usize;
    }

    tx.commit()
        .await
        .map_err(|e| ServerFnError::new(format!("提交移动失败: {e}")))?;

    Ok(affected)
}

// ---------------------------------------------------------------------------
// 回收站
// ---------------------------------------------------------------------------

/// 回收站列表，最近删的在前。
#[server]
pub async fn list_trash() -> Result<Vec<TrashItem>, ServerFnError> {
    use crate::db;

    sqlx::query_as::<_, TrashItem>(
        "SELECT id, title, url, deleted_at FROM bookmarks \
         WHERE deleted_at IS NOT NULL ORDER BY deleted_at DESC, id DESC",
    )
    .fetch_all(db::pool())
    .await
    .map_err(|e| ServerFnError::new(format!("读取回收站失败: {e}")))
}

/// 把回收站里的书签还原回列表，返回还原条数。
#[server]
pub async fn restore_bookmarks(ids: Vec<i64>) -> Result<usize, ServerFnError> {
    use crate::db;

    if ids.is_empty() {
        return Ok(0);
    }

    let mut tx = db::pool()
        .begin()
        .await
        .map_err(|e| ServerFnError::new(format!("开始事务失败: {e}")))?;

    let mut affected = 0usize;
    for id in &ids {
        affected += sqlx::query(
            "UPDATE bookmarks SET deleted_at = NULL WHERE id = ? AND deleted_at IS NOT NULL",
        )
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(|e| ServerFnError::new(format!("还原书签失败: {e}")))?
        .rows_affected() as usize;
    }

    tx.commit()
        .await
        .map_err(|e| ServerFnError::new(format!("提交还原失败: {e}")))?;

    Ok(affected)
}

/// 彻底删除（不可恢复）。传全部 id 即"清空回收站"。
#[server]
pub async fn purge_bookmarks(ids: Vec<i64>) -> Result<usize, ServerFnError> {
    use crate::db;

    if ids.is_empty() {
        return Ok(0);
    }

    let mut tx = db::pool()
        .begin()
        .await
        .map_err(|e| ServerFnError::new(format!("开始事务失败: {e}")))?;

    let mut affected = 0usize;
    for id in &ids {
        affected += sqlx::query("DELETE FROM bookmarks WHERE id = ? AND deleted_at IS NOT NULL")
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(|e| ServerFnError::new(format!("彻底删除失败: {e}")))?
            .rows_affected() as usize;
    }

    tx.commit()
        .await
        .map_err(|e| ServerFnError::new(format!("提交彻底删除失败: {e}")))?;

    Ok(affected)
}

// ---------------------------------------------------------------------------
// 分组
// ---------------------------------------------------------------------------

/// 列出所有分组，按自定义顺序。
#[server]
pub async fn list_groups() -> Result<Vec<Group>, ServerFnError> {
    use crate::db;

    sqlx::query_as::<_, Group>("SELECT id, name, icon FROM groups ORDER BY sort_order ASC, id ASC")
        .fetch_all(db::pool())
        .await
        .map_err(|e| ServerFnError::new(format!("加载分组失败: {e}")))
}

/// 新增分组，落在末尾。返回新 id。
#[server]
pub async fn add_group(input: NewGroup) -> Result<i64, ServerFnError> {
    use crate::db;

    let name = input.name.trim().to_string();
    if name.is_empty() {
        return Err(ServerFnError::new("分组名不能为空"));
    }

    let pool = db::pool();

    let exists: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM groups WHERE name = ?")
        .bind(&name)
        .fetch_one(pool)
        .await
        .map_err(|e| ServerFnError::new(format!("检查分组失败: {e}")))?;
    if exists > 0 {
        return Err(ServerFnError::new(format!("分组「{name}」已存在")));
    }

    sqlx::query_scalar::<_, i64>(
        "INSERT INTO groups (name, icon, sort_order) \
         VALUES (?, ?, COALESCE((SELECT MAX(sort_order) FROM groups), 0) + 1) RETURNING id",
    )
    .bind(&name)
    .bind(input.icon.trim())
    .fetch_one(pool)
    .await
    .map_err(|e| ServerFnError::new(format!("保存分组失败: {e}")))
}

/// 重命名 / 改图标。
#[server]
pub async fn update_group(id: i64, input: NewGroup) -> Result<(), ServerFnError> {
    use crate::db;

    let name = input.name.trim().to_string();
    if name.is_empty() {
        return Err(ServerFnError::new("分组名不能为空"));
    }

    let pool = db::pool();

    let clash: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM groups WHERE name = ? AND id <> ?")
        .bind(&name)
        .bind(id)
        .fetch_one(pool)
        .await
        .map_err(|e| ServerFnError::new(format!("检查分组失败: {e}")))?;
    if clash > 0 {
        return Err(ServerFnError::new(format!("分组「{name}」已存在")));
    }

    let affected = sqlx::query("UPDATE groups SET name = ?, icon = ? WHERE id = ?")
        .bind(&name)
        .bind(input.icon.trim())
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| ServerFnError::new(format!("修改分组失败: {e}")))?
        .rows_affected();

    if affected == 0 {
        return Err(ServerFnError::new("该分组已不存在"));
    }

    Ok(())
}

/// 删除分组。组内书签归到"未分组"，书签本身不删。
#[server]
pub async fn delete_group(id: i64) -> Result<(), ServerFnError> {
    use crate::db;

    let pool = db::pool();
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| ServerFnError::new(format!("开始事务失败: {e}")))?;

    // 回收站里的书签也要一起归位，否则还原后仍指向已删的分组。
    // 多分组之后不能只清 `group_id`：要把这个 id 从每条的 `group_ids` 里摘掉，
    // 再按剩下的第一个重算主分组。书签量级不大（< 5000），逐条读改写最直白。
    let rows: Vec<(i64, Option<i64>, String)> =
        sqlx::query_as("SELECT id, group_id, group_ids FROM bookmarks")
            .fetch_all(&mut *tx)
            .await
            .map_err(|e| ServerFnError::new(format!("读取组内书签失败: {e}")))?;
    // 重算主分组时只认还存在的分组，避免把一条本来就悬空的 id 提成主分组。
    let known_groups: Vec<i64> = sqlx::query_scalar("SELECT id FROM groups")
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| ServerFnError::new(format!("读取分组失败: {e}")))?;

    for (bookmark_id, primary, raw) in rows {
        // 与 `Bookmark::group_id_list` 用同一个解析器：主分组也参与判断，
        // 否则"只有 group_id 指向要删的组"的老数据会被漏掉。
        let ids = crate::models::parse_group_ids(&raw, primary);
        let references = primary == Some(id) || ids.iter().any(|known| *known == id);
        if !references {
            continue;
        }

        let remaining: Vec<i64> = ids
            .into_iter()
            .filter(|known| *known != id && known_groups.contains(known))
            .collect();
        let next_primary = remaining.first().copied();
        let next_raw = remaining
            .iter()
            .map(|known| known.to_string())
            .collect::<Vec<_>>()
            .join(",");
        sqlx::query("UPDATE bookmarks SET group_id = ?, group_ids = ? WHERE id = ?")
            .bind(next_primary)
            .bind(next_raw)
            .bind(bookmark_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| ServerFnError::new(format!("归位组内书签失败: {e}")))?;
    }

    sqlx::query("DELETE FROM groups WHERE id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(|e| ServerFnError::new(format!("删除分组失败: {e}")))?;

    tx.commit()
        .await
        .map_err(|e| ServerFnError::new(format!("提交删除分组失败: {e}")))?;

    Ok(())
}

/// 按传入的 id 顺序重写分组的 `sort_order`（0, 1, 2, …）。
#[server]
pub async fn reorder_groups(ids: Vec<i64>) -> Result<(), ServerFnError> {
    use crate::db;

    let mut tx = db::pool()
        .begin()
        .await
        .map_err(|e| ServerFnError::new(format!("开始事务失败: {e}")))?;

    for (index, id) in ids.iter().enumerate() {
        sqlx::query("UPDATE groups SET sort_order = ? WHERE id = ?")
            .bind(index as i64)
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(|e| ServerFnError::new(format!("更新分组排序失败: {e}")))?;
    }

    tx.commit()
        .await
        .map_err(|e| ServerFnError::new(format!("提交分组排序失败: {e}")))?;

    Ok(())
}

// ---------------------------------------------------------------------------
// 搜索引擎
// ---------------------------------------------------------------------------

/// 列出所有搜索引擎，按 id 升序（预置的百度/谷歌/必应在最前）。
#[server]
pub async fn list_engines() -> Result<Vec<Engine>, ServerFnError> {
    use crate::db;

    sqlx::query_as::<_, Engine>(
        "SELECT id, name, url, icon, active, prefix FROM engines ORDER BY id",
    )
    .fetch_all(db::pool())
    .await
    .map_err(|e| ServerFnError::new(format!("加载搜索引擎失败: {e}")))
}

/// 规范化前缀：小写、只留字母数字与 `-`，避免和关键词粘连产生歧义。
#[cfg(feature = "server")]
fn normalize_prefix(raw: &str) -> String {
    raw.trim()
        .to_ascii_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect()
}

/// 新增搜索引擎。地址必须含 `%s` 占位；若当前没有任何引擎被选中，新的自动生效。
#[server]
pub async fn add_engine(input: NewEngine) -> Result<i64, ServerFnError> {
    use crate::db;

    let name = input.name.trim().to_string();
    let url = input.url.trim().to_string();
    let icon = input.icon.trim().to_string();
    let prefix = normalize_prefix(&input.prefix);

    if name.is_empty() {
        return Err(ServerFnError::new("名称不能为空"));
    }
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err(ServerFnError::new("地址需以 http:// 或 https:// 开头"));
    }
    if !url.contains("%s") {
        return Err(ServerFnError::new("地址必须包含 %s 作为搜索词占位"));
    }

    let pool = db::pool();

    if !prefix.is_empty() {
        let clash: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM engines WHERE prefix = ?")
            .bind(&prefix)
            .fetch_one(pool)
            .await
            .map_err(|e| ServerFnError::new(format!("检查前缀失败: {e}")))?;
        if clash > 0 {
            return Err(ServerFnError::new(format!(
                "前缀「{prefix}」已被其他引擎占用"
            )));
        }
    }

    let id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO engines (name, url, icon, active, prefix) VALUES (?, ?, ?, 0, ?) RETURNING id",
    )
    .bind(name)
    .bind(url)
    .bind(icon)
    .bind(prefix)
    .fetch_one(pool)
    .await
    .map_err(|e| ServerFnError::new(format!("保存搜索引擎失败: {e}")))?;

    // 一个都没有选中时（例如列表曾被清空），让新引擎顶上
    let active_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM engines WHERE active = 1")
        .fetch_one(pool)
        .await
        .map_err(|e| ServerFnError::new(format!("检查当前引擎失败: {e}")))?;

    if active_count == 0 {
        sqlx::query("UPDATE engines SET active = 1 WHERE id = ?")
            .bind(id)
            .execute(pool)
            .await
            .map_err(|e| ServerFnError::new(format!("设置当前引擎失败: {e}")))?;
    }

    Ok(id)
}

/// 修改搜索引擎（名称 / 地址 / 图标 / 前缀）。当前引擎标记不变。
#[server]
pub async fn update_engine(id: i64, input: NewEngine) -> Result<(), ServerFnError> {
    use crate::db;

    let name = input.name.trim().to_string();
    let url = input.url.trim().to_string();
    let icon = input.icon.trim().to_string();
    let prefix = normalize_prefix(&input.prefix);

    if name.is_empty() {
        return Err(ServerFnError::new("名称不能为空"));
    }
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err(ServerFnError::new("地址需以 http:// 或 https:// 开头"));
    }
    if !url.contains("%s") {
        return Err(ServerFnError::new("地址必须包含 %s 作为搜索词占位"));
    }

    let pool = db::pool();

    if !prefix.is_empty() {
        let clash: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM engines WHERE prefix = ? AND id <> ?")
                .bind(&prefix)
                .bind(id)
                .fetch_one(pool)
                .await
                .map_err(|e| ServerFnError::new(format!("检查前缀失败: {e}")))?;
        if clash > 0 {
            return Err(ServerFnError::new(format!(
                "前缀「{prefix}」已被其他引擎占用"
            )));
        }
    }

    let affected =
        sqlx::query("UPDATE engines SET name = ?, url = ?, icon = ?, prefix = ? WHERE id = ?")
            .bind(name)
            .bind(url)
            .bind(icon)
            .bind(prefix)
            .bind(id)
            .execute(pool)
            .await
            .map_err(|e| ServerFnError::new(format!("修改搜索引擎失败: {e}")))?
            .rows_affected();

    if affected == 0 {
        return Err(ServerFnError::new("该搜索引擎已不存在"));
    }

    Ok(())
}

/// 删除搜索引擎。若删掉的是当前使用的那个，自动回退到列表中第一个。
#[server]
pub async fn delete_engine(id: i64) -> Result<(), ServerFnError> {
    use crate::db;

    let pool = db::pool();

    sqlx::query("DELETE FROM engines WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| ServerFnError::new(format!("删除搜索引擎失败: {e}")))?;

    let active_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM engines WHERE active = 1")
        .fetch_one(pool)
        .await
        .map_err(|e| ServerFnError::new(format!("检查当前引擎失败: {e}")))?;

    if active_count == 0 {
        sqlx::query(
            "UPDATE engines SET active = 1 \
             WHERE id = (SELECT id FROM engines ORDER BY id LIMIT 1)",
        )
        .execute(pool)
        .await
        .map_err(|e| ServerFnError::new(format!("回退当前引擎失败: {e}")))?;
    }

    Ok(())
}

/// 切换当前使用的搜索引擎。
#[server]
pub async fn set_active_engine(id: i64) -> Result<(), ServerFnError> {
    use crate::db;

    let pool = db::pool();

    sqlx::query("UPDATE engines SET active = 0")
        .execute(pool)
        .await
        .map_err(|e| ServerFnError::new(format!("切换搜索引擎失败: {e}")))?;

    sqlx::query("UPDATE engines SET active = 1 WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| ServerFnError::new(format!("切换搜索引擎失败: {e}")))?;

    Ok(())
}

// ---------------------------------------------------------------------------
// 主页设置
// ---------------------------------------------------------------------------

/// 读取主页设置。缺键时回落到默认值。
#[server]
pub async fn get_settings() -> Result<Settings, ServerFnError> {
    use crate::db;

    let rows: Vec<(String, String)> = sqlx::query_as("SELECT key, value FROM settings")
        .fetch_all(db::pool())
        .await
        .map_err(|e| ServerFnError::new(format!("读取设置失败: {e}")))?;

    Ok(Settings::from_pairs(rows))
}

/// 保存主页设置（逐键 upsert）。
#[server]
pub async fn save_settings(input: Settings) -> Result<(), ServerFnError> {
    use crate::db;
    use crate::models::is_allowed_background;

    let mut input = input;

    // 手填的那条地址（池还空着时就靠它）照旧要过形状校验。
    // 池本身在下面逐行校验——非法行要报出来，不能静默丢掉。
    let url = input.background_url.trim().to_string();
    if !url.is_empty() && !is_allowed_background(&url) {
        return Err(ServerFnError::new(
            "背景图片地址需以 http:// 、https:// 或 / 开头",
        ));
    }
    for line in &input.background_urls {
        let line = line.trim();
        if !line.is_empty() && !is_allowed_background(line) {
            let shown: String = line.chars().take(60).collect();
            return Err(ServerFnError::new(format!(
                "背景池里的地址需以 http:// 、https:// 或 / 开头：{shown}"
            )));
        }
    }

    // 当前地址必须落在池里（池为空时它就是手填的那条）。`to_pairs` 落库前
    // 还会再收敛一次，这里先归一是为了让校验看到的就是最终形态。
    let (pool, current) = input.normalized_background();
    input.background_urls = pool;
    input.background_url = current;

    let mut tx = db::pool()
        .begin()
        .await
        .map_err(|e| ServerFnError::new(format!("开始事务失败: {e}")))?;

    // 取值范围由 Settings::to_pairs 统一夹好，落库即是可信值
    for (key, value) in input.to_pairs() {
        sqlx::query(
            "INSERT INTO settings (key, value) VALUES (?, ?) \
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        )
        .bind(key)
        .bind(value)
        .execute(&mut *tx)
        .await
        .map_err(|e| ServerFnError::new(format!("保存设置失败: {e}")))?;
    }

    tx.commit()
        .await
        .map_err(|e| ServerFnError::new(format!("提交设置失败: {e}")))?;

    Ok(())
}

// ---------------------------------------------------------------------------
// 背景壁纸
// ---------------------------------------------------------------------------

/// 把外链图片「存到本地」：服务端抓一次，落进 `WALLPAPER_DIR`，返回新的访问地址。
///
/// 为什么要绕服务端而不是让浏览器下载：抓取要避开防盗链（服务端请求带 UA、
/// 不带页面的 Referer，往往比 img 更容易拿到图），落盘也只有服务端能做。
/// 走 server function 而不是新开一条路由：输入输出都是 JSON，正是它的形状；
/// `POST /wallpaper` 那条原始字节路由是给上传用的，两者不混。
#[server]
pub async fn save_wallpaper_from_url(url: String) -> Result<String, ServerFnError> {
    let url = url.trim().to_string();
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err(ServerFnError::new(
            "只有 http:// 或 https:// 开头的图片地址才需要存到本地",
        ));
    }

    crate::wallpaper::save_from_url(&url)
        .await
        .map_err(ServerFnError::new)
}

// ---------------------------------------------------------------------------
// 快捷添加令牌
// ---------------------------------------------------------------------------

/// 读取快捷添加用的令牌，没有就现生成一个（首次打开设置面板即就位）。
#[server]
pub async fn get_api_token() -> Result<String, ServerFnError> {
    crate::quickadd::ensure_api_token(crate::db::pool())
        .await
        .map_err(ServerFnError::new)
}

/// 换一个新令牌。旧的书签小工具随之失效。
#[server]
pub async fn regenerate_api_token() -> Result<String, ServerFnError> {
    crate::quickadd::regenerate_api_token(crate::db::pool())
        .await
        .map_err(ServerFnError::new)
}

// ---------------------------------------------------------------------------
// 导入 / 导出
// ---------------------------------------------------------------------------

/// 批量导入。自动识别浏览器导出的书签 HTML 与本应用的 JSON 备份。
///
/// 按 URL 去重（含库内已有的）；新记录追加在现有排序之后，并保持文件内顺序。
/// JSON 备份里的 `group` 会按名字对应到分组，分组不存在则自动建。
/// 浏览器 HTML 里的文件夹路径仍然进 tags（分类改由分组承担，tags 只作检索词）。
#[server]
pub async fn import_bookmarks(text: String) -> Result<ImportReport, ServerFnError> {
    use crate::db;

    let parsed = crate::transfer::parse_any(&text);
    if parsed.is_empty() {
        return Err(ServerFnError::new(
            "没解析出任何书签：请粘贴浏览器导出的书签 HTML，或本应用导出的 JSON 备份",
        ));
    }

    let pool = db::pool();
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| ServerFnError::new(format!("开始事务失败: {e}")))?;

    let existing: Vec<String> = sqlx::query_scalar("SELECT url FROM bookmarks")
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| ServerFnError::new(format!("读取已有书签失败: {e}")))?;
    let mut seen: std::collections::HashSet<String> = existing.into_iter().collect();

    let mut next_sort: i64 =
        sqlx::query_scalar("SELECT COALESCE(MAX(sort_order), 0) FROM bookmarks")
            .fetch_one(&mut *tx)
            .await
            .map_err(|e| ServerFnError::new(format!("计算排序位失败: {e}")))?;

    // 已有的分组名 → id，边导入边补
    let rows: Vec<(i64, String)> = sqlx::query_as("SELECT id, name FROM groups")
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| ServerFnError::new(format!("读取分组失败: {e}")))?;
    let mut group_ids: std::collections::HashMap<String, i64> =
        rows.into_iter().map(|(id, name)| (name, id)).collect();
    let mut next_group_sort: i64 =
        sqlx::query_scalar("SELECT COALESCE(MAX(sort_order), 0) FROM groups")
            .fetch_one(&mut *tx)
            .await
            .map_err(|e| ServerFnError::new(format!("计算分组排序位失败: {e}")))?;

    // 缩略图：等事务提交后再落盘（见循环里的说明）
    let mut inline_icons: Vec<(String, String, Vec<u8>)> = Vec::new();

    let mut report = ImportReport::default();

    for item in parsed {
        let url = item.url.trim().to_string();
        let title = item.title.trim().to_string();

        if !crate::transfer::is_usable_url(&url) || seen.contains(&url) {
            report.skipped += 1;
            continue;
        }

        // 备份里的分组名：没有就建一个，保证还原后分组结构还在。
        // 多分组备份带 `groups`；旧备份只有 `group`，退回单值。
        let mut wanted: Vec<String> = item.groups.clone();
        if wanted.is_empty() && !item.group.trim().is_empty() {
            wanted.push(item.group.trim().to_string());
        }
        let mut bookmark_group_ids: Vec<i64> = Vec::new();
        for name in wanted {
            let name = name.trim();
            if name.is_empty() || bookmark_group_ids.len() >= 20 {
                continue;
            }
            let id = match group_ids.get(name) {
                Some(id) => *id,
                None => {
                    next_group_sort += 1;
                    let id = sqlx::query_scalar::<_, i64>(
                        "INSERT INTO groups (name, icon, sort_order) VALUES (?, '', ?) RETURNING id",
                    )
                    .bind(name)
                    .bind(next_group_sort)
                    .fetch_one(&mut *tx)
                    .await
                    .map_err(|e| ServerFnError::new(format!("创建分组失败: {e}")))?;
                    group_ids.insert(name.to_string(), id);
                    id
                }
            };
            if !bookmark_group_ids.contains(&id) {
                bookmark_group_ids.push(id);
            }
        }
        let group_id = bookmark_group_ids.first().copied();
        let group_ids_csv = bookmark_group_ids
            .iter()
            .map(|id| id.to_string())
            .collect::<Vec<_>>()
            .join(",");

        next_sort += 1;

        // 浏览器导出的 HTML 里内联了站点图标（`ICON="data:…"`）时先收集起来，
        // **等事务提交后**再写图标缓存：写缓存会另取一条池连接去写 favicons 表，
        // 在外层写事务还开着的时候写，SQLite 直接报 `database is locked`。
        // 收在这里还有个好处：导入整体回滚时不会留下图标。
        if let Some(inline) = item.inline_icon.as_deref() {
            if let Some((content_type, data)) =
                crate::favicon::decode_data_uri(inline.trim_start_matches("data:"))
            {
                if let Some(host) = crate::favicon::host_of(&url) {
                    inline_icons.push((host, content_type, data));
                }
            }
        }

        sqlx::query(
            "INSERT INTO bookmarks \
             (title, url, description, tags, group_id, group_ids, icon, span_w, span_h, sort_order) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(if title.is_empty() { url.clone() } else { title })
        .bind(&url)
        .bind(item.description.trim())
        .bind(normalize_tags(&item.tags))
        .bind(group_id)
        .bind(&group_ids_csv)
        .bind(item.icon.trim())
        // JSON 备份里的跨度原样搬回（Netscape 解析出来的恒为 1）
        .bind(clamp_span(item.span_w))
        .bind(clamp_span(item.span_h))
        .bind(next_sort)
        .execute(&mut *tx)
        .await
        .map_err(|e| ServerFnError::new(format!("写入书签失败: {e}")))?;

        seen.insert(url);
        report.imported += 1;
    }

    tx.commit()
        .await
        .map_err(|e| ServerFnError::new(format!("提交导入失败: {e}")))?;

    // 事务已结束，这时写图标缓存不会再和它抢写锁
    for (host, content_type, data) in inline_icons {
        crate::favicon::store_icon(&host, &content_type, &data).await;
    }

    Ok(report)
}

/// 把全部书签（不含回收站）按当前显示顺序导出为 JSON 备份。
///
/// 不是 server function——由 `/export/bookmarks.json` 路由直接调用。
#[cfg(feature = "server")]
pub async fn export_bookmarks_json() -> String {
    use crate::db;
    use crate::models::ImportedBookmark;

    // 只要书签：备份是拿去导入别的书签工具的，组件在那里没有对应物
    // （`/export/db.sqlite` 那份完整快照才包含组件）。
    let sql = build_search_sql(0, true);

    let rows = sqlx::query_as::<_, Bookmark>(sqlx::AssertSqlSafe(sql))
        .fetch_all(db::pool())
        .await
        .unwrap_or_default();

    // 分组写成名字，跨库导入才认得
    let group_names: std::collections::HashMap<i64, String> =
        sqlx::query_as::<_, (i64, String)>("SELECT id, name FROM groups")
            .fetch_all(db::pool())
            .await
            .unwrap_or_default()
            .into_iter()
            .collect();

    let items: Vec<ImportedBookmark> = rows
        .iter()
        .map(|bookmark| {
            let names: Vec<String> = bookmark
                .group_id_list()
                .into_iter()
                .filter_map(|id| group_names.get(&id).cloned())
                .collect();
            ImportedBookmark::from_bookmark(bookmark, &names)
        })
        .collect();

    crate::transfer::to_json(&items)
}

/// 清空站点图标缓存（库表记录 + 磁盘文件），返回清掉的条目数。
///
/// 取不到的站点会留一条"空"的负缓存记录，在有效期内不重试；所以修好网络
/// （或对方站点恢复）之后，界面上仍会一直缺图标。这个入口用来立刻清掉重来，
/// 不必等 TTL 过期、也不必进容器删文件。
#[server]
pub async fn clear_icon_cache() -> Result<usize, ServerFnError> {
    Ok(crate::favicon::clear_cache().await)
}

/// 把全部书签导出为 Netscape 书签 HTML（浏览器可直接导入）。
///
/// 不是 server function——由 `/export/bookmarks.html` 路由直接调用，理由同 JSON 导出。
#[cfg(feature = "server")]
pub async fn export_bookmarks_html() -> String {
    use crate::db;
    use crate::transfer::ExportRow;

    // 只要书签。这一份是给浏览器导入的，组件的 url 是空串，写出去就是一条
    // 打不开的链接，会一直留在用户的书签栏里。
    let sql = build_search_sql(0, true);

    let rows = sqlx::query_as::<_, Bookmark>(sqlx::AssertSqlSafe(sql))
        .fetch_all(db::pool())
        .await
        .unwrap_or_default();

    let group_names: std::collections::HashMap<i64, String> =
        sqlx::query_as::<_, (i64, String)>("SELECT id, name FROM groups")
            .fetch_all(db::pool())
            .await
            .unwrap_or_default()
            .into_iter()
            .collect();

    let items: Vec<ExportRow> = rows
        .iter()
        .map(|bookmark| ExportRow {
            title: bookmark.title.clone(),
            url: bookmark.url.clone(),
            description: bookmark.description.clone(),
            group: bookmark
                .group_id_list()
                .first()
                .and_then(|id| group_names.get(id).cloned())
                .unwrap_or_default(),
        })
        .collect();

    crate::transfer::to_netscape(&items)
}

/// 打一份数据库的一致快照并返回字节。
///
/// 用 `VACUUM INTO` 而不是直接拷贝文件：库开着 WAL，最近的写入还在 `-wal` 里，
/// 拷 `mtab.db` 会丢掉那部分（README 里"数据落在 /data/mtab.db"那种说法容易
/// 让人以为直接拷文件就行）。`VACUUM INTO` 是 SQLite 官方给的一致快照做法，
/// 产出一个单文件、可直接拿去替换或另存。
#[cfg(feature = "server")]
pub async fn export_database_snapshot() -> Result<Vec<u8>, String> {
    use crate::db;

    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let path = std::env::temp_dir().join(format!("nekotab-snapshot-{nanos}.sqlite"));

    // VACUUM INTO 要求目标文件不存在；并发的两次导出各有各的时间戳，不会撞
    let target = path.to_string_lossy().to_string();
    if let Err(e) = sqlx::query("VACUUM INTO ?")
        .bind(&target)
        .execute(db::pool())
        .await
    {
        return Err(format!("生成数据库快照失败: {e}"));
    }

    let bytes = tokio::fs::read(&path)
        .await
        .map_err(|e| format!("读取快照失败: {e}"))?;
    let _ = tokio::fs::remove_file(&path).await;

    Ok(bytes)
}

// ---------------------------------------------------------------------------
// 一言
// ---------------------------------------------------------------------------

/// 「一言」接口返回体（只取需要的字段）。
#[cfg(feature = "server")]
#[derive(serde::Deserialize)]
struct Hitokoto {
    hitokoto: String,
    #[serde(default)]
    from: String,
    #[serde(default)]
    from_who: Option<String>,
}

/// 复用一个客户端，避免每次请求都重做 TLS 握手。favicon 代取也复用它。
#[cfg(feature = "server")]
pub(crate) fn http_client() -> &'static reqwest::Client {
    static CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();

    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(8))
            .build()
            .expect("创建 HTTP 客户端失败")
    })
}

/// 拉一条一言。由服务端代取，浏览器不直接访问第三方：既避开跨域，
/// 也能统一做超时与失败兜底（失败时前端保留上一条，不打断页面）。
#[server]
pub async fn get_quote() -> Result<Quote, ServerFnError> {
    let body = http_client()
        .get("https://v1.hitokoto.cn/")
        .send()
        .await
        .map_err(|e| ServerFnError::new(format!("请求一言接口失败: {e}")))?
        .json::<Hitokoto>()
        .await
        .map_err(|e| ServerFnError::new(format!("解析一言响应失败: {e}")))?;

    // from_who 是作者、from 是出处，两者可能缺一个
    let source = match (body.from_who.filter(|w| !w.is_empty()), body.from) {
        (Some(who), from) if !from.is_empty() => format!("{who}《{from}》"),
        (Some(who), _) => who,
        (None, from) => from,
    };

    Ok(Quote {
        text: body.hitokoto,
        source,
    })
}

/// 查一次天气。同样由服务端代取：浏览器直连会被 CORS 挡，而且统一在服务端取
/// 才能用上复用客户端 + 8 秒超时那一套（见 [`http_client`]）。
///
/// 与「一言」不同的是，**失败要说出原因**：一言取不到就保持空白无所谓，天气卡片空了
/// 用户只会以为是坏的。所以这里的 Err 文案是要直接摆到卡片上的。
///
/// 地点是用户输入，但它只进查询参数（已百分号编码），目标地址是写死的——
/// 不存在让对方指定请求地址那类问题，这一点与 [`suggest`] 的取舍一致。
#[server]
pub async fn get_weather(location: String, fahrenheit: bool) -> Result<Weather, ServerFnError> {
    let location = crate::models::WeatherConfig::clean_location(&location);
    if location.is_empty() {
        return Err(ServerFnError::new("先填一个地点"));
    }

    crate::weather::fetch(&location, fahrenheit)
        .await
        .map_err(ServerFnError::new)
}

/// 只更新一张组件卡片的参数。
///
/// 待办卡片上的勾选/新增走这里：它只改 `config`，不动标题、跨度、底色与顺序，
/// 因此不必把整张卡片回传，也不会因为客户端持有一份旧快照而把别的字段写回去。
/// 参数仍按 kind 规范化，脏数据进不了库。
#[server]
pub async fn update_widget_config(id: i64, config: String) -> Result<(), ServerFnError> {
    use crate::db;
    use crate::models::{
        CalendarConfig, HotSearchConfig, NoteConfig, TodoConfig, WeatherConfig, KIND_CALENDAR,
        KIND_HOT, KIND_NOTE, KIND_TODO, KIND_WEATHER,
    };

    let kind: Option<String> = sqlx::query_scalar("SELECT kind FROM bookmarks WHERE id = ?")
        .bind(id)
        .fetch_optional(db::pool())
        .await
        .map_err(|e| ServerFnError::new(format!("读取卡片失败: {e}")))?;

    let Some(kind) = kind else {
        return Err(ServerFnError::new("该卡片已不存在"));
    };

    let normalized = match kind.as_str() {
        // 两种时钟的参数在各自的设置表单里改，这里不开放
        KIND_CALENDAR => CalendarConfig::parse(&config).to_json(),
        KIND_TODO => TodoConfig::parse(&config).clean().to_json(),
        KIND_HOT => {
            let parsed = HotSearchConfig::parse(&config);
            HotSearchConfig {
                source: HotSearchConfig::clean_source(&parsed.source),
            }
            .to_json()
        }
        KIND_NOTE => {
            let text = NoteConfig::parse(&config).text;
            NoteConfig {
                text: NoteConfig::clean(&text),
            }
            .to_json()
        }
        KIND_WEATHER => {
            let parsed = WeatherConfig::parse(&config);
            WeatherConfig {
                location: WeatherConfig::clean_location(&parsed.location),
                fahrenheit: parsed.fahrenheit,
            }
            .to_json()
        }
        _ => return Err(ServerFnError::new("该卡片不是可编辑参数的组件")),
    };

    sqlx::query("UPDATE bookmarks SET config = ? WHERE id = ?")
        .bind(normalized)
        .bind(id)
        .execute(db::pool())
        .await
        .map_err(|e| ServerFnError::new(format!("保存组件参数失败: {e}")))?;

    Ok(())
}

/// 热搜榜。服务端代取并解析，客户端只拿 `(标题, 地址)`。
///
/// `source` 走白名单；服务端只请求写死的地址，不接受客户端传 URL。
#[server]
pub async fn get_hot_search(source: String) -> Result<Vec<HotItem>, ServerFnError> {
    let source = crate::models::HotSearchConfig::clean_source(&source);
    crate::hot::fetch(&source).await.map_err(ServerFnError::new)
}

/// 搜索联想。由服务端代取引擎的联想接口（浏览器直连会被 CORS 挡）。
///
/// 入参是**库里引擎的 id**，不是地址——服务端拿 id 反查 URL，再对照
/// `suggest::endpoint_for` 认得的端点表。这样客户端无法让我们去请求任意地址
/// （SSRF），和 `/icon/{host}` 只服务已收藏站点是同一个取舍。
///
/// 联想失败一律返回空表、不报错：它只是锦上添花，出口抖一下、对方限流一次，
/// 都不该在界面上留下痕迹。搜索本身不受影响。
#[server]
pub async fn suggest(engine_id: i64, query: String) -> Result<Vec<String>, ServerFnError> {
    use crate::db;
    use crate::suggest;

    let query = query.trim();
    if query.is_empty() {
        return Ok(Vec::new());
    }

    let engine_url: Option<String> = sqlx::query_scalar("SELECT url FROM engines WHERE id = ?")
        .bind(engine_id)
        .fetch_optional(db::pool())
        .await
        .map_err(|e| ServerFnError::new(format!("读取搜索引擎失败: {e}")))?;

    let Some(engine_url) = engine_url else {
        return Ok(Vec::new());
    };
    let Some(endpoint) = suggest::endpoint_for(&engine_url) else {
        return Ok(Vec::new());
    };

    let url = endpoint.url.replace("%s", &suggest::encode_query(query));

    // 单独给更短的超时：联想要跟着输入走，8 秒的通用超时等不起，
    // 慢一点就该当作"这家没有联想"，而不是让列表悬在那里。
    let response = http_client()
        .get(&url)
        .timeout(std::time::Duration::from_millis(2500))
        .send()
        .await;

    let Ok(response) = response else {
        return Ok(Vec::new());
    };
    if !response.status().is_success() {
        return Ok(Vec::new());
    }
    let Ok(body) = response.text().await else {
        return Ok(Vec::new());
    };

    let mut items = suggest::parse(endpoint.kind, &body);
    items.truncate(8);
    Ok(items)
}

#[cfg(all(test, feature = "server"))]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;
    use sqlx::SqlitePool;

    async fn seed() -> SqlitePool {
        // 内存库是「每连接一份」，因此固定单连接，保证建表和查询落在同一库上。
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("connect in-memory sqlite");

        sqlx::query(
            "CREATE TABLE bookmarks (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                title       TEXT NOT NULL,
                url         TEXT NOT NULL,
                description TEXT NOT NULL DEFAULT '',
                tags        TEXT NOT NULL DEFAULT '',
                group_id    INTEGER,
                -- 与迁移 007 一致：多分组的完整归属（逗号分隔）
                group_ids   TEXT NOT NULL DEFAULT '',
                icon        TEXT NOT NULL DEFAULT '',
                created_at  TEXT NOT NULL DEFAULT (datetime('now')),
                sort_order  INTEGER NOT NULL DEFAULT 0,
                deleted_at  TEXT,
                -- 与迁移 004 一致：卡片跨几格，默认一格
                span_w      INTEGER NOT NULL DEFAULT 1,
                span_h      INTEGER NOT NULL DEFAULT 1,
                -- 与迁移 005 一致：卡片种类与组件参数
                kind        TEXT NOT NULL DEFAULT '',
                config      TEXT NOT NULL DEFAULT '',
                -- 与迁移 006 一致：卡片的面
                surface     TEXT NOT NULL DEFAULT ''
            )",
        )
        .execute(&pool)
        .await
        .expect("create table");

        let rows = [
            (
                "Rust 官网",
                "https://rust-lang.org",
                "系统编程语言",
                "编程,rust",
                None,
            ),
            (
                "Dioxus",
                "https://dioxuslabs.com",
                "Rust 全栈框架",
                "rust,web",
                Some(1),
            ),
            (
                "Hacker News",
                "https://news.ycombinator.com",
                "科技新闻",
                "news",
                None,
            ),
            (
                "50% off",
                "https://example.com/sale",
                "折扣_信息",
                "shopping",
                Some(1),
            ),
        ];
        for (title, url, desc, tags, group_id) in rows {
            sqlx::query(
                "INSERT INTO bookmarks (title, url, description, tags, group_id) \
                 VALUES (?, ?, ?, ?, ?)",
            )
            .bind(title)
            .bind(url)
            .bind(desc)
            .bind(tags)
            .bind(group_id)
            .execute(&pool)
            .await
            .expect("insert");
        }

        // 与迁移 003 一致的回填：升序排 sort_order 等价于原来的 id 降序
        sqlx::query("UPDATE bookmarks SET sort_order = -id")
            .execute(&pool)
            .await
            .expect("backfill sort_order");

        pool
    }

    /// 复用与 `list_bookmarks` 完全相同的构造逻辑。
    async fn run(pool: &SqlitePool, query: &str) -> Vec<Bookmark> {
        let keywords: Vec<String> = query.split_whitespace().map(str::to_string).collect();
        let sql = build_search_sql(keywords.len(), false);
        let mut q = sqlx::query_as::<_, Bookmark>(sqlx::AssertSqlSafe(sql));
        for keyword in &keywords {
            let pattern = like_pattern(keyword);
            for _ in 0..4 {
                q = q.bind(pattern.clone());
            }
        }
        q.fetch_all(pool).await.expect("query")
    }

    #[tokio::test]
    async fn empty_query_returns_all() {
        let pool = seed().await;
        assert_eq!(run(&pool, "").await.len(), 4);
    }

    #[tokio::test]
    async fn matches_any_of_four_fields() {
        let pool = seed().await;
        assert_eq!(run(&pool, "rust").await.len(), 2, "title/tags 命中");
        assert_eq!(run(&pool, "官网").await.len(), 1, "title 命中");
        assert_eq!(run(&pool, "news").await.len(), 1, "tags 命中");
        assert_eq!(run(&pool, "ycombinator").await.len(), 1, "url 命中");
    }

    #[tokio::test]
    async fn keywords_are_anded() {
        let pool = seed().await;
        // rust 命中 #1#2，编程 命中 #1 → 交集仅 #1
        assert_eq!(run(&pool, "rust 编程").await.len(), 1);
        // rust 命中 #1#2，news 命中 #3 → 交集为空
        assert_eq!(run(&pool, "rust news").await.len(), 0);
    }

    #[tokio::test]
    async fn like_wildcards_are_escaped() {
        let pool = seed().await;
        // 若未转义，'%' 会匹配全部 4 行
        assert_eq!(run(&pool, "%").await.len(), 1, "只应命中含字面 % 的那条");
        // 若未转义，'_' 会匹配任意单字符
        assert_eq!(run(&pool, "_").await.len(), 1, "只应命中含字面 _ 的那条");
    }

    #[tokio::test]
    async fn newest_first() {
        let pool = seed().await;
        let ids: Vec<i64> = run(&pool, "").await.iter().map(|b| b.id).collect();
        assert_eq!(ids, vec![4, 3, 2, 1]);
    }

    #[tokio::test]
    async fn custom_sort_order_is_respected() {
        let pool = seed().await;

        // 手工把 #1 提到最前、#4 挪到最后
        sqlx::query("UPDATE bookmarks SET sort_order = ? WHERE id = ?")
            .bind(-10_i64)
            .bind(1_i64)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("UPDATE bookmarks SET sort_order = ? WHERE id = ?")
            .bind(10_i64)
            .bind(4_i64)
            .execute(&pool)
            .await
            .unwrap();

        let ids: Vec<i64> = run(&pool, "").await.iter().map(|b| b.id).collect();
        assert_eq!(ids, vec![1, 3, 2, 4]);
    }

    #[tokio::test]
    async fn soft_deleted_rows_are_excluded() {
        let pool = seed().await;

        sqlx::query("UPDATE bookmarks SET deleted_at = datetime('now') WHERE id = 2")
            .execute(&pool)
            .await
            .unwrap();

        let ids: Vec<i64> = run(&pool, "").await.iter().map(|b| b.id).collect();
        assert_eq!(ids, vec![4, 3, 1], "删除的不该出现在列表里");
        // 但按关键词也搜不到它
        assert_eq!(run(&pool, "dioxus").await.len(), 0);
    }

    #[tokio::test]
    async fn group_id_is_returned() {
        let pool = seed().await;
        let items = run(&pool, "").await;
        let dioxus = items.iter().find(|b| b.title == "Dioxus").unwrap();
        assert_eq!(dioxus.group_id, Some(1));
        let rust = items.iter().find(|b| b.title == "Rust 官网").unwrap();
        assert_eq!(rust.group_id, None, "未分组是 NULL");
    }

    #[tokio::test]
    async fn multi_group_ids_are_stored_and_read_back() {
        let pool = seed().await;

        let mut card = row("多分组", "https://multi.test");
        card.group_id = Some(1);
        card.group_ids = "1,2".to_string();
        let outcome = insert_new_bookmark(&pool, card).await.expect("insert");
        assert!(outcome.created);

        let items = run(&pool, "").await;
        let card = items.iter().find(|b| b.title == "多分组").unwrap();
        assert_eq!(card.group_id, Some(1), "主分组是第一个");
        assert_eq!(card.group_ids, "1,2");
        assert_eq!(card.group_id_list(), vec![1, 2]);
    }

    #[test]
    fn tags_normalize() {
        assert_eq!(normalize_tags(" rust , web ,, "), "rust,web");
        assert_eq!(normalize_tags("rust,rust,web"), "rust,web", "重复项去掉");
    }

    #[test]
    fn prefixes_are_sanitized() {
        assert_eq!(normalize_prefix(" GH "), "gh");
        assert_eq!(normalize_prefix("b-i-n-g"), "b-i-n-g");
        assert_eq!(normalize_prefix("a b/c"), "abc");
        assert_eq!(normalize_prefix("   "), "");
    }

    // ---- 新增去重 ----

    async fn count(pool: &SqlitePool) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM bookmarks")
            .fetch_one(pool)
            .await
            .expect("count")
    }

    /// 一个最小可入库行；只关心去重与跨度时用得上。
    fn row(title: &str, url: &str) -> CleanBookmark {
        CleanBookmark {
            title: title.to_string(),
            url: url.to_string(),
            description: String::new(),
            tags: String::new(),
            group_id: None,
            group_ids: String::new(),
            icon: String::new(),
            span_w: 1,
            span_h: 1,
            kind: crate::models::KIND_BOOKMARK.to_string(),
            config: String::new(),
            surface: String::new(),
        }
    }

    /// 一个最小可入库的组件行：没有网址，参数留空（等价于组件默认值）。
    fn widget_row(kind: &str) -> CleanBookmark {
        CleanBookmark {
            title: crate::models::widget_label(kind).to_string(),
            url: String::new(),
            description: String::new(),
            tags: String::new(),
            group_id: None,
            group_ids: String::new(),
            icon: String::new(),
            span_w: 2,
            span_h: 2,
            kind: kind.to_string(),
            config: String::new(),
            // 组件卡默认有面，但那由渲染时的默认决定；库里仍存"没设过"
            surface: String::new(),
        }
    }

    #[tokio::test]
    async fn existing_url_is_not_inserted_again() {
        let pool = seed().await;

        let hit = insert_new_bookmark(&pool, row("Rust 官网（重复）", "https://rust-lang.org"))
            .await
            .expect("insert");

        assert!(!hit.created, "同一个网址不该写第二条");
        assert_eq!(hit.id, 1, "应返回已有记录的 id");
        assert!(!hit.in_trash);
        assert_eq!(count(&pool).await, 4, "总数不变");

        // 全新网址照常写入
        let fresh = insert_new_bookmark(&pool, row("新的", "https://new.example"))
            .await
            .expect("insert");
        assert!(fresh.created);
        assert_eq!(count(&pool).await, 5);
    }

    #[tokio::test]
    async fn url_in_trash_also_counts_as_duplicate() {
        let pool = seed().await;

        sqlx::query("UPDATE bookmarks SET deleted_at = datetime('now') WHERE id = 1")
            .execute(&pool)
            .await
            .unwrap();

        let hit = insert_new_bookmark(&pool, row("Rust 官网", "https://rust-lang.org"))
            .await
            .expect("insert");

        assert!(!hit.created, "回收站里的同网址也算重复");
        assert!(hit.in_trash, "要能区分出它躺在回收站里");
        assert_eq!(count(&pool).await, 4);
    }

    #[tokio::test]
    async fn spans_are_stored_and_read_back() {
        let pool = seed().await;

        let mut spanning = row("大卡片", "https://big.example");
        spanning.span_w = 2;
        spanning.span_h = 3;
        let outcome = insert_new_bookmark(&pool, spanning).await.expect("insert");

        // 列表接口要把它带出来（Bookmark 的 FromRow 列清单里得有这两列）
        let items = run(&pool, "big").await;
        let saved = items.iter().find(|b| b.id == outcome.id).expect("找到");
        assert_eq!((saved.span_w, saved.span_h), (2, 3));

        // 老数据 / 没设过的仍是 1×1
        let plain = items.iter().find(|b| b.title == "Rust 官网");
        assert!(plain.is_none(), "关键词只筛 big");
        let all = run(&pool, "").await;
        let rust = all.iter().find(|b| b.title == "Rust 官网").expect("找到");
        assert_eq!((rust.span_w, rust.span_h), (1, 1));
    }

    #[tokio::test]
    async fn card_surface_is_normalised_by_clean_bookmark() {
        // 归一化在 clean_bookmark 这一层做完（CleanBookmark 是"已校验"的类型，
        // insert 只管写）。组件那条路不碰数据库，所以这里不需要连接池。
        let input = NewBookmark {
            title: "数字时钟".to_string(),
            kind: crate::models::KIND_CLOCK.to_string(),
            surface: "rainbow".to_string(),
            ..Default::default()
        };
        let cleaned = clean_bookmark(&input).await.expect("组件不需要数据库");
        assert_eq!(cleaned.surface, "", "认不出的面应收敛成'没设过'");

        let cleaned = clean_bookmark(&NewBookmark {
            surface: "solid".to_string(),
            ..input
        })
        .await
        .expect("组件不需要数据库");
        assert_eq!(cleaned.surface, "solid", "白名单里的值原样保留");
    }

    #[tokio::test]
    async fn card_surface_is_read_back_with_the_row() {
        let pool = seed().await;

        let mut solid = widget_row(crate::models::KIND_NOTE);
        solid.surface = "solid".to_string();
        let outcome = insert_new_bookmark(&pool, solid).await.expect("insert");

        // Bookmark 的 FromRow 列清单里得有 surface 这一列
        let items = run(&pool, "").await;
        let saved = items
            .iter()
            .find(|card| card.id == outcome.id)
            .expect("找到");
        assert_eq!(saved.surface, "solid");
        assert_eq!(saved.surface_class(), "solid");

        // 既有书签（surface 是空串）解析出来仍是"无面"——这次迁移不该改变它们的样子
        let plain = items
            .iter()
            .find(|card| card.title == "Rust 官网")
            .expect("找到");
        assert_eq!(plain.surface, "");
        assert_eq!(plain.surface_class(), "none");
    }

    #[tokio::test]
    async fn unknown_widget_kind_is_rejected() {
        // 不在白名单里的类型直接拒收：放进去就等于让渲染层收到没见过的分支
        let bad = NewBookmark {
            title: "彩虹".to_string(),
            kind: "rainbow".to_string(),
            ..Default::default()
        };
        let err = clean_bookmark(&bad).await.err().expect("未知类型应被拒收");
        assert!(err.contains("不认识的卡片类型"), "实际：{err}");
    }

    /// 组件这条路不该碰数据库（它没有网址、不进分组），所以不需要连接池也能测。
    #[tokio::test]
    async fn widget_needs_no_url_and_gets_a_normalised_config() {
        let clock = NewBookmark {
            title: "数字时钟".to_string(),
            kind: crate::models::KIND_CLOCK.to_string(),
            // 多塞一个字段、少写一个字段：落库的应当是"这一版认识的字段 + 默认值"
            config: r#"{"hour12":true,"junk":1}"#.to_string(),
            span_w: 2,
            span_h: 2,
            ..Default::default()
        };

        let cleaned = clean_bookmark(&clock).await.expect("组件不需要数据库");
        assert_eq!(cleaned.kind, crate::models::KIND_CLOCK);
        assert_eq!(cleaned.url, "", "组件没有网址");
        assert_eq!(cleaned.group_id, None, "组件不进分组");
        assert_eq!(cleaned.tags, "");
        assert_eq!((cleaned.span_w, cleaned.span_h), (2, 2));
        assert_eq!(
            crate::models::ClockConfig::parse(&cleaned.config),
            crate::models::ClockConfig {
                hour12: true,
                show_seconds: true
            },
            "多余的字段被丢掉，缺的字段按默认补上"
        );

        // 书签那一侧照旧：没有合法网址一律拒绝
        let bare = NewBookmark {
            title: "没网址".to_string(),
            ..Default::default()
        };
        assert!(clean_bookmark(&bare).await.is_err());
    }

    #[tokio::test]
    async fn note_text_is_capped_before_it_reaches_the_database() {
        use crate::models::{NoteConfig, NOTE_MAX_CHARS};

        // 内容是自由文本，一次粘贴就可能上千字，而它会跟着每次列表响应一起回去
        let input = NewBookmark {
            title: "记事本".to_string(),
            kind: crate::models::KIND_NOTE.to_string(),
            config: NoteConfig {
                text: "字".repeat(NOTE_MAX_CHARS + 10),
            }
            .to_json(),
            ..Default::default()
        };

        let cleaned = clean_bookmark(&input).await.expect("组件不需要数据库");
        let saved = NoteConfig::parse(&cleaned.config);
        assert_eq!(saved.text.chars().count(), NOTE_MAX_CHARS, "落库前已截断");
        assert_eq!(cleaned.url, "", "记事本同样没有网址");
        assert_eq!(cleaned.group_id, None);
    }

    #[tokio::test]
    async fn second_widget_of_the_same_kind_is_not_inserted() {
        let pool = seed().await;

        let first = insert_new_bookmark(&pool, widget_row(crate::models::KIND_CLOCK))
            .await
            .expect("insert");
        assert!(first.created);
        assert_eq!(count(&pool).await, 5);

        // 组件的 url 是空串，若沿用"按 url 去重"就会让这第二张被判成重复——
        // 而它本来就是重复，只是判据该是 kind 而不是 url
        let again = insert_new_bookmark(&pool, widget_row(crate::models::KIND_CLOCK))
            .await
            .expect("insert");
        assert!(!again.created, "同一种组件只要一个");
        assert_eq!(again.id, first.id);
        assert_eq!(count(&pool).await, 5, "没有多写一张");

        // 空网址不该让两张组件相互误判：另一个 kind 仍是独立的
        let other = CleanBookmark {
            kind: "chart".to_string(),
            ..widget_row(crate::models::KIND_CLOCK)
        };
        // chart 不在白名单里，但去重这一层只认 kind，所以它照样能落库；
        // 白名单由 clean_bookmark 把关（见上一个用例）
        let other = insert_new_bookmark(&pool, other).await.expect("insert");
        assert!(other.created, "不同 kind 互不干扰");
        assert_eq!(count(&pool).await, 6);
    }

    #[tokio::test]
    async fn widgets_stay_out_of_the_bookmark_export() {
        let pool = seed().await;
        insert_new_bookmark(&pool, widget_row(crate::models::KIND_CLOCK))
            .await
            .expect("insert");

        // 列表两样都要取回来，前端才能把组件置顶、不参与筛选与分页
        let all: Vec<Bookmark> =
            sqlx::query_as::<_, Bookmark>(sqlx::AssertSqlSafe(build_search_sql(0, false)))
                .fetch_all(&pool)
                .await
                .expect("query");
        assert_eq!(all.len(), 5);
        assert_eq!(all.iter().filter(|card| card.is_widget()).count(), 1);
        // 组件也带得回参数列（FromRow 的列清单里得有 kind / config）
        assert_eq!(
            all.iter()
                .find(|card| card.is_widget())
                .map(|card| card.config.as_str()),
            Some(""),
            "没传参数时 config 是空串，由 ClockConfig::parse 回落默认值"
        );

        // 导出只要书签：组件的 url 是空串，写进书签文件就是一条打不开的链接
        let exported: Vec<Bookmark> =
            sqlx::query_as::<_, Bookmark>(sqlx::AssertSqlSafe(build_search_sql(0, true)))
                .fetch_all(&pool)
                .await
                .expect("query");
        assert_eq!(exported.len(), 4, "组件不出现在书签导出里");
        assert!(exported.iter().all(|card| !card.is_widget()));
    }

    #[tokio::test]
    async fn quick_add_falls_back_to_host_as_title() {
        let pool = seed().await;

        let outcome = quick_add(&pool, "https://quick.example/some/page", "", "")
            .await
            .expect("quick add");
        assert!(outcome.created);

        let title: String = sqlx::query_scalar("SELECT title FROM bookmarks WHERE id = ?")
            .bind(outcome.id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(title, "quick.example", "没有标题时用主机名兜底");
    }

    #[tokio::test]
    async fn editing_a_legacy_duplicate_url_is_still_allowed() {
        let pool = seed().await;

        // 造一条历史重复：与 #1 同 URL（去重是后加的，老库里可能就有这种）
        sqlx::query("INSERT INTO bookmarks (title, url) VALUES (?, ?)")
            .bind("Rust 官网（旧重复）")
            .bind("https://rust-lang.org")
            .execute(&pool)
            .await
            .unwrap();

        // 不改 URL 的编辑必须放行，否则这种书签连标题都改不动
        assert!(
            ensure_url_not_taken(&pool, 1, "https://rust-lang.org")
                .await
                .is_ok(),
            "URL 没变时不该查重"
        );

        // 改成别人的 URL 要拦住
        assert!(
            ensure_url_not_taken(&pool, 1, "https://dioxuslabs.com")
                .await
                .is_err(),
            "撞上别的条目应拒绝"
        );

        // 改成全新的 URL 放行
        assert!(ensure_url_not_taken(&pool, 1, "https://brand-new.example")
            .await
            .is_ok());
    }

    #[tokio::test]
    async fn quick_add_rejects_non_http_urls() {
        let pool = seed().await;

        assert!(quick_add(&pool, "javascript:alert(1)", "x", "")
            .await
            .is_err());
        assert!(quick_add(&pool, "", "x", "").await.is_err());
        assert!(quick_add(&pool, "chrome://bookmarks", "x", "")
            .await
            .is_err());
        assert_eq!(count(&pool).await, 4, "被拒绝的都没写进去");
    }
}
