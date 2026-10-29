use std::collections::{HashMap, HashSet};

use dioxus::prelude::*;

use crate::backend::{
    delete_bookmark, delete_bookmarks, delete_engine, get_settings, list_bookmarks, list_engines,
    list_groups, move_bookmarks, reorder_bookmarks, set_active_engine,
};
use crate::components::{
    build_tabs, group_value, parse_filter, AddCardDialog, AddTab, BatchMoveList, BookmarkForm,
    CardGrid, Clock, ConfirmDialog, ContextMenu, Drawer, EngineSettings, Filter, GroupTabs, Modal,
    Pager, QuoteLine, SearchBar, SearchMode, SettingsPanel, WidgetForm, GROUP_UNGROUPED,
    SEARCH_INPUT_ID,
};
use crate::models::{widget_label, Bookmark, Group, Settings};
use dioxus::html::geometry::WheelDelta;

/// 弹窗状态机。
#[derive(Clone, PartialEq)]
enum ModalState {
    None,
    /// 「添加卡片」弹窗。`tab` 是打开时停在的标签页，`kind` 是已经选好的组件类型
    /// （从右键菜单直接点某个组件进来时带着）。书签与组件共用这一个弹窗，入口因此
    /// 只剩一处——原来「加书签」和「加组件」是两条独立的路径。
    AddCard {
        tab: AddTab,
        kind: Option<String>,
    },
    Edit {
        bookmark: Bookmark,
    },
    EngineSettings,
    /// 删除确认。单条与批量共用：`ids` 是要删的集合，`message` 是文案。
    ConfirmDelete {
        ids: Vec<i64>,
        message: String,
    },
    BatchMove,
}

