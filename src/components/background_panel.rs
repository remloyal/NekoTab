use dioxus::prelude::*;

use crate::backend::{error_text, save_wallpaper_from_url};
use crate::models::{
    is_allowed_background, normalize_pool, BACKGROUND_POOL_MAX, BACKGROUND_URL_MAX,
};

/// 文件选择器的 id：上传脚本靠它取到 `<input type="file">`。
const FILE_INPUT_ID: &str = "nekotab-wallpaper-file";

/// 列出本机壁纸的脚本。返回 `[{ name, bytes }]`。
const LIST_SCRIPT: &str = r#"
fetch('/wallpaper')
    .then((r) => r.json())
    .then((items) => dioxus.send({ ok: true, items }))
    .catch((e) => dioxus.send({ ok: false, items: [], added: [], error: String(e) }));
"#;

/// 上传选中的所有文件。
///
/// `multiple` 让一次选多张成为可能。为什么用 `Promise.all` 而不是 `for await`：
/// 与下面几条脚本保持同一种形状（promise 链 + 最后 `dioxus.send`）。**原生 JS
/// 回调里绝不能碰 Dioxus 的运行时 API**（写 signal、spawn 都会 panic，release 下
/// 整页死掉），所以这里只 `dioxus.send`，Rust 侧的处理放回事件触发的 spawn 里。
///
/// 体积先在客户端挡一道：超过上限的请求发出去，服务端会在读请求体时把连接掐断，
/// 浏览器只会报 "Failed to fetch"——不如根本不发，直接说清楚是哪张、多大。
const UPLOAD_SCRIPT: &str = r#"
const MAX_BYTES = 16 * 1024 * 1024;    // 与 wallpaper::MAX_WALLPAPER_BYTES 一致
const input = document.getElementById('nekotab-wallpaper-file');
const picked = input && input.files ? Array.from(input.files) : [];
const files = picked.filter((file) => file.size <= MAX_BYTES);
const oversize = picked.filter((file) => file.size > MAX_BYTES);

if (!files.length) {
    dioxus.send({
        ok: false,
        items: [],
        added: [],
        error: oversize.length
            ? ('图片过大（上限 16 MB）：' + oversize.map((f) => f.name + ' ' + (f.size / 1048576).toFixed(1) + ' MB').join('、'))
            : '没有选择文件',
    });
} else {
    Promise.all(files.map((file) =>
        fetch('/wallpaper', { method: 'POST', headers: { 'Content-Type': file.type }, body: file })
            .then(async (r) => {
                const text = await r.text();
                return r.ok ? { url: JSON.parse(text).url } : { error: text || ('HTTP ' + r.status) };
            })
            .catch((e) => ({ error: String(e) }))
    )).then((results) => {
        dioxus.send({
            ok: results.every((one) => one.url),
            items: [],
            added: results.filter((one) => one.url).map((one) => one.url),
            error: (results.find((one) => one.error) || {}).error || (oversize.length ? ('有 ' + oversize.length + ' 张超过 16 MB，没上传') : ''),
        });
    });
}
"#;

/// 删除一张本机壁纸；`NAME` 由调用处注入。
const DELETE_SCRIPT: &str = r#"
fetch('/wallpaper/' + encodeURIComponent(NAME), { method: 'DELETE' })
    .then((r) => dioxus.send({ ok: r.ok, items: [], added: [], error: r.ok ? '' : ('HTTP ' + r.status) }))
    .catch((e) => dioxus.send({ ok: false, items: [], added: [], error: String(e) }));
"#;

/// 脚本回传的统一结果。
#[derive(serde::Deserialize)]
struct ScriptReply {
    ok: bool,
    #[serde(default)]
    items: Vec<LocalFile>,
    #[serde(default)]
    added: Vec<String>,
    #[serde(default)]
    error: String,
}

