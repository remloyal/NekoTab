//! 上传的壁纸。
//!
//! 与站点图标（`favicon.rs`）同一套路数：字节落盘，文件用不透明文件名（不含原始
//! 文件名，免得暴露用户本机信息或路径），目录由 `WALLPAPER_DIR` 指定。
//!
//! 壁纸是"用户自己的东西"，不是可再生的缓存，所以**没有**自动清理、也不属于
//! 「清空图标缓存」的范围；增删都由用户在设置里显式操作。
//!
//! 上传走一条普通 axum 路由（`POST /wallpaper`，请求体就是图片字节，类型由
//! `Content-Type` 给出）。没用 multipart：那要额外引入解析依赖，而这里只需要
//! 一个文件，直接发原始字节最省事，浏览器侧一个 `fetch(url, {body: file})` 即可。

use std::path::PathBuf;
use std::sync::OnceLock;

/// 单个壁纸的体积上限。壁纸是整屏大图，给得比图标宽裕得多。
pub const MAX_WALLPAPER_BYTES: usize = 16 * 1024 * 1024;

/// 支持的图片类型（`Content-Type` 前缀匹配到具体类型）。
const ALLOWED: [&str; 7] = [
    "image/png",
    "image/jpeg",
    "image/webp",
    "image/gif",
    "image/avif",
    "image/bmp",
    "image/svg+xml",
];

/// 由 MIME 推扩展名；不是支持的图片类型则返回 `None`。
pub fn extension_for(content_type: &str) -> Option<&'static str> {
    let base = content_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();

    match base.as_str() {
        "image/png" => Some("png"),
        "image/jpeg" | "image/jpg" => Some("jpg"),
        "image/webp" => Some("webp"),
        "image/gif" => Some("gif"),
        "image/avif" => Some("avif"),
        "image/bmp" => Some("bmp"),
        "image/svg+xml" => Some("svg"),
        _ => None,
    }
}

/// 该类型是否允许上传。
pub fn is_allowed(content_type: &str) -> bool {
    let base = content_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    ALLOWED.contains(&base.as_str())
}

/// 文件名是否安全（只允许我们自己生成的形状：十六进制 + 一个扩展名）。
///
/// 上传接口会把文件名拼进路径，必须挡住 `../` 之类；同时也不接受任意后缀，
/// 免得把目录里别的东西读出去。
pub fn is_safe_name(name: &str) -> bool {
    let Some((stem, ext)) = name.rsplit_once('.') else {
        return false;
    };
    !stem.is_empty()
        && stem.len() <= 64
        && stem.chars().all(|c| c.is_ascii_hexdigit())
        && matches!(ext, "png" | "jpg" | "webp" | "gif" | "avif" | "bmp" | "svg")
}

/// 生成不透明且不重复的文件名（16 位十六进制 + 扩展名）。
pub fn new_file_name(extension: &str) -> String {
    use std::sync::atomic::{AtomicU64, Ordering};

    static SEQ: AtomicU64 = AtomicU64::new(0);

    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);

    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in nanos
        .to_le_bytes()
        .iter()
        .chain(seq.to_le_bytes().iter())
        .chain(b"wallpaper")
    {
        hash = (hash ^ *byte as u64).wrapping_mul(0x0000_0100_0000_01b3);
    }

    format!("{hash:016x}.{extension}")
}

/// 壁纸目录，启动时解析一次。
static WALLPAPER_DIR: OnceLock<PathBuf> = OnceLock::new();

#[cfg(feature = "server")]
pub fn init_wallpaper_dir() -> std::io::Result<PathBuf> {
    let dir = std::env::var("WALLPAPER_DIR")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("data/wallpaper"));

    std::fs::create_dir_all(&dir)?;
    let _ = WALLPAPER_DIR.set(dir);
    Ok(WALLPAPER_DIR.get().cloned().unwrap_or_default())
}

#[cfg(feature = "server")]
pub fn wallpaper_dir() -> PathBuf {
    WALLPAPER_DIR
        .get()
        .cloned()
        .unwrap_or_else(|| PathBuf::from("data/wallpaper"))
}

#[cfg(feature = "server")]
pub fn path_of(name: &str) -> Option<PathBuf> {
    is_safe_name(name).then(|| wallpaper_dir().join(name))
}

#[cfg(feature = "server")]
mod server {
    use dioxus::server::axum::{
        body::{Body, Bytes},
        extract::Path as AxumPath,
        http::{header, HeaderMap, StatusCode},
        response::{IntoResponse, Response},
        Json,
    };

