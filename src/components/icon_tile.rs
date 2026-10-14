use dioxus::prelude::*;

use crate::components::icons;
use crate::models::Bookmark;

/// 由字符串生成稳定的色相（FNV-1a 取模 360），供图标底色使用。
pub(crate) fn hash_hue(input: &str) -> u32 {
    let mut hash: u32 = 2_166_136_261;
    for byte in input.bytes() {
        hash = (hash ^ byte as u32).wrapping_mul(16_777_619);
    }
    hash % 360
}

/// 图标首字：取名称第一个字符并大写。
pub(crate) fn icon_initial(name: &str) -> String {
    name.chars()
        .next()
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_else(|| "?".to_string())
}

/// 书签自定义图标的渲染。空串返回 `None`（表示走站点自动代取那条路）。
fn custom_icon(icon: &str) -> Option<Element> {
    if let Some(id) = icons::builtin_id(icon) {
        return icons::builtin_svg(id).map(|svg| rsx! { div { class: "tile-custom", {svg} } });
    }
    if icon.starts_with("http://") || icon.starts_with("https://") || icon.starts_with('/') {
        return Some(rsx! {
            img { class: "tile-favicon", src: "{icon}", alt: "", loading: "lazy" }
        });
    }
    None
}

fn icon_of(bookmark: &Bookmark) -> (String, u32) {
    (icon_initial(&bookmark.title), hash_hue(&bookmark.url))
}

fn safe_href(url: &str) -> String {
    if url.starts_with("http://") || url.starts_with("https://") {
        url.to_string()
    } else {
        "#".to_string()
    }
}

/// 长按判定：按住超过该毫秒数即进入编辑态。
#[cfg(feature = "web")]
const LONG_PRESS_MS: u32 = 500;

/// 位移超过该像素数就当作"滑动"，不再算长按（或点击）。
const MOVE_TOLERANCE: f64 = 8.0;

