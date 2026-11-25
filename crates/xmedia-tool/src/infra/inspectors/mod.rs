pub mod book;
pub mod book_content;
pub mod book_normalize;
pub mod comic;
pub mod comic_ops;
pub mod music;
pub mod music_ops;
pub mod process;
pub mod video;
pub mod video_ops;
pub mod zh_text;

use std::sync::Arc;

use xmedia_core::domain::error::DomainError;
use xmedia_core::infra::ffmpeg::FfmpegToolkit;
use xmedia_formats::FormatError;

use crate::domain::ports::MediaInspectorPort;

pub(crate) use xmedia_formats::common::append_xml_ref;

pub fn build_default_inspectors(ffmpeg: FfmpegToolkit) -> Vec<Arc<dyn MediaInspectorPort>> {
    vec![
        Arc::new(book::EpubBookInspector),
        Arc::new(comic::CbzComicInspector),
        Arc::new(music::LoftyMusicInspector),
        Arc::new(video::VideoNfoInspector::new(ffmpeg)),
    ]
}

pub(crate) fn map_format_err(err: FormatError) -> DomainError {
    match err {
        FormatError::Validation(msg) => DomainError::Validation(msg),
        FormatError::UnsupportedFormat(msg) => DomainError::UnsupportedFormat(msg),
        FormatError::NotFound(msg) => DomainError::NotFound(msg),
        FormatError::Conflict(msg) => DomainError::Conflict(msg),
        FormatError::Io(msg) => DomainError::Storage(msg),
    }
}
