use dioxus::prelude::*;

use crate::backend::{error_text, import_bookmarks, save_settings};
use crate::models::{ImportReport, Settings};

/// 主页设置面板（抽屉内容）：外观 + 数据导入导出。
#[component]
pub fn SettingsPanel(
    settings: Settings,
    on_saved: EventHandler<()>,
    on_imported: EventHandler<()>,
) -> Element {
    let initial = settings.clone();

    let mut background_url = use_signal(|| initial.background_url.clone());
    let mut background_dim = use_signal(|| initial.background_dim);
    let mut background_random = use_signal(|| initial.background_random);
    let mut show_clock = use_signal(|| initial.show_clock);
    let mut show_quote = use_signal(|| initial.show_quote);

    let mut saving = use_signal(|| false);
    let mut save_error = use_signal(|| Option::<String>::None);
    let mut saved = use_signal(|| false);

    let mut import_text = use_signal(String::new);
    let mut importing = use_signal(|| false);
    let mut import_error = use_signal(|| Option::<String>::None);
    let mut import_report = use_signal(|| Option::<ImportReport>::None);

    let save = move |_| {
        if saving() {
            return;
        }

        let payload = Settings {
            background_url: background_url(),
            background_dim: background_dim(),
            background_random: background_random(),
            show_clock: show_clock(),
            show_quote: show_quote(),
        };

        spawn(async move {
            saving.set(true);
            save_error.set(None);
            saved.set(false);

            match save_settings(payload).await {
                Ok(()) => {
                    saved.set(true);
                    saving.set(false);
                    on_saved.call(());
                }
                Err(e) => {
                    save_error.set(Some(error_text(&e)));
                    saving.set(false);
                }
            }
        });
    };

    let run_import = move |_| {
        if importing() {
            return;
        }

        let text = import_text();
        if text.trim().is_empty() {
            import_error.set(Some("请先粘贴内容".to_string()));
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
                    importing.set(false);
                    on_imported.call(());
                }
                Err(e) => {
                    import_error.set(Some(error_text(&e)));
                    importing.set(false);
                }
            }
        });
    };

    rsx! {
        section { class: "panel-section",
            h3 { class: "panel-heading", "外观" }

            label { class: "field",
                span { class: "field-label", "背景图片地址" }
                input {
                    class: "input",
                    placeholder: "留空则使用内置渐变壁纸",
                    value: "{background_url}",
                    oninput: move |e| background_url.set(e.value()),
                }
            }

            label { class: "field",
                span { class: "field-label", "亮度遮罩　{background_dim()}%" }
                input {
                    class: "range",
                    r#type: "range",
                    min: "0",
                    max: "80",
                    value: "{background_dim}",
                    oninput: move |e| {
                        background_dim.set(e.value().parse().unwrap_or(35))
                    },
                }
                p { class: "field-hint", "仅在设置了背景图片时生效，用来压暗背景保证文字可读。" }
            }

            label { class: "switch",
                input {
                    r#type: "checkbox",
                    checked: background_random(),
                    onchange: move |e| background_random.set(e.checked()),
                }
                span { "每次打开换一张" }
            }
            p { class: "field-hint",
                "适用于随机图接口。图片接口通常返回很长的缓存头（如 30 天），而地址又不变，"
                "不勾选的话浏览器会一直用同一张缓存图；勾选后每次打开会在地址后追加一个随机参数。"
            }

            label { class: "switch",
                input {
                    r#type: "checkbox",
                    checked: show_clock(),
                    onchange: move |e| show_clock.set(e.checked()),
                }
                span { "显示时钟" }
            }

            label { class: "switch",
                input {
                    r#type: "checkbox",
                    checked: show_quote(),
                    onchange: move |e| show_quote.set(e.checked()),
                }
                span { "显示一言" }
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

        section { class: "panel-section",
            h3 { class: "panel-heading", "数据" }

            div { class: "field",
                span { class: "field-label", "导入书签" }
                textarea {
                    class: "textarea",
                    rows: "6",
                    placeholder: "把浏览器导出的书签 HTML 全文粘贴到这里（也支持本应用导出的 JSON 备份）",
                    value: "{import_text}",
                    oninput: move |e| import_text.set(e.value()),
                }
                p { class: "field-hint",
                    "浏览器：书签管理器 → 导出书签 → 得到 .html 文件，用文本编辑器打开后全选复制。"
                }
            }

            div { class: "form-actions",
                button {
                    class: "btn-primary",
                    r#type: "button",
                    disabled: importing(),
                    onclick: run_import,
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
                p { class: "field-hint", "含标签与顺序，可直接粘回上方文本框恢复。" }
            }
        }
    }
}
