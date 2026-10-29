//! 可选的整站访问密码。
//!
//! 部署时设置环境变量 `ACCESS_PASSWORD` 即启用：未登录的页面请求会跳到 `/login`，
//! server function / 导出 / 图标 / 壁纸这些数据入口直接返回 401。登录成功后写一个
//! 30 天有效的签名 Cookie，服务端不存会话表——签名里带过期时间，密钥由密码
//! （或可选的 `SESSION_SECRET`）派生，重启后旧 Cookie 仍然有效，改密码则全部失效。
//!
//! 取舍：
//! * 单密码、单用户，与"自部署书签页"的定位一致；不做账号体系。
//! * Cookie 是 `HttpOnly; SameSite=Lax`，HTTPS 下再加 `Secure`（看 `X-Forwarded-Proto`）。
//! * 连续失败若干次会全局锁 60 秒，挡住最基本的暴力尝试；真正的公网部署仍建议
//!   在反向代理层再加一道限流与 HTTPS。
//! * `/api/bookmark` 用自己的一次性令牌，不受这道密码影响——书签小工具从别的站点
//!   发起请求时没有这里的会话 Cookie。

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use dioxus::server::axum::body::Body;
use dioxus::server::axum::extract::Request;
use dioxus::server::axum::http::{header, StatusCode};
use dioxus::server::axum::middleware::Next;
use dioxus::server::axum::response::{IntoResponse, Response};

const COOKIE_NAME: &str = "nekotab_session";
/// 会话有效期：30 天。
const SESSION_SECS: u64 = 30 * 24 * 60 * 60;
/// 连续失败多少次后锁定。
const MAX_FAILS: u32 = 8;
/// 锁定多久。
const LOCK_FOR: Duration = Duration::from_secs(60);

struct Config {
    password: String,
    key: ring::hmac::Key,
}

static CONFIG: OnceLock<Option<Config>> = OnceLock::new();

#[derive(Default)]
struct Failures {
    count: u32,
    locked_until: Option<Instant>,
}

static FAILURES: OnceLock<Mutex<Failures>> = OnceLock::new();

/// 从环境变量读配置。`ACCESS_PASSWORD` 为空即整站不设防（保持向后兼容）。
///
/// 会话密钥优先取 `SESSION_SECRET`；没设时从库里取（没有就生成一个存进
/// `settings.session_secret`），再用它和访问密码派生。这样 Cookie 里的签名
/// 不是"直接用密码当 HMAC key"——只拿到一个 Cookie 的人无法离线快速枚举密码。
pub async fn init_from_env() {
    let password = std::env::var("ACCESS_PASSWORD").unwrap_or_default();
    if password.trim().is_empty() {
        if CONFIG.set(None).is_err() {
            eprintln!("[NekoTab] 访问密码配置已初始化过，忽略本次环境变量");
        } else {
            println!("[NekoTab] 未设置 ACCESS_PASSWORD，访问密码未启用");
        }
        return;
    }

    // 独立密钥优先：轮换密码时不想让所有会话一起失效，就固定 SESSION_SECRET。
    let env_secret = std::env::var("SESSION_SECRET")
        .ok()
        .filter(|value| !value.trim().is_empty());

    let (key, source) = match env_secret {
        Some(secret) => (
            ring::hmac::Key::new(ring::hmac::HMAC_SHA256, secret.as_bytes()),
            "SESSION_SECRET",
        ),
        None => match persisted_session_secret().await {
            Some(secret) => {
                // 用随机持久密钥再派生一次：密码变了旧会话自然失效，
                // 而 Cookie 本身不足以离线验证密码猜测。
                let base = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, secret.as_bytes());
                let derived = ring::hmac::sign(&base, password.as_bytes());
                (
                    ring::hmac::Key::new(ring::hmac::HMAC_SHA256, derived.as_ref()),
                    "数据库随机密钥",
                )
            }
            None => {
                eprintln!(
                    "[NekoTab] 生成会话密钥失败，退回用访问密码派生；建议设置 SESSION_SECRET"
                );
                (
                    ring::hmac::Key::new(ring::hmac::HMAC_SHA256, password.as_bytes()),
                    "访问密码",
                )
            }
        },
    };

    if CONFIG.set(Some(Config { password, key })).is_err() {
        eprintln!("[NekoTab] 访问密码配置已初始化过，忽略本次环境变量");
        return;
    }
    println!("[NekoTab] 访问密码已启用（登录状态保持 30 天，密钥来源：{source}）");
}

