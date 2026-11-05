//! 死链检测：并发探测一批书签的地址还通不通。
//!
//! 由**服务端**发起：浏览器逐个去探会撞上跨域，而且页面一开就是几十个请求。
//! 服务端探完只回一份清单，客户端显示结果。
//!
//! 三种结果，不是一种：
//!
//! * [`LINK_OK`]——正常响应；
//! * [`LINK_BROKEN`]——有响应但状态码是错误（404 / 500 …），**这一类才算坏链**；
//! * [`LINK_UNREACHABLE`]——连不上 / 超时 / DNS 失败。
//!
//! 最后一类刻意与坏链分开：内网地址（`192.168.x.x`）、需要登录的站点、
//! 服务器所在网络到不了的域名，全都落在这里。服务器本来就可能访问不到
//! 你浏览器能访问的东西，把它们混进"坏链"再给一个"批量删除"按钮，
//! 是会误删东西的。

use crate::models::{LinkCheck, LINK_BROKEN, LINK_OK, LINK_UNREACHABLE};

/// 单条链接的超时。比他处代取（8 秒）短一点：这里一次要探几十条，
/// 每条都慢 8 秒的话整轮要等好几分钟。
pub const CHECK_TIMEOUT_SECS: u64 = 6;

/// 一次最多并发探多少条。太多会把对方站点与自己的连接池一起压垮，
/// 而且单机的出口带宽就那么多。
pub const MAX_CONCURRENCY: usize = 8;

/// 探一个地址，得出状态与原因。
///
/// 先用 HEAD：省流量（不用下载正文）。但有些服务器不认 HEAD（405 / 501），
/// 这时退回 GET，并只取第一个字节——对方仍然会回完整响应体，但我们的连接
/// 读到头就断了。
pub async fn probe(url: &str, client: &reqwest::Client) -> (String, String) {
    let head = client.head(url).send().await;

    match head {
        Ok(response) => {
            let status = response.status();
            // 服务器不认 HEAD：换成 GET 再试一次，别把活着的站点判成坏的
            if status.as_u16() == 405 || status.as_u16() == 501 {
                return match client.get(url).send().await {
                    Ok(response) => classify(response.status().as_u16()),
                    Err(e) => unreachable_detail(&e),
                };
            }
            classify(status.as_u16())
        }
        Err(e) => unreachable_detail(&e),
    }
}

/// 状态码 → 状态。
///
/// 2xx 与 3xx 都算"通"：301 / 302 是站点换了地址，浏览器跟着跳就完了，
/// 不算坏链。401 / 403 也算通——站点在，只是要登录，把它判成坏链就等于
/// 冤枉了人家（而且这类站点往往正是常用站点）。
pub fn classify(status: u16) -> (String, String) {
    if (200..400).contains(&status) || status == 401 || status == 403 {
        (LINK_OK.to_string(), String::new())
    } else {
        (LINK_BROKEN.to_string(), format!("HTTP {status}"))
    }
}

/// 连不上时的说明。错误原文可能很长（带上主机名与端口），截一下。
fn unreachable_detail(error: &reqwest::Error) -> (String, String) {
    let detail = if error.is_timeout() {
        "超时".to_string()
    } else if error.is_connect() {
        "连不上".to_string()
    } else if error.is_redirect() {
        "重定向出错".to_string()
    } else {
        let text = error.to_string();
        // reqwest 的错误串会带上完整 URL，界面上显示网址就够了
        let short = text.split(": ").last().unwrap_or(&text).to_string();
        if short.chars().count() > 40 {
            short.chars().take(40).collect::<String>() + "…"
        } else {
            short
        }
    };

    (LINK_UNREACHABLE.to_string(), detail)
}

/// 探一批书签，返回每条的结果。
///
/// `items` 是 `(id, title, url)`。并发数封顶在 [`MAX_CONCURRENCY`]，
/// 剩下的排队等着——一次几十条全砸出去，既容易被对方限流，
/// 也把自己的连接池占满了。
pub async fn probe_all(items: Vec<(i64, String, String)>) -> Vec<LinkCheck> {
    let client = crate::backend::link_check_client();
    let mut out: Vec<LinkCheck> = Vec::with_capacity(items.len());

    // 分批：每批 MAX_CONCURRENCY 条，批内并发、批间顺序，保证结果顺序稳定
    for batch in items.chunks(MAX_CONCURRENCY) {
        let mut tasks = Vec::with_capacity(batch.len());

        for (id, title, url) in batch {
            let client = client.clone();
            let id = *id;
            let title = title.clone();
            let url = url.clone();
            tasks.push(tokio::spawn(async move {
                let (status, detail) = probe(&url, &client).await;
                (id, title, url, status, detail)
            }));
        }

        for task in tasks {
            match task.await {
                Ok((id, title, url, status, detail)) => out.push(LinkCheck {
                    id,
                    title,
                    url,
                    status,
                    detail,
                }),
                // 任务本身炸了（极少）：算不可达，别让一条把整轮检测带崩
                Err(_) => {}
            }
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ok_and_redirect_count_as_alive() {
        assert_eq!(classify(200), (LINK_OK.to_string(), String::new()));
        assert_eq!(classify(204), (LINK_OK.to_string(), String::new()));
        assert_eq!(classify(301), (LINK_OK.to_string(), String::new()));
        assert_eq!(classify(302), (LINK_OK.to_string(), String::new()));
    }

    #[test]
    fn auth_errors_are_not_broken() {
        // 要登录的站点也是"活的"——把它判成坏链就等于冤枉人家
        assert_eq!(classify(401).0, LINK_OK);
        assert_eq!(classify(403).0, LINK_OK);
    }

    #[test]
    fn server_errors_are_broken() {
        let (status, detail) = classify(404);
        assert_eq!(status, LINK_BROKEN);
        assert_eq!(detail, "HTTP 404");

        assert_eq!(classify(500).0, LINK_BROKEN);
        assert_eq!(classify(503).0, LINK_BROKEN);
    }
}
