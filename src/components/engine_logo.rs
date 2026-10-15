use dioxus::prelude::*;

use super::icons::{hash_hue, initial};
use crate::models::Engine;

/// 引擎图标：配了图片地址就用图片，否则用名称首字的彩色圆点（离线可用）。
pub(crate) fn engine_logo(engine: &Engine) -> Element {
    if engine.icon.starts_with("http://") || engine.icon.starts_with("https://") {
        rsx! {
            img {
                class: "engine-logo",
                src: "{engine.icon}",
                alt: "{engine.name}",
                loading: "lazy",
            }
        }
    } else {
        let hue = hash_hue(&engine.name);
        let letter = initial(&engine.name);
        rsx! {
            span {
                class: "engine-logo engine-logo-text",
                style: "background: hsl({hue} 62% 48%);",
                "{letter}"
            }
        }
    }
}
