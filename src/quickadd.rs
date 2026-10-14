//! 快捷添加：书签小工具与 `POST /api/bookmark` 共用的入口。整个文件仅在
//! `server` feature 下编译。
//!
//! 加书签本来必须先回到 NekoTab 页面右键，这里开一个带令牌的入口，任意页面都能
//! 把当前链接推进来。
//!
//! **为什么是带令牌的普通路由，而不是 server function**：server function 走的是
//! Dioxus 自己的调用协议，脚本/扩展要复刻它不现实；而且书签小工具只能靠"新开一个
//! 地址"提交（跨站 `fetch` 会被 CORS 挡，导航不受限），所以 `GET` 才是小工具能用的
//! 形态。两条路径（GET 给人和小工具、POST 给脚本）共用同一段落库逻辑。
//!
//! 令牌是**写入权限**，等同于"谁能加书签"。它存在 `settings` 表的 `api_token` 键上，
//! 不放进 [`crate::models::Settings`]——那是会被整份覆盖保存的结构，令牌混进去有
//! 被"另一次保存"带空的风险，单独读写更稳。

use std::collections::HashMap;

use dioxus::server::axum::{
    extract::Query,
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use sqlx::SqlitePool;

/// 令牌在 `settings` 表里的键。
const TOKEN_KEY: &str = "api_token";

/// `POST /api/bookmark` 的请求体。
#[derive(Debug, Default, serde::Deserialize)]
pub struct QuickAddRequest {
    #[serde(default)]
    pub token: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub description: String,
}

// ---------------------------------------------------------------------------
// 令牌
// ---------------------------------------------------------------------------

/// 生成一个 32 位十六进制的新令牌。
///
/// 随机源用 SQLite 的 `randomblob`，而不是引入 `rand`：它从操作系统取随机数，
/// 这里只需要"不可猜"，不值得为它多一个依赖。
async fn new_token(pool: &SqlitePool) -> Result<String, String> {
    sqlx::query_scalar::<_, String>("SELECT lower(hex(randomblob(16)))")
        .fetch_one(pool)
        .await
        .map_err(|e| format!("生成令牌失败: {e}"))
}

async fn read_token(pool: &SqlitePool) -> Result<Option<String>, String> {
    let value: Option<String> = sqlx::query_scalar("SELECT value FROM settings WHERE key = ?")
        .bind(TOKEN_KEY)
        .fetch_optional(pool)
        .await
        .map_err(|e| format!("读取令牌失败: {e}"))?;

    // 空值是"没设过"，不是"令牌为空"
    Ok(value.filter(|token| !token.trim().is_empty()))
}

async fn write_token(pool: &SqlitePool, token: &str) -> Result<(), String> {
    sqlx::query(
        "INSERT INTO settings (key, value) VALUES (?, ?) \
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(TOKEN_KEY)
    .bind(token)
    .execute(pool)
    .await
    .map(|_| ())
    .map_err(|e| format!("保存令牌失败: {e}"))
}

/// 取当前令牌；没有就现生成一个并落库（首次打开设置面板时自动就位）。
pub async fn ensure_api_token(pool: &SqlitePool) -> Result<String, String> {
    if let Some(token) = read_token(pool).await? {
        return Ok(token);
    }

    let token = new_token(pool).await?;
    write_token(pool, &token).await?;
    Ok(token)
}

/// 换一个新令牌。旧的书签小工具随之失效。
pub async fn regenerate_api_token(pool: &SqlitePool) -> Result<String, String> {
    let token = new_token(pool).await?;
    write_token(pool, &token).await?;
    Ok(token)
}

/// 令牌是否对得上。
///
/// 还没生成过令牌时一律拒绝——"没有令牌"绝不能退化成"不校验"，否则第一次
/// 部署、设置面板还没打开过的实例就是完全敞开的。
async fn token_matches(pool: &SqlitePool, provided: &str) -> Result<bool, String> {
    let Some(expected) = read_token(pool).await? else {
        return Ok(false);
    };

    Ok(!provided.is_empty() && provided == expected)
}

// ---------------------------------------------------------------------------
// 路由
// ---------------------------------------------------------------------------

/// `GET /api/bookmark?token=…&url=…&title=…`
///
/// 书签小工具的目标。给浏览器看，所以回一段小 HTML：提示结果并尝试自动关掉
/// 这个标签页（小工具是新开窗口提交的，留着没有意义）。
pub async fn add_handler(Query(params): Query<HashMap<String, String>>) -> Response {
    let pool = crate::db::pool();
    let get = |key: &str| params.get(key).map(String::as_str).unwrap_or("");

    match token_matches(pool, get("token")).await {
        Ok(true) => {}
        Ok(false) => {
            return html_page(
                StatusCode::FORBIDDEN,
                "令牌不正确或已失效。请在 NekoTab「设置 → 快捷添加」里重新复制书签小工具。",
            )
        }
        Err(e) => return html_page(StatusCode::INTERNAL_SERVER_ERROR, &e),
    }

    match crate::backend::quick_add(pool, get("url"), get("title"), get("description")).await {
        Ok(outcome) if outcome.created => html_page(StatusCode::OK, "已添加到 NekoTab。"),
        Ok(outcome) if outcome.in_trash => html_page(
            StatusCode::OK,
            "该网址已在回收站里，没有重复添加。要重新收下，先去回收站还原或彻底删除。",
        ),
        Ok(_) => html_page(StatusCode::OK, "该网址已在列表中，没有重复添加。"),
        Err(e) => html_page(StatusCode::BAD_REQUEST, &e),
    }
}

/// `POST /api/bookmark`，请求体是 JSON。给脚本 / 浏览器扩展用，
/// 回 JSON 而不是 HTML。
pub async fn post_handler(Json(body): Json<QuickAddRequest>) -> Response {
    let pool = crate::db::pool();

    match token_matches(pool, &body.token).await {
        Ok(true) => {}
        Ok(false) => {
            return (
                StatusCode::FORBIDDEN,
                Json(serde_json::json!({ "ok": false, "error": "令牌不正确或已失效" })),
            )
                .into_response()
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "ok": false, "error": e })),
            )
                .into_response()
        }
    }

    match crate::backend::quick_add(pool, &body.url, &body.title, &body.description).await {
        Ok(outcome) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "ok": true,
                "id": outcome.id,
                "created": outcome.created,
                "in_trash": outcome.in_trash,
            })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "ok": false, "error": e })),
        )
            .into_response(),
    }
}

