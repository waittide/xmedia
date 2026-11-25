pub use xmedia_formats::book::epub_write::{
    PackBookMetadata, PackChapterInput, package_chapters_to_txt,
};

/// Packages online chapters into a normalized, Deflate-compressed EPUB 3.0 archive.
pub fn package_chapters_to_epub(
    meta: &PackBookMetadata,
    chapters: &[PackChapterInput],
) -> Result<Vec<u8>, String> {
    xmedia_formats::book::epub_write::package_chapters_to_epub(meta, chapters, None)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Read};
    use zip::ZipArchive;

    use super::*;

    #[test]
    fn epub_packager_maps_raster_cover_extensions_and_skips_svg() {
        let svg_meta = PackBookMetadata {
            identifier: None,
            title: "Test".into(),
            author: "Auth".into(),
            description: None,
            cover_bytes: Some((b"<svg/>".to_vec(), "image/svg+xml".into())),
        };
        let epub_no_svg = package_chapters_to_epub(&svg_meta, &[]).unwrap();
        let mut archive = ZipArchive::new(Cursor::new(epub_no_svg)).unwrap();
        let mut opf = String::new();
        archive
            .by_name("OEBPS/content.opf")
            .unwrap()
            .read_to_string(&mut opf)
            .unwrap();
        assert!(!opf.contains("cover-image"));
        assert!(!opf.contains("image/svg+xml"));

        let avif_meta = PackBookMetadata {
            identifier: None,
            title: "AVIF".into(),
            author: "Auth".into(),
            description: None,
            cover_bytes: Some((
                b"\x00\x00\x00\x1cftypavif".to_vec(),
                "image/avif; charset=binary".into(),
            )),
        };
        let epub_avif = package_chapters_to_epub(&avif_meta, &[]).unwrap();
        let mut archive2 = ZipArchive::new(Cursor::new(epub_avif)).unwrap();
        assert!(archive2.by_name("OEBPS/cover.avif").is_ok());
        let mut opf2 = String::new();
        archive2
            .by_name("OEBPS/content.opf")
            .unwrap()
            .read_to_string(&mut opf2)
            .unwrap();
        assert!(opf2.contains(r#"href="cover.avif" media-type="image/avif""#));
    }
}
