pub mod online;
pub mod opds;
pub mod rest;
pub mod state;

use axum::Router;
use state::ComicHttpState;

pub fn rest_router(state: ComicHttpState) -> Router {
    rest::router().merge(online::router()).with_state(state)
}

pub fn opds_router(state: ComicHttpState) -> Router {
    opds::router().with_state(state)
}
