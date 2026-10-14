use dioxus::prelude::*;

/// 壁纸管理（设置 → 外观里的一节）。
///
/// 三个操作都走 `/wallpaper` 那组路由。**请求发在 JS 里**（而不是 Rust 侧的 HTTP
/// 客户端）：这样不必为客户端再引一个 fetch 库。关键是 JS 里只调用 `dioxus.send`
/// 把结果发回来——**原生 JS 回调里不能碰 Dioxus 的运行时 API**（写 signal、spawn
/// 都会 panic，release 下整页死掉），Rust 侧的处理一律放回事件触发的 spawn 里。
const LIST_SCRIPT: &str = r#"
fetch('/wallpaper')
    .then((r) => r.json())
    .then((names) => dioxus.send({ ok: true, names }))
    .catch((e) => dioxus.send({ ok: false, error: String(e) }));
"#;

const UPLOAD_SCRIPT: &str = r#"
const input = document.getElementById('nekotab-wallpaper-file');
const file = input && input.files && input.files[0];
if (!file) {
    dioxus.send({ ok: false, error: "没有选择文件" });
} else {
    fetch('/wallpaper', { method: 'POST', headers: { 'Content-Type': file.type }, body: file })
        .then(async (r) => {
            const text = await r.text();
            if (r.ok) { dioxus.send({ ok: true, url: JSON.parse(text).url }); }
            else { dioxus.send({ ok: false, error: text || ('HTTP ' + r.status) }); }
        })
        .catch((e) => dioxus.send({ ok: false, error: String(e) }));
}
"#;

const DELETE_SCRIPT: &str = r#"
fetch('/wallpaper/' + encodeURIComponent(NAME), { method: 'DELETE' })
    .then((r) => dioxus.send({ ok: r.ok, error: r.ok ? '' : ('HTTP ' + r.status) }))
    .catch((e) => dioxus.send({ ok: false, error: String(e) }));
"#;

const FILE_INPUT_ID: &str = "nekotab-wallpaper-file";

/// 脚本回传的统一结果。
#[derive(serde::Deserialize)]
struct ScriptReply {
    ok: bool,
    #[serde(default)]
    names: Vec<String>,
    #[serde(default)]
    url: String,
    #[serde(default)]
    error: String,
}

/// 列出已有壁纸。
async fn list_wallpapers() -> Result<Vec<String>, String> {
    let reply = document::eval(LIST_SCRIPT)
        .recv::<ScriptReply>()
        .await
        .map_err(|e| e.to_string())?;
    if reply.ok {
        Ok(reply.names)
    } else {
        Err(reply.error)
    }
}

/// 删除一张壁纸；`url` 形如 `/wallpaper/xxxx.png`。
async fn delete_wallpaper(url: &str) -> Result<(), String> {
    let name = url.rsplit('/').next().unwrap_or("").to_string();
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

#[component]
pub fn WallpaperPanel(
    /// 当前的背景地址（用来标出"使用中"那张）
    current: String,
    /// 选为背景
    on_pick: EventHandler<String>,
    /// 状态提示（成功或失败都走这里）
    on_message: EventHandler<Option<String>>,
) -> Element {
    let mut refresh = use_signal(|| 0u32);
    let mut busy = use_signal(|| false);

    let list = use_resource(move || async move {
        let _ = refresh();
        list_wallpapers().await
    });

    let items: Vec<String> = match (&*list.read()).clone() {
        Some(Ok(names)) => names,
        _ => Vec::new(),
    };

    let upload = move |_| {
        if busy() {
            return;
        }

        spawn(async move {
            busy.set(true);
            on_message.call(None);

            let reply = document::eval(UPLOAD_SCRIPT).recv::<ScriptReply>().await;
            match reply {
                Ok(reply) if reply.ok => on_message.call(Some(format!(
                    "已上传 {}，点「设为背景」即可使用",
                    reply.url
                ))),
                Ok(reply) => on_message.call(Some(format!("上传失败：{}", reply.error))),
                Err(e) => on_message.call(Some(format!("上传失败：{e}"))),
            }

            busy.set(false);
            refresh.set(refresh() + 1);
        });
    };

    // `was_current` 由调用处算好传进来，而不是在闭包里读 `current`：闭包一旦
    // 捕获那个 String 就不再是 Copy，没法在 for 循环里反复用。
    let remove = move |url: String, was_current: bool| {
        // 删掉正在用的那张，就等于把背景指向一个已经不存在的文件——背景会变成
        // 一片空白而没有任何提示。索性顺手把背景地址清掉（清成空即回到内置渐变），
        // 仍在草稿里，用户点保存才落库。
        spawn(async move {
            busy.set(true);
            match delete_wallpaper(&url).await {
                Ok(()) => {
                    if was_current {
                        on_pick.call(String::new());
                    }
                    on_message.call(Some(if was_current {
                        "已删除（它正是当前背景，已改回内置渐变，记得保存设置）".to_string()
                    } else {
                        "已删除".to_string()
                    }));
                }
                Err(e) => on_message.call(Some(format!("删除失败：{e}"))),
            }
            busy.set(false);
            refresh.set(refresh() + 1);
        });
    };

    rsx! {
        section { class: "panel-section",
            h3 { class: "panel-heading", "壁纸" }

            div { class: "inline-form",
                label {
                    class: "btn-ghost file-btn",
                    r#for: FILE_INPUT_ID,
                    if busy() { "处理中…" } else { "上传图片" }
                    input {
                        id: FILE_INPUT_ID,
                        class: "visually-hidden",
                        r#type: "file",
                        accept: "image/png,image/jpeg,image/webp,image/gif,image/avif,image/bmp,image/svg+xml",
                        onchange: move |_| upload(()),
                    }
                }
                span { class: "field-hint", "png / jpg / webp / gif / avif / bmp / svg，单张上限 16 MB" }
            }

            if items.is_empty() {
                p { class: "field-hint", "还没有上传过壁纸。上传后会出现在这里，点一下即设为背景。" }
            } else {
                div { class: "wallpaper-list",
                    for name in items {
                        {
                            let url = format!("/wallpaper/{name}");
                            let in_use = current == url;
                            let pick_url = url.clone();
                            let del_url = url.clone();
                            rsx! {
                                div { key: "{name}", class: if in_use { "wallpaper-item active" } else { "wallpaper-item" },
                                    img { class: "wallpaper-thumb", src: "{url}", alt: "", loading: "lazy" }
                                    div { class: "wallpaper-actions",
                                        button {
                                            class: "icon-btn",
                                            r#type: "button",
                                            title: "设为背景",
                                            onclick: move |_| on_pick.call(pick_url.clone()),
                                            if in_use { "使用中" } else { "设为背景" }
                                        }
                                        button {
                                            class: "icon-btn danger",
                                            r#type: "button",
                                            title: "删除这张壁纸",
                                            disabled: busy(),
                                            onclick: {
                                                let url = del_url.clone();
                                                move |_| remove(url.clone(), in_use)
                                            },
                                            "删除"
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                p { class: "field-hint",
                    "壁纸存在 WALLPAPER_DIR（默认 data/wallpaper）下，文件名是不透明哈希。"
                    "删掉还在用的那张会让背景取不到，记得同时换一张。"
                }
            }
        }
    }
}
