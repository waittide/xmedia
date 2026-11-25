use xmedia_core::domain::error::DomainError;
use xmedia_formats::music::probe_audio_path;

use super::*;
use crate::infra::inspectors::zh_text::ZhConvertDirection;

#[test]
fn lrc_transform_shifts_timestamps_strips_credits_and_converts_zh() {
    let raw = "[00:01.00]作词：方文山\n[00:02.50]QQ音乐独家首发\n[00:10.50]故事的小黃花\n[00:00.20]從出生那年就飄著";
    let (out, removed) = transform_lrc_text(raw, -500, true, Some(ZhConvertDirection::T2s));
    assert_eq!(removed, 2);
    assert_eq!(out, "[00:10.00]故事的小黄花\n[00:00.00]从出生那年就飘着");
}

#[test]
fn cue_parser_extracts_album_and_track_timestamps() {
    let cue = r#"PERFORMER "周杰伦"
TITLE "叶惠美"
FILE "YeHuiMei.flac" WAVE
  TRACK 01 AUDIO
    TITLE "以父之名"
    PERFORMER "周杰伦"
    INDEX 01 00:00:00
  TRACK 02 AUDIO
    TITLE "晴天"
    INDEX 01 05:42:37
"#;
    let parsed = parse_cue_sheet(cue.as_bytes()).unwrap();
    assert_eq!(parsed.album_title.as_deref(), Some("叶惠美"));
    assert_eq!(parsed.album_performer.as_deref(), Some("周杰伦"));
    assert_eq!(parsed.audio_file, "YeHuiMei.flac");
    assert_eq!(parsed.tracks.len(), 2);
    assert_eq!(parsed.tracks[1].title, "晴天");
    assert!((parsed.tracks[1].start_secs - (5.0 * 60.0 + 42.0 + 37.0 / 75.0)).abs() < 0.01);
}

#[test]
fn split_cue_album_rejects_audio_file_path_traversal() {
    let tmp = tempfile::tempdir().unwrap();
    let outside = tmp.path().join("secret.flac");
    std::fs::write(&outside, b"secret").unwrap();
    let album_dir = tmp.path().join("album");
    std::fs::create_dir_all(&album_dir).unwrap();

    let traversal_cue = album_dir.join("evil.cue");
    std::fs::write(
        &traversal_cue,
        "FILE \"../secret.flac\" WAVE\n  TRACK 01 AUDIO\n    TITLE \"T1\"\n    INDEX 01 00:00:00\n",
    )
    .unwrap();
    let err = split_cue_album(&traversal_cue, "ffmpeg").unwrap_err();
    assert!(matches!(err, DomainError::Validation(_)));

    let abs_cue = album_dir.join("abs.cue");
    std::fs::write(
        &abs_cue,
        format!(
            "FILE \"{}\" WAVE\n  TRACK 01 AUDIO\n    TITLE \"T1\"\n    INDEX 01 00:00:00\n",
            outside.display()
        ),
    )
    .unwrap();
    let err_abs = split_cue_album(&abs_cue, "ffmpeg").unwrap_err();
    assert!(matches!(err_abs, DomainError::Validation(_)));
}

#[test]
fn process_music_lyrics_persists_id3v2_unsync_lyrics_in_wav() {
    let tmp = tempfile::tempdir().unwrap();
    let wav_path = tmp.path().join("song.wav");
    let mut wav = Vec::with_capacity(48);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&40u32.to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&8000u32.to_le_bytes());
    wav.extend_from_slice(&16000u32.to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&4u32.to_le_bytes());
    wav.extend_from_slice(&[0u8; 4]);
    std::fs::write(&wav_path, &wav).unwrap();

    let report = process_music_lyrics(
        &wav_path,
        &MusicLyricsToolInput {
            offset_ms: 0,
            strip_credit_noise: false,
            zh_direction: None,
            sync_mode: "both".to_string(),
            custom_text: Some("[00:05.00]晴天测试歌词".to_string()),
        },
    )
    .unwrap();
    assert!(report.updated_embedded);
    assert!(report.updated_sidecar);

    let reread = probe_audio_path(&wav_path, "wav", "song").unwrap();
    assert_eq!(
        reread.embedded_lyrics.as_deref(),
        Some("[00:05.00]晴天测试歌词")
    );
}
