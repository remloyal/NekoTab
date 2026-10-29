use dioxus::prelude::*;

use super::engine_logo::engine_logo;
use crate::backend::{add_engine, error_text, update_engine};
use crate::models::{Engine, NewEngine};

/// 内置常用引擎：`(名称, 地址, 图标, 前缀)`。
///
/// 图标留空走名称首字——搜索引擎不在书签表里，`/icon/{host}` 只服务已收藏站点，
/// 拿不到它们的站点图标。比起外链一个第三方图标，首字圆点离线可用且不会失效。
const ENGINE_PRESETS: [(&str, &str, &str, &str); 9] = [
    ("百度", "https://www.baidu.com/s?wd=%s", "", "bd"),
    ("必应", "https://www.bing.com/search?q=%s", "", "bing"),
    ("Google", "https://www.google.com/search?q=%s", "", "gg"),
    ("搜狗", "https://www.sogou.com/web?query=%s", "", "sg"),
    ("360", "https://www.so.com/s?q=%s", "", "360"),
    (
        "开发者搜索",
        "https://kaifa.baidu.com/searchPage?module=SEARCH&wd=%s",
        "",
        "dev",
    ),
    (
        "B站",
        "https://search.bilibili.com/all?keyword=%s",
        "",
        "bili",
    ),
    ("微博", "https://s.weibo.com/weibo?q=%s", "", "wb"),
    ("DuckDuckGo", "https://duckduckgo.com/?q=%s", "", "ddg"),
];

