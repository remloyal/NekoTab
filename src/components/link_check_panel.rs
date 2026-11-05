use dioxus::prelude::*;

use crate::backend::{check_links, clear_link_status, delete_bookmarks, error_text};
use crate::models::{LinkCheck, LINK_BROKEN, LINK_UNREACHABLE};

/// 链接检查（主页设置抽屉内容）。
///
/// 书签会烂掉：站点改名、域名过期、文章下线。这里一次性把所有书签的地址探一遍，
/// 把结果分成三类摆出来：
///
/// * **坏链**（有响应但状态码是错的）——唯一会被标到卡片上、也唯一提供"批量清理"的一类；
/// * **不可达**（连不上 / 超时 / DNS 失败）——多半是内网地址、要登录的站点、
///   或者服务器所在网络到不了的域名，**不等于坏链**，所以不给一键删除；
/// * 正常的不列，只报个总数。
#[component]
pub fn LinkCheckPanel(on_changed: EventHandler<()>) -> Element {
    let mut checking = use_signal(|| false);
    let mut error = use_signal(|| Option::<String>::None);
    // 检测结果。`None` = 还没查过；`Some(vec![])` = 查过，没有书签
    let mut results = use_signal(|| Option::<Vec<LinkCheck>>::None);
    let mut confirming_delete = use_signal(|| false);

    let items = results().unwrap_or_default();
    let broken: Vec<LinkCheck> = items
        .iter()
        .filter(|item| item.status == LINK_BROKEN)
        .cloned()
        .collect();
    let unreachable: Vec<LinkCheck> = items
        .iter()
        .filter(|item| item.status == LINK_UNREACHABLE)
        .cloned()
        .collect();
    let alive = items.len() - broken.len() - unreachable.len();
    let broken_ids: Vec<i64> = broken.iter().map(|item| item.id).collect();

    let run = move |_| {
        if checking() {
            return;
        }

        spawn(async move {
            checking.set(true);
            error.set(None);
            confirming_delete.set(false);

            match check_links(Vec::new()).await {
                Ok(list) => results.set(Some(list)),
                Err(e) => error.set(Some(error_text(&e))),
            }

            checking.set(false);
            // 检测结果写回了书签行，卡片上的标记要跟着更新
            on_changed.call(());
        });
    };

    // 把坏链一次性移进回收站（软删除，还能还原）
    let delete_broken = move |_| {
        if broken_ids.is_empty() {
            return;
        }
        // 克隆一份再进异步块：直接按值捕获会把闭包变成 FnOnce，
        // 而事件处理器要的是 FnMut
        let ids = broken_ids.clone();

        spawn(async move {
            checking.set(true);
            error.set(None);
            confirming_delete.set(false);

            match delete_bookmarks(ids.clone()).await {
                Ok(count) => {
                    // 删掉的那些要从清单里去掉，剩下的结果保持原样
                    if let Some(mut list) = results() {
                        list.retain(|item| !ids.contains(&item.id));
                        results.set(Some(list));
                    }
                    error.set(Some(format!("已移入回收站 {count} 条")));
                }
                Err(e) => error.set(Some(error_text(&e))),
            }

            checking.set(false);
            on_changed.call(());
        });
    };

    let clear = move |_| {
        spawn(async move {
            checking.set(true);
            error.set(None);

            match clear_link_status().await {
                Ok(_) => results.set(None),
                Err(e) => error.set(Some(error_text(&e))),
            }

            checking.set(false);
            on_changed.call(());
        });
    };

    rsx! {
        section { class: "panel-section",
            h3 { class: "panel-heading", "链接检查" }

            p { class: "field-hint",
                "把所有书签的地址探一遍，找出点不开的那些。检测由服务端发起，\
                 一条一条来，几十条要等一会儿。"
            }

            div { class: "form-actions start",
                button {
                    class: "btn-ghost",
                    r#type: "button",
                    disabled: checking(),
                    onclick: run,
                    if checking() { "检测中…" } else { "检测全部链接" }
                }
                if results().is_some() {
                    button {
                        class: "btn-ghost",
                        r#type: "button",
                        disabled: checking(),
                        onclick: clear,
                        "清除检测结果"
                    }
                }
            }

            if let Some(message) = error() {
                p { class: "form-error", "{message}" }
            }

            if checking() {
                p { class: "field-hint", "正在逐条探测…" }
            } else if let Some(list) = results() {
                if list.is_empty() {
                    p { class: "field-hint", "没有书签可查。" }
                } else {
                    p { class: "field-hint",
                        "共 {list.len()} 条：正常 {alive} 条，坏链 {broken.len()} 条，连不上 {unreachable.len()} 条。"
                    }

                    if !broken.is_empty() {
                        div { class: "form-actions start",
                            if confirming_delete() {
                                button {
                                    class: "btn-danger",
                                    r#type: "button",
                                    disabled: checking(),
                                    onclick: delete_broken,
                                    "确认移入回收站"
                                }
                                button {
                                    class: "btn-ghost",
                                    r#type: "button",
                                    onclick: move |_| confirming_delete.set(false),
                                    "取消"
                                }
                            } else {
                                button {
                                    class: "btn-danger",
                                    r#type: "button",
                                    onclick: move |_| confirming_delete.set(true),
                                    "把 {broken.len()} 条坏链移入回收站"
                                }
                            }
                        }

                        ul { class: "trash-list",
                            for item in broken {
                                li { key: "{item.id}", class: "trash-row",
                                    div { class: "trash-main",
                                        span { class: "trash-title", "{item.title}" }
                                        span { class: "trash-url", "{item.url}" }
                                        span { class: "trash-time", "{item.detail}" }
                                    }
                                }
                            }
                        }
                    }

                    if !unreachable.is_empty() {
                        p { class: "field-hint",
                            "下面这些连不上，但多半不是坏链：内网地址、需要登录的站点、\
                             或者服务器所在网络到不了的域名。留着，不提供一键清理。"
                        }
                        ul { class: "trash-list",
                            for item in unreachable {
                                li { key: "{item.id}", class: "trash-row",
                                    div { class: "trash-main",
                                        span { class: "trash-title", "{item.title}" }
                                        span { class: "trash-url", "{item.url}" }
                                        span { class: "trash-time", "{item.detail}" }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
