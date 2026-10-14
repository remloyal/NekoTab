use std::collections::{HashMap, HashSet};

use dioxus::prelude::*;

use crate::backend::{
    delete_bookmark, delete_bookmarks, delete_engine, get_settings, list_bookmarks, list_engines,
    list_groups, move_bookmarks, reorder_bookmarks, set_active_engine,
};
use crate::components::{
    build_tabs, group_value, parse_filter, BatchMoveList, BookmarkForm, BookmarkGrid,
    Clock, ConfirmDialog, ContextMenu, Drawer, EngineSettings, Filter, GroupTabs, Modal, Pager,
    QuoteLine, SearchBar, SearchMode, SettingsPanel, GROUP_UNGROUPED, SEARCH_INPUT_ID,
};
use crate::models::{Bookmark, Group, Settings};
use dioxus::html::geometry::WheelDelta;

/// 弹窗状态机。
#[derive(Clone, PartialEq)]
enum ModalState {
    None,
    Add,
    Edit { bookmark: Bookmark },
    EngineSettings,
    /// 删除确认。单条与批量共用：`ids` 是要删的集合，`message` 是文案。
    ConfirmDelete { ids: Vec<i64>, message: String },
    BatchMove,
}

/// 把 `dragged` 挪到 `target` 的位置（在传进来的完整顺序上做）。
///
/// 落位语义：移除被拖项之后，`target` 所在的位置就是插入点——于是"拖到谁身上
/// 就占谁的位置"，向左向右都对称（早先的写法向右拖会越过目标一格）。
/// 两个 id 有一个不在列表里时返回 `None`，调用方据此放弃这次落位。
fn move_to_position(full: &[i64], dragged: i64, target: i64) -> Option<Vec<i64>> {
    if dragged == target {
        return Some(full.to_vec());
    }

    let mut order = full.to_vec();
    let from = order.iter().position(|&x| x == dragged)?;
    order.remove(from);
    let to = order.iter().position(|&x| x == target)?;
    order.insert(to, dragged);

    Some(order)
}

/// 一条书签的有效分组：`group_id` 指向已删除的分组时按「未分组」处理。
///
/// 删除分组时会把组内书签的 `group_id` 清空，正常流程不会留下悬空引用；
/// 这里兜住的是"删分组与读列表并发"这类边角，避免书签在任何标签下都看不见。
fn effective_group(bookmark: &Bookmark, known: &HashSet<i64>) -> Option<i64> {
    bookmark.group_id.filter(|id| known.contains(id))
}

/// 关键词匹配：空格分词，词之间 AND，每词在 title / url / description / tags 里找。
///
/// 口径与服务端 `list_bookmarks(query)` 的 SQL 一致，只是搬到客户端做，
/// 这样输入即筛，不用每敲一个字就发一次请求。
fn matches_keyword(bookmark: &Bookmark, query: &str) -> bool {
    let keywords: Vec<String> = query
        .split_whitespace()
        .map(|word| word.to_lowercase())
        .collect();
    if keywords.is_empty() {
        return true;
    }

    let title = bookmark.title.to_lowercase();
    let url = bookmark.url.to_lowercase();
    let description = bookmark.description.to_lowercase();
    let tags = bookmark.tags.to_lowercase();

    keywords.iter().all(|word| {
        title.contains(word)
            || url.contains(word)
            || description.contains(word)
            || tags.contains(word)
    })
}

/// 按分组过滤值统计条数，键与标签页的 `value` 一致。
fn count_by_group(items: &[Bookmark], known: &HashSet<i64>) -> HashMap<String, usize> {
    let mut counts: HashMap<String, usize> = HashMap::new();

    for item in items {
        let value = match effective_group(item, known) {
            Some(id) => group_value(id),
            None => GROUP_UNGROUPED.to_string(),
        };
        *counts.entry(value).or_insert(0) += 1;
    }

    counts
}

/// 滚轮事件的纵向增量。
///
/// `WheelDelta` 按 `deltaMode` 分成像素 / 行 / 页三种单位，数值不能直接比较，
/// 但方向是可靠的——翻页只关心方向，所以统一取出 y。
fn wheel_delta_y(evt: &WheelEvent) -> f64 {
    match evt.delta() {
        WheelDelta::Pixels(v) => v.y,
        WheelDelta::Lines(v) => v.y,
        WheelDelta::Pages(v) => v.y,
    }
}

