use dioxus::prelude::*;

use super::background_panel::BackgroundPanel;
use super::group_settings::GroupSettings;
use super::quick_add_panel::QuickAddPanel;
use super::trash_panel::TrashPanel;
use crate::backend::{clear_icon_cache, error_text, import_bookmarks, save_settings};
use crate::models::{Group, ImportReport, Settings, BACKGROUND_PRESETS, THEMES};

/// 文件选择器的 id，读文件时靠它取到 `<input type="file">`。
const IMPORT_FILE_ID: &str = "nekotab-import-file";

/// 读取所选文件的脚本（交给 `document::eval` 跑）。
///
/// **这里只允许调用 `dioxus.send`**——它是 Dioxus 提供的通道。绝不能在其中碰
/// Dioxus 的运行时 API（写 signal、`spawn`、调事件处理器）：这些都要取
/// 「当前作用域」，而原生 JS 回调不在任何作用域里，dioxus-core 会
/// `scope_stack.last().unwrap()` 直接 panic；release 下 `panic = "abort"`，
/// 于是整个页面死掉（现象是控制台一句 `RuntimeError: unreachable`，界面再也不响应）。
///
/// 早先的实现正是踩了这个坑：用 wasm-bindgen 的 `Closure` 直接回调 Rust，
/// 在回调里写 signal 并调用 `spawn`，一选文件就崩。现在读到的文本只是发回来，
/// 后续所有动作都在 `onchange` 事件触发的那次 `spawn` 里做（task 轮询时
/// Dioxus 会把作用域压在栈上，见 dioxus-core `tasks.rs`）。
const READ_FILE_SCRIPT: &str = r#"
const input = document.getElementById('nekotab-import-file');
const file = input && input.files && input.files[0];
if (!file) {
    dioxus.send(null);
} else {
    const reader = new FileReader();
    reader.onload = () => dioxus.send(String(reader.result ?? ''));
    reader.onerror = () => dioxus.send(null);
    reader.readAsText(file);
}
"#;

/// 设置面板的分区。
#[derive(Clone, Copy, PartialEq, Eq)]
enum Section {
    Site,
    Appearance,
    Background,
    Layout,
    Clock,
    Behavior,
    Data,
    QuickAdd,
    Groups,
    Trash,
}

/// `(分区, 菜单文字)`。顺序即左侧菜单的显示顺序。
const SECTIONS: [(Section, &str); 10] = [
    (Section::Site, "站点信息"),
    (Section::Appearance, "外观"),
    (Section::Background, "背景"),
    (Section::Layout, "卡片与布局"),
    (Section::Clock, "时钟与一言"),
    (Section::Behavior, "打开与翻页"),
    (Section::Data, "数据"),
    (Section::QuickAdd, "快捷添加"),
    (Section::Groups, "分组管理"),
    (Section::Trash, "回收站"),
];

impl Section {
    /// 这一节是否用那套"草稿 + 保存"的设置项。
    fn is_settings(self) -> bool {
        matches!(
            self,
            Section::Site
                | Section::Appearance
                | Section::Background
                | Section::Layout
                | Section::Clock
                | Section::Behavior
        )
    }
}

/// 背景那几项的快照：既用来判断"改了没有"，也记下"已经落库的是哪一版"。
///
/// 只比这五项、而不是整个 `Settings`：背景是"看着调"的东西——传完图、点完选图、
/// 拖完遮罩就该当场看到结果，为一个动作再点一次「保存设置」是多余的。
/// 主题、间距那些偏好不在此列，仍然手动保存。
#[derive(Clone, PartialEq)]
struct BackgroundSnapshot {
    preset: String,
    urls: Vec<String>,
    url: String,
    dim: i64,
    random: bool,
}

impl BackgroundSnapshot {
    fn of(settings: &Settings) -> Self {
        Self {
            preset: settings.background_preset.clone(),
            urls: settings.background_urls.clone(),
            url: settings.background_url.clone(),
            dim: settings.background_dim,
            random: settings.background_random,
        }
    }
}

