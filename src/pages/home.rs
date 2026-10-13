use dioxus::prelude::*;

use crate::backend::{
    delete_bookmark, delete_engine, get_settings, list_bookmarks, list_engines, reorder_bookmarks,
    set_active_engine,
};
use crate::components::{
    BookmarkForm, BookmarkGrid, Clock, ConfirmDialog, ContextMenu, Drawer, EngineSettings, Modal,
    QuoteLine, SearchBar, SettingsPanel, TagTabs,
};
use crate::models::{Bookmark, Settings};

/// 弹窗状态机。
#[derive(Clone, PartialEq)]
enum ModalState {
    None,
    Add,
    Edit { bookmark: Bookmark },
    EngineSettings,
    ConfirmDelete { id: i64, title: String },
}

/// 汇总所有书签的标签与条数，按名称排序（"全部" 由 `TagTabs` 自己补）。
fn collect_tags(items: &[Bookmark]) -> Vec<(String, usize)> {
    let mut counts: Vec<(String, usize)> = Vec::new();

    for bookmark in items {
        for tag in bookmark.tags.split(',') {
            let tag = tag.trim();
            if tag.is_empty() {
                continue;
            }

            match counts.iter_mut().find(|(name, _)| name == tag) {
                Some((_, count)) => *count += 1,
                None => counts.push((tag.to_string(), 1)),
            }
        }
    }

    counts.sort_by(|a, b| a.0.cmp(&b.0));
    counts
}

fn has_tag(bookmark: &Bookmark, tag: &str) -> bool {
    bookmark.tags.split(',').any(|t| t.trim() == tag)
}

