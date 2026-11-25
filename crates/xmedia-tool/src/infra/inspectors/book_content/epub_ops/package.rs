use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};

use quick_xml::escape::escape;
use uuid::Uuid;
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_formats::{decode_text_smart, sniff_image_mime};
use zip::ZipWriter;

use crate::domain::media::BookFileMeta;
use crate::infra::atomic_fs::atomic_write_via_temp;
use crate::infra::inspectors::book::{
    find_sidecar_cover, read_book_meta_any, replace_opf_metadata_block,
};

use super::super::epub_io::{
    ManifestNode, epub_deflated_zip_options, epub_entry_zip_options, epub_stored_zip_options,
    ext_for_image_mime, rebuild_opf_manifest_and_spine, render_nav_xhtml, render_toc_ncx,
    write_zip_entry,
};
use super::super::txt::segment_txt_chapters;
use super::super::xhtml::{
    plain_to_xhtml_body, render_book_jacket_xhtml, sanitize_xhtml_fragment,
    wrap_full_xhtml_document,
};
use super::super::{ChapterEditMode, DEFAULT_EPUB_CSS, SaveBookChapterInput};

pub(crate) struct FreshEpubChapter {
    pub title: String,
    pub level: u8,
    pub body_xhtml: String,
}

pub(crate) struct FreshEpubImage {
    pub rel_href: String,
    pub media_type: &'static str,
    pub bytes: Vec<u8>,
}

pub(crate) struct FreshEpubBuildOptions<'a> {
    pub css: &'a str,
    pub generate_jacket: bool,
    pub mark_normalized: bool,
    pub best_compression: bool,
    pub cover_wrapper_class: &'a str,
    pub cover_alt: &'a str,
}

const DEFAULT_CONTAINER_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>
"#;

