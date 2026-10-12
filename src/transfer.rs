//! 书签的导入 / 导出。纯函数，不碰数据库，便于单测。
//!
//! 导入支持两种输入并自动识别：
//! - 浏览器导出的 **Netscape 书签 HTML**（文件夹路径映射为标签）；
//! - 本应用自己的 **JSON 备份**。
//!
//! 导出固定为 JSON：无损（含标签与顺序），且与解析端对称，能原样搬回。

use serde::{Deserialize, Serialize};

use crate::models::ImportedBookmark;

/// 自动识别输入格式。
pub fn parse_any(input: &str) -> Vec<ImportedBookmark> {
    let trimmed = input.trim_start();
    if trimmed.starts_with('[') || trimmed.starts_with('{') {
        parse_json(trimmed).unwrap_or_default()
    } else {
        parse_netscape(trimmed)
    }
}

/// 解析 JSON 备份，兼容裸数组与 `{ "bookmarks": [...] }` 两种形状。
pub fn parse_json(input: &str) -> Option<Vec<ImportedBookmark>> {
    #[derive(Deserialize)]
    struct Backup {
        bookmarks: Vec<ImportedBookmark>,
    }

    if let Ok(list) = serde_json::from_str::<Vec<ImportedBookmark>>(input) {
        return Some(list);
    }
    serde_json::from_str::<Backup>(input)
        .ok()
        .map(|backup| backup.bookmarks)
}

/// 序列化为 JSON 备份。
pub fn to_json(items: &[ImportedBookmark]) -> String {
    #[derive(Serialize)]
    struct Backup<'a> {
        bookmarks: &'a [ImportedBookmark],
    }

    serde_json::to_string_pretty(&Backup { bookmarks: items })
        .unwrap_or_else(|_| "{\"bookmarks\":[]}".to_string())
}

/// 解析浏览器导出的 Netscape 书签 HTML。
///
/// 只认三类东西：`<H3>` 文件夹名、`<A HREF>` 书签、`<DD>` 描述；
/// `<DL>` / `</DL>` 用于维护文件夹栈，从而把嵌套路径变成标签。
pub fn parse_netscape(html: &str) -> Vec<ImportedBookmark> {
    let mut out: Vec<ImportedBookmark> = Vec::new();
    let mut folders: Vec<String> = Vec::new();
    let mut pending_folder: Option<String> = None;
    let mut collecting_desc = false;
    let mut rest = html;

    while let Some(lt) = rest.find('<') {
        let text = &rest[..lt];

        // `<DD>` 之后的文字是上一条书签的描述
        if collecting_desc {
            let cleaned = decode_entities(text).trim().to_string();
            if !cleaned.is_empty() {
                if let Some(last) = out.last_mut() {
                    if last.description.is_empty() {
                        last.description = cleaned;
                    }
                }
            }
        }

        let after = &rest[lt + 1..];
        let Some(gt) = after.find('>') else { break };
        let tag = &after[..gt];
        rest = &after[gt + 1..];

        let (closing, name) = split_tag(tag);
        collecting_desc = false;

        if closing {
            if name.eq_ignore_ascii_case("dl") {
                folders.pop();
            }
            continue;
        }

        if name.eq_ignore_ascii_case("h3") {
            if let Some((start, len)) = find_close_tag(rest, "h3") {
                pending_folder = Some(decode_entities(&rest[..start]).trim().to_string());
                rest = &rest[start + len..];
            }
        } else if name.eq_ignore_ascii_case("a") {
            let href = attr_value(tag, "href").unwrap_or_default();

            if let Some((start, len)) = find_close_tag(rest, "a") {
                let title = decode_entities(&rest[..start]).trim().to_string();
                rest = &rest[start + len..];

                // 解析器只负责忠实提取；URL 是否可用由导入环节判定，
                // 这样"跳过 N 条"的口径才和源文件对得上。
                if !href.trim().is_empty() {
                    out.push(ImportedBookmark {
                        title: if title.is_empty() { href.clone() } else { title },
                        url: href,
                        description: String::new(),
                        tags: folders.join(","),
                    });
                }
            }
        } else if name.eq_ignore_ascii_case("dd") {
            collecting_desc = true;
        } else if name.eq_ignore_ascii_case("dl") {
            // 紧跟在 `<H3>` 之后的 `<DL>` 才是该文件夹的开始
            if let Some(folder) = pending_folder.take() {
                folders.push(folder);
            }
        }
    }

    out
}

