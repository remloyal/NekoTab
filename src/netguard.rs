//! 出站请求的"能不能去"判定（SSRF 防护）。
//!
//! 服务端会替用户去取三类东西：站点图标、网页标题（抓来预填名称）、壁纸原图。
//! 这些请求的地址有一部分是**用户自己给的**（书签的网址、"存到本地"填的地址），
//! 有一部分是**对方站点给的**——页面里 `<link rel="icon">` 声明的图标地址，
//! 以及 302 跳过去的目标。
//!
//! 后者是 SSRF 的口子：一个被收藏的第三方站点（作者可控）可以把服务端的请求
//! 引到内网地址上，再把响应读回来。实测把图标地址指到 `127.0.0.1` 的另一个
//! 端口，`/icon/{host}` 会把那个端口上的内容原样送回浏览器——文档里"这个接口
//! 只服务本站已收藏的站点"那句话只管住了**入口**，没管住**出口**。
//!
//! 规则只有一条：**目标地址必须解析到公网 IP**。唯一的例外是"目标与用户
//! 指定的那个地址完全是同一个端点"——那本来就是用户自己要访问的内网地址，
//! README 里也认了内网站点的图标多半取不到，那是另一回事。
//!
//! 于是：
//! * 第三方站点声明的图标地址若指向内网 → 拒绝；
//! * 第三方站点 302 跳到内网 → 拒绝（每一跳都重新判）；
//! * 用户自己收藏的内网地址 → 照旧照去。

use std::net::{IpAddr, ToSocketAddrs};

/// 一个地址的"主机 + 端口"。
///
/// 端口要一起比：只比主机的话，`127.0.0.1:19000` 指到 `127.0.0.1:19001`
/// 会被当成"同一个地方"放过去——那正是要挡的那条路。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Endpoint {
    pub host: String,
    pub port: u16,
}

impl Endpoint {
    /// 取一个 URL 的主机与端口。
    pub(crate) fn of(url: &str) -> Option<Self> {
        let parsed = reqwest::Url::parse(url).ok()?;
        let host = parsed.host_str()?.to_string();
        let port = parsed.port_or_known_default().unwrap_or(80);
        Some(Self { host, port })
    }

    /// 主机名（不带端口），解析 DNS 时用。
    fn hostname(&self) -> &str {
        // IPv6 字面量在 URL 里带方括号，解析 DNS 时要摘掉
        self.host
            .strip_prefix('[')
            .and_then(|rest| rest.strip_suffix(']'))
            .unwrap_or(&self.host)
    }
}

/// 两个地址是不是"同一个端点"。
///
/// 端口相同才算；但 http(80) ↔ https(443) 那对标准端口算同一个——同一个站点
/// 从 http 跳到 https 是常事，不该因此把图标判成"跳到了别处"。
pub(crate) fn same_endpoint(a: &Endpoint, b: &Endpoint) -> bool {
    a.host.eq_ignore_ascii_case(&b.host)
        && (a.port == b.port
            || (a.port == 80 && b.port == 443)
            || (a.port == 443 && b.port == 80))
}

/// 这个 IP 是不是内网地址。
///
/// 挡的是环回（127/8、::1）、私有段（10/8、172.16/12、192.168/16、fc00::/7）、
/// 链路本地（169.254/16、fe80::/10）、以及云上元数据那种特殊地址。
pub(crate) fn is_private_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                // 运营商级 NAT（100.64.0.0/10）
                || (o[0] == 100 && (64..128).contains(&o[1]))
                // 文档用段（192.0.2 / 198.51.100 / 203.0.113）
                || (o[0] == 192 && o[1] == 0 && o[2] == 2)
                || (o[0] == 198 && o[1] == 51 && o[2] == 100)
                || (o[0] == 203 && o[1] == 0 && o[2] == 113)
        }
        IpAddr::V6(v6) => {
            v6.is_loopback()
                || v6.is_unspecified()
                // 唯一本地地址 fc00::/7
                || (v6.segments()[0] & 0xfe00) == 0xfc00
                // 链路本地 fe80::/10
                || (v6.segments()[0] & 0xffc0) == 0xfe80
                // IPv4 映射 / 兼容写法（::ffff:10.0.0.1）
                || v6
                    .to_ipv4()
                    .map(|v4| is_private_ip(IpAddr::V4(v4)))
                    .unwrap_or(false)
        }
    }
}

