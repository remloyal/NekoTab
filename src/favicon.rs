//! 站点图标（favicon）。
//!
//! 由**服务端代取**，浏览器只请求本服务的 `/icon/{host}`：
//! 既避开跨域，也让图标缓存落在自己的磁盘上（而不是每次打开都去打扰对方站点）。
//!
//! 取图顺序：先试站点自带的 `/favicon.ico`，不是图片再抓首页 HTML 解析
//! `<link rel="icon">`。两道都必须校验 `Content-Type` 真的是图片——实测有站点
//! 对任意路径都返回 200 + `text/html`，不校验就会把整个网页当成图标存下来。
//!
//! 图标字节写在 `ICON_DIR`（默认 `data/icon`）下的独立文件里，`favicons` 表只留
//! 元数据：几十 KB 一张的图标塞进 SQLite 会让 mtab.db 迅速膨胀，备份、WAL 和
//! `VACUUM` 都跟着变重，而图标本来就是可重取的缓存。

/// 从 URL 里取出主机名（含端口，转小写）。
///
/// 结果既用来拼前端的 `/icon/{host}`，也用来拼服务端的出站地址，因此字符集
/// 卡得很严：只允许字母数字与 `. - : _`，天然排除 `/`、`?` 等路径穿越字符。
pub fn host_of(url: &str) -> Option<String> {
    let rest = url.trim();
    let rest = rest
        .strip_prefix("https://")
        .or_else(|| rest.strip_prefix("http://"))?;

    // 去掉 userinfo、路径、查询串与片段
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host = authority.rsplit('@').next().unwrap_or(authority);

    if host.is_empty() {
        return None;
    }

    let host = host.to_ascii_lowercase();
    is_valid_host(&host).then_some(host)
}

/// 取 URL 的 scheme，只认 http / https。仅服务端代取时需要。
#[cfg(feature = "server")]
pub fn scheme_of(url: &str) -> Option<&'static str> {
    let url = url.trim();
    if url.starts_with("https://") {
        Some("https")
    } else if url.starts_with("http://") {
        Some("http")
    } else {
        None
    }
}

pub fn is_valid_host(host: &str) -> bool {
    !host.is_empty()
        && host.len() <= 253
        && !host.starts_with('.')
        && !host.contains("..")
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':' | '_'))
}

/// 图标在磁盘上的文件名（不含目录）。
///
/// host 里唯一对文件系统不友好的字符是 `:`（端口分隔符，Windows 上是非法字符），
/// 换成 `_`。其余字符集在 [`is_valid_host`] 里已经卡过，不含路径分隔符。
///
/// 只在服务端落盘时用；客户端只请求 `/icon/{host}`，不关心文件怎么命名。
#[cfg_attr(not(feature = "server"), allow(dead_code))]
pub fn icon_file_name(host: &str, content_type: &str) -> String {
    let host = host.replace(':', "_");
    format!("{host}.{}", extension_for(content_type))
}

/// 由 MIME 推扩展名。只为让人直接翻 `data/icon` 时看得懂，
/// 服务端回响应时用的是表里存的 `content_type`。
#[cfg_attr(not(feature = "server"), allow(dead_code))]
fn extension_for(content_type: &str) -> &'static str {
    let base = content_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();

    match base.as_str() {
        "image/png" => "png",
        "image/jpeg" | "image/jpg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/svg+xml" => "svg",
        "image/avif" => "avif",
        "image/bmp" => "bmp",
        "image/x-icon" | "image/vnd.microsoft.icon" | "image/ico" => "ico",
        _ => "img",
    }
}

#[cfg(feature = "server")]
mod server {
    use std::path::PathBuf;
    use std::sync::OnceLock;

    use dioxus::server::axum::{
        body::Body,
        extract::Path as AxumPath,
        http::{header, StatusCode},
        response::Response,
    };

    use super::{host_of, icon_file_name, is_valid_host, scheme_of};
    use crate::{backend, db, transfer};

