use dioxus::prelude::*;

use super::engine_logo::engine_logo;
use crate::engine::build_search_url;
use crate::models::Engine;

/// 搜索框的两种用途。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchMode {
    /// 搜网页：回车跳转到搜索引擎，**并开新标签页**（不顶掉当前书签页）
    Web,
    /// 筛书签：输入即筛下方的卡片，回车不跳转
    Filter,
}

impl SearchMode {
    pub fn label(self) -> &'static str {
        match self {
            SearchMode::Web => "网页",
            SearchMode::Filter => "书签",
        }
    }

    pub fn toggled(self) -> Self {
        match self {
            SearchMode::Web => SearchMode::Filter,
            SearchMode::Filter => SearchMode::Web,
        }
    }
}

/// 输入框的元素 id：`Ctrl+K` 与 `/` 靠它把焦点送回来。
pub const SEARCH_INPUT_ID: &str = "mtab-search-input";

/// 拆出引擎前缀。`gh rust` + 引擎前缀 `gh` → `("gh", "rust")`。
///
/// 前缀必须是**独立的一段**（后面紧跟空格），否则输入 `github` 会被误当成前缀 `gh`。
pub fn split_prefix<'a>(
    query: &'a str,
    engines: &'a [Engine],
) -> Option<(&'a Engine, &'a str)> {
    let trimmed = query.trim_start();
    let (head, rest) = trimmed.split_once(char::is_whitespace)?;
    let head = head.to_ascii_lowercase();
    if head.is_empty() {
        return None;
    }

    let engine = engines
        .iter()
        .find(|engine| !engine.prefix.is_empty() && engine.prefix == head)?;

    let rest = rest.trim_start();
    if rest.is_empty() {
        // 只输了前缀还没输关键词：不当作前缀，免得把搜索词吞掉
        return None;
    }

    Some((engine, rest))
}

/// 顶部搜索框。左侧选搜索引擎，回车或点箭头跳到该引擎的结果页。
///
/// 两种模式共用同一个输入框：切换按钮在右侧，切到「书签」时引擎选择器收起，
/// 输入实时回传给父级用于筛选卡片。
#[component]
pub fn SearchBar(
    value: String,
    oninput: EventHandler<String>,
    mode: SearchMode,
    on_mode_change: EventHandler<SearchMode>,
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

    // 前缀可以临时顶掉当前引擎，因此先把提交目标算出来，供 action 与 onsubmit 共用
    let prefixed = split_prefix(&value, &engines);
    let prefix_hint = prefixed.map(|(engine, _)| (engine.prefix.clone(), engine.name.clone()));
    let submit_engine = prefixed
        .map(|(engine, _)| engine.clone())
        .or_else(|| active.clone());
    let submit_query = prefixed
        .map(|(_, rest)| rest.to_string())
        .unwrap_or_else(|| value.clone());

    let action = if mode == SearchMode::Web {
        submit_engine
            .as_ref()
            .map(|engine| build_search_url(&engine.url, &submit_query))
            .unwrap_or_default()
    } else {
        String::new()
    };

    let can_submit = mode == SearchMode::Web && active.is_some() && !value.trim().is_empty();

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
                    if !target_url.is_empty() {
                        // 搜索一律开新标签页：用当前页去搜会把书签页顶掉
                        crate::dom::open_url(&target_url, true);
                    }
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
                if mode == SearchMode::Web {
                    div { class: "engine",
                        button {
                            class: "engine-btn",
                            r#type: "button",
                            title: "选择搜索引擎",
                            onclick: move |_| menu_open.set(!menu_open()),
                            // 前缀生效时状态就变了，得如实显示，否则用户看不出"刚才那条为什么用这个引擎"
                            if let Some(engine) = submit_engine.clone() {
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
                                        if !engine.prefix.is_empty() {
                                            code { class: "engine-prefix", "{engine.prefix}" }
                                        }
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
                } else {
                    // 筛书签模式：引擎不参与，左侧换成固定的书签标识
                    span { class: "engine-btn engine-btn-static", title: "筛选书签",
                        span { class: "engine-logo engine-logo-gear", "⌕" }
                    }
                }

                input {
                    id: SEARCH_INPUT_ID,
                    class: "search",
                    r#type: "search",
                    autofocus: true,
                    autocomplete: "off",
                    // 故意不给 name：提交时不会往 action 上再拼参数
                    placeholder: if mode == SearchMode::Web { "搜一搜，看一看" } else { "在书签里找…" },
                    value: "{value}",
                    oninput: move |evt| oninput.call(evt.value()),
                }

                if !value.is_empty() {
                    button {
                        class: "search-go",
                        r#type: "button",
                        title: "清空",
                        onclick: move |_| oninput.call(String::new()),
                        "✕"
                    }
                }

                button {
                    class: "mode-toggle",
                    r#type: "button",
                    title: if mode == SearchMode::Web { "切换到「筛选书签」" } else { "切换到「搜网页」" },
                    onclick: move |_| on_mode_change.call(mode.toggled()),
                    "{mode.label()}"
                }

                if mode == SearchMode::Web {
                    button {
                        class: "search-go",
                        r#type: "submit",
                        title: "搜索",
                        disabled: !can_submit,
                        "→"
                    }
                }
            }

            if let Some((prefix, name)) = prefix_hint {
                p { class: "search-hint", "前缀「{prefix}」生效：将用 {name} 搜索" }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn engine(name: &str, prefix: &str, active: bool) -> Engine {
        Engine {
            id: 1,
            name: name.to_string(),
            url: "https://example.com/?q=%s".to_string(),
            icon: String::new(),
            active,
            prefix: prefix.to_string(),
        }
    }

    #[test]
    fn splits_known_prefix() {
        let engines = vec![engine("GitHub", "gh", true), engine("百度", "", false)];

        let (found, rest) = split_prefix("gh rust lang", &engines).expect("前缀应命中");
        assert_eq!(found.name, "GitHub");
        assert_eq!(rest, "rust lang");

        // 大小写不敏感
        let (found, rest) = split_prefix("GH  rust", &engines).expect("前缀应命中");
        assert_eq!(found.name, "GitHub");
        assert_eq!(rest, "rust", "多余空白吃掉");
    }

    #[test]
    fn does_not_swallow_plain_words() {
        let engines = vec![engine("GitHub", "gh", true)];

        // 没有前缀的引擎不参与；`github` 不该被当成 `gh` + `ithub`
        assert!(split_prefix("github", &engines).is_none());
        assert!(split_prefix("golang 教程", &engines).is_none());
        // 只有前缀、还没输关键词时按普通搜索词处理
        assert!(split_prefix("gh", &engines).is_none());
        assert!(split_prefix("gh   ", &engines).is_none());
        // 空输入
        assert!(split_prefix("", &engines).is_none());
        assert!(split_prefix("   ", &engines).is_none());
    }

    #[test]
    fn mode_toggles() {
        assert_eq!(SearchMode::Web.toggled(), SearchMode::Filter);
        assert_eq!(SearchMode::Filter.toggled(), SearchMode::Web);
    }
}