/// 「本机壁纸」的一项：文件名 + 体积。
///
/// 与服务端 `wallpaper::WallpaperEntry` 一一对应。那边整个模块是
/// `#[cfg(feature = "server")]`（壁纸只在服务端读写文件），客户端拿不到它的类型，
/// 只能按同一份 JSON 形状再声明一遍。
#[derive(serde::Deserialize, Clone, PartialEq)]
struct LocalFile {
    name: String,
    bytes: u64,
}

/// 本机文件的访问地址（也是它在池里的样子）。
fn url_of(name: &str) -> String {
    format!("/wallpaper/{name}")
}

/// 从池里的一条本机地址取回文件名；外链返回空串。
fn file_name_of(url: &str) -> String {
    url.rsplit('/').next().unwrap_or("").to_string()
}

/// 外链地址的站点名，供卡片上显示。
fn host_of(url: &str) -> String {
    url.split("//")
        .nth(1)
        .and_then(|rest| rest.split('/').next())
        .unwrap_or(url)
        .to_string()
}

/// 字节数说成人话（"62.3 MB"）。壁纸不参与自动清理，看不看得见占用决定了会不会去删。
fn human_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;

    let value = bytes as f64;
    if value >= MB {
        format!("{:.1} MB", value / MB)
    } else if value >= KB {
        format!("{:.0} KB", value / KB)
    } else {
        format!("{bytes} B")
    }
}

/// 往池里追加地址，返回 `(新池, 新加进去几条)`。
///
/// 过滤与截断交给 `models::normalize_pool`——与落库那条路径同一个口径，
/// 重复 / 非法 / 超长 / 装不下都在这儿挡掉，界面因此不必担心"池里混进脏值"。
fn extend_pool(pool: &[String], urls: impl IntoIterator<Item = String>) -> (Vec<String>, usize) {
    let before = normalize_pool(pool).len();
    let candidates: Vec<String> = pool.iter().cloned().chain(urls).collect();
    let next = normalize_pool(candidates);
    let added = next.len().saturating_sub(before);

    (next, added)
}

/// 列出本机已有的壁纸。
async fn list_files() -> Result<Vec<LocalFile>, String> {
    let reply = document::eval(LIST_SCRIPT)
        .recv::<ScriptReply>()
        .await
        .map_err(|e| e.to_string())?;
    if reply.ok {
        Ok(reply.items)
    } else {
        Err(reply.error)
    }
}

/// 删掉一张本机壁纸；`url` 形如 `/wallpaper/xxxx.png`。
async fn delete_file(url: &str) -> Result<(), String> {
    let name = file_name_of(url);
    let script = format!(
        "const NAME = {};\n{DELETE_SCRIPT}",
        serde_json::to_string(&name).unwrap_or_else(|_| "\"\"".to_string())
    );

    let reply = document::eval(&script)
        .recv::<ScriptReply>()
        .await
        .map_err(|e| e.to_string())?;
    if reply.ok {
        Ok(())
    } else {
        Err(reply.error)
    }
}