/// 拆出标签名，并标记是否为闭合标签。`/DL` → `(true, "dl")`。
fn split_tag(tag: &str) -> (bool, String) {
    let trimmed = tag.trim();
    let (closing, body) = match trimmed.strip_prefix('/') {
        Some(rest) => (true, rest),
        None => (false, trimmed),
    };
    let end = body
        .find(|c: char| c.is_whitespace() || c == '/')
        .unwrap_or(body.len());
    (closing, body[..end].to_string())
}

/// 找到 `</name>`，返回 (起始位置, 总长度)。
fn find_close_tag(haystack: &str, name: &str) -> Option<(usize, usize)> {
    let bytes = haystack.as_bytes();
    let mut i = 0;

    while i + 1 < bytes.len() {
        if bytes[i] == b'<' && bytes[i + 1] == b'/' {
            let mut j = i + 2;
            while j < bytes.len() && bytes[j].is_ascii_alphanumeric() {
                j += 1;
            }
            let found = &haystack[i + 2..j];
            let mut k = j;
            while k < bytes.len() && bytes[k] != b'>' {
                k += 1;
            }
            if k >= bytes.len() {
                return None;
            }
            if found.eq_ignore_ascii_case(name) {
                return Some((i, k + 1 - i));
            }
            i = k + 1;
        } else {
            i += 1;
        }
    }

    None
}

/// 取属性值，支持单/双引号与无引号三种写法，属性名大小写不敏感。
///
/// 也让 favicon 模块复用来解析 `<link rel="icon" href="...">`。
pub(crate) fn attr_value(tag: &str, name: &str) -> Option<String> {
    // to_ascii_lowercase 只动 ASCII，字节长度不变，因此偏移量可直接沿用
    let lower = tag.to_ascii_lowercase();
    let key = format!("{}=", name.to_ascii_lowercase());
    let pos = lower.find(&key)?;

    let rest = tag[pos + key.len()..].trim_start();
    let quote = rest.chars().next()?;

    if quote == '"' || quote == '\'' {
        let body = &rest[1..];
        let end = body.find(quote)?;
        Some(body[..end].to_string())
    } else {
        let end = rest
            .find(char::is_whitespace)
            .unwrap_or(rest.len());
        Some(rest[..end].to_string())
    }
}

fn is_web_url(url: &str) -> bool {
    let url = url.trim().to_ascii_lowercase();
    url.starts_with("http://") || url.starts_with("https://")
}

/// 判定一条导入项是否可用：只接受 http(s)。与导入环节的口径保持一致。
pub fn is_usable_url(url: &str) -> bool {
    is_web_url(url)
}

/// 解码 HTML 实体：常见命名实体 + 十进制 / 十六进制数字实体。
pub(crate) fn decode_entities(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;

    while let Some(pos) = rest.find('&') {
        out.push_str(&rest[..pos]);
        let after = &rest[pos + 1..];

        match after.find(';').and_then(|semi| {
            decode_one(&after[..semi]).map(|ch| (ch, semi))
        }) {
            Some((ch, semi)) => {
                out.push(ch);
                rest = &after[semi + 1..];
            }
            None => {
                // 不是可识别实体，原样保留 `&`
                out.push('&');
                rest = after;
            }
        }
    }

    out.push_str(rest);
    out
}

