use std::fs::File;
use std::io::{Cursor, Read, Write};
use std::path::Path;

use image::{ImageBuffer, ImageFormat, Rgb};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use super::*;
use crate::infra::inspectors::comic::read_comic_meta;

fn make_png_bytes(w: u32, h: u32, color: [u8; 3]) -> Vec<u8> {
    let img: ImageBuffer<Rgb<u8>, Vec<u8>> = ImageBuffer::from_fn(w, h, |_, _| Rgb(color));
    let mut cursor = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(img)
        .write_to(&mut cursor, ImageFormat::Png)
        .unwrap();
    cursor.into_inner()
}

fn create_sample_cbz(path: &Path, pages: &[(&str, Vec<u8>)]) {
    let f = File::create(path).unwrap();
    let mut zw = ZipWriter::new(f);
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    zw.start_file("__MACOSX/._junk", stored).unwrap();
    zw.write_all(b"junk").unwrap();
    zw.start_file("credits.txt", stored).unwrap();
    zw.write_all(b"Translation notes").unwrap();
    for (name, bytes) in pages {
        zw.start_file(*name, stored).unwrap();
        zw.write_all(bytes).unwrap();
    }
    zw.finish().unwrap();
}

#[test]
fn comic_pages_list_reorder_delete_double_page_and_merge_work() {
    let tmp = tempfile::tempdir().unwrap();
    let cbz1 = tmp.path().join("vol1.cbz");
    let cbz2 = tmp.path().join("vol2.cbz");

    create_sample_cbz(
        &cbz1,
        &[
            ("p1_ad.png", make_png_bytes(400, 600, [255, 0, 0])),
            ("p2_cover.png", make_png_bytes(400, 600, [0, 255, 0])),
            ("p3_spread.png", make_png_bytes(900, 600, [0, 0, 255])),
        ],
    );
    create_sample_cbz(
        &cbz2,
        &[("p1.png", make_png_bytes(400, 600, [120, 120, 120]))],
    );

    let snap = list_comic_pages(&cbz1).unwrap();
    assert_eq!(snap.total_pages, 3);
    assert!(!snap.pages[0].double_page);
    assert!(snap.pages[2].double_page);

    let report = save_comic_pages(
        &cbz1,
        &[
            SaveComicPageInput {
                entry_name: "p2_cover.png".into(),
                page_type: "FrontCover".into(),
                double_page: false,
            },
            SaveComicPageInput {
                entry_name: "p3_spread.png".into(),
                page_type: "Story".into(),
                double_page: true,
            },
        ],
        ComicImageReencodeFormat::Jpeg,
        82,
        None,
    )
    .unwrap();
    assert_eq!(report.page_count, 2);
    assert_eq!(report.removed_junk_entries, 1);
    {
        let rf = File::open(&cbz1).unwrap();
        let mut za = ZipArchive::new(rf).unwrap();
        assert!(za.by_name("__MACOSX/._junk").is_err());
        let mut notes = String::new();
        za.by_name("credits.txt")
            .unwrap()
            .read_to_string(&mut notes)
            .unwrap();
        assert_eq!(notes, "Translation notes");
    }

    let snap2 = list_comic_pages(&cbz1).unwrap();
    assert_eq!(snap2.total_pages, 2);
    assert_eq!(snap2.pages[0].page_type, "FrontCover");
    assert!(snap2.pages[1].double_page);

    let merged = tmp.path().join("omnibus.cbz");
    let merge_rep = merge_comics_to_cbz(
        &[cbz1.clone(), cbz2.clone()],
        &merged,
        "omnibus.cbz".into(),
        Some("测试漫画".into()),
        Some("合订本".into()),
        Some(1.5),
    )
    .unwrap();
    assert_eq!(merge_rep.total_pages, 3);
    let merged_meta = read_comic_meta(&merged).unwrap();
    assert_eq!(merged_meta.series.as_deref(), Some("测试漫画"));
    assert_eq!(merged_meta.volume, Some(1.5));
    assert_eq!(merged_meta.page_count, 3);
}

#[test]
fn comic_image_directory_pages_list_and_read_work() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("迷宫饭 第01话");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("001.png"), make_png_bytes(400, 600, [10, 20, 30])).unwrap();
    std::fs::write(dir.join("002.png"), make_png_bytes(900, 600, [30, 20, 10])).unwrap();

    let snap = list_comic_pages(&dir).unwrap();
    assert_eq!(snap.format, "folder");
    assert_eq!(snap.total_pages, 2);
    assert!(!snap.pages[0].double_page);
    assert!(snap.pages[1].double_page);

    let img = read_comic_page_image(&dir, "001.png", None).unwrap();
    assert_eq!(img.mime_type, "image/png");
    assert!(read_comic_page_image(&dir, "../001.png", None).is_err());
}
