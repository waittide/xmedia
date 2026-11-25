use super::nfo::VideoNfoMeta;
use super::nfo_preserve::{
    NfoPreserveKind, append_actors_xml, append_optional_or_preserved, append_streamdetails_xml,
    append_unique_ids_xml, extract_preserved_nfo_nodes,
};
use crate::common::xml_escape_clean;

/// Renders a `<movie>` or `<episodedetails>` NFO while preserving `<fileinfo><streamdetails>`,
/// `<uniqueid>`, `<actor>` roles/thumbs, `<premiered>`, `<mpaa>`, `<country>`, `<outline>`, and custom tags.
pub fn render_video_nfo_xml_preserving(meta: &VideoNfoMeta, existing_xml: Option<&[u8]>) -> String {
    let preserved = existing_xml
        .map(|b| extract_preserved_nfo_nodes(b, NfoPreserveKind::Video))
        .unwrap_or_default();
    let root_tag = if meta.is_episode {
        "episodedetails"
    } else {
        "movie"
    };
    let title_str = meta
        .title
        .as_deref()
        .or(meta.raw_title.as_deref())
        .unwrap_or("");

    let mut xml = String::with_capacity(1024);
    xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n");
    xml.push_str(&format!("<{root_tag}>\n"));
    xml.push_str(&format!(
        "  <title>{}</title>\n",
        xml_escape_clean(title_str)
    ));

    if let Some(ref orig) = meta.original_title
        && !orig.trim().is_empty()
    {
        xml.push_str(&format!(
            "  <originaltitle>{}</originaltitle>\n",
            xml_escape_clean(orig.trim())
        ));
    }
    if meta.is_episode {
        if let Some(ref show) = meta.show_title
            && !show.trim().is_empty()
        {
            xml.push_str(&format!(
                "  <showtitle>{}</showtitle>\n",
                xml_escape_clean(show.trim())
            ));
        }
        if let Some(season) = meta.season_number {
            xml.push_str(&format!("  <season>{season}</season>\n"));
            xml.push_str(&format!("  <seasonnumber>{season}</seasonnumber>\n"));
        }
        if let Some(ep) = meta.episode_number {
            xml.push_str(&format!("  <episode>{ep}</episode>\n"));
        }
    }
    if let Some(year) = meta.year {
        xml.push_str(&format!("  <year>{year}</year>\n"));
    }
    append_optional_or_preserved(
        &mut xml,
        "premiered",
        meta.premiered.as_ref(),
        preserved.premiered_xml.as_ref(),
    );
    append_optional_or_preserved(
        &mut xml,
        "mpaa",
        meta.mpaa.as_ref(),
        preserved.mpaa_xml.as_ref(),
    );
    match meta.country.as_ref() {
        Some(c) => {
            if !c.trim().is_empty() {
                xml.push_str(&format!(
                    "  <country>{}</country>\n",
                    xml_escape_clean(c.trim())
                ));
            }
        }
        None => {
            for raw_c in &preserved.countries_xml {
                xml.push_str(raw_c);
            }
        }
    }
    if let Some(rating) = meta.rating {
        xml.push_str(&format!("  <rating>{rating:.1}</rating>\n"));
    }
    let has_plot = meta
        .summary
        .as_deref()
        .map(str::trim)
        .is_some_and(|s| !s.is_empty());
    if let Some(ref plot) = meta.summary
        && !plot.trim().is_empty()
    {
        xml.push_str(&format!(
            "  <plot>{}</plot>\n",
            xml_escape_clean(plot.trim())
        ));
    }
    if let Some(ref outline) = meta.outline
        && !outline.trim().is_empty()
    {
        xml.push_str(&format!(
            "  <outline>{}</outline>\n",
            xml_escape_clean(outline.trim())
        ));
    } else if has_plot && let Some(ref outline_raw) = preserved.outline_xml {
        xml.push_str(outline_raw);
    }
    if let Some(ref studio) = meta.studio
        && !studio.trim().is_empty()
    {
        xml.push_str(&format!(
            "  <studio>{}</studio>\n",
            xml_escape_clean(studio.trim())
        ));
    }
    for g in &meta.genres {
        if !g.trim().is_empty() {
            xml.push_str(&format!(
                "  <genre>{}</genre>\n",
                xml_escape_clean(g.trim())
            ));
        }
    }
    for tg in &meta.tags {
        if !tg.trim().is_empty() {
            xml.push_str(&format!("  <tag>{}</tag>\n", xml_escape_clean(tg.trim())));
        }
    }
    for dir in &meta.directors {
        if !dir.trim().is_empty() {
            xml.push_str(&format!(
                "  <director>{}</director>\n",
                xml_escape_clean(dir.trim())
            ));
        }
    }
    append_actors_xml(
        &mut xml,
        &meta.actors,
        &meta.actor_details,
        &preserved.actor_blocks,
    );
    if !meta.is_episode
        && let Some(ref set_name) = meta.collection_set
        && !set_name.trim().is_empty()
    {
        xml.push_str("  <set>\n");
        xml.push_str(&format!(
            "    <name>{}</name>\n",
            xml_escape_clean(set_name.trim())
        ));
        xml.push_str("  </set>\n");
    }
    if !meta.locked_fields.is_empty() {
        xml.push_str(&format!(
            "  <lockedfields>{}</lockedfields>\n",
            xml_escape_clean(&meta.locked_fields.join("|"))
        ));
    }
    append_unique_ids_xml(
        &mut xml,
        meta.tmdb_id.as_deref(),
        meta.bangumi_id.as_deref(),
        &meta.unique_ids,
        &preserved.other_uniqueids,
    );
    for extra in &preserved.extra_elements {
        xml.push_str(extra);
    }
    if meta.width.is_some()
        || meta.video_codec.is_some()
        || !meta.audio_streams.is_empty()
        || !meta.subtitle_streams.is_empty()
    {
        append_streamdetails_xml(&mut xml, meta);
    } else if let Some(ref preserved_fileinfo) = preserved.fileinfo_xml {
        xml.push_str(preserved_fileinfo);
    }
    xml.push_str(&format!("</{root_tag}>\n"));
    xml
}

