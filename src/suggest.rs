//! 搜索联想：把「哪个引擎能用什么联想接口」和几家接口的解析收到一起。
//!
//! 联想接口是各家私有的端点，格式互不相同（有 JSON、有 JSONP、字段名也各家各叫），
//! 没法像搜索地址那样用一个 `%s` 模板通吃。所以这里是一张**写死的表**：按引擎
//! 地址的 host 认出来是哪家，就用对应的小解析器。认不出来的引擎就没有联想——
//! 首页那几条本地书签匹配照常，功能不残缺。
//!
//! 两个安全/可用性约束：
//!   * 请求由**服务端**代取（浏览器直连会被 CORS 挡）；
//!   * 端点**只按库里的引擎 id 反查**，不接受客户端传地址——否则这就是个
//!     任意出站请求的入口（SSRF），和 `/icon/{host}` 只服务已收藏站点是同一个道理。
//!
//! 纯函数（认引擎 + 解析响应）都写在这里，方便单测；发请求的部分在 `backend.rs`。

/// 响应格式。每家一个，解析方式见 `parse`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// 百度：`{"g":[{"q":"…"},…]}`
    Baidu,
    /// 必应 / 谷歌：`["rust",["rustdesk","rust游戏",…],…]`
    ArrayPair,
    /// 360：`{"result":[{"word":"…"},…]}`
    So360,
    /// 搜狗：`window.sogou.sug(["rust",["…",…],…])`——JSONP，得先把壳剥掉
    Sogou,
}

/// 一个联想端点：`%s` 是查询词占位，和引擎的搜索模板同一套写法。
#[derive(Debug, Clone, Copy)]
pub struct Endpoint {
    pub url: &'static str,
    pub kind: Kind,
}

/// 按引擎地址认出联想端点。认不出来返回 `None`（该引擎就没有联想）。
pub fn endpoint_for(engine_url: &str) -> Option<Endpoint> {
    let host = host_of(engine_url)?;

    // 用后缀匹配，覆盖 www. / cn. / m. 这些常见前缀
    let table: &[(&str, Endpoint)] = &[
        (
            "baidu.com",
            Endpoint {
                url: "https://www.baidu.com/sugrec?prod=pc&wd=%s",
                kind: Kind::Baidu,
            },
        ),
        (
            "bing.com",
            Endpoint {
                url: "https://api.bing.com/osjson.aspx?query=%s",
                kind: Kind::ArrayPair,
            },
        ),
        // 谷歌的联想端点在部分网络下不可达；取不到就是没有联想，不影响搜索本身
        (
            "google.com",
            Endpoint {
                url: "https://suggestqueries.google.com/complete/search?client=firefox&q=%s",
                kind: Kind::ArrayPair,
            },
        ),
        (
            "google.com.hk",
            Endpoint {
                url: "https://suggestqueries.google.com/complete/search?client=firefox&q=%s",
                kind: Kind::ArrayPair,
            },
        ),
        (
            "so.com",
            Endpoint {
                url: "https://sug.so.360.cn/suggest?word=%s",
                kind: Kind::So360,
            },
        ),
        (
            "360.cn",
            Endpoint {
                url: "https://sug.so.360.cn/suggest?word=%s",
                kind: Kind::So360,
            },
        ),
        (
            "sogou.com",
            Endpoint {
                url: "https://sor.html5.qq.com/api/getsug?key=%s",
                kind: Kind::Sogou,
            },
        ),
    ];

    table
        .iter()
        .find(|(suffix, _)| host == *suffix || host.ends_with(&format!(".{suffix}")))
        .map(|(_, endpoint)| *endpoint)
}

/// 取 URL 的 host（只认 http/https）。联想端点只需要 host，比 favicon 那边宽松：
/// 不校验合法性，认不出就是没有联想。
fn host_of(url: &str) -> Option<String> {
    let rest = url.trim();
    let rest = rest
        .strip_prefix("https://")
        .or_else(|| rest.strip_prefix("http://"))?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host = authority.rsplit('@').next().unwrap_or(authority);
    (!host.is_empty()).then(|| host.to_ascii_lowercase())
}