/// 主页：背景 + 设置入口 + 时钟 + 一言 + 搜索引擎框 + 分类标签页 + 图标网格。
#[component]
pub fn Home() -> Element {
    // 首屏（含 SSR）用 server future 直出，hydrate 时直接复用序列化好的值。
    // 刻意不带 refresh 依赖：一旦重跑，use_server_future 会把状态置回 Pending
    // 触发 suspend，Suspense 边界闪一下「加载中」。
    let initial = use_server_future(move || async move { list_bookmarks(String::new()).await })?;
    let initial_settings = use_server_future(move || async move { get_settings().await })?;

    let mut refresh = use_signal(|| 0u32);
    let mut settings_refresh = use_signal(|| 0u32);
    let mut groups_refresh = use_signal(|| 0u32);
    let mut engines_refresh = use_signal(|| 0u32);

    let bookmarks_res = use_resource(move || async move {
        // 首次渲染（refresh 仍为 0）直接用服务端带来的数据，避免首屏多打一次接口
        if refresh() == 0 {
            if let Some(v) = (&*initial.peek()).clone() {
                return v;
            }
        }
        list_bookmarks(String::new()).await
    });

    let settings_res = use_resource(move || async move {
        if settings_refresh() == 0 {
            if let Some(v) = (&*initial_settings.peek()).clone() {
                return v;
            }
        }
        get_settings().await
    });

    let groups_res = use_resource(move || async move {
        let _ = groups_refresh();
        list_groups().await
    });

    let engines_res = use_resource(move || async move {
        let _ = engines_refresh();
        list_engines().await
    });

    // ---- 状态 ----
    let mut query = use_signal(String::new);
    let mut search_mode = use_signal(|| SearchMode::Web);
    let mut active_group = use_signal(String::new);
    let mut page = use_signal(|| 0usize);
    let mut editing = use_signal(|| false);
    let mut selecting = use_signal(|| false);
    let mut selected = use_signal(Vec::<i64>::new);
    let mut drawer_open = use_signal(|| false);
    let mut menu = use_signal(|| Option::<(f64, f64, Option<Bookmark>)>::None);
    let mut modal = use_signal(|| ModalState::None);

    // ---- 快照：一律取克隆值，避免在 rsx 里和借用纠缠 ----
    let snapshot = match (&*bookmarks_res.read()).clone() {
        Some(v) => Some(v),
        None => (&*initial.read()).clone(),
    };
    let settings: Settings = match (&*settings_res.read()).clone() {
        Some(Ok(found)) => found,
        Some(Err(_)) => Settings::default(),
        None => match (&*initial_settings.read()).clone() {
            Some(Ok(found)) => found,
            _ => Settings::default(),
        },
    };
    let groups: Vec<Group> = match (&*groups_res.read()).clone() {
        Some(Ok(list)) => list,
        _ => Vec::new(),
    };
    let engines = match (&*engines_res.read()).clone() {
        Some(Ok(list)) => list,
        _ => Vec::new(),
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
    let total = all_items.len();

    // ---- 筛选 ----
    let known_group_ids: HashSet<i64> = groups.iter().map(|group| group.id).collect();
    // 关键词只在"筛书签"模式生效；搜网页模式下输入框的内容与列表无关
    let keyword = if search_mode() == SearchMode::Filter {
        query()
    } else {
        String::new()
    };

    let keyword_matched: Vec<Bookmark> = all_items
        .iter()
        .filter(|item| matches_keyword(item, &keyword))
        .cloned()
        .collect();

    // 搜索联想里的"本地命中"那几条。只有「搜网页」模式才需要——筛书签模式下
    // 下方网格本身就在实时筛，再给个面板是重复。
    let local_suggest: Vec<Bookmark> = if search_mode() == SearchMode::Web && !query().trim().is_empty()
    {
        all_items
            .iter()
            .filter(|item| matches_keyword(item, &query()))
            .take(4)
            .cloned()
            .collect()
    } else {
        Vec::new()
    };

    // 标签页的条数跟着当前结果集走（和原来的行为一致）
    let counts = count_by_group(&keyword_matched, &known_group_ids);
    let tabs = build_tabs(&groups, &counts);

    // 选中的分组若在结果里不存在了（被删掉，或该组已空），回落到「全部」
    let selected_tab = active_group();
    let active_value = if selected_tab.is_empty() || tabs.iter().any(|tab| tab.value == selected_tab)
    {
        selected_tab
    } else {
        String::new()
    };
    let filter = parse_filter(&active_value);

    let matched: Vec<Bookmark> = keyword_matched
        .into_iter()
        .filter(|item| match filter {
            Filter::All => true,
            Filter::Ungrouped => effective_group(item, &known_group_ids).is_none(),
            Filter::Group(id) => effective_group(item, &known_group_ids) == Some(id),
        })
        .collect();

    // ---- 分页 ----
    let page_size = settings.page_size.max(0) as usize;
    let page_count = if page_size == 0 {
        1
    } else {
        matched.len().div_ceil(page_size).max(1)
    };
    let current_page = page().min(page_count - 1);
    let view: Vec<Bookmark> = if page_size == 0 {
        matched.clone()
    } else {
        let start = current_page * page_size;
        let end = (start + page_size).min(matched.len());
        matched[start..end].to_vec()
    };

    let is_editing = editing();
    let is_selecting = selecting();

    // ---- 背景 ----
    // 随机模式用的一次性参数：每次打开页面生成一个，用来绕开图片接口的长缓存。
    // 只在客户端生成——SSR 阶段设置本就还没加载，两边都是"空"，因此不会
    // hydration 失配。
    #[cfg_attr(feature = "server", allow(unused_mut))]
    let mut bg_nonce = use_signal(String::new);
    use_effect(move || {
        #[cfg(feature = "web")]
        if bg_nonce().is_empty() {
            bg_nonce.set((js_sys::Date::now() as u64).to_string());
        }
    });

    // 首屏把焦点送进页面。加载完成后 activeElement 是 body，而 body 不在
    // .stage-root 子树里，挂在根节点上的快捷键（Esc / Ctrl+K / 翻页）一个都收不到。
    // 只做一次，且仅在焦点确实无处可去时——别抢用户已经点好的位置。
    #[cfg_attr(not(feature = "web"), allow(unused_variables))]
    let focus_once = use_hook(|| std::cell::Cell::new(false));
    use_effect(move || {
        #[cfg(feature = "web")]
        if !focus_once.get() {
            focus_once.set(true);
            if crate::dom::focus_is_on_body() {
                crate::dom::focus_element(SEARCH_INPUT_ID);
            }
        }
    });

    let has_wallpaper = !settings.background_url.is_empty();
    let nonce = bg_nonce();
    let wallpaper_ready = has_wallpaper && (!settings.background_random || !nonce.is_empty());
    // 壁纸层。地址为空（或随机参数还没就绪）时显式写 none：
    // dioxus 的解释器把 style 设成空串时，会把"之前设过、现在没了"的内联属性
    // 补回来，于是"清空背景图"要刷新才生效。写死 none 就绕开了这条规则——
    // 属性始终有值，没有需要补回来的东西。
    let bg_style = if wallpaper_ready {
        // 括号、引号、分号会破坏 url("…") 的写法，直接抹掉
        let mut url = settings.background_url.replace(['"', '\'', ';', '\\'], "");
        if settings.background_random {
            let sep = if url.contains('?') { '&' } else { '?' };
            url = format!("{url}{sep}_={nonce}");
        }
        format!("background-image: url(\"{url}\");")
    } else {
        "background-image: none;".to_string()
    };

    // 底衬层：默认渐变或内置预设。单独一层，这样壁纸层写 none 时能透出它；
    // 合成一层的话，"内联 background-image"和"CSS 类里的渐变"会互相打架。
    let base_class = if !has_wallpaper && !settings.background_preset.is_empty() {
        format!("bg-base preset-{}", settings.background_preset)
    } else {
        "bg-base".to_string()
    };

    let dim_alpha = if wallpaper_ready {
        settings.background_dim.clamp(0, 80) as f32 / 100.0
    } else {
        0.0
    };

    // ---- 布局：卡片尺寸/间距/列数都用 CSS 变量下发 ----
    let stage_style = format!(
        "--tile-size: {}px; --tile-gap-x: {}px; --tile-gap-y: {}px; --grid-template: {};",
        settings.tile_size.clamp(56, 160),
        settings.tile_gap_x.clamp(0, 48),
        settings.tile_gap_y.clamp(0, 64),
        settings.grid_template_columns(),
    );

    // ---- 拖拽落位 ----
    //
    // 落位语义是「把被拖的这条挪到目标那条的位置」，在全量顺序上做，因此
    // **跨页拖拽也成立**（分页只影响显示，order_ids 始终是全量）。
    // 早先用的是"在视图内置换"的合并写法，视图外的位置不动；那个写法在跨页时
    // 会直接失效——被拖的条目不在新页的视图集合里，校验不过就整单放弃。
    let full_ids: Vec<i64> = all_items.iter().map(|item| item.id).collect();
    let on_reorder = move |(dragged, target): (i64, i64)| {
        if dragged == target {
            return;
        }

        let Some(order) = move_to_position(&full_ids, dragged, target) else {
            return;
        };

        spawn(async move {
            if reorder_bookmarks(order).await.is_ok() {
                refresh.set(refresh() + 1);
            }
        });
    };

    // ---- 选择 ----
    let toggle_select = move |id: i64| {
        let mut list = selected();
        match list.iter().position(|&x| x == id) {
            Some(pos) => {
                list.remove(pos);
            }
            None => list.push(id),
        }
        selected.set(list);
    };

    let mut exit_selecting = move || {
        selecting.set(false);
        selected.set(Vec::new());
    };

    let selected_ids = selected();
    let selected_count = selected_ids.len();
    // "全选本页"与跨页拖拽都要知道当前页有哪些 id
    let view_ids: Vec<i64> = view.iter().map(|item| item.id).collect();
    let view_for_select_all = view_ids.clone();

    // ---- 翻页 ----
    // 先取出页面配置：闭包要能 Copy 才能被多处捕获，所以不能让它们抓住整个 settings
    let page_loop = settings.page_loop;
    let mut last_wheel = use_signal(|| 0.0f64);
    let mut jump_page = move |target: usize| {
        let target = if page_loop {
            if page_count == 0 {
                0
            } else {
                target % page_count
            }
        } else {
            target.min(page_count.saturating_sub(1))
        };
        page.set(target);
    };

    // 触屏滑动翻页用这次按下的起点
    let mut swipe_start = use_signal(|| Option::<f64>::None);
    // 拖拽翻页的节流时间戳
    let mut last_flip = use_signal(|| 0.0f64);

    let delete_title = match &modal() {
        ModalState::ConfirmDelete { message, .. } => message.clone(),
        _ => String::new(),
    };

    let run_delete = move |ids: Vec<i64>| {
        spawn(async move {
            // 单条走 delete_bookmark，多条走批量版本；两者行为一致（都是软删除）
            let ok = if ids.len() == 1 {
                delete_bookmark(ids[0]).await.is_ok()
            } else {
                delete_bookmarks(ids).await.is_ok()
            };
            if ok {
                selected.set(Vec::new());
                refresh.set(refresh() + 1);
            }
        });
    };

    rsx! {
        // 主题色跟着主题走（移动端地址栏）。dioxus 0.7 没有 document::Html，
        // 所以主题属性落在 .stage-root 上，body 的背景由 .bg 那一层负责铺。
        document::Meta {
            name: "theme-color",
            content: if settings.theme == "light" { "#eef1f6" } else { "#0b0d13" },
        }

        div {
            class: "stage-root",
            style: "{stage_style}",
            "data-theme": "{settings.theme}",
            // 键盘事件要靠焦点冒泡上来，所以根节点必须可获得焦点
            tabindex: "0",

            onkeydown: move |evt| {
                let key = evt.key();
                let modifiers = evt.modifiers();

                // Ctrl/Cmd + K：回到搜索框
                if matches!(&key, Key::Character(c) if c.eq_ignore_ascii_case("k"))
                    && (modifiers.contains(Modifiers::CONTROL) || modifiers.contains(Modifiers::META))
                {
                    evt.prevent_default();
                    crate::dom::focus_element(SEARCH_INPUT_ID);
                    return;
                }

                if key == Key::Escape {
                    // 按层级从最上层往下关：菜单 → 弹窗 → 抽屉 → 选择/编辑态
                    if menu().is_some() {
                        menu.set(None);
                    } else if modal() != ModalState::None {
                        modal.set(ModalState::None);
                    } else if drawer_open() {
                        drawer_open.set(false);
                    } else if selecting() {
                        exit_selecting();
                    } else if editing() {
                        editing.set(false);
                    } else {
                        return;
                    }
                    evt.prevent_default();
                    return;
                }

                // 翻页键。焦点停在有内容的文本框里时让路给光标
                if page_count > 1 && !crate::dom::focus_is_editing() {
                    match key {
                        Key::ArrowLeft | Key::PageUp => {
                            evt.prevent_default();
                            jump_page(current_page.saturating_sub(1));
                        }
                        Key::ArrowRight | Key::PageDown => {
                            evt.prevent_default();
                            jump_page(current_page + 1);
                        }
                        _ => {}
                    }
                }
            },

            // 空白处右键 → 背景菜单
            oncontextmenu: move |evt| {
                evt.prevent_default();
                let coords = evt.client_coordinates();
                menu.set(Some((coords.x, coords.y, None)));
            },

            // 触屏左右滑动翻页（只认触摸，鼠标拖拽不受影响）
            onpointerdown: move |evt| {
                if page_count > 1 && evt.pointer_type() == "touch" && !editing() {
                    swipe_start.set(Some(evt.client_coordinates().x));
                }
            },
            onpointerup: move |evt| {
                let Some(start) = swipe_start() else { return };
                swipe_start.set(None);
                if page_count <= 1 {
                    return;
                }
                let dx = evt.client_coordinates().x - start;
                if dx.abs() < 60.0 {
                    return;
                }
                if dx < 0.0 {
                    jump_page(current_page + 1);
                } else {
                    jump_page(current_page.saturating_sub(1));
                }
            },

            div { class: "{base_class}" }
            div { class: "bg", style: "{bg_style}" }
            div { class: "bg-dim", style: "background: rgba(0, 0, 0, {dim_alpha});" }

            button {
                class: "settings-btn",
                r#type: "button",
                title: "主页设置（Ctrl+K 回到搜索框，Esc 关闭面板）",
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
                Clock {
                    show_lunar: settings.clock_show_lunar,
                    show_day: settings.clock_show_day,
                }
            }
            if settings.show_quote {
                QuoteLine {}
            }

            SearchBar {
                value: query(),
                oninput: move |v: String| query.set(v),
                mode: search_mode(),
                on_mode_change: move |mode: SearchMode| {
                    search_mode.set(mode);
                    // 换模式后把旧输入清掉：搜网页的关键词拿去筛卡片通常是空的，
                    // 留着会让人以为"书签都不见了"
                    query.set(String::new());
                },
                engines: engines.clone(),
                on_select_engine: move |id: i64| {
                    spawn(async move {
                        if set_active_engine(id).await.is_ok() {
                            engines_refresh.set(engines_refresh() + 1);
                        }
                    });
                },
                on_open_settings: move |_| modal.set(ModalState::EngineSettings),
                local_matches: local_suggest.clone(),
            }

            if settings.hide_grid {
                p { class: "hint", "简约模式已开启：主页设置 → 打开与翻页，可关闭。" }
            } else {
                section {
                    class: "stage",
                    // 开了滚轮翻页才拦滚轮，否则照常滚动页面
                    onwheel: move |evt: WheelEvent| {
                        if !settings.page_wheel || page_count <= 1 {
                            return;
                        }

                        // delta 的单位随设备不同（像素 / 行 / 页），只要方向
                        let delta = wheel_delta_y(&evt);
                        if delta == 0.0 {
                            return;
                        }

                        // 一次滚动会连发多个 wheel，节流一下免得一划跳好几页
                        let now = crate::dom::now_ms();
                        if now - last_wheel() < 320.0 {
                            evt.prevent_default();
                            return;
                        }
                        last_wheel.set(now);
                        evt.prevent_default();

                        if delta > 0.0 {
                            jump_page(current_page + 1);
                        } else {
                            jump_page(current_page.saturating_sub(1));
                        }
                    },

                    if !tabs.is_empty() {
                        GroupTabs {
                            tabs: tabs.clone(),
                            active: active_value.clone(),
                            on_select: move |tag: String| {
                                active_group.set(tag);
                                page.set(0);
                            },
                        }
                    }

                    if let Some(err) = error.clone() {
                        div { class: "state state-error", "加载失败：{err}" }
                    } else if loading {
                        div { class: "state state-loading", "加载中…" }
                    } else {
                        if matched.is_empty() {
                            div { class: "hint",
                                if total == 0 {
                                    "还没有书签：在空白处右键 → 添加卡片"
                                } else {
                                    "没有符合条件的书签。"
                                }
                            }
                        }
                        BookmarkGrid {
                            items: view.clone(),
                            editing: is_editing,
                            selecting: is_selecting,
                            selected: selected_ids.clone(),
                            new_tab: settings.open_in_new_tab,
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
                                    ids: vec![bookmark.id],
                                    message: format!(
                                        "确定删除「{}」吗？它会进入回收站，可以在「主页设置 → 回收站」里还原。",
                                        bookmark.title
                                    ),
                                });
                            },
                            on_toggle_select: toggle_select,
                            on_reorder,
                            // 拖到翻页按钮上翻页：节流一下，免得一次悬停连翻好几页
                            on_page_flip: move |direction: isize| {
                                let now = crate::dom::now_ms();
                                if now - last_flip() < 700.0 {
                                    return;
                                }
                                last_flip.set(now);

                                let target = if direction < 0 {
                                    current_page.saturating_sub(1)
                                } else {
                                    current_page + 1
                                };
                                jump_page(target);
                            },
                        }

                        Pager {
                            page: current_page,
                            page_count,
                            page_size,
                            total: matched.len(),
                            on_jump: move |target: usize| jump_page(target),
                        }
                    }
                }
            }

            // ---- 底栏：编辑态 / 批量选择态 ----
            if is_selecting {
                div { class: "edit-bar",
                    span { class: "edit-hint", "已选 {selected_count} 个" }
                    button {
                        class: "btn-ghost",
                        r#type: "button",
                        onclick: move |_| {
                            let ids = view_for_select_all.clone();
                            let current = selected();
                            // 本页全选中时再点一次＝取消本页选择，避免只能靠逐个点
                            let all_picked = !ids.is_empty()
                                && ids.iter().all(|id| current.contains(id));
                            let mut next: Vec<i64> = current
                                .into_iter()
                                .filter(|id| !ids.contains(id))
                                .collect();
                            if !all_picked {
                                next.extend(ids);
                            }
                            selected.set(next);
                        },
                        if page_size == 0 { "全选" } else { "全选本页" }
                    }
                    button {
                        class: "btn-ghost",
                        r#type: "button",
                        disabled: selected_count == 0,
                        onclick: move |_| modal.set(ModalState::BatchMove),
                        "移到分组"
                    }
                    button {
                        class: "btn-danger",
                        r#type: "button",
                        disabled: selected_count == 0,
                        onclick: move |_| {
                            modal.set(ModalState::ConfirmDelete {
                                ids: selected(),
                                message: format!(
                                    "确定删除选中的 {} 个书签吗？它们会进入回收站，可以在设置里还原。",
                                    selected_count
                                ),
                            });
                        },
                        "删除"
                    }
                    button {
                        class: "btn-primary",
                        r#type: "button",
                        onclick: move |_| exit_selecting(),
                        "退出选择"
                    }
                }
            } else if is_editing {
                div { class: "edit-bar",
                    span { class: "edit-hint", "按住卡片拖动排序 · 点卡片修改 · 点 ✕ 删除" }
                    button {
                        class: "btn-ghost",
                        r#type: "button",
                        onclick: move |_| {
                            editing.set(false);
                            selecting.set(true);
                            selected.set(Vec::new());
                        },
                        "批量选择"
                    }
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
                    groups: groups.clone(),
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
                            ids: vec![bookmark.id],
                            message: format!(
                                "确定删除「{}」吗？它会进入回收站，可以在「主页设置 → 回收站」里还原。",
                                bookmark.title
                            ),
                        });
                    },
                    on_move: move |(bookmark, group_id): (Bookmark, Option<i64>)| {
                        menu.set(None);
                        spawn(async move {
                            if move_bookmarks(vec![bookmark.id], group_id).await.is_ok() {
                                refresh.set(refresh() + 1);
                                groups_refresh.set(groups_refresh() + 1);
                            }
                        });
                    },
                    on_batch: move |bookmark: Bookmark| {
                        menu.set(None);
                        editing.set(false);
                        selecting.set(true);
                        selected.set(vec![bookmark.id]);
                    },
                }
            }

            if drawer_open() {
                Drawer {
                    title: "主页设置",
                    wide: true,
                    on_close: move |_| drawer_open.set(false),
                    SettingsPanel {
                        settings: settings.clone(),
                        groups: groups.clone(),
                        on_settings_saved: move |_| {
                            settings_refresh.set(settings_refresh() + 1);
                            // 卡片尺寸之类的设置可能改了排序/分组展示，一起刷新
                            groups_refresh.set(groups_refresh() + 1);
                        },
                        on_bookmarks_changed: move |_| refresh.set(refresh() + 1),
                        on_groups_changed: move |_| {
                            groups_refresh.set(groups_refresh() + 1);
                            refresh.set(refresh() + 1);
                        },
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
                            groups: groups.clone(),
                            // 当前就停在某个分组下时，默认归到那一组
                            default_group: match filter {
                                Filter::Group(id) => Some(id),
                                _ => None,
                            },
                            on_cancel: move |_| modal.set(ModalState::None),
                            on_saved: move |_| {
                                modal.set(ModalState::None);
                                refresh.set(refresh() + 1);
                                groups_refresh.set(groups_refresh() + 1);
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
                            groups: groups.clone(),
                            default_group: None,
                            on_cancel: move |_| modal.set(ModalState::None),
                            on_saved: move |_| {
                                modal.set(ModalState::None);
                                refresh.set(refresh() + 1);
                                groups_refresh.set(groups_refresh() + 1);
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
                ModalState::ConfirmDelete { ids, .. } => rsx! {
                    ConfirmDialog {
                        title: "删除书签",
                        message: delete_title.clone(),
                        confirm_label: "移入回收站",
                        on_cancel: move |_| modal.set(ModalState::None),
                        on_confirm: move |_| {
                            modal.set(ModalState::None);
                            let ids = ids.clone();
                            run_delete(ids);
                        },
                    }
                },
                ModalState::BatchMove => rsx! {
                    Modal {
                        title: "移动到分组",
                        on_close: move |_| modal.set(ModalState::None),
                        BatchMoveList {
                            groups: groups.clone(),
                            count: selected_count,
                            on_cancel: move |_| modal.set(ModalState::None),
                            on_pick: move |group_id: Option<i64>| {
                                let ids = selected();
                                spawn(async move {
                                    if move_bookmarks(ids, group_id).await.is_ok() {
                                        modal.set(ModalState::None);
                                        selected.set(Vec::new());
                                        refresh.set(refresh() + 1);
                                        groups_refresh.set(groups_refresh() + 1);
                                    }
                                });
                            },
                        }
                    }
                },
                ModalState::None => rsx! {},
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Bookmark;

    fn bookmark(id: i64, title: &str, url: &str, description: &str, tags: &str) -> Bookmark {
        Bookmark {
            id,
            title: title.to_string(),
            url: url.to_string(),
            description: description.to_string(),
            tags: tags.to_string(),
            group_id: None,
            icon: String::new(),
            created_at: String::new(),
        }
    }

    #[test]
    fn keyword_matches_all_four_fields() {
        let item = bookmark(1, "Rust 官网", "https://rust-lang.org", "系统编程语言", "编程,rust");

        assert!(matches_keyword(&item, "rust"), "标题命中");
        assert!(matches_keyword(&item, "官网"), "标题中文命中");
        assert!(matches_keyword(&item, "lang"), "URL 命中");
        assert!(matches_keyword(&item, "编程语言"), "描述命中");
        assert!(matches_keyword(&item, "编程"), "标签命中");
        assert!(!matches_keyword(&item, "python"));
    }

    #[test]
    fn keyword_is_case_insensitive() {
        let item = bookmark(1, "GitHub", "https://github.com", "", "");
        assert!(matches_keyword(&item, "github"));
        assert!(matches_keyword(&item, "GITHUB"));
    }

    #[test]
    fn keywords_are_anded() {
        let item = bookmark(1, "Rust 官网", "https://rust-lang.org", "", "编程");
        assert!(matches_keyword(&item, "rust 官网"));
        assert!(!matches_keyword(&item, "rust python"), "多词是 AND");
    }

    #[test]
    fn empty_keyword_matches_everything() {
        let item = bookmark(1, "A", "https://a.com", "", "");
        assert!(matches_keyword(&item, ""));
        assert!(matches_keyword(&item, "   "));
    }

    #[test]
    fn effective_group_drops_dangling_ids() {
        let known: HashSet<i64> = [1, 2].into_iter().collect();

        let mut item = bookmark(1, "A", "https://a.com", "", "");
        item.group_id = Some(1);
        assert_eq!(effective_group(&item, &known), Some(1));

        item.group_id = Some(99);
        assert_eq!(effective_group(&item, &known), None, "指向已删分组＝未分组");

        item.group_id = None;
        assert_eq!(effective_group(&item, &known), None);
    }

    #[test]
    fn counts_include_ungrouped_bucket() {
        let known: HashSet<i64> = [1].into_iter().collect();
        let mut a = bookmark(1, "A", "https://a.com", "", "");
        a.group_id = Some(1);
        let mut b = bookmark(2, "B", "https://b.com", "", "");
        b.group_id = Some(99); // 悬空 → 未分组
        let c = bookmark(3, "C", "https://c.com", "", "");

        let counts = count_by_group(&[a, b, c], &known);
        assert_eq!(counts.get(&crate::components::group_value(1)), Some(&1));
        assert_eq!(counts.get(GROUP_UNGROUPED), Some(&2));
    }

    #[test]
    fn move_lands_on_the_targets_position() {
        let full = vec![1, 2, 3, 4];

        // 向右拖：把 1 拖到 3 身上 → 1 占 3 原来的位置
        assert_eq!(move_to_position(&full, 1, 3).unwrap(), vec![2, 1, 3, 4]);
        // 向左拖：把 4 拖到 2 身上 → 4 占 2 原来的位置
        assert_eq!(move_to_position(&full, 4, 2).unwrap(), vec![1, 4, 2, 3]);
        // 拖到自己身上：顺序不变
        assert_eq!(move_to_position(&full, 2, 2).unwrap(), full);
    }

    #[test]
    fn move_preserves_the_set_of_ids() {
        let full = vec![10, 20, 30, 40, 50];

        for (dragged, target) in [(10, 50), (50, 10), (30, 30), (20, 40)] {
            let moved = move_to_position(&full, dragged, target).unwrap();
            assert_eq!(moved.len(), full.len());
            let mut sorted = moved.clone();
            sorted.sort_unstable();
            assert_eq!(sorted, full, "不丢条目、不重复");
        }
    }

    #[test]
    fn move_rejects_unknown_ids() {
        let full = vec![1, 2, 3];
        assert!(move_to_position(&full, 9, 1).is_none(), "被拖项不在列表里");
        assert!(move_to_position(&full, 1, 9).is_none(), "目标不在列表里");
    }

    /// 跨页拖拽靠的就是这个：移动在全量顺序上做，与"当前看的是哪一页"无关。
    #[test]
    fn move_works_across_pages() {
        // 全量 1..8，每页 4 条；把第 1 页的 2 拖到第 2 页的 7 身上
        let full: Vec<i64> = (1..=8).collect();
        let moved = move_to_position(&full, 2, 7).unwrap();
        assert_eq!(moved, vec![1, 3, 4, 5, 6, 2, 7, 8]);
        // 第 2 页（下标 4..8）现在以 2 开头，说明确实跨过去了
        assert_eq!(&moved[4..8], &[6, 2, 7, 8]);
    }
}
