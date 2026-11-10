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
use std::sync::atomic::{AtomicU64, Ordering};
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
    /// 会话版本号。登出时 +1，之前签发的 Cookie 全部作废。
    ///
    /// 签名里带着它，于是"这个会话是不是登出之前签的"这件事能验出来——
    /// 只清浏览器 Cookie 是不够的：Cookie 是无状态的，复制走的那一份在 30 天
    /// 内照样能用，复制来的会话没法靠清 Cookie 止损。
    ///
    /// 单用户部署里"退出登录"就意味着"所有设备上的会话一起作废"，不做
    /// 多设备会话表——多设备各自登出反而是书签页自己会遇到的常态。
    version: AtomicU64,
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

    // 会话版本号：登出时 +1，之前签发的 Cookie 全部作废。
    let version = persisted_session_version().await;

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
                // 取不到随机持久密钥，就只剩"用访问密码本身当 HMAC key"这一条路，
                // 而那正好把"拿到 Cookie 的人无法离线枚举密码"这条保证弄没了：
                // 拿到一个 Cookie 就能对候选密码逐个离线验签。
                // 与其悄悄降级，不如拒绝启动——部署的人显式设一个 SESSION_SECRET
                // 就行，总比上线一个能被离线爆破的密码保护强。
                eprintln!(
                    "[NekoTab] 无法读取或生成持久会话密钥，拒绝启动。\n\
                     [NekoTab]   请设置 SESSION_SECRET（一串随机字符）后重试；\n\
                     [NekoTab]   也可以暂时清空 ACCESS_PASSWORD 关掉整站密码。"
                );
                std::process::exit(1);
            }
        },
    };

    if CONFIG
        .set(Some(Config {
            password,
            key,
            version: AtomicU64::new(version),
        }))
        .is_err()
    {
        eprintln!("[NekoTab] 访问密码配置已初始化过，忽略本次环境变量");
        return;
    }
    println!("[NekoTab] 访问密码已启用（登录状态保持 30 天，密钥来源：{source}）");
}

/// 从 `settings` 表取会话版本号；没有就是 0。
///
/// 存进库是为了重启后仍然认得"这个会话是登出前签的"——只放在内存里的话，
/// 重启一次就把所有旧 Cookie 放回来了。
async fn persisted_session_version() -> u64 {
    let pool = crate::db::pool();

    let existing: Option<String> =
        sqlx::query_scalar("SELECT value FROM settings WHERE key = 'session_version'")
            .fetch_optional(pool)
            .await
            .ok()
            .flatten();
    if let Some(value) = existing.as_deref().and_then(|v| v.trim().parse::<u64>().ok()) {
        return value;
    }

    let _ = sqlx::query("INSERT OR IGNORE INTO settings (key, value) VALUES ('session_version', '0')")
        .execute(pool)
        .await;

    sqlx::query_scalar("SELECT value FROM settings WHERE key = 'session_version'")
        .fetch_optional(pool)
        .await
        .ok()
        .flatten()
        .and_then(|value: String| value.trim().parse::<u64>().ok())
        .unwrap_or(0)
}

/// 从 `settings` 表取会话密钥；没有就生成 32 字节随机数存进去。
///
/// 用 SQLite 的 `randomblob` 而不是再引一个随机数 crate。`INSERT OR IGNORE`
/// 兜住并发启动：两个进程同时生成时，后写的那个会被忽略，再读一次拿到先写的。
///
/// 读不到就返回 `None`，由调用方拒绝启动。这里重试几次：刚启动时数据库文件
/// 可能正被另一个进程占着，一次读失败就拒绝启动太脆了。
async fn persisted_session_secret() -> Option<String> {
    for attempt in 0..3 {
        match load_or_create_session_secret().await {
            Some(secret) => return Some(secret),
            None if attempt < 2 => {
                tokio::time::sleep(Duration::from_millis(300)).await;
            }
            None => {}
        }
    }
    None
}