/// 背景池（设置 → 背景里的一节）。
///
/// 池里两类条目混在一起：外链地址、本机上传（`/wallpaper/xxxx.png`）。它们对用户
/// 是同一件事（"一张可用的背景"），差别只在两个来源特有的动作上——外链多一个
/// 「存到本地」，本机多一个「删除文件」。
///
/// 池是**唯一可编辑的入口**：以前"填地址"和"上传列表"是两条互不相干的路，同一个
/// 概念有两个真相来源。现在地址也往池里加，源只有一处。
///
/// 池底下还有一条「本机还有 N 张没在池里」的折叠条带：本机文件不等于池里条目
/// （旧版本上传的、手动丢进 `WALLPAPER_DIR` 的、被移出池的都在这一档），没有它
/// 那些文件会从界面上消失、删不掉，而壁纸目录是不参与任何自动清理的。
#[component]
pub fn BackgroundPanel(
    /// 背景池（设置草稿里的那份）
    pool: Vec<String>,
    /// 当前背景地址：池里被选中的那条；池为空时是手填的那条
    current: String,
    /// 「每次打开换一张」是否开着（只影响这里的说明文案）
    rotating: bool,
    /// 池变了（加 / 移出）
    on_pool: EventHandler<Vec<String>>,
    /// 设为当前背景
    on_pick: EventHandler<String>,
    /// 状态提示（成功或失败都走这里）
    on_message: EventHandler<Option<String>>,
) -> Element {
    let mut refresh = use_signal(|| 0u32);
    let mut busy = use_signal(|| false);
    let mut url_input = use_signal(String::new);
    let mut show_loose = use_signal(|| false);

    let list = use_resource(move || async move {
        let _ = refresh();
        list_files().await
    });

    let files: Vec<LocalFile> = match (&*list.read()).clone() {
        Some(Ok(items)) => items,
        _ => Vec::new(),
    };

    // 本机有、但不在池里的那些
    let loose: Vec<LocalFile> = files
        .iter()
        .filter(|file| !pool.iter().any(|url| *url == url_of(&file.name)))
        .cloned()
        .collect();

    let files_bytes: u64 = files.iter().map(|file| file.bytes).sum();
    let files_count = files.len();
    let full = pool.len() >= BACKGROUND_POOL_MAX;

    // 下面每个处理器都要读池与当前地址，而 `move` 闭包一旦捕获值类型就把那份吃掉了
    // （第二个处理器就没得用）。包一层 `Rc`：各自克隆的是句柄，读的是同一份数据。
    let pool = std::rc::Rc::new(pool);
    let current = std::rc::Rc::new(current);

    // ---- 上传（可多选）：成功即入池，并把最后一张设为当前 ----
    let upload = {
        let pool = pool.clone();
        move |_| {
            if busy() {
                return;
            }

            // 要带进异步块的东西先克隆出来：直接捕获外层的池会被 move 走，
            // 这个闭包就只剩 `FnOnce`，没法给列表里每张卡片都用一次
            let pool = pool.clone();

            spawn(async move {
                busy.set(true);
                on_message.call(None);

                let reply = document::eval(UPLOAD_SCRIPT).recv::<ScriptReply>().await;
                let (added, error) = match reply {
                    Ok(reply) => (reply.added, reply.error),
                    Err(e) => (Vec::new(), e.to_string()),
                };

                if !added.is_empty() {
                    let (next, count) = extend_pool(&pool, added.clone());
                    on_pool.call(next);
                    // 刚传上来的那张多半就是想用的，直接设为当前
                    on_pick.call(added.last().cloned().unwrap_or_default());

                    let skipped = added.len() - count;
                    let mut message = format!("已上传 {count} 张，并加入池");
                    if skipped > 0 {
                        message.push_str(&format!("（{skipped} 张重复，没重复加）"));
                    }
                    if !error.is_empty() {
                        message.push_str(&format!("；有文件失败：{error}"));
                    }
                    on_message.call(Some(message));
                } else {
                    on_message.call(Some(if error.is_empty() {
                        "没有可加入的图片".to_string()
                    } else {
                        format!("上传失败：{error}")
                    }));
                }

                busy.set(false);
                refresh.set(refresh() + 1);
            });
        }
    };

    // ---- 粘贴地址入池 ----
    let add_url = {
        let pool = pool.clone();
        move |_| {
            if busy() {
                return;
            }

            let raw = url_input().trim().to_string();
            if raw.is_empty() {
                on_message.call(Some("先粘贴一个图片地址".to_string()));
                return;
            }
            if !is_allowed_background(&raw) || raw.len() > BACKGROUND_URL_MAX {
                on_message.call(Some(
                    "地址需以 http:// 、https:// 或 / 开头（过长的地址也不收）".to_string(),
                ));
                return;
            }
            if pool.iter().any(|known| *known == raw) {
                on_message.call(Some("这一条已经在池里了".to_string()));
                return;
            }
            if full {
                on_message.call(Some(format!("池最多 {BACKGROUND_POOL_MAX} 张，先移出几张再加")));
                return;
            }

            on_pool.call(extend_pool(&pool, [raw.clone()]).0);
            on_pick.call(raw);
            url_input.set(String::new());
            on_message.call(Some("已加入池并设为当前背景".to_string()));
        }
    };

    // ---- 存到本地：外链抓一份回来，池里那条换成地址 ----
    let save_local = {
        let pool = pool.clone();
        let current = current.clone();
        move |url: String| {
            if busy() {
                return;
            }

            let pool = pool.clone();
            // `async move` 会把用到的东西都移进去，所以"是不是当前这张"先在外面算成 bool
            let was_current = current.as_str() == url.as_str();

            spawn(async move {
                busy.set(true);
                on_message.call(Some("正在抓取，最多等 8 秒…".to_string()));

                match save_wallpaper_from_url(url.clone()).await {
                    Ok(stored) => {
                        let next: Vec<String> = pool
                            .iter()
                            .map(|known| if *known == url { stored.clone() } else { known.clone() })
                            .collect();
                        on_pool.call(next);
                        if was_current {
                            on_pick.call(stored);
                        }
                        on_message.call(Some(
                            "已存到本机，池里那条已换成它——之后不再依赖对方站点".to_string(),
                        ));
                    }
                    Err(e) => on_message.call(Some(format!("存到本地失败：{}", error_text(&e)))),
                }

                busy.set(false);
                refresh.set(refresh() + 1);
            });
        }
    };

    // ---- 移出池（不动本机文件） ----
    let remove = {
        let pool = pool.clone();
        let current = current.clone();
        move |url: String| {
            if busy() {
                return;
            }

            let next: Vec<String> = pool.iter().filter(|known| known.as_str() != url.as_str()).cloned().collect();
            let was_current = current.as_str() == url.as_str();
            let fallback = next.first().cloned().unwrap_or_default();

            on_pool.call(next);
            if was_current {
                on_pick.call(fallback);
            }
            on_message.call(Some(if url.starts_with("http") {
                "已移出池".to_string()
            } else {
                "已移出池（文件还在本机，可以在下面的「本机文件」里加回来）".to_string()
            }));
        }
    };

    // ---- 删除本机文件（连人带池一起少一条） ----
    let delete_local = {
        let pool = pool.clone();
        let current = current.clone();
        move |url: String| {
            if busy() {
                return;
            }

            let next: Vec<String> = pool.iter().filter(|known| known.as_str() != url.as_str()).cloned().collect();
            let pool_len = pool.len();
            let was_current = current.as_str() == url.as_str();
            let fallback = next.first().cloned().unwrap_or_default();

            spawn(async move {
                busy.set(true);
                match delete_file(&url).await {
                    Ok(()) => {
                        if next.len() != pool_len {
                            on_pool.call(next);
                            if was_current {
                                on_pick.call(fallback);
                            }
                        }
                        on_message.call(Some(if was_current {
                            "已删除本机文件；它正是当前背景，已换成池里的另一张".to_string()
                        } else {
                            "已删除本机文件".to_string()
                        }));
                    }
                    Err(e) => on_message.call(Some(format!("删除失败：{e}"))),
                }
                busy.set(false);
                refresh.set(refresh() + 1);
            });
        }
    };

    // ---- 本机文件加回池 ----
    let adopt = {
        let pool = pool.clone();
        move |url: String| {
            if busy() {
                return;
            }
            if full {
                on_message.call(Some(format!("池最多 {BACKGROUND_POOL_MAX} 张，先移出几张再加")));
                return;
            }

            let (next, added) = extend_pool(&pool, [url.clone()]);
            if added == 0 {
                on_message.call(Some("这张已经在池里了".to_string()));
                return;
            }
            on_pool.call(next);
            on_pick.call(url);
            on_message.call(Some("已加入池并设为当前背景".to_string()));
        }
    };

    rsx! {
        section { class: "panel-section",
            h3 { class: "panel-heading", "背景池" }

            div { class: "inline-form",
                label {
                    class: "btn-ghost file-btn",
                    r#for: FILE_INPUT_ID,
                    if busy() { "处理中…" } else { "上传图片" }
                    input {
                        id: FILE_INPUT_ID,
                        class: "visually-hidden",
                        r#type: "file",
                        multiple: true,
                        accept: "image/png,image/jpeg,image/webp,image/gif,image/avif,image/bmp,image/svg+xml",
                        onchange: upload,
                    }
                }
                input {
                    class: "input",
                    placeholder: "或粘贴一个图片地址（也可直接填随机图接口）",
                    value: "{url_input}",
                    oninput: move |e| url_input.set(e.value()),
                }
                button {
                    class: "btn-ghost",
                    r#type: "button",
                    disabled: busy(),
                    onclick: add_url,
                    "添加"
                }
            }
            p { class: "field-hint",
                "png / jpg / webp / gif / avif / bmp / svg，单张上限 16 MB；一次可选多张，也可一次粘贴一条地址。"
            }

            if pool.is_empty() {
                p { class: "field-hint",
                    "池里还没有背景。上传图片或粘贴地址，也可以就用上面的内置渐变。"
                }
            } else {
                div { class: "wallpaper-list",
                    for url in pool.iter().cloned() {
                        {
                            let label = if url.starts_with("http") {
                                host_of(&url)
                            } else {
                                match files.iter().find(|file| url_of(&file.name) == url) {
                                    Some(file) => format!("本机 · {}", human_size(file.bytes)),
                                    None => "本机图片".to_string(),
                                }
                            };
                            rsx! {
                                PoolCard {
                                    key: "{url}",
                                    url: url.clone(),
                                    label,
                                    in_use: url.as_str() == current.as_str(),
                                    busy: busy(),
                                    on_pick,
                                    on_remove: remove.clone(),
                                    on_save_local: save_local.clone(),
                                    on_delete_file: delete_local.clone(),
                                }
                            }
                        }
                    }
                }
                p { class: "field-hint",
                    if rotating {
                        "「每次打开换一张」已开着：每次打开从池里随机挑一张。高亮的那张是关掉开关时用的。"
                    } else {
                        "点缩略图即设为当前背景。想让每次打开都不一样，去打开「每次打开换一张」。"
                    }
                }
            }

            // 本机文件兜底条带：只有存在"不在池里的文件"时才出现
            if !loose.is_empty() {
                div { class: "local-files",
                    button {
                        class: "local-files-head",
                        r#type: "button",
                        onclick: move |_| show_loose.set(!show_loose()),
                        if show_loose() { "▾ " } else { "▸ " }
                        "本机还有 {loose.len()} 张没在池里"
                    }
                    if show_loose() {
                        div { class: "wallpaper-list",
                            for file in loose {
                                LooseFileCard {
                                    key: "{file.name}",
                                    url: url_of(&file.name),
                                    label: human_size(file.bytes),
                                    busy: busy(),
                                    on_adopt: adopt.clone(),
                                    on_delete_file: delete_local.clone(),
                                }
                            }
                        }
                        p { class: "field-hint",
                            "这些文件在 WALLPAPER_DIR 里但没参与轮换（旧版本上传的、手动丢进目录的、"
                            "或你自己移出池的）。加回池才会出现在背景里；删掉才真的释放磁盘。"
                        }
                    }
                }
            }

            if files_count > 0 {
                p { class: "field-hint",
                    "本机壁纸共 {files_count} 张 · {human_size(files_bytes)}，存在 WALLPAPER_DIR"
                    "（默认 data/wallpaper）下，文件名是不透明哈希。它们不参与任何自动清理。"
                }
            }
        }
    }
}

