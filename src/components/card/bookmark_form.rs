use dioxus::prelude::*;

use crate::backend::{add_bookmark, error_text, update_bookmark};
use crate::components::icon_picker::IconPicker;
use crate::models::{
    clamp_span, AddOutcome, Bookmark, Group, NewBookmark, CARD_SURFACES, SPAN_MAX, SPAN_PRESETS,
};

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

    let mut title = use_signal(|| {
        existing
            .as_ref()
            .map(|b| b.title.clone())
            .unwrap_or_default()
    });
    let mut url = use_signal(|| existing.as_ref().map(|b| b.url.clone()).unwrap_or_default());
    let mut description = use_signal(|| {
        existing
            .as_ref()
            .map(|b| b.description.clone())
            .unwrap_or_default()
    });
    let mut icon = use_signal(|| {
        existing
            .as_ref()
            .map(|b| b.icon.clone())
            .unwrap_or_default()
    });
    // 标签不再是界面上的东西（分类只由分组承担），但库里已有的值要原样带过去：
    // update 会整行覆盖，这里若不回传就等于把老数据清空了。
    let existing_tags = existing
        .as_ref()
        .map(|b| b.tags.clone())
        .unwrap_or_default();
    // 底色。空串是"没设过"——书签卡的默认是"无面"，由 models::effective_surface 解析。
    let mut surface = use_signal(|| {
        existing
            .as_ref()
            .map(|b| b.surface.clone())
            .unwrap_or_default()
    });
    // 一条书签可以属于多个分组；老数据只 group_id 时也在这里统一成列表。
    // 指向已删除分组的 id 先滤掉，避免"网格里显示未分组、弹窗里却没选中任何分组"。
    let known_group_ids: Vec<i64> = groups.iter().map(|group| group.id).collect();
    let mut group_ids = use_signal(|| {
        existing
            .as_ref()
            .map(|b| {
                b.group_id_list()
                    .into_iter()
                    .filter(|id| known_group_ids.contains(id))
                    .collect::<Vec<_>>()
            })
            .filter(|ids| !ids.is_empty())
            .or_else(|| default_group.map(|id| vec![id]))
            .unwrap_or_default()
    });
    // 卡片占几格。老数据没有这个字段（0）时按一格显示
    let mut span_w = use_signal(|| clamp_span(existing.as_ref().map(|b| b.span_w).unwrap_or(1)));
    let mut span_h = use_signal(|| clamp_span(existing.as_ref().map(|b| b.span_h).unwrap_or(1)));
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
            // 主分组取第一个；完整归属由 group_ids 承载
            group_id: group_ids().first().copied(),
            group_ids: group_ids(),
            icon: icon(),
            span_w: span_w(),
            span_h: span_h(),
            // 这个表单只管网址导航；组件走 WidgetForm
            kind: crate::models::KIND_BOOKMARK.to_string(),
            // 书签没有组件参数。update 的 SQL 里不写 kind，所以种类改不掉
            config: String::new(),
            surface: surface(),
        };

        spawn(async move {
            busy.set(true);
            error.set(None);

            // 编辑走 update（无返回值），新增要知道是否真的写进去了，
            // 统一成 AddOutcome 再分支
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
                    // 先把 busy 复位，再回调关闭弹窗（避免组件卸载后写 signal）
                    busy.set(false);
                    on_saved.call(());
                }
                // 网址已存在：不关弹窗，把原因摆出来，好让人改网址或取消
                Ok(outcome) => {
                    error.set(Some(if outcome.in_trash {
                        "该网址已在回收站里，没有重复添加".to_string()
                    } else {
                        "该网址已在列表中，没有重复添加".to_string()
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

            div { class: "field",
                span { class: "field-label", "分组（可多选）" }
                div { class: "segmented wrap",
                    button {
                        class: if group_ids().is_empty() { "segment active" } else { "segment" },
                        r#type: "button",
                        onclick: move |_| group_ids.set(Vec::new()),
                        "未分组"
                    }
                    for group in groups {
                        button {
                            key: "{group.id}",
                            class: if group_ids().contains(&group.id) { "segment active" } else { "segment" },
                            r#type: "button",
                            onclick: move |_| {
                                let id = group.id;
                                let mut list = group_ids();
                                if let Some(pos) = list.iter().position(|known| *known == id) {
                                    list.remove(pos);
                                } else {
                                    list.push(id);
                                }
                                group_ids.set(list);
                            },
                            "{group.name}"
                        }
                    }
                }
                p { class: "field-hint",
                    "一条书签可以同时出现在多个分类标签页下；一个都不选就是「未分组」。"
                    "分组在设置 → 分组管理里增删。"
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
                    "卡片占几格（宽 × 高），最大 {SPAN_MAX}×{SPAN_MAX}。只影响排版，不影响顺序、分页与拖拽；"
                    "窗口太窄放不下时宽度那一维会自动收回一格。"
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
                    "默认按网址自动取站点图标，取不到时回退成名称首字；也可以选内置图标、"
                    "用一两个字做彩色图标，或填图片地址。"
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
                    "书签卡默认透明（图标块自带底色）；压深色壁纸看不清时，可以给它一层半透明或实底。"
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
