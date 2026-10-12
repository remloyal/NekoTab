use dioxus::prelude::*;

use super::engine_logo::engine_logo;
use crate::engine::build_search_url;
use crate::models::Engine;

/// 在新标签页打开地址。
///
/// 不能用原生表单 GET 来代替：按 HTML 规范，表单提交会**替换** action URL 里
/// 原有的查询串，搜索词会被清掉（实测拿到的是 `.../s?`）。而且各引擎的参数名
/// 不同（百度 `wd`、谷歌 `q`），还有把词放在路径里的模板，原生表单无法通用。
#[cfg(feature = "web")]
fn open_in_new_tab(url: &str) {
    if let Some(window) = web_sys::window() {
        // 由用户手势（点击/回车）触发，不会被弹窗拦截
        let _ = window.open_with_url_and_target(url, "_blank");
    }
}

#[cfg(not(feature = "web"))]
fn open_in_new_tab(_url: &str) {}

/// 顶部搜索框：左侧选搜索引擎，回车或点箭头跳到该引擎的结果页。
///
/// 输入仍实时回传给父级用于筛选书签，所以"打字筛书签、回车搜网页"两种用法
/// 同时成立。提交走原生 GET 表单（`action` 里已含替换好的搜索词、输入框不带
/// `name`），因此不需要任何 JS 跳转代码。
#[component]
pub fn SearchBar(
    value: String,
    oninput: EventHandler<String>,
    engines: Vec<Engine>,
    on_select_engine: EventHandler<i64>,
    on_open_settings: EventHandler<()>,
) -> Element {
    let mut menu_open = use_signal(|| false);

    let active = engines
        .iter()
        .find(|engine| engine.active)
        .cloned()
        .or_else(|| engines.first().cloned());

    let action = active
        .as_ref()
        .map(|engine| build_search_url(&engine.url, &value))
        .unwrap_or_default();

    rsx! {
        form {
            class: "search-form",
            // action/target 只作为"JS 没跑起来"时的静态兜底；正常路径走下面的 onsubmit
            action: "{action}",
            method: "GET",
            target: "_blank",
            onsubmit: {
                let target_url = action.clone();
                move |evt| {
                    // 拦掉原生提交（它会把查询串清空），改由 JS 精确打开
                    evt.prevent_default();
                    open_in_new_tab(&target_url);
                }
            },

            // 点击别处收起下拉
            if menu_open() {
                div {
                    class: "engine-backdrop",
                    onclick: move |_| menu_open.set(false),
                }
            }

            div { class: "search-box",
                div { class: "engine",
                    button {
                        class: "engine-btn",
                        r#type: "button",
                        title: "选择搜索引擎",
                        onclick: move |_| menu_open.set(!menu_open()),
                        if let Some(engine) = active.clone() {
                            {engine_logo(&engine)}
                        }
                        span { class: "engine-caret", "▾" }
                    }

                    if menu_open() {
                        div { class: "engine-menu",
                            for engine in engines.clone() {
                                button {
                                    class: if engine.active { "engine-item active" } else { "engine-item" },
                                    r#type: "button",
                                    onclick: {
                                        let id = engine.id;
                                        move |_| {
                                            on_select_engine.call(id);
                                            menu_open.set(false);
                                        }
                                    },
                                    {engine_logo(&engine)}
                                    span { class: "engine-name", "{engine.name}" }
                                    if engine.active {
                                        span { class: "engine-check", "✓" }
                                    }
                                }
                            }

                            if !engines.is_empty() {
                                div { class: "engine-sep" }
                            }

                            button {
                                class: "engine-item",
                                r#type: "button",
                                onclick: move |_| {
                                    menu_open.set(false);
                                    on_open_settings.call(());
                                },
                                span { class: "engine-logo engine-logo-gear", "⚙" }
                                span { class: "engine-name", "设置" }
                            }
                        }
                    }
                }

                input {
                    class: "search",
                    r#type: "search",
                    autofocus: true,
                    autocomplete: "off",
                    // 故意不给 name：提交时不会往 action 上再拼参数
                    placeholder: "搜一搜，看一看",
                    value: "{value}",
                    oninput: move |evt| oninput.call(evt.value()),
                }

                button {
                    class: "search-go",
                    r#type: "submit",
                    title: "搜索",
                    disabled: active.is_none(),
                    "→"
                }
            }
        }
    }
}
