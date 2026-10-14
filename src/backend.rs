//! 服务端接口。`#[server]` 宏会把每个函数拆成：
//! - 客户端 stub（发 HTTP 请求）
//! - 服务端实现（直接执行函数体）
//!
//! 因此本模块在 web / server 两侧都会编译，函数体里对 `crate::db`、`sqlx`
//! 的引用只存在于服务端实现中，不会污染 wasm 产物。

use dioxus::prelude::*;

use crate::models::{
    Bookmark, Engine, Group, ImportReport, NewBookmark, NewEngine, NewGroup, Quote, Settings,
    TrashItem,
};

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
#[cfg(feature = "server")]
fn build_search_sql(keyword_count: usize) -> String {
    const CLAUSE: &str = "(title LIKE ? ESCAPE '\\' \
                           OR url LIKE ? ESCAPE '\\' \
                           OR description LIKE ? ESCAPE '\\' \
                           OR tags LIKE ? ESCAPE '\\')";

    let mut sql = String::from(
        "SELECT id, title, url, description, tags, group_id, icon, created_at FROM bookmarks \
         WHERE deleted_at IS NULL",
    );

    if keyword_count > 0 {
        let clauses = vec![CLAUSE; keyword_count];
        sql.push_str(" AND ");
        sql.push_str(&clauses.join(" AND "));
    }

    sql.push_str(" ORDER BY sort_order ASC, id ASC");
    sql
}

/// 校验并规范化书签入参，返回 `(title, url, description, tags, group_id, icon)`。
/// 新增与修改共用，避免两处校验漂移。
#[cfg(feature = "server")]
async fn clean_bookmark(
    input: &NewBookmark,
) -> Result<(String, String, String, String, Option<i64>, String), String> {
    let title = input.title.trim().to_string();
    let url = input.url.trim().to_string();
    let description = input.description.trim().to_string();

    if title.is_empty() {
        return Err("标题不能为空".to_string());
    }
    if url.is_empty() {
        return Err("URL 不能为空".to_string());
    }
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err("URL 需以 http:// 或 https:// 开头".to_string());
    }

    // 分组可能已被删掉，落库前确认一次；不存在就当归到"未分组"，
    // 免得留下指向空气的 group_id 让条目在任何分组标签下都不显示。
    let group_id = match input.group_id {
        Some(id) => sqlx::query_scalar::<_, i64>("SELECT id FROM groups WHERE id = ?")
            .bind(id)
            .fetch_optional(crate::db::pool())
            .await
            .map_err(|e| format!("检查分组失败: {e}"))?,
        None => None,
    };

    // 图标：空串=自动代取，其余与分组图标同一套约定
    let icon = input.icon.trim().to_string();
    let icon_ok = icon.is_empty()
        || icon.starts_with("i:")
        || icon.starts_with("http://")
        || icon.starts_with("https://")
        || icon.starts_with('/');
    if !icon_ok {
        return Err("图标需为内置图标、http(s) 地址或以 / 开头的本站路径".to_string());
    }

    Ok((
        title,
        url,
        description,
        normalize_tags(&input.tags),
        group_id,
        icon,
    ))
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
    let sql = build_search_sql(keywords.len());

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

/// 新增书签，返回新记录 id。新记录排在列表最前（`sort_order` 取当前最小值再减一）。
#[server]
pub async fn add_bookmark(input: NewBookmark) -> Result<i64, ServerFnError> {
    use crate::db;

    let (title, url, description, tags, group_id, icon) =
        clean_bookmark(&input).await.map_err(ServerFnError::new)?;

    sqlx::query_scalar::<_, i64>(
        "INSERT INTO bookmarks (title, url, description, tags, group_id, icon, sort_order) \
         VALUES (?, ?, ?, ?, ?, ?, COALESCE((SELECT MIN(sort_order) FROM bookmarks), 0) - 1) \
         RETURNING id",
    )
    .bind(title)
    .bind(url)
    .bind(description)
    .bind(tags)
    .bind(group_id)
    .bind(icon)
    .fetch_one(db::pool())
    .await
    .map_err(|e| ServerFnError::new(format!("保存书签失败: {e}")))
}