fn decode_one(entity: &str) -> Option<char> {
    match entity {
        "amp" => return Some('&'),
        "lt" => return Some('<'),
        "gt" => return Some('>'),
        "quot" => return Some('"'),
        "apos" => return Some('\''),
        "nbsp" => return Some(' '),
        _ => {}
    }

    let code = entity.strip_prefix('#')?;
    let value = match code.strip_prefix(['x', 'X']) {
        Some(hex) => u32::from_str_radix(hex, 16).ok()?,
        None => code.parse::<u32>().ok()?,
    };
    char::from_u32(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"<!DOCTYPE NETSCAPE-Bookmark-file-1>
<META HTTP-EQUIV="Content-Type" CONTENT="text/html; charset=UTF-8">
<TITLE>Bookmarks</TITLE>
<H1>Bookmarks</H1>
<DL><p>
    <DT><A HREF="https://rust-lang.org" ADD_DATE="1700000000">Rust 官网</A>
    <DD>系统编程语言
    <DT><H3 ADD_DATE="1700000000">开发工具</H3>
    <DL><p>
        <DT><A HREF="https://github.com">GitHub</A>
        <DT><H3>前端</H3>
        <DL><p>
            <DT><A HREF="https://developer.mozilla.org">MDN &amp; Docs</A>
        </DL><p>
        <DT><A HREF="javascript:alert(1)">危险链接</A>
    </DL><p>
    <DT><A HREF="https://example.com/a b">无引号标题源</A>
</DL><p>
"#;

    #[test]
    fn parses_folders_into_tags() {
        let items = parse_netscape(SAMPLE);
        let github = items.iter().find(|i| i.url == "https://github.com").unwrap();
        assert_eq!(github.tags, "开发工具");
        assert_eq!(github.title, "GitHub");
    }

    #[test]
    fn nested_folders_join_with_comma() {
        let items = parse_netscape(SAMPLE);
        let mdn = items
            .iter()
            .find(|i| i.url == "https://developer.mozilla.org")
            .unwrap();
        assert_eq!(mdn.tags, "开发工具,前端");
    }

    #[test]
    fn decodes_entities() {
        let items = parse_netscape(SAMPLE);
        let mdn = items
            .iter()
            .find(|i| i.url == "https://developer.mozilla.org")
            .unwrap();
        assert_eq!(mdn.title, "MDN & Docs");
    }

    #[test]
    fn parser_keeps_non_web_urls_for_the_importer_to_judge() {
        // 解析器忠实提取，javascript: 也保留；是否可用由导入环节判定，
        // 这样"跳过 N 条"的口径才和源文件对得上。
        let items = parse_netscape(SAMPLE);
        assert!(items.iter().any(|i| i.url.starts_with("javascript:")));

        assert!(!is_usable_url("javascript:alert(1)"));
        assert!(!is_usable_url("place:sort=8"));
        assert!(is_usable_url("https://a.com"));
        assert!(is_usable_url("http://a.com"));
    }

    #[test]
    fn attaches_dd_description() {
        let items = parse_netscape(SAMPLE);
        let rust = items.iter().find(|i| i.url == "https://rust-lang.org").unwrap();
        assert_eq!(rust.description, "系统编程语言");
        // 根目录下的书签没有标签
        assert_eq!(rust.tags, "");
    }

    #[test]
    fn json_round_trip() {
        let items = parse_netscape(SAMPLE);
        let json = to_json(&items);
        let back = parse_json(&json).expect("解析自身备份");
        assert_eq!(items, back);
    }

    #[test]
    fn parse_any_dispatches() {
        // 以 `[` 开头走 JSON
        let via_json = parse_any(r#"[{"title":"A","url":"https://a.com","description":"","tags":"x"}]"#);
        assert_eq!(via_json.len(), 1);
        // 否则走 HTML
        let via_html = parse_any(SAMPLE);
        assert_eq!(via_html.len(), parse_netscape(SAMPLE).len());
    }

    #[test]
    fn attr_value_handles_quoting() {
        assert_eq!(attr_value("A HREF=\"https://a.com\"", "href").as_deref(), Some("https://a.com"));
        assert_eq!(attr_value("a href='https://b.com'", "HREF").as_deref(), Some("https://b.com"));
        assert_eq!(attr_value("A HREF=https://c.com X=1", "href").as_deref(), Some("https://c.com"));
        assert_eq!(attr_value("A TITLE=\"x\"", "href"), None);
    }
}
