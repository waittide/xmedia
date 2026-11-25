use std::io::Cursor;

use encoding_rs::GB18030;
use lofty::file::{AudioFile, TaggedFileExt};
use lofty::picture::{MimeType, Picture, PictureType};
use lofty::tag::{ItemKey, TagType};
use xmedia_formats::music::{
    AudioFormatExt, AudioMetaPatch, ReplayGainInfo, apply_audio_patch_in_place,
    ensure_mutable_primary_tag, extract_embedded_lyrics_from_path, open_tagged_path,
    parse_lrc_bytes, probe_audio_path, read_embedded_cover_from_path, resolve_hybrid_lyrics,
    shift_and_clean_lrc_text, write_embedded_cover_in_place,
};
use xmedia_formats::video::{
    NfoRootKind, VideoFormatExt, VideoNfoMeta, convert_ass_to_vtt, convert_srt_to_vtt,
    convert_subtitle_bytes_to_vtt, parse_nfo_xml, parse_sidecar_subtitle_lang_and_label,
    parse_video_path, render_season_nfo_xml_preserving, render_tvshow_nfo_xml_preserving,
    render_video_nfo_xml_preserving, shift_webvtt_timestamps,
};

fn make_wav(samples: u32) -> Vec<u8> {
    let sample_rate: u32 = 8000;
    let byte_rate = sample_rate * 2;
    let data_size = samples * 2;
    let riff_size = 36 + data_size;

    let mut buf = Vec::with_capacity((riff_size + 8) as usize);
    buf.extend_from_slice(b"RIFF");
    buf.extend_from_slice(&riff_size.to_le_bytes());
    buf.extend_from_slice(b"WAVEfmt ");
    buf.extend_from_slice(&16u32.to_le_bytes());
    buf.extend_from_slice(&1u16.to_le_bytes());
    buf.extend_from_slice(&1u16.to_le_bytes());
    buf.extend_from_slice(&sample_rate.to_le_bytes());
    buf.extend_from_slice(&byte_rate.to_le_bytes());
    buf.extend_from_slice(&2u16.to_le_bytes());
    buf.extend_from_slice(&16u16.to_le_bytes());
    buf.extend_from_slice(b"data");
    buf.extend_from_slice(&data_size.to_le_bytes());
    buf.resize((riff_size + 8) as usize, 0);
    buf
}

fn make_png(width: u32, height: u32) -> Vec<u8> {
    let img = image::RgbImage::from_pixel(width, height, image::Rgb([90, 120, 200]));
    let mut buf = Vec::new();
    img.write_to(&mut Cursor::new(&mut buf), image::ImageFormat::Png)
        .expect("encode test png");
    buf
}

#[test]
fn audio_format_extensions_cover_all_canonical_formats() {
    for (ext, expected) in [
        ("flac", AudioFormatExt::Flac),
        ("mp3", AudioFormatExt::Mp3),
        ("m4a", AudioFormatExt::M4a),
        ("alac", AudioFormatExt::Alac),
        ("aac", AudioFormatExt::Aac),
        ("ogg", AudioFormatExt::Ogg),
        ("opus", AudioFormatExt::Opus),
        ("wav", AudioFormatExt::Wav),
        ("aiff", AudioFormatExt::Aiff),
        ("aif", AudioFormatExt::Aiff),
        ("ape", AudioFormatExt::Ape),
        ("wv", AudioFormatExt::Wv),
    ] {
        assert_eq!(AudioFormatExt::from_extension(ext), Some(expected));
    }
    assert!(AudioFormatExt::Flac.is_lossless());
    assert!(AudioFormatExt::Wv.is_lossless());
    assert!(!AudioFormatExt::Mp3.is_lossless());
}

