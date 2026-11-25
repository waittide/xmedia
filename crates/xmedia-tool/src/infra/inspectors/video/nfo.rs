use std::path::Path;

use xmedia_formats::video::{
    NfoAudioStream, NfoRootKind, NfoSubtitleStream, VideoNfoMeta, parse_nfo_into as fmt_parse_nfo,
    render_tvshow_nfo_xml_preserving as fmt_render_tvshow_nfo,
    render_video_nfo_xml_preserving as fmt_render_video_nfo,
};
pub(crate) use xmedia_formats::video::{
    find_existing_nfo, find_show_nfo_for_episode, render_season_nfo_xml_preserving,
};

use super::artwork::{find_clearlogo_path, find_fanart_path, find_poster_path, find_thumb_path};
use super::filename::{guess_video_from_path, list_sidecar_subtitles};
use crate::domain::media::{VideoFileMeta, VideoMetaPatch};

fn file_meta_to_nfo_meta(meta: &VideoFileMeta) -> VideoNfoMeta {
    VideoNfoMeta {
        root_kind: if meta.is_episode {
            NfoRootKind::EpisodeDetails
        } else {
            NfoRootKind::Movie
        },
        is_episode: meta.is_episode,
        title: Some(meta.title.clone()),
        raw_title: Some(meta.title.clone()),
        season_title: None,
        original_title: meta.original_title.clone(),
        show_title: meta.show_title.clone(),
        season_number: meta.season_number,
        episode_number: meta.episode_number,
        year: meta.year,
        premiered: None,
        mpaa: None,
        country: None,
        rating: meta.rating,
        summary: meta.summary.clone(),
        outline: None,
        studio: meta.studio.clone(),
        genres: meta.genres.clone(),
        tags: meta.tags.clone(),
        directors: meta.directors.clone(),
        actors: meta.actors.clone(),
        actor_details: Vec::new(),
        unique_ids: Vec::new(),
        tmdb_id: meta.tmdb_id.clone(),
        bangumi_id: meta.bangumi_id.clone(),
        collection_set: meta.collection_set.clone(),
        locked_fields: meta.locked_fields.clone(),
        video_codec: meta.video_codec.clone(),
        width: meta.width,
        height: meta.height,
        duration_secs: (meta.duration_secs > 0).then_some(meta.duration_secs),
        audio_streams: meta
            .audio_streams
            .iter()
            .map(|a| NfoAudioStream {
                codec: a.codec.clone(),
                language: a.language.clone(),
                channels: u32::from(a.channels),
            })
            .collect(),
        subtitle_streams: meta
            .subtitle_streams
            .iter()
            .map(|s| NfoSubtitleStream {
                language: s.language.clone(),
            })
            .collect(),
    }
}

pub(super) fn parse_nfo_into(xml_bytes: &[u8], meta: &mut VideoFileMeta) {
    let mut nfo = VideoNfoMeta::default();
    if !fmt_parse_nfo(xml_bytes, &mut nfo) {
        return;
    }
    if matches!(nfo.root_kind, NfoRootKind::EpisodeDetails) {
        meta.is_episode = true;
    } else if matches!(nfo.root_kind, NfoRootKind::Movie) {
        meta.is_episode = false;
    }
    if let Some(t) = nfo
        .title
        .or(nfo.raw_title)
        .or(nfo.season_title)
        .filter(|s| !s.trim().is_empty())
    {
        meta.title = t;
    }
    if nfo.original_title.is_some() {
        meta.original_title = nfo.original_title;
    }
    if nfo.show_title.is_some() {
        meta.show_title = nfo.show_title;
    }
    if nfo.season_number.is_some() {
        meta.season_number = nfo.season_number;
    }
    if nfo.episode_number.is_some() {
        meta.episode_number = nfo.episode_number;
    }
    if nfo.year.is_some() {
        meta.year = nfo.year;
    }
    if nfo.rating.is_some() {
        meta.rating = nfo.rating;
    }
    if let Some(plot) = nfo.summary.or(nfo.outline) {
        meta.summary = Some(plot);
    }
    for g in nfo.genres {
        if !meta.genres.iter().any(|x| x.eq_ignore_ascii_case(&g)) {
            meta.genres.push(g);
        }
    }
    for tg in nfo.tags {
        if !meta.tags.iter().any(|x| x.eq_ignore_ascii_case(&tg)) {
            meta.tags.push(tg);
        }
    }
    for d in nfo.directors {
        if !meta.directors.contains(&d) {
            meta.directors.push(d);
        }
    }
    for a in nfo.actors {
        if !meta.actors.contains(&a) {
            meta.actors.push(a);
        }
    }
    if nfo.studio.is_some() {
        meta.studio = nfo.studio;
    }
    if nfo.collection_set.is_some() {
        meta.collection_set = nfo.collection_set;
    }
    for lf in nfo.locked_fields {
        if !meta.locked_fields.contains(&lf) {
            meta.locked_fields.push(lf);
        }
    }
    if nfo.tmdb_id.is_some() {
        meta.tmdb_id = nfo.tmdb_id;
    }
    if nfo.bangumi_id.is_some() {
        meta.bangumi_id = nfo.bangumi_id;
    }
}

