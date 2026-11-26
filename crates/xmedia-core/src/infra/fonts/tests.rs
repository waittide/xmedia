use tempfile::TempDir;
use xmedia_formats::font::FontFormat;

use super::*;

#[tokio::test]
async fn fs_font_store_add_list_open_persist_and_delete_roundtrip() {
    let tmp = TempDir::new().unwrap();
    let fonts_dir = tmp.path().join("fonts");
    let store = FsFontStore::new(&fonts_dir);

    assert!(store.list().await.unwrap().is_empty());

    let woff2_bytes = b"wOF2\x00\x01\x00\x00synthetic-woff2-payload";
    let added = store
        .add("LXGWWenKai-Regular.woff2", None, woff2_bytes)
        .await
        .unwrap();
    assert_eq!(added.name, "LXGWWenKai-Regular");
    assert_eq!(added.file_name, "LXGWWenKai-Regular.woff2");
    assert_eq!(added.format, FontFormat::Woff2);
    assert_eq!(added.css_format, "woff2");
    assert_eq!(added.mime_type, "font/woff2");
    assert_eq!(added.size_bytes, woff2_bytes.len() as u64);
    assert!(added.created_at.parse::<u64>().is_ok());

    let custom_named = store
        .add(
            "custom.ttf",
            Some("  霞鹜文楷;{}<>\"'\\  "),
            &[0x00, 0x01, 0x00, 0x00, 1, 2, 3, 4],
        )
        .await
        .unwrap();
    assert_eq!(custom_named.name, "霞鹜文楷");
    assert_eq!(custom_named.format, FontFormat::Ttf);
    assert_eq!(custom_named.css_format, "truetype");
    assert_eq!(custom_named.mime_type, "font/ttf");

    // Reopen from disk to verify manifest.json persistence
    let reopened = FsFontStore::new(&fonts_dir);
    let listed = reopened.list().await.unwrap();
    assert_eq!(listed.len(), 2);
    assert_eq!(listed[0], added);
    assert_eq!(listed[1], custom_named);

    let (meta, raw) = reopened.open_file(added.id).await.unwrap();
    assert_eq!(meta, added);
    assert_eq!(raw, woff2_bytes);

    reopened.delete(added.id).await.unwrap();
    assert!(matches!(
        reopened.open_file(added.id).await,
        Err(DomainError::NotFound(_))
    ));
    assert!(matches!(
        reopened.delete(added.id).await,
        Err(DomainError::NotFound(_))
    ));
    assert!(!fonts_dir.join(format!("{}.woff2", added.id)).exists());
    assert_eq!(reopened.list().await.unwrap(), vec![custom_named]);
}

#[tokio::test]
async fn fs_font_store_rejects_empty_and_invalid_magic_payloads() {
    let tmp = TempDir::new().unwrap();
    let store = FsFontStore::new(tmp.path().join("fonts"));

    assert!(matches!(
        store.add("empty.ttf", None, b"").await,
        Err(DomainError::Validation(_))
    ));
    assert!(matches!(
        store
            .add("disguised.ttf", None, b"<html>not a font</html>")
            .await,
        Err(DomainError::UnsupportedFormat(_))
    ));
    assert!(store.list().await.unwrap().is_empty());
}