/// 目标主机是不是指向内网。
///
/// 主机名要解析一次才知道，所以这是异步的。解析不出来按"不安全"处理——
/// 宁可不去，也不给一条解析不了的地址开后门。
async fn resolves_to_private(target: &Endpoint) -> bool {
    let host = target.hostname();
    if let Ok(ip) = host.parse::<IpAddr>() {
        return is_private_ip(ip);
    }
    match tokio::net::lookup_host((host, target.port)).await {
        Ok(addrs) => {
            let mut any = false;
            for addr in addrs {
                any = true;
                if is_private_ip(addr.ip()) {
                    return true;
                }
            }
            !any
        }
        Err(_) => true,
    }
}

/// 这个地址可以去取吗。
///
/// `origin` 是"用户自己指定的那个地址"（书签的网址、"存到本地"填的地址）：
/// 目标与它完全是同一个端点时放行——那本来就是用户要访问的内网地址。
/// 其余情况一律要求目标解析到公网。
pub(crate) async fn ensure_allowed(url: &str, origin: Option<&Endpoint>) -> Result<(), String> {
    let target = Endpoint::of(url).ok_or_else(|| format!("地址不合法：{url}"))?;
    if let Some(origin) = origin {
        if same_endpoint(origin, &target) {
            return Ok(());
        }
    }
    if resolves_to_private(&target).await {
        return Err(format!("拒绝访问内网地址（{}）", target.host));
    }
    Ok(())
}

/// 同步版的"是不是内网"判定，给重定向策略用。
///
/// 重定向策略是个同步闭包，没法 await。这里的 DNS 解析会短暂阻塞当前线程，
/// 但只有**跨端点**的跳转才会走到这里（同一端点的跳转直接放行，不解析），
/// 而跨端点的跳转本来就不多——这个代价比放一个 SSRF 出去小得多。
fn resolves_to_private_blocking(target: &Endpoint) -> bool {
    let host = target.hostname();
    if let Ok(ip) = host.parse::<IpAddr>() {
        return is_private_ip(ip);
    }
    match (host, target.port).to_socket_addrs() {
        Ok(mut addrs) => addrs.any(|addr| is_private_ip(addr.ip())),
        Err(_) => true,
    }
}

/// 重定向策略：每一跳都重新判一次目标。
///
/// 规则：目标与上一跳是同一个端点就放行（同一站点换路径、http↔https 是常事）；
/// 否则目标必须解析到公网地址。这样"第三方站点 302 到内网"这条路被堵住了，
/// 而站点正常的跳转不受影响。
pub(crate) fn redirect_policy() -> reqwest::redirect::Policy {
    reqwest::redirect::Policy::custom(|attempt| {
        let target = Endpoint::of(attempt.url().as_str());
        // previous() 给的是整条重定向历史，取最后一个（也就是上一跳）
        let previous = attempt
            .previous()
            .last()
            .and_then(|url| Endpoint::of(url.as_str()));

        match (target, previous) {
            (Some(target), Some(previous)) if same_endpoint(&previous, &target) => attempt.follow(),
            (Some(target), _) if !resolves_to_private_blocking(&target) => attempt.follow(),
            _ => attempt.stop(),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_and_private_ranges_are_blocked() {
        for ip in [
            "127.0.0.1",
            "127.1.2.3",
            "10.0.0.5",
            "172.16.0.1",
            "172.31.255.255",
            "192.168.1.10",
            "169.254.169.254",
            "0.0.0.0",
            "100.64.0.1",
            "::1",
            "fc00::1",
            "fe80::1",
            "::ffff:127.0.0.1",
        ] {
            assert!(is_private_ip(ip.parse().unwrap()), "{ip} 应该算内网");
        }
    }

    #[test]
    fn public_addresses_are_allowed() {
        for ip in [
            "8.8.8.8",
            "1.1.1.1",
            "172.32.0.1",
            "172.15.255.255",
            "11.0.0.1",
            "2606:4700::1111",
        ] {
            assert!(!is_private_ip(ip.parse().unwrap()), "{ip} 不该算内网");
        }
    }

    #[test]
    fn endpoints_compare_by_host_and_port() {
        let a = Endpoint::of("http://127.0.0.1:19000/x").unwrap();
        let b = Endpoint::of("http://127.0.0.1:19001/y").unwrap();
        let c = Endpoint::of("http://127.0.0.1:19000/y").unwrap();
        assert!(!same_endpoint(&a, &b), "端口不同就是不同的地方");
        assert!(same_endpoint(&a, &c));
        // http ↔ https 算同一个站点
        let http = Endpoint::of("http://example.com/").unwrap();
        let https = Endpoint::of("https://example.com/").unwrap();
        assert!(same_endpoint(&http, &https));
        // 大小写不影响主机名
        let upper = Endpoint::of("http://Example.COM/").unwrap();
        assert!(same_endpoint(&http, &upper));
    }
}
