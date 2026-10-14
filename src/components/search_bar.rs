use dioxus::prelude::*;

use super::engine_logo::engine_logo;
#[cfg(feature = "web")]
use crate::backend::suggest;
use crate::engine::build_search_url;
use crate::models::Engine;

/// 输入停止后多久才去要联想。太短会把每个字母都发出去，太长会觉得迟钝。
/// 只在 web 侧用得上（防抖靠 gloo-timers）。
#[cfg(feature = "web")]
const SUGGEST_DEBOUNCE_MS: u32 = 180;

/// 输入变化后，防抖去要一次引擎联想（仅 web）。
///
/// `token` 是防抖令牌：每次输入 +1，晚回来的响应发现令牌变了就丢掉——联想请求的
/// 耗时不可控（本机 0.2 秒、跨网可能 2 秒），不这么做，慢的那次会把新结果盖掉。
#[cfg(feature = "web")]
fn kick_suggest(
    engine_id: i64,
    query: String,
    my_token: u32,
    token: Signal<u32>,
    mut remote: Signal<Vec<String>>,
) {
    spawn(async move {
        gloo_timers::future::TimeoutFuture::new(SUGGEST_DEBOUNCE_MS).await;
        if token() != my_token {
            return;
        }
        if let Ok(list) = suggest(engine_id, query).await {
            if token() == my_token {
                remote.set(list);
            }
        }
    });
}

#[cfg(not(feature = "web"))]
fn kick_suggest(
    _engine_id: i64,
    _query: String,
    _my_token: u32,
    _token: Signal<u32>,
    _remote: Signal<Vec<String>>,
) {
}

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
pub const SEARCH_INPUT_ID: &str = "nekotab-search-input";

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
///
/// 「搜网页」模式下输入会带出一张**引擎联想**面板（点一下拿那个词去搜）。
/// 只有「搜网页」模式有：筛书签模式里引擎不参与，而且下方网格本来就在实时筛。
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
    // 引擎给的联想词
    let mut remote = use_signal(Vec::<String>::new);
    // 防抖令牌，见 kick_suggest
    let mut token = use_signal(|| 0u32);
    // 上下键高亮到第几行
    let mut highlight = use_signal(|| Option::<usize>::None);
    // 被主动收起过（Esc / 选完一项 / 焦点离开）：在输入再次变化前不再弹出来
    let mut dismissed = use_signal(|| false);

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

    // ---- 联想面板 ----
    let items: Vec<String> = remote();
    let show_panel =
        mode == SearchMode::Web && !dismissed() && !value.trim().is_empty() && !items.is_empty();

    // 闭包体里把 Signal 拷成局部再 set：Signal 是 Copy，这样闭包本身就只是 `Fn`
    // （而不是 `FnMut`），才能在多个事件回调里反复调用而不用到处 `mut`/克隆。
    let dismiss = move || {
        let mut dismissed = dismissed;
        let mut highlight = highlight;
        dismissed.set(true);
        highlight.set(None);
    };

    // 选了某个联想词：拿它去搜（一律开新标签页，别顶掉当前书签页）
    let engine_for_pick = submit_engine.clone();
    let pick = move |word: String| {
        let mut dismissed = dismissed;
        let mut highlight = highlight;
        dismissed.set(true);
        highlight.set(None);
        if let Some(engine) = engine_for_pick.as_ref() {
            let url = build_search_url(&engine.url, &word);
            crate::dom::open_url(&url, true);
        }
    };

    let engine_for_suggest = submit_engine.clone();

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
                    oninput: move |evt| {
                        let text = evt.value();
                        oninput.call(text.clone());

                        // 输入变了：旧面板状态作废，重新要一次联想
                        highlight.set(None);
                        dismissed.set(false);
                        remote.set(Vec::new());
                        let next = token() + 1;
                        token.set(next);

                        let query = text.trim().to_string();
                        if mode == SearchMode::Web && !query.is_empty() {
                            if let Some(engine) = engine_for_suggest.as_ref() {
                                kick_suggest(engine.id, query, next, token, remote);
                            }
                        }
                    },
                    onkeydown: {
                        let items = items.clone();
                        let pick = pick.clone();
                        move |evt| match evt.key() {
                            // 上下键在联想里移动高亮；没有联想时不要拦，
                            // 让方向键继续去做翻页（根节点的快捷键）
                            Key::ArrowDown if show_panel => {
                                evt.prevent_default();
                                let next = match highlight() {
                                    Some(index) => (index + 1).min(items.len() - 1),
                                    None => 0,
                                };
                                highlight.set(Some(next));
                            }
                            Key::ArrowUp if show_panel => {
                                evt.prevent_default();
                                highlight.set(Some(match highlight() {
                                    Some(0) | None => 0,
                                    Some(index) => index - 1,
                                }));
                            }
                            Key::Enter if show_panel => {
                                if let Some(index) = highlight() {
                                    // 拦掉表单提交，改用高亮那一项
                                    evt.prevent_default();
                                    pick(items[index].clone());
                                }
                            }
                            Key::Escape if show_panel => {
                                // 只收面板，不要让根节点的 Esc 再关一层弹窗
                                evt.prevent_default();
                                evt.stop_propagation();
                                dismiss();
                            }
                            _ => {}
                        }
                    },
                    // 焦点离开就收起。面板里那些行在 mousedown 时拦了默认行为，
                    // 所以点它们不会先触发这里的 blur。
                    onblur: move |_| dismiss(),
                }

                if !value.is_empty() {
                    button {
                        class: "search-go",
                        r#type: "button",
                        title: "清空",
                        onclick: move |_| {
                            oninput.call(String::new());
                            remote.set(Vec::new());
                            token.set(token() + 1);
                            dismiss();
                        },
                        "✕"
                    }
                }

                button {
                    class: "mode-toggle",
                    r#type: "button",
                    title: if mode == SearchMode::Web { "切换到「筛选书签」" } else { "切换到「搜网页」" },
                    onclick: move |_| {
                        on_mode_change.call(mode.toggled());
                        remote.set(Vec::new());
                        token.set(token() + 1);
                        dismiss();
                    },
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

                // 联想面板。挂在 .search-box 里（它是 position: relative），
                // 绝对定位吊在整条搜索框下面。
                if show_panel {
                    div { class: "suggest-panel",
                        for (index, item) in items.iter().enumerate() {
                            {
                                let item = item.clone();
                                rsx! {
                                    button {
                                        key: "{index}",
                                        class: if highlight() == Some(index) { "suggest-item active" } else { "suggest-item" },
                                        r#type: "button",
                                        // 拦住默认行为，输入框就不会先 blur —— 否则面板会
                                        // 在 click 之前先收起，点了个空
                                        onmousedown: move |evt| evt.prevent_default(),
                                        onclick: {
                                            let item = item.clone();
                                            let pick = pick.clone();
                                            move |_| pick(item.clone())
                                        },
                                        span { class: "suggest-icon", "⌕" }
                                        span { class: "suggest-text", "{item}" }
                                    }
                                }
                            }
                        }
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