async fn load_or_create_session_secret() -> Option<String> {
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

/// 被签名的内容：会话版本 + 过期时间。
///
/// 版本进签名，于是"这个会话是不是登出之前签的"这件事能被验出来。
fn token_payload(version: u64, expiry: u64) -> String {
    format!("{version}.{expiry}")
}

fn make_token(cfg: &Config) -> String {
    let version = cfg.version.load(Ordering::Relaxed);
    let expiry = now_secs() + SESSION_SECS;
    let signature = ring::hmac::sign(&cfg.key, token_payload(version, expiry).as_bytes());
    format!("{version}.{expiry}.{}", hex(signature.as_ref()))
}

fn token_ok(cfg: &Config, token: &str) -> bool {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return false;
    }
    let Some(version) = parts[0].parse::<u64>().ok() else {
        return false;
    };
    let Some(expiry) = parts[1].parse::<u64>().ok() else {
        return false;
    };
    let signature = parts[2];
    // 登出后版本号变了 → 之前签的会话一律作废
    if version != cfg.version.load(Ordering::Relaxed) {
        return false;
    }
    if expiry < now_secs() {
        return false;
    }
    let Some(signature) = unhex(signature) else {
        return false;
    };
    let expected = ring::hmac::sign(&cfg.key, token_payload(version, expiry).as_bytes());
    constant_eq(expected.as_ref(), &signature)
}

/// 登出：会话版本 +1，之前签发的 Cookie 全部作废。
///
/// 内存里的版本号总是先加（哪怕写库失败也加）：宁可"这一次进程里旧会话立刻
/// 失效"，也不要"登出看起来成功了、旧 Cookie 却还能用"。写库失败只影响重启
/// 之后——那种情况下会打一行日志提示。
async fn bump_session_version(cfg: &Config) {
    let next = cfg.version.fetch_add(1, Ordering::SeqCst) + 1;
    let pool = crate::db::pool();
    let result = sqlx::query(
        "INSERT INTO settings (key, value) VALUES ('session_version', ?1) \
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(next.to_string())
    .execute(pool)
    .await;
    if result.is_err() {
        eprintln!("[NekoTab] 会话版本写库失败，重启后旧登录状态可能恢复");
    }
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

/// 把请求路径规整成逐段的形式：折叠重复斜杠、去掉 `.`、解掉 `..`。
///
/// 守卫按路径判定"要不要鉴权"，所以判定用的路径必须和路由匹配时看到的是同一个
/// 东西。否则 `/assets/../api/xxx` 会被当成"静态产物前缀下的公开路径"放行，
/// 而路由那边解完 `..` 命中的却是需要鉴权的接口。
///
/// 越界的 `..`（如 `/../etc/passwd`）解到根就停住，不会跑到根之外去。
fn normalize_path(path: &str) -> String {
    let mut segments: Vec<&str> = Vec::new();
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop();
            }
            other => segments.push(other),
        }
    }
    if segments.is_empty() {
        "/".to_string()
    } else {
        format!("/{}", segments.join("/"))
    }
}