/// 一条结果小页面。`message` 会转义后再拼进 HTML。
fn html_page(status: StatusCode, message: &str) -> Response {
    let body = format!(
        "<!doctype html><html lang=\"zh\"><head><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
         <title>NekoTab</title></head>\
         <body style=\"margin:0;display:grid;place-items:center;height:100vh;\
         font:15px/1.6 system-ui,-apple-system,'Segoe UI',sans-serif;\
         background:#0b0d13;color:#eef1f6;text-align:center\">\
         <p style=\"max-width:32em;padding:0 1.5em\">{}</p>\
         <script>setTimeout(function(){{window.close();}},1200);</script>\
         </body></html>",
        escape_html(message)
    );

    (
        status,
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        body,
    )
        .into_response()
}

/// 转义 HTML 文本。标题来自任意网页，必须转义后再进小页面。
fn escape_html(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    async fn pool_with_settings() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("connect in-memory sqlite");

        sqlx::query("CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL)")
            .execute(&pool)
            .await
            .expect("create settings");

        pool
    }

    #[test]
    fn escapes_html_metacharacters() {
        assert_eq!(
            escape_html(r#"<img src=x onerror="alert('1')">&"#),
            "&lt;img src=x onerror=&quot;alert(&#39;1&#39;)&quot;&gt;&amp;"
        );
        // 普通中文原样保留
        assert_eq!(escape_html("你好 NekoTab"), "你好 NekoTab");
    }

    #[tokio::test]
    async fn token_is_created_once_and_stable() {
        let pool = pool_with_settings().await;

        let first = ensure_api_token(&pool).await.expect("ensure");
        assert_eq!(first.len(), 32, "32 位十六进制");
        assert!(first.chars().all(|c| c.is_ascii_hexdigit()));

        let again = ensure_api_token(&pool).await.expect("ensure again");
        assert_eq!(first, again, "已有令牌不能被换掉");
    }

    #[tokio::test]
    async fn regenerate_invalidates_the_old_token() {
        let pool = pool_with_settings().await;

        let old = ensure_api_token(&pool).await.expect("ensure");
        assert!(token_matches(&pool, &old).await.expect("match old"));

        let new = regenerate_api_token(&pool).await.expect("regenerate");
        assert_ne!(old, new);
        assert!(token_matches(&pool, &new).await.expect("match new"));
        assert!(
            !token_matches(&pool, &old).await.expect("match old again"),
            "旧令牌必须失效"
        );
    }

    #[tokio::test]
    async fn missing_token_never_authorises() {
        let pool = pool_with_settings().await;

        // 还没生成过令牌：空串、随便什么串都不许通过
        assert!(!token_matches(&pool, "").await.expect("empty"));
        assert!(!token_matches(&pool, "guess").await.expect("guess"));
    }
}
