use quick_xml::Reader;
use quick_xml::events::Event;

use super::nfo::{NfoActor, NfoUniqueId, VideoNfoMeta};
use crate::common::{append_xml_ref, decode_text_smart, xml_escape_clean};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum NfoPreserveKind {
    Video,
    TvShow,
    Season,
}

#[derive(Default)]
pub(crate) struct PreservedNfoNodes {
    pub(crate) fileinfo_xml: Option<String>,
    pub(crate) actor_blocks: Vec<(String, String)>,
    pub(crate) other_uniqueids: Vec<(String, bool)>,
    pub(crate) outline_xml: Option<String>,
    pub(crate) premiered_xml: Option<String>,
    pub(crate) mpaa_xml: Option<String>,
    pub(crate) countries_xml: Vec<String>,
    pub(crate) extra_elements: Vec<String>,
}

fn is_managed_nfo_tag(kind: NfoPreserveKind, tag_lower: &str) -> bool {
    match kind {
        NfoPreserveKind::Video => matches!(
            tag_lower,
            "title"
                | "originaltitle"
                | "showtitle"
                | "season"
                | "seasonnumber"
                | "episode"
                | "year"
                | "premiered"
                | "mpaa"
                | "country"
                | "rating"
                | "plot"
                | "outline"
                | "studio"
                | "genre"
                | "tag"
                | "director"
                | "actor"
                | "set"
                | "lockedfields"
                | "tmdbid"
                | "uniqueid"
                | "fileinfo"
        ),
        NfoPreserveKind::TvShow => matches!(
            tag_lower,
            "title"
                | "originaltitle"
                | "showtitle"
                | "year"
                | "premiered"
                | "mpaa"
                | "country"
                | "rating"
                | "plot"
                | "outline"
                | "studio"
                | "genre"
                | "tag"
                | "actor"
                | "tmdbid"
                | "uniqueid"
        ),
        NfoPreserveKind::Season => matches!(
            tag_lower,
            "season"
                | "seasonnumber"
                | "title"
                | "year"
                | "premiered"
                | "plot"
                | "outline"
                | "tmdbid"
                | "uniqueid"
        ),
    }
}

fn parse_uniqueid_attrs(e: &quick_xml::events::BytesStart<'_>) -> (Option<String>, bool) {
    let mut uid_type = None;
    let mut is_default = false;
    for attr in e.attributes().flatten() {
        let key = attr.key.local_name();
        let Ok(val) = attr.normalized_value(quick_xml::XmlVersion::Implicit1_0) else {
            continue;
        };
        if key.as_ref().eq_ignore_ascii_case("type") {
            uid_type = Some(val.trim().to_ascii_lowercase());
        } else if key.as_ref().eq_ignore_ascii_case("default") {
            is_default = val.trim().eq_ignore_ascii_case("true");
        }
    }
    (uid_type, is_default)
}

fn strip_default_attr_on_uniqueid(raw_uid: &str) -> String {
    let Some(gt_idx) = raw_uid.find('>') else {
        return raw_uid.to_string();
    };
    let head = raw_uid[..gt_idx]
        .replace(" default=\"true\"", "")
        .replace(" default='true'", "")
        .replace(" default=\"TRUE\"", "");
    format!("{head}{}", &raw_uid[gt_idx..])
}