/// 池里的一张背景。
///
/// 单独一个组件而不是在循环里内联：每张卡片要自己记"缩略图加载失败了"，
/// 而 hook 不能开在循环里。
#[component]
fn PoolCard(
    url: String,
    label: String,
    in_use: bool,
    busy: bool,
    on_pick: EventHandler<String>,
    on_remove: EventHandler<String>,
    on_save_local: EventHandler<String>,
    on_delete_file: EventHandler<String>,
) -> Element {
    let external = url.starts_with("http");

    rsx! {
        div { class: if in_use { "wallpaper-item active" } else { "wallpaper-item" },
            Thumb { url: url.clone() }
            span { class: "wallpaper-name", title: "{url}", "{label}" }
            div { class: "wallpaper-actions",
                button {
                    class: "icon-btn",
                    r#type: "button",
                    title: if in_use { "当前背景" } else { "设为当前背景" },
                    onclick: {
                        let url = url.clone();
                        move |_| on_pick.call(url.clone())
                    },
                    if in_use { "使用中" } else { "设为当前" }
                }
                if external {
                    button {
                        class: "icon-btn",
                        r#type: "button",
                        title: "抓一份到自己服务器上，之后不再依赖对方站点",
                        disabled: busy,
                        onclick: {
                            let url = url.clone();
                            move |_| on_save_local.call(url.clone())
                        },
                        "存本地"
                    }
                }
                button {
                    class: "icon-btn",
                    r#type: "button",
                    title: if external { "从池里移出" } else { "从池里移出，文件仍留在本机" },
                    disabled: busy,
                    onclick: {
                        let url = url.clone();
                        move |_| on_remove.call(url.clone())
                    },
                    "移出池"
                }
                if !external {
                    button {
                        class: "icon-btn danger",
                        r#type: "button",
                        title: "从磁盘删掉这张图（不可恢复）",
                        disabled: busy,
                        onclick: {
                            let url = url.clone();
                            move |_| on_delete_file.call(url.clone())
                        },
                        "删除"
                    }
                }
            }
        }
    }
}