    const USER_AGENT: &str =
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
         (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";

    /// 图标字节上限，超过视为异常（避免把大图写进磁盘）
    const MAX_ICON_BYTES: usize = 512 * 1024;

    /// 解析图标声明时最多读取的 HTML 字节数
    const MAX_HTML_BYTES: usize = 256 * 1024;

    /// 取不到时的负缓存有效期（天）
    const MISS_TTL: &str = "-7 days";

    /// 图标目录，启动时解析一次（`ICON_DIR`，默认 `data/icon`）。
    static ICON_DIR: OnceLock<PathBuf> = OnceLock::new();

    /// 解析并创建图标目录。在 `main` 里于服务端启动时调用一次。
    pub fn init_icon_dir() -> std::io::Result<PathBuf> {
        let dir = std::env::var("ICON_DIR")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("data/icon"));

        std::fs::create_dir_all(&dir)?;
        // 已经初始化过（例如重复调用）就用最早那份
        let _ = ICON_DIR.set(dir);
        Ok(ICON_DIR.get().cloned().unwrap_or_default())
    }

    fn icon_dir() -> PathBuf {
        // 没初始化时（例如只跑单测）回落到默认目录，不 panic
        ICON_DIR
            .get()
            .cloned()
            .unwrap_or_else(|| PathBuf::from("data/icon"))
    }

    /// `GET /icon/{host}`：命中缓存直接返回，否则代取并落盘。
    pub async fn icon_handler(AxumPath(host): AxumPath<String>) -> Response {
        let host = host.to_ascii_lowercase();
        if !is_valid_host(&host) {
            return not_found();
        }

        let pool = db::pool();

        // 只服务"确实是本站已收藏的站点"。这既是正确性要求，也避免这个
        // 无鉴权接口变成任意出站探测（SSRF）的入口——要探测就得先加书签。
        // 回收站里的书签也算：还原后图标本该还在，没必要重取。
        let urls: Vec<String> = sqlx::query_scalar("SELECT url FROM bookmarks")
            .fetch_all(pool)
            .await
            .unwrap_or_default();

        let scheme = urls.iter().find_map(|url| {
            if host_of(url).as_deref() == Some(host.as_str()) {
                scheme_of(url)
            } else {
                None
            }
        });
        let Some(scheme) = scheme else {
            return not_found();
        };

        // 缓存：有文件名就是图标；文件名为空但是新记录，说明前阵子确认取不到
        let cached = sqlx::query_as::<_, (String, String)>(
            "SELECT content_type, file_name FROM favicons WHERE host = ? \
             AND (length(file_name) > 0 OR fetched_at > datetime('now', ?))",
        )
        .bind(&host)
        .bind(MISS_TTL)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten();

        if let Some((content_type, file_name)) = cached {
            if file_name.is_empty() {
                return not_found();
            }
            // 文件被手工清理掉时当作没命中，重新代取
            if let Some(data) = read_icon(&file_name).await {
                return image_response(content_type, data);
            }
        }

        let (content_type, data) = fetch_icon(&host, scheme).await.unwrap_or_default();

        let file_name = if data.is_empty() {
            String::new()
        } else {
            let name = icon_file_name(&host, &content_type);
            if write_icon(&name, &data).await.is_err() {
                // 写盘失败（磁盘满、权限不足）不影响这次响应：直接把字节发出去，
                // 下次请求再试。缓存没落地只是慢一点，不该变成 500。
                let _ = clear_cache_row(pool, &host).await;
                return image_response(content_type, data);
            }
            name
        };

        // 换了扩展名（例如从 .ico 换成 .png）时清掉旧文件，免得同一站点留两份
        let previous: Option<String> =
            sqlx::query_scalar("SELECT file_name FROM favicons WHERE host = ?")
                .bind(&host)
                .fetch_optional(pool)
                .await
                .ok()
                .flatten();
        if let Some(previous) = previous.filter(|old| !old.is_empty() && *old != file_name) {
            remove_icon(&previous).await;
        }

        let _ = sqlx::query(
            "INSERT INTO favicons (host, content_type, file_name, fetched_at) \
             VALUES (?, ?, ?, datetime('now')) \
             ON CONFLICT(host) DO UPDATE SET \
                content_type = excluded.content_type, \
                file_name = excluded.file_name, \
                fetched_at = excluded.fetched_at",
        )
        .bind(&host)
        .bind(&content_type)
        .bind(&file_name)
        .execute(pool)
        .await;

        if data.is_empty() {
            not_found()
        } else {
            image_response(content_type, data)
        }
    }