pub(crate) fn extract_preserved_nfo_nodes(
    xml_bytes: &[u8],
    kind: NfoPreserveKind,
) -> PreservedNfoNodes {
    let (decoded, _) = decode_text_smart(xml_bytes);
    let xml_str = decoded.trim_start_matches('\u{feff}');
    let mut reader = Reader::from_str(xml_str);
    reader.config_mut().trim_text(false);

    let mut out = PreservedNfoNodes::default();
    let mut depth = 0usize;
    let mut top_elem: Option<(String, usize, Option<String>, bool)> = None;
    let mut in_actor_name = false;
    let mut actor_name_acc = String::new();

    loop {
        let pos_before = reader.buffer_position() as usize;
        match reader.read_event() {
            Ok(Event::Start(ref e)) => {
                let name = e.local_name().as_ref().to_ascii_lowercase();
                if depth == 1 {
                    let (uid_type, uid_default) = if name == "uniqueid" {
                        parse_uniqueid_attrs(e)
                    } else {
                        (None, false)
                    };
                    top_elem = Some((name, pos_before, uid_type, uid_default));
                    actor_name_acc.clear();
                    in_actor_name = false;
                } else if depth == 2
                    && top_elem.as_ref().is_some_and(|(t, _, _, _)| t == "actor")
                    && name == "name"
                {
                    in_actor_name = true;
                    actor_name_acc.clear();
                }
                depth += 1;
            }
            Ok(Event::Empty(ref e)) => {
                if depth == 1 {
                    let pos_after = reader.buffer_position() as usize;
                    let name = e.local_name().as_ref().to_ascii_lowercase();
                    if let Some(raw_slice) = xml_str.get(pos_before..pos_after)
                        && !is_managed_nfo_tag(kind, &name)
                    {
                        out.extra_elements.push(format!("  {}\n", raw_slice.trim()));
                    }
                }
            }
            Ok(Event::Text(ref t)) if in_actor_name && depth == 3 => {
                actor_name_acc.push_str(&t.xml10_content());
            }
            Ok(Event::GeneralRef(ref r)) if in_actor_name && depth == 3 => {
                append_xml_ref(&mut actor_name_acc, r);
            }
            Ok(Event::CData(ref c)) if in_actor_name && depth == 3 => {
                actor_name_acc.push_str(c.as_ref());
            }
            Ok(Event::End(_)) => {
                if depth == 3 && in_actor_name {
                    in_actor_name = false;
                }
                depth = depth.saturating_sub(1);
                if depth == 1
                    && let Some((name, start_idx, uid_type, uid_default)) = top_elem.take()
                {
                    let pos_after = reader.buffer_position() as usize;
                    if let Some(raw_slice) = xml_str.get(start_idx..pos_after) {
                        let trimmed_raw = raw_slice.trim();
                        let formatted = format!("  {trimmed_raw}\n");
                        match name.as_str() {
                            "fileinfo" if kind == NfoPreserveKind::Video => {
                                out.fileinfo_xml = Some(formatted);
                            }
                            "actor" if kind != NfoPreserveKind::Season => {
                                let clean_actor = actor_name_acc.trim().to_string();
                                if !clean_actor.is_empty() {
                                    out.actor_blocks.push((clean_actor, formatted));
                                }
                            }
                            "uniqueid" => {
                                if !matches!(
                                    uid_type.as_deref(),
                                    Some("tmdb") | Some("bangumi") | Some("bgm")
                                ) {
                                    out.other_uniqueids
                                        .push((trimmed_raw.to_string(), uid_default));
                                }
                            }
                            "outline" => out.outline_xml = Some(formatted),
                            "premiered" => out.premiered_xml = Some(formatted),
                            "mpaa" => out.mpaa_xml = Some(formatted),
                            "country" => out.countries_xml.push(formatted),
                            other if !is_managed_nfo_tag(kind, other) => {
                                out.extra_elements.push(formatted);
                            }
                            _ => {}
                        }
                    }
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }

    out
}

fn clean_id(raw: &str) -> &str {
    raw.trim().split(':').next().unwrap_or("").trim()
}

pub(crate) fn append_unique_ids_xml(
    xml: &mut String,
    tmdb_id: Option<&str>,
    bangumi_id: Option<&str>,
    meta_unique_ids: &[NfoUniqueId],
    preserved_uniqueids: &[(String, bool)],
) {
    let has_tmdb = tmdb_id.map(clean_id).is_some_and(|s| !s.is_empty());
    let has_bgm = bangumi_id.map(clean_id).is_some_and(|s| !s.is_empty());
    let has_managed = has_tmdb || has_bgm;

    if let Some(tmdb) = tmdb_id.map(clean_id).filter(|s| !s.is_empty()) {
        xml.push_str(&format!(
            "  <uniqueid type=\"tmdb\" default=\"true\">{}</uniqueid>\n",
            xml_escape_clean(tmdb)
        ));
    }
    if let Some(bgm) = bangumi_id.map(clean_id).filter(|s| !s.is_empty()) {
        let default_attr = if has_tmdb { "" } else { " default=\"true\"" };
        xml.push_str(&format!(
            "  <uniqueid type=\"bangumi\"{default_attr}>{}</uniqueid>\n",
            xml_escape_clean(bgm)
        ));
    }
    if !preserved_uniqueids.is_empty() {
        for (raw_uid, was_default) in preserved_uniqueids {
            if has_managed && *was_default {
                xml.push_str("  ");
                xml.push_str(&strip_default_attr_on_uniqueid(raw_uid));
                xml.push('\n');
            } else {
                xml.push_str("  ");
                xml.push_str(raw_uid);
                xml.push('\n');
            }
        }
    } else {
        for uid in meta_unique_ids {
            if matches!(uid.id_type.as_str(), "tmdb" | "bangumi" | "bgm")
                || uid.value.trim().is_empty()
            {
                continue;
            }
            let default_attr = if !has_managed && uid.is_default {
                " default=\"true\""
            } else {
                ""
            };
            let type_attr = if uid.id_type.trim().is_empty() {
                String::new()
            } else {
                format!(" type=\"{}\"", xml_escape_clean(uid.id_type.trim()))
            };
            xml.push_str(&format!(
                "  <uniqueid{type_attr}{default_attr}>{}</uniqueid>\n",
                xml_escape_clean(uid.value.trim())
            ));
        }
    }
}

pub(crate) fn append_actors_xml(
    xml: &mut String,
    actors: &[String],
    actor_details: &[NfoActor],
    preserved_actors: &[(String, String)],
) {
    let mut used = vec![false; preserved_actors.len()];
    for actor in actors {
        let clean = actor.trim();
        if clean.is_empty() {
            continue;
        }
        if let Some((idx, (_, block))) = preserved_actors
            .iter()
            .enumerate()
            .find(|(i, (saved_name, _))| !used[*i] && saved_name == clean)
        {
            used[idx] = true;
            xml.push_str(block);
            continue;
        }
        let detail = actor_details.iter().find(|d| d.name.trim() == clean);
        xml.push_str("  <actor>\n");
        xml.push_str(&format!("    <name>{}</name>\n", xml_escape_clean(clean)));
        if let Some(d) = detail {
            if let Some(ref role) = d.role
                && !role.trim().is_empty()
            {
                xml.push_str(&format!(
                    "    <role>{}</role>\n",
                    xml_escape_clean(role.trim())
                ));
            }
            if let Some(ref thumb) = d.thumb
                && !thumb.trim().is_empty()
            {
                xml.push_str(&format!(
                    "    <thumb>{}</thumb>\n",
                    xml_escape_clean(thumb.trim())
                ));
            }
        }
        xml.push_str("  </actor>\n");
    }
}

pub(crate) fn append_optional_or_preserved(
    xml: &mut String,
    tag: &str,
    val: Option<&String>,
    preserved_raw: Option<&String>,
) {
    match val {
        Some(v) => {
            let clean = v.trim();
            if !clean.is_empty() {
                xml.push_str(&format!("  <{tag}>{}</{tag}>\n", xml_escape_clean(clean)));
            }
        }
        None => {
            if let Some(raw) = preserved_raw {
                xml.push_str(raw);
            }
        }
    }
}

pub(crate) fn append_streamdetails_xml(xml: &mut String, meta: &VideoNfoMeta) {
    xml.push_str("  <fileinfo>\n    <streamdetails>\n");
    if meta.width.is_some() || meta.video_codec.is_some() {
        xml.push_str("      <video>\n");
        if let Some(ref codec) = meta.video_codec {
            xml.push_str(&format!(
                "        <codec>{}</codec>\n",
                xml_escape_clean(codec)
            ));
        }
        if let Some(w) = meta.width {
            xml.push_str(&format!("        <width>{w}</width>\n"));
        }
        if let Some(h) = meta.height {
            xml.push_str(&format!("        <height>{h}</height>\n"));
        }
        if let Some(dur) = meta.duration_secs.filter(|&d| d > 0) {
            xml.push_str(&format!(
                "        <durationinseconds>{dur}</durationinseconds>\n"
            ));
        }
        xml.push_str("      </video>\n");
    }
    for a in &meta.audio_streams {
        xml.push_str("      <audio>\n");
        if !a.codec.trim().is_empty() {
            xml.push_str(&format!(
                "        <codec>{}</codec>\n",
                xml_escape_clean(a.codec.trim())
            ));
        }
        if let Some(ref lang) = a.language {
            xml.push_str(&format!(
                "        <language>{}</language>\n",
                xml_escape_clean(lang)
            ));
        }
        if a.channels > 0 {
            xml.push_str(&format!("        <channels>{}</channels>\n", a.channels));
        }
        xml.push_str("      </audio>\n");
    }
    for s in &meta.subtitle_streams {
        if let Some(ref lang) = s.language {
            xml.push_str("      <subtitle>\n");
            xml.push_str(&format!(
                "        <language>{}</language>\n",
                xml_escape_clean(lang)
            ));
            xml.push_str("      </subtitle>\n");
        }
    }
    xml.push_str("    </streamdetails>\n  </fileinfo>\n");
}