/// 把查询词转义成能塞进 URL 的形状（空格要变 `%20`，中文要变百分号编码）。
pub fn encode_query(query: &str) -> String {
    let mut out = String::with_capacity(query.len());
    for byte in query.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// 解析联想响应。任何认不出的形状都回空表——联想失败不该影响搜索。
pub fn parse(kind: Kind, body: &str) -> Vec<String> {
    let value = match kind {
        Kind::Sogou => match unwrap_jsonp(body) {
            Some(json) => json,
            None => return Vec::new(),
        },
        _ => body,
    };

    let Ok(root) = serde_json::from_str::<serde_json::Value>(value) else {
        return Vec::new();
    };

    let items: Vec<String> = match kind {
        Kind::Baidu => root
            .get("g")
            .and_then(|g| g.as_array())
            .map(|list| list.iter().filter_map(|item| str_at(item, "q")).collect())
            .unwrap_or_default(),
        Kind::So360 => root
            .get("result")
            .and_then(|r| r.as_array())
            .map(|list| list.iter().filter_map(|item| str_at(item, "word")).collect())
            .unwrap_or_default(),
        // ["查询词", ["建议1","建议2",…], …]：取第 2 项
        Kind::ArrayPair => root
            .as_array()
            .and_then(|list| list.get(1))
            .and_then(|suggestions| suggestions.as_array())
            .map(|list| {
                list.iter()
                    .filter_map(|item| item.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
        Kind::Sogou => root
            .as_array()
            .and_then(|list| list.get(1))
            .and_then(|suggestions| suggestions.as_array())
            .map(|list| {
                list.iter()
                    .filter_map(|item| item.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
    };

    dedup_and_trim(items)
}

/// 取对象里的字符串字段。
fn str_at(value: &serde_json::Value, key: &str) -> Option<String> {
    value.get(key)?.as_str().map(str::to_string)
}

/// 剥掉 JSONP 外壳：`window.sogou.sug( … )` → `…`。
///
/// 从第一个 `(` 取到最后一个 `)`，比逐个匹配函数名稳（各家包法会变）。
fn unwrap_jsonp(body: &str) -> Option<&str> {
    let start = body.find('(')? + 1;
    let end = body.rfind(')')?;
    (end > start).then(|| body[start..end].trim())
}

/// 去掉空项与重复项，并压掉过长的一条。
///
/// 重复是真实存在的：同一次响应里百度会给出大小写不同的同一条建议。
fn dedup_and_trim(items: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for item in items {
        let item = item.trim().to_string();
        if item.is_empty() || item.chars().count() > 60 {
            continue;
        }
        if out.iter().any(|existing| existing.eq_ignore_ascii_case(&item)) {
            continue;
        }
        out.push(item);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_engines_by_host() {
        assert_eq!(endpoint_for("https://www.baidu.com/s?wd=%s").unwrap().kind, Kind::Baidu);
        assert_eq!(endpoint_for("https://www.bing.com/search?q=%s").unwrap().kind, Kind::ArrayPair);
        assert_eq!(endpoint_for("https://cn.bing.com/search?q=%s").unwrap().kind, Kind::ArrayPair);
        assert_eq!(endpoint_for("https://www.google.com/search?q=%s").unwrap().kind, Kind::ArrayPair);
        assert_eq!(endpoint_for("https://www.so.com/s?q=%s").unwrap().kind, Kind::So360);
        assert_eq!(endpoint_for("https://www.sogou.com/web?query=%s").unwrap().kind, Kind::Sogou);

        // 认不出来就是没有联想，不是错误
        assert!(endpoint_for("https://github.com/search?q=%s").is_none());
        assert!(endpoint_for("https://developer.mozilla.org/search?q=%s").is_none());
        assert!(endpoint_for("not a url").is_none());
    }

    #[test]
    fn does_not_match_a_lookalike_host() {
        // 只有"完全相等"或"以 .后缀 结尾"才算，`notbaidu.com` 不能蹭上 baidu.com
        assert!(endpoint_for("https://notbaidu.com/search?q=%s").is_none());
        assert!(endpoint_for("https://evil-bing.com/search?q=%s").is_none());
    }

    #[test]
    fn encodes_query_for_url() {
        assert_eq!(encode_query("rust lang"), "rust%20lang");
        assert_eq!(encode_query("rust语言"), "rust%E8%AF%AD%E8%A8%80");
        // 这些字符在查询串里必须转义，否则会截断 URL
        assert_eq!(encode_query("a&b=c"), "a%26b%3Dc");
        assert_eq!(encode_query("100%"), "100%25");
    }

    #[test]
    fn parses_baidu() {
        let body = r#"{"q":"rust","p":false,"g":[{"type":"sug","sa":"s_1","q":"rust语言"},{"type":"sug","sa":"s_2","q":"rust游戏"}]}"#;
        assert_eq!(parse(Kind::Baidu, body), vec!["rust语言", "rust游戏"]);
        // 空结果（百度用 g 缺失表示）
        assert!(parse(Kind::Baidu, r#"{"q":"rust","p":false}"#).is_empty());
    }

    #[test]
    fn parses_bing_and_google_shape() {
        let body = r#"["rust",["rustdesk","rust游戏","rust语言"],[],[],{"google:suggestrelevance":[1,2,3]}]"#;
        assert_eq!(
            parse(Kind::ArrayPair, body),
            vec!["rustdesk", "rust游戏", "rust语言"]
        );
        assert!(parse(Kind::ArrayPair, r#"["rust"]"#).is_empty());
    }

    #[test]
    fn parses_so360() {
        let body = r#"{"errorcode":0,"query":"rust","result":[{"face":"","word":"rustdesk官网"},{"word":"rust游戏"}]}"#;
        assert_eq!(parse(Kind::So360, body), vec!["rustdesk官网", "rust游戏"]);
    }

    #[test]
    fn parses_sogou_jsonp() {
        let body = r#"window.sogou.sug(["rust",["rust","rust游戏","rust官网"],["0;15;0;0"]]);"#;
        assert_eq!(parse(Kind::Sogou, body), vec!["rust", "rust游戏", "rust官网"]);
    }

    #[test]
    fn bad_input_gives_empty_not_panic() {
        // 超时被拦截的页面、限流提示、半截 JSON……都只该得到空表
        for body in [
            "",
            "not json at all",
            "<html>502 Bad Gateway</html>",
            "[",
            r#"{"g":"不是数组"}"#,
            "window.sogou.sug(坏掉的",
        ] {
            for kind in [Kind::Baidu, Kind::ArrayPair, Kind::So360, Kind::Sogou] {
                assert!(parse(kind, body).is_empty(), "{kind:?} 解析 {body:?} 应为空");
            }
        }
    }

    #[test]
    fn drops_blank_and_duplicate_suggestions() {
        let body = r#"["rust",["rust","","  ","RUST","rust游戏"],[]]"#;
        assert_eq!(parse(Kind::ArrayPair, body), vec!["rust", "rust游戏"]);
    }
}
