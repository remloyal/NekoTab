use dioxus::prelude::*;

use crate::pages::Home;

/// 本期只有主页一个路由。
#[derive(Routable, Clone, PartialEq, Debug)]
pub enum Route {
    #[route("/")]
    Home {},
}
