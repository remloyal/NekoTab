use dioxus::prelude::*;

use super::icon_picker::IconPicker;
use crate::backend::{add_bookmark, error_text, update_bookmark};
use crate::models::{Bookmark, Group, NewBookmark};

/// 书签表单，作为弹窗内容使用。
///
/// `bookmark` 为 `None` 时是新增；为 `Some` 时是修改，字段会预填并调用
/// `update_bookmark`。两种模式共用同一套字段与校验。
#[component]
pub fn BookmarkForm(
    bookmark: Option<Bookmark>,
    groups: Vec<Group>,
    /// 新增时的默认分组（跟着当前筛选走，省得每次都手工选）
    default_group: Option<i64>,
    on_saved: EventHandler<()>,
    on_cancel: EventHandler<()>,
) -> Element {
    let existing = bookmark.clone();
    let editing_id = existing.as_ref().map(|b| b.id);
    let is_edit = editing_id.is_some();
    let submit_label = if is_edit { "保存修改" } else { "保存" };

    let mut title = use_signal(|| existing.as_ref().map(|b| b.title.clone()).unwrap_or_default());
    let mut url = use_signal(|| existing.as_ref().map(|b| b.url.clone()).unwrap_or_default());
    let mut description = use_signal(|| {
        existing
            .as_ref()
            .map(|b| b.description.clone())
            .unwrap_or_default()
    });
    let mut icon = use_signal(|| existing.as_ref().map(|b| b.icon.clone()).unwrap_or_default());
    // 标签不再是界面上的东西（分类只由分组承担），但库里已有的值要原样带过去：
    // update 会整行覆盖，这里若不回传就等于把老数据清空了。
    let existing_tags = existing.as_ref().map(|b| b.tags.clone()).unwrap_or_default();
    // 下拉框的值统一用字符串：空串即"未分组"，这样 select 的 value 不必处理 Option
    let mut group_id = use_signal(|| {
        existing
            .as_ref()
            .and_then(|b| b.group_id)
            .or(default_group)
            .map(|id| id.to_string())
            .unwrap_or_default()
    });
    let mut busy = use_signal(|| false);
    let mut error = use_signal(|| Option::<String>::None);

    let submit = move |_| {
        if busy() {
            return;
        }

        let input = NewBookmark {
            title: title(),
            url: url(),
            description: description(),
            tags: existing_tags.clone(),
            group_id: group_id().parse().ok(),
            icon: icon(),
        };

        spawn(async move {
            busy.set(true);
            error.set(None);

            let result = match editing_id {
                Some(id) => update_bookmark(id, input).await,
                None => add_bookmark(input).await.map(|_| ()),
            };

            match result {
                Ok(()) => {
                    // 先把 busy 复位，再回调关闭弹窗（避免组件卸载后写 signal）
                    busy.set(false);
                    on_saved.call(());
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
                    placeholder: "例如：GitHub",
                    value: "{title}",
                    oninput: move |e| title.set(e.value()),
                }
            }

            label { class: "field",
                span { class: "field-label", "网址" }
                input {
                    class: "input",
                    placeholder: "https://example.com",
                    value: "{url}",
                    oninput: move |e| url.set(e.value()),
                }
            }

            label { class: "field",
                span { class: "field-label", "描述" }
                input {
                    class: "input",
                    placeholder: "可选",
                    value: "{description}",
                    oninput: move |e| description.set(e.value()),
                }
            }

            label { class: "field",
                span { class: "field-label", "分组" }
                select {
                    class: "input",
                    value: "{group_id}",
                    onchange: move |e| group_id.set(e.value()),
                    option { value: "", "未分组" }
                    for group in groups {
                        option { value: "{group.id}", "{group.name}" }
                    }
                }
                p { class: "field-hint",
                    "分组决定卡片出现在哪个分类标签页下（一条书签只属于一个分组）；在设置 → 分组管理里增删。"
                }
            }

            div { class: "field",
                span { class: "field-label", "图标" }
                IconPicker {
                    value: icon(),
                    on_change: move |v: String| icon.set(v),
                    auto_label: "自动（取站点图标）",
                }
                p { class: "field-hint",
                    "默认按网址自动取站点图标，取不到时回退成名称首字；也可以在这里指定内置图标或图片地址。"
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
