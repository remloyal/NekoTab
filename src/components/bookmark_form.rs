use dioxus::prelude::*;

use crate::backend::{add_bookmark, error_text, update_bookmark};
use crate::models::{Bookmark, NewBookmark};

/// 书签表单，作为弹窗内容使用。
///
/// `bookmark` 为 `None` 时是新增；为 `Some` 时是修改，字段会预填并调用
/// `update_bookmark`。两种模式共用同一套字段与校验。
#[component]
pub fn BookmarkForm(
    bookmark: Option<Bookmark>,
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
    let mut tags = use_signal(|| existing.as_ref().map(|b| b.tags.clone()).unwrap_or_default());
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
            tags: tags(),
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
                span { class: "field-label", "标签" }
                input {
                    class: "input",
                    placeholder: "逗号分隔，会成为上方分类，如：开发, 文档",
                    value: "{tags}",
                    oninput: move |e| tags.set(e.value()),
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