/// 修改已有书签（标题 / URL / 描述 / 标签 / 分组 / 图标），不改排序位。
#[server]
pub async fn update_bookmark(id: i64, input: NewBookmark) -> Result<(), ServerFnError> {
    use crate::db;

    let (title, url, description, tags, group_id, icon) =
        clean_bookmark(&input).await.map_err(ServerFnError::new)?;

    let affected = sqlx::query(
        "UPDATE bookmarks SET title = ?, url = ?, description = ?, tags = ?, group_id = ?, \
         icon = ? WHERE id = ?",
    )
    .bind(title)
    .bind(url)
    .bind(description)
    .bind(tags)
    .bind(group_id)
    .bind(icon)
    .bind(id)
    .execute(db::pool())
    .await
    .map_err(|e| ServerFnError::new(format!("修改书签失败: {e}")))?
    .rows_affected();

    if affected == 0 {
        return Err(ServerFnError::new("该书签已不存在"));
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
        affected += sqlx::query("UPDATE bookmarks SET group_id = ? WHERE id = ? AND group_id IS NOT ?")
            .bind(group_id)
            .bind(id)
            .bind(group_id)
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

    sqlx::query_as::<_, Group>(
        "SELECT id, name, icon FROM groups ORDER BY sort_order ASC, id ASC",
    )
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

    // 回收站里的书签也要一起归位，否则还原后仍指向已删的分组
    sqlx::query("UPDATE bookmarks SET group_id = NULL WHERE group_id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(|e| ServerFnError::new(format!("归位组内书签失败: {e}")))?;

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
            return Err(ServerFnError::new(format!("前缀「{prefix}」已被其他引擎占用")));
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

    let background_url = input.background_url.trim().to_string();
    let allowed = background_url.is_empty()
        || background_url.starts_with("http://")
        || background_url.starts_with("https://")
        || background_url.starts_with('/');
    if !allowed {
        return Err(ServerFnError::new(
            "背景图片地址需以 http:// 、https:// 或 / 开头",
        ));
    }

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

    let mut next_sort: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(sort_order), 0) FROM bookmarks")
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| ServerFnError::new(format!("计算排序位失败: {e}")))?;

    // 已有的分组名 → id，边导入边补
    let rows: Vec<(i64, String)> = sqlx::query_as("SELECT id, name FROM groups")
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| ServerFnError::new(format!("读取分组失败: {e}")))?;
    let mut group_ids: std::collections::HashMap<String, i64> = rows
        .into_iter()
        .map(|(id, name)| (name, id))
        .collect();
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

        // 备份里的分组名：没有就建一个，保证还原后分组结构还在
        let group_id = match item.group.trim() {
            "" => None,
            name => match group_ids.get(name) {
                Some(id) => Some(*id),
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
                    Some(id)
                }
            },
        };

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
            "INSERT INTO bookmarks (title, url, description, tags, group_id, icon, sort_order) \
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(if title.is_empty() { url.clone() } else { title })
        .bind(&url)
        .bind(item.description.trim())
        .bind(normalize_tags(&item.tags))
        .bind(group_id)
        .bind(item.icon.trim())
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

    let sql = build_search_sql(0);

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
            let name = bookmark
                .group_id
                .and_then(|id| group_names.get(&id).cloned())
                .unwrap_or_default();
            ImportedBookmark::from_bookmark(bookmark, &name)
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

    let sql = build_search_sql(0);

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
                .group_id
                .and_then(|id| group_names.get(&id).cloned())
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

/// 搜索联想。由服务端代取引擎的联想接口（浏览器直连会被 CORS 挡）。
///
/// 入参是**库里引擎的 id**，不是地址——服务端拿 id 反查 URL，再对照
/// `suggest::endpoint_for` 认得的端点表。这样客户端无法让我们去请求任意地址
/// （SSRF），和 `/icon/{host}` 只服务已收藏站点是同一个取舍。
///
/// 联想失败一律返回空表、不报错：它只是锦上添花，网络不通（纯内网部署）时
/// 不该在界面上留下任何痕迹。本地书签匹配不受影响。
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

    let url = endpoint
        .url
        .replace("%s", &suggest::encode_query(query));

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
                icon        TEXT NOT NULL DEFAULT '',
                created_at  TEXT NOT NULL DEFAULT (datetime('now')),
                sort_order  INTEGER NOT NULL DEFAULT 0,
                deleted_at  TEXT
            )",
        )
        .execute(&pool)
        .await
        .expect("create table");

        let rows = [
            ("Rust 官网", "https://rust-lang.org", "系统编程语言", "编程,rust", None),
            ("Dioxus", "https://dioxuslabs.com", "Rust 全栈框架", "rust,web", Some(1)),
            ("Hacker News", "https://news.ycombinator.com", "科技新闻", "news", None),
            ("50% off", "https://example.com/sale", "折扣_信息", "shopping", Some(1)),
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
        let sql = build_search_sql(keywords.len());
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
}