pub(crate) fn write_fresh_epub_package(
    epub_path: &Path,
    meta: &BookFileMeta,
    chapters: &[FreshEpubChapter],
    extra_images: &[FreshEpubImage],
    cover: Option<&(Vec<u8>, &'static str, &'static str)>,
    opts: &FreshEpubBuildOptions<'_>,
) -> DomainResult<()> {
    let lang = meta.language.as_deref().unwrap_or("zh-CN");
    let book_uuid = format!("urn:uuid:{}", Uuid::now_v7());

    atomic_write_via_temp(epub_path, |staging| {
        let out_file = File::create(staging)
            .map_err(|e| DomainError::Storage(format!("failed creating epub staging: {e}")))?;
        let mut writer = ZipWriter::new(BufWriter::new(out_file));

        let stored = epub_stored_zip_options();
        let deflated = epub_deflated_zip_options(opts.best_compression);

        write_zip_entry(
            &mut writer,
            "mimetype",
            stored,
            b"application/epub+zip",
            "mimetype",
        )?;

        write_zip_entry(
            &mut writer,
            "META-INF/container.xml",
            deflated,
            DEFAULT_CONTAINER_XML.as_bytes(),
            "container",
        )?;

        write_zip_entry(
            &mut writer,
            "OEBPS/Styles/style.css",
            deflated,
            opts.css.as_bytes(),
            "css",
        )?;

        let mut manifest_nodes = vec![
            ManifestNode {
                id: "style-css".to_string(),
                href: "Styles/style.css".to_string(),
                full_path: "OEBPS/Styles/style.css".to_string(),
                media_type: "text/css".to_string(),
                properties: None,
            },
            ManifestNode {
                id: "nav".to_string(),
                href: "nav.xhtml".to_string(),
                full_path: "OEBPS/nav.xhtml".to_string(),
                media_type: "application/xhtml+xml".to_string(),
                properties: Some("nav".to_string()),
            },
            ManifestNode {
                id: "ncx".to_string(),
                href: "toc.ncx".to_string(),
                full_path: "OEBPS/toc.ncx".to_string(),
                media_type: "application/x-dtbncx+xml".to_string(),
                properties: None,
            },
        ];
        let mut spine_ids = Vec::new();
        let mut toc_triples: Vec<(String, String, u8)> = Vec::with_capacity(chapters.len() + 1);

        let mut cover_item_id: Option<&str> = None;
        let mut cover_rel_for_jacket: Option<String> = None;
        if let Some((cov_bytes, cov_mime, cov_ext)) = cover {
            let img_rel = format!("Images/cover.{cov_ext}");
            let img_full = format!("OEBPS/{img_rel}");
            write_zip_entry(
                &mut writer,
                &img_full,
                epub_entry_zip_options(&img_full, cov_mime, opts.best_compression),
                cov_bytes,
                "cover image",
            )?;

            let cover_xhtml = wrap_full_xhtml_document(
                "封面",
                lang,
                None,
                Some("../Styles/style.css"),
                &format!(
                    "  <div class=\"{}\"><img src=\"../{img_rel}\" alt=\"{}\"/></div>",
                    opts.cover_wrapper_class,
                    escape(opts.cover_alt)
                ),
            );
            write_zip_entry(
                &mut writer,
                "OEBPS/Text/cover.xhtml",
                deflated,
                cover_xhtml.as_bytes(),
                "cover xhtml",
            )?;

            manifest_nodes.push(ManifestNode {
                id: "cover-image".to_string(),
                href: img_rel.clone(),
                full_path: img_full,
                media_type: (*cov_mime).to_string(),
                properties: Some("cover-image".to_string()),
            });
            manifest_nodes.push(ManifestNode {
                id: "cover-page".to_string(),
                href: "Text/cover.xhtml".to_string(),
                full_path: "OEBPS/Text/cover.xhtml".to_string(),
                media_type: "application/xhtml+xml".to_string(),
                properties: None,
            });
            spine_ids.push("cover-page".to_string());
            cover_item_id = Some("cover-image");
            cover_rel_for_jacket = Some(format!("../{img_rel}"));
        }

        if opts.generate_jacket {
            let jacket_xhtml = render_book_jacket_xhtml(
                meta,
                cover_rel_for_jacket.as_deref(),
                Some("../Styles/style.css"),
            );
            write_zip_entry(
                &mut writer,
                "OEBPS/Text/xmedia_jacket.xhtml",
                deflated,
                jacket_xhtml.as_bytes(),
                "jacket",
            )?;

            manifest_nodes.push(ManifestNode {
                id: "xmedia-jacket".to_string(),
                href: "Text/xmedia_jacket.xhtml".to_string(),
                full_path: "OEBPS/Text/xmedia_jacket.xhtml".to_string(),
                media_type: "application/xhtml+xml".to_string(),
                properties: None,
            });
            spine_ids.push("xmedia-jacket".to_string());
            toc_triples.push((
                "书籍简介".to_string(),
                "OEBPS/Text/xmedia_jacket.xhtml".to_string(),
                0,
            ));
        }

        for (idx, asset) in extra_images.iter().enumerate() {
            let full_path = format!("OEBPS/{}", asset.rel_href);
            write_zip_entry(
                &mut writer,
                &full_path,
                epub_entry_zip_options(&full_path, asset.media_type, opts.best_compression),
                &asset.bytes,
                "image",
            )?;
            manifest_nodes.push(ManifestNode {
                id: format!("img-{:04}", idx + 1),
                href: asset.rel_href.clone(),
                full_path,
                media_type: asset.media_type.to_string(),
                properties: None,
            });
        }

        for (idx, ch) in chapters.iter().enumerate() {
            let seq = idx + 1;
            let title = if ch.title.trim().is_empty() {
                format!("第 {seq} 章")
            } else {
                ch.title.trim().to_string()
            };
            let level = ch.level.min(1);
            let rel_href = format!("Text/chapter_{seq:04}.xhtml");
            let full_path = format!("OEBPS/{rel_href}");
            let item_id = format!("ch-{seq:04}");

            let doc = wrap_full_xhtml_document(
                &title,
                lang,
                None,
                Some("../Styles/style.css"),
                &ch.body_xhtml,
            );
            write_zip_entry(&mut writer, &full_path, deflated, doc.as_bytes(), "chapter")?;

            manifest_nodes.push(ManifestNode {
                id: item_id.clone(),
                href: rel_href,
                full_path: full_path.clone(),
                media_type: "application/xhtml+xml".to_string(),
                properties: None,
            });
            spine_ids.push(item_id);
            toc_triples.push((title, full_path, level));
        }

        let oebps_dir = Path::new("OEBPS");
        let nav_xml = render_nav_xhtml(&meta.title, lang, &toc_triples, oebps_dir);
        write_zip_entry(
            &mut writer,
            "OEBPS/nav.xhtml",
            deflated,
            nav_xml.as_bytes(),
            "nav",
        )?;

        let ncx_xml = render_toc_ncx(&book_uuid, &meta.title, &toc_triples, oebps_dir);
        write_zip_entry(
            &mut writer,
            "OEBPS/toc.ncx",
            deflated,
            ncx_xml.as_bytes(),
            "ncx",
        )?;

        let base_opf = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<package xmlns=\"http://www.idpf.org/2007/opf\" version=\"3.0\" unique-identifier=\"bookid\">\n  <metadata xmlns:dc=\"http://purl.org/dc/elements/1.1/\">\n    <dc:identifier id=\"bookid\">{}</dc:identifier>\n  </metadata>\n  <manifest></manifest>\n  <spine toc=\"ncx\"></spine>\n</package>\n",
            escape(&book_uuid)
        );
        let mut effective_meta = meta.clone();
        if opts.mark_normalized {
            effective_meta.is_normalized = true;
        }
        let with_meta =
            replace_opf_metadata_block(&base_opf, &effective_meta, cover_item_id, "bookid", "3.0");
        let final_opf = rebuild_opf_manifest_and_spine(
            &with_meta,
            &manifest_nodes,
            &spine_ids,
            "ncx",
            None,
            &effective_meta,
            "bookid",
            "3.0",
        );

        write_zip_entry(
            &mut writer,
            "OEBPS/content.opf",
            deflated,
            final_opf.as_bytes(),
            "opf",
        )?;

        writer
            .finish()
            .map_err(|e| DomainError::Storage(format!("epub zip finalize error: {e}")))?;
        Ok(())
    })
}

