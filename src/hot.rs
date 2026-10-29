//! 热搜数据源：地址拼装、响应解析与十分钟缓存。
//!
//! 只在服务端编译。客户端拿到的已经是 `Vec<HotItem>`（标题 + 可点开的地址），
//! 不接触任何上游格式。端点写死在表里、只按 `source` 白名单反查，客户端无法
//! 让服务端请求任意地址。
//!
//! 上游都是各家自己的 JSON，字段名与嵌套层数互不相同，所以每家一个解析函数；
//! 解析函数是纯函数，单测直接拿样例 JSON 校验，不必联网。

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::engine::encode_query;
use crate::models::HotItem;

/// 缓存多久。客户端每十分钟刷一次，服务端跟着同样节奏即可。
const CACHE_TTL: Duration = Duration::from_secs(10 * 60);
/// 一张卡片最多放几条。
const MAX_ITEMS: usize = 20;

const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
                  (KHTML, like Gecko) Chrome/123.0.0.0 Safari/537.36";

fn cache() -> &'static Mutex<HashMap<String, (Instant, Vec<HotItem>)>> {
    static CACHE: OnceLock<Mutex<HashMap<String, (Instant, Vec<HotItem>)>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn cache_get(source: &str) -> Option<Vec<HotItem>> {
    let guard = cache().lock().ok()?;
    let (at, items) = guard.get(source)?;
    (at.elapsed() < CACHE_TTL).then(|| items.clone())
}

fn cache_set(source: &str, items: &[HotItem]) {
    if let Ok(mut guard) = cache().lock() {
        guard.insert(source.to_string(), (Instant::now(), items.to_vec()));
    }
}

/// 拉一个热搜榜。失败时返回一句可展示的原因。
pub async fn fetch(source: &str) -> Result<Vec<HotItem>, String> {
    if let Some(items) = cache_get(source) {
        return Ok(items);
    }

    let (url, referer) = match source {
        "baidu" => ("https://top.baidu.com/api/board?tab=realtime", None),
        "zhihu" => (
            "https://www.zhihu.com/api/v4/creators/rank/hot?domain=0&period=hour&limit=50&offset=0",
            None,
        ),
        "bilibili" => (
            "https://api.bilibili.com/x/web-interface/ranking/v2?rid=0&type=all",
            Some("https://www.bilibili.com"),
        ),
        "weibo" => (
            "https://weibo.com/ajax/statuses/hot_band",
            Some("https://weibo.com"),
        ),
        "toutiao" => (
            "https://www.toutiao.com/hot-event/hot-board/?origin=toutiao_pc",
            Some("https://www.toutiao.com"),
        ),
        _ => return Err("不认识的热搜来源".to_string()),
    };

    let mut request = crate::backend::http_client()
        .get(url)
        .header(reqwest::header::USER_AGENT, UA);
    if let Some(referer) = referer {
        request = request.header(reqwest::header::REFERER, referer);
    }

    let body = request
        .send()
        .await
        .map_err(|e| format!("请求热搜接口失败: {e}"))?
        .text()
        .await
        .map_err(|e| format!("读取热搜响应失败: {e}"))?;

    let items = match source {
        "baidu" => parse_baidu(&body),
        "zhihu" => parse_zhihu(&body),
        "bilibili" => parse_bilibili(&body),
        "weibo" => parse_weibo(&body),
        "toutiao" => parse_toutiao(&body),
        _ => Ok(Vec::new()),
    }?;

    cache_set(source, &items);
    Ok(items)
}

fn parse_json(body: &str) -> Result<Value, String> {
    serde_json::from_str(body).map_err(|e| format!("解析热搜响应失败: {e}"))
}

/// 去重、去空标题、截到上限。各家榜单偶尔有重复条目，卡片上重复一遍没有意义。
///
/// 地址只留 http(s)：`open_url` 会把地址交给 `window.open`，上游若返回
/// `javascript:` 之类的地址，这里就是最后一道闸。
fn finish(mut items: Vec<HotItem>) -> Vec<HotItem> {
    let mut seen: Vec<String> = Vec::new();
    items.retain(|item| {
        let title = item.title.trim();
        let url_ok = item.url.starts_with("http://") || item.url.starts_with("https://");
        if title.is_empty() || !url_ok || seen.iter().any(|s| s == title) {
            false
        } else {
            seen.push(title.to_string());
            true
        }
    });
    items.truncate(MAX_ITEMS);
    items
}

/// 百度热搜：`data.cards[0]`，置顶在 `topContent`，其余在 `content`。
pub fn parse_baidu(body: &str) -> Result<Vec<HotItem>, String> {
    let root = parse_json(body)?;
    let card = &root["data"]["cards"][0];
    let mut items = Vec::new();

    if let Some(top) = card.get("topContent").and_then(|v| v.as_array()) {
        for item in top.iter().take(1) {
            push_baidu(item, &mut items);
        }
    }
    if let Some(list) = card.get("content").and_then(|v| v.as_array()) {
        for item in list {
            push_baidu(item, &mut items);
        }
    }

    Ok(finish(items))
}