/// 从 `settings` 表取会话密钥；没有就生成 32 字节随机数存进去。
///
/// 用 SQLite 的 `randomblob` 而不是再引一个随机数 crate。`INSERT OR IGNORE`
/// 兜住并发启动：两个进程同时生成时，后写的那个会被忽略，再读一次拿到先写的。
async fn persisted_session_secret() -> Option<String> {
    let pool = crate::db::pool();

    let existing: Option<String> =
        sqlx::query_scalar("SELECT value FROM settings WHERE key = 'session_secret'")
            .fetch_optional(pool)
            .await
            .ok()
            .flatten();
    if let Some(value) = existing.filter(|value| !value.trim().is_empty()) {
        return Some(value);
    }

    let generated: String = sqlx::query_scalar("SELECT lower(hex(randomblob(32)))")
        .fetch_one(pool)
        .await
        .ok()?;
    sqlx::query("INSERT OR IGNORE INTO settings (key, value) VALUES ('session_secret', ?)")
        .bind(&generated)
        .execute(pool)
        .await
        .ok()?;

    sqlx::query_scalar("SELECT value FROM settings WHERE key = 'session_secret'")
        .fetch_optional(pool)
        .await
        .ok()
        .flatten()
        .filter(|value: &String| !value.trim().is_empty())
}

fn config() -> Option<&'static Config> {
    CONFIG.get().and_then(|value| value.as_ref())
}

/// 是否启用了访问密码。给设置面板/文档用。
#[cfg_attr(not(feature = "server"), allow(dead_code))]
#[allow(dead_code)]
pub fn enabled() -> bool {
    config().is_some()
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    if text.len() % 2 != 0 {
        return None;
    }
    let mut out = Vec::with_capacity(text.len() / 2);
    let bytes = text.as_bytes();
    for chunk in bytes.chunks(2) {
        let high = (chunk[0] as char).to_digit(16)?;
        let low = (chunk[1] as char).to_digit(16)?;
        out.push((high * 16 + low) as u8);
    }
    Some(out)
}

fn make_token(cfg: &Config) -> String {
    let expiry = now_secs() + SESSION_SECS;
    let signature = ring::hmac::sign(&cfg.key, expiry.to_string().as_bytes());
    format!("{expiry}.{}", hex(signature.as_ref()))
}

fn token_ok(cfg: &Config, token: &str) -> bool {
    let Some((expiry, signature)) = token.split_once('.') else {
        return false;
    };
    let Ok(expiry) = expiry.parse::<u64>() else {
        return false;
    };
    if expiry < now_secs() {
        return false;
    }
    let Some(signature) = unhex(signature) else {
        return false;
    };
    let expected = ring::hmac::sign(&cfg.key, expiry.to_string().as_bytes());
    constant_eq(expected.as_ref(), &signature)
}

/// 常量时间比较（长度不同直接返回 false——长度本身不是秘密）。
pub(crate) fn constant_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// 从请求头里取会话 Cookie。多个 Cookie 头都看一遍。
fn session_token(headers: &dioxus::server::axum::http::HeaderMap) -> Option<String> {
    for value in headers.get_all(header::COOKIE) {
        let Ok(value) = value.to_str() else {
            continue;
        };
        for pair in value.split(';') {
            let pair = pair.trim();
            if let Some(raw) = pair.strip_prefix(&format!("{COOKIE_NAME}=")) {
                if !raw.is_empty() {
                    return Some(raw.to_string());
                }
            }
        }
    }
    None
}

fn authenticated(cfg: &Config, headers: &dioxus::server::axum::http::HeaderMap) -> bool {
    session_token(headers)
        .map(|token| token_ok(cfg, &token))
        .unwrap_or(false)
}

/// 路径是否在某个前缀之下（要求按段匹配，`/assetsX` 不算 `/assets`）。
fn under(path: &str, prefix: &str) -> bool {
    path == prefix || path.starts_with(&format!("{prefix}/"))
}

