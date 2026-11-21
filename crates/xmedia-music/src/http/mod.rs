pub mod online;
pub mod rest;
pub mod state;
pub mod subsonic;

use axum::Router;
use axum::response::Response;
use xmedia_core::http::{
    into_range_http_response, into_stream_http_response, sanitize_download_filename,
};

use crate::application::streaming::ResolvedAudioStream;
use state::MusicHttpState;

pub fn rest_router(state: MusicHttpState) -> Router {
    rest::router().merge(online::router()).with_state(state)
}

pub fn subsonic_router(state: MusicHttpState) -> Router {
    subsonic::router().with_state(state)
}

pub(crate) fn into_audio_http_response(
    resolved: ResolvedAudioStream,
    as_attachment: bool,
) -> Response {
    match resolved {
        ResolvedAudioStream::RawRange {
            track,
            range_stream,
            is_partial,
        } => {
            let filename = sanitize_download_filename(&track.title, track.format.extension());
            into_range_http_response(
                range_stream,
                is_partial,
                track.format.mime_type(),
                Some(&filename),
                as_attachment,
                Some(track.duration_secs),
            )
        }
        ResolvedAudioStream::Transcoded { track, transcoded } => {
            let filename = sanitize_download_filename(&track.title, &transcoded.content_extension);
            into_stream_http_response(
                transcoded.stream,
                &transcoded.mime_type,
                Some(&filename),
                as_attachment,
                Some(track.duration_secs),
                transcoded.estimated_content_length,
            )
        }
    }
}
