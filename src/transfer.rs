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

/// 一条待导出的书签。
#[derive(Debug, Clone, PartialEq)]
pub struct ExportRow {
    pub title: String,
    pub url: String,
    pub description: String,
    /// 分组名；空串表示未分组
    pub group: String,
}

/// 导出为 Netscape 书签 HTML —— 也就是浏览器能直接导入的那种格式。
///
/// 与导入端对称：分组写成文件夹（`<H3>` + 嵌套 `<DL>`），未分组的放在根层。
/// 有了它数据就不只进不出——想搬回浏览器、或换个导航页，拿这个文件就行。
pub fn to_netscape(rows: &[ExportRow]) -> String {
    // 分组按首次出现的顺序输出，和界面上的标签栏顺序一致
    let mut order: Vec<&str> = Vec::new();
    for row in rows {
        let group = row.group.as_str();
        if !group.is_empty() && !order.contains(&group) {
            order.push(group);
        }
    }

    let mut out = String::from(concat!(
        "<!DOCTYPE NETSCAPE-Bookmark-file-1>\n",
        "<META HTTP-EQUIV=\"Content-Type\" CONTENT=\"text/html; charset=UTF-8\">\n",
        "<TITLE>NekoTab 书签</TITLE>\n",
        "<H1>NekoTab 书签</H1>\n",
        "<DL><p>\n",
    ));

    for group in &order {
        out.push_str(&format!(
            "    <DT><H3>{}</H3>\n    <DL><p>\n",
            encode_entities(group)
        ));
        for row in rows.iter().filter(|row| row.group == *group) {
            out.push_str(&bookmark_line(row, "        "));
        }
        out.push_str("    </DL><p>\n");
    }

    for row in rows.iter().filter(|row| row.group.is_empty()) {
        out.push_str(&bookmark_line(row, "    "));
    }

    out.push_str("</DL><p>\n");
    out
}

/// 一条书签的 HTML（`<DT><A HREF>` + 可选的 `<DD>` 描述）。
fn bookmark_line(row: &ExportRow, indent: &str) -> String {
    let mut line = format!(
        "{indent}<DT><A HREF=\"{}\">{}</A>\n",
        encode_entities(&row.url),
        encode_entities(&row.title)
    );
    if !row.description.trim().is_empty() {
        line.push_str(&format!(
            "{indent}<DD>{}\n",
            encode_entities(row.description.trim())
        ));
    }
    line
}

/// HTML 实体编码。只处理会破坏结构的那几个字符。
fn encode_entities(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(ch),
        }
    }
    out
}

/// 浏览器导出时把所有书签包在一个容器文件夹里，它只表示"放在哪个容器"，
/// 不表达用户的分类意图。导入时跳过它，否则整个标签栏只剩一个「书签栏」。
const CONTAINER_FOLDERS: [&str; 16] = [
    // 中文浏览器
    "书签栏",
    "收藏夹栏",
    "收藏夹",
    "书签菜单",
    "其他书签",
    "其它书签",
    "移动设备书签",
    "手机书签",
    // 英文浏览器
    "bookmarks bar",
    "bookmarks toolbar",
    "favorites bar",
    "bookmarks menu",
    "other bookmarks",
    "favorites",
    "mobile bookmarks",
    "bookmarks",
];

/// 该文件夹名是不是浏览器自带的容器（不表达用户的分类意图）。
fn is_container_folder(folder: &str) -> bool {
    CONTAINER_FOLDERS
        .iter()
        .any(|container| folder.eq_ignore_ascii_case(container))
}

/// 由文件夹路径推出分组名。
///
/// 取**第一个非容器文件夹**：那才是用户真正的顶层分类（用整条路径当分组名的话，
/// 层层嵌套会拼出一堆长名字；用最内层的话，「工作/项目A」和「工作/项目B」会
/// 变成两个平级分组、看不出共同的上层）。更深层级的路径仍然写进标签，
/// 所以信息没有丢——搜索照样能命中。
fn group_from_folders(folders: &[String]) -> String {
    folders
        .iter()
        .find(|folder| !is_container_folder(folder))
        .cloned()
        .unwrap_or_default()
}

