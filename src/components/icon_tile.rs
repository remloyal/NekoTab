use dioxus::prelude::*;

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

/// 长按判定：手指/鼠标位移超过该像素数就当作滑动，取消长按。
const MOVE_TOLERANCE: f64 = 8.0;

/// 单个书签图标块。
///
/// 普通态：点击新窗口打开，右键呼出菜单。
/// 编辑态（`editing`）：显示 ✕ 删除角标与拖拽手柄，点击改为打开修改弹窗，
/// 并可用 HTML5 拖拽重排。
#[component]
pub fn IconTile(
    bookmark: Bookmark,
    editing: bool,
    on_context: EventHandler<(f64, f64)>,
    on_enter_edit: EventHandler<()>,
    on_edit: EventHandler<Bookmark>,
    on_delete: EventHandler<Bookmark>,
    on_drag_start: EventHandler<i64>,
    on_drop_on: EventHandler<i64>,
) -> Element {
    let (initial, hue) = icon_of(&bookmark);
    let tooltip = if bookmark.description.is_empty() {
        bookmark.url.clone()
    } else {
        bookmark.description.clone()
    };
    let id = bookmark.id;
    let draggable = editing;

    // 取不到站点图标时回退到首字块
    let mut icon_failed = use_signal(|| false);
    // 站点图标由本服务的 /icon/{host} 代取；host 解析不出来就不请求
    let icon_host = crate::favicon::host_of(&bookmark.url);

    // 长按检测：按下时的坐标 + 递增令牌（令牌变了就说明这次按下已作废）
    let mut press_origin = use_signal(|| Option::<(f64, f64)>::None);
    let mut press_token = use_signal(|| 0u32);
    let mut drag_over = use_signal(|| false);

    rsx! {
        a {
            class: if drag_over() { "tile drag-over" } else { "tile" },
            href: safe_href(&bookmark.url),
            target: "_blank",
            rel: "noopener noreferrer",
            title: "{tooltip}",
            draggable: if draggable { "true" } else { "false" },

            onpointerdown: move |evt| {
                if editing {
                    return;
                }
                let coords = evt.client_coordinates();
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
                if let Some((origin_x, origin_y)) = press_origin() {
                    let coords = evt.client_coordinates();
                    if (coords.x - origin_x).abs() > MOVE_TOLERANCE
                        || (coords.y - origin_y).abs() > MOVE_TOLERANCE
                    {
                        // 判定为滑动，放弃长按
                        press_origin.set(None);
                    }
                }
            },
            onpointerup: move |_| press_origin.set(None),
            onpointerleave: move |_| press_origin.set(None),
            onpointercancel: move |_| press_origin.set(None),

            oncontextmenu: move |evt| {
                evt.prevent_default();
                // 阻止冒泡，否则会连带触发背景的空白处菜单
                evt.stop_propagation();
                let coords = evt.client_coordinates();
                on_context.call((coords.x, coords.y));
            },

            // 编辑态点击改为打开修改弹窗，不再跳转
            onclick: {
                let target = bookmark.clone();
                move |evt| {
                    if editing {
                        evt.prevent_default();
                        evt.stop_propagation();
                        on_edit.call(target.clone());
                    }
                }
            },

            ondragstart: move |evt| {
                // 有些浏览器要求 drag 必须带数据才算有效拖拽
                let _ = evt.data_transfer().set_data("text/plain", &id.to_string());
                on_drag_start.call(id);
            },
            ondragover: move |evt| {
                if draggable {
                    evt.prevent_default();
                    drag_over.set(true);
                }
            },
            ondragleave: move |_| drag_over.set(false),
            ondrop: move |evt| {
                if draggable {
                    evt.prevent_default();
                    evt.stop_propagation();
                    drag_over.set(false);
                    on_drop_on.call(id);
                }
            },
            ondragend: move |_| drag_over.set(false),

            if editing {
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
                if editing {
                    span { class: "tile-grip", title: "拖动调整位置", "⠿" }
                }
            }

            div {
                class: "tile-icon",
                style: "background: linear-gradient(140deg, hsl({hue} 62% 54%), hsl({(hue + 30) % 360} 60% 42%));",
                span { class: "tile-letter", "{initial}" }
                // 取到站点图标就盖在首字色块之上；404 时 onerror 隐藏它，露出首字
                if let Some(host) = icon_host.clone() {
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