    use super::{extension_for, is_allowed, new_file_name, path_of, wallpaper_dir, MAX_WALLPAPER_BYTES};

    /// 抓外链图片时用的 UA。与 `favicon` 那处同一个理由：有些站点对没有 UA
    /// 的请求直接拒绝，而我们对它来说和浏览器没区别。
    const USER_AGENT: &str =
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
         (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";

    /// `GET /wallpaper` 的一项：文件名 + 体积。
    ///
    /// 体积是给面板显示"本机 5 张 · 62 MB"用的——壁纸不参与任何自动清理，
    /// 要不要删只能靠看得见占用来判断。
    #[derive(serde::Serialize)]
    pub struct WallpaperEntry {
        pub name: String,
        pub bytes: u64,
    }

    /// `GET /wallpaper`：列出已有壁纸（按文件名倒序，新的在前）。
    pub async fn list_handler() -> Response {
        let mut items: Vec<WallpaperEntry> = Vec::new();

        if let Ok(mut entries) = tokio::fs::read_dir(wallpaper_dir()).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                let Some(name) = entry.file_name().to_str().map(str::to_string) else {
                    continue;
                };
                if !super::is_safe_name(&name) {
                    continue;
                }
                // 取不到体积（正好被删、没权限）记 0：宁可显示"一张 0 字节"，
                // 也不要让它从列表里整条消失
                let bytes = entry.metadata().await.map(|meta| meta.len()).unwrap_or(0);
                items.push(WallpaperEntry { name, bytes });
            }
        }

        items.sort_by(|a, b| b.name.cmp(&a.name));

        Json(items).into_response()
    }

    /// 把一份图片字节落进壁纸目录，返回它的访问地址（`/wallpaper/<name>`）。
    ///
    /// 上传（字节来自请求体）与「存到本地」（字节来自别人的服务器）共用这一处：
    /// 两者的差别只在字节从哪来，命名、落盘、错误文案都该一致。
    async fn store(data: Vec<u8>, extension: &str) -> Result<String, String> {
        let name = new_file_name(extension);
        let path = path_of(&name).ok_or_else(|| "生成文件名失败".to_string())?;

        let write = tokio::task::spawn_blocking(move || std::fs::write(&path, &data)).await;
        if !matches!(write, Ok(Ok(()))) {
            return Err("写入壁纸失败".to_string());
        }

        Ok(format!("/wallpaper/{name}"))
    }

    /// 抓一张外链图片存到本机（背景池里的「存到本地」）。
    ///
    /// 与 `favicon` 那套代取同源：服务端去拿、带 UA、只有确认是图片才收。两处不同：
    ///   * 体积上限按壁纸算（16 MB），而且**边读边卡**——图标是"全读进内存再看大小"，
    ///     壁纸按那个写法，一个几百 MB 的响应就能把服务打爆；
    ///   * 失败要给出可读原因（超时 / 不是图片 / 超限 / HTTP 状态），它会被摆到界面上。
    pub async fn save_from_url(url: &str) -> Result<String, String> {
        let mut response = crate::backend::http_client()
            .get(url)
            .header(reqwest::header::USER_AGENT, USER_AGENT)
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    "请求超时".to_string()
                } else if e.is_connect() {
                    "连不上对方站点".to_string()
                } else {
                    format!("请求失败: {e}")
                }
            })?;

        let status = response.status();
        if !status.is_success() {
            return Err(format!("对方返回 HTTP {status}"));
        }

        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
            .to_string();
        if !content_type.starts_with("image/") {
            // 有站点对任意路径都返回 200 + text/html（图标那边实测过），
            // 不校验就会把一整个网页存成壁纸
            return Err(format!("不是图片（{content_type}）"));
        }

        let Some(extension) = extension_for(&content_type) else {
            return Err(format!("不支持的图片格式（{content_type}）"));
        };

        let mut data: Vec<u8> = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|e| format!("读取响应失败: {e}"))?
        {
            if data.len() + chunk.len() > MAX_WALLPAPER_BYTES {
                return Err("图片过大（上限 16 MB）".to_string());
            }
            data.extend_from_slice(&chunk);
        }

        if data.is_empty() {
            return Err("响应为空".to_string());
        }

        store(data, extension).await
    }

    /// `POST /wallpaper`：请求体是图片字节，`Content-Type` 给出类型。
    /// 返回新壁纸的访问地址。
    ///
    /// 用 `HeaderMap` 而不是 `Option<HeaderValue>`：axum 里后者不是合法的提取器
    /// （`Option<T>` 的提取器实现要求 `T: FromRequestParts`），`HeaderMap` 才是。
    pub async fn upload_handler(headers: HeaderMap, body: Bytes) -> Response {
        let content_type = headers
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
            .to_string();

        let Some(extension) = extension_for(&content_type).filter(|_| is_allowed(&content_type))
        else {
            return error(StatusCode::UNSUPPORTED_MEDIA_TYPE, "只接受常见图片格式（png / jpg / webp / gif / avif / bmp / svg）");
        };

        if body.is_empty() {
            return error(StatusCode::BAD_REQUEST, "请求体为空");
        }
        if body.len() > MAX_WALLPAPER_BYTES {
            return error(StatusCode::PAYLOAD_TOO_LARGE, "图片过大（上限 16 MB）");
        }

        match store(body.to_vec(), extension).await {
            Ok(url) => Json(serde_json::json!({ "url": url })).into_response(),
            Err(message) => error(StatusCode::INTERNAL_SERVER_ERROR, &message),
        }
    }

    /// `DELETE /wallpaper/{name}`：删除一张壁纸。
    pub async fn delete_handler(AxumPath(name): AxumPath<String>) -> Response {
        let Some(path) = path_of(&name) else {
            return error(StatusCode::BAD_REQUEST, "文件名不合法");
        };

        match tokio::task::spawn_blocking(move || std::fs::remove_file(path)).await {
            Ok(Ok(())) => StatusCode::NO_CONTENT.into_response(),
            Ok(Err(e)) if e.kind() == std::io::ErrorKind::NotFound => {
                error(StatusCode::NOT_FOUND, "壁纸不存在")
            }
            _ => error(StatusCode::INTERNAL_SERVER_ERROR, "删除壁纸失败"),
        }
    }

    /// `GET /wallpaper/{name}`：把壁纸发出去。
    pub async fn serve_handler(AxumPath(name): AxumPath<String>) -> Response {
        let Some(path) = path_of(&name) else {
            return error(StatusCode::BAD_REQUEST, "文件名不合法");
        };

        let read = tokio::task::spawn_blocking(move || std::fs::read(path)).await;
        let Ok(Ok(data)) = read else {
            return error(StatusCode::NOT_FOUND, "壁纸不存在");
        };

        let content_type = name
            .rsplit_once('.')
            .map(|(_, ext)| match ext {
                "png" => "image/png",
                "jpg" => "image/jpeg",
                "webp" => "image/webp",
                "gif" => "image/gif",
                "avif" => "image/avif",
                "bmp" => "image/bmp",
                _ => "image/svg+xml",
            })
            .unwrap_or("application/octet-stream");

        Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, content_type)
            // 文件名带随机成分、内容也不会变，可以长缓存
            .header(header::CACHE_CONTROL, "public, max-age=31536000, immutable")
            .body(Body::from(data))
            .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
    }

    fn error(status: StatusCode, message: &str) -> Response {
        (status, message.to_string()).into_response()
    }
}

