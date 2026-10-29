use dioxus::prelude::*;

use super::icons::{builtin_svg, builtin_value, GROUP_ICONS, ICON_PREFIX};
use crate::models::{text_icon_parts, text_icon_value, TEXT_ICON_PREFIX};

/// 文字图标可选的底色。与内置图标的线框风格搭配，深色浅色主题下都能看清白字。
const TEXT_COLORS: [&str; 8] = [
    "#5b8cff", "#8b5cf6", "#ec4899", "#ef4444", "#b45309", "#047857", "#0f766e", "#64748b",
];

/// 图标选择器。分组设置与书签表单共用同一份实现，免得两处样式与行为漂移。
///
/// 取值约定（与 `models::text_icon_parts` 等一致）：空串 = 自动 / 首字，
/// `i:xxx` = 内置图标，`t:#rrggbb:字` = 文字图标，`http…` / `/…` = 图片地址。
///
/// 组件是**受控**的：值只由父级持有，这里不存副本。早先存过本地草稿，结果父级
/// 换成另一条记录的值时界面不同步——受控写法没有这个问题。
#[component]
pub fn IconPicker(
    value: String,
    on_change: EventHandler<String>,
    /// 给"自动"那一档的说明文字；传 `None` 表示不提供这一档（分组没有可代取的图标）
    #[props(default)]
    auto_label: Option<String>,
) -> Element {
    // 只有"用户点了某一档、但值还没填"的情况需要本地状态；
    // 值本身就是对应格式时直接按那一档显示，不必额外记。
    let mut custom_clicked = use_signal(|| false);
    let mut text_clicked = use_signal(|| false);

    let is_text = value.starts_with(TEXT_ICON_PREFIX);
    // 提前算成 bool：`value` 会被多个 onclick 闭包按需捕获，直接在里面用它会被移走。
    let value_is_builtin = value.starts_with(ICON_PREFIX);
    let value_is_text = is_text;
    let text = text_icon_parts(&value);
    let custom = !is_text && !value.starts_with(ICON_PREFIX) && !value.is_empty()
        || (!is_text && custom_clicked());
    let show_text = text_clicked() || is_text;

    let is_builtin = value.is_empty() || value.starts_with(ICON_PREFIX);
    let builtin_active = if auto_label.is_some() {
        // 有"自动"档时，内置档只在选了具体内置图标时高亮
        is_builtin && !value.is_empty()
    } else {
        is_builtin
    };

    // 文字档的草稿：已选颜色/文字优先，否则给一组可用的默认值。
    let text_color = text
        .as_ref()
        .map(|(color, _)| color.clone())
        .unwrap_or_else(|| TEXT_COLORS[0].to_string());
    let text_chars = text
        .as_ref()
        .map(|(_, chars)| chars.clone())
        .unwrap_or_else(|| "字".to_string());
    let mut text_color_signal = use_signal(|| text_color.clone());
    let mut text_chars_signal = use_signal(|| text_chars.clone());

    let on_change = on_change;

    rsx! {
        div { class: "segmented wrap",
            if let Some(label) = auto_label.clone() {
                button {
                    class: if value.is_empty() { "segment active" } else { "segment" },
                    r#type: "button",
                    onclick: move |_| {
                        custom_clicked.set(false);
                        text_clicked.set(false);
                        on_change.call(String::new());
                    },
                    "{label}"
                }
            }
            button {
                class: if builtin_active { "segment active" } else { "segment" },
                r#type: "button",
                onclick: move |_| {
                    custom_clicked.set(false);
                    text_clicked.set(false);
                    // 从别的档切过来时给个具体图标，否则这一档看起来"选了但没值"
                    if !value_is_builtin {
                        on_change.call(builtin_value("link"));
                    }
                },
                "内置图标"
            }
            button {
                class: if show_text { "segment active" } else { "segment" },
                r#type: "button",
                onclick: move |_| {
                    custom_clicked.set(false);
                    text_clicked.set(true);
                    if !value_is_text {
                        let next = text_icon_value(&text_color_signal(), &text_chars_signal())
                            .unwrap_or_else(|| format!("{TEXT_ICON_PREFIX}{}:字", TEXT_COLORS[0]));
                        on_change.call(next);
                    }
                },
                "文字"
            }
            button {
                class: if custom { "segment active" } else { "segment" },
                r#type: "button",
                onclick: move |_| {
                    text_clicked.set(false);
                    custom_clicked.set(true);
                    // 从文字/内置档切过来时把值清掉：否则地址输入框里会留着
                    // `t:#…` 或 `i:…` 这种不是地址的东西，看起来像"填错了"。
                    if value_is_text || value_is_builtin {
                        on_change.call(String::new());
                    }
                },
                "自定义地址"
            }
        }

        if show_text {
            div { class: "text-icon-editor",
                div { class: "text-icon-preview",
                    span {
                        class: "group-icon group-icon-text",
                        style: "background: {text_color_signal()};",
                        "{text_chars_signal()}"
                    }
                }
                label { class: "field",
                    span { class: "field-label", "文字（最多 2 个）" }
                    input {
                        class: "input",
                        maxlength: "2",
                        value: "{text_chars_signal()}",
                        oninput: move |e| {
                            let chars: String = e.value().chars().take(2).collect();
                            text_chars_signal.set(chars.clone());
                            if chars.trim().is_empty() {
                                // 清空文字＝回到"自动/首字"，而不是保留一个看不见的旧图标
                                text_clicked.set(false);
                                on_change.call(String::new());
                            } else if let Some(next) =
                                text_icon_value(&text_color_signal(), &chars)
                            {
                                on_change.call(next);
                            }
                        },
                    }
                }
                div { class: "field",
                    span { class: "field-label", "底色" }
                    div { class: "color-swatches",
                        for color in TEXT_COLORS {
                            button {
                                key: "{color}",
                                class: if text_color_signal() == color { "color-swatch active" } else { "color-swatch" },
                                r#type: "button",
                                title: "{color}",
                                style: "background: {color};",
                                onclick: move |_| {
                                    text_color_signal.set(color.to_string());
                                    if let Some(next) = text_icon_value(color, &text_chars_signal()) {
                                        on_change.call(next);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        } else if custom {
            input {
                class: "input",
                placeholder: "https://example.com/icon.png",
                value: "{value}",
                oninput: move |e| on_change.call(e.value()),
            }
        } else {
            div { class: "icon-grid",
                button {
                    class: if value.is_empty() { "icon-cell active" } else { "icon-cell" },
                    r#type: "button",
                    title: if auto_label.is_some() {
                        "自动：取站点图标，取不到回退名称首字"
                    } else {
                        "不用图标（显示名称首字）"
                    },
                    onclick: move |_| on_change.call(String::new()),
                    span { class: "icon-cell-text",
                        if auto_label.is_some() { "自动" } else { "首字" }
                    }
                }

                for (id, label, _) in GROUP_ICONS {
                    button {
                        key: "{id}",
                        class: if value == builtin_value(id) { "icon-cell active" } else { "icon-cell" },
                        r#type: "button",
                        title: "{label}",
                        onclick: move |_| on_change.call(builtin_value(id)),
                        if let Some(svg) = builtin_svg(id) {
                            {svg}
                        }
                    }
                }
            }
        }
    }
}

/// 当前值是否属于"自定义地址"档（非空、且不是内置图标或文字图标）。
#[cfg_attr(not(test), allow(dead_code))]
fn is_custom(value: &str) -> bool {
    !value.is_empty() && !value.starts_with(ICON_PREFIX) && !value.starts_with(TEXT_ICON_PREFIX)
}

#[cfg(test)]
mod tests {
    use super::is_custom;

    #[test]
    fn recognises_custom_values() {
        assert!(!is_custom(""), "空值是自动档");
        assert!(!is_custom("i:code"), "内置图标不是自定义");
        assert!(!is_custom("t:#5b8cff:猫"), "文字图标不是自定义");
        assert!(is_custom("https://a.com/i.png"));
        assert!(is_custom("/wallpaper/x.png"));
    }
}