    async fn read_icon(file_name: &str) -> Option<Vec<u8>> {
        let path = icon_dir().join(file_name);
        tokio::task::spawn_blocking(move || std::fs::read(path).ok())
            .await
            .ok()
            .flatten()
    }

    async fn write_icon(file_name: &str, data: &[u8]) -> std::io::Result<()> {
        let path = icon_dir().join(file_name);
        let tmp = PathBuf::from(format!("{}.tmp", path.display()));
        let data = data.to_vec();

        // 先写临时文件再改名，避免半个文件被当成图标读走并缓存 7 天
        tokio::task::spawn_blocking(move || -> std::io::Result<()> {
            std::fs::write(&tmp, &data)?;
            std::fs::rename(&tmp, &path)
        })
        .await
        .map_err(|e| std::io::Error::other(e.to_string()))?
    }

    async fn remove_icon(file_name: &str) {
        let path = icon_dir().join(file_name);
        let _ = tokio::task::spawn_blocking(move || std::fs::remove_file(path)).await;
    }

    async fn clear_cache_row(pool: &sqlx::SqlitePool, host: &str) {
        let _ = sqlx::query("DELETE FROM favicons WHERE host = ?")
            .bind(host)
            .execute(pool)
            .await;
    }

    /// 取图标：`/favicon.ico` → 首页声明 → 无。
    async fn fetch_icon(host: &str, scheme: &str) -> Option<(String, Vec<u8>)> {
        if let Some(found) = fetch_image(&format!("{scheme}://{host}/favicon.ico")).await {
            return Some(found);
        }

        let html = fetch_html_head(&format!("{scheme}://{host}/")).await?;
        for href in icon_hrefs(&html).into_iter().take(3) {
            let Some(url) = resolve_url(scheme, host, &href) else {
                continue;
            };
            if let Some(found) = fetch_image(&url).await {
                return Some(found);
            }
        }

        None
    }

    /// 抓一个 URL，只有确认是图片才返回。
    async fn fetch_image(url: &str) -> Option<(String, Vec<u8>)> {
        let response = backend::http_client()
            .get(url)
            .header(reqwest::header::USER_AGENT, USER_AGENT)
            .send()
            .await
            .ok()?;

        if !response.status().is_success() {
            return None;
        }

        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        if !content_type.starts_with("image/") {
            return None;
        }

        let data = response.bytes().await.ok()?.to_vec();
        if data.is_empty() || data.len() > MAX_ICON_BYTES {
            return None;
        }

        Some((content_type, data))
    }

    /// 只读到 `</head>` 或上限为止，避免为了一个图标下载整个页面。
    async fn fetch_html_head(url: &str) -> Option<String> {
        let mut response = backend::http_client()
            .get(url)
            .header(reqwest::header::USER_AGENT, USER_AGENT)
            .send()
            .await
            .ok()?;

        if !response.status().is_success() {
            return None;
        }

        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_ascii_lowercase();
        if !content_type.contains("html") {
            return None;
        }

        let mut buf: Vec<u8> = Vec::new();
        while let Ok(Some(chunk)) = response.chunk().await {
            buf.extend_from_slice(&chunk);
            if buf.len() >= MAX_HTML_BYTES || find_bytes(&buf, b"</head").is_some() {
                break;
            }
        }

        Some(String::from_utf8_lossy(&buf).into_owned())
    }

