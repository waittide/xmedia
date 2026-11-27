pub mod online;
pub mod opds;
pub mod rest;
pub mod state;

use std::sync::Arc;

use axum::Router;
use state::ComicHttpState;
use xmedia_source::crud::PluginRegistry;

pub use online::plugin_handler;

pub fn rest_router(state: ComicHttpState) -> Router {
    rest::router().merge(online::router()).with_state(state)
}

pub fn plugins_router(state: &ComicHttpState) -> Router {
    xmedia_source::crud::plugins_router(
        state.auth.clone(),
        Arc::new(PluginRegistry::single(online::plugin_handler(
            state.services.online.clone(),
        ))),
    )
}

pub fn opds_router(state: ComicHttpState) -> Router {
    opds::router().with_state(state)
}
