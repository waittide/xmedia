use quick_xml::events::Event;
use quick_xml::reader::Reader;

use crate::application::ports::ComicInfoMetadata;
use crate::domain::series::ReadingDirection;

pub fn parse_comic_info_xml(xml_bytes: &[u8]) -> Option<ComicInfoMetadata> {
    let mut reader = Reader::from_reader(xml_bytes);
    reader.config_mut().trim_text(false);

    let mut buf = Vec::new();
    let mut current_tag: Option<String> = None;
    let mut text_acc = String::new();
    let mut meta = ComicInfoMetadata::default();
    let mut manga_raw: Option<String> = None;
    let mut found_any = false;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let tag = e.local_name().as_ref().to_ascii_lowercase();
                current_tag = Some(tag);
                text_acc.clear();
            }
            Ok(Event::End(_)) => {
                if let Some(tag) = current_tag.take() {
                    let trimmed = text_acc.trim();
                    if !trimmed.is_empty() {
                        found_any = true;
                        assign_field(&mut meta, &mut manga_raw, &tag, trimmed);
                    }
                }
                text_acc.clear();
            }
            Ok(Event::Text(t)) => {
                if current_tag.is_some() {
                    text_acc.push_str(&t.xml10_content());
                }
            }
            Ok(Event::GeneralRef(r)) => {
                if current_tag.is_some() {
                    append_xml_ref(&mut text_acc, &r);
                }
            }
            Ok(Event::CData(c)) => {
                if current_tag.is_some() {
                    text_acc.push_str(c.as_ref());
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => return None,
            _ => {}
        }
        buf.clear();
    }

    if !found_any {
        return None;
    }

    let is_webtoon = meta.genres.iter().chain(meta.tags.iter()).any(|item| {
        let l = item.to_ascii_lowercase();
        l.contains("webtoon") || l.contains("long strip") || item.contains("条漫")
    });

    if is_webtoon {
        meta.reading_direction = Some(ReadingDirection::Webtoon);
    } else if let Some(manga) = manga_raw {
        let lower = manga.to_ascii_lowercase();
        if lower == "yesandrighttoleft" || lower == "yes" || lower == "true" {
            meta.reading_direction = Some(ReadingDirection::Rtl);
        } else if lower == "no" || lower == "false" {
            meta.reading_direction = Some(ReadingDirection::Ltr);
        }
    }

    Some(meta)
}

fn assign_field(
    meta: &mut ComicInfoMetadata,
    manga_raw: &mut Option<String>,
    tag: &str,
    value: &str,
) {
    match tag {
        "series" => meta.series = Some(value.to_owned()),
        "title" => meta.title = Some(value.to_owned()),
        "number" => {
            meta.number = value.parse::<f64>().ok();
        }
        "volume" => {
            if let Ok(v) = value.parse::<f64>()
                && v >= 0.0
            {
                meta.volume = Some(v);
            }
        }
        "summary" => meta.summary = Some(value.to_owned()),
        "writer" => meta.writers.extend(split_csv(value)),
        "penciller" | "artist" | "coverartist" => meta.pencillers.extend(split_csv(value)),
        "publisher" => meta.publisher = Some(value.to_owned()),
        "genre" => meta.genres.extend(split_csv(value)),
        "tags" => meta.tags.extend(split_csv(value)),
        "agerating" => meta.age_rating = Some(value.to_owned()),
        "languageiso" => meta.language_iso = Some(value.to_owned()),
        "year" => meta.year = value.parse::<i32>().ok(),
        "pagecount" => meta.page_count = value.parse::<u32>().ok(),
        "manga" => *manga_raw = Some(value.to_owned()),
        _ => {}
    }
}

fn split_csv(value: &str) -> Vec<String> {
    value
        .split([',', ';', '、'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}

pub(crate) fn append_xml_ref(out: &mut String, r: &quick_xml::events::BytesRef<'_>) {
    if let Ok(Some(ch)) = r.resolve_char_ref() {
        out.push(ch);
        return;
    }
    if let Some(resolved) = quick_xml::escape::resolve_xml_entity(r.as_ref()) {
        out.push_str(resolved);
    } else {
        out.push('&');
        out.push_str(r.as_ref());
        out.push(';');
    }
}