    /// 找出所有声明了图标的 `<link>` 的 href，`apple-touch-icon` 排到最后。
    fn icon_hrefs(html: &str) -> Vec<String> {
        // to_ascii_lowercase 只动 ASCII，字节长度不变，偏移量可直接沿用
        let lower = html.to_ascii_lowercase();
        let mut hrefs: Vec<String> = Vec::new();
        let mut cursor = 0;

        while let Some(offset) = lower[cursor..].find("<link") {
            let start = cursor + offset;
            let Some(tag_end) = html[start..].find('>') else {
                break;
            };
            let tag = &html[start..start + tag_end];
            cursor = start + tag_end;

            let rel = transfer::attr_value(tag, "rel")
                .unwrap_or_default()
                .to_ascii_lowercase();
            if !rel.contains("icon") {
                continue;
            }

            if let Some(href) = transfer::attr_value(tag, "href") {
                let href = transfer::decode_entities(&href).trim().to_string();
                if !href.is_empty() {
                    hrefs.push(href);
                }
            }
        }

        hrefs.sort_by_key(|href| href.contains("apple-touch") as u8);
        hrefs
    }

    /// 把 href 补成绝对地址。
    fn resolve_url(scheme: &str, host: &str, href: &str) -> Option<String> {
        if href.starts_with("http://") || href.starts_with("https://") {
            return Some(href.to_string());
        }
        if let Some(rest) = href.strip_prefix("//") {
            return Some(format!("{scheme}://{rest}"));
        }
        if href.starts_with('/') {
            return Some(format!("{scheme}://{host}{href}"));
        }
        if href.is_empty() || href.starts_with("data:") {
            return None;
        }
        Some(format!("{scheme}://{host}/{href}"))
    }

    fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
        haystack
            .windows(needle.len())
            .position(|window| window == needle)
    }

    fn image_response(content_type: String, data: Vec<u8>) -> Response {
        Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, content_type)
            .header(header::CACHE_CONTROL, "public, max-age=604800")
            .body(Body::from(data))
            .unwrap_or_else(|_| not_found())
    }

    fn not_found() -> Response {
        Response::builder()
            .status(StatusCode::NOT_FOUND)
            .header(header::CACHE_CONTROL, "public, max-age=3600")
            .body(Body::empty())
            .expect("构造 404 响应")
    }
}

#[cfg(feature = "server")]
pub use server::{icon_handler, init_icon_dir};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_host() {
        assert_eq!(host_of("https://github.com/a/b?c=1#d").as_deref(), Some("github.com"));
        assert_eq!(host_of("http://Example.COM").as_deref(), Some("example.com"));
        assert_eq!(host_of("https://user:pw@example.com/x").as_deref(), Some("example.com"));
        assert_eq!(host_of("https://192.168.1.10:8080/x").as_deref(), Some("192.168.1.10:8080"));
        assert_eq!(host_of("javascript:alert(1)"), None);
        assert_eq!(host_of("https://"), None);
        assert_eq!(host_of("https://exa mple.com"), None);
    }

    #[test]
    fn rejects_traversal_shaped_hosts() {
        assert!(!is_valid_host("evil.com/../x"));
        assert!(!is_valid_host("a..b"));
        assert!(!is_valid_host(".hidden"));
        assert!(!is_valid_host(""));
        assert!(is_valid_host("sub.example.com"));
        assert!(is_valid_host("localhost:3000"));
    }

    #[test]
    fn reads_scheme() {
        assert_eq!(scheme_of("https://a.com"), Some("https"));
        assert_eq!(scheme_of("http://a.com"), Some("http"));
        assert_eq!(scheme_of("ftp://a.com"), None);
    }

    #[test]
    fn icon_file_name_is_filesystem_safe() {
        // 端口分隔符是 Windows 上的非法字符，必须换掉
        assert_eq!(icon_file_name("localhost:3000", "image/png"), "localhost_3000.png");
        assert_eq!(icon_file_name("github.com", "image/svg+xml"), "github.com.svg");
        assert_eq!(icon_file_name("a.com", "image/x-icon"), "a.com.ico");
        assert_eq!(icon_file_name("a.com", "image/vnd.microsoft.icon"), "a.com.ico");
        // 带参数的类型也要认出来
        assert_eq!(icon_file_name("a.com", "image/png; charset=binary"), "a.com.png");
        // 未知类型留个 .img，服务端回响应时用的是表里的 content_type
        assert_eq!(icon_file_name("a.com", "image/tiff"), "a.com.img");
        assert!(!icon_file_name("a.com", "image/png").contains(['/', '\\']));
    }
}