/// 主页设置面板：左侧分区菜单 + 右侧内容。
///
/// 早先是单个 400px 抽屉从上滚到底，加了分组管理与回收站之后太长，改成两栏：
/// 左边选分区，右边只看这一节。
///
/// 所有设置字段装在一个 `Settings` 草稿里（而不是一个字段一个 signal），加设置项
/// 时只需要改 `models.rs` 的 `to_pairs` / `from_pairs` 和这里的控件。设置存的是
/// 键值表，所以**加字段不需要数据库迁移**。
#[component]
pub fn SettingsPanel(
    settings: Settings,
    groups: Vec<Group>,
    on_settings_saved: EventHandler<()>,
    on_bookmarks_changed: EventHandler<()>,
    on_groups_changed: EventHandler<()>,
) -> Element {
    let mut active = use_signal(|| Section::Appearance);
    // 首次渲染即从服务端带来的设置初始化；重挂面板时自然同步
    let mut draft = use_signal(|| settings.clone());

    let mut saving = use_signal(|| false);
    let mut save_error = use_signal(|| Option::<String>::None);
    let mut saved = use_signal(|| false);

    let mut import_text = use_signal(String::new);
    let mut import_name = use_signal(String::new);
    let mut importing = use_signal(|| false);
    let mut reading_file = use_signal(|| false);
    let mut import_error = use_signal(|| Option::<String>::None);
    let mut import_report = use_signal(|| Option::<ImportReport>::None);
    let mut wallpaper_msg = use_signal(|| Option::<String>::None);
    let mut clearing_icons = use_signal(|| false);
    let mut icon_clear_msg = use_signal(|| Option::<String>::None);

    // 已经落库的那一版背景。初值就是面板打开时服务端给的那份。
    let mut bg_saved = use_signal(|| BackgroundSnapshot::of(&settings));
    // "已经有一个存盘循环在跑"的标记。只读 `peek`（不订阅），所以它自己变来变去
    // 不会把 effect 再勾起来跑一次。
    let mut bg_saving = use_signal(|| false);

    // 背景的改动即时生效，不必再点一次「保存设置」：上传、选图、移出是"动作"，
    // 遮罩与轮换开关是"看着调"的旋钮，都该当场看到结果。
    //
    // 拖遮罩滑块会连发 input 事件，所以由一个循环合并处理：睡一小会儿，看草稿与
    // 已落库的那版还差就存一次，循环到两者一致为止。循环期间新的改动会被它下一轮
    // 捡走，effect 那边直接让路。
    use_effect(move || {
        if BackgroundSnapshot::of(&draft()) == bg_saved() || *bg_saving.peek() {
            return;
        }

        bg_saving.set(true);
        spawn(async move {
            loop {
                #[cfg(feature = "web")]
                gloo_timers::future::TimeoutFuture::new(400).await;

                let snapshot = BackgroundSnapshot::of(&draft());
                if snapshot == bg_saved() {
                    break;
                }

                match save_settings(draft()).await {
                    Ok(()) => {
                        bg_saved.set(snapshot);
                        save_error.set(None);
                        saved.set(true);
                        // 首页那份设置要跟着刷新，背景才会当场换掉
                        on_settings_saved.call(());
                    }
                    Err(e) => {
                        save_error.set(Some(error_text(&e)));
                        break;
                    }
                }
            }

            // 这里没有 await，改動不可能插进来（wasm 单线程），所以不会漏掉最后一次
            bg_saving.set(false);
        });
    });

    let section = active();

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

                    // `<title>` / meta / favicon 是 SSR 时写进 head 的；Dioxus 的
                    // document 元素在水合后不会再插入。站点信息改完直接刷新一次，
                    // 比留着一份"标题变了、描述没变"的 head 更可预期。
                    #[cfg(feature = "web")]
                    if section == Section::Site {
                        spawn(async move {
                            gloo_timers::future::TimeoutFuture::new(600).await;
                            let _ = document::eval("window.location.reload();");
                        });
                    }
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
                    // 导入会按文件夹建分组
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
        if importing() || reading_file() {
            return;
        }

        // 这一次 spawn 是安全的：它由 onchange 事件触发，作用域正常。
        // 读文件本身交给 document::eval，JS 里只做 dioxus.send。
        spawn(async move {
            reading_file.set(true);
            import_error.set(None);
            import_report.set(None);

            let mut eval = document::eval(READ_FILE_SCRIPT);
            let text = eval.recv::<Option<String>>().await.ok().flatten();
            reading_file.set(false);

            match text {
                Some(text) if !text.trim().is_empty() => {
                    import_name.set("已读取文件，正在导入…".to_string());
                    run_import(text);
                }
                _ => {
                    import_name.set(String::new());
                    import_error.set(Some("读取文件失败，或没有选择文件".to_string()));
                }
            }
        });
    };

    rsx! {
        div { class: "settings-panes",
            nav { class: "settings-menu",
                for (target, label) in SECTIONS {
                    button {
                        key: "{label}",
                        class: if section == target { "settings-menu-item active" } else { "settings-menu-item" },
                        r#type: "button",
                        onclick: move |_| active.set(target),
                        "{label}"
                    }
                }
            }

            div { class: "settings-content",
                match section {
                    // ---------- 站点信息 ----------
                    Section::Site => rsx! {
                        section { class: "panel-section",
                            h3 { class: "panel-heading", "站点信息" }
                            p { class: "field-hint",
                                "这些是浏览器标签页、分享卡片上的信息，不是导航页的内容。"
                                "保存后页面会自动刷新一次，让 head 里的标题/图标立即生效。"
                            }

                            label { class: "field",
                                span { class: "field-label", "标题" }
                                input {
                                    class: "input",
                                    maxlength: "60",
                                    placeholder: "NekoTab",
                                    value: "{draft().site_title}",
                                    oninput: move |e| draft.with_mut(|s| s.site_title = e.value()),
                                }
                            }

                            label { class: "field",
                                span { class: "field-label", "描述" }
                                input {
                                    class: "input",
                                    maxlength: "200",
                                    placeholder: "极简自部署书签导航",
                                    value: "{draft().site_description}",
                                    oninput: move |e| draft.with_mut(|s| s.site_description = e.value()),
                                }
                            }

                            label { class: "field",
                                span { class: "field-label", "关键词（可选）" }
                                input {
                                    class: "input",
                                    maxlength: "200",
                                    placeholder: "书签, 导航, 起始页",
                                    value: "{draft().site_keywords}",
                                    oninput: move |e| draft.with_mut(|s| s.site_keywords = e.value()),
                                }
                            }

                            label { class: "field",
                                span { class: "field-label", "站点图标" }
                                input {
                                    class: "input",
                                    placeholder: "留空用内置图标；也可填 https://… 或 /…",
                                    value: "{draft().site_logo}",
                                    oninput: move |e| draft.with_mut(|s| s.site_logo = e.value()),
                                }
                            }

                            label { class: "field",
                                span { class: "field-label", "主题色" }
                                input {
                                    class: "input",
                                    maxlength: "7",
                                    placeholder: "留空跟随主题；例如 #0b0d13",
                                    value: "{draft().site_theme_color}",
                                    oninput: move |e| draft.with_mut(|s| s.site_theme_color = e.value()),
                                }
                                p { class: "field-hint",
                                    "移动端地址栏的颜色，填 #rrggbb 或 #rgb；留空则跟随深浅色主题。"
                                }
                            }
                        }

                        section { class: "panel-section",
                            h3 { class: "panel-heading", "访问控制" }
                            p { class: "field-hint",
                                "部署时设置环境变量 ACCESS_PASSWORD 后，整站需要密码才能进入，"
                                "登录状态保持 30 天。没有设置时这一节不影响使用。"
                            }
                            a {
                                class: "btn-ghost export-link",
                                href: "/logout",
                                "退出登录"
                            }
                        }
                    },

                    // ---------- 外观 ----------
                    Section::Appearance => rsx! {
                        section { class: "panel-section",
                            h3 { class: "panel-heading", "外观" }

                            div { class: "field",
                                span { class: "field-label", "主题" }
                                div { class: "segmented",
                                    for name in THEMES {
                                        button {
                                            key: "{name}",
                                            class: if draft().theme == name { "segment active" } else { "segment" },
                                            r#type: "button",
                                            onclick: move |_| draft.with_mut(|s| s.theme = name.to_string()),
                                            match name {
                                                "system" => "跟随系统",
                                                "light" => "浅色",
                                                _ => "深色",
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    },

                    // ---------- 背景 ----------
                    // 单开一节而不是挤在「外观」里：这里的东西（内置渐变 + 池 +
                    // 遮罩 + 轮换开关）本来就比外观那一项多得多，池还需要整屏放缩略图。
                    Section::Background => rsx! {
                        p { class: "field-hint", "背景的改动即时生效，不用点下面的「保存设置」。" }

                        section { class: "panel-section",
                            h3 { class: "panel-heading", "内置渐变" }
                            div { class: "segmented wrap",
                                button {
                                    class: if draft().background_preset.is_empty() { "segment active" } else { "segment" },
                                    r#type: "button",
                                    onclick: move |_| draft.with_mut(|s| s.background_preset = String::new()),
                                    "默认"
                                }
                                for name in BACKGROUND_PRESETS {
                                    button {
                                        key: "{name}",
                                        class: if draft().background_preset == name { "segment active" } else { "segment" },
                                        r#type: "button",
                                        title: "{name}",
                                        onclick: move |_| draft.with_mut(|s| s.background_preset = name.to_string()),
                                        div { class: "preset-swatch preset-{name}" }
                                    }
                                }
                            }
                            p { class: "field-hint",
                                "内置渐变不依赖外网，比外链图片可靠（外链可能限流或防盗链）。池里有背景时此项被忽略。"
                            }
                        }

                        BackgroundPanel {
                            pool: draft().background_urls.clone(),
                            current: draft().background_url.clone(),
                            rotating: draft().background_random,
                            on_pool: move |urls: Vec<String>| {
                                draft.with_mut(|s| s.background_urls = urls);
                                saved.set(false);
                            },
                            on_pick: move |url: String| {
                                draft.with_mut(|s| s.background_url = url);
                                saved.set(false);
                            },
                            on_message: move |msg: Option<String>| wallpaper_msg.set(msg),
                        }

                        if let Some(msg) = wallpaper_msg() {
                            p { class: "field-hint", "{msg}" }
                        }

                        section { class: "panel-section",
                            h3 { class: "panel-heading", "显示" }

                            label { class: "field",
                                span { class: "field-label", "亮度遮罩　{draft().background_dim}%" }
                                input {
                                    class: "range",
                                    r#type: "range",
                                    min: "0",
                                    max: "80",
                                    value: "{draft().background_dim}",
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
                                "池里有两张以上：每次打开随机挑一张（两次抽到同一张是可能的）。只有一张"
                                "（或池是空的、只填了一条地址）没有可换的：改为给地址追加一个随机参数——"
                                "图片接口通常返回 30 天缓存头而地址又不变，不加参数的话浏览器会一直用同一张缓存图。"
                            }
                        }
                    },

                    // ---------- 卡片与布局 ----------
                    Section::Layout => rsx! {
                        section { class: "panel-section",
                            h3 { class: "panel-heading", "效果预览" }
                            div {
                                class: "preview-grid",
                                style: "grid-template-columns: repeat(auto-fit, {draft().tile_size}px); gap: {draft().tile_gap_y}px {draft().tile_gap_x}px; max-width: {draft().grid_max_width()};",
                                for label in ["A", "B", "C"] {
                                    div { class: "preview-tile", style: "width: {draft().tile_size}px;",
                                        div {
                                            class: "preview-icon",
                                            style: "width: calc({draft().tile_size}px * 0.58); height: calc({draft().tile_size}px * 0.58);",
                                            "{label}"
                                        }
                                        span { class: "preview-label", "示例" }
                                    }
                                }
                            }
                            p { class: "field-hint",
                                if draft().grid_columns > 0 {
                                    "每行最多 {draft().grid_columns} 列，放不下时自动换行。"
                                } else {
                                    "列数自适应：按容器宽度决定每行放几个。"
                                }
                            }
                        }

                        section { class: "panel-section",
                            h3 { class: "panel-heading", "卡片与布局" }

                            label { class: "field",
                                span { class: "field-label", "卡片宽度　{draft().tile_size}px" }
                                input {
                                    class: "range",
                                    r#type: "range",
                                    min: "56",
                                    max: "160",
                                    step: "4",
                                    value: "{draft().tile_size}",
                                    oninput: move |e| draft.with_mut(|s| {
                                        s.tile_size = e.value().parse().unwrap_or(96)
                                    }),
                                }
                            }

                            label { class: "field",
                                span { class: "field-label", "横向间距　{draft().tile_gap_x}px" }
                                input {
                                    class: "range",
                                    r#type: "range",
                                    min: "0",
                                    max: "48",
                                    step: "2",
                                    value: "{draft().tile_gap_x}",
                                    oninput: move |e| draft.with_mut(|s| {
                                        s.tile_gap_x = e.value().parse().unwrap_or(8)
                                    }),
                                }
                            }

                            label { class: "field",
                                span { class: "field-label", "纵向间距　{draft().tile_gap_y}px" }
                                input {
                                    class: "range",
                                    r#type: "range",
                                    min: "0",
                                    max: "64",
                                    step: "2",
                                    value: "{draft().tile_gap_y}",
                                    oninput: move |e| draft.with_mut(|s| {
                                        s.tile_gap_y = e.value().parse().unwrap_or(20)
                                    }),
                                }
                            }

                            label { class: "field",
                                span { class: "field-label", "每行最大列数　（0 = 不限）" }
                                input {
                                    class: "input",
                                    r#type: "number",
                                    min: "0",
                                    max: "16",
                                    value: "{draft().grid_columns}",
                                    oninput: move |e| draft.with_mut(|s| {
                                        s.grid_columns = e.value().parse().unwrap_or(0)
                                    }),
                                }
                                p { class: "field-hint", "最多排几列；窗口变窄放不下时会自动换行，不会横向溢出。" }
                            }
                        }
                    },

                    // ---------- 时钟与一言 ----------
                    Section::Clock => rsx! {
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
                                    checked: draft().clock_show_seconds,
                                    onchange: move |e| draft.with_mut(|s| s.clock_show_seconds = e.checked()),
                                }
                                span { "显示秒" }
                            }

                            label { class: "switch",
                                input {
                                    r#type: "checkbox",
                                    checked: draft().clock_hour12,
                                    onchange: move |e| draft.with_mut(|s| s.clock_hour12 = e.checked()),
                                }
                                span { "12 小时制" }
                            }
                            p { class: "field-hint", "关闭为 24 小时制；开启后时间前面带「上午 / 下午」。" }

                            label { class: "switch",
                                input {
                                    r#type: "checkbox",
                                    checked: draft().show_quote,
                                    onchange: move |e| draft.with_mut(|s| s.show_quote = e.checked()),
                                }
                                span { "显示一言" }
                            }
                        }
                    },

                    // ---------- 打开与翻页 ----------
                    Section::Behavior => rsx! {
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
                        }
                    },

                    // ---------- 数据 ----------
                    Section::Data => rsx! {
                        section { class: "panel-section",
                            h3 { class: "panel-heading", "导入书签" }

                            div { class: "field",
                                div { class: "inline-form",
                                    label {
                                        class: "btn-ghost file-btn",
                                        r#for: IMPORT_FILE_ID,
                                        if reading_file() { "读取中…" } else { "选择文件" }
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
                                    rows: "6",
                                    placeholder: "或把浏览器导出的书签 HTML 全文粘贴到这里（也支持本应用导出的 JSON 备份）",
                                    value: "{import_text}",
                                    oninput: move |e| import_text.set(e.value()),
                                }
                                p { class: "field-hint",
                                    "浏览器：书签管理器 → 导出书签 → 得到 .html 文件，可直接选文件，也可以打开后复制粘贴。"
                                }
                                p { class: "field-hint",
                                    "浏览器 HTML 的**文件夹会变成分组**（取第一层非容器文件夹，"
                                    "「书签栏 / Bookmarks bar」这类容器会被跳过）；"
                                    "JSON 备份按里面的分组名还原，不存在就自动建。按 URL 去重。"
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
                        }

                        section { class: "panel-section",
                            h3 { class: "panel-heading", "站点图标缓存" }
                            p { class: "field-hint",
                                "卡片图标由服务端代取并缓存（字节存在 ICON_DIR，库里只留元数据）。"
                                "取不到的站点会记一条负缓存，在有效期内不再重试——所以修好网络、"
                                "或对方站点恢复之后，图标可能仍缺一会儿。清掉缓存即可立刻全部重新代取。"
                            }
                            div { class: "form-actions start",
                                button {
                                    class: "btn-ghost",
                                    r#type: "button",
                                    disabled: clearing_icons(),
                                    onclick: move |_| {
                                        spawn(async move {
                                            clearing_icons.set(true);
                                            icon_clear_msg.set(None);
                                            match clear_icon_cache().await {
                                                Ok(n) => {
                                                    icon_clear_msg.set(Some(format!(
                                                        "已清掉 {n} 条记录，重新打开页面或刷新即可看到重新代取的图标"
                                                    )));
                                                    clearing_icons.set(false);
                                                }
                                                Err(e) => {
                                                    icon_clear_msg.set(Some(format!("清空失败：{}", error_text(&e))));
                                                    clearing_icons.set(false);
                                                }
                                            }
                                        });
                                    },
                                    if clearing_icons() { "清空中…" } else { "清空图标缓存" }
                                }
                                if let Some(msg) = icon_clear_msg() {
                                    span { class: "field-hint", "{msg}" }
                                }
                            }
                        }

                        section { class: "panel-section",
                            h3 { class: "panel-heading", "导出书签" }
                            a {
                                class: "btn-ghost export-link",
                                href: "/export/bookmarks.json",
                                download: "nekotab-bookmarks.json",
                                "下载 JSON 备份"
                            }
                            p { class: "field-hint", "含分组与顺序，可直接粘回上方文本框恢复。" }
                        }
                    },

                    // ---------- 快捷添加 ----------
                    Section::QuickAdd => rsx! {
                        QuickAddPanel {}
                    },

                    // ---------- 分组管理 ----------
                    Section::Groups => rsx! {
                        GroupSettings { groups, on_changed: move |_| on_groups_changed.call(()) }
                    },

                    // ---------- 回收站 ----------
                    Section::Trash => rsx! {
                        TrashPanel { on_changed: move |_| on_bookmarks_changed.call(()) }
                    },
                }

                // 设置类分区共用一个保存条
                if section.is_settings() {
                    div { class: "settings-savebar",
                        if let Some(message) = save_error() {
                            span { class: "form-error", "{message}" }
                        }
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
                }
            }
        }
    }
}