#[test]
fn lofty_audio_patch_roundtrips_id3v2_uslt_replaygain_compilation_and_preserves_back_cover() {
    let tmp = tempfile::tempdir().unwrap();
    let wav_path = tmp.path().join("track01.wav");
    std::fs::write(&wav_path, make_wav(8000)).unwrap();

    // Seed a CoverBack image first to prove CoverFront replacement preserves non-front pictures
    let back_png = make_png(3, 3);
    {
        let mut tf = open_tagged_path(&wav_path).unwrap();
        let tag = ensure_mutable_primary_tag(&mut tf);
        assert_eq!(tag.tag_type(), TagType::Id3v2);
        tag.push_picture(
            Picture::unchecked(back_png.clone())
                .pic_type(PictureType::CoverBack)
                .mime_type(MimeType::Png)
                .build(),
        );
        tf.save_to_path(&wav_path, lofty::config::WriteOptions::default())
            .unwrap();
    }

    let front_png = make_png(4, 4);
    write_embedded_cover_in_place(&wav_path, &front_png, "image/png").unwrap();

    let patch = AudioMetaPatch {
        title: Some("晴天".into()),
        artist: Some("周杰伦".into()),
        album_artist: None,
        album: Some("叶惠美".into()),
        track_number: Some(3),
        disc_number: Some(1),
        year: Some(2003),
        genre: Some("Mandopop".into()),
        comment: Some("Remastered".into()),
        is_compilation: Some(true),
        embedded_lyrics: Some("[00:01.50]故事的小黄花\n[00:05.00]从出生那年就飘着".into()),
        replay_gain: Some(ReplayGainInfo {
            track_gain_db: Some(-6.25),
            track_peak: Some(0.912345),
            album_gain_db: Some(-5.5),
            album_peak: Some(0.98),
        }),
    };
    apply_audio_patch_in_place(&wav_path, &patch).unwrap();

    // Verify ID3v2 specifically stored USLT under `ItemKey::UnsyncLyrics`
    let raw_tf = open_tagged_path(&wav_path).unwrap();
    let id3 = raw_tf.tag(TagType::Id3v2).expect("ID3v2 tag on WAV");
    assert!(id3.get_string(ItemKey::UnsyncLyrics).is_some());
    assert_eq!(id3.pictures().len(), 2);

    let probed = probe_audio_path(&wav_path, "wav", "Fallback").unwrap();
    assert_eq!(probed.format, AudioFormatExt::Wav);
    assert_eq!(probed.title, "晴天");
    assert_eq!(probed.track_artist, "周杰伦");
    assert!(probed.is_compilation);
    assert_eq!(probed.effective_album_artist, "Various Artists");
    assert_eq!(probed.album_title, "叶惠美");
    assert_eq!(probed.track_number, Some(3));
    assert_eq!(probed.year, Some(2003));
    assert!(probed.has_embedded_lyrics);
    assert!((probed.replay_gain.track_gain_db.unwrap() - (-6.25)).abs() < 0.02);

    let extracted_cover = read_embedded_cover_from_path(&wav_path).unwrap().unwrap();
    assert_eq!(extracted_cover.bytes, front_png);

    let embedded_lrc = extract_embedded_lyrics_from_path(&wav_path).unwrap();
    assert!(embedded_lrc.synced);
    assert_eq!(embedded_lrc.lines.len(), 2);
    assert_eq!(embedded_lrc.lines[0].start_ms, 1500);
    assert_eq!(embedded_lrc.lines[0].text, "故事的小黄花");
}

#[test]
fn lrc_parser_handles_gb18030_synced_plain_and_credit_stripping() {
    let raw_lrc = "[ar:王菲]\n[ti:红豆]\n[offset:-200]\n[00:12.34][01:12.34]还没好好的感受\n";
    let (gbk_bytes, _, _) = GB18030.encode(raw_lrc);
    let parsed = parse_lrc_bytes(&gbk_bytes);
    assert!(parsed.synced);
    assert_eq!(parsed.display_artist.as_deref(), Some("王菲"));
    assert_eq!(parsed.display_title.as_deref(), Some("红豆"));
    assert_eq!(parsed.offset_ms, -200);
    assert_eq!(parsed.lines.len(), 2);
    assert_eq!(parsed.lines[0].start_ms, 12_340);
    assert_eq!(parsed.lines[1].start_ms, 72_340);

    let plain = resolve_hybrid_lyrics(None, Some("第一行纯文本\n第二行纯文本".as_bytes()));
    assert!(!plain.synced);
    assert!(plain.plain_text.contains("第二行纯文本"));

    let (shifted, removed) =
        shift_and_clean_lrc_text("[00:01.00]作词：林夕\n[00:10.50]雪花绽放的气候", -500, true);
    assert_eq!(removed, 1);
    assert_eq!(shifted, "[00:10.00]雪花绽放的气候");
}