/// 本机有、但不在池里的一张。
#[component]
fn LooseFileCard(
    url: String,
    label: String,
    busy: bool,
    on_adopt: EventHandler<String>,
    on_delete_file: EventHandler<String>,
) -> Element {
    rsx! {
        div { class: "wallpaper-item",
            Thumb { url: url.clone() }
            span { class: "wallpaper-name", title: "{url}", "{label}" }
            div { class: "wallpaper-actions",
                button {
                    class: "icon-btn",
                    r#type: "button",
                    title: "加入池，之后参与轮换",
                    disabled: busy,
                    onclick: {
                        let url = url.clone();
                        move |_| on_adopt.call(url.clone())
                    },
                    "加入池"
                }
                button {
                    class: "icon-btn danger",
                    r#type: "button",
                    title: "从磁盘删掉这张图（不可恢复）",
                    disabled: busy,
                    onclick: {
                        let url = url.clone();
                        move |_| on_delete_file.call(url.clone())
                    },
                    "删除"
                }
            }
        }
    }
}

/// 缩略图。加载失败就换成一行提示——壁纸是整屏大图，一张 404 的缩略图在网格里
/// 只会显示浏览器的碎图图标，看不出是"这条地址坏了"还是"面板坏了"。
///
/// 这里不需要 `CardGrid` 那套"hydration 后补查"：面板只在抽屉打开时挂载，
/// 图片都是客户端渲染的，`onerror` 在创建时就已挂好。
#[component]
fn Thumb(url: String) -> Element {
    let mut failed = use_signal(|| false);

    rsx! {
        div { class: "wallpaper-thumb-box",
            if failed() {
                span { class: "wallpaper-thumb-broken", "取不到" }
            } else {
                img {
                    class: "wallpaper-thumb",
                    src: "{url}",
                    alt: "",
                    loading: "lazy",
                    onerror: move |_| failed.set(true),
                }
            }
        }
    }
}