/// Renders series-level `tvshow.nfo` (`<tvshow>`) non-destructively.
pub fn render_tvshow_nfo_xml_preserving(
    meta: &VideoNfoMeta,
    show_plot: Option<&str>,
    existing_xml: Option<&[u8]>,
) -> String {
    let preserved = existing_xml
        .map(|b| extract_preserved_nfo_nodes(b, NfoPreserveKind::TvShow))
        .unwrap_or_default();
    let show_title = meta
        .show_title
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .or(meta.title.as_deref())
        .or(meta.raw_title.as_deref())
        .unwrap_or("");
    let mut xml = String::with_capacity(1024);
    xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<tvshow>\n");
    xml.push_str(&format!(
        "  <title>{}</title>\n",
        xml_escape_clean(show_title.trim())
    ));
    if let Some(ref orig) = meta.original_title
        && !orig.trim().is_empty()
    {
        xml.push_str(&format!(
            "  <originaltitle>{}</originaltitle>\n",
            xml_escape_clean(orig.trim())
        ));
    }
    if let Some(year) = meta.year {
        xml.push_str(&format!("  <year>{year}</year>\n"));
    }
    append_optional_or_preserved(
        &mut xml,
        "premiered",
        meta.premiered.as_ref(),
        preserved.premiered_xml.as_ref(),
    );
    append_optional_or_preserved(
        &mut xml,
        "mpaa",
        meta.mpaa.as_ref(),
        preserved.mpaa_xml.as_ref(),
    );
    match meta.country.as_ref() {
        Some(c) if !c.trim().is_empty() => {
            xml.push_str(&format!(
                "  <country>{}</country>\n",
                xml_escape_clean(c.trim())
            ));
        }
        None => {
            for raw_c in &preserved.countries_xml {
                xml.push_str(raw_c);
            }
        }
        _ => {}
    }
    if let Some(rating) = meta.rating {
        xml.push_str(&format!("  <rating>{rating:.1}</rating>\n"));
    }
    let effective_plot = show_plot
        .or(meta.summary.as_deref())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    if let Some(plot) = effective_plot {
        xml.push_str(&format!("  <plot>{}</plot>\n", xml_escape_clean(plot)));
        if let Some(ref outline_raw) = preserved.outline_xml {
            xml.push_str(outline_raw);
        }
    }
    if let Some(ref studio) = meta.studio
        && !studio.trim().is_empty()
    {
        xml.push_str(&format!(
            "  <studio>{}</studio>\n",
            xml_escape_clean(studio.trim())
        ));
    }
    for g in &meta.genres {
        if !g.trim().is_empty() {
            xml.push_str(&format!(
                "  <genre>{}</genre>\n",
                xml_escape_clean(g.trim())
            ));
        }
    }
    for tg in &meta.tags {
        if !tg.trim().is_empty() {
            xml.push_str(&format!("  <tag>{}</tag>\n", xml_escape_clean(tg.trim())));
        }
    }
    append_actors_xml(
        &mut xml,
        &meta.actors,
        &meta.actor_details,
        &preserved.actor_blocks,
    );
    append_unique_ids_xml(
        &mut xml,
        meta.tmdb_id.as_deref(),
        meta.bangumi_id.as_deref(),
        &meta.unique_ids,
        &preserved.other_uniqueids,
    );
    for extra in &preserved.extra_elements {
        xml.push_str(extra);
    }
    xml.push_str("</tvshow>\n");
    xml
}

/// Renders season-level `season.nfo` (`<season>`), emitting both `<season>` and `<seasonnumber>`.
pub fn render_season_nfo_xml_preserving(
    season_number: u32,
    season_title: Option<&str>,
    year: Option<i32>,
    season_plot: Option<&str>,
    tmdb_id: Option<&str>,
    bangumi_id: Option<&str>,
    existing_xml: Option<&[u8]>,
) -> String {
    let preserved = existing_xml
        .map(|b| extract_preserved_nfo_nodes(b, NfoPreserveKind::Season))
        .unwrap_or_default();
    let default_title = format!("Season {season_number:02}");
    let title = season_title
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(&default_title);
    let mut xml = String::with_capacity(512);
    xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<season>\n");
    xml.push_str(&format!("  <season>{season_number}</season>\n"));
    xml.push_str(&format!("  <seasonnumber>{season_number}</seasonnumber>\n"));
    xml.push_str(&format!("  <title>{}</title>\n", xml_escape_clean(title)));
    if let Some(y) = year {
        xml.push_str(&format!("  <year>{y}</year>\n"));
    }
    if let Some(ref prem) = preserved.premiered_xml {
        xml.push_str(prem);
    }
    if let Some(plot) = season_plot.map(str::trim).filter(|s| !s.is_empty()) {
        xml.push_str(&format!("  <plot>{}</plot>\n", xml_escape_clean(plot)));
        if let Some(ref outline_raw) = preserved.outline_xml {
            xml.push_str(outline_raw);
        }
    }
    append_unique_ids_xml(
        &mut xml,
        tmdb_id,
        bangumi_id,
        &[],
        &preserved.other_uniqueids,
    );
    for extra in &preserved.extra_elements {
        xml.push_str(extra);
    }
    xml.push_str("</season>\n");
    xml
}