/// 单个书签图标块。
///
/// - 普通态：点击打开（新标签页或当前页，由设置决定），右键呼出菜单；
/// - 编辑态：可拖拽排序（鼠标与触屏都走 Pointer Events），点击打开修改弹窗，
///   右上角 ✕ 删除；
/// - 选择态：点击切换选中，配合底栏做批量删除 / 打标签 / 移动分组。
///
/// 拖拽的判定与落点计算都在父级 `BookmarkGrid` 里：它同时是"网格内的公共祖先"，
/// 因此无论指针捕获成功与否，指针移动都能冒泡到它。这里只负责按下时把坐标报上去。
#[component]
#[allow(clippy::too_many_arguments)]
pub fn IconTile(
    bookmark: Bookmark,
    editing: bool,
    selecting: bool,
    selected: bool,
    /// 自己正被拖动
    dragging: bool,
    /// 自己被拖到了头上（落点提示）
    drag_over: bool,
    /// 刚刚发生过拖拽，这次的 click 不该被当成点击
    suppress_click: bool,
    /// 打开方式：true 开新标签页
    new_tab: bool,
    on_context: EventHandler<(f64, f64)>,
    on_enter_edit: EventHandler<()>,
    on_edit: EventHandler<Bookmark>,
    on_delete: EventHandler<Bookmark>,
    on_toggle_select: EventHandler<i64>,
    /// 编辑态按下的坐标（拖拽起点）
    on_pointer_down: EventHandler<(f64, f64)>,
) -> Element {
    let (initial, hue) = icon_of(&bookmark);
    let tooltip = if bookmark.description.is_empty() {
        bookmark.url.clone()
    } else {
        bookmark.description.clone()
    };
    let id = bookmark.id;

    // 取不到站点图标时回退到首字块
    let mut icon_failed = use_signal(|| false);
    // 站点图标由本服务的 /icon/{host} 代取；host 解析不出来就不请求
    let icon_host = crate::favicon::host_of(&bookmark.url);
    // 自定义图标优先：内置图标画 svg，地址直接当图片用；空串表示按站点自动代取
    let custom = custom_icon(&bookmark.icon);

    // 长按检测：按下时的坐标 + 递增令牌（令牌变了就说明这次按下已作废）
    let mut press_origin = use_signal(|| Option::<(f64, f64)>::None);
    let mut press_token = use_signal(|| 0u32);

    let is_edit = editing && !selecting;

    let class = {
        let mut class = String::from("tile");
        if selected {
            class.push_str(" tile-selected");
        }
        if dragging {
            class.push_str(" tile-dragging");
        }
        if drag_over {
            class.push_str(" drag-over");
        }
        class
    };

    rsx! {
        a {
            class: "{class}",
            // 拖拽完全交给 Pointer Events：HTML5 原生拖放在触屏上不触发，
            // 而且 <a> 的默认拖拽会把链接地址当成拖拽数据，两者会打架。
            "data-bookmark-id": "{id}",
            href: safe_href(&bookmark.url),
            target: if new_tab { "_blank" } else { "_self" },
            rel: "noopener noreferrer",
            title: "{tooltip}",
            draggable: "false",

            onpointerdown: move |evt| {
                let coords = evt.client_coordinates();

                if is_edit {
                    // 尽力把指针捕获到这张卡片上（拖出网格也能继续跟踪）。
                    // 捕获失败也无所谓：移动事件仍会从下方的元素冒泡到网格。
                    let _ = crate::dom::capture_pointer(&evt);
                    on_pointer_down.call((coords.x, coords.y));
                    return;
                }

                if selecting {
                    return;
                }

                // 普通态：长按进入编辑态
                press_origin.set(Some((coords.x, coords.y)));
                let token = press_token() + 1;
                press_token.set(token);

                spawn(async move {
                    #[cfg(feature = "web")]
                    {
                        gloo_timers::future::TimeoutFuture::new(LONG_PRESS_MS).await;
                        // 期间没有移动、也没有新的按下，才算长按
                        if press_token() == token && press_origin().is_some() {
                            press_origin.set(None);
                            on_enter_edit.call(());
                        }
                    }
                });
            },

            onpointermove: move |evt| {
                if is_edit {
                    return;
                }
                // 普通态：位移超过阈值即判定为滑动，放弃长按
                if let Some((origin_x, origin_y)) = press_origin() {
                    let coords = evt.client_coordinates();
                    if (coords.x - origin_x).abs() > MOVE_TOLERANCE
                        || (coords.y - origin_y).abs() > MOVE_TOLERANCE
                    {
                        press_origin.set(None);
                    }
                }
            },

            onpointerup: move |_| press_origin.set(None),
            onpointercancel: move |_| press_origin.set(None),
            onpointerleave: move |_| press_origin.set(None),

            oncontextmenu: move |evt| {
                evt.prevent_default();
                // 阻止冒泡，否则会连带触发背景的空白处菜单
                evt.stop_propagation();
                let coords = evt.client_coordinates();
                on_context.call((coords.x, coords.y));
            },

            onclick: {
                let target = bookmark.clone();
                move |evt| {
                    if suppress_click {
                        // 刚拖完，这次 click 是拖拽的尾巴
                        evt.prevent_default();
                        return;
                    }

                    if selecting {
                        evt.prevent_default();
                        on_toggle_select.call(target.id);
                        return;
                    }

                    // 编辑态点击改为打开修改弹窗，不再跳转
                    if editing {
                        evt.prevent_default();
                        evt.stop_propagation();
                        on_edit.call(target.clone());
                    }
                }
            },

            if selecting {
                span {
                    class: if selected { "tile-pick picked" } else { "tile-pick" },
                    title: if selected { "取消选择" } else { "选择" },
                    if selected { "✓" }
                }
            } else if editing {
                span {
                    class: "tile-del",
                    title: "删除该书签",
                    onclick: {
                        let target = bookmark.clone();
                        move |evt| {
                            evt.prevent_default();
                            evt.stop_propagation();
                            on_delete.call(target.clone());
                        }
                    },
                    "✕"
                }
                span { class: "tile-grip", title: "按住拖动调整位置", "⠿" }
            }

            div {
                class: "tile-icon",
                style: "background: linear-gradient(140deg, hsl({hue} 62% 54%), hsl({(hue + 30) % 360} 60% 42%));",
                span { class: "tile-letter", "{initial}" }
                if let Some(icon) = custom {
                    {icon}
                } else if let Some(host) = icon_host.clone() {
                    // 取到站点图标就盖在首字色块之上；404 时藏掉它，露出首字。
                    // 但在 onerror 挂上之前就失败的（SSR 那批），这里收不到通知，
                    // 由 BookmarkGrid 挂载后补查一次补齐，见 dom::mark_broken_favicons。
                    if !icon_failed() {
                        img {
                            class: "tile-favicon",
                            src: "/icon/{host}",
                            alt: "",
                            loading: "lazy",
                            onerror: move |_| icon_failed.set(true),
                        }
                    }
                }
            }
            div { class: "tile-label", "{bookmark.title}" }
        }
    }
}
