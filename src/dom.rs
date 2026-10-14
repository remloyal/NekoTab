//! 浏览器侧的小工具（仅 `web` 编译）。
//!
//! 集中放几件必须直接摸 DOM 的事：开新标签页、聚焦搜索框、把指针捕获到卡片上、
//! 按坐标找落点卡片、读本地文件。这些在组件里内联会淹掉交互逻辑，所以单独一层。
//!
//! 每个函数在非 web 构建下都有一个空实现，这样组件代码不必到处写 `#[cfg]`。

/// 打开地址。`new_tab` 为假时当前页跳转。
#[cfg(feature = "web")]
pub fn open_url(url: &str, new_tab: bool) {
    if let Some(window) = web_sys::window() {
        // 由用户手势（点击/回车）触发，不会被弹窗拦截
        let target = if new_tab { "_blank" } else { "_self" };
        let _ = window.open_with_url_and_target(url, target);
    }
}

#[cfg(not(feature = "web"))]
pub fn open_url(_url: &str, _new_tab: bool) {}

/// 让某个元素获得焦点（搜索框用）。
#[cfg(feature = "web")]
pub fn focus_element(id: &str) {
    use wasm_bindgen::JsCast;

    let Some(element) = document().and_then(|doc| doc.get_element_by_id(id)) else {
        return;
    };
    if let Ok(element) = element.dyn_into::<web_sys::HtmlElement>() {
        let _ = element.focus();
    }
}

#[cfg(not(feature = "web"))]
pub fn focus_element(_id: &str) {}

/// 把指针捕获到当前事件的目标元素上。
///
/// 捕获之后，即使用户把指针拖出卡片（甚至拖出网格），后续的
/// pointermove / pointerup 仍然会派发到这张卡片，拖拽不会中途"掉线"。
/// 返回是否捕获成功——失败时调用方仍可退回"只在网格内有效"的行为。
#[cfg(feature = "web")]
pub fn capture_pointer(evt: &dioxus::events::PointerEvent) -> bool {
    use wasm_bindgen::JsCast;

    let data = evt.data();
    let Some(native) = data.downcast::<web_sys::PointerEvent>() else {
        return false;
    };

    let Some(target) = native.target() else {
        return false;
    };
    let Ok(element) = target.dyn_into::<web_sys::Element>() else {
        return false;
    };

    element.set_pointer_capture(native.pointer_id()).is_ok()
}

#[cfg(not(feature = "web"))]
pub fn capture_pointer(_evt: &dioxus::events::PointerEvent) -> bool {
    false
}

/// 按视口坐标找出落在哪张卡片上，返回卡片的 `data-bookmark-id`。
///
/// 用 `element_from_point` 而不是逐个读卡片的 bounding rect：拖拽时每移动一次
/// 都要判定，前者是一次命中测试，后者是 N 次布局读取。
#[cfg(feature = "web")]
pub fn bookmark_id_at(x: f64, y: f64) -> Option<i64> {
    let element = document()?.element_from_point(x as f32, y as f32)?;
    let card = element.closest("[data-bookmark-id]").ok().flatten()?;
    card.get_attribute("data-bookmark-id")?.parse().ok()
}

#[cfg(not(feature = "web"))]
pub fn bookmark_id_at(_x: f64, _y: f64) -> Option<i64> {
    None
}

/// 按视口坐标找最近一个带指定 `data-*` 属性的祖先元素，返回该属性的值。
///
/// 拖拽时用它识别"指针是否落在翻页按钮上"，从而支持跨页拖拽。
#[cfg(feature = "web")]
pub fn data_attr_at(x: f64, y: f64, attr: &str) -> Option<String> {
    let element = document()?.element_from_point(x as f32, y as f32)?;
    let selector = format!("[{attr}]");
    let node = element.closest(&selector).ok().flatten()?;
    node.get_attribute(attr)
}

#[cfg(not(feature = "web"))]
pub fn data_attr_at(_x: f64, _y: f64, _attr: &str) -> Option<String> {
    None
}

/// 给"已经加载失败"的站点图标打上 `data-broken`（CSS 会把它隐藏，露出下面的首字）。
///
/// 为什么需要这一刀：`<img>` 的 `error` 事件**不冒泡**，而 SSR 吐出的 HTML 里图片
/// 就开始加载了——同源 `/icon/{host}` 返回 404 往往比 wasm 下载、hydration 都快，
/// 等 Dioxus 把 `onerror` 挂上去，事件早发完了。那张卡片于是永远挂着浏览器的
/// "碎图"占位符，还正好盖住底下的首字色块。
///
/// hydration 之后补查一次 `naturalWidth` 就覆盖了这种情况：此刻还在飞的图片，
/// 会在之后失败时派发 error，而那时 `onerror` 已经就位。此后新增的图片都是客户端
/// 渲染的，监听在创建时就已挂好，`onerror` 自己管得住。
#[cfg(feature = "web")]
pub async fn mark_broken_favicons() {
    const SCRIPT: &str = r#"
        document.querySelectorAll('img.tile-favicon').forEach((img) => {
            // complete 为真但拿不到宽高，就是"已经失败"（不是还在加载）
            if (img.complete && img.naturalWidth === 0) {
                img.setAttribute('data-broken', '');
            }
        });
    "#;

    let _ = dioxus::prelude::document::eval(SCRIPT).await;
}

