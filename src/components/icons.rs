//! 分组图标：内置图标集 + 统一的渲染入口。
//!
//! 图标以「`i:` + id」的形式存在分组的 `icon` 字段里（例如 `i:code`），
//! 与图片地址（`http…`）、首字回退（空串）共用同一个字段，因此不需要改表结构。
//!
//! 图标是自己画的简单几何图形（24×24 网格、线框、`stroke-width 1.8`），
//! 不引第三方图标库：一来避免为几十个图标引入依赖与许可问题，二来这些形状
//! 足够简单，肉眼可核对。每个图标是一组 `<path>` 的 `d`，共用描边属性。
//!
//! 圆用两段圆弧表达（`M cx-r,cy a r,r 0 1 0 2r,0 a r,r 0 1 0 -2r,0`），
//! 这样整个图标集只有一种元素类型，渲染代码不必分支。

use dioxus::prelude::*;

/// `(id, 中文名, 各段路径)`。id 会以 `i:{id}` 落库。
#[rustfmt::skip]
pub const GROUP_ICONS: [(&str, &str, &[&str]); 20] = [
    ("home", "首页", &["M3.5 10.5 L12 3.5 L20.5 10.5", "M6 9.5 V20 H18 V9.5"]),
    ("star", "收藏", &["M12 3.6 L14.1 9.1 L20 9.4 L15.4 13.1 L16.9 18.8 L12 15.6 L7.1 18.8 L8.6 13.1 L4 9.4 L9.9 9.1 Z"]),
    ("heart", "喜欢", &["M12 19.5 C9 17 4.5 13.5 4.5 9.8 A3.9 3.9 0 0 1 12 7.6 A3.9 3.9 0 0 1 19.5 9.8 C19.5 13.5 15 17 12 19.5 Z"]),
    ("folder", "文件夹", &["M3.5 7 H10 L12 9.5 H20.5 V18.5 H3.5 Z"]),
    ("bookmark", "书签", &["M6 4 H18 V20.5 L12 16 L6 20.5 Z"]),
    ("tag", "标签", &["M20.5 4.5 V12 L12 20.5 L3.5 12 L12 3.5 H20.5 Z", "M15.3 7.5 a1.2 1.2 0 1 0 2.4 0 a1.2 1.2 0 1 0 -2.4 0"]),
    ("globe", "网站", &["M3.5 12 a8.5 8.5 0 1 0 17 0 a8.5 8.5 0 1 0 -17 0", "M3.5 12 H20.5", "M12 3.5 C8 7.5 8 16.5 12 20.5", "M12 3.5 C16 7.5 16 16.5 12 20.5"]),
    ("mail", "邮件", &["M3.5 6 H20.5 V18 H3.5 Z", "M3.5 6.5 L12 13 L20.5 6.5"]),
    ("calendar", "日历", &["M4 5.5 H20 V20 H4 Z", "M4 10 H20", "M8.5 3.5 V7", "M15.5 3.5 V7"]),
    ("clock", "时间", &["M3.5 12 a8.5 8.5 0 1 0 17 0 a8.5 8.5 0 1 0 -17 0", "M12 7.5 V12.5 L16 14.5"]),
    ("chat", "社区", &["M4 5.5 H20 V16 H9 L4.5 20 Z"]),
    ("cart", "购物", &["M3.5 5 H6.2 L8.6 15.5 H18 L20.5 8 H6.6", "M7.9 19 a1.6 1.6 0 1 0 3.2 0 a1.6 1.6 0 1 0 -3.2 0", "M15.4 19 a1.6 1.6 0 1 0 3.2 0 a1.6 1.6 0 1 0 -3.2 0"]),
    ("code", "编程", &["M9 8 L4.5 12 L9 16", "M15 8 L19.5 12 L15 16"]),
    ("image", "设计", &["M4 5.5 H20 V18.5 H4 Z", "M4 15 L9 10.5 L13 14.5 L16.5 11.5 L20 15", "M7.6 9.5 a1.4 1.4 0 1 0 2.8 0 a1.4 1.4 0 1 0 -2.8 0"]),
    ("music", "音乐", &["M10 16.5 V6 L18 4.5 V15", "M5.5 17 a2.5 2.5 0 1 0 5 0 a2.5 2.5 0 1 0 -5 0", "M13.5 15.5 a2.5 2.5 0 1 0 5 0 a2.5 2.5 0 1 0 -5 0"]),
    ("video", "影音", &["M3.5 6 H20.5 V18 H3.5 Z", "M10 9.5 L15 12 L10 14.5 Z"]),
    ("book", "阅读", &["M4.5 5 H12 V19.5 H4.5 Z", "M12 5 H19.5 V19.5 H12 Z"]),
    ("cloud", "云端", &["M7.5 18 H17 A4 4 0 0 0 16.6 10.1 A5.5 5.5 0 0 0 6 11.5 A3.3 3.3 0 0 0 7.5 18 Z"]),
    ("search", "搜索", &["M4.5 10.5 a6 6 0 1 0 12 0 a6 6 0 1 0 -12 0", "M15 15 L20 20"]),
    ("link", "链接", &["M9.5 14.5 L14.5 9.5", "M8 11.5 L6 13.5 A3.5 3.5 0 0 0 11 18.5 L13 16.5", "M16 12.5 L18 10.5 A3.5 3.5 0 0 0 13 5.5 L11 7.5"]),
];

