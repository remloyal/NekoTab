//! 服务端接口。`#[server]` 宏会把每个函数拆成：
//! - 客户端 stub（发 HTTP 请求）
//! - 服务端实现（直接执行函数体）
//!
//! 因此本模块在 web / server 两侧都会编译，函数体里对 `crate::db`、`sqlx`
//! 的引用只存在于服务端实现中，不会污染 wasm 产物。

use dioxus::prelude::*;

use crate::models::{
    Bookmark, Engine, ImportReport, NewBookmark, NewEngine, Quote, Settings,
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
#[cfg(feature = "server")]
fn build_search_sql(keyword_count: usize) -> String {
    const CLAUSE: &str = "(title LIKE ? ESCAPE '\\' \
                           OR url LIKE ? ESCAPE '\\' \
                           OR description LIKE ? ESCAPE '\\' \
                           OR tags LIKE ? ESCAPE '\\')";

    let mut sql = String::from(
        "SELECT id, title, url, description, tags, created_at FROM bookmarks",
    );

    if keyword_count > 0 {
        let clauses = vec![CLAUSE; keyword_count];
        sql.push_str(" WHERE ");
        sql.push_str(&clauses.join(" AND "));
    }

    sql.push_str(" ORDER BY sort_order ASC, id ASC");
    sql
}

/// 校验并规范化书签入参，返回 `(title, url, description, tags)`。
/// 新增与修改共用，避免两处校验漂移。
#[cfg(feature = "server")]
fn clean_bookmark(input: &NewBookmark) -> Result<(String, String, String, String), String> {
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

    Ok((title, url, description, normalize_tags(&input.tags)))
}

/// 规范化标签：按逗号拆分、去空白、丢空项、重新用逗号连接。
#[cfg(feature = "server")]
fn normalize_tags(raw: &str) -> String {
    raw.split(',')
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .collect::<Vec<_>>()
        .join(",")
}

/// 列出书签。`query` 按空格拆成多个关键词，关键词之间 AND，
/// 每个关键词在 title / url / description / tags 四个字段间 OR。
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

    let (title, url, description, tags) =
        clean_bookmark(&input).map_err(ServerFnError::new)?;

    sqlx::query_scalar::<_, i64>(
        "INSERT INTO bookmarks (title, url, description, tags, sort_order) \
         VALUES (?, ?, ?, ?, COALESCE((SELECT MIN(sort_order) FROM bookmarks), 0) - 1) \
         RETURNING id",
    )
    .bind(title)
    .bind(url)
    .bind(description)
    .bind(tags)
    .fetch_one(db::pool())
    .await
    .map_err(|e| ServerFnError::new(format!("保存书签失败: {e}")))
}

