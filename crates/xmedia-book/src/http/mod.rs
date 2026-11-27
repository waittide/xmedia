pub mod online;
pub(crate) mod opds_common;
pub mod opds_v1;
pub mod opds_v2;
pub mod rest;
pub mod state;

use std::sync::Arc;

use axum::Router;
use state::BookHttpState;
use xmedia_source::crud::PluginRegistry;

pub use online::plugin_handler;

pub fn rest_router(state: BookHttpState) -> Router {
    rest::router().merge(online::router()).with_state(state)
}

pub fn plugins_router(state: &BookHttpState) -> Router {
    xmedia_source::crud::plugins_router(
        state.auth.clone(),
        Arc::new(PluginRegistry::single(online::plugin_handler(
            state.services.online.clone(),
        ))),
    )
}

pub fn opds_router(state: BookHttpState) -> Router {
    Router::new()
        .merge(opds_v1::router())
        .merge(opds_v2::router())
        .with_state(state)
}