/// 由文件夹路径推出标签。
///
/// 容器文件夹要剔掉：它在分组那边本来就被跳过了，留着当标签只会凭空多出一堆
/// 「书签栏」——而这一条标签对检索毫无用处（它不代表任何内容），却让标签栏
/// 看起来像第二排分组。
fn tags_from_folders(folders: &[String]) -> String {
    folders
        .iter()
        .filter(|folder| !is_container_folder(folder))
        .cloned()
        .collect::<Vec<_>>()
        .join(",")
}

/// 解析浏览器导出的 Netscape 书签 HTML。
///
/// 只认三类东西：`<H3>` 文件夹名、`<A HREF>` 书签、`<DD>` 描述；
/// `<DL>` / `</DL>` 用于维护文件夹栈，从而把嵌套路径变成标签与分组。
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
            // href 也要解码实体：浏览器导出时属性值是 HTML 转义过的，
            // URL 里的 & 会写成 &amp;，不解码就会把 &amp; 原样存进库。
            let href = decode_entities(&attr_value(tag, "href").unwrap_or_default());

            // Firefox 一类浏览器会把**已缓存的站点图标**内联在 ICON 属性里
            // （`ICON="data:image/png;base64,…"`）。这往往是内网站点唯一能拿到
            // 图标的来源——服务端代取那些地址是取不到的。这里只原样取出来，
            // 解码与落盘交给导入环节（它才拿得到数据库与图标目录）。
            let inline_icon = attr_value(tag, "icon")
                .map(|value| decode_entities(&value))
                .map(|value| value.trim().to_string())
                .filter(|value| value.starts_with("data:image/"));

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
                        // 完整路径进标签（检索用），首层分类进分组（归属用）
                        tags: tags_from_folders(&folders),
                        group: group_from_folders(&folders),
                        icon: String::new(),
                        // Netscape 格式没有"跨度"这个概念，导入即 1×1
                        span_w: 1,
                        span_h: 1,
                        inline_icon,
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

    /// URL 里的 `&` 在浏览器导出文件里是 `&amp;`，必须解码回来。
    #[test]
    fn decodes_entities_in_href() {
        let items = parse_netscape(
            r#"<DL><p><DT><A HREF="https://a.com/s?q=1&amp;lang=zh">带查询串</A></DL><p>"#,
        );
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].url, "https://a.com/s?q=1&lang=zh");
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

    /// 导出的 HTML 必须能被自己的解析器读回来（也就是浏览器也能导入）。
    #[test]
    fn netscape_export_round_trips() {
        let rows = vec![
            ExportRow {
                title: "GitHub & Co".to_string(),
                url: "https://github.com".to_string(),
                description: "代码托管".to_string(),
                group: "开发".to_string(),
            },
            ExportRow {
                title: "散装".to_string(),
                url: "https://a.com/b?c=1&d=2".to_string(),
                description: String::new(),
                group: String::new(),
            },
        ];

        let html = to_netscape(&rows);
        // 头部不该带前导空白（续行写法容易把缩进带进去）
        assert!(html.starts_with("<!DOCTYPE NETSCAPE-Bookmark-file-1>
<META"));

        let back = parse_netscape(&html);
        assert_eq!(back.len(), rows.len());

        let github = back.iter().find(|i| i.url == "https://github.com").unwrap();
        assert_eq!(github.title, "GitHub & Co", "实体编码/解码要对称");
        assert_eq!(github.group, "开发", "分组要还原成文件夹");
        assert_eq!(github.description, "代码托管");

        let loose = back.iter().find(|i| i.url.starts_with("https://a.com")).unwrap();
        assert_eq!(loose.url, "https://a.com/b?c=1&d=2", "URL 里的 & 要还原");
        assert_eq!(loose.group, "", "未分组的仍在根层");
    }

    #[test]
    fn json_round_trip() {
        let items = parse_netscape(SAMPLE);
        let json = to_json(&items);
        let back = parse_json(&json).expect("解析自身备份");
        assert_eq!(items, back);
    }

    #[test]
    fn json_round_trip_keeps_groups() {
        let items = vec![ImportedBookmark {
            title: "GitHub".to_string(),
            url: "https://github.com".to_string(),
            description: "代码托管".to_string(),
            tags: "dev".to_string(),
            group: "开发".to_string(),
            icon: "i:code".to_string(),
            span_w: 2,
            span_h: 2,
            inline_icon: None,
        }];

        let back = parse_json(&to_json(&items)).expect("解析自身备份");
        assert_eq!(back, items);
        assert_eq!(back[0].group, "开发");
        assert_eq!((back[0].span_w, back[0].span_h), (2, 2), "跨度要进备份");
    }

    #[test]
    fn missing_group_field_defaults_to_empty() {
        // 旧版本的备份没有 group 字段，必须仍能导入
        let back = parse_json(r#"[{"title":"A","url":"https://a.com"}]"#).expect("解析旧备份");
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].group, "");
    }

    /// 浏览器 HTML 的文件夹现在也用来定分组：取第一个非容器文件夹。
    #[test]
    fn netscape_folders_become_groups() {
        let items = parse_netscape(SAMPLE);

        let github = items.iter().find(|i| i.url == "https://github.com").unwrap();
        assert_eq!(github.group, "开发工具");
        assert_eq!(github.tags, "开发工具", "标签仍是完整路径");

        // 嵌套：分组取首层，深层留在标签里
        let mdn = items
            .iter()
            .find(|i| i.url == "https://developer.mozilla.org")
            .unwrap();
        assert_eq!(mdn.group, "开发工具", "分组只取顶层，不拼整条路径");
        assert_eq!(mdn.tags, "开发工具,前端", "深层路径不丢，进标签");

        // 导出根目录下的书签没有分组
        let rust = items.iter().find(|i| i.url == "https://rust-lang.org").unwrap();
        assert_eq!(rust.group, "", "根目录下的书签归未分组");
    }

    /// 浏览器会把所有书签套在一个容器文件夹里（书签栏 / Bookmarks bar…），
    /// 那不是用户的分类，必须跳过，否则整个标签栏只有一个「书签栏」。
    #[test]
    fn skips_browser_container_folders() {
        let html = r#"<DL><p>
            <DT><H3>书签栏</H3>
            <DL><p>
                <DT><A HREF="https://a.com">A</A>
                <DT><H3>开发</H3>
                <DL><p>
                    <DT><A HREF="https://b.com">B</A>
                </DL><p>
            </DL><p>
            <DT><H3>Other Bookmarks</H3>
            <DL><p>
                <DT><A HREF="https://c.com">C</A>
            </DL><p>
        </DL><p>"#;

        let items = parse_netscape(html);
        let by = |url: &str| items.iter().find(|i| i.url == url).unwrap().group.clone();

        assert_eq!(by("https://a.com"), "", "容器下直接放的书签归未分组");
        assert_eq!(by("https://b.com"), "开发", "容器被跳过，取下层真实分类");
        assert_eq!(by("https://c.com"), "", "英文容器名同样跳过");
    }

    /// 容器文件夹也不该变成标签。
    ///
    /// 标签栏看起来像"第二排分组"的主要来源就是它：整个收藏夹都在「书签栏」下，
    /// 于是每条书签都带上一个「书签栏」标签——既不表达内容，检索也永远命中全集。
    #[test]
    fn skips_container_folders_in_tags() {
        let html = r#"<DL><p>
            <DT><H3>书签栏</H3>
            <DL><p>
                <DT><A HREF="https://a.com">A</A>
                <DT><H3>开发</H3>
                <DL><p>
                    <DT><H3>前端</H3>
                    <DL><p><DT><A HREF="https://b.com">B</A></DL><p>
                </DL><p>
            </DL><p>
        </DL><p>"#;

        let items = parse_netscape(html);
        let tags = |url: &str| items.iter().find(|i| i.url == url).unwrap().tags.clone();

        assert_eq!(tags("https://a.com"), "", "容器名不进标签");
        assert_eq!(tags("https://b.com"), "开发,前端", "容器之外的路径照旧全进标签");
    }

    /// 容器名大小写不敏感；不是容器的首层照常当分组。
    #[test]
    fn container_match_ignores_case_and_keeps_real_first_level() {
        let items = parse_netscape(
            r#"<DL><p>
                <DT><H3>FAVORITES BAR</H3>
                <DL><p>
                    <DT><H3>工具</H3>
                    <DL><p><DT><A HREF="https://x.com">X</A></DL><p>
                </DL><p>
            </DL><p>"#,
        );
        assert_eq!(items[0].group, "工具");

        let plain = parse_netscape(
            r#"<DL><p><DT><H3>我的收藏</H3><DL><p><DT><A HREF="https://y.com">Y</A></DL><p></DL><p>"#,
        );
        assert_eq!(plain[0].group, "我的收藏", "不认识的文件夹名当真实分类");
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