pub(crate) fn render_video_nfo_xml_preserving(
    meta: &VideoFileMeta,
    existing_xml: Option<&[u8]>,
) -> String {
    let nfo = file_meta_to_nfo_meta(meta);
    fmt_render_video_nfo(&nfo, existing_xml)
}

pub(crate) fn render_tvshow_nfo_xml_preserving(
    meta: &VideoFileMeta,
    show_plot: Option<&str>,
    existing_xml: Option<&[u8]>,
) -> String {
    let nfo = file_meta_to_nfo_meta(meta);
    fmt_render_tvshow_nfo(&nfo, show_plot, existing_xml)
}

pub(crate) fn read_base_video_meta(path: &Path) -> VideoFileMeta {
    let container = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("mp4")
        .to_ascii_lowercase();
    let guessed = guess_video_from_path(path);
    let existing_nfo = find_existing_nfo(path, guessed.is_episode);
    let default_nfo = path.with_extension("nfo");

    let mut meta = VideoFileMeta {
        container,
        is_episode: guessed.is_episode,
        title: guessed.title,
        show_title: guessed.show_title,
        season_number: guessed.season_number,
        episode_number: guessed.episode_number,
        year: guessed.year,
        has_nfo: existing_nfo.is_some(),
        nfo_filename: existing_nfo
            .as_ref()
            .unwrap_or(&default_nfo)
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default(),
        has_poster: find_poster_path(path).is_some(),
        has_fanart: find_fanart_path(path).is_some(),
        has_thumb: find_thumb_path(path).is_some(),
        has_clearlogo: find_clearlogo_path(path).is_some(),
        sidecar_subtitles: list_sidecar_subtitles(path),
        ..Default::default()
    };

    if let Some(ref nfo_path) = existing_nfo
        && let Ok(bytes) = std::fs::read(nfo_path)
    {
        parse_nfo_into(&bytes, &mut meta);
    }

    if meta.is_episode
        && let Some(show_nfo_path) = find_show_nfo_for_episode(path)
        && let Ok(bytes) = std::fs::read(show_nfo_path)
    {
        let mut show_meta = VideoFileMeta::default();
        parse_nfo_into(&bytes, &mut show_meta);
        if meta.show_title.is_none() && !show_meta.title.trim().is_empty() {
            meta.show_title = Some(show_meta.title);
        }
        if meta.year.is_none() {
            meta.year = show_meta.year;
        }
        if meta.genres.is_empty() {
            meta.genres = show_meta.genres;
        }
        if meta.tags.is_empty() {
            meta.tags = show_meta.tags;
        }
        if meta.actors.is_empty() {
            meta.actors = show_meta.actors;
        }
        if meta.studio.is_none() {
            meta.studio = show_meta.studio;
        }
        if meta.tmdb_id.is_none() {
            meta.tmdb_id = show_meta.tmdb_id;
        }
        if meta.bangumi_id.is_none() {
            meta.bangumi_id = show_meta.bangumi_id;
        }
    }

    meta
}