/// 这些路径不经过密码：登录页本身、静态产物、书签小工具接口。
fn is_public(path: &str) -> bool {
    path == "/login"
        || path == "/logout"
        || path == "/favicon.ico"
        || path == "/favicon.svg"
        || path == "/robots.txt"
        || path == "/manifest.json"
        || path == "/manifest.webmanifest"
        || under(path, "/assets")
        || under(path, "/dist")
        || under(path, "/wasm")
        || under(path, "/_dioxus")
        || under(path, "/__dx")
        || path == "/api/bookmark"
}

/// 数据类入口：未登录时返回 401 而不是跳转，免得 fetch / img 拿到一段 HTML。
fn is_data_path(path: &str) -> bool {
    path.starts_with("/api/")
        || path.starts_with("/export/")
        || path.starts_with("/icon/")
        || path == "/wallpaper"
        || path.starts_with("/wallpaper/")
}

fn redirect(location: &str) -> Response {
    Response::builder()
        .status(StatusCode::SEE_OTHER)
        .header(header::LOCATION, location)
        .body(Body::empty())
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

fn set_cookie(token: &str, secure: bool) -> String {
    let secure = if secure { "; Secure" } else { "" };
    format!("{COOKIE_NAME}={token}; Path=/; Max-Age={SESSION_SECS}; HttpOnly; SameSite=Lax{secure}")
}

fn clear_cookie(secure: bool) -> String {
    let secure = if secure { "; Secure" } else { "" };
    format!("{COOKIE_NAME}=; Path=/; Max-Age=0; HttpOnly; SameSite=Lax{secure}")
}

fn is_https(headers: &dioxus::server::axum::http::HeaderMap) -> bool {
    // 反代可能写成 `https, http`（多跳）；只看第一跳。
    headers
        .get("x-forwarded-proto")
        .and_then(|value| value.to_str().ok())
        .map(|value| {
            value
                .split(',')
                .next()
                .unwrap_or("")
                .trim()
                .eq_ignore_ascii_case("https")
        })
        .unwrap_or(false)
}

fn sanitize_next(raw: &str) -> String {
    let raw = raw.trim();
    let loops_back = raw == "/login"
        || raw.starts_with("/login?")
        || raw == "/logout"
        || raw.starts_with("/logout?");
    if raw.starts_with('/')
        && !raw.starts_with("//")
        && !raw.contains('\\')
        && !raw.contains('{')
        && !raw.contains('}')
        && raw.is_ascii()
        && !raw.chars().any(|ch| ch.is_control())
        && !loops_back
    {
        raw.to_string()
    } else {
        "/".to_string()
    }
}

fn query_param(query: Option<&str>, name: &str) -> Option<String> {
    for pair in query?.split('&') {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        if key == name {
            return Some(form_decode(value, false));
        }
    }
    None
}

/// 解码 `application/x-www-form-urlencoded` 里的一个字段。
fn form_decode(raw: &str, plus_is_space: bool) -> String {
    let bytes = raw.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' if plus_is_space => out.push(b' '),
            b'%' if i + 2 < bytes.len() => {
                let high = (bytes[i + 1] as char).to_digit(16);
                let low = (bytes[i + 2] as char).to_digit(16);
                match (high, low) {
                    (Some(high), Some(low)) => {
                        out.push((high * 16 + low) as u8);
                        i += 2;
                    }
                    _ => out.push(b'%'),
                }
            }
            byte => out.push(byte),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn parse_form(body: &str) -> HashMap<String, String> {
    body.split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            (form_decode(key, true), form_decode(value, true))
        })
        .collect()
}

