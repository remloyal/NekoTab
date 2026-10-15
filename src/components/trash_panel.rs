use dioxus::prelude::*;

use crate::backend::{error_text, list_trash, purge_bookmarks, restore_bookmarks};
use crate::models::TrashItem;

/// 回收站（主页设置抽屉内容）。
///
/// 删除书签走的是软删除（`deleted_at` 置时间戳），列表查询一直带
/// `deleted_at IS NULL`，所以这里见到的是"删了但还在库里"的那些。
/// 彻底删除才真的 `DELETE`。
#[component]
pub fn TrashPanel(on_changed: EventHandler<()>) -> Element {
    let refresh = use_signal(|| 0u32);
    let mut busy = use_signal(|| false);
    let mut error = use_signal(|| Option::<String>::None);
    let mut confirming_clear = use_signal(|| false);

    let items_res = use_resource(move || async move {
        let _ = refresh();
        list_trash().await
    });

    let items: Vec<TrashItem> = match (&*items_res.read()).clone() {
        Some(Ok(list)) => list,
        _ => Vec::new(),
    };
    let loading = (&*items_res.read()).is_none();
    // 单独取一份 id：clear_all 要把它移进闭包，而 items 还要留给 rsx 渲染
    let trash_ids: Vec<i64> = items.iter().map(|item| item.id).collect();

    // 每次操作后既刷新本地列表，也让父级把主列表重查一遍。
    // 做成普通函数而不是闭包：闭包要 `&mut` 才能调用，被 `move` 进 async 块后
    // 编译器会要求绑定可变，几个调用点各要一份；Signal 与 EventHandler 都是 Copy，
    // 传值反而最省事。
    fn notify(mut refresh: Signal<u32>, on_changed: EventHandler<()>) {
        refresh.set(refresh() + 1);
        on_changed.call(());
    }

    let restore = move |id: i64| {
        spawn(async move {
            busy.set(true);
            error.set(None);
            match restore_bookmarks(vec![id]).await {
                Ok(_) => {
                    busy.set(false);
                    notify(refresh, on_changed);
                }
                Err(e) => {
                    error.set(Some(error_text(&e)));
                    busy.set(false);
                }
            }
        });
    };

    let purge = move |id: i64| {
        spawn(async move {
            busy.set(true);
            error.set(None);
            match purge_bookmarks(vec![id]).await {
                Ok(_) => {
                    busy.set(false);
                    notify(refresh, on_changed);
                }
                Err(e) => {
                    error.set(Some(error_text(&e)));
                    busy.set(false);
                }
            }
        });
    };

    let clear_all = move |_| {
        let ids = trash_ids.clone();
        if ids.is_empty() {
            return;
        }

        spawn(async move {
            busy.set(true);
            error.set(None);
            confirming_clear.set(false);
            match purge_bookmarks(ids).await {
                Ok(_) => {
                    busy.set(false);
                    notify(refresh, on_changed);
                }
                Err(e) => {
                    error.set(Some(error_text(&e)));
                    busy.set(false);
                }
            }
        });
    };

    rsx! {
        section { class: "panel-section",
            h3 { class: "panel-heading", "回收站" }

            p { class: "field-hint",
                "删除的书签先放到这里，可以还原。只有「彻底删除」才会真正从库里移除。"
            }

            div { class: "form-actions start",
                span { class: "trash-count", "共 {items.len()} 条" }
                if confirming_clear() {
                    button {
                        class: "btn-danger",
                        r#type: "button",
                        disabled: busy(),
                        onclick: clear_all,
                        "确认清空"
                    }
                    button {
                        class: "btn-ghost",
                        r#type: "button",
                        onclick: move |_| confirming_clear.set(false),
                        "取消"
                    }
                } else {
                    button {
                        class: "btn-ghost",
                        r#type: "button",
                        disabled: busy() || items.is_empty(),
                        onclick: move |_| confirming_clear.set(true),
                        "清空回收站"
                    }
                }
            }

            if let Some(message) = error() {
                p { class: "form-error", "{message}" }
            }

            if loading {
                p { class: "field-hint", "读取中…" }
            } else if items.is_empty() {
                p { class: "field-hint", "回收站是空的。" }
            } else {
                ul { class: "trash-list",
                    for item in items {
                        li { key: "{item.id}", class: "trash-row",
                            div { class: "trash-main",
                                span { class: "trash-title", "{item.title}" }
                                // 组件的 url 是空串（见 models::is_widget_kind），
                                // 直接渲染会留一行空白，看起来像坏了
                                span { class: "trash-url",
                                    if item.url.is_empty() { "（组件）" } else { "{item.url}" }
                                }
                                span { class: "trash-time", "删除于 {item.deleted_at}（UTC）" }
                            }
                            div { class: "group-actions",
                                button {
                                    class: "icon-btn",
                                    r#type: "button",
                                    title: "还原到书签列表",
                                    disabled: busy(),
                                    onclick: {
                                        let id = item.id;
                                        move |_| restore(id)
                                    },
                                    "还原"
                                }
                                button {
                                    class: "icon-btn danger",
                                    r#type: "button",
                                    title: "彻底删除（不可恢复）",
                                    disabled: busy(),
                                    onclick: {
                                        let id = item.id;
                                        move |_| purge(id)
                                    },
                                    "彻底删除"
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
