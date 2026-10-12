use dioxus::prelude::*;

mod backend;
mod components;
#[cfg(feature = "server")]
mod db;
mod engine;
mod favicon;
mod models;
mod pages;
mod routes;
// 导入解析 / 导出序列化只在服务端进行，客户端只负责传文本与展示回执
#[cfg(feature = "server")]
mod transfer;

use routes::Route;

const MAIN_CSS: Asset = asset!("/assets/main.css");

#[component]
fn App() -> Element {
    rsx! {
        document::Stylesheet { href: MAIN_CSS }
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
                "attachment; filename=\"mtab-bookmarks.json\"",
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

            // 手工注册的路由优先于 SSR 兜底
            let router = dioxus::server::router(App)
                .route(
                    "/export/bookmarks.json",
                    dioxus::server::axum::routing::get(export_bookmarks),
                )
                .route(
                    "/icon/{host}",
                    dioxus::server::axum::routing::get(favicon::icon_handler),
                );

            Ok(router)
        });
    }

    // 客户端（wasm）：hydrate 已有的 SSR 输出。
    #[cfg(not(feature = "server"))]
    dioxus::launch(App);
}