fn push_baidu(item: &Value, out: &mut Vec<HotItem>) {
    let Some(word) = item.get("word").and_then(|v| v.as_str()) else {
        return;
    };
    out.push(HotItem {
        title: word.to_string(),
        url: format!("https://www.baidu.com/s?wd={}", encode_query(word)),
    });
}

/// 知乎热榜：`data[].question.{title,url}`。
pub fn parse_zhihu(body: &str) -> Result<Vec<HotItem>, String> {
    let root = parse_json(body)?;
    let mut items = Vec::new();
    if let Some(list) = root.get("data").and_then(|v| v.as_array()) {
        for item in list {
            let question = &item["question"];
            let title = question.get("title").and_then(|v| v.as_str()).unwrap_or("");
            let url = question.get("url").and_then(|v| v.as_str()).unwrap_or("");
            items.push(HotItem {
                title: title.to_string(),
                url: url.to_string(),
            });
        }
    }
    Ok(finish(items))
}

/// B 站排行榜：`data.list[].{title,bvid}`。
pub fn parse_bilibili(body: &str) -> Result<Vec<HotItem>, String> {
    let root = parse_json(body)?;
    let mut items = Vec::new();
    if let Some(list) = root
        .get("data")
        .and_then(|v| v.get("list"))
        .and_then(|v| v.as_array())
    {
        for item in list {
            let title = item.get("title").and_then(|v| v.as_str()).unwrap_or("");
            let bvid = item.get("bvid").and_then(|v| v.as_str()).unwrap_or("");
            items.push(HotItem {
                title: title.to_string(),
                url: if bvid.is_empty() {
                    String::new()
                } else {
                    format!("https://www.bilibili.com/video/{bvid}")
                },
            });
        }
    }
    Ok(finish(items))
}

/// 微博热搜：`data.band_list[].word`。
pub fn parse_weibo(body: &str) -> Result<Vec<HotItem>, String> {
    let root = parse_json(body)?;
    let mut items = Vec::new();
    if let Some(list) = root
        .get("data")
        .and_then(|v| v.get("band_list"))
        .and_then(|v| v.as_array())
    {
        for item in list {
            let word = item.get("word").and_then(|v| v.as_str()).unwrap_or("");
            items.push(HotItem {
                title: word.to_string(),
                url: format!("https://s.weibo.com/weibo?q={}", encode_query(word)),
            });
        }
    }
    Ok(finish(items))
}

/// 头条热榜：`data[].{Title,Url}`。
pub fn parse_toutiao(body: &str) -> Result<Vec<HotItem>, String> {
    let root = parse_json(body)?;
    let mut items = Vec::new();
    if let Some(list) = root.get("data").and_then(|v| v.as_array()) {
        for item in list {
            let title = item.get("Title").and_then(|v| v.as_str()).unwrap_or("");
            let url = item.get("Url").and_then(|v| v.as_str()).unwrap_or("");
            items.push(HotItem {
                title: title.to_string(),
                url: url.to_string(),
            });
        }
    }
    Ok(finish(items))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn baidu_reads_top_and_list() {
        let body = r#"{"data":{"cards":[{"topContent":[{"word":"置顶"}],
            "content":[{"word":"第一条"},{"word":"第二条"}]}]}}"#;
        let items = parse_baidu(body).unwrap();
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].title, "置顶");
        assert!(items[1].url.contains("wd=%E7%AC%AC%E4%B8%80%E6%9D%A1"));
    }

    #[test]
    fn zhihu_reads_question() {
        let body = r#"{"data":[{"question":{"title":"为什么","url":"https://www.zhihu.com/question/1"}}]}"#;
        let items = parse_zhihu(body).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "为什么");
    }

    #[test]
    fn bilibili_reads_bvid() {
        let body = r#"{"data":{"list":[{"title":"视频","bvid":"BV1xx"}]}}"#;
        let items = parse_bilibili(body).unwrap();
        assert_eq!(items[0].url, "https://www.bilibili.com/video/BV1xx");
    }

    #[test]
    fn weibo_and_toutiao_read_their_shapes() {
        let weibo = r#"{"data":{"band_list":[{"word":"话题"}]}}"#;
        assert_eq!(parse_weibo(weibo).unwrap()[0].title, "话题");

        let toutiao = r#"{"data":[{"Title":"头条","Url":"https://www.toutiao.com/a"}]}"#;
        assert_eq!(
            parse_toutiao(toutiao).unwrap()[0].url,
            "https://www.toutiao.com/a"
        );
    }

    #[test]
    fn duplicates_and_empty_titles_are_dropped() {
        let body =
            r#"{"data":{"cards":[{"content":[{"word":"同一条"},{"word":"同一条"},{"word":""}]}]}}"#;
        assert_eq!(parse_baidu(body).unwrap().len(), 1);
    }
}
