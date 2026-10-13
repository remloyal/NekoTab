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
/// **刻意不含域名**：`ls data/icon` 不该把收藏了哪些站点暴露出来。名字由
/// 「纳秒时间戳 + 进程内自增序号 + 主机名」混出的哈希生成，同一主机不会重复生成
/// （第一次的名字记在 `favicons.file_name` 里，之后命中缓存直接用）。
///
/// 扩展名按 MIME 推，只为翻目录时看得懂，不含任何站点信息。
///
/// 只在服务端落盘时用；客户端只请求 `/icon/{host}`，不关心文件怎么命名。
#[cfg_attr(not(feature = "server"), allow(dead_code))]
pub fn icon_file_name(host: &str, content_type: &str) -> String {
    format!("{}.{}", opaque_name(host), extension_for(content_type))
}

/// 生成一个不透明且不重复的文件名主体（32 位十六进制）。
///
/// 不引 `rand`：两个不同种子的 FNV-1a 混上时间戳、序号与主机名，对"文件名不撞车"
/// 这个量级的需求足够，而且输出定长、只含十六进制字符（文件系统安全）。
fn opaque_name(host: &str) -> String {
    use std::sync::atomic::{AtomicU64, Ordering};

    static SEQ: AtomicU64 = AtomicU64::new(0);

    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);

    let mut first: u64 = 0xcbf2_9ce4_8422_2325;
    let mut second: u64 = 0x9e37_79b9_7f4a_7c15;

    for byte in nanos
        .to_le_bytes()
        .iter()
        .chain(seq.to_le_bytes().iter())
        .chain(host.as_bytes())
    {
        first = (first ^ *byte as u64).wrapping_mul(0x0000_0100_0000_01b3);
        second = (second ^ (*byte as u64).wrapping_add(0x9e37_79b9)).wrapping_mul(0x0000_0100_0000_01b3);
    }

    format!("{first:016x}{second:016x}")
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

    /// 取不到时的负缓存有效期。
    ///
    /// 不要设得太长：负缓存的用意是"别对同一个取不到的站点反复重试"，
    /// 但一次**瞬时**失败（容器刚起、DNS 还没就绪、对方站点抖一下）会因此把
    /// 图标钉死很久，而界面上看起来就是"图标全挂了"。6 小时的话，真是瞬时问题
    /// 半天内自愈，而取不到的站点一小时也就多试一次，代价可以忽略。
    const MISS_TTL: &str = "-6 hours";

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
        let urls: Vec<String> = match sqlx::query_scalar("SELECT url FROM bookmarks")
            .fetch_all(pool)
            .await
        {
            Ok(urls) => urls,
            Err(e) => {
                // 这里不能吞掉错误：查库失败会让"所有主机都不在书签里"，
                // 于是所有图标一起 404，而日志里什么都不留，非常难查。
                eprintln!("[mtab] 取图标前读书签失败，本次请求无法服务: {e}");
                return not_found();
            }
        };

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

        let mut steps = Vec::new();
        let (content_type, data) = match fetch_icon(&host, scheme, &mut steps).await {
            Some(found) => found,
            None => {
                // 一行日志说明每一步为什么失败。以前这里是静默的，
                // 线上"图标全取不到"只能靠猜。
                eprintln!("[mtab] 取不到图标 {host}：{}", steps.join("；"));
                (String::new(), Vec::new())
            }
        };

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

    /// 清空全部图标缓存：库表记录 + 磁盘文件。返回清掉的条目数。
    ///
    /// 负缓存（取不到时记的"空"记录）在有效期里不会重试，所以修好网络之后
    /// 界面上仍会一直缺图标。给一个显式的"清掉重来"入口，比让人等 TTL
    /// 过期或者进容器删库友好得多。
    pub async fn clear_cache() -> usize {
        let pool = db::pool();

        let files: Vec<String> =
            sqlx::query_scalar("SELECT file_name FROM favicons WHERE length(file_name) > 0")
                .fetch_all(pool)
                .await
                .unwrap_or_default();
        let count = files.len();

        for name in &files {
            remove_icon(name).await;
        }

        // 表里没记到的孤儿文件（例如改了扩展名之后留下的）一并清掉：
        // 这个目录是本服务专属的缓存目录，里面的东西都可以丢。
        if let Ok(mut entries) = tokio::fs::read_dir(icon_dir()).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                let _ = tokio::fs::remove_file(entry.path()).await;
            }
        }

        let _ = sqlx::query("DELETE FROM favicons").execute(pool).await;

        count
    }

    /// 取图标：`/favicon.ico` → 首页声明 → `/apple-touch-icon.png`。
    ///
    /// 每一步失败的原因都记进 `steps`，最终取不到时由调用方打一行日志。
    /// 之前这里完全静默，线上出现"图标全取不到"时只能靠猜。
    async fn fetch_icon(
        host: &str,
        scheme: &str,
        steps: &mut Vec<String>,
    ) -> Option<(String, Vec<u8>)> {
        let ico = format!("{scheme}://{host}/favicon.ico");
        match fetch_image(&ico).await {
            Ok(found) => return Some(found),
            Err(why) => steps.push(format!("favicon.ico: {why}")),
        }

        // 首页声明的图标。很多站点 favicon.ico 是 HTML（软 404），只能靠这一条
        match fetch_html_head(&format!("{scheme}://{host}/")).await {
            Ok(html) => {
                let hrefs = icon_hrefs(&html);
                if hrefs.is_empty() {
                    steps.push("首页未声明任何图标".to_string());
                }
                for href in hrefs.into_iter().take(3) {
                    match resolve_icon_ref(scheme, host, &href) {
                        // 内联的 data: URI 不走网络，直接解出来
                        Some(IconRef::Inline(content_type, data)) => {
                            if !content_type.starts_with("image/") {
                                steps.push(format!("内联图标类型不是图片: {content_type}"));
                                continue;
                            }
                            if data.is_empty() || data.len() > MAX_ICON_BYTES {
                                steps.push(format!("内联图标体积异常: {} 字节", data.len()));
                                continue;
                            }
                            return Some((content_type, data));
                        }
                        Some(IconRef::Url(url)) => match fetch_image(&url).await {
                            Ok(found) => return Some(found),
                            Err(why) => steps.push(format!("{url}: {why}")),
                        },
                        None => steps.push(format!("无法解析图标地址: {}", truncate(&href, 40))),
                    }
                }
            }
            Err(why) => steps.push(format!("首页: {why}")),
        }

        // 约定路径兜底：不少站点只放 apple-touch-icon.png 而不在 head 里声明
        let apple = format!("{scheme}://{host}/apple-touch-icon.png");
        match fetch_image(&apple).await {
            Ok(found) => Some(found),
            Err(why) => {
                steps.push(format!("apple-touch-icon.png: {why}"));
                None
            }
        }
    }

    /// 抓一个 URL，只有确认是图片才返回；失败时给出可读原因。
    async fn fetch_image(url: &str) -> Result<(String, Vec<u8>), String> {
        let response = backend::http_client()
            .get(url)
            .header(reqwest::header::USER_AGENT, USER_AGENT)
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    "超时".to_string()
                } else if e.is_connect() {
                    "连不上".to_string()
                } else {
                    format!("请求失败: {e}")
                }
            })?;

        let status = response.status();
        if !status.is_success() {
            return Err(format!("HTTP {status}"));
        }

        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        if !content_type.starts_with("image/") {
            // 有站点对任意路径都返回 200 + text/html（实测 claude.ai），
            // 不校验就会把整个网页当成图标存下来
            return Err(format!("不是图片（{content_type}）"));
        }

        let data = response
            .bytes()
            .await
            .map_err(|e| format!("读取响应失败: {e}"))?
            .to_vec();
        if data.is_empty() {
            return Err("响应为空".to_string());
        }
        if data.len() > MAX_ICON_BYTES {
            return Err(format!("超过上限（{} 字节）", data.len()));
        }

        Ok((content_type, data))
    }

    /// 只读到 `</head>` 或上限为止，避免为了一个图标下载整个页面。
    async fn fetch_html_head(url: &str) -> Result<String, String> {
        let mut response = backend::http_client()
            .get(url)
            .header(reqwest::header::USER_AGENT, USER_AGENT)
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    "超时".to_string()
                } else if e.is_connect() {
                    "连不上".to_string()
                } else {
                    format!("请求失败: {e}")
                }
            })?;

        let status = response.status();
        if !status.is_success() {
            return Err(format!("HTTP {status}"));
        }

        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_ascii_lowercase();
        if !content_type.contains("html") {
            return Err(format!("不是 HTML（{content_type}）"));
        }

        let mut buf: Vec<u8> = Vec::new();
        while let Ok(Some(chunk)) = response.chunk().await {
            buf.extend_from_slice(&chunk);
            if buf.len() >= MAX_HTML_BYTES || find_bytes(&buf, b"</head").is_some() {
                break;
            }
        }

        Ok(String::from_utf8_lossy(&buf).into_owned())
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

    /// 图标声明解析后的形态。
    pub(super) enum IconRef {
        /// 一个可下载的绝对地址
        Url(String),
        /// 内联在页面里的 `data:` URI，直接带着类型与字节
        Inline(String, Vec<u8>),
    }

    /// 解析一条图标声明：可能是地址，也可能是内联的 `data:` URI。
    ///
    /// 内联图标很常见（现代站点与 SPA 模板常把 favicon 直接写进 head），
    /// 以前这里对 `data:` 一律返回 None，等于白丢了一类图标。
    pub(super) fn resolve_icon_ref(scheme: &str, host: &str, href: &str) -> Option<IconRef> {
        let href = href.trim();
        if let Some(rest) = href.strip_prefix("data:") {
            let (content_type, data) = decode_data_uri(rest)?;
            return Some(IconRef::Inline(content_type, data));
        }
        resolve_url(scheme, host, href).map(IconRef::Url)
    }

    /// 解析 `data:` URI：`[<mediatype>][;base64],<data>`。
    ///
    /// mediatype 取第一个 `;` 之前的部分；`base64` 只有作为 `;` 之后的分量才算标志
    /// （按 RFC 2397，`data:base64,…` 里的 `base64` 是 mediatype 而不是标志）。
    pub(super) fn decode_data_uri(rest: &str) -> Option<(String, Vec<u8>)> {
        let (meta, payload) = rest.split_once(',')?;

        let mut segments = meta.split(';');
        let media = segments.next().unwrap_or("").trim();
        let is_base64 = segments.any(|part| part.trim().eq_ignore_ascii_case("base64"));

        let content_type = if media.is_empty() {
            "text/plain".to_string()
        } else {
            media.to_ascii_lowercase()
        };

        let data = if is_base64 {
            decode_base64(payload)?
        } else {
            percent_decode(payload)
        };

        Some((content_type, data))
    }

    /// 标准 base64 解码；忽略空白与 `=` 填充。非法字符直接判失败。
    pub(super) fn decode_base64(input: &str) -> Option<Vec<u8>> {
        fn value(byte: u8) -> Option<u32> {
            match byte {
                b'A'..=b'Z' => Some((byte - b'A') as u32),
                b'a'..=b'z' => Some((byte - b'a') as u32 + 26),
                b'0'..=b'9' => Some((byte - b'0') as u32 + 52),
                b'+' => Some(62),
                b'/' => Some(63),
                _ => None,
            }
        }

        let mut out = Vec::with_capacity(input.len() / 4 * 3);
        let mut acc: u32 = 0;
        let mut bits: u32 = 0;

        for byte in input.bytes() {
            if byte == b'=' || byte.is_ascii_whitespace() {
                continue;
            }
            let v = value(byte)?;
            acc = (acc << 6) | v;
            bits += 6;
            if bits >= 8 {
                bits -= 8;
                out.push((acc >> bits) as u8);
            }
        }

        Some(out)
    }

    /// 百分号解码。非 base64 的 `data:` URI 用它，内联 SVG 常见于这种写法。
    pub(super) fn percent_decode(input: &str) -> Vec<u8> {
        let bytes = input.as_bytes();
        let mut out = Vec::with_capacity(bytes.len());
        let mut i = 0;

        while i < bytes.len() {
            if bytes[i] == b'%' && i + 2 < bytes.len() {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3])
                    .ok()
                    .and_then(|h| u8::from_str_radix(h, 16).ok());
                if let Some(decoded) = hex {
                    out.push(decoded);
                    i += 3;
                    continue;
                }
            }
            out.push(bytes[i]);
            i += 1;
        }

        out
    }

    /// 日志里截断过长的地址。
    fn truncate(text: &str, limit: usize) -> String {
        if text.chars().count() <= limit {
            text.to_string()
        } else {
            let mut out: String = text.chars().take(limit).collect();
            out.push('…');
            out
        }
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
        if href.is_empty() {
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
pub use server::{clear_cache, icon_handler, init_icon_dir};

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
    fn icon_file_name_hides_the_host() {
        // 文件名里不能出现域名（含端口形式）
        let name = icon_file_name("github.com", "image/png");
        assert!(!name.contains("github"), "文件名不该含域名：{name}");
        assert_eq!(name.rsplit('.').next(), Some("png"));

        let with_port = icon_file_name("localhost:3000", "image/x-icon");
        assert!(!with_port.contains("localhost"), "也不该含主机名：{with_port}");
        assert!(!with_port.contains(':'), "Windows 上冒号非法：{with_port}");
        assert_eq!(with_port.rsplit('.').next(), Some("ico"));

        // 只含十六进制与一个点，文件系统安全
        let stem = name.split('.').next().unwrap();
        assert_eq!(stem.len(), 32, "定长 32 位十六进制");
        assert!(stem.chars().all(|c| c.is_ascii_hexdigit()), "{stem}");
        assert!(!name.contains(['/', '\\']));

        // 同一主机连续两次生成不同名字（避免任何形式的可预测性）
        assert_ne!(icon_file_name("a.com", "image/png"), icon_file_name("a.com", "image/png"));

        // 扩展名仍按 MIME 推
        assert_eq!(icon_file_name("a.com", "image/svg+xml").rsplit('.').next(), Some("svg"));
        assert_eq!(icon_file_name("a.com", "image/vnd.microsoft.icon").rsplit('.').next(), Some("ico"));
        assert_eq!(icon_file_name("a.com", "image/png; charset=binary").rsplit('.').next(), Some("png"));
        assert_eq!(icon_file_name("a.com", "image/tiff").rsplit('.').next(), Some("img"));
    }

    #[cfg(feature = "server")]
    mod icon_ref {
        use crate::favicon::server::{
            decode_base64, decode_data_uri, percent_decode, resolve_icon_ref, IconRef,
        };

        #[test]
        fn decodes_base64() {
            // "Hello!" 的标准 base64
            assert_eq!(decode_base64("SGVsbG8h").unwrap(), b"Hello!");
            // 带填充与换行
            assert_eq!(decode_base64("SGVs\nbG8h").unwrap(), b"Hello!");
            assert_eq!(decode_base64("YQ==").unwrap(), b"a");
            assert_eq!(decode_base64("").unwrap(), b"");
            // 非法字符判失败，不静默产出错字节
            assert!(decode_base64("****").is_none());
        }

        #[test]
        fn decodes_percent_escapes() {
            assert_eq!(percent_decode("%3Csvg%3E"), b"<svg>");
            assert_eq!(percent_decode("a%20b"), b"a b");
            // 不完整的转义原样保留
            assert_eq!(percent_decode("100%"), b"100%");
            assert_eq!(percent_decode("%zz"), b"%zz");
        }

        #[test]
        fn parses_data_uris() {
            // base64 内联 PNG（1×1 透明像素）
            let png = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";
            let (ct, data) = decode_data_uri(png.strip_prefix("data:").unwrap()).unwrap();
            assert_eq!(ct, "image/png");
            assert_eq!(&data[..8], b"\x89PNG\r\n\x1a\n", "PNG magic");

            // 百分号编码的内联 SVG（没有 ;base64）
            let svg = "data:image/svg+xml,%3Csvg%20xmlns%3D%22x%22%3E%3C/svg%3E";
            let (ct, data) = decode_data_uri(svg.strip_prefix("data:").unwrap()).unwrap();
            assert_eq!(ct, "image/svg+xml");
            assert_eq!(String::from_utf8(data).unwrap(), r#"<svg xmlns="x"></svg>"#);

            // 省略 mediatype 时按 text/plain（`;base64` 前有个空分量）
            let (ct, data) = decode_data_uri(";base64,YQ==").unwrap();
            assert_eq!(ct, "text/plain");
            assert_eq!(data, b"a");

            // 缺逗号
            assert!(decode_data_uri("image/png;base64").is_none());
        }

        #[test]
        fn resolves_refs_with_data_uri_support() {
            // 普通地址照旧
            match resolve_icon_ref("https", "a.com", "/icon.png") {
                Some(IconRef::Url(u)) => assert_eq!(u, "https://a.com/icon.png"),
                _ => panic!("应解析成地址"),
            }
            match resolve_icon_ref("https", "a.com", "//cdn.a.com/i.png") {
                Some(IconRef::Url(u)) => assert_eq!(u, "https://cdn.a.com/i.png"),
                _ => panic!("协议相对地址应补 scheme"),
            }

            // 以前 data: 一律返回 None（白丢一类图标），现在应当解出来
            match resolve_icon_ref("https", "a.com", "data:image/svg+xml,%3Csvg%3E") {
                Some(IconRef::Inline(ct, data)) => {
                    assert_eq!(ct, "image/svg+xml");
                    assert_eq!(data, b"<svg>");
                }
                _ => panic!("data: URI 应被解成内联图标"),
            }

            assert!(resolve_icon_ref("https", "a.com", "").is_none());
        }
    }
}