pub fn convert_txt_to_epub(
    txt_path: &Path,
    chapters_override: Option<&[SaveBookChapterInput]>,
    stylesheets_override: Option<&[String]>,
    delete_source_txt: bool,
) -> DomainResult<PathBuf> {
    let ext = txt_path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if ext != "txt" {
        return Err(DomainError::Validation(
            "only TXT files can be converted to EPUB".into(),
        ));
    }

    let epub_path = txt_path.with_extension("epub");
    if epub_path.exists() {
        return Err(DomainError::Conflict(format!(
            "目标文件 '{}' 已存在，请先重命名或移除已有同名 EPUB",
            epub_path
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default()
        )));
    }

    let meta = read_book_meta_any(txt_path)?;

    let resolved_chapters: Vec<SaveBookChapterInput> = if let Some(list) = chapters_override {
        if list.is_empty() {
            return Err(DomainError::Validation(
                "至少需要包含一个章节才能生成 EPUB".into(),
            ));
        }
        list.to_vec()
    } else {
        let bytes = std::fs::read(txt_path)
            .map_err(|e| DomainError::Storage(format!("failed reading txt: {e}")))?;
        let (decoded, _) = decode_text_smart(&bytes);
        segment_txt_chapters(&decoded, None, None)?
            .into_iter()
            .map(|c| SaveBookChapterInput {
                title: c.title,
                level: c.level,
                mode: ChapterEditMode::Plain,
                plain_text: c.plain_text,
                xhtml_body: c.xhtml_body,
                source_href: None,
            })
            .collect()
    };

    let cover_payload: Option<(Vec<u8>, &'static str, &'static str)> = find_sidecar_cover(txt_path)
        .and_then(|cov_path| {
            let bytes = std::fs::read(cov_path).ok()?;
            if bytes.is_empty() {
                return None;
            }
            let mime = sniff_image_mime(&bytes);
            Some((bytes, mime, ext_for_image_mime(mime)))
        });

    let chosen_css = stylesheets_override
        .and_then(|s| s.first())
        .map(String::as_str)
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(DEFAULT_EPUB_CSS);

    let fresh_chapters: Vec<FreshEpubChapter> = resolved_chapters
        .iter()
        .enumerate()
        .map(|(idx, ch)| {
            let seq = idx + 1;
            let title = if ch.title.trim().is_empty() {
                format!("第 {seq} 章")
            } else {
                ch.title.trim().to_string()
            };
            let body_xhtml = match ch.mode {
                ChapterEditMode::Plain => plain_to_xhtml_body(&title, &ch.plain_text),
                ChapterEditMode::Xhtml => {
                    if ch.xhtml_body.trim().is_empty() {
                        plain_to_xhtml_body(&title, &ch.plain_text)
                    } else {
                        sanitize_xhtml_fragment(&ch.xhtml_body)
                    }
                }
            };
            FreshEpubChapter {
                title,
                level: ch.level,
                body_xhtml,
            }
        })
        .collect();

    write_fresh_epub_package(
        &epub_path,
        &meta,
        &fresh_chapters,
        &[],
        cover_payload.as_ref(),
        &FreshEpubBuildOptions {
            css: chosen_css,
            generate_jacket: false,
            mark_normalized: false,
            best_compression: false,
            cover_wrapper_class: "cover-wrap",
            cover_alt: "Cover",
        },
    )?;

    if delete_source_txt {
        let _ = std::fs::remove_file(txt_path);
    }

    Ok(epub_path)
}
