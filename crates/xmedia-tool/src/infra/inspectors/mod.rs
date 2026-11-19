pub mod book;
pub mod book_content;
pub mod comic;
pub mod comic_ops;
pub mod music;
pub mod video;
pub mod zh_text;

use std::sync::Arc;

use xmedia_core::infra::ffmpeg::FfmpegToolkit;

use crate::domain::ports::MediaInspectorPort;

pub fn build_default_inspectors(ffmpeg: FfmpegToolkit) -> Vec<Arc<dyn MediaInspectorPort>> {
    vec![
        Arc::new(book::EpubBookInspector),
        Arc::new(comic::CbzComicInspector),
        Arc::new(music::LoftyMusicInspector),
        Arc::new(video::VideoNfoInspector::new(ffmpeg)),
    ]
}

pub(crate) fn append_xml_ref(out: &mut String, r: &quick_xml::events::BytesRef<'_>) {
    if let Ok(Some(ch)) = r.resolve_char_ref() {
        out.push(ch);
        return;
    }
    if let Some(resolved) = quick_xml::escape::resolve_xml_entity(r.as_ref()) {
        out.push_str(resolved);
    } else {
        out.push('&');
        out.push_str(r.as_ref());
        out.push(';');
    }
}
