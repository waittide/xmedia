use xmedia_core::domain::library::MediaKind;

use crate::domain::media::{BookFileMeta, MediaMetadataPatch, MediaMetadataPayload, MusicFileMeta};
use crate::infra::inspectors::zh_text::ZhConvertDirection;

use super::{
    BatchMetadataEditInput, BatchTextReplaceRule, build_batch_patch_for_payload,
    payload_to_restore_patch,
};

#[test]
fn batch_edit_supports_regex_watermark_removal_tags_and_zh_convert() {
    let payload = MediaMetadataPayload::Book(BookFileMeta {
        title: "【XX下載站】三體全集".into(),
        authors: vec!["劉慈欣".into()],
        tags: vec!["舊標籤".into(), "科幻".into()],
        ..Default::default()
    });
    let input = BatchMetadataEditInput {
        media_kind: MediaKind::Book,
        add_tags: vec!["雨果奖".into()],
        remove_tags: vec!["舊標籤".into()],
        replace_rule: Some(BatchTextReplaceRule {
            find: r"【[^】]+】".into(),
            replace: String::new(),
            use_regex: true,
            fields: vec!["title".into()],
        }),
        zh_direction: Some(ZhConvertDirection::T2s),
        ..Default::default()
    };

    let patch = build_batch_patch_for_payload(&payload, "santi", 1, &input)
        .unwrap()
        .expect("should produce book patch");
    let MediaMetadataPatch::Book(bp) = patch else {
        panic!("expected Book patch");
    };
    assert_eq!(bp.title.as_deref(), Some("三体全集"));
    assert_eq!(bp.authors, Some(vec!["刘慈欣".to_string()]));
    assert_eq!(
        bp.tags,
        Some(vec!["科幻".to_string(), "雨果奖".to_string()])
    );
}

#[test]
fn batch_edit_supports_title_template_variables_and_lossless_restore() {
    let payload = MediaMetadataPayload::Music(MusicFileMeta {
        title: "Track01".into(),
        artist: "周杰伦".into(),
        album: "范特西".into(),
        year: Some(2001),
        ..Default::default()
    });
    let input = BatchMetadataEditInput {
        media_kind: MediaKind::Music,
        title_template: Some("${counter:2}. ${filename} - ${creator}".into()),
        ..Default::default()
    };
    let patch = build_batch_patch_for_payload(&payload, "爱在西元前", 3, &input)
        .unwrap()
        .expect("should produce music patch");
    let MediaMetadataPatch::Music(mp) = patch else {
        panic!("expected Music patch");
    };
    assert_eq!(mp.title.as_deref(), Some("03. 爱在西元前 - 周杰伦"));

    let restore = payload_to_restore_patch(&payload);
    let MediaMetadataPatch::Music(rp) = restore else {
        panic!("expected Music restore patch");
    };
    assert_eq!(rp.title.as_deref(), Some("Track01"));
    assert_eq!(rp.artist.as_deref(), Some("周杰伦"));
    assert_eq!(rp.year, Some(2001));
}