pub(crate) fn apply_video_patch_onto(meta: &mut VideoFileMeta, vp: VideoMetaPatch) {
    if let Some(is_ep) = vp.is_episode {
        meta.is_episode = is_ep;
    }
    if let Some(title) = vp.title
        && !title.trim().is_empty()
    {
        meta.title = title.trim().to_string();
    }
    if let Some(orig) = vp.original_title {
        meta.original_title = (!orig.trim().is_empty()).then(|| orig.trim().to_string());
    }
    if let Some(show) = vp.show_title {
        meta.show_title = (!show.trim().is_empty()).then(|| show.trim().to_string());
    }
    if let Some(s) = vp.season_number {
        meta.season_number = if s < 0 { None } else { Some(s as u32) };
    }
    if vp.episode_number.is_some() {
        meta.episode_number = vp.episode_number.filter(|&n| n > 0);
    }
    if vp.year.is_some() {
        meta.year = vp.year.filter(|&y| y > 0);
    }
    if vp.rating.is_some() {
        meta.rating = vp.rating.filter(|&r| r > 0.0);
    }
    if let Some(summary) = vp.summary {
        meta.summary = (!summary.trim().is_empty()).then(|| summary.trim().to_string());
    }
    if let Some(genres) = vp.genres {
        meta.genres = genres
            .into_iter()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
    }
    if let Some(tags) = vp.tags {
        meta.tags = tags
            .into_iter()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
    }
    if let Some(actors) = vp.actors {
        meta.actors = actors
            .into_iter()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
    }
    if let Some(directors) = vp.directors {
        meta.directors = directors
            .into_iter()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
    }
    if let Some(studio) = vp.studio {
        meta.studio = (!studio.trim().is_empty()).then(|| studio.trim().to_string());
    }
    if let Some(tmdb) = vp.tmdb_id {
        meta.tmdb_id = (!tmdb.trim().is_empty()).then(|| tmdb.trim().to_string());
    }
    if let Some(bgm) = vp.bangumi_id {
        meta.bangumi_id = (!bgm.trim().is_empty()).then(|| bgm.trim().to_string());
    }
    if let Some(set_name) = vp.collection_set {
        meta.collection_set = (!set_name.trim().is_empty()).then(|| set_name.trim().to_string());
    }
    if let Some(locks) = vp.locked_fields {
        let mut clean = Vec::new();
        for f in locks {
            let low = f.trim().to_ascii_lowercase();
            if !low.is_empty() && !clean.contains(&low) {
                clean.push(low);
            }
        }
        meta.locked_fields = clean;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn video_nfo_preserves_streamdetails_uniqueids_actor_roles_and_extended_tags() {
        let original_xml = br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<movie>
  <title>The Matrix</title>
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
  <actor>
    <name>Old Actor</name>
    <role>Extra</role>
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
</movie>"#;

        let mut meta = VideoFileMeta::default();
        parse_nfo_into(original_xml, &mut meta);
        meta.title = "黑客帝国".to_string();
        meta.actors = vec!["Keanu Reeves".to_string(), "Carrie-Anne Moss".to_string()];

        let updated = render_video_nfo_xml_preserving(&meta, Some(original_xml));
        assert!(updated.contains("<title>黑客帝国</title>"));
        assert!(updated.contains("<premiered>1999-03-31</premiered>"));
        assert!(updated.contains("<mpaa>Rated R</mpaa>"));
        assert!(updated.contains("<country>United States</country>"));
        assert!(updated.contains("<uniqueid type=\"tmdb\" default=\"true\">603</uniqueid>"));
        assert!(updated.contains("<uniqueid type=\"imdb\">tt0133093</uniqueid>"));
        assert!(updated.contains("<uniqueid type=\"tvdb\">221</uniqueid>"));
        assert!(updated.contains("<role>Neo</role>"));
        assert!(updated.contains("<thumb>https://img.example/neo.jpg</thumb>"));
        assert!(!updated.contains("Old Actor"));
        assert!(updated.contains("<name>Carrie-Anne Moss</name>"));
        assert!(updated.contains("<codec>hevc</codec>"));
        assert!(updated.contains("<width>3840</width>"));
    }

    #[test]
    fn season_nfo_renders_both_season_and_seasonnumber_and_preserves_extra_nodes() {
        let prev = br#"<season>
  <seasonnumber>2</seasonnumber>
  <premiered>2024-01-05</premiered>
  <uniqueid type="tvdb">998877</uniqueid>
</season>"#;
        let rendered = render_season_nfo_xml_preserving(
            2,
            Some("第 2 季"),
            Some(2024),
            Some("第二季剧情"),
            Some("209867"),
            None,
            Some(prev),
        );
        assert!(rendered.contains("<season>2</season>"));
        assert!(rendered.contains("<seasonnumber>2</seasonnumber>"));
        assert!(rendered.contains("<premiered>2024-01-05</premiered>"));
        assert!(rendered.contains("<uniqueid type=\"tvdb\">998877</uniqueid>"));

        let mut parsed = VideoFileMeta::default();
        parse_nfo_into(rendered.as_bytes(), &mut parsed);
        assert_eq!(parsed.season_number, Some(2));
        assert_eq!(parsed.title, "第 2 季");
    }
}
