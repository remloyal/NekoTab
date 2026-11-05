//! 从网页 HTML 的 head 里提取标题与描述。
//!
//! 新增书签时由服务端代取目标页面，把「名称 / 描述」预填好，省掉手打一遍。
//! 只解析 head 里的 `<title>` 与 `<meta>`，不引入 HTML 解析 crate：需要的语法面
//! 极窄（标签名 + 带引号的属性），一个几十行的扫描器就够，而且能脱离网络单测。
//!
//! 编码按 UTF-8 解（抓取那一层用 `from_utf8_lossy`）。仍在用 GBK 的老站点标题
//! 可能变成乱码——这是本项目不为一个非核心功能引入编码表的取舍；抓到乱码时
//! 用户改一下名字即可，不影响保存。

use crate::models::PageMeta;

/// 名称最多保留的字符数。再长就撑爆卡片标题，也不像"名字"了。
pub const TITLE_MAX_CHARS: usize = 120;

/// 描述最多保留的字符数。
pub const DESCRIPTION_MAX_CHARS: usize = 300;

/// 解析页面信息。`og:title` / `twitter:title` 优先于 `<title>`：
/// 后者常带「页面名 - 站点名」这类后缀，而前者通常就是页面自己想要的名字。
pub fn parse_page_meta(html: &str) -> PageMeta {
    let mut social_title = String::new();
    let mut meta_title = String::new();
    let mut description = String::new();

    for tag in meta_tags(html) {
        let attrs = tag_attrs(tag);
        let get = |name: &str| {
            attrs
                .iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.as_str())
                .unwrap_or("")
        };

        let name = get("name").to_ascii_lowercase();
        let property = get("property").to_ascii_lowercase();
        let content = get("content");
        if content.trim().is_empty() {
            continue;
        }

        if social_title.is_empty() && (property == "og:title" || property == "twitter:title") {
            social_title = clean_text(content, TITLE_MAX_CHARS);
        } else if meta_title.is_empty() && name == "title" {
            meta_title = clean_text(content, TITLE_MAX_CHARS);
        } else if description.is_empty()
            && (name == "description"
                || property == "og:description"
                || property == "twitter:description")
        {
            description = clean_text(content, DESCRIPTION_MAX_CHARS);
        }
    }

    let title = if !social_title.is_empty() {
        social_title
    } else if !meta_title.is_empty() {
        meta_title
    } else {
        title_of(html)
    };

    PageMeta { title, description }
}

/// 取 `<title>…</title>` 的内容。属性（`<title lang=…>`）一并跳过。
fn title_of(html: &str) -> String {
    let lower = html.to_ascii_lowercase();
    let Some(open) = lower.find("<title") else {
        return String::new();
    };
    let Some(gt) = html[open..].find('>') else {
        return String::new();
    };
    let start = open + gt + 1;
    let Some(end) = lower[start..].find("</title") else {
        return String::new();
    };

    clean_text(&html[start..start + end], TITLE_MAX_CHARS)
}

/// 逐个取出 `<meta …>` 标签的原文（尖括号内、`meta` 之后的部分）。
fn meta_tags(html: &str) -> Vec<&str> {
    // to_ascii_lowercase 只动 ASCII，字节长度不变，偏移量可直接沿用
    let lower = html.to_ascii_lowercase();
    let mut tags = Vec::new();
    let mut cursor = 0;

    while let Some(offset) = lower[cursor..].find("<meta") {
        let start = cursor + offset;
        let Some(end) = html[start..].find('>') else {
            break;
        };
        // 跳过 "<meta" 这 5 个 ASCII 字节
        tags.push(&html[start + 5..start + end]);
        cursor = start + end + 1;
    }

    tags
}