/// 主页：背景 + 设置入口 + 时钟 + 一言 + 搜索引擎框 + 分类标签页 + 图标网格。
#[component]
pub fn Home() -> Element {
    let mut query = use_signal(String::new);
    let mut refresh = use_signal(|| 0u32);
    let mut engines_refresh = use_signal(|| 0u32);
    let mut settings_refresh = use_signal(|| 0u32);
    let mut active_tag = use_signal(String::new);
    let mut editing = use_signal(|| false);
    let mut drawer_open = use_signal(|| false);
    let mut menu = use_signal(|| Option::<(f64, f64, Option<Bookmark>)>::None);
    let mut modal = use_signal(|| ModalState::None);

    // 列表始终取全量：搜索框是"搜网页"用的，不筛选书签。
    // 分类筛选仍按标签在客户端做——正因为取的是全量，筛选和拖拽重排都不受影响。
    //
    // 这里用两个 hook 分工，而不是单个带 refresh 依赖的 use_server_future：
    //
    // * `initial` 只在 SSR 阶段跑一次，结果随 HTML 一起下发；hydrate 时直接复用
    //   序列化好的值，不会再发一次请求。它刻意**不带 refresh 依赖**——一旦重跑，
    //   use_server_future 会把状态置回 Pending 触发 suspend，而 Suspense 边界在
    //   子组件 suspend 时显示 fallback，列表就会闪一下「加载中」。
    // * `bookmarks` 仍是客户端的 use_resource，负责增删改排序后的重查。它会保留
    //   旧值，所以重查期间不会闪。
    let initial = use_server_future(move || async move { list_bookmarks(String::new()).await })?;
    let bookmarks = use_resource(move || async move {
        // 首次渲染（refresh 仍为 0）直接用服务端带来的数据，避免首屏多打一次接口
        if refresh() == 0 {
            if let Some(v) = (&*initial.peek()).clone() {
                return v;
            }
        }
        list_bookmarks(String::new()).await
    });

    let engines_res = use_resource(move || async move {
        let _ = engines_refresh();
        list_engines().await
    });

    let settings_res = use_resource(move || async move {
        let _ = settings_refresh();
        get_settings().await
    });

    // 取克隆快照，后续一律用普通值计算，避免在 rsx 中与借用/移动纠缠。
    // 首屏（含 SSR 阶段）优先用服务端直出的数据；客户端资源就绪后换成它。
    let snapshot = match (&*bookmarks.read()).clone() {
        Some(v) => Some(v),
        None => (&*initial.read()).clone(),
    };
    let engines: Vec<_> = match (&*engines_res.read()).clone() {
        Some(Ok(list)) => list,
        _ => Vec::new(),
    };
    let settings: Settings = match (&*settings_res.read()).clone() {
        Some(Ok(found)) => found,
        _ => Settings::default(),
    };

    let loading = snapshot.is_none();
    let error: Option<String> = match &snapshot {
        Some(Err(e)) => Some(e.to_string()),
        _ => None,
    };
    let all_items: Vec<Bookmark> = match &snapshot {
        Some(Ok(items)) => items.clone(),
        _ => Vec::new(),
    };

    let tags = collect_tags(&all_items);
    let total = all_items.len();

    // 若选中的标签在结果里已不存在，自动回落到"全部"
    let selected = active_tag();
    let active = if selected.is_empty() || tags.iter().any(|(name, _)| name == &selected) {
        selected
    } else {
        String::new()
    };

    let visible: Vec<Bookmark> = if active.is_empty() {
        all_items.clone()
    } else {
        all_items
            .iter()
            .filter(|b| has_tag(b, &active))
            .cloned()
            .collect()
    };

    let is_editing = editing();

    // 随机模式用的一次性参数：每次打开页面生成一个，用来绕开图片接口的长缓存。
    // 只在客户端生成——SSR 阶段设置本就还没加载，两边都是"空"，因此不会
    // hydration 失配。
    // 服务端分支里没有写入，`mut` 只在 web 下需要
    #[cfg_attr(feature = "server", allow(unused_mut))]
    let mut bg_nonce = use_signal(String::new);
    use_effect(move || {
        #[cfg(feature = "web")]
        if bg_nonce().is_empty() {
            bg_nonce.set((js_sys::Date::now() as u64).to_string());
        }
    });

    // 背景：空则交给 body 的渐变，非空则铺图；遮罩只在铺图时叠加，
    // 这样默认观感与没有该功能时完全一致。
    let has_wallpaper = !settings.background_url.is_empty();
    let nonce = bg_nonce();
    // 开了随机但随机参数还没就绪时先不渲染，免得先请求一次不带参数的地址再换掉
    let ready = has_wallpaper && (!settings.background_random || !nonce.is_empty());

    let bg_style = if ready {
        let mut url = settings.background_url.replace(['"', '\'', ';', '\\'], "");
        if settings.background_random {
            let sep = if url.contains('?') { '&' } else { '?' };
            url = format!("{url}{sep}_={nonce}");
        }
        format!("background-image: url(\"{url}\");")
    } else {
        String::new()
    };
    let dim_alpha = if ready {
        settings.background_dim.clamp(0, 80) as f32 / 100.0
    } else {
        0.0
    };

    let delete_title = match &modal() {
        ModalState::ConfirmDelete { title, .. } => title.clone(),
        _ => String::new(),
    };

    // 拖拽落下：把被拖项移动到目标项的位置，再把完整顺序写回服务端
    let order_ids: Vec<i64> = all_items.iter().map(|b| b.id).collect();
    let on_reorder = move |(dragged, target): (i64, i64)| {
        if dragged == target {
            return;
        }

        let mut ids = order_ids.clone();
        let (Some(from), Some(to)) = (
            ids.iter().position(|&x| x == dragged),
            ids.iter().position(|&x| x == target),
        ) else {
            return;
        };

        let moved = ids.remove(from);
        ids.insert(to, moved);

        spawn(async move {
            if reorder_bookmarks(ids).await.is_ok() {
                refresh.set(refresh() + 1);
            }
        });
    };

    rsx! {
        div {
            class: "stage-root",
            // 空白处右键 → 背景菜单
            oncontextmenu: move |evt| {
                evt.prevent_default();
                let coords = evt.client_coordinates();
                menu.set(Some((coords.x, coords.y, None)));
            },

            div { class: "bg", style: "{bg_style}" }
            div { class: "bg-dim", style: "background: rgba(0, 0, 0, {dim_alpha});" }

            button {
                class: "settings-btn",
                r#type: "button",
                title: "主页设置",
                onclick: move |_| drawer_open.set(true),
                svg {
                    view_box: "0 0 24 24",
                    fill: "none",
                    stroke: "currentColor",
                    stroke_width: "2",
                    stroke_linecap: "round",
                    line { x1: "4", y1: "7", x2: "20", y2: "7" }
                    line { x1: "4", y1: "12", x2: "20", y2: "12" }
                    line { x1: "4", y1: "17", x2: "20", y2: "17" }
                    circle { cx: "9", cy: "7", r: "2.3", fill: "currentColor", stroke: "none" }
                    circle { cx: "15", cy: "12", r: "2.3", fill: "currentColor", stroke: "none" }
                    circle { cx: "10", cy: "17", r: "2.3", fill: "currentColor", stroke: "none" }
                }
            }

            if settings.show_clock {
                Clock {}
            }
            if settings.show_quote {
                QuoteLine {}
            }

            SearchBar {
                value: query(),
                oninput: move |v| query.set(v),
                engines: engines.clone(),
                on_select_engine: move |id: i64| {
                    spawn(async move {
                        if set_active_engine(id).await.is_ok() {
                            engines_refresh.set(engines_refresh() + 1);
                        }
                    });
                },
                on_open_settings: move |_| modal.set(ModalState::EngineSettings),
            }

            section { class: "stage",
                if !tags.is_empty() {
                    TagTabs {
                        tags: tags.clone(),
                        total,
                        active: active.clone(),
                        on_select: move |tag| active_tag.set(tag),
                    }
                }

                if let Some(err) = error.clone() {
                    div { class: "state state-error", "加载失败：{err}" }
                } else if loading {
                    div { class: "state state-loading", "加载中…" }
                } else {
                    if visible.is_empty() {
                        // 标签都是从现有书签派生的，所以只有"一条都没有"时才会空
                        div { class: "hint", "还没有书签：在空白处右键 → 添加卡片" }
                    }
                    BookmarkGrid {
                        items: visible.clone(),
                        editing: is_editing,
                        on_add: move |_| modal.set(ModalState::Add),
                        on_context: move |(bookmark, x, y): (Bookmark, f64, f64)| {
                            menu.set(Some((x, y, Some(bookmark))));
                        },
                        on_enter_edit: move |_| editing.set(true),
                        on_edit: move |bookmark: Bookmark| {
                            modal.set(ModalState::Edit { bookmark });
                        },
                        on_delete: move |bookmark: Bookmark| {
                            modal.set(ModalState::ConfirmDelete {
                                id: bookmark.id,
                                title: bookmark.title.clone(),
                            });
                        },
                        on_reorder,
                    }
                }
            }

            if is_editing {
                div { class: "edit-bar",
                    span { class: "edit-hint", "拖动卡片调整顺序 · 点卡片修改 · 点 ✕ 删除" }
                    button {
                        class: "btn-primary",
                        r#type: "button",
                        onclick: move |_| editing.set(false),
                        "完成"
                    }
                }
            }

            if let Some((x, y, target)) = menu() {
                ContextMenu {
                    x,
                    y,
                    target: target.clone(),
                    on_close: move |_| menu.set(None),
                    on_refresh: move |_| {
                        menu.set(None);
                        refresh.set(refresh() + 1);
                    },
                    on_add: move |_| {
                        menu.set(None);
                        modal.set(ModalState::Add);
                    },
                    on_edit: move |bookmark: Bookmark| {
                        menu.set(None);
                        modal.set(ModalState::Edit { bookmark });
                    },
                    on_delete: move |bookmark: Bookmark| {
                        menu.set(None);
                        modal.set(ModalState::ConfirmDelete {
                            id: bookmark.id,
                            title: bookmark.title.clone(),
                        });
                    },
                }
            }

            if drawer_open() {
                Drawer {
                    title: "主页设置",
                    on_close: move |_| drawer_open.set(false),
                    SettingsPanel {
                        settings: settings.clone(),
                        on_saved: move |_| settings_refresh.set(settings_refresh() + 1),
                        on_imported: move |_| refresh.set(refresh() + 1),
                    }
                }
            }

            match modal() {
                ModalState::Add => rsx! {
                    Modal {
                        title: "添加网址导航",
                        on_close: move |_| modal.set(ModalState::None),
                        BookmarkForm {
                            bookmark: None,
                            on_cancel: move |_| modal.set(ModalState::None),
                            on_saved: move |_| {
                                modal.set(ModalState::None);
                                refresh.set(refresh() + 1);
                            },
                        }
                    }
                },
                ModalState::Edit { bookmark } => rsx! {
                    Modal {
                        title: "修改网址导航",
                        on_close: move |_| modal.set(ModalState::None),
                        BookmarkForm {
                            // key 让切换不同书签时重新挂载，避免沿用上一个的预填值
                            key: "{bookmark.id}",
                            bookmark: Some(bookmark.clone()),
                            on_cancel: move |_| modal.set(ModalState::None),
                            on_saved: move |_| {
                                modal.set(ModalState::None);
                                refresh.set(refresh() + 1);
                            },
                        }
                    }
                },
                ModalState::EngineSettings => rsx! {
                    Modal {
                        title: "搜索引擎设置",
                        on_close: move |_| modal.set(ModalState::None),
                        EngineSettings {
                            engines: engines.clone(),
                            on_select: move |id: i64| {
                                spawn(async move {
                                    if set_active_engine(id).await.is_ok() {
                                        engines_refresh.set(engines_refresh() + 1);
                                    }
                                });
                            },
                            on_delete: move |id: i64| {
                                spawn(async move {
                                    if delete_engine(id).await.is_ok() {
                                        engines_refresh.set(engines_refresh() + 1);
                                    }
                                });
                            },
                            on_added: move |_| engines_refresh.set(engines_refresh() + 1),
                        }
                    }
                },
                ModalState::ConfirmDelete { id, .. } => rsx! {
                    ConfirmDialog {
                        title: "删除书签",
                        message: "确定删除「{delete_title}」吗？此操作不可撤销。",
                        confirm_label: "确认删除",
                        on_cancel: move |_| modal.set(ModalState::None),
                        on_confirm: move |_| {
                            modal.set(ModalState::None);
                            spawn(async move {
                                if delete_bookmark(id).await.is_ok() {
                                    refresh.set(refresh() + 1);
                                }
                            });
                        },
                    }
                },
                ModalState::None => rsx! {},
            }
        }
    }
}
