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

/// 读取 `<input type="file">` 里选中的第一个文件的文本内容。
///
/// 用 `FileReader.readAsText` 而不是 `Blob::text()`：后者的 Promise 要在 wasm 里
/// 接一个 async 运行时，而这里只需要一个回调。
///
/// 回调在读取完成（或失败）后触发一次；失败时参数为 `None`。
#[cfg(feature = "web")]
pub fn read_selected_file_text<F>(input_id: &str, on_done: F)
where
    F: FnOnce(Option<String>) + 'static,
{
    use wasm_bindgen::closure::Closure;
    use wasm_bindgen::JsCast;

    let Some(input) = document()
        .and_then(|doc| doc.get_element_by_id(input_id))
        .and_then(|el| el.dyn_into::<web_sys::HtmlInputElement>().ok())
    else {
        on_done(None);
        return;
    };

    let Some(file) = input.files().and_then(|files| files.get(0)) else {
        on_done(None);
        return;
    };

    let reader = match web_sys::FileReader::new() {
        Ok(reader) => reader,
        Err(_) => {
            on_done(None);
            return;
        }
    };

    // `once_into_js` 正好对上 onload 只触发一次的语义：既能消费 on_done，
    // 也不会像 `forget` 那样把闭包永久泄漏在 JS 堆上。
    let reader_for_callback = reader.clone();
    let onload = Closure::once_into_js(move || {
        let text = reader_for_callback
            .result()
            .ok()
            .and_then(|value| value.as_string());
        on_done(text);
    });
    reader.set_onload(Some(onload.unchecked_ref()));

    // `read_as_text` 失败时 onload 不会触发，也就没有回调——调用方表现为"没反应"。
    // 这是文件选择本身出了问题，属于用户可恢复的操作，不必 panic。
    let _ = reader.read_as_text(&file);
}

#[cfg(not(feature = "web"))]
pub fn read_selected_file_text<F>(_input_id: &str, _on_done: F)
where
    F: FnOnce(Option<String>) + 'static,
{
}

#[cfg(feature = "web")]
fn document() -> Option<web_sys::Document> {
    web_sys::window()?.document()
}
