use dioxus::prelude::*;

use super::icons::{builtin_svg, builtin_value, GROUP_ICONS, ICON_PREFIX};

/// 图标选择器。分组设置与书签表单共用同一份实现，免得两处样式与行为漂移。
///
/// 取值约定（与 `icons` 模块一致）：空串 = 自动 / 首字，`i:xxx` = 内置图标，
/// `http…` 或 `/…` = 图片地址。
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
    // 只有"用户点了自定义地址档、但值还没填"这一种情况需要本地状态；
    // 值本身是地址时直接按地址档显示，不必额外记。
    let mut custom_clicked = use_signal(|| false);
    let custom = custom_clicked() || is_custom(&value);

    let is_builtin = value.is_empty() || value.starts_with(ICON_PREFIX);
    let builtin_active = if auto_label.is_some() {
        // 有"自动"档时，内置档只在选了具体内置图标时高亮
        is_builtin && !value.is_empty()
    } else {
        is_builtin
    };

    let on_change = on_change;

    rsx! {
        div { class: "segmented",
            if let Some(label) = auto_label.clone() {
                button {
                    class: if value.is_empty() { "segment active" } else { "segment" },
                    r#type: "button",
                    onclick: move |_| {
                        custom_clicked.set(false);
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
                    // 从地址档切过来时给个具体图标，否则这一档看起来"选了但没值"
                    if !value.starts_with(ICON_PREFIX) {
                        on_change.call(builtin_value("link"));
                    }
                },
                "内置图标"
            }
            button {
                class: if custom { "segment active" } else { "segment" },
                r#type: "button",
                onclick: move |_| custom_clicked.set(true),
                "自定义地址"
            }
        }

        if custom {
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

/// 当前值是否属于"自定义地址"档（非空、且不是内置图标）。
fn is_custom(value: &str) -> bool {
    !value.is_empty() && !value.starts_with(ICON_PREFIX)
}

#[cfg(test)]
mod tests {
    use super::is_custom;

    #[test]
    fn recognises_custom_values() {
        assert!(!is_custom(""), "空值是自动档");
        assert!(!is_custom("i:code"), "内置图标不是自定义");
        assert!(is_custom("https://a.com/i.png"));
        assert!(is_custom("/wallpaper/x.png"));
    }
}
