use dioxus::prelude::*;

use super::group_settings::GroupSettings;
use super::trash_panel::TrashPanel;
use crate::backend::{error_text, import_bookmarks, save_settings};
use crate::models::{Group, ImportReport, Settings, BACKGROUND_PRESETS, THEMES};

/// 文件选择器的 id，读文件时靠它取到 `<input type="file">`。
const IMPORT_FILE_ID: &str = "mtab-import-file";

/// 主页设置面板（抽屉内容）。
///
/// 所有字段装在一个 `Settings` 草稿里（而不是一个字段一个 signal），加设置项时
/// 只需要改 `models.rs` 的 `to_pairs` / `from_pairs` 和这里的控件，不必再改签名。
/// 设置存的是键值表，所以**加字段不需要数据库迁移**。
#[component]
pub fn SettingsPanel(
    settings: Settings,
    groups: Vec<Group>,
    on_settings_saved: EventHandler<()>,
    on_bookmarks_changed: EventHandler<()>,
    on_groups_changed: EventHandler<()>,
) -> Element {
    // 首次渲染即从服务端带来的设置初始化；删除或还原操作会重挂面板，届时自然同步
    let mut draft = use_signal(|| settings.clone());

    let mut saving = use_signal(|| false);
    let mut save_error = use_signal(|| Option::<String>::None);
    let mut saved = use_signal(|| false);

    let mut import_text = use_signal(String::new);
    let mut import_name = use_signal(String::new);
    let mut importing = use_signal(|| false);
    let mut import_error = use_signal(|| Option::<String>::None);
    let mut import_report = use_signal(|| Option::<ImportReport>::None);

    let save = move |_| {
        if saving() {
            return;
        }

        let payload = draft();

        spawn(async move {
            saving.set(true);
            save_error.set(None);
            saved.set(false);

            match save_settings(payload).await {
                Ok(()) => {
                    saved.set(true);
                    saving.set(false);
                    on_settings_saved.call(());
                }
                Err(e) => {
                    save_error.set(Some(error_text(&e)));
                    saving.set(false);
                }
            }
        });
    };

    // 导入的文本可能来自粘贴，也可能来自选中的文件；两者共用同一条提交路径
    let mut run_import = move |text: String| {
        if importing() {
            return;
        }
        if text.trim().is_empty() {
            import_error.set(Some("请先粘贴内容或选择一个文件".to_string()));
            return;
        }

        spawn(async move {
            importing.set(true);
            import_error.set(None);
            import_report.set(None);

            match import_bookmarks(text).await {
                Ok(report) => {
                    import_report.set(Some(report));
                    import_text.set(String::new());
                    import_name.set(String::new());
                    importing.set(false);
                    on_bookmarks_changed.call(());
                    // 导入可能新建了分组
                    on_groups_changed.call(());
                }
                Err(e) => {
                    import_error.set(Some(error_text(&e)));
                    importing.set(false);
                }
            }
        });
    };

    let pick_file = move |_| {
        crate::dom::read_selected_file_text(IMPORT_FILE_ID, move |text| match text {
            Some(text) => {
                import_name.set("已读取文件，点「开始导入」提交".to_string());
                // 直接导入：文件选择本身就是明确动作，不必再点一次按钮
                run_import(text);
            }
            None => import_error.set(Some("读取文件失败，或没有选择文件".to_string())),
        });
    };

    let dim = draft().background_dim;
    let tile_size = draft().tile_size;
    let gap_x = draft().tile_gap_x;
    let gap_y = draft().tile_gap_y;
    let columns = draft().grid_columns;
    let theme = draft().theme.clone();
    let preset = draft().background_preset.clone();

    rsx! {
        // ---------- 效果预览 ----------
        section { class: "panel-section",
            h3 { class: "panel-heading", "效果预览" }
            div {
                class: "preview-grid",
                style: "grid-template-columns: repeat(auto-fit, {tile_size}px); gap: {gap_y}px {gap_x}px;",
                for label in ["A", "B", "C"] {
                    div { class: "preview-tile",
                        style: "width: {tile_size}px;",
                        div {
                            class: "preview-icon",
                            style: "width: calc({tile_size}px * 0.58); height: calc({tile_size}px * 0.58);",
                            "{label}"
                        }
                        span { class: "preview-label", "示例" }
                    }
                }
            }
            p { class: "field-hint",
                if columns > 0 {
                    "每行固定 {columns} 列（列数设为 0 时按宽度自适应）。"
                } else {
                    "列数自适应：按容器宽度决定每行放几个。"
                }
            }
        }

        // ---------- 外观 ----------
        section { class: "panel-section",
            h3 { class: "panel-heading", "外观" }

            label { class: "field",
                span { class: "field-label", "主题" }
                div { class: "segmented",
                    for name in THEMES {
                        button {
                            key: "{name}",
                            class: if theme == name { "segment active" } else { "segment" },
                            r#type: "button",
                            onclick: move |_| draft.with_mut(|s| s.theme = name.to_string()),
                            if name == "dark" { "深色" } else { "浅色" }
                        }
                    }
                }
            }

            label { class: "field",
                span { class: "field-label", "内置壁纸" }
                div { class: "segmented wrap",
                    button {
                        class: if preset.is_empty() { "segment active" } else { "segment" },
                        r#type: "button",
                        onclick: move |_| draft.with_mut(|s| s.background_preset = String::new()),
                        "默认"
                    }
                    for name in BACKGROUND_PRESETS {
                        button {
                            key: "{name}",
                            class: if preset == name { "segment active" } else { "segment" },
                            r#type: "button",
                            title: "{name}",
                            onclick: move |_| draft.with_mut(|s| s.background_preset = name.to_string()),
                            div { class: "preset-swatch preset-{name}" }
                        }
                    }
                }
                p { class: "field-hint",
                    "内置渐变不依赖外网，比外链图片可靠（外链可能限流或防盗链）。设置背景图后此项被忽略。"
                }
            }

            label { class: "field",
                span { class: "field-label", "背景图片地址" }
                input {
                    class: "input",
                    placeholder: "留空则使用上面的内置壁纸",
                    value: "{draft().background_url}",
                    oninput: move |e| draft.with_mut(|s| s.background_url = e.value()),
                }
            }

            label { class: "field",
                span { class: "field-label", "亮度遮罩　{dim}%" }
                input {
                    class: "range",
                    r#type: "range",
                    min: "0",
                    max: "80",
                    value: "{dim}",
                    oninput: move |e| draft.with_mut(|s| {
                        s.background_dim = e.value().parse().unwrap_or(35)
                    }),
                }
                p { class: "field-hint", "仅在设置了背景图片时生效，用来压暗背景保证文字可读。" }
            }

            label { class: "switch",
                input {
                    r#type: "checkbox",
                    checked: draft().background_random,
                    onchange: move |e| draft.with_mut(|s| s.background_random = e.checked()),
                }
                span { "每次打开换一张" }
            }
            p { class: "field-hint",
                "适用于随机图接口。图片接口通常返回很长的缓存头（如 30 天），而地址又不变，"
                "不勾选的话浏览器会一直用同一张缓存图；勾选后每次打开会在地址后追加一个随机参数。"
            }
        }

        // ---------- 布局 ----------
        section { class: "panel-section",
            h3 { class: "panel-heading", "卡片与布局" }

            label { class: "field",
                span { class: "field-label", "卡片宽度　{tile_size}px" }
                input {
                    class: "range",
                    r#type: "range",
                    min: "56",
                    max: "160",
                    step: "4",
                    value: "{tile_size}",
                    oninput: move |e| draft.with_mut(|s| {
                        s.tile_size = e.value().parse().unwrap_or(96)
                    }),
                }
            }

            label { class: "field",
                span { class: "field-label", "横向间距　{gap_x}px" }
                input {
                    class: "range",
                    r#type: "range",
                    min: "0",
                    max: "48",
                    step: "2",
                    value: "{gap_x}",
                    oninput: move |e| draft.with_mut(|s| {
                        s.tile_gap_x = e.value().parse().unwrap_or(8)
                    }),
                }
            }

            label { class: "field",
                span { class: "field-label", "纵向间距　{gap_y}px" }
                input {
                    class: "range",
                    r#type: "range",
                    min: "0",
                    max: "64",
                    step: "2",
                    value: "{gap_y}",
                    oninput: move |e| draft.with_mut(|s| {
                        s.tile_gap_y = e.value().parse().unwrap_or(20)
                    }),
                }
            }

            label { class: "field",
                span { class: "field-label", "每行列数　（0 = 自适应）" }
                input {
                    class: "input",
                    r#type: "number",
                    min: "0",
                    max: "16",
                    value: "{columns}",
                    oninput: move |e| draft.with_mut(|s| {
                        s.grid_columns = e.value().parse().unwrap_or(0)
                    }),
                }
                p { class: "field-hint", "设成固定列数后，窗口变窄也不会换行，可能横向溢出。" }
            }
        }

        // ---------- 时间 ----------
        section { class: "panel-section",
            h3 { class: "panel-heading", "时钟与一言" }

            label { class: "switch",
                input {
                    r#type: "checkbox",
                    checked: draft().show_clock,
                    onchange: move |e| draft.with_mut(|s| s.show_clock = e.checked()),
                }
                span { "显示时钟" }
            }

            label { class: "switch",
                input {
                    r#type: "checkbox",
                    checked: draft().clock_show_lunar,
                    onchange: move |e| draft.with_mut(|s| s.clock_show_lunar = e.checked()),
                }
                span { "显示农历" }
            }
            p { class: "field-hint", "如「二〇二六年九月初四」，本地算出来的，不发请求。" }

            label { class: "switch",
                input {
                    r#type: "checkbox",
                    checked: draft().clock_show_day,
                    onchange: move |e| draft.with_mut(|s| s.clock_show_day = e.checked()),
                }
                span { "显示第几天 / 第几周" }
            }

            label { class: "switch",
                input {
                    r#type: "checkbox",
                    checked: draft().show_quote,
                    onchange: move |e| draft.with_mut(|s| s.show_quote = e.checked()),
                }
                span { "显示一言" }
            }
        }

        // ---------- 行为 ----------
        section { class: "panel-section",
            h3 { class: "panel-heading", "打开与翻页" }

            label { class: "switch",
                input {
                    r#type: "checkbox",
                    checked: draft().open_in_new_tab,
                    onchange: move |e| draft.with_mut(|s| s.open_in_new_tab = e.checked()),
                }
                span { "在新标签页打开书签" }
            }
            p { class: "field-hint", "关掉则当前页直接跳转（书签页会被顶掉）。搜索框始终开新标签页。" }

            label { class: "switch",
                input {
                    r#type: "checkbox",
                    checked: draft().hide_grid,
                    onchange: move |e| draft.with_mut(|s| s.hide_grid = e.checked()),
                }
                span { "简约模式（隐藏整个卡片区）" }
            }
            p { class: "field-hint", "只留壁纸、时钟和搜索框，适合把这个页面当纯起始页用。" }

            label { class: "field",
                span { class: "field-label", "每页卡片数　（0 = 不分页）" }
                input {
                    class: "input",
                    r#type: "number",
                    min: "0",
                    max: "200",
                    value: "{draft().page_size}",
                    oninput: move |e| draft.with_mut(|s| {
                        s.page_size = e.value().parse().unwrap_or(0)
                    }),
                }
                p { class: "field-hint",
                    "书签多了一屏滚到底会很久。分页只影响显示，拖拽排序仍然作用在全量顺序上。"
                }
            }

            label { class: "switch",
                input {
                    r#type: "checkbox",
                    checked: draft().page_wheel,
                    onchange: move |e| draft.with_mut(|s| s.page_wheel = e.checked()),
                }
                span { "鼠标滚轮翻页" }
            }
            p { class: "field-hint", "开启后滚轮不再滚动页面，而是翻页。只在设置了每页卡片数时生效。" }

            label { class: "switch",
                input {
                    r#type: "checkbox",
                    checked: draft().page_loop,
                    onchange: move |e| draft.with_mut(|s| s.page_loop = e.checked()),
                }
                span { "循环翻页" }
            }

            div { class: "form-actions",
                if saved() {
                    span { class: "saved-hint", "已保存" }
                }
                button {
                    class: "btn-primary",
                    r#type: "button",
                    disabled: saving(),
                    onclick: save,
                    if saving() { "保存中…" } else { "保存设置" }
                }
            }

            if let Some(message) = save_error() {
                p { class: "form-error", "{message}" }
            }
        }

        // ---------- 数据 ----------
        section { class: "panel-section",
            h3 { class: "panel-heading", "数据" }

            div { class: "field",
                span { class: "field-label", "导入书签" }
                div { class: "inline-form",
                    label {
                        class: "btn-ghost file-btn",
                        r#for: IMPORT_FILE_ID,
                        "选择文件"
                        input {
                            id: IMPORT_FILE_ID,
                            class: "visually-hidden",
                            r#type: "file",
                            accept: ".html,.htm,.json,text/html,application/json",
                            onchange: pick_file,
                        }
                    }
                    if !import_name().is_empty() {
                        span { class: "field-hint", "{import_name}" }
                    }
                }
                textarea {
                    class: "textarea",
                    rows: "5",
                    placeholder: "或把浏览器导出的书签 HTML 全文粘贴到这里（也支持本应用导出的 JSON 备份）",
                    value: "{import_text}",
                    oninput: move |e| import_text.set(e.value()),
                }
                p { class: "field-hint",
                    "浏览器：书签管理器 → 导出书签 → 得到 .html 文件，可直接选文件，也可以打开后复制粘贴。"
                    "JSON 备份里的分组会一并还原（分组不存在时自动创建）；浏览器 HTML 的文件夹路径进标签。"
                }
            }

            div { class: "form-actions",
                button {
                    class: "btn-primary",
                    r#type: "button",
                    disabled: importing(),
                    onclick: move |_| run_import(import_text()),
                    if importing() { "导入中…" } else { "开始导入" }
                }
            }

            if let Some(report) = import_report() {
                p { class: "import-ok",
                    "已导入 {report.imported} 条，跳过 {report.skipped} 条（URL 重复或非法）"
                }
            }
            if let Some(message) = import_error() {
                p { class: "form-error", "{message}" }
            }

            div { class: "field",
                span { class: "field-label", "导出书签" }
                a {
                    class: "btn-ghost export-link",
                    href: "/export/bookmarks.json",
                    download: "mtab-bookmarks.json",
                    "下载 JSON 备份"
                }
                p { class: "field-hint", "含标签、分组与顺序，可直接粘回上方文本框恢复。" }
            }
        }

        // 分组与回收站各自管自己的数据；分组列表由父级传入（标签栏也要用同一份）
        GroupSettings {
            groups,
            on_changed: move |_| on_groups_changed.call(()),
        }

        TrashPanel { on_changed: move |_| on_bookmarks_changed.call(()) }
    }
}
