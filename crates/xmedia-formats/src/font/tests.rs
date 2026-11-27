use super::*;

struct NameRecordSpec<'a> {
    platform_id: u16,
    encoding_id: u16,
    language_id: u16,
    name_id: u16,
    bytes: &'a [u8],
}

fn encode_utf16be(text: &str) -> Vec<u8> {
    text.encode_utf16().flat_map(u16::to_be_bytes).collect()
}

fn build_synthetic_sfnt_with_names(magic: [u8; 4], records: &[NameRecordSpec<'_>]) -> Vec<u8> {
    let mut name_records = Vec::with_capacity(records.len() * 12);
    let mut string_pool = Vec::new();
    for r in records {
        let offset = string_pool.len() as u16;
        let len = r.bytes.len() as u16;
        name_records.extend_from_slice(&r.platform_id.to_be_bytes());
        name_records.extend_from_slice(&r.encoding_id.to_be_bytes());
        name_records.extend_from_slice(&r.language_id.to_be_bytes());
        name_records.extend_from_slice(&r.name_id.to_be_bytes());
        name_records.extend_from_slice(&len.to_be_bytes());
        name_records.extend_from_slice(&offset.to_be_bytes());
        string_pool.extend_from_slice(r.bytes);
    }

    let mut name_table = Vec::new();
    name_table.extend_from_slice(&0u16.to_be_bytes()); // format 0
    name_table.extend_from_slice(&(records.len() as u16).to_be_bytes());
    let string_offset = (6 + name_records.len()) as u16;
    name_table.extend_from_slice(&string_offset.to_be_bytes());
    name_table.extend_from_slice(&name_records);
    name_table.extend_from_slice(&string_pool);

    // sfnt header (12 bytes) + 1 table record (16 bytes) = offset 28
    let mut font = Vec::new();
    font.extend_from_slice(&magic);
    font.extend_from_slice(&1u16.to_be_bytes()); // numTables
    font.extend_from_slice(&16u16.to_be_bytes()); // searchRange
    font.extend_from_slice(&0u16.to_be_bytes()); // entrySelector
    font.extend_from_slice(&0u16.to_be_bytes()); // rangeShift

    font.extend_from_slice(b"name");
    font.extend_from_slice(&0u32.to_be_bytes()); // checkSum
    font.extend_from_slice(&28u32.to_be_bytes()); // offset
    font.extend_from_slice(&(name_table.len() as u32).to_be_bytes()); // length
    font.extend_from_slice(&name_table);
    font
}

#[test]
fn font_format_serde_and_metadata_helpers_match_wire_contract() {
    let cases = [
        (FontFormat::Ttf, "\"ttf\"", "ttf", "font/ttf", "truetype"),
        (FontFormat::Otf, "\"otf\"", "otf", "font/otf", "opentype"),
        (
            FontFormat::Ttc,
            "\"ttc\"",
            "ttc",
            "font/collection",
            "truetype",
        ),
        (FontFormat::Woff, "\"woff\"", "woff", "font/woff", "woff"),
        (
            FontFormat::Woff2,
            "\"woff2\"",
            "woff2",
            "font/woff2",
            "woff2",
        ),
    ];

    for (fmt, json_str, ext, mime, css) in cases {
        assert_eq!(serde_json::to_string(&fmt).unwrap(), json_str);
        assert_eq!(serde_json::from_str::<FontFormat>(json_str).unwrap(), fmt);
        assert_eq!(fmt.extension(), ext);
        assert_eq!(fmt.mime_type(), mime);
        assert_eq!(fmt.css_format(), css);
    }
}

#[test]
fn inspect_font_bytes_sniffs_all_valid_magic_headers_and_falls_back_to_stem() {
    let ttf_v1 = inspect_font_bytes("LXGWWenKai-Regular.ttf", &[0x00, 0x01, 0x00, 0x00, 0, 0])
        .expect("valid TTF 0x00010000");
    assert_eq!(ttf_v1.format, FontFormat::Ttf);
    assert_eq!(ttf_v1.family_name, "LXGWWenKai-Regular");

    let ttf_true = inspect_font_bytes("AppleFont.bin", b"true\x00\x00").expect("valid Apple TTF");
    assert_eq!(ttf_true.format, FontFormat::Ttf);
    assert_eq!(ttf_true.family_name, "AppleFont");

    let otf = inspect_font_bytes("SourceHanSerif-Bold.otf", b"OTTO\x00\x00").expect("valid OTF");
    assert_eq!(otf.format, FontFormat::Otf);
    assert_eq!(otf.family_name, "SourceHanSerif-Bold");

    let ttc = inspect_font_bytes("NotoSansCJK.ttc", b"ttcf\x00\x01\x00\x00").expect("valid TTC");
    assert_eq!(ttc.format, FontFormat::Ttc);
    assert_eq!(ttc.family_name, "NotoSansCJK");

    let woff =
        inspect_font_bytes("Inter-Regular.woff", b"wOFF\x00\x01\x00\x00").expect("valid WOFF");
    assert_eq!(woff.format, FontFormat::Woff);
    assert_eq!(woff.family_name, "Inter-Regular");

    let woff2 = inspect_font_bytes("C:\\Fonts\\JetBrainsMono.woff2", b"wOF2\x00\x01\x00\x00")
        .expect("valid WOFF2");
    assert_eq!(woff2.format, FontFormat::Woff2);
    assert_eq!(woff2.family_name, "JetBrainsMono");
}

#[test]
fn inspect_font_bytes_extracts_utf16be_and_mac_roman_names_preferring_id_16() {
    let fam_id1 = encode_utf16be("LXGW WenKai Regular");
    let fam_id16 = encode_utf16be("LXGW WenKai");
    let ttf = build_synthetic_sfnt_with_names(
        [0x00, 0x01, 0x00, 0x00],
        &[
            NameRecordSpec {
                platform_id: 3,
                encoding_id: 1,
                language_id: 0x0409,
                name_id: 1,
                bytes: &fam_id1,
            },
            NameRecordSpec {
                platform_id: 3,
                encoding_id: 1,
                language_id: 0x0409,
                name_id: 16,
                bytes: &fam_id16,
            },
        ],
    );

    let info = inspect_font_bytes("ugly-upload-123.ttf", &ttf).unwrap();
    assert_eq!(info.format, FontFormat::Ttf);
    assert_eq!(info.family_name, "LXGW WenKai");

    // Platform 0 (Unicode UTF-16BE) CJK family name
    let unicode_cjk = encode_utf16be("霞鹜文楷");
    let otf_unicode = build_synthetic_sfnt_with_names(
        *b"OTTO",
        &[NameRecordSpec {
            platform_id: 0,
            encoding_id: 3,
            language_id: 0,
            name_id: 1,
            bytes: &unicode_cjk,
        }],
    );
    let info_cjk = inspect_font_bytes("font.otf", &otf_unicode).unwrap();
    assert_eq!(info_cjk.format, FontFormat::Otf);
    assert_eq!(info_cjk.family_name, "霞鹜文楷");

    // Platform 1 (Mac Roman: 0x8E is 'é' in Mac Roman)
    let mac_roman = [b'C', b'a', b'f', 0x8E, b' ', b'S', b'e', b'r', b'i', b'f'];
    let otf_mac = build_synthetic_sfnt_with_names(
        *b"OTTO",
        &[NameRecordSpec {
            platform_id: 1,
            encoding_id: 0,
            language_id: 0,
            name_id: 1,
            bytes: &mac_roman,
        }],
    );
    let info_mac = inspect_font_bytes("cafe.otf", &otf_mac).unwrap();
    assert_eq!(info_mac.family_name, "Café Serif");
}

#[test]
fn inspect_font_bytes_handles_truncated_or_corrupt_tables_without_panicking() {
    let utf16_evil = encode_utf16be("SafeFamily;}{<>\"'\\");
    let full = build_synthetic_sfnt_with_names(
        [0x00, 0x01, 0x00, 0x00],
        &[NameRecordSpec {
            platform_id: 3,
            encoding_id: 1,
            language_id: 0x0409,
            name_id: 1,
            bytes: &utf16_evil,
        }],
    );
    let sanitized = inspect_font_bytes("fallback.ttf", &full).unwrap();
    assert_eq!(sanitized.family_name, "SafeFamily");

    // Truncate at every byte boundary >= 4 to ensure zero panics and clean fallback
    for cut in 4..full.len() {
        let res = inspect_font_bytes("FallbackStem.ttf", &full[..cut]).unwrap();
        assert_eq!(res.format, FontFormat::Ttf);
        assert!(!res.family_name.is_empty());
    }

    // Corrupt numTables = 65535 and bogus offsets
    let mut corrupt = vec![0x00, 0x01, 0x00, 0x00, 0xFF, 0xFF, 0, 0, 0, 0, 0, 0];
    corrupt.extend_from_slice(b"name\x00\x00\x00\x00\xFF\xFF\xFF\xFF\xFF\xFF\xFF\xFF");
    let recovered = inspect_font_bytes("CorruptFont.ttf", &corrupt).unwrap();
    assert_eq!(recovered.format, FontFormat::Ttf);
    assert_eq!(recovered.family_name, "CorruptFont");
}

#[test]
fn inspect_font_bytes_rejects_empty_and_disguised_html_or_png_payloads() {
    assert!(matches!(
        inspect_font_bytes("empty.ttf", b""),
        Err(FormatError::Validation(_))
    ));
    assert!(matches!(
        inspect_font_bytes("tiny.ttf", b"ab"),
        Err(FormatError::UnsupportedFormat(_))
    ));
    assert!(matches!(
        inspect_font_bytes("xss.ttf", b"<!DOCTYPE html><script>alert(1)</script>"),
        Err(FormatError::UnsupportedFormat(_))
    ));
    assert!(matches!(
        inspect_font_bytes("fake.woff2", b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR"),
        Err(FormatError::UnsupportedFormat(_))
    ));
}

#[test]
fn extract_font_families_from_reader_reads_ttc_faces_and_prefers_zh_cn() {
    use std::io::Cursor;

    let en_name = encode_utf16be("LXGW WenKai");
    let zh_name = encode_utf16be("霞鹜文楷");
    let face_a = build_synthetic_sfnt_with_names(
        [0x00, 0x01, 0x00, 0x00],
        &[
            NameRecordSpec {
                platform_id: 3,
                encoding_id: 1,
                language_id: 0x0409,
                name_id: 16,
                bytes: &en_name,
            },
            NameRecordSpec {
                platform_id: 3,
                encoding_id: 1,
                language_id: 0x0804,
                name_id: 16,
                bytes: &zh_name,
            },
        ],
    );

    let mono_name = encode_utf16be("JetBrains Mono");
    let face_b = build_synthetic_sfnt_with_names(
        *b"OTTO",
        &[NameRecordSpec {
            platform_id: 3,
            encoding_id: 1,
            language_id: 0x0409,
            name_id: 1,
            bytes: &mono_name,
        }],
    );

    // Build a 2-face TTC: 12-byte header + 2 * 4-byte offsets = 20 bytes
    let face_a_offset = 20u32;
    let face_b_offset = face_a_offset + face_a.len() as u32;

    let fixup_face = |raw: Vec<u8>, base_offset: u32| -> Vec<u8> {
        let mut out = raw;
        let orig_tbl_offset = u32::from_be_bytes([out[20], out[21], out[22], out[23]]);
        let new_tbl_offset = base_offset + orig_tbl_offset;
        out[20..24].copy_from_slice(&new_tbl_offset.to_be_bytes());
        out
    };

    let fixed_a = fixup_face(face_a, face_a_offset);
    let fixed_b = fixup_face(face_b, face_b_offset);

    let mut ttc = Vec::new();
    ttc.extend_from_slice(b"ttcf");
    ttc.extend_from_slice(&0x0001_0000u32.to_be_bytes());
    ttc.extend_from_slice(&2u32.to_be_bytes());
    ttc.extend_from_slice(&face_a_offset.to_be_bytes());
    ttc.extend_from_slice(&face_b_offset.to_be_bytes());
    ttc.extend_from_slice(&fixed_a);
    ttc.extend_from_slice(&fixed_b);

    let mut cursor = Cursor::new(ttc);
    let families = extract_font_families_from_reader(&mut cursor);
    assert_eq!(
        families,
        vec!["霞鹜文楷".to_string(), "JetBrains Mono".to_string()]
    );
}
