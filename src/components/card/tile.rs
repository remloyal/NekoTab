use dioxus::prelude::*;

use super::widgets::{ClockWidget, DialWidget, NoteWidget, WeatherWidget};
use crate::components::icons;
use crate::models::{
    Bookmark, ClockConfig, DialConfig, NoteConfig, WeatherConfig, KIND_CLOCK, KIND_DIAL,
    KIND_NOTE, KIND_WEATHER,
};

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
    // 首字与稳定色相的算法在 icons 里：搜索引擎图标（engine_logo）用的是同一套回退，
    // 不该让那个组件伸手进卡片文件来拿
    (icons::initial(&bookmark.title), icons::hash_hue(&bookmark.url))
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

/// 单个卡片（书签或组件）。
///
/// - 普通态：点击打开（新标签页或当前页，由设置决定），右键呼出菜单；
/// - 编辑态：可拖拽排序（鼠标与触屏都走 Pointer Events），点击打开修改弹窗，
///   右上角 ✕ 删除；
/// - 选择态：点击切换选中，配合底栏做批量删除 / 打标签 / 移动分组。
///
/// **书签与组件共用这一层**：跨度、容器查询、拖拽落点（`data-bookmark-id`）、
/// 编辑态与选择态的角标，两者完全一样，只有"正文"和"点下去做什么"不同。所以这里是
/// 一条正文分支，而不是两个几乎相同的卡片组件——那样每加一种交互都要改两遍。
///
/// 拖拽的判定与落点计算都在父级 `CardGrid` 里：它同时是"网格内的公共祖先"，
/// 因此无论指针捕获成功与否，指针移动都能冒泡到它。这里只负责按下时把坐标报上去。
#[component]
#[allow(clippy::too_many_arguments)]
pub fn CardTile(
    bookmark: Bookmark,
    /// 横向占几格（已按当前可用列数夹过）
    span_w: i64,
    /// 纵向占几格
    span_h: i64,
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
    let is_widget = bookmark.is_widget();
    // 组件的 url 是空串，拿它当提示等于没有提示，所以退回名称
    let tooltip = if !bookmark.description.is_empty() {
        bookmark.description.clone()
    } else if is_widget {
        bookmark.title.clone()
    } else {
        bookmark.url.clone()
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

    // 这张卡片该用的面（"没设过"在这里被解析成该种类的默认：组件卡有面、书签卡没有）。
    // 解析放在 models 里，渲染层不必自己判断 kind 或处理空串。
    let surface = bookmark.surface_class();

    // 组件正文：按 kind 分发。加新组件时这里加一条分支。
    let widget_body: Option<Element> = is_widget.then(|| match bookmark.kind.as_str() {
        KIND_CLOCK => rsx! {
            ClockWidget { config: ClockConfig::parse(&bookmark.config) }
        },
        KIND_DIAL => rsx! {
            DialWidget { config: DialConfig::parse(&bookmark.config) }
        },
        KIND_NOTE => rsx! {
            NoteWidget { config: NoteConfig::parse(&bookmark.config) }
        },
        KIND_WEATHER => rsx! {
            WeatherWidget {
                // 取数是跟着参数走的：改了地点若不重挂，`use_future` 会一直用挂载时
                // 捕获的那个地点，卡片就停在旧城市上。key 一变就重挂，等于重新取。
                key: "{bookmark.config}",
                config: WeatherConfig::parse(&bookmark.config),
            }
        },
        // 未知 kind 理论上进不来（服务端有白名单）。真出现了就不画正文，
        // 但保留卡片本身——至少还能右键把它删掉，而不是凭空少一块。
        _ => rsx! {},
    });

    let class = {
        let mut class = String::from("tile");
        if is_widget {
            class.push_str(" tile-widget");
        }
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
            // 面的取值由 models::effective_surface 决定，CSS 按这个属性上底色
            "data-surface": "{surface}",
            // 占几格告诉 CSS；图标大小则由 .tile 的容器查询按格子尺寸算，不必在这里算
            style: "--span-w: {span_w}; --span-h: {span_h};",
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
                        return;
                    }

                    // 普通态的组件：没有可去的地方（url 是空串），点它改为打开设置。
                    // 记事本的正文就在设置里改，点一下就能写——否则还得右键找菜单。
                    // 必须 prevent_default，否则会去走 <a href="#">，把页面滚回顶部。
                    if target.is_widget() {
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

            if let Some(body) = widget_body {
                {body}
                // 组件卡也要名字。几张组件摆一起时，光看内容不好认是哪一个，
                // 而名字原来只存在于鼠标悬停的 tooltip 里。
                div { class: "tile-label", "{bookmark.title}" }
            } else {
                div {
                    class: "tile-icon",
                    style: "background: linear-gradient(140deg, hsl({hue} 62% 54%), hsl({(hue + 30) % 360} 60% 42%));",
                    span { class: "tile-letter", "{initial}" }
                    if let Some(icon) = custom {
                        {icon}
                    } else if let Some(host) = icon_host.clone() {
                        // 取到站点图标就盖在首字色块之上；404 时藏掉它，露出首字。
                        // 但在 onerror 挂上之前就失败的（SSR 那批），这里收不到通知，
                        // 由 CardGrid 挂载后补查一次补齐，见 dom::mark_broken_favicons。
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
}