fn html_escape(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for ch in raw.chars() {
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

async fn site_title() -> String {
    // 读一次库；失败或没设就回落。登录页是纯 HTML，没有 SSR 可依赖。
    sqlx::query_scalar::<_, String>("SELECT value FROM settings WHERE key = 'site_title'")
        .fetch_optional(crate::db::pool())
        .await
        .ok()
        .flatten()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "NekoTab".to_string())
}

fn login_html(title: &str, next: &str, error: Option<&str>) -> String {
    let error = error
        .map(|message| format!(r#"<p class="error">{}</p>"#, html_escape(message)))
        .unwrap_or_default();
    LOGIN_HTML
        .replace("{{NEXT}}", &html_escape(next))
        .replace("{{ERROR}}", &error)
        // 标题最后替换：标题里若恰好含有 `{{NEXT}}` 之类的字样，不会再被当成占位符。
        .replace("{{TITLE}}", &html_escape(title))
}

const LOGIN_HTML: &str = r#"<!doctype html>
<html lang="zh"><head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<meta name="robots" content="noindex">
<title>登录 · {{TITLE}}</title>
<style>
:root{color-scheme:dark}
*{box-sizing:border-box}
body{margin:0;min-height:100vh;display:grid;place-items:center;
  background:radial-gradient(1200px 600px at 20% 0%,#1b2440,#0b0d13 60%);
  color:#e8ecf6;font:15px/1.6 system-ui,-apple-system,"Segoe UI",sans-serif;padding:24px}
.card{width:min(380px,100%);padding:32px 28px;border-radius:20px;
  background:rgba(22,26,38,.82);border:1px solid rgba(255,255,255,.09);
  box-shadow:0 24px 60px rgba(0,0,0,.45);backdrop-filter:blur(14px)}
h1{margin:0 0 6px;font-size:21px;letter-spacing:.02em}
p.sub{margin:0 0 22px;color:#94a0b8;font-size:13px}
label{display:block;margin:0 0 8px;font-size:12.5px;color:#aab4c8}
input{width:100%;height:44px;padding:0 14px;border-radius:12px;color:#eef2fb;
  background:#0f131d;border:1px solid #2a3244;outline:none;font-size:15px}
input:focus{border-color:#5b8cff;box-shadow:0 0 0 3px rgba(91,140,255,.22)}
button{width:100%;height:44px;margin-top:18px;border:0;border-radius:12px;cursor:pointer;
  background:linear-gradient(135deg,#5b8cff,#7c5cff);color:#fff;font-size:15px;font-weight:600}
button:active{transform:translateY(1px)}
.error{margin:0 0 14px;padding:9px 12px;border-radius:10px;font-size:13px;
  color:#ffb4b4;background:rgba(239,68,68,.14);border:1px solid rgba(239,68,68,.3)}
</style></head>
<body>
<form class="card" method="post" action="/login">
  <h1>{{TITLE}}</h1>
  <p class="sub">这个页面需要访问密码</p>
  {{ERROR}}
  <label for="password">密码</label>
  <input id="password" name="password" type="password" autocomplete="current-password" autofocus required>
  <input type="hidden" name="next" value="{{NEXT}}">
  <button type="submit">进入</button>
</form>
</body></html>"#;

fn failure_locked() -> bool {
    let guard = FAILURES.get_or_init(|| Mutex::new(Failures::default()));
    match guard.lock() {
        Ok(mut state) => {
            if let Some(until) = state.locked_until {
                if Instant::now() < until {
                    return true;
                }
                state.locked_until = None;
                state.count = 0;
            }
            false
        }
        Err(_) => false,
    }
}

fn record_failure() {
    let guard = FAILURES.get_or_init(|| Mutex::new(Failures::default()));
    if let Ok(mut state) = guard.lock() {
        state.count += 1;
        if state.count >= MAX_FAILS {
            state.locked_until = Some(Instant::now() + LOCK_FOR);
        }
    }
}

fn reset_failures() {
    let guard = FAILURES.get_or_init(|| Mutex::new(Failures::default()));
    if let Ok(mut state) = guard.lock() {
        state.count = 0;
        state.locked_until = None;
    }
}

/// `GET /login`。
pub async fn login_page(request: Request) -> Response {
    let Some(cfg) = config() else {
        return redirect("/");
    };
    let next = query_param(request.uri().query(), "next")
        .map(|value| sanitize_next(&value))
        .unwrap_or_else(|| "/".to_string());

    if authenticated(cfg, request.headers()) {
        return redirect(&next);
    }

    let error = match query_param(request.uri().query(), "error").as_deref() {
        Some("1") => Some("密码不对，再试一次"),
        Some("2") => Some("尝试次数过多，请一分钟后再试"),
        _ => None,
    };
    let title = site_title().await;
    // 必须显式声明 text/html：`String` 的默认响应类型是 text/plain，
    // 浏览器会直接把登录页当源码展示。
    (
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        login_html(&title, &next, error),
    )
        .into_response()
}

/// `POST /login`。
pub async fn login_submit(request: Request) -> Response {
    let Some(cfg) = config() else {
        return redirect("/");
    };

    let secure = is_https(request.headers());
    let (parts, body) = request.into_parts();
    let Ok(body) = dioxus::server::axum::body::to_bytes(body, 64 * 1024).await else {
        return redirect("/login?error=1");
    };
    let form = parse_form(&String::from_utf8_lossy(&body));
    let next = sanitize_next(form.get("next").map(String::as_str).unwrap_or("/"));
    let password = form.get("password").cloned().unwrap_or_default();

    let matches = constant_eq(password.as_bytes(), cfg.password.as_bytes());

    let _ = parts;
    if !matches {
        // 锁只拦"继续猜错"，不拦正确密码：否则匿名者连发 8 次错误就能
        // 把站长也锁在门外（全局计数器的 DoS）。
        if failure_locked() {
            return redirect(&format!(
                "/login?error=2&next={}",
                crate::engine::encode_query(&next)
            ));
        }
        record_failure();
        return redirect(&format!(
            "/login?error=1&next={}",
            crate::engine::encode_query(&next)
        ));
    }

    reset_failures();
    let token = make_token(cfg);
    Response::builder()
        .status(StatusCode::SEE_OTHER)
        .header(header::LOCATION, next)
        .header(header::SET_COOKIE, set_cookie(&token, secure))
        .body(Body::empty())
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

/// `GET /logout`：清 Cookie 回登录页。没启用密码时直接回首页。
pub async fn logout(request: Request) -> Response {
    let secure = is_https(request.headers());
    let target = if config().is_some() { "/login" } else { "/" };
    Response::builder()
        .status(StatusCode::SEE_OTHER)
        .header(header::LOCATION, target)
        .header(header::SET_COOKIE, clear_cookie(secure))
        .body(Body::empty())
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

/// 整站守卫：未启用密码时直接放行；启用时保护页面与数据入口。
pub async fn guard(request: Request, next: Next) -> Response {
    let Some(cfg) = config() else {
        return next.run(request).await;
    };

    let path = request.uri().path().to_string();

    if is_public(&path) || authenticated(cfg, request.headers()) {
        return next.run(request).await;
    }

    if is_data_path(&path) {
        return (StatusCode::UNAUTHORIZED, "请先登录").into_response();
    }

    let target = request
        .uri()
        .path_and_query()
        .map(|value| value.as_str())
        .unwrap_or("/");
    redirect(&format!(
        "/login?next={}",
        crate::engine::encode_query(&sanitize_next(target))
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_round_trip_and_expiry() {
        let key = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, b"secret");
        let cfg = Config {
            password: "pw".to_string(),
            key,
        };
        let token = make_token(&cfg);
        assert!(token_ok(&cfg, &token));
        // 改一个字节就失效
        let mut broken = token.clone();
        broken.pop();
        assert!(!token_ok(&cfg, &broken));
    }

    #[test]
    fn next_is_sanitized() {
        assert_eq!(sanitize_next("/abc?x=1"), "/abc?x=1");
        assert_eq!(sanitize_next("//evil.com"), "/");
        assert_eq!(sanitize_next("https://evil.com"), "/");
        assert_eq!(sanitize_next("/a\r\nb"), "/");
        assert_eq!(sanitize_next("/中文"), "/", "非 ASCII 不进 Location 头");
        assert_eq!(sanitize_next("/login?next=%2F"), "/", "不往回跳登录页");
        assert_eq!(sanitize_next("/logout"), "/", "不往回跳登出");
        // 浏览器把 `\` 当 `/`，`/\evil.com` 会变成 `//evil.com`
        assert_eq!(sanitize_next("/\\evil.com"), "/", "反斜杠不能绕过");
    }

    #[test]
    fn public_paths_are_segment_aware() {
        assert!(is_public("/assets/main.css"));
        assert!(is_public("/assets"));
        assert!(!is_public("/assetsX"));
        assert!(!is_public("/wasmX"));
        assert!(is_data_path("/wallpaper"));
        assert!(is_data_path("/wallpaper/x.png"));
        assert!(!is_data_path("/wallpaperX"));
    }

    #[test]
    fn form_round_trip() {
        let form = parse_form("password=a+b%26c&next=%2Fabc");
        assert_eq!(form.get("password").unwrap(), "a b&c");
        assert_eq!(form.get("next").unwrap(), "/abc");
    }
}