#[cfg(feature = "server")]
pub use server::{delete_handler, list_handler, save_from_url, serve_handler, upload_handler};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_images() {
        assert!(is_allowed("image/png"));
        assert!(is_allowed("image/jpeg"));
        assert!(is_allowed("image/svg+xml; charset=utf-8"));
        assert!(!is_allowed("text/html"));
        assert!(!is_allowed("application/octet-stream"));
        assert!(!is_allowed(""));
    }

    #[test]
    fn maps_extension() {
        assert_eq!(extension_for("image/png"), Some("png"));
        assert_eq!(extension_for("image/jpeg"), Some("jpg"));
        assert_eq!(extension_for("image/webp; x=1"), Some("webp"));
        assert_eq!(extension_for("text/html"), None);
    }

    #[test]
    fn rejects_unsafe_names() {
        // 目录穿越与任意后缀都不接受
        assert!(!is_safe_name("../../etc/passwd"));
        assert!(!is_safe_name("a/../b.png"));
        assert!(!is_safe_name("照片.png"), "非十六进制不接受");
        assert!(!is_safe_name("abc.exe"));
        assert!(!is_safe_name("abc"));
        assert!(!is_safe_name(".png"));
        assert!(!is_safe_name(""));

        assert!(is_safe_name("0123456789abcdef.png"));
        assert!(is_safe_name("deadbeef.svg"));
    }

    #[test]
    fn generated_names_are_safe_and_unique() {
        let a = new_file_name("png");
        let b = new_file_name("png");
        assert!(is_safe_name(&a), "{a}");
        assert!(is_safe_name(&b), "{b}");
        assert_ne!(a, b, "两次生成不应撞名");
        assert!(a.ends_with(".png"));
    }
}