#[test]
fn video_nfo_non_destructive_roundtrip_preserves_streamdetails_actors_uniqueids_and_seasons() {
    let movie_xml = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<movie>
  <title>The Matrix &amp; Reloaded &#x2605;</title>
  <year>1999</year>
  <premiered>1999-03-31</premiered>
  <mpaa>Rated R</mpaa>
  <country>United States</country>
  <uniqueid type="imdb" default="true">tt0133093</uniqueid>
  <uniqueid type="tvdb">221</uniqueid>
  <uniqueid type="tmdb">603</uniqueid>
  <actor>
    <name>Keanu Reeves</name>
    <role>Neo</role>
    <thumb>https://img.example/neo.jpg</thumb>
  </actor>
  <fileinfo>
    <streamdetails>
      <video>
        <codec>hevc</codec>
        <width>3840</width>
        <height>2160</height>
      </video>
    </streamdetails>
  </fileinfo>
  <customtag>keep-value</customtag>
</movie>"#;

    let parsed = parse_nfo_xml(movie_xml).expect("valid movie nfo");
    assert_eq!(parsed.root_kind, NfoRootKind::Movie);
    assert_eq!(parsed.title.as_deref(), Some("The Matrix & Reloaded ★"));
    assert_eq!(parsed.premiered.as_deref(), Some("1999-03-31"));
    assert_eq!(parsed.mpaa.as_deref(), Some("Rated R"));
    assert_eq!(parsed.country.as_deref(), Some("United States"));
    assert_eq!(parsed.actor_details[0].role.as_deref(), Some("Neo"));
    assert_eq!(parsed.width, Some(3840));

    // Caller with partial metadata (`premiered`/`mpaa`/`country`/`width` unset) still preserves everything
    let minimal_patch = VideoNfoMeta {
        is_episode: false,
        title: Some("黑客帝国".into()),
        year: Some(1999),
        actors: vec!["Keanu Reeves".into(), "Carrie-Anne Moss".into()],
        tmdb_id: Some("603".into()),
        ..Default::default()
    };
    let rendered = render_video_nfo_xml_preserving(&minimal_patch, Some(movie_xml));
    assert!(rendered.contains("<title>黑客帝国</title>"));
    assert!(rendered.contains("<premiered>1999-03-31</premiered>"));
    assert!(rendered.contains("<mpaa>Rated R</mpaa>"));
    assert!(rendered.contains("<country>United States</country>"));
    assert!(rendered.contains("<uniqueid type=\"tmdb\" default=\"true\">603</uniqueid>"));
    assert!(rendered.contains("<uniqueid type=\"imdb\">tt0133093</uniqueid>"));
    assert!(rendered.contains("<role>Neo</role>"));
    assert!(rendered.contains("<thumb>https://img.example/neo.jpg</thumb>"));
    assert!(rendered.contains("<name>Carrie-Anne Moss</name>"));
    assert!(rendered.contains("<codec>hevc</codec>"));
    assert!(rendered.contains("<customtag>keep-value</customtag>"));

    // Episode & Season NFOs accept `<seasonnumber>` and emit both `<season>` + `<seasonnumber>`
    let ep_meta = VideoNfoMeta {
        is_episode: true,
        title: Some("Journey's End".into()),
        show_title: Some("Frieren".into()),
        season_number: Some(1),
        episode_number: Some(28),
        ..Default::default()
    };
    let ep_xml = render_video_nfo_xml_preserving(&ep_meta, None);
    assert!(ep_xml.contains("<season>1</season>"));
    assert!(ep_xml.contains("<seasonnumber>1</seasonnumber>"));

    let season_xml = render_season_nfo_xml_preserving(
        2,
        Some("第 2 季"),
        Some(2026),
        Some("新篇章"),
        Some("209867"),
        None,
        Some(b"<season><premiered>2026-01-10</premiered></season>"),
    );
    assert!(season_xml.contains("<season>2</season>"));
    assert!(season_xml.contains("<seasonnumber>2</seasonnumber>"));
    assert!(season_xml.contains("<premiered>2026-01-10</premiered>"));
    let parsed_season = parse_nfo_xml(season_xml.as_bytes()).unwrap();
    assert_eq!(parsed_season.root_kind, NfoRootKind::Season);
    assert_eq!(parsed_season.season_number, Some(2));
    assert_eq!(parsed_season.season_title.as_deref(), Some("第 2 季"));

    let tvshow_xml = render_tvshow_nfo_xml_preserving(&ep_meta, Some("奇幻冒险"), None);
    let parsed_show = parse_nfo_xml(tvshow_xml.as_bytes()).unwrap();
    assert_eq!(parsed_show.root_kind, NfoRootKind::TvShow);
    assert_eq!(parsed_show.show_title.as_deref(), Some("Frieren"));
}

