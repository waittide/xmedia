pub mod online;
pub(crate) mod opds_common;
pub mod opds_v1;
pub mod opds_v2;
pub mod rest;
pub mod state;

use axum::Router;
use state::BookHttpState;

pub fn rest_router(state: BookHttpState) -> Router {
    rest::router().merge(online::router()).with_state(state)
}

pub fn opds_router(state: BookHttpState) -> Router {
    Router::new()
        .merge(opds_v1::router())
        .merge(opds_v2::router())
        .with_state(state)
}
