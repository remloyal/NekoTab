use dioxus::prelude::*;

use crate::backend::{error_text, regenerate_api_token};
// 只在客户端调用：SSR 阶段不去问服务端要令牌（首屏两边都渲染成"空"）
#[cfg(feature = "web")]
use crate::backend::get_api_token;

/// 「快捷添加」设置分区：书签小工具 + 令牌。
///
/// 令牌和来源地址都只能在客户端拿到（SSR 不知道自己的对外域名，令牌也要等服务端
/// 生成），所以首屏先渲染成"读取中"，挂载后再填。两边首次渲染都是空，不会 hydration 失配。
#[component]
pub fn QuickAddPanel() -> Element {
    let mut token = use_signal(String::new);
    // 服务端分支里没有写入（下面那处只在 web 下编译），`mut` 只在 web 下需要
    #[cfg_attr(feature = "server", allow(unused_mut))]
    let mut origin = use_signal(String::new);
    let mut busy = use_signal(|| false);
    let mut message = use_signal(|| Option::<String>::None);

    #[cfg_attr(not(feature = "web"), allow(unused_variables))]
    let init_once = use_hook(|| std::cell::Cell::new(false));
    use_effect(move || {
        #[cfg(feature = "web")]
        if !init_once.get() {
            init_once.set(true);
            origin.set(crate::dom::current_origin());
            spawn(async move {
                match get_api_token().await {
                    Ok(value) => token.set(value),
                    Err(e) => message.set(Some(error_text(&e))),
                }
            });
        }
    });

    let bookmarklet = build_bookmarklet(&origin(), &token());
    let ready = !bookmarklet.is_empty();

    rsx! {
        section { class: "panel-section",
            h3 { class: "panel-heading", "快捷添加" }
            p { class: "field-hint",
                "在别的页面看到想收的链接，不用先回到这里——把下面的书签小工具拖到浏览器书签栏，"
                "在任意网页点一下即可加入。同一条网址不会重复添加。"
            }

            div { class: "field",
                span { class: "field-label", "书签小工具" }
                if ready {
                    div { class: "quick-actions",
                        // 拖到书签栏安装（浏览器地址栏拖拽安装 bookmarklet 的经典做法）
                        a {
                            class: "bookmarklet",
                            href: "{bookmarklet}",
                            draggable: "true",
                            title: "拖到书签栏",
                            "＋ 添加到 NekoTab"
                        }
                        button {
                            class: "btn-ghost",
                            r#type: "button",
                            onclick: move |_| {
                                crate::dom::copy_text(&build_bookmarklet(&origin(), &token()));
                                message.set(Some("已复制书签小工具代码".to_string()));
                            },
                            "复制代码"
                        }
                    }
                    p { class: "field-hint",
                        "拖不动的话（有些浏览器不让拖 javascript: 链接），点「复制代码」，"
                        "然后在书签栏手动新建一个书签、把代码粘进地址栏。"
                    }
                } else {
                    p { class: "field-hint", "正在读取令牌…" }
                }
            }

            div { class: "field",
                span { class: "field-label", "令牌" }
                div { class: "quick-actions",
                    code { class: "quick-token", if token().is_empty() { "…" } else { "{token}" } }
                    button {
                        class: "btn-ghost",
                        r#type: "button",
                        disabled: token().is_empty(),
                        onclick: move |_| {
                            crate::dom::copy_text(&token());
                            message.set(Some("已复制令牌".to_string()));
                        },
                        "复制"
                    }
                    button {
                        class: "btn-ghost",
                        r#type: "button",
                        disabled: busy() || token().is_empty(),
                        onclick: move |_| {
                            if busy() {
                                return;
                            }
                            spawn(async move {
                                busy.set(true);
                                message.set(None);
                                match regenerate_api_token().await {
                                    Ok(fresh) => {
                                        token.set(fresh);
                                        message.set(Some(
                                            "已换新令牌，旧的书签小工具需要重新拖一次。".to_string(),
                                        ));
                                    }
                                    Err(e) => message.set(Some(error_text(&e))),
                                }
                                busy.set(false);
                            });
                        },
                        if busy() { "换新中…" } else { "重新生成" }
                    }
                }
                p { class: "field-hint",
                    "令牌等同于「加书签的写入权限」，已经嵌在书签小工具里，别外泄。"
                    "换新令牌会让旧的小工具立即失效。"
                }
            }

            div { class: "field",
                span { class: "field-label", "脚本接口" }
                code { class: "quick-token", "POST /api/bookmark" }
                p { class: "field-hint",
                    "给脚本或浏览器扩展用：POST 一个 JSON，字段为 token / url / title（可选）。"
                    "成功后回 JSON：ok 为 true，created 为 false 表示该网址已存在、没有重复写入。"
                }
            }

            if let Some(msg) = message() {
                p { class: "field-hint", "{msg}" }
            }
        }
    }
}

/// 拼出书签小工具的代码；地址或令牌还没就位时返回空串。
///
/// 小工具只能靠"新开一个地址"提交——跨站 `fetch` 会被 CORS 挡，导航不会。
/// 所以这里是 `window.open` 一个带参数的 `GET`，而不是 POST。
fn build_bookmarklet(origin: &str, token: &str) -> String {
    if origin.is_empty() || token.is_empty() {
        return String::new();
    }

    format!(
        "javascript:(function(){{var u='{origin}/api/bookmark?token={token}&url='\
         +encodeURIComponent(location.href)+'&title='+encodeURIComponent(document.title);\
         window.open(u,'_blank','width=460,height=320');}})()"
    )
}

#[cfg(test)]
mod tests {
    use super::build_bookmarklet;

    #[test]
    fn bookmarklet_needs_both_origin_and_token() {
        assert_eq!(build_bookmarklet("", "abc"), "");
        assert_eq!(build_bookmarklet("https://tab.example", ""), "");
    }

    #[test]
    fn bookmarklet_carries_origin_token_and_current_page() {
        let code = build_bookmarklet("https://tab.example", "deadbeef");
        assert!(code.starts_with("javascript:"));
        assert!(code.contains("https://tab.example/api/bookmark?token=deadbeef"));
        // 提交的是"当前页"，所以地址与标题都得在运行时现取
        assert!(code.contains("encodeURIComponent(location.href)"));
        assert!(code.contains("encodeURIComponent(document.title)"));
    }
}