/// 内置图标的值前缀，落库形如 `i:code`。
pub const ICON_PREFIX: &str = "i:";

/// 把 id 拼成可落库的图标值。
pub fn builtin_value(id: &str) -> String {
    format!("{ICON_PREFIX}{id}")
}

/// 从图标值里取出内置图标 id；不是内置图标则返回 `None`。
pub fn builtin_id(value: &str) -> Option<&str> {
    let id = value.strip_prefix(ICON_PREFIX)?;
    GROUP_ICONS
        .iter()
        .any(|(known, _, _)| *known == id)
        .then_some(id)
}

/// 首字回退用的字符。
pub fn initial(name: &str) -> String {
    name.chars().next().unwrap_or('?').to_uppercase().to_string()
}

/// 渲染一个内置图标（不含外层容器）。`id` 不存在时返回 `None`。
pub fn builtin_svg(id: &str) -> Option<Element> {
    let (_, _, paths) = GROUP_ICONS.iter().find(|(known, _, _)| *known == id)?;

    Some(rsx! {
        svg {
            class: "group-icon-svg",
            view_box: "0 0 24 24",
            fill: "none",
            stroke: "currentColor",
            stroke_width: "1.8",
            stroke_linecap: "round",
            stroke_linejoin: "round",
            for d in paths.iter() {
                path { d: "{d}" }
            }
        }
    })
}

/// 分组图标：内置图标 / 图片地址 / 名称首字，三者共用同一个 `icon` 字段。
///
/// 三个组件（标签栏、分组管理、批量移动）都走这一个入口，免得图标样式漂移。
pub fn group_icon(icon: &str, name: &str, class: &str) -> Element {
    if let Some(id) = builtin_id(icon) {
        if let Some(svg) = builtin_svg(id) {
            return rsx! { span { class: "group-icon {class}", {svg} } };
        }
    }

    if icon.starts_with("http://") || icon.starts_with("https://") {
        return rsx! {
            img { class: "group-icon {class}", src: "{icon}", alt: "", loading: "lazy" }
        };
    }

    let text = initial(name);
    rsx! { span { class: "group-icon {class}", "{text}" } }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_values_round_trip() {
        let value = builtin_value("code");
        assert_eq!(value, "i:code");
        assert_eq!(builtin_id(&value), Some("code"));

        // 未知 id 不认，交给首字回退
        assert_eq!(builtin_id("i:没有这个"), None);
        // 图片地址与空串都不是内置图标
        assert_eq!(builtin_id("https://a.com/i.png"), None);
        assert_eq!(builtin_id(""), None);
    }

    #[test]
    fn icon_table_is_consistent() {
        let mut ids: Vec<&str> = GROUP_ICONS.iter().map(|(id, _, _)| *id).collect();
        let count = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), count, "id 不能重复");

        for (id, label, paths) in GROUP_ICONS {
            assert!(!id.is_empty() && !label.is_empty(), "{id} 缺少 id 或名称");
            assert!(!paths.is_empty(), "{id} 没有路径");
            for d in paths {
                assert!(d.starts_with('M'), "{id} 的路径必须以 M 起头：{d}");
            }
            assert!(builtin_svg(id).is_some(), "{id} 渲染失败");
        }
    }

    #[test]
    fn initial_falls_back_sensibly() {
        assert_eq!(initial("开发"), "开");
        assert_eq!(initial("rust"), "R");
        assert_eq!(initial(""), "?");
    }
}