#[cfg(not(feature = "web"))]
pub async fn mark_broken_favicons() {}

/// 焦点是否正停在"正在编辑的文本"上——翻页快捷键要不要让路就看它。
///
/// 判断比"焦点是不是输入框"细一层：搜索框默认自动聚焦，如果只看标签名，
/// 方向键翻页就永远用不上；而空输入框里方向键本就没有光标可移，
/// 所以空输入框仍允许翻页。多行文本框与下拉框一律让路（方向键在那里有意义）。
#[cfg(feature = "web")]
pub fn focus_is_editing() -> bool {
    use wasm_bindgen::JsCast;

    let Some(active) = document().and_then(|doc| doc.active_element()) else {
        return false;
    };

    match active.tag_name().to_ascii_uppercase().as_str() {
        "TEXTAREA" | "SELECT" => true,
        "INPUT" => {
            let Ok(input) = active.dyn_into::<web_sys::HtmlInputElement>() else {
                return false;
            };
            // 勾选/滑块类控件没有光标语义，方向键照常翻页
            if matches!(
                input.type_().as_str(),
                "checkbox" | "radio" | "range" | "button" | "submit" | "file"
            ) {
                return false;
            }
            !input.value().is_empty()
        }
        _ => false,
    }
}

#[cfg(not(feature = "web"))]
pub fn focus_is_editing() -> bool {
    false
}

/// 焦点是否"没落在页面里的任何元素上"（null 或 body）。
///
/// 加载完成后浏览器不会自动把焦点给任何元素，此时 `document.activeElement`
/// 是 body——而 body 不在 `.stage-root` 的子树里，挂在根节点上的键盘事件
/// 收不到按键。所以首屏要把焦点主动送进页面。
#[cfg(feature = "web")]
pub fn focus_is_on_body() -> bool {
    match document().and_then(|doc| doc.active_element()) {
        None => true,
        Some(active) => active.tag_name().eq_ignore_ascii_case("body"),
    }
}

#[cfg(not(feature = "web"))]
#[cfg_attr(not(feature = "web"), allow(dead_code))]
pub fn focus_is_on_body() -> bool {
    false
}

/// 视口尺寸，用于把浮层拉回可视范围内。
#[cfg(feature = "web")]
pub fn viewport_size() -> (f64, f64) {
    let Some(window) = web_sys::window() else {
        return (0.0, 0.0);
    };
    let width = window
        .inner_width()
        .ok()
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let height = window
        .inner_height()
        .ok()
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    (width, height)
}

#[cfg(not(feature = "web"))]
pub fn viewport_size() -> (f64, f64) {
    // 服务端不渲染浮层，返回 0 时下面的 min/max 会退化成"贴左上角"
    (0.0, 0.0)
}

/// 单调递增的毫秒时间戳，用于事件节流。
#[cfg(feature = "web")]
pub fn now_ms() -> f64 {
    js_sys::Date::now()
}

#[cfg(not(feature = "web"))]
pub fn now_ms() -> f64 {
    0.0
}

/// 装上 panic 钩子。
///
/// release 构建开了 `panic = "abort"`，wasm 里任何 panic 都只会变成一个
/// `RuntimeError: unreachable`，浏览器控制台上连一句话都没有——出问题时只能靠猜。
/// 钩子在 abort 之前执行，把消息与位置打成一行错误，并写进一个固定 id 的 DOM 节点：
/// 应用崩死之后这个节点还在，从外部（比如自动化、用户截图）也能读到现场。
#[cfg(feature = "web")]
pub fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let message = format!("[NekoTab] panic: {info}");
        web_sys::console::error_1(&message.as_str().into());

        if let Some(document) = document() {
            let node = document
                .get_element_by_id("nekotab-panic")
                .or_else(|| {
                    let node = document.create_element("div").ok()?;
                    let _ = node.set_attribute(
                        "style",
                        "position:fixed;left:0;bottom:0;z-index:9999;max-width:100vw;\
                         padding:6px 10px;font:12px/1.5 monospace;color:#fff;\
                         background:rgba(180,30,30,.92);white-space:pre-wrap;",
                    );
                    document.body()?.append_child(&node).ok()?;
                    Some(node)
                });

            if let Some(node) = node {
                let _ = node.set_attribute("id", "nekotab-panic");
                node.set_text_content(Some(&message));
            }
        }
    }));
}

#[cfg(not(feature = "web"))]
#[cfg_attr(not(feature = "web"), allow(dead_code))]
pub fn install_panic_hook() {}


#[cfg(feature = "web")]
fn document() -> Option<web_sys::Document> {
    web_sys::window()?.document()
}
