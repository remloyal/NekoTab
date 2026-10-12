//! 搜索引擎相关的纯函数。独立于 UI 与数据库，便于单测。

/// 对搜索词做 URL 百分号编码。
///
/// 保留 RFC 3986 的非保留字符，其余按 UTF-8 字节转 `%XX`；
/// 空格编码为 `%20`（`+` 在部分引擎的路径形式里不通用，故不用）。
pub fn encode_query(input: &str) -> String {
    let mut out = String::with_capacity(input.len() * 3);
    for byte in input.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char);
            }
            _ => out.push_str(&format!("%{:02X}", byte)),
        }
    }
    out
}

/// 把引擎地址模板里的 `%s` 替换为编码后的搜索词。
pub fn build_search_url(template: &str, query: &str) -> String {
    template.replace("%s", &encode_query(query))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_reserved_and_multibyte() {
        assert_eq!(encode_query("a b"), "a%20b");
        assert_eq!(encode_query("rust&c=1"), "rust%26c%3D1");
        assert_eq!(encode_query("中文"), "%E4%B8%AD%E6%96%87");
        assert_eq!(encode_query("a-b_c.d~e"), "a-b_c.d~e");
    }

    #[test]
    fn substitutes_placeholder() {
        assert_eq!(
            build_search_url("https://www.bing.com/search?q=%s", "rust lang"),
            "https://www.bing.com/search?q=rust%20lang"
        );
        assert_eq!(build_search_url("https://x/?q=%s", ""), "https://x/?q=");
    }

    #[test]
    fn replaces_every_occurrence() {
        assert_eq!(build_search_url("%s/%s", "a"), "a/a");
    }
}
