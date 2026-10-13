use dioxus::prelude::*;

mod backend;
mod components;
#[cfg(feature = "server")]
mod db;
mod dom;
mod engine;
mod favicon;
// 农历（含第几天/第几周）只被客户端时钟用到，服务端编译时整体未被引用
#[cfg_attr(not(feature = "web"), allow(dead_code))]
mod lunar;
mod models;
mod pages;
mod routes;
// 导入解析 / 导出序列化只在服务端进行，客户端只负责传文本与展示回执
#[cfg(feature = "server")]
mod transfer;

use routes::Route;

const MAIN_CSS: Asset = asset!("/assets/main.css");
const FAVICON_SVG: Asset = asset!("/assets/favicon.svg");

/// 浏览器主题色（移动端地址栏）
const THEME_COLOR: &str = "#0b0d13";

#[component]
fn App() -> Element {
    rsx! {
        document::Stylesheet { href: MAIN_CSS }

        // 站点自身的图标。之前 head 里什么都没有，标签页显示的是浏览器默认图标。
        document::Link { rel: "icon", r#type: "image/svg+xml", href: FAVICON_SVG.to_string() }
        document::Meta { name: "theme-color", content: THEME_COLOR }
        document::Meta { name: "description", content: "极简自部署书签导航" }

        Router::<Route> {}
    }
}

/// 导出全部书签为 JSON 备份（按当前显示顺序）。
///
/// 走普通 axum 路由而不是 server function：这样浏览器直接以文件形式下载，
/// 客户端不需要任何 Blob / 下载相关的 wasm 胶水代码，也能用 curl 直接验证。
#[cfg(feature = "server")]
async fn export_bookmarks() -> impl dioxus::server::axum::response::IntoResponse {
    use dioxus::server::axum::http::header;

    let body = backend::export_bookmarks_json().await;

    (
        [
            (header::CONTENT_TYPE, "application/json; charset=utf-8"),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=\"nekotab-bookmarks.json\"",
            ),
        ],
        body,
    )
}

fn main() {
    // 服务端：先初始化数据库连接池，再把 axum router 交给 dioxus 启动。
    #[cfg(feature = "server")]
    {
        dioxus::serve(|| async move {
            let _ = dotenvy::dotenv();
            let database_url = std::env::var("DATABASE_URL")
                .unwrap_or_else(|_| "sqlite://data/mtab.db?mode=rwc".to_string());

            db::init_pool(&database_url)
                .await
                .expect("初始化数据库失败");

            // 图标字节写在磁盘上（`ICON_DIR`，默认 data/icon），库表只留元数据。
            // 建不出来也不致命：代取到的图标会直接回给浏览器，只是不落缓存。
            match favicon::init_icon_dir() {
                Ok(dir) => println!("[NekoTab] 图标目录: {}", dir.display()),
                Err(e) => eprintln!("[NekoTab] 图标目录创建失败，图标将不落盘: {e}"),
            }

            // 手工注册的路由优先于 SSR 兜底
            let router = dioxus::server::router(App)
                .route(
                    "/export/bookmarks.json",
                    dioxus::server::axum::routing::get(export_bookmarks),
                )
                .route(
                    "/icon/{host}",
                    dioxus::server::axum::routing::get(favicon::icon_handler),
                )
                // 响应压缩，gzip 与 brotli 都开。
                //
                // 静态产物（wasm / js / css）已在构建期压成 .br（见 Dioxus.toml 的
                // pre_compress，质量 11），dioxus-server 的 ServeFile::precompressed_br()
                // 直接发那份、运行时零 CPU；本层不会重复压它们——响应已带
                // content-encoding，压缩层会跳过。
                //
                // 这里负责的是动态响应：SSR 的 HTML、server function 的 JSON。
                // 质量显式设 6：tower-http 给实时 brotli 的默认是 4（沿用 nginx 的
                // 取舍），抬到 6 多花的 CPU 可忽略；gzip 默认本来就是 6，不受影响。
                .layer(tower_http::compression::CompressionLayer::new().quality(
                    tower_http::compression::CompressionLevel::Precise(6),
                ));

            Ok(router)
        });
    }

    // 客户端（wasm）：hydrate 已有的 SSR 输出。
    #[cfg(not(feature = "server"))]
    {
        // 先装 panic 钩子再启动：release 下 panic 只会变成 unreachable，
        // 没有钩子的话控制台上什么都看不到
        dom::install_panic_hook();
        dioxus::launch(App);
    }
}