/// 把卡片分成「组件」与「书签」两组，各自保持原有顺序。
///
/// 分流只做一次，下面是两条不同的路：书签照旧走关键词 → 分组 → 分页；组件直接
/// 拼回显示列表的最前面，**不参与**这三步——组件是"少、固定、一眼要看到"的东西，
/// 被搜索筛掉或翻到第二页都只会让版面突然缺一块。
fn split_cards(cards: &[Bookmark]) -> (Vec<Bookmark>, Vec<Bookmark>) {
    let mut widgets = Vec::new();
    let mut bookmarks = Vec::new();

    for card in cards {
        if card.is_widget() {
            widgets.push(card.clone());
        } else {
            bookmarks.push(card.clone());
        }
    }

    (widgets, bookmarks)
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

/// 一条书签的有效分组列表：指向已删除分组的 id 被丢掉，剩下的按原顺序去重。
///
/// 删除分组时服务端会把该 id 从 `group_ids` 里摘掉，正常流程不会留下悬空引用；
/// 这里兜住的是"删分组与读列表并发"这类边角，避免书签在任何标签下都看不见。
fn effective_groups(bookmark: &Bookmark, known: &HashSet<i64>) -> Vec<i64> {
    let mut out: Vec<i64> = Vec::new();
    for id in bookmark.group_id_list() {
        if known.contains(&id) && !out.contains(&id) {
            out.push(id);
        }
    }
    out
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

/// 一段字符串的散列（FNV-1a，与壁纸取文件名用的是同一个常数）。
///
/// 只用来把 nonce 摊到背景池的下标上，不需要任何密码学强度。
#[cfg_attr(not(feature = "web"), allow(dead_code))]
fn hash_str(value: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in value.as_bytes() {
        hash = (hash ^ *byte as u64).wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// 背景池这次轮到第几条；`None` 表示不该轮换（池里不足两条）。
///
/// 用 nonce 取下标而不是用随机数：同一次页面加载里组件会重渲染很多次，
/// 每次都得算出同一张，否则界面一动背景就换。nonce 每次打开都不同，
/// 随机性照样有——代价是池里只有两张时，有 1/2 的概率连着两次给同一张。
/// 要去掉这个概率就得记点状态（localStorage 或服务端游标），本应用不留任何
/// 这类痕迹：新标签页不该往浏览器里写东西。
#[cfg_attr(not(feature = "web"), allow(dead_code))]
fn rotation_index(pool: &[String], nonce: &str) -> Option<usize> {
    if pool.len() < 2 {
        return None;
    }

    Some((hash_str(nonce) % pool.len() as u64) as usize)
}

/// 按分组过滤值统计条数，键与标签页的 `value` 一致。
///
/// 多分组书签会在它所属的每个分组下各计一次；一条分组都没归的记账到「未分组」。
/// 「全部」的条数不能把这张表加起来（会重复计数），由调用方单独传总数。
fn count_by_group(items: &[Bookmark], known: &HashSet<i64>) -> HashMap<String, usize> {
    let mut counts: HashMap<String, usize> = HashMap::new();

    for item in items {
        let groups = effective_groups(item, known);
        if groups.is_empty() {
            *counts.entry(GROUP_UNGROUPED.to_string()).or_insert(0) += 1;
        } else {
            for id in groups {
                *counts.entry(group_value(id)).or_insert(0) += 1;
            }
        }
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

    // ---- 组件 / 书签分流 ----
    // 分流在最前面做，于是下面整条"关键词 → 分组 → 分页"的管线只跑书签，
    // 组件在最后拼回显示列表的最前面（见下方 `view`）。
    let (widgets, bookmarks) = split_cards(&all_items);
    // 分组条数与"还没有书签"的空态提示都只看书签：有组件不等于有书签
    let total = bookmarks.len();

    // ---- 筛选 ----
    let known_group_ids: HashSet<i64> = groups.iter().map(|group| group.id).collect();
    // 关键词只在"筛书签"模式生效；搜网页模式下输入框的内容与列表无关
    let keyword = if search_mode() == SearchMode::Filter {
        query()
    } else {
        String::new()
    };

    let keyword_matched: Vec<Bookmark> = bookmarks
        .iter()
        .filter(|item| matches_keyword(item, &keyword))
        .cloned()
        .collect();

    // 标签页的条数跟着当前结果集走（和原来的行为一致）
    let counts = count_by_group(&keyword_matched, &known_group_ids);
    let tabs = build_tabs(&groups, &counts, keyword_matched.len());

    // 选中的分组若在结果里不存在了（被删掉，或该组已空），回落到「全部」
    let selected_tab = active_group();
    let active_value =
        if selected_tab.is_empty() || tabs.iter().any(|tab| tab.value == selected_tab) {
            selected_tab
        } else {
            String::new()
        };
    let filter = parse_filter(&active_value);

    let matched: Vec<Bookmark> = keyword_matched
        .into_iter()
        .filter(|item| match filter {
            Filter::All => true,
            Filter::Ungrouped => effective_groups(item, &known_group_ids).is_empty(),
            Filter::Group(id) => effective_groups(item, &known_group_ids).contains(&id),
        })
        .collect();

    // ---- 分页（只对书签） ----
    let page_size = settings.page_size.max(0) as usize;
    let page_count = if page_size == 0 {
        1
    } else {
        matched.len().div_ceil(page_size).max(1)
    };
    let current_page = page().min(page_count - 1);
    let page_of_bookmarks: Vec<Bookmark> = if page_size == 0 {
        matched.clone()
    } else {
        let start = current_page * page_size;
        let end = (start + page_size).min(matched.len());
        matched[start..end].to_vec()
    };

    // 显示列表 = 组件（按它们自己的顺序）++ 本页书签。
    // 组件与书签的 sort_order 共用一列，但显示时按 kind 分两段，所以两边互不干扰：
    // 拖拽也只在同类之间生效（见 on_reorder），于是这一段始终是稳定的前缀。
    let view: Vec<Bookmark> = widgets.iter().cloned().chain(page_of_bookmarks).collect();

    let is_editing = editing();
    let is_selecting = selecting();

    // ---- 背景 ----
    // 这次显示哪张，由下面那个 effect 定，渲染只读结果。为什么不在渲染里算：
    // 随机源只在客户端存在，而渲染在 SSR 也得跑一遍——那时按"设置里选中的那条"
    // 渲染即可（两边都是"空"，不会 hydration 失配）。
    #[cfg_attr(feature = "server", allow(unused_mut))]
    let mut bg_picked = use_signal(|| Option::<String>::None);

    // 每次打开页面生成一个时间戳：既给外链当"绕开长缓存"的随机参数，
    // 也当背景池轮换的随机源。只在客户端生成（见上）。
    #[cfg_attr(feature = "server", allow(unused_mut))]
    let mut bg_nonce = use_signal(String::new);
    use_effect(move || {
        #[cfg(feature = "web")]
        {
            if bg_nonce().is_empty() {
                bg_nonce.set((js_sys::Date::now() as u64).to_string());
            }

            // 读资源而不是用上面那份快照：设置可能晚于挂载才到位，
            // 这里顺带订上，到了会再跑一次
            let fresh = match (&*settings_res.read()).clone() {
                Some(Ok(found)) => found,
                Some(Err(_)) => Settings::default(),
                None => match (&*initial_settings.read()).clone() {
                    Some(Ok(found)) => found,
                    _ => return,
                },
            };

            let nonce = bg_nonce();
            let picked = match (fresh.background_random && !nonce.is_empty())
                .then(|| rotation_index(&fresh.background_urls, &nonce))
                .flatten()
            {
                Some(index) => fresh.background_urls[index].clone(),
                None => fresh.background_url.clone(),
            };

            // peek：不要订阅自己写的信号
            if *bg_picked.peek() != Some(picked.clone()) {
                bg_picked.set(Some(picked));
            }
        }
    });

    // ---- 主题 ----
    // 系统深浅色偏好。`None` 表示还没探到——SSR 阶段就是这样，此时「跟随系统」
    // 按深色渲染，客户端探到后再重渲染，于是首屏不会 hydration 失配。
    #[cfg_attr(feature = "server", allow(unused_mut))]
    let mut system_dark = use_signal(|| Option::<bool>::None);
    #[cfg_attr(not(feature = "web"), allow(unused_variables))]
    let theme_probe_once = use_hook(|| std::cell::Cell::new(false));
    use_effect(move || {
        #[cfg(feature = "web")]
        if !theme_probe_once.get() {
            theme_probe_once.set(true);
            system_dark.set(Some(crate::dom::prefers_dark()));
            // 系统换主题时跟着换（只在选「跟随系统」时才有可见效果）
            crate::dom::watch_color_scheme(move |dark| system_dark.set(Some(dark)));
        }
    });
    let theme = settings.effective_theme(system_dark());

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

    // 轮换的结果还没送到（SSR、或随机参数未就绪）时用设置里选中的那条：
    // 它在"随机开着但池不足两条"时就是最终要显示的那张。
    let picked = bg_picked().unwrap_or_else(|| settings.background_url.clone());

    let has_wallpaper = !picked.is_empty();
    let nonce = bg_nonce();
    let wallpaper_ready = has_wallpaper && (!settings.background_random || !nonce.is_empty());
    // 壁纸层。地址为空（或随机参数还没就绪）时显式写 none：
    // dioxus 的解释器把 style 设成空串时，会把"之前设过、现在没了"的内联属性
    // 补回来，于是"清空背景图"要刷新才生效。写死 none 就绕开了这条规则——
    // 属性始终有值，没有需要补回来的东西。
    let bg_style = if wallpaper_ready {
        // 括号、引号、分号会破坏 url("…") 的写法，直接抹掉
        let mut url = picked.replace(['"', '\'', ';', '\\'], "");
        // 追加随机参数只为绕开图片接口的长缓存。本机的 `/wallpaper/xxx` 内容不会变、
        // 响应本来就带 immutable 长缓存，再追加只是白下一次请求——所以只加在外链上。
        if settings.background_random && url.starts_with("http") {
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
    // 列数只封顶（--grid-max 是 max-width），轨道数本身始终自适应，
    // 所以窄屏放不下时会自动换行，而不是横向溢出。
    // --stage-max 是外层那一列的上限：网格的 100% 取的是它，所以它必须放得下
    // 设定的列数，否则 940px 会先把列数卡死在 9 列（见 models::stage_max_width）。
    let stage_style = format!(
        "--tile-size: {}px; --tile-gap-x: {}px; --tile-gap-y: {}px; \
         --grid-template: {}; --grid-max: {}; --stage-max: {};",
        settings.tile_size.clamp(56, 160),
        settings.tile_gap_x.clamp(0, 48),
        settings.tile_gap_y.clamp(0, 64),
        settings.grid_template_columns(),
        settings.grid_max_width(),
        settings.stage_max_width(),
    );

    // ---- 网格实际列数 ----
    // 跨格卡片不能超过容器当前排得出的列数（否则会撑出隐式列、横向溢出），
    // 而列数由容器宽度决定、跟设置无关，所以只能实测宽度后现算。
    #[cfg_attr(feature = "server", allow(unused_mut))]
    let mut grid_width = use_signal(|| 0.0f64);
    use_future(move || async move {
        #[cfg(feature = "web")]
        loop {
            // 直接量 DOM，不用 eval 的消息通道（那个通道在脚本同步部分跑完就关了，
            // 异步回调里的 send 收不到，见 dom::element_width 的说明）
            if let Some(width) = crate::dom::element_width(crate::dom::GRID_ELEMENT_ID) {
                // peek 而不是读信号：免得这个循环订阅自己写的值
                if (width - *grid_width.peek()).abs() > 0.5 {
                    grid_width.set(width);
                }
            }
            // 宽度只在窗口尺寸变化时变，250ms 足够跟上，也不至于把重排拖到肉眼可见
            gloo_timers::future::TimeoutFuture::new(250).await;
        }
    });
    // 还没量到宽度时传 None：不夹取，而不是把每张卡都压成 1 格
    let grid_columns = (grid_width() > 0.0)
        .then(|| crate::models::columns_for(grid_width(), settings.tile_size, settings.tile_gap_x));

    // ---- 拖拽落位 ----
    //
    // 落位语义是「把被拖的这条挪到目标那条的位置」，在**全量顺序**上做，因此
    // **跨页拖拽也成立**（分页只影响显示，order_ids 始终是全量）。
    // 早先用的是"在视图内置换"的合并写法，视图外的位置不动；那个写法在跨页时
    // 会直接失效——被拖的条目不在新页的视图集合里，校验不过就整单放弃。
    //
    // 这里的"全量顺序"取的是**显示顺序**（组件在前、随后是全部书签），而不是服务端
    // 返回的原始行序：两者在首次重排之前可能不一致，用显示顺序算落位才不会出现
    // "看到的顺序"和"算出来的顺序"错位。
    let full_ids: Vec<i64> = widgets
        .iter()
        .chain(bookmarks.iter())
        .map(|card| card.id)
        .collect();
    let widget_ids: HashSet<i64> = widgets.iter().map(|card| card.id).collect();
    let on_reorder = move |(dragged, target): (i64, i64)| {
        if dragged == target {
            return;
        }

        // 只认同类落位：组件固定在书签之前，把书签拖进组件区（或反过来）没有可解释
        // 的结果——显示列表始终按 kind 分两段，落了位也看不出变化，只会白白改一遍顺序。
        if widget_ids.contains(&dragged) != widget_ids.contains(&target) {
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
    // "全选本页"只认书签：组件不属于任何分组，也不该被批量删除。
    let view_for_select_all: Vec<i64> = view
        .iter()
        .filter(|item| !item.is_widget())
        .map(|item| item.id)
        .collect();

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

    let favicon_href = if settings.site_logo.trim().is_empty() {
        crate::FAVICON_SVG.to_string()
    } else {
        settings.site_logo.trim().to_string()
    };
    // 主题色：设置了就用它；留空跟随深浅色（与加这个设置之前的行为一致）。
    let theme_color = if !settings.site_theme_color.trim().is_empty() {
        settings.site_theme_color.trim().to_string()
    } else if theme == "light" {
        "#eef1f6".to_string()
    } else {
        "#0b0d13".to_string()
    };

    rsx! {
        // 站点品牌化：标题、描述、关键词、图标与地址栏主题色都来自设置。
        // 放在 Home 而不是 App，是因为这些值要等 settings 到位；SSR 阶段
        // `use_server_future` 已经把它们带过来了，首屏就是最终值。
        document::Title { "{settings.site_title}" }
        document::Meta { name: "description", content: "{settings.site_description}" }
        if !settings.site_keywords.trim().is_empty() {
            document::Meta { name: "keywords", content: "{settings.site_keywords}" }
        }
        document::Link { rel: "icon", href: "{favicon_href}" }
        document::Meta {
            name: "theme-color",
            content: "{theme_color}",
        }

        div {
            class: "stage-root",
            style: "{stage_style}",
            "data-theme": "{theme}",
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
                    hour12: settings.clock_hour12,
                    show_seconds: settings.clock_show_seconds,
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
                        CardGrid {
                            items: view.clone(),
                            columns: grid_columns,
                            editing: is_editing,
                            selecting: is_selecting,
                            selected: selected_ids.clone(),
                            new_tab: settings.open_in_new_tab,
                            on_add: move |_| {
                                modal.set(ModalState::AddCard {
                                    tab: AddTab::Bookmark,
                                    kind: None,
                                })
                            },
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
                            on_widget_changed: move |_| refresh.set(refresh() + 1),
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
                        modal.set(ModalState::AddCard {
                            tab: AddTab::Bookmark,
                            kind: None,
                        });
                    },
                    on_add_widget: move |kind: String| {
                        menu.set(None);
                        // 菜单是快捷方式：直接停到该组件的参数表单上，
                        // 而不是让用户再在弹窗里点一次
                        modal.set(ModalState::AddCard {
                            tab: AddTab::Widget,
                            kind: Some(kind),
                        });
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
                ModalState::AddCard { tab, kind } => rsx! {
                    Modal {
                        title: "添加卡片",
                        on_close: move |_| modal.set(ModalState::None),
                        AddCardDialog {
                            initial_tab: tab,
                            initial_kind: kind,
                            groups: groups.clone(),
                            // 当前就停在某个分组下时，默认归到那一组
                            default_group: match filter {
                                Filter::Group(id) => Some(id),
                                _ => None,
                            },
                            existing_widgets: widgets.clone(),
                            on_close: move |_| modal.set(ModalState::None),
                            on_saved: move |_| {
                                modal.set(ModalState::None);
                                refresh.set(refresh() + 1);
                                groups_refresh.set(groups_refresh() + 1);
                            },
                        }
                    }
                },
                ModalState::Edit { bookmark } => {
                    // 编辑入口是共用的（点卡片、右键菜单），但表单要按种类分：
                    // 组件没有网址、描述、分组、图标那些字段
                    let is_widget = bookmark.is_widget();
                    let card = bookmark.clone();
                    let title = if is_widget {
                        format!("设置{}", widget_label(&card.kind))
                    } else {
                        "修改网址导航".to_string()
                    };

                    rsx! {
                        Modal {
                            title,
                            on_close: move |_| modal.set(ModalState::None),
                            if is_widget {
                                WidgetForm {
                                    // key 让切换不同卡片时重新挂载，避免沿用上一个的预填值
                                    key: "{card.id}",
                                    card: Some(card.clone()),
                                    kind: card.kind.clone(),
                                    on_cancel: move |_| modal.set(ModalState::None),
                                    on_saved: move |_| {
                                        modal.set(ModalState::None);
                                        refresh.set(refresh() + 1);
                                    },
                                }
                            } else {
                                BookmarkForm {
                                    key: "{card.id}",
                                    bookmark: Some(card.clone()),
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
                        }
                    }
                }
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

    /// 池里不足两条就不轮换（`None` 让调用方退回"设置里选中的那条"）。
    #[test]
    fn rotation_needs_at_least_two_entries() {
        let one = vec!["https://a.test/1.jpg".to_string()];
        assert_eq!(rotation_index(&one, "1792163779698"), None);
        assert_eq!(rotation_index(&[], "1792163779698"), None);

        let two = vec![
            "https://a.test/1.jpg".to_string(),
            "https://a.test/2.jpg".to_string(),
        ];
        assert!(rotation_index(&two, "1792163779698").is_some());
    }

    /// 下标始终落在池内，且同一次加载（同一个 nonce）算出来必须一样——
    /// 组件会重渲染很多次，每次都得是同一张。
    #[test]
    fn rotation_is_stable_per_nonce_and_in_range() {
        let pool: Vec<String> = (0..5).map(|i| format!("https://a.test/{i}.jpg")).collect();

        for nonce in ["1", "2", "1792163779698", "1792163781242", "乱写"] {
            let first = rotation_index(&pool, nonce);
            assert_eq!(first, rotation_index(&pool, nonce), "同一 nonce 必须稳定");
            assert!(first.unwrap() < pool.len(), "下标不能越界");
        }

        // 不同 nonce 会落到不同的条目上（不要求"必然不同"，但也不能永远同一张）
        let picks: std::collections::HashSet<Option<usize>> = ["1", "2", "3", "4", "5", "6"]
            .iter()
            .map(|nonce| rotation_index(&pool, nonce))
            .collect();
        assert!(
            picks.len() > 1,
            "十个 nonce 全落到同一条上，说明散列没起作用"
        );
    }

    fn bookmark(id: i64, title: &str, url: &str, description: &str, tags: &str) -> Bookmark {
        Bookmark {
            id,
            title: title.to_string(),
            url: url.to_string(),
            description: description.to_string(),
            tags: tags.to_string(),
            group_id: None,
            group_ids: String::new(),
            icon: String::new(),
            created_at: String::new(),
            span_w: 1,
            span_h: 1,
            // 这些用例只管书签；组件的分流另有专门用例
            kind: crate::models::KIND_BOOKMARK.to_string(),
            config: String::new(),
            surface: String::new(),
        }
    }

    #[test]
    fn keyword_matches_all_four_fields() {
        let item = bookmark(
            1,
            "Rust 官网",
            "https://rust-lang.org",
            "系统编程语言",
            "编程,rust",
        );

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
        item.group_ids = "1".to_string();
        assert_eq!(effective_groups(&item, &known), vec![1]);

        item.group_ids = "99".to_string();
        assert!(
            effective_groups(&item, &known).is_empty(),
            "指向已删分组＝未分组"
        );

        item.group_ids = String::new();
        assert!(effective_groups(&item, &known).is_empty());

        // 多分组：已知的留下，未知的丢掉，顺序保持
        item.group_ids = "2,99,1,2".to_string();
        assert_eq!(effective_groups(&item, &known), vec![2, 1]);
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

    fn widget(id: i64, kind: &str) -> Bookmark {
        Bookmark {
            kind: kind.to_string(),
            ..bookmark(id, crate::models::widget_label(kind), "", "", "")
        }
    }

    /// 分流只按"是不是组件"分开，不重排——两组的相对顺序都要保持原样，
    /// 否则页面上卡片的顺序会莫名其妙地变。
    #[test]
    fn split_keeps_each_group_in_its_original_order() {
        let cards = vec![
            bookmark(1, "A", "https://a.com", "", ""),
            widget(2, crate::models::KIND_CLOCK),
            bookmark(3, "B", "https://b.com", "", ""),
            widget(4, crate::models::KIND_CLOCK),
        ];

        let (widgets, bookmarks) = split_cards(&cards);
        assert_eq!(widgets.iter().map(|c| c.id).collect::<Vec<_>>(), vec![2, 4]);
        assert_eq!(
            bookmarks.iter().map(|c| c.id).collect::<Vec<_>>(),
            vec![1, 3]
        );
    }

    /// 分流决定"页面上会出现什么"，所以不能丢卡片、也不能重复。
    #[test]
    fn split_loses_nothing() {
        let cards = vec![
            bookmark(1, "A", "https://a.com", "", ""),
            widget(2, crate::models::KIND_CLOCK),
        ];

        let (widgets, bookmarks) = split_cards(&cards);
        assert_eq!(widgets.len() + bookmarks.len(), cards.len());
    }

    /// 白名单外的 kind 按书签处理：渲染层因此不会碰到没见过的组件分支。
    #[test]
    fn unknown_kind_is_not_treated_as_a_widget() {
        let cards = vec![widget(7, "chart")];

        let (widgets, bookmarks) = split_cards(&cards);
        assert!(widgets.is_empty(), "白名单外的类型不算组件");
        assert_eq!(bookmarks.len(), 1, "它退回书签那一侧");
    }
}
