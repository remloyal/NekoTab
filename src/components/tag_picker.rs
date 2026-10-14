use dioxus::prelude::*;

/// 标签选择器。
///
/// 标签是"检索词"，一条书签可以带多个，所以这里不是下拉框而是**可点选的标签列表**
/// （带条数，点一下加/去），再配一个输入框用来新增库里还没有的标签。
/// 早先是一个裸输入框，要用户自己记住并手打逗号分隔的字符串：既看不出有哪些标签、
/// 也容易打错。
#[component]
pub fn TagPicker(
    /// 当前选中，逗号分隔
    value: String,
    /// 库里已有的标签与条数
    known: Vec<(String, usize)>,
    on_change: EventHandler<String>,
) -> Element {
    let mut draft = use_signal(String::new);
    let on_change = on_change;

    let selected = split_tags(&value);

    // 已选中但库里还没有的标签也要显示（否则用户看不到自己刚加的）
    let extras: Vec<String> = selected
        .iter()
        .filter(|tag| !known.iter().any(|(name, _)| name == *tag))
        .cloned()
        .collect();


    rsx! {
        if !known.is_empty() || !selected.is_empty() {
            div { class: "tag-cloud",
                for tag in extras {
                    button {
                        key: "new-{tag}",
                        class: "tag-chip active",
                        r#type: "button",
                        title: "取消选择",
                        onclick: {
                            let selected = selected.clone();
                            let tag = tag.clone();
                            move |_| on_change.call(toggle_tag(&selected, &tag))
                        },
                        "{tag}"
                        span { class: "tag-chip-count", "新" }
                    }
                }
                for (tag, count) in known {
                    button {
                        key: "{tag}",
                        class: if selected.contains(&tag) { "tag-chip active" } else { "tag-chip" },
                        r#type: "button",
                        title: if selected.contains(&tag) { "取消选择" } else { "选择该标签" },
                        onclick: {
                            let selected = selected.clone();
                            let tag = tag.clone();
                            move |_| on_change.call(toggle_tag(&selected, &tag))
                        },
                        "{tag}"
                        span { class: "tag-chip-count", "{count}" }
                    }
                }
            }
        }

        div { class: "inline-form",
            input {
                class: "input",
                placeholder: "新增标签…",
                value: "{draft}",
                oninput: move |e| draft.set(e.value()),
                onkeydown: {
                    let value = value.clone();
                    move |evt| {
                        if evt.key() == Key::Enter {
                            evt.prevent_default();
                            let tag = draft().trim().to_string();
                            draft.set(String::new());
                            if let Some(next) = append_tag(&value, &tag) {
                                on_change.call(next);
                            }
                        }
                    }
                },
            }
            button {
                class: "btn-ghost",
                r#type: "button",
                disabled: draft().trim().is_empty(),
                onclick: {
                    let value = value.clone();
                    move |_| {
                        let tag = draft().trim().to_string();
                        draft.set(String::new());
                        if let Some(next) = append_tag(&value, &tag) {
                            on_change.call(next);
                        }
                    }
                },
                "添加"
            }
        }
    }
}

/// 追加一个新标签；已有或为空则返回 `None`。
fn append_tag(current: &str, tag: &str) -> Option<String> {
    if tag.is_empty() || split_tags(current).iter().any(|t| t == tag) {
        return None;
    }
    let mut next = split_tags(current);
    next.push(tag.to_string());
    Some(next.join(","))
}

/// 在选中集合里加/去一个标签，返回逗号分隔的新值。
///
/// 做成纯函数而不是闭包：闭包会捕获 `selected`，在多个按钮里复用就要它可 Copy，
/// 而 `Vec<String>` 不是。纯函数每次传进去，反而最直接。
fn toggle_tag(selected: &[String], tag: &str) -> String {
    let mut next: Vec<String> = selected.to_vec();
    match next.iter().position(|t| t == tag) {
        Some(pos) => {
            next.remove(pos);
        }
        None => next.push(tag.to_string()),
    }
    next.join(",")
}

/// 按逗号拆分并清理（与后端 `normalize_tags` 同一口径）。
fn split_tags(raw: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for tag in raw.split(',') {
        let tag = tag.trim();
        if !tag.is_empty() && !out.iter().any(|existing| existing == tag) {
            out.push(tag.to_string());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{split_tags, toggle_tag};

    #[test]
    fn splits_and_cleans() {
        assert_eq!(split_tags("a, b ,, c"), vec!["a", "b", "c"]);
        assert_eq!(split_tags("a,a"), vec!["a"], "去重");
        assert_eq!(split_tags(""), Vec::<String>::new());
        assert_eq!(split_tags("  "), Vec::<String>::new());
    }

    #[test]
    fn appends_only_new_tags() {
        use super::append_tag;
        assert_eq!(append_tag("a", "b").as_deref(), Some("a,b"));
        assert_eq!(append_tag("", "x").as_deref(), Some("x"));
        assert_eq!(append_tag("a", "a"), None, "已有就不重复加");
        assert_eq!(append_tag("a", ""), None, "空标签忽略");
    }

    #[test]
    fn toggles_tags_on_and_off() {
        let selected = split_tags("a,b");
        assert_eq!(toggle_tag(&selected, "c"), "a,b,c", "加进去");
        assert_eq!(toggle_tag(&selected, "a"), "b", "去掉");
        assert_eq!(toggle_tag(&[], "x"), "x", "从空开始");
    }
}
