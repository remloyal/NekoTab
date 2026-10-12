use dioxus::prelude::*;

/// 分类标签页。`tags` 是 `(标签名, 条数)`；"全部" 的条数由 `total` 给出。
#[component]
pub fn TagTabs(
    tags: Vec<(String, usize)>,
    total: usize,
    active: String,
    on_select: EventHandler<String>,
) -> Element {
    // (显示名, 条数, 选中后写入的值, 是否选中)
    let mut entries: Vec<(String, usize, String, bool)> =
        vec![("全部".to_string(), total, String::new(), active.is_empty())];

    for (name, count) in tags {
        let is_active = name == active;
        entries.push((name.clone(), count, name, is_active));
    }

    rsx! {
        nav { class: "tabs",
            for (label, count, value, is_active) in entries {
                button {
                    class: if is_active { "tab active" } else { "tab" },
                    onclick: {
                        let selected = value.clone();
                        move |_| on_select.call(selected.clone())
                    },
                    span { class: "tab-name", "{label}" }
                    span { class: "tab-count", "{count}" }
                }
            }
        }
    }
}