/// 这些路径不经过密码：登录页本身、静态产物、书签小工具接口。
///
/// 前提是**落到这里的路径只可能被静态路由接住**。Dioxus 的 SSR 兜底会把任何
/// 没被路由命中的路径渲染成整页，而整页里带着全部书签的数据——所以一个"其实
/// 没有对应文件"的前缀一旦被放行，就等于把整页送出去（`/favicon.ico` 就曾经
/// 是这样的一个洞：它不在 public 目录里，于是落到 SSR 兜底上，浏览器每个
/// 页面都会自动去请求它）。
///
/// main.rs 里那条 `/{*path}` 的 404 兜底就是为此存在的：前缀下的文件不存在时
/// 落到它上面（404），而不是落到 SSR 兜底上（整页）。守卫这一层因此只管
/// "要不要鉴权"，不管"有没有这个文件"。
fn is_public(path: &str) -> bool {
    path == "/login"
        || path == "/logout"
        || path == "/favicon.ico"
        || path == "/favicon.svg"
        || path == "/robots.txt"
        || path == "/manifest.json"
        || path == "/manifest.webmanifest"
        || under(path, "/assets")
        || under(path, "/wasm")
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

/// 记一次失败，返回"这次尝试是否应该被挡下"。
///
/// 检查与计数在**同一个临界区**里做完：分成"先查后记"两次加锁的话，两次之间
/// 留着窗口，那个窗口里并发的请求会各自看到"还没锁"，限流就能被并发绕过。
///
/// 锁中毒（某个线程持锁时 panic 了）不当成"没有锁"——那会让一次 panic 把限流
/// 永久废掉。取回中毒的锁继续用：计数器的数据可能不完整，但限流本身还在。
fn register_failure() -> bool {
    let guard = FAILURES.get_or_init(|| Mutex::new(Failures::default()));
    let mut state = guard.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let now = Instant::now();

    if let Some(until) = state.locked_until {
        if now < until {
            return true;
        }
        // 锁过期，重新计数
        state.locked_until = None;
        state.count = 0;
    }

    state.count += 1;
    if state.count >= MAX_FAILS {
        state.locked_until = Some(now + LOCK_FOR);
        return true;
    }
    false
}

fn reset_failures() {
    let guard = FAILURES.get_or_init(|| Mutex::new(Failures::default()));
    let mut state = guard.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    state.count = 0;
    state.locked_until = None;
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
        let locked = register_failure();
        let error = if locked { 2 } else { 1 };
        return redirect(&format!(
            "/login?error={error}&next={}",
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

/// `GET /logout`：作废当前会话并清 Cookie，回登录页。没启用密码时直接回首页。
///
/// 只清 Cookie 是不够的：Cookie 是无状态的签名串，复制走的那一份在 30 天内
/// 照样能用，"退出登录"对它不起作用。所以这里把**会话版本 +1**——版本进了
/// 签名，之前签发的会话（不管在哪个设备上）全部作废。
pub async fn logout(request: Request) -> Response {
    let secure = is_https(request.headers());
    let target = if config().is_some() { "/login" } else { "/" };
    if let Some(cfg) = config() {
        bump_session_version(cfg).await;
    }
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

    // 判定必须用在规整过的路径上：否则 `/assets/../api/xxx` 会被当成
    // "静态产物前缀下的公开路径"放行，而路由那边解完 `..` 命中的却是
    // 需要鉴权的接口。
    let path = normalize_path(request.uri().path());

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
            version: AtomicU64::new(0),
        };
        let token = make_token(&cfg);
        assert!(token_ok(&cfg, &token));
        // 改一个字节就失效
        let mut broken = token.clone();
        broken.pop();
        assert!(!token_ok(&cfg, &broken));
    }

    #[test]
    fn logout_invalidates_issued_tokens() {
        let key = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, b"secret");
        let cfg = Config {
            password: "pw".to_string(),
            key,
            version: AtomicU64::new(3),
        };
        let token = make_token(&cfg);
        assert!(token_ok(&cfg, &token));
        // 登出：版本 +1 之后，之前签发的会话一律作废
        cfg.version.fetch_add(1, Ordering::SeqCst);
        assert!(!token_ok(&cfg, &token), "登出前的 Cookie 必须失效");
        // 但新签发的还能用
        let fresh = make_token(&cfg);
        assert!(token_ok(&cfg, &fresh));
    }

    #[test]
    fn paths_are_normalized_before_the_public_check() {
        // 静态产物前缀 + `..` 不能被当成公开路径放行
        assert_eq!(normalize_path("/assets/../api/x"), "/api/x");
        assert_eq!(normalize_path("/assets//main.css"), "/assets/main.css");
        assert_eq!(normalize_path("/./assets/main.css"), "/assets/main.css");
        assert_eq!(normalize_path("/"), "/");
        assert_eq!(normalize_path("/../.."), "/");
        assert!(!is_public(&normalize_path("/assets/../api/x")));
        assert!(is_public(&normalize_path("/assets/main.css")));
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
