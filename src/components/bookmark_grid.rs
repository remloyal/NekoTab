use dioxus::prelude::*;

use crate::components::IconTile;
use crate::models::Bookmark;

/// 位移超过该像素数才算"开始拖动"（用来区分点击与拖拽）。
const DRAG_TOLERANCE: f64 = 8.0;

/// 图标网格。末尾固定一个「添加卡片」块。
///
/// 拖拽状态与判定都留在这里：网格是"所有卡片的公共祖先"，所以指针移动——无论
/// 指针捕获成功还是失败——都会冒泡到它。对外只暴露「把 A 放到 B 的位置」这一个
/// 事件，调用方不必关心中间的指针细节。鼠标与触屏走同一条路径（Pointer Events）。
#[component]
#[allow(clippy::too_many_arguments)]
pub fn BookmarkGrid(
    items: Vec<Bookmark>,
    editing: bool,
    selecting: bool,
    selected: Vec<i64>,
    /// 打开方式：true 开新标签页
    new_tab: bool,
    on_add: EventHandler<()>,
    on_context: EventHandler<(Bookmark, f64, f64)>,
    on_enter_edit: EventHandler<()>,
    on_edit: EventHandler<Bookmark>,
    on_delete: EventHandler<Bookmark>,
    on_toggle_select: EventHandler<i64>,
    on_reorder: EventHandler<(i64, i64)>,
    /// 拖拽时把指针停在翻页按钮上：+1 下一页，-1 上一页（跨页拖拽）
    on_page_flip: EventHandler<isize>,
) -> Element {
    // 已按下但还没超过拖拽阈值：(卡片 id, 起点 x, 起点 y)
    let mut armed = use_signal(|| Option::<(i64, f64, f64)>::None);
    // 正在拖动谁
    let mut dragging = use_signal(|| Option::<i64>::None);
    // 当前落在谁头上
    let mut over = use_signal(|| Option::<i64>::None);
    // 刚拖完：这一次 click 要吞掉（否则抬手会误开修改弹窗）
    let mut just_dragged = use_signal(|| false);

    let is_draggable = editing && !selecting;

    // hydration 后补查一次图标加载状态：SSR 那批 HTML 里的 <img> 可能在 onerror
    // 挂上之前就已经失败了（同源 404 往往比 wasm 下载还快），不补这一刀就会一直
    // 顶着浏览器的"碎图"占位符。之后新增的图片都是客户端渲染、监听创建时已就位，
    // 各自的 onerror 管得住，所以这里跑一次就够。
    use_effect(move || {
        spawn(async move {
            crate::dom::mark_broken_favicons().await;
        });
    });

    let mut class = String::from("grid");
    if is_draggable {
        class.push_str(" editing");
    }
    if selecting {
        class.push_str(" selecting");
    }
    if dragging().is_some() {
        class.push_str(" dragging");
    }

    let mut finish = move || {
        let (dragged, target) = (dragging(), over());
        armed.set(None);
        dragging.set(None);
        over.set(None);

        if let (Some(dragged), Some(target)) = (dragged, target) {
            if dragged != target {
                on_reorder.call((dragged, target));
            }
        }
    };

    rsx! {
        div {
            class: "{class}",

            // 拖拽过程中指针可能落在任何一张卡片上，也可能滑出网格；
            // 事件最终都会冒泡到这里，所以移动与抬手都在这一层处理。
            onpointermove: move |evt| {
                if !is_draggable {
                    return;
                }

                let coords = evt.client_coordinates();

                // 还没进入拖拽态：先看位移够不够
                if dragging().is_none() {
                    let Some((id, origin_x, origin_y)) = armed() else {
                        return;
                    };
                    if (coords.x - origin_x).abs() <= DRAG_TOLERANCE
                        && (coords.y - origin_y).abs() <= DRAG_TOLERANCE
                    {
                        return;
                    }

                    dragging.set(Some(id));
                    over.set(Some(id));
                    just_dragged.set(true);
                }

                // 指针停在翻页按钮上时翻页，翻完继续拖（落点在下一页上）
                if let Some(direction) = crate::dom::data_attr_at(coords.x, coords.y, "data-page-flip") {
                    on_page_flip.call(if direction == "prev" { -1 } else { 1 });
                    return;
                }

                // 落点用坐标做命中测试：比逐个读卡片矩形便宜，也不受滚动影响
                let target = crate::dom::bookmark_id_at(coords.x, coords.y);
                if target != over() {
                    over.set(target);
                }
            },
            onpointerup: move |_| {
                if dragging().is_some() {
                    finish();
                } else {
                    armed.set(None);
                }
            },
            onpointercancel: move |_| {
                armed.set(None);
                dragging.set(None);
                over.set(None);
            },

            for item in items {
                IconTile {
                    key: "{item.id}",
                    bookmark: item.clone(),
                    editing,
                    selecting,
                    selected: selected.contains(&item.id),
                    dragging: dragging() == Some(item.id),
                    drag_over: over() == Some(item.id) && dragging() != Some(item.id),
                    suppress_click: just_dragged(),
                    new_tab,
                    on_context: {
                        let target = item.clone();
                        move |(x, y): (f64, f64)| on_context.call((target.clone(), x, y))
                    },
                    on_enter_edit,
                    on_edit,
                    on_delete,
                    on_toggle_select,
                    on_pointer_down: {
                        let id = item.id;
                        move |(x, y): (f64, f64)| {
                            // 新的一次按下：清掉上一次拖拽留下的"吞点击"标记
                            just_dragged.set(false);
                            armed.set(Some((id, x, y)));
                        }
                    },
                }
            }

            if !selecting {
                button {
                    class: "tile tile-add",
                    r#type: "button",
                    title: "添加卡片",
                    onclick: move |_| on_add.call(()),
                    div { class: "tile-icon tile-icon-add", "＋" }
                    div { class: "tile-label", "添加卡片" }
                }
            }
        }
    }
}
