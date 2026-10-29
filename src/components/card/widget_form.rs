use dioxus::prelude::*;

use crate::backend::{add_bookmark, error_text, update_bookmark};
use crate::models::{
    clamp_span, widget_label, AddOutcome, Bookmark, CalendarConfig, ClockConfig, DialConfig,
    HotSearchConfig, NewBookmark, NoteConfig, TodoConfig, TodoItem, WeatherConfig, CARD_SURFACES,
    HOT_SOURCES, KIND_CALENDAR, KIND_CLOCK, KIND_DIAL, KIND_HOT, KIND_NOTE, KIND_TODO,
    KIND_WEATHER, NOTE_MAX_CHARS, SPAN_MAX, SPAN_PRESETS, TODO_MAX_ITEMS, TODO_TEXT_MAX_CHARS,
    WEATHER_MAX_LOCATION_CHARS,
};

/// 组件卡片的表单，作为弹窗内容使用。
///
/// `card` 为 `None` 时是新增（种类取 `kind`），为 `Some` 时是修改。
///
/// 与 `BookmarkForm` 分成两个组件，而不是在它里面按种类分支：两者的字段几乎没有
/// 交集（组件没有网址、分组、标签、图标），合在一起会让每个字段都挂一个"当前是
/// 什么卡片"的判断，而那种判断正是以后加字段时最容易漏的地方。
#[component]
pub fn WidgetForm(
    card: Option<Bookmark>,
    /// 新增时的种类；修改时以 `card.kind` 为准
    kind: String,
    on_saved: EventHandler<()>,
    on_cancel: EventHandler<()>,
) -> Element {
    let existing = card.clone();
    let kind = existing
        .as_ref()
        .map(|card| card.kind.clone())
        .unwrap_or(kind);
    let editing_id = existing.as_ref().map(|card| card.id);
    let label = widget_label(&kind);
    let submit_label = if editing_id.is_some() {
        "保存修改"
    } else {
        "添加"
    };

    // 参数按种类还原；认不出来的种类写法回落默认值。三种参数各解一遍——
    // 只有与当前 kind 对上的那一份会被读到，但都解一次的代价可以忽略，
    // 而好处是这里的信号初始化不必再按 kind 分叉。
    let raw_config = existing
        .as_ref()
        .map(|card| card.config.as_str())
        .unwrap_or("");
    let clock = ClockConfig::parse(raw_config);
    let dial = DialConfig::parse(raw_config);
    let note = NoteConfig::parse(raw_config);
    let weather = WeatherConfig::parse(raw_config);
    let calendar = CalendarConfig::parse(raw_config);
    let todo = TodoConfig::parse(raw_config);
    let hot = HotSearchConfig::parse(raw_config);

    let mut title = use_signal(|| {
        existing
            .as_ref()
            .map(|card| card.title.clone())
            .unwrap_or_else(|| label.to_string())
    });
    let mut hour12 = use_signal(|| clock.hour12);
    // 两种时钟都有"秒"这一个开关，是同一个概念，共用一个信号；取哪一份看是什么组件
    let seconds_default = if kind == KIND_DIAL {
        dial.show_seconds
    } else {
        clock.show_seconds
    };
    let mut show_seconds = use_signal(|| seconds_default);
    // 「数字刻度」只有表盘有
    let mut show_numbers = use_signal(|| dial.show_numbers);
    let mut text = use_signal(|| note.text.clone());
    let mut location = use_signal(|| weather.location.clone());
    let mut fahrenheit = use_signal(|| weather.fahrenheit);
    let mut cal_lunar = use_signal(|| calendar.show_lunar);
    let mut cal_monday = use_signal(|| calendar.week_start_monday);
    let mut hot_source = use_signal(|| hot.source.clone());
    // 待办用"一行一条"的文本编辑：`[x] ` 前缀表示已完成。比逐个条目的输入框
    // 更容易批量粘贴，也和记事本的多行文本是一套肌肉记忆。
    let mut todo_lines = use_signal(|| {
        todo.items
            .iter()
            .map(|item| {
                if item.done {
                    format!("[x] {}", item.text)
                } else {
                    item.text.clone()
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    });
    // 组件默认占 2×2：它就是要显示内容的，一格放不下
    let mut span_w = use_signal(|| clamp_span(existing.as_ref().map(|c| c.span_w).unwrap_or(2)));
    let mut span_h = use_signal(|| clamp_span(existing.as_ref().map(|c| c.span_h).unwrap_or(2)));
    // 底色。空串是"没设过"，也就是选卡面板上那个「默认」——组件卡的默认是"半透明"，
    // 由 models::effective_surface 在渲染时解析，这里存的仍是用户的原始选择。
    let mut surface = use_signal(|| {
        existing
            .as_ref()
            .map(|card| card.surface.clone())
            .unwrap_or_default()
    });
    let mut busy = use_signal(|| false);
    let mut error = use_signal(|| Option::<String>::None);

    // 组件上不展示、但 update 会整行覆盖的字段：原样带回去，免得编辑一次就被清空
    let keep_description = existing
        .as_ref()
        .map(|card| card.description.clone())
        .unwrap_or_default();
    let keep_icon = existing
        .as_ref()
        .map(|card| card.icon.clone())
        .unwrap_or_default();

    // 提交闭包要拿走一份 kind，而下面渲染时还要用它判断显示哪一组参数
    let submit_kind = kind.clone();

    let submit = move |_| {
        if busy() {
            return;
        }

        let input = NewBookmark {
            title: title(),
            // 组件没有网址：服务端按 kind 走另一条校验，不要求 http(s)
            url: String::new(),
            description: keep_description.clone(),
            tags: String::new(),
            group_id: None,
            group_ids: Vec::new(),
            icon: keep_icon.clone(),
            span_w: span_w(),
            span_h: span_h(),
            kind: submit_kind.clone(),
            config: match submit_kind.as_str() {
                KIND_CLOCK => ClockConfig {
                    hour12: hour12(),
                    show_seconds: show_seconds(),
                }
                .to_json(),
                KIND_DIAL => DialConfig {
                    show_seconds: show_seconds(),
                    show_numbers: show_numbers(),
                }
                .to_json(),
                KIND_NOTE => NoteConfig { text: text() }.to_json(),
                KIND_WEATHER => WeatherConfig {
                    location: location(),
                    fahrenheit: fahrenheit(),
                }
                .to_json(),
                KIND_CALENDAR => CalendarConfig {
                    show_lunar: cal_lunar(),
                    week_start_monday: cal_monday(),
                }
                .to_json(),
                KIND_TODO => TodoConfig {
                    items: parse_todo_lines(&todo_lines()),
                }
                .to_json(),
                KIND_HOT => HotSearchConfig {
                    source: hot_source(),
                }
                .to_json(),
                _ => String::new(),
            },
            surface: surface(),
        };

        spawn(async move {
            busy.set(true);
            error.set(None);

            let outcome = match editing_id {
                Some(id) => update_bookmark(id, input).await.map(|_| AddOutcome {
                    id,
                    created: true,
                    in_trash: false,
                }),
                None => add_bookmark(input).await,
            };

            match outcome {
                Ok(outcome) if outcome.created => {
                    busy.set(false);
                    on_saved.call(());
                }
                // 同一种组件已经有了一张：不关弹窗，说清楚是哪一张挡住了
                Ok(outcome) => {
                    error.set(Some(if outcome.in_trash {
                        format!("已经有一个「{label}」在回收站里，先去还原或彻底删除。")
                    } else {
                        format!("已经有一个「{label}」了，同一种组件只需要一个。")
                    }));
                    busy.set(false);
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
                    autofocus: true,
                    value: "{title}",
                    oninput: move |e| title.set(e.value()),
                }
                p { class: "field-hint", "鼠标停在卡片上时显示的名字。" }
            }

            if kind == KIND_CLOCK {
                div { class: "field",
                    span { class: "field-label", "显示" }
                    label { class: "switch",
                        input {
                            r#type: "checkbox",
                            checked: hour12(),
                            onchange: move |e| hour12.set(e.checked()),
                        }
                        span { "12 小时制" }
                    }
                    label { class: "switch",
                        input {
                            r#type: "checkbox",
                            checked: show_seconds(),
                            onchange: move |e| show_seconds.set(e.checked()),
                        }
                        span { "显示秒" }
                    }
                    p { class: "field-hint",
                        "小格子里会只留时间；高度够（跨行）时才显示日期。"
                    }
                }
            }

            if kind == KIND_DIAL {
                div { class: "field",
                    span { class: "field-label", "显示" }
                    label { class: "switch",
                        input {
                            r#type: "checkbox",
                            checked: show_seconds(),
                            onchange: move |e| show_seconds.set(e.checked()),
                        }
                        span { "秒针" }
                    }
                    label { class: "switch",
                        input {
                            r#type: "checkbox",
                            checked: show_numbers(),
                            onchange: move |e| show_numbers.set(e.checked()),
                        }
                        span { "数字刻度" }
                    }
                    p { class: "field-hint",
                        "关掉秒针只剩时针与分针；关掉数字刻度就是 12 条短线。表盘按格子的短边等比缩放。"
                    }
                }
            }

            if kind == KIND_WEATHER {
                label { class: "field",
                    span { class: "field-label", "地点" }
                    input {
                        class: "input",
                        maxlength: "{WEATHER_MAX_LOCATION_CHARS}",
                        placeholder: "例如：北京、Shanghai",
                        value: "{location}",
                        oninput: move |e| location.set(e.value()),
                    }
                    p { class: "field-hint",
                        "填地名就行，经纬度由服务端去查。数据源是 Open-Meteo（不需要密钥），"
                        "所以需要服务端能出网；取不到时卡片上会直说原因。"
                    }
                }
                div { class: "field",
                    span { class: "field-label", "单位" }
                    label { class: "switch",
                        input {
                            r#type: "checkbox",
                            checked: fahrenheit(),
                            onchange: move |e| fahrenheit.set(e.checked()),
                        }
                        span { "华氏度" }
                    }
                }
            }

            if kind == KIND_NOTE {
                label { class: "field",
                    span { class: "field-label", "内容" }
                    textarea {
                        class: "input textarea",
                        rows: "6",
                        maxlength: "{NOTE_MAX_CHARS}",
                        placeholder: "写点什么……换行会照原样保留。",
                        value: "{text}",
                        oninput: move |e| text.set(e.value()),
                    }
                    p { class: "field-hint",
                        "最多 {NOTE_MAX_CHARS} 字。超出格子高度的部分会裁掉，放大格子（跨格）能多显示一些。"
                    }
                }
            }

            if kind == KIND_CALENDAR {
                div { class: "field",
                    span { class: "field-label", "显示" }
                    label { class: "switch",
                        input {
                            r#type: "checkbox",
                            checked: cal_lunar(),
                            onchange: move |e| cal_lunar.set(e.checked()),
                        }
                        span { "显示今天的农历" }
                    }
                    label { class: "switch",
                        input {
                            r#type: "checkbox",
                            checked: cal_monday(),
                            onchange: move |e| cal_monday.set(e.checked()),
                        }
                        span { "一周从周一开始" }
                    }
                    p { class: "field-hint",
                        "卡面上可用 ‹ › 切换月份；窗口太窄时日期网格会按格子大小自动缩排。"
                    }
                }
            }

            if kind == KIND_TODO {
                label { class: "field",
                    span { class: "field-label", "待办（一行一条）" }
                    textarea {
                        class: "input textarea",
                        rows: "8",
                        placeholder: "买菜\n写完周报\n[x] 已经做完的事",
                        value: "{todo_lines}",
                        oninput: move |e| todo_lines.set(e.value()),
                    }
                    p { class: "field-hint",
                        "最多 {TODO_MAX_ITEMS} 条，每条 {TODO_TEXT_MAX_CHARS} 字。"
                        "行首写 [x] 表示已完成；卡片上也可以直接勾选。"
                    }
                }
            }

            if kind == KIND_HOT {
                div { class: "field",
                    span { class: "field-label", "数据源" }
                    div { class: "segmented wrap",
                        for (id, name) in HOT_SOURCES {
                            button {
                                key: "{id}",
                                class: if hot_source() == id { "segment active" } else { "segment" },
                                r#type: "button",
                                onclick: move |_| hot_source.set(id.to_string()),
                                "{name}"
                            }
                        }
                    }
                    p { class: "field-hint",
                        "由服务端代取并缓存十分钟，卡片每十分钟刷新一次；取不到时会直接写明原因。"
                    }
                }
            }

            div { class: "field",
                span { class: "field-label", "格子大小" }
                div { class: "segmented wrap",
                    for (w, h) in SPAN_PRESETS {
                        button {
                            key: "{w}x{h}",
                            class: if span_w() == w && span_h() == h { "segment active" } else { "segment" },
                            r#type: "button",
                            onclick: move |_| {
                                span_w.set(w);
                                span_h.set(h);
                            },
                            "{w}×{h}"
                        }
                    }
                }
                p { class: "field-hint",
                    "组件默认占 2×2。最大 {SPAN_MAX}×{SPAN_MAX}；窗口太窄放不下时宽度那一维会自动收回一格。"
                }
            }

            div { class: "field",
                span { class: "field-label", "卡片底色" }
                div { class: "segmented wrap",
                    for (id, label) in CARD_SURFACES {
                        button {
                            key: "{label}",
                            class: if surface() == id { "segment active" } else { "segment" },
                            r#type: "button",
                            onclick: move |_| surface.set(id.to_string()),
                            "{label}"
                        }
                    }
                }
                p { class: "field-hint",
                    "组件卡默认带一层半透明的底：它的内容是文字与细线，压在花哨壁纸上会看不清。"
                    "底色跟着主题走（深色主题深底浅字、浅色主题白底深字），不用自己配色。"
                }
            }

            if let Some(message) = error() {
                p { class: "form-error", "{message}" }
            }

            div { class: "form-actions",
                button {
                    class: "btn-ghost",
                    r#type: "button",
                    onclick: move |_| on_cancel.call(()),
                    "取消"
                }
                button {
                    class: "btn-primary",
                    r#type: "submit",
                    disabled: busy(),
                    if busy() { "保存中…" } else { "{submit_label}" }
                }
            }
        }
    }
}

/// 把卡片表单里的"一行一条"文本解析成待办列表。
///
/// 行首 `[x]` / `[X]` 表示已完成，其余按未完成；空行丢掉，按字符截断并截到上限。
/// 这是纯函数，单测能直接覆盖那条容易出错的 `[x]` 前缀逻辑。
fn parse_todo_lines(raw: &str) -> Vec<TodoItem> {
    raw.lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() {
                return None;
            }
            let (done, text) = if let Some(rest) = line
                .strip_prefix("[x]")
                .or_else(|| line.strip_prefix("[X]"))
            {
                (true, rest.trim())
            } else {
                (false, line)
            };
            let text: String = text.chars().take(TODO_TEXT_MAX_CHARS).collect();
            if text.is_empty() {
                None
            } else {
                Some(TodoItem { text, done })
            }
        })
        .take(TODO_MAX_ITEMS)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::parse_todo_lines;

    #[test]
    fn parses_todo_lines() {
        let items = parse_todo_lines("买菜\n[x] 周报\n  \n[X] 健身");
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].text, "买菜");
        assert!(!items[0].done);
        assert!(items[1].done);
        assert_eq!(items[1].text, "周报");
        assert!(items[2].done);
    }
}