#[test]
fn subtitle_converters_and_video_filename_parser_handle_sxxexx_year_and_resolution() {
    let srt = "1\n00:00:05,250 --> 00:00:08,500\n你好，世界\n";
    let vtt = convert_srt_to_vtt(srt);
    assert!(vtt.starts_with("WEBVTT\n\n"));
    assert!(vtt.contains("00:00:05.250 --> 00:00:08.500"));

    let shifted = shift_webvtt_timestamps(&vtt, 5.0);
    assert!(shifted.contains("00:00:00.250 --> 00:00:03.500"));

    let ass = "[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:01:02.34,0:01:05.60,Default,,0,0,0,,{\\pos(320,50)}第一行\\N\\N第二行\n";
    let ass_vtt = convert_ass_to_vtt(ass);
    assert!(ass_vtt.contains("00:01:02.340 --> 00:01:05.600\n第一行\n第二行\n\n"));

    let (gbk_srt, _, _) = GB18030.encode("1\n00:00:01,000 --> 00:00:02,000\n中文编码字幕\n");
    let decoded_vtt = convert_subtitle_bytes_to_vtt("srt", &gbk_srt);
    assert!(decoded_vtt.contains("中文编码字幕"));

    let ep = parse_video_path("[SubGroup] Frieren.2023.S01E03.Magic.1080p.WEB-DL.x265.mkv");
    assert_eq!(
        VideoFormatExt::from_extension("mkv"),
        Some(VideoFormatExt::Mkv)
    );
    assert!(ep.is_episode);
    assert_eq!(ep.show_title.as_deref(), Some("Frieren"));
    assert_eq!(ep.season_number, Some(1));
    assert_eq!(ep.episode_number, Some(3));
    assert_eq!(ep.title, "Magic");
    assert_eq!(ep.year, Some(2023));
    assert_eq!(ep.resolution.as_deref(), Some("1080p"));

    let movie = parse_video_path("Dune.Part.Two.2024.UHD.BluRay.2160p.HEVC.mp4");
    assert!(!movie.is_episode);
    assert_eq!(movie.title, "Dune Part Two");
    assert_eq!(movie.year, Some(2024));
    assert_eq!(movie.resolution.as_deref(), Some("2160p"));

    let (lang, label) = parse_sidecar_subtitle_lang_and_label("沙丘2", "沙丘2.chs.srt");
    assert_eq!(lang.as_deref(), Some("zh-CN"));
    assert_eq!(label, "简体中文");
}
