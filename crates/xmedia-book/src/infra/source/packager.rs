pub use xmedia_formats::book::epub_write::{
    PackBookMetadata, PackChapterInput, package_chapters_to_txt,
};

use crate::application::ports::BookPackagerPort;

/// Packages online chapters into a normalized, Deflate-compressed EPUB 3.0 archive.
pub fn package_chapters_to_epub(
    meta: &PackBookMetadata,
    chapters: &[PackChapterInput],
) -> Result<Vec<u8>, String> {
    xmedia_formats::book::epub_write::package_chapters_to_epub(meta, chapters, None)
        .map_err(|e| e.to_string())
}

#[derive(Debug, Default, Clone, Copy)]
pub struct BookFormatPackager;

impl BookPackagerPort for BookFormatPackager {
    fn package_epub(
        &self,
        meta: &PackBookMetadata,
        chapters: &[PackChapterInput],
    ) -> Result<Vec<u8>, String> {
        package_chapters_to_epub(meta, chapters)
    }

    fn package_txt(&self, meta: &PackBookMetadata, chapters: &[PackChapterInput]) -> Vec<u8> {
        package_chapters_to_txt(meta, chapters)
    }
}
