use dioxus::prelude::*;

use super::engine_logo::engine_logo;
use crate::backend::{add_engine, error_text};
use crate::models::{Engine, NewEngine};

/// 搜索引擎设置（弹窗内容）：上方新增表单，下方已有引擎卡片列表。
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
    let mut busy = use_signal(|| false);
    let mut error = use_signal(|| Option::<String>::None);

    let submit = move |_| {
        if busy() {
            return;
        }

        let input = NewEngine {
            name: name(),
            url: url(),
            icon: icon(),
        };

        spawn(async move {
            busy.set(true);
            error.set(None);

            match add_engine(input).await {
                Ok(_) => {
                    name.set(String::new());
                    url.set(String::new());
                    icon.set(String::new());
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

    rsx! {
        form {
            class: "form",
            onsubmit: move |evt| {
                evt.prevent_default();
                submit(());
            },

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

            if let Some(message) = error() {
                p { class: "form-error", "{message}" }
            }

            div { class: "form-actions",
                button {
                    class: "btn-primary",
                    r#type: "submit",
                    disabled: busy(),
                    if busy() { "保存中…" } else { "新增" }
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

                    if engine.active {
                        span { class: "engine-badge", "✓" }
                    }

                    {engine_logo(&engine)}
                    span { class: "engine-card-name", "{engine.name}" }
                }
            }
        }
    }
}