/// 解析标签里的属性：键统一小写，值保留原样。
///
/// 比 `transfer::attr_value` 多认"无值属性"与自闭合的 `/`，是给 head 里
/// `<meta>` 这类标签用的。多字节 UTF-8 不会与 ASCII 分隔符混淆，所以按字节
/// 推进、在分隔符处切片都是安全的。
fn tag_attrs(tag: &str) -> Vec<(String, String)> {
    let bytes = tag.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;

    // 跳过可能残留的标签名（`meta` 已在取值时跳过，这里兜住 `<meta>` 这类写法）
    while i < bytes.len() && !bytes[i].is_ascii_whitespace() && bytes[i] != b'>' {
        i += 1;
    }

    while i < bytes.len() {
        while i < bytes.len() && (bytes[i].is_ascii_whitespace() || bytes[i] == b'/') {
            i += 1;
        }
        if i >= bytes.len() || bytes[i] == b'>' {
            break;
        }

        let key_start = i;
        while i < bytes.len()
            && !bytes[i].is_ascii_whitespace()
            && bytes[i] != b'='
            && bytes[i] != b'>'
            && bytes[i] != b'/'
        {
            i += 1;
        }
        let key = tag[key_start..i].to_ascii_lowercase();

        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() || bytes[i] != b'=' {
            if !key.is_empty() {
                out.push((key, String::new()));
            }
            continue;
        }

        i += 1; // '='
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }

        let value = if bytes[i] == b'"' || bytes[i] == b'\'' {
            let quote = bytes[i];
            i += 1;
            let start = i;
            while i < bytes.len() && bytes[i] != quote {
                i += 1;
            }
            let value = tag[start..i].to_string();
            if i < bytes.len() {
                i += 1;
            }
            value
        } else {
            let start = i;
            while i < bytes.len() && !bytes[i].is_ascii_whitespace() && bytes[i] != b'>' {
                i += 1;
            }
            tag[start..i].to_string()
        };

        out.push((key, value));
    }

    out
}

/// 实体解码 + 折叠空白 + 按字符截断。
///
/// HTML 源码里的换行与缩进会原样进到标题里（`<title>\n  GitHub\n</title>`），
/// 不折叠的话卡片上会多出一堆空格。
fn clean_text(raw: &str, max_chars: usize) -> String {
    let decoded = crate::transfer::decode_entities(raw);
    let mut out = String::with_capacity(decoded.len());
    let mut pending_space = false;
    let mut count = 0;

    for ch in decoded.chars() {
        if ch.is_whitespace() {
            pending_space = !out.is_empty();
            continue;
        }
        if count >= max_chars {
            break;
        }
        if pending_space {
            out.push(' ');
            pending_space = false;
        }
        out.push(ch);
        count += 1;
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_title_and_collapses_whitespace() {
        let meta = parse_page_meta(
            "<html><head><title>\n  GitHub &amp; Friends\n  </title></head></html>",
        );
        assert_eq!(meta.title, "GitHub & Friends");
        assert_eq!(meta.description, "");
    }

    #[test]
    fn prefers_social_title_over_title_tag() {
        let html = r#"<head>
            <title>Rust 文档 - 站点名</title>
            <meta property="og:title" content="Rust 文档">
        </head>"#;
        assert_eq!(parse_page_meta(html).title, "Rust 文档");
    }

    #[test]
    fn falls_back_to_meta_name_title() {
        let html = r#"<head><meta name="title" content="来自 meta 的标题"></head>"#;
        assert_eq!(parse_page_meta(html).title, "来自 meta 的标题");
    }

    #[test]
    fn reads_description_from_name_or_og() {
        let html = r#"<head>
            <meta name="description" content="第一段描述">
            <meta property="og:description" content="第二段描述">
        </head>"#;
        // 先出现的那个定稿，后面的不覆盖
        assert_eq!(parse_page_meta(html).description, "第一段描述");
    }

    #[test]
    fn attribute_order_quotes_and_case_do_not_matter() {
        let html = "<HEAD><META CONTENT='大小写无关' PROPERTY='OG:TITLE'><TITLE>x</TITLE></HEAD>";
        let meta = parse_page_meta(html);
        assert_eq!(meta.title, "大小写无关");
    }

    #[test]
    fn ignores_empty_and_unrelated_meta() {
        let html = r#"<head>
            <meta name="viewport" content="width=device-width">
            <meta name="description" content="   ">
            <title></title>
        </head>"#;
        let meta = parse_page_meta(html);
        assert_eq!(meta.title, "");
        assert_eq!(meta.description, "");
    }

    #[test]
    fn missing_head_yields_empty_meta() {
        let meta = parse_page_meta("<html><body>没有 head</body></html>");
        assert_eq!(meta.title, "");
        assert_eq!(meta.description, "");
    }

    #[test]
    fn truncates_by_chars_not_bytes() {
        let long = "中".repeat(TITLE_MAX_CHARS + 20);
        let html = format!("<head><title>{long}</title></head>");
        let title = parse_page_meta(&html).title;
        assert_eq!(title.chars().count(), TITLE_MAX_CHARS);
        assert!(title.chars().all(|ch| ch == '中'));
    }

    #[test]
    fn content_containing_equals_does_not_break_attributes() {
        let html = r#"<head><meta name="description" content="key=value 与 a=b"></head>"#;
        assert_eq!(parse_page_meta(html).description, "key=value 与 a=b");
    }
}