/// 修改已有书签（标题 / URL / 描述 / 标签），不改排序位。
#[server]
pub async fn update_bookmark(id: i64, input: NewBookmark) -> Result<(), ServerFnError> {
    use crate::db;

    let (title, url, description, tags) =
        clean_bookmark(&input).map_err(ServerFnError::new)?;

    let affected = sqlx::query(
        "UPDATE bookmarks SET title = ?, url = ?, description = ?, tags = ? WHERE id = ?",
    )
    .bind(title)
    .bind(url)
    .bind(description)
    .bind(tags)
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
/// 前端只在没有搜索词时允许拖拽，此时传入的是全量列表，因此能整体覆盖。
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

/// 删除书签。幂等：删除不存在的 id 不报错。
#[server]
pub async fn delete_bookmark(id: i64) -> Result<(), ServerFnError> {
    use crate::db;

    sqlx::query("DELETE FROM bookmarks WHERE id = ?")
        .bind(id)
        .execute(db::pool())
        .await
        .map_err(|e| ServerFnError::new(format!("删除书签失败: {e}")))?;

    Ok(())
}

// ---------------------------------------------------------------------------
// 搜索引擎
// ---------------------------------------------------------------------------

/// 列出所有搜索引擎，按 id 升序（预置的百度/谷歌/必应在最前）。
#[server]
pub async fn list_engines() -> Result<Vec<Engine>, ServerFnError> {
    use crate::db;

    sqlx::query_as::<_, Engine>("SELECT id, name, url, icon, active FROM engines ORDER BY id")
        .fetch_all(db::pool())
        .await
        .map_err(|e| ServerFnError::new(format!("加载搜索引擎失败: {e}")))
}

/// 新增搜索引擎。地址必须含 `%s` 占位；若当前没有任何引擎被选中，新的自动生效。
#[server]
pub async fn add_engine(input: NewEngine) -> Result<i64, ServerFnError> {
    use crate::db;

    let name = input.name.trim().to_string();
    let url = input.url.trim().to_string();
    let icon = input.icon.trim().to_string();

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

    let id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO engines (name, url, icon, active) VALUES (?, ?, ?, 0) RETURNING id",
    )
    .bind(name)
    .bind(url)
    .bind(icon)
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

    let mut settings = Settings::default();
    for (key, value) in rows {
        match key.as_str() {
            "background_url" => settings.background_url = value,
            "background_dim" => {
                settings.background_dim = value.parse().unwrap_or(35).clamp(0, 80);
            }
            "background_random" => settings.background_random = value != "0",
            "show_clock" => settings.show_clock = value != "0",
            "show_quote" => settings.show_quote = value != "0",
            _ => {}
        }
    }

    Ok(settings)
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

    let pairs = [
        ("background_url", background_url),
        ("background_dim", input.background_dim.clamp(0, 80).to_string()),
        (
            "background_random",
            if input.background_random { "1" } else { "0" }.to_string(),
        ),
        (
            "show_clock",
            if input.show_clock { "1" } else { "0" }.to_string(),
        ),
        (
            "show_quote",
            if input.show_quote { "1" } else { "0" }.to_string(),
        ),
    ];

    let mut tx = db::pool()
        .begin()
        .await
        .map_err(|e| ServerFnError::new(format!("开始事务失败: {e}")))?;

    for (key, value) in pairs {
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
#[server]
pub async fn import_bookmarks(text: String) -> Result<ImportReport, ServerFnError> {
    use crate::db;

    let parsed = crate::transfer::parse_any(&text);
    if parsed.is_empty() {
        return Err(ServerFnError::new(
            "没解析出任何书签：请粘贴浏览器导出的书签 HTML，或本应用导出的 JSON 备份",
        ));
    }

    let mut tx = db::pool()
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

    let mut report = ImportReport::default();

    for item in parsed {
        let url = item.url.trim().to_string();
        let title = item.title.trim().to_string();

        if !crate::transfer::is_usable_url(&url) || seen.contains(&url) {
            report.skipped += 1;
            continue;
        }

        next_sort += 1;

        sqlx::query(
            "INSERT INTO bookmarks (title, url, description, tags, sort_order) \
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(if title.is_empty() { url.clone() } else { title })
        .bind(&url)
        .bind(item.description.trim())
        .bind(normalize_tags(&item.tags))
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

    Ok(report)
}

/// 把全部书签按当前显示顺序导出为 JSON 备份。
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

    let items: Vec<ImportedBookmark> = rows.into_iter().map(ImportedBookmark::from).collect();
    crate::transfer::to_json(&items)
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
                created_at  TEXT NOT NULL DEFAULT (datetime('now')),
                sort_order  INTEGER NOT NULL DEFAULT 0
            )",
        )
        .execute(&pool)
        .await
        .expect("create table");

        let rows = [
            ("Rust 官网", "https://rust-lang.org", "系统编程语言", "编程,rust"),
            ("Dioxus", "https://dioxuslabs.com", "Rust 全栈框架", "rust,web"),
            ("Hacker News", "https://news.ycombinator.com", "科技新闻", "news"),
            ("50% off", "https://example.com/sale", "折扣_信息", "shopping"),
        ];
        for (title, url, desc, tags) in rows {
            sqlx::query("INSERT INTO bookmarks (title, url, description, tags) VALUES (?, ?, ?, ?)")
                .bind(title)
                .bind(url)
                .bind(desc)
                .bind(tags)
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
}
