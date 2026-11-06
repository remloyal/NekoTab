use dioxus::prelude::*;

use crate::backend::{add_bookmark, error_text, fetch_page_meta, update_bookmark};
use crate::components::icon_picker::IconPicker;
use crate::models::{
    clamp_span, AddOutcome, Bookmark, Group, NewBookmark, CARD_SURFACES, SPAN_MAX, SPAN_PRESETS,
};

/// 抓一次页面信息并回填表单。
///
/// 只填空位：用户已经打好的名称 / 描述不会被代取结果顶掉。`overwrite_title`
/// 只有手动点「重新抓取」时才为真——那是明确的"以页面为准"的意图。
async fn fill_meta(
    link: String,
    mut title: Signal<String>,
    mut description: Signal<String>,
    overwrite_title: bool,
) -> Result<(), String> {
    let meta = fetch_page_meta(link).await.map_err(|e| error_text(&e))?;

    if !meta.title.is_empty() && (overwrite_title || title().trim().is_empty()) {
        title.set(meta.title);
    }
    if !meta.description.is_empty() && description().trim().is_empty() {
        description.set(meta.description);
    }

    Ok(())
}

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
    // 一条书签只属于一个分组。指向已删除分组的 id 先滤掉，
    // 避免"网格里显示未分组、弹窗里却没选中任何分组"。
    let known_group_ids: Vec<i64> = groups.iter().map(|group| group.id).collect();
    let mut group_id = use_signal(|| {
        existing
            .as_ref()
            .and_then(|b| b.group_id)
            .filter(|id| known_group_ids.contains(id))
            .or(default_group)
    });
    // 卡片占几格。老数据没有这个字段（0）时按一格显示
    let mut span_w = use_signal(|| clamp_span(existing.as_ref().map(|b| b.span_w).unwrap_or(1)));
    let mut span_h = use_signal(|| clamp_span(existing.as_ref().map(|b| b.span_h).unwrap_or(1)));
    let mut busy = use_signal(|| false);
    let mut error = use_signal(|| Option::<String>::None);
    // 抓标题：进行中标记、给用户的一句提示、以及已经抓过的地址（失焦时别重复请求）
    let mut meta_busy = use_signal(|| false);
    let mut meta_note = use_signal(|| Option::<String>::None);
    let mut fetched_url = use_signal(|| Option::<String>::None);

    // 失焦时自动抓（名称空着才抓），「抓取标题」按钮是手动重抓。
    let mut run_fetch = move |manual: bool| {
        if meta_busy() {
            return;
        }

        let link = url().trim().to_string();
        if !(link.starts_with("http://") || link.starts_with("https://")) {
            if manual {
                meta_note.set(Some("网址需以 http:// 或 https:// 开头".to_string()));
            }
            return;
        }
        if !manual && !title().trim().is_empty() {
            return;
        }
        if !manual && fetched_url().as_deref() == Some(link.as_str()) {
            return;
        }

        // 在进行中的标记要在 spawn 之前置位：失焦与按钮点击可能背靠背发生，
        // 晚一步的话两次请求会同时飞出去。
        meta_busy.set(true);
        meta_note.set(None);

        spawn(async move {
            match fill_meta(link.clone(), title, description, manual).await {
                Ok(()) => {
                    fetched_url.set(Some(link));
                    if title().trim().is_empty() {
                        meta_note.set(Some("这个页面没给出标题，手动填一下吧".to_string()));
                    }
                }
                // 抓不到就只是少一条辅助信息，不打断填写与保存。同样记下地址，
                // 免得之后每次失焦都再等一次超时；想重试可以点「重新抓取」。
                Err(message) => {
                    fetched_url.set(Some(link));
                    meta_note.set(Some(format!("没抓到标题：{message}")));
                }
            }

            meta_busy.set(false);
        });
    };

    let submit = move |_| {
        if busy() {
            return;
        }

        // 每次提交克隆一份：`spawn` 里的 async move 块按值捕获它，直接引用外层
        // 变量会让这个闭包变成 FnOnce，而事件处理器要的是 FnMut。
        let tags = existing_tags.clone();

        spawn(async move {
            busy.set(true);
            error.set(None);
            meta_note.set(None);

            // 新增时名称还空着（比如在网址框里直接回车提交）：先替用户抓一次。
            // 抓到就随这次保存一起写进去；抓不到再交给下面的校验去提示。
            if !is_edit && title().trim().is_empty() {
                let link = url().trim().to_string();
                if link.starts_with("http://") || link.starts_with("https://") {
                    let _ = fill_meta(link, title, description, false).await;
                }
            }

            let input = NewBookmark {
                title: title(),
                url: url(),
                description: description(),
                tags,
                group_id: group_id(),
                icon: icon(),
                span_w: span_w(),
                span_h: span_h(),
                // 这个表单只管网址导航；组件走 WidgetForm
                kind: crate::models::KIND_BOOKMARK.to_string(),
                // 书签没有组件参数。update 的 SQL 里不写 kind，所以种类改不掉
                config: String::new(),
                surface: surface(),
            };

            // 新增时名称空着：抓取也没能补上，给一句比"标题不能为空"更具体的提示
            if !is_edit && input.title.trim().is_empty() {
                error.set(Some(
                    "没抓到标题，请手动填写名称（这个页面可能不让访问）".to_string(),
                ));
                busy.set(false);
                return;
            }

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

            div { class: "field",
                div { class: "field-label-row",
                    span { class: "field-label", "名称" }
                    button {
                        class: "btn-ghost btn-mini",
                        r#type: "button",
                        disabled: meta_busy(),
                        title: "按上面的网址抓取页面标题",
                        onclick: move |_| run_fetch(true),
                        if meta_busy() {
                            "抓取中…"
                        } else if title().trim().is_empty() {
                            "抓取标题"
                        } else {
                            "重新抓取"
                        }
                    }
                }
                input {
                    class: "input",
                    autofocus: true,
                    placeholder: "例如：GitHub",
                    value: "{title}",
                    oninput: move |e| title.set(e.value()),
                }
                if let Some(note) = meta_note() {
                    p { class: "field-hint", "{note}" }
                }
            }

            label { class: "field",
                span { class: "field-label", "网址" }
                input {
                    class: "input",
                    placeholder: "https://example.com",
                    value: "{url}",
                    oninput: move |e| url.set(e.value()),
                    // 填完网址离开输入框时就替用户抓一次；名称已经写了则不打扰
                    onblur: move |_| run_fetch(false),
                }
                p { class: "field-hint", "离开输入框时会自动抓一次页面标题，抓不到也可以手填。" }
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
                span { class: "field-label", "分组" }
                div { class: "segmented wrap",
                    button {
                        class: if group_id().is_none() { "segment active" } else { "segment" },
                        r#type: "button",
                        onclick: move |_| group_id.set(None),
                        "未分组"
                    }
                    for group in groups {
                        button {
                            key: "{group.id}",
                            class: if group_id() == Some(group.id) { "segment active" } else { "segment" },
                            r#type: "button",
                            onclick: move |_| group_id.set(Some(group.id)),
                            "{group.name}"
                        }
                    }
                }
                p { class: "field-hint",
                    "一条书签归一个分组；选「未分组」就是不放进任何分类。"
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
