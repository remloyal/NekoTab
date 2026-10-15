use dioxus::prelude::*;

use crate::models::NoteConfig;

/// 「记事本」组件的卡片正文。
///
/// 与数字时钟不同，这里**没有**客户端专属状态：正文直接来自库里那条卡片，所以 SSR
/// 渲染的就是最终结果，不存在 hydration 失配——那一套（首屏空着、挂载后再填）是为
/// "服务端不知道的东西"准备的，而笔记内容是服务端知道的东西。
///
/// 卡片是只读的展示面。改内容走它自己的编辑弹窗（点卡片即可打开）：卡片上直接打字要
/// 和三条既有行为抢指针——长按进编辑态、从卡片任意处起手拖拽、编辑态下的
/// `touch-action: none`——再叠一层防抖自动保存，那是另一件事，不混在这一版里。
#[component]
pub fn NoteWidget(config: NoteConfig) -> Element {
    let text = config.text;
    let empty = text.trim().is_empty();

    rsx! {
        div { class: "widget widget-note",
            if empty {
                span { class: "widget-note-empty", "点一下写点什么" }
            } else {
                // 换行与连续空格都照原样显示（笔记是按行读的），多余部分由格子的
                // overflow 裁掉
                span { class: "widget-note-text", "{text}" }
            }
        }
    }
}