/// 搜索引擎设置（弹窗内容）：上方新增/编辑表单，下方已有引擎卡片列表。
#[component]
pub fn EngineSettings(
    engines: Vec<Engine>,
    on_select: EventHandler<i64>,
    on_delete: EventHandler<i64>,
    on_added: EventHandler<()>,
) -> Element {
    let mut name = use_signal(String::new);
    let mut url = use_signal(String::new);
    let mut icon = use_signal(String::new);
    let mut prefix = use_signal(String::new);
    // `Some(id)` 表示正在编辑那张卡片；表单提交走 update 而不是 add。
    let mut editing = use_signal(|| Option::<i64>::None);
    let mut busy = use_signal(|| false);
    let mut error = use_signal(|| Option::<String>::None);

    let mut reset = move || {
        name.set(String::new());
        url.set(String::new());
        icon.set(String::new());
        prefix.set(String::new());
        editing.set(None);
    };

    let submit = move |_| {
        if busy() {
            return;
        }

        let input = NewEngine {
            name: name(),
            url: url(),
            icon: icon(),
            prefix: prefix(),
        };
        let target = editing();

        spawn(async move {
            busy.set(true);
            error.set(None);

            let result = match target {
                Some(id) => update_engine(id, input).await,
                None => add_engine(input).await.map(|_| ()),
            };

            match result {
                Ok(()) => {
                    reset();
                    busy.set(false);
                    on_added.call(());
                }
                Err(e) => {
                    error.set(Some(error_text(&e)));
                    busy.set(false);
                }
            }
        });
    };

    let submit_label = if editing().is_some() {
        "保存修改"
    } else {
        "新增"
    };

    rsx! {
        form {
            class: "form",
            onsubmit: move |evt| {
                evt.prevent_default();
                submit(());
            },

            if editing().is_some() {
                div { class: "editing-banner",
                    span { "正在修改「{name}」" }
                    button {
                        class: "btn-ghost",
                        r#type: "button",
                        onclick: move |_| {
                            reset();
                            error.set(None);
                        },
                        "取消"
                    }
                }
            }

            label { class: "field",
                span { class: "field-label", "名称" }
                input {
                    class: "input",
                    placeholder: "搜索引擎名称",
                    value: "{name}",
                    oninput: move |e| name.set(e.value()),
                }
            }

            label { class: "field",
                span { class: "field-label", "地址（用 %s 代表搜索词）" }
                input {
                    class: "input",
                    placeholder: "https://www.example.com/search?q=%s",
                    value: "{url}",
                    oninput: move |e| url.set(e.value()),
                }
            }

            label { class: "field",
                span { class: "field-label", "图标（可选图片地址）" }
                input {
                    class: "input",
                    placeholder: "留空则用名称首字",
                    value: "{icon}",
                    oninput: move |e| icon.set(e.value()),
                }
            }

            label { class: "field",
                span { class: "field-label", "搜索前缀（可选）" }
                input {
                    class: "input",
                    placeholder: "如 gh，则输入「gh rust」用本引擎搜 rust",
                    value: "{prefix}",
                    oninput: move |e| prefix.set(e.value()),
                }
                p { class: "field-hint",
                    "只留字母、数字与连字符，会自动转小写。前缀必须单独成段（后面跟空格才生效），"
                    "因此不会把「github」误当成前缀「gh」。"
                }
            }

            if let Some(message) = error() {
                p { class: "form-error", "{message}" }
            }

            div { class: "form-actions",
                button {
                    class: "btn-primary",
                    r#type: "submit",
                    disabled: busy(),
                    if busy() { "保存中…" } else { "{submit_label}" }
                }
            }
        }

        section { class: "panel-section",
            h3 { class: "panel-heading", "常用引擎" }
            p { class: "field-hint", "点一下即添加；已经有的会标出来。" }
            div { class: "preset-chips",
                for (preset_name, preset_url, preset_icon, preset_prefix) in ENGINE_PRESETS {
                    {
                        let exists = engines
                            .iter()
                            .any(|engine| engine.name == preset_name || engine.url == preset_url);
                        let prefix_taken = !preset_prefix.is_empty()
                            && engines.iter().any(|engine| engine.prefix == preset_prefix);
                        let blocked = exists || prefix_taken;
                        rsx! {
                            button {
                                key: "{preset_name}",
                                class: "preset-chip",
                                r#type: "button",
                                disabled: blocked,
                                onclick: move |_| {
                                    if blocked {
                                        return;
                                    }
                                    spawn(async move {
                                        let input = NewEngine {
                                            name: preset_name.to_string(),
                                            url: preset_url.to_string(),
                                            icon: preset_icon.to_string(),
                                            prefix: preset_prefix.to_string(),
                                        };
                                        match add_engine(input).await {
                                            Ok(_) => on_added.call(()),
                                            Err(e) => error.set(Some(error_text(&e))),
                                        }
                                    });
                                },
                                "{preset_name}"
                                if exists {
                                    span { class: "preset-added", "已有" }
                                } else if prefix_taken {
                                    span { class: "preset-added", "前缀占用" }
                                }
                            }
                        }
                    }
                }
            }
        }

        p { class: "engine-tip",
            "点选卡片切换当前引擎；悬停右上角可删除。删除当前引擎会自动回退到第一个。"
        }

        div { class: "engine-list",
            for engine in engines {
                div {
                    class: if engine.active { "engine-card active" } else { "engine-card" },
                    onclick: {
                        let id = engine.id;
                        move |_| on_select.call(id)
                    },

                    button {
                        class: "engine-card-del",
                        r#type: "button",
                        title: "删除",
                        onclick: {
                            let id = engine.id;
                            move |evt| {
                                evt.stop_propagation();
                                on_delete.call(id);
                            }
                        },
                        "✕"
                    }

                    button {
                        class: "engine-card-edit",
                        r#type: "button",
                        title: "修改",
                        onclick: {
                            let engine = engine.clone();
                            move |evt| {
                                evt.stop_propagation();
                                name.set(engine.name.clone());
                                url.set(engine.url.clone());
                                icon.set(engine.icon.clone());
                                prefix.set(engine.prefix.clone());
                                editing.set(Some(engine.id));
                                error.set(None);
                            }
                        },
                        "编辑"
                    }

                    if engine.active {
                        span { class: "engine-badge", "✓" }
                    }

                    {engine_logo(&engine)}
                    span { class: "engine-card-name", "{engine.name}" }
                    if !engine.prefix.is_empty() {
                        code { class: "engine-prefix", "{engine.prefix}" }
                    }
                }
            }
        }
    }
}
