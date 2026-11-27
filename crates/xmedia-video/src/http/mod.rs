pub mod jellyfin;
pub mod online;
pub mod rest;
pub mod state;

use std::sync::Arc;

use axum::Router;
use axum::response::Response;
use xmedia_core::http::{
    into_range_http_response, into_stream_http_response, sanitize_download_filename,
};
use xmedia_source::crud::PluginRegistry;

use crate::application::streaming::ResolvedVideoStream;
pub use online::plugin_handler;
use state::VideoHttpState;

pub fn rest_router(state: VideoHttpState) -> Router {
    rest::router().merge(online::router()).with_state(state)
}

pub fn plugins_router(state: &VideoHttpState) -> Router {
    xmedia_source::crud::plugins_router(
        state.auth.clone(),
        Arc::new(PluginRegistry::single(online::plugin_handler(
            state.services.online.clone(),
        ))),
    )
}

pub fn jellyfin_router(state: VideoHttpState) -> Router {
    jellyfin::router().with_state(state)
}

pub(crate) fn into_video_http_response(
    resolved: ResolvedVideoStream,
    as_attachment: bool,
) -> Response {
    match resolved {
        ResolvedVideoStream::DirectRange {
            video,
            range_stream,
            is_partial,
        } => {
            let filename = sanitize_download_filename(&video.title, video.container.as_ref());
            into_range_http_response(
                range_stream,
                is_partial,
                video.container.mime_type(),
                Some(&filename),
                as_attachment,
                Some(video.duration_secs),
            )
        }
        ResolvedVideoStream::TranscodedFmp4 { video, stream } => {
            let filename = sanitize_download_filename(&video.title, "mp4");
            into_stream_http_response(
                stream,
                "video/mp4",
                Some(&filename),
                as_attachment,
                Some(video.duration_secs),
                None,
            )
        }
    }
}
