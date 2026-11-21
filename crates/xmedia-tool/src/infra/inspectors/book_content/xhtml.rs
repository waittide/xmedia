use std::sync::LazyLock;

use quick_xml::Reader;
use quick_xml::escape::escape;
use quick_xml::events::Event;
use regex::Regex;

use crate::domain::media::BookFileMeta;
use crate::infra::inspectors::append_xml_ref;

pub fn plain_to_xhtml_body(title: &str, plain: &str) -> String {
    let mut buf = String::new();
    let clean_title = title.trim();
    if !clean_title.is_empty() {
        buf.push_str(&format!("  <h2>{}</h2>\n", escape(clean_title)));
    }
    for line in plain.lines() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        buf.push_str(&format!("  <p>{}</p>\n", escape(t)));
    }
    buf.trim_end().to_string()
}

pub fn xhtml_body_to_plain(xhtml_body: &str, strip_heading: Option<&str>) -> String {
    let mut reader = Reader::from_str(xhtml_body);
    reader.config_mut().trim_text(false);

    let mut paragraphs: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut skip_depth = 0usize;

    let flush_current = |cur: &mut String, out: &mut Vec<String>| {
        let collapsed: String = cur.split_whitespace().collect::<Vec<_>>().join(" ");
        cur.clear();
        if !collapsed.is_empty() {
            out.push(collapsed);
        }
    };

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) => {
                let name = e.local_name().as_ref().to_ascii_lowercase();
                if matches!(name.as_str(), "script" | "style" | "head") {
                    skip_depth += 1;
                } else if skip_depth == 0
                    && matches!(
                        name.as_str(),
                        "p" | "div"
                            | "h1"
                            | "h2"
                            | "h3"
                            | "h4"
                            | "h5"
                            | "h6"
                            | "li"
                            | "blockquote"
                            | "section"
                            | "article"
                            | "tr"
                            | "br"
                    )
                {
                    flush_current(&mut current, &mut paragraphs);
                }
            }
            Ok(Event::Empty(ref e)) => {
                if skip_depth == 0 && e.local_name().as_ref().eq_ignore_ascii_case("br") {
                    flush_current(&mut current, &mut paragraphs);
                }
            }
            Ok(Event::Text(ref t)) if skip_depth == 0 => {
                current.push_str(&t.xml_content(quick_xml::XmlVersion::Implicit1_0));
            }
            Ok(Event::GeneralRef(ref r)) if skip_depth == 0 => {
                append_xml_ref(&mut current, r);
            }
            Ok(Event::CData(ref c)) if skip_depth == 0 => {
                current.push_str(c.as_ref());
            }
            Ok(Event::End(ref e)) => {
                let name = e.local_name().as_ref().to_ascii_lowercase();
                if matches!(name.as_str(), "script" | "style" | "head") {
                    skip_depth = skip_depth.saturating_sub(1);
                } else if skip_depth == 0
                    && matches!(
                        name.as_str(),
                        "p" | "div"
                            | "h1"
                            | "h2"
                            | "h3"
                            | "h4"
                            | "h5"
                            | "h6"
                            | "li"
                            | "blockquote"
                            | "section"
                            | "article"
                            | "tr"
                    )
                {
                    flush_current(&mut current, &mut paragraphs);
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    flush_current(&mut current, &mut paragraphs);

    if let Some(h) = strip_heading.map(str::trim).filter(|s| !s.is_empty())
        && paragraphs.first().is_some_and(|first| first.trim() == h)
    {
        paragraphs.remove(0);
    }

    paragraphs.join("\n")
}

pub(super) fn detect_rich_media_in_xhtml(xhtml: &str) -> bool {
    static RICH_RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)<\s*(?:img|svg|image|table|video|audio|figure|canvas)\b")
            .expect("valid rich media regex")
    });
    RICH_RE.is_match(xhtml)
}

pub(super) fn split_xhtml_head_and_body(full_xhtml: &str) -> (Option<String>, String) {
    static HEAD_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?is)<head\b[^>]*>.*?</head>").expect("valid head regex"));
    static BODY_RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?is)<body\b[^>]*>(?P<inner>.*?)</body>").expect("valid body regex")
    });

    let head = HEAD_RE.find(full_xhtml).map(|m| m.as_str().to_string());
    let body_inner = BODY_RE
        .captures(full_xhtml)
        .and_then(|c| c.name("inner"))
        .map(|m| m.as_str().trim().to_string())
        .unwrap_or_else(|| full_xhtml.trim().to_string());
    (head, body_inner)
}

pub(crate) fn extract_heading_from_xhtml(full_xhtml: &str) -> Option<String> {
    static H_RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?is)<(?:h1|h2|h3|title)\b[^>]*>(?P<t>.*?)</(?:h1|h2|h3|title)>")
            .expect("valid heading regex")
    });
    for caps in H_RE.captures_iter(full_xhtml) {
        if let Some(m) = caps.name("t") {
            let plain = xhtml_body_to_plain(m.as_str(), None);
            let trimmed = plain.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

pub(crate) fn wrap_full_xhtml_document(
    title: &str,
    lang: &str,
    preserved_head: Option<&str>,
    css_rel_href: Option<&str>,
    body_inner: &str,
) -> String {
    let head_block = if let Some(h) = preserved_head {
        h.to_string()
    } else {
        let css_line = css_rel_href
            .map(|href| {
                format!(
                    "  <link rel=\"stylesheet\" type=\"text/css\" href=\"{}\"/>\n",
                    escape(href)
                )
            })
            .unwrap_or_default();
        format!(
            "<head>\n  <meta charset=\"utf-8\"/>\n  <title>{}</title>\n{}</head>",
            escape(if title.trim().is_empty() {
                "Chapter"
            } else {
                title.trim()
            }),
            css_line
        )
    };

    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE html>\n<html xmlns=\"http://www.w3.org/1999/xhtml\" xml:lang=\"{lang}\" lang=\"{lang}\">\n{head_block}\n<body>\n{body_inner}\n</body>\n</html>\n",
        lang = escape(if lang.trim().is_empty() {
            "zh-CN"
        } else {
            lang.trim()
        }),
    )
}

pub(super) fn sanitize_xhtml_fragment(raw: &str) -> String {
    static VOID_TAG_RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)<(?P<tag>br|hr|img|meta|link|col|input) (?P<attrs>[^>]*?)(?P<slash>/?)>")
            .expect("valid void tag regex")
    });
    let replaced_entities = raw.replace("&nbsp;", "&#160;");
    VOID_TAG_RE
        .replace_all(&replaced_entities, |caps: &regex::Captures<'_>| {
            let tag = caps.name("tag").map(|m| m.as_str()).unwrap_or("br");
            let attrs = caps.name("attrs").map(|m| m.as_str()).unwrap_or("");
            let slash = caps.name("slash").map(|m| m.as_str()).unwrap_or("");
            if slash == "/" || attrs.trim_end().ends_with('/') {
                caps.get(0).map(|m| m.as_str()).unwrap_or("").to_string()
            } else {
                format!("<{tag}{attrs}/>")
            }
        })
        .into_owned()
}

pub(crate) fn render_book_jacket_xhtml(
    meta: &BookFileMeta,
    cover_rel_href: Option<&str>,
    css_rel_href: Option<&str>,
) -> String {
    let lang = meta
        .language
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("zh-CN");
    let css_link = css_rel_href
        .map(|h| {
            format!(
                "  <link rel=\"stylesheet\" type=\"text/css\" href=\"{}\"/>\n",
                escape(h)
            )
        })
        .unwrap_or_default();

    let jacket_css = r#"  <style type="text/css">
    .xmedia-jacket { max-width: 38em; margin: 1.2em auto; padding: 1.2em 1.4em; border: 1px solid rgba(128,128,128,0.28); border-radius: 10px; line-height: 1.75; }
    .xmedia-jacket-head { text-align: center; margin-bottom: 1.2em; }
    .xmedia-jacket-cover { margin: 0 auto 1em; text-align: center; }
    .xmedia-jacket-cover img { max-height: 14em; max-width: 65%; border-radius: 6px; box-shadow: 0 4px 14px rgba(0,0,0,0.16); }
    .xmedia-jacket-title { font-size: 1.45em; font-weight: 700; margin: 0.2em 0 0.35em; line-height: 1.3; text-indent: 0; }
    .xmedia-jacket-author { font-size: 1em; opacity: 0.85; margin: 0 0 0.5em; text-indent: 0; }
    .xmedia-jacket-series { display: inline-block; font-size: 0.88em; padding: 0.15em 0.7em; border-radius: 999px; background: rgba(128,128,128,0.14); margin-bottom: 0.5em; }
    .xmedia-jacket-meta { font-size: 0.88em; opacity: 0.82; margin: 0.8em 0; padding: 0.65em 0.9em; background: rgba(128,128,128,0.08); border-radius: 6px; }
    .xmedia-jacket-meta div { margin: 0.2em 0; text-indent: 0; }
    .xmedia-jacket-tags { margin: 0.7em 0; text-align: center; }
    .xmedia-jacket-tag { display: inline-block; font-size: 0.8em; padding: 0.12em 0.55em; margin: 0.15em; border: 1px solid rgba(128,128,128,0.3); border-radius: 4px; }
    .xmedia-jacket-hr { border: 0; border-top: 1px solid rgba(128,128,128,0.24); margin: 1em 0; }
    .xmedia-jacket-desc p { margin: 0 0 0.75em; text-indent: 2em; text-align: justify; }
  </style>"#;

    let mut body = String::from(
        "  <section class=\"xmedia-jacket\" epub:type=\"frontmatter\">\n    <div class=\"xmedia-jacket-head\">\n",
    );
    if let Some(cov) = cover_rel_href {
        body.push_str(&format!(
            "      <div class=\"xmedia-jacket-cover\"><img src=\"{}\" alt=\"封面\"/></div>\n",
            escape(cov)
        ));
    }
    body.push_str(&format!(
        "      <h1 class=\"xmedia-jacket-title\">{}</h1>\n",
        escape(meta.title.trim())
    ));
    if !meta.authors.is_empty() {
        let joined = meta.authors.join(" / ");
        body.push_str(&format!(
            "      <p class=\"xmedia-jacket-author\">{}</p>\n",
            escape(&joined)
        ));
    }
    if let Some(ser) = meta
        .series
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let mut badge = ser.to_string();
        if let Some(sub) = meta
            .subseries
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            badge.push_str(" · ");
            badge.push_str(sub);
        }
        if let Some(idx) = meta
            .series_index
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            badge.push_str(&format!(" #{idx}"));
        }
        body.push_str(&format!(
            "      <div class=\"xmedia-jacket-series\">{}</div>\n",
            escape(&badge)
        ));
    }
    body.push_str("    </div>\n");

    if !meta.tags.is_empty() {
        body.push_str("    <div class=\"xmedia-jacket-tags\">\n");
        for tag in &meta.tags {
            let t = tag.trim();
            if !t.is_empty() {
                body.push_str(&format!(
                    "      <span class=\"xmedia-jacket-tag\">{}</span>\n",
                    escape(t)
                ));
            }
        }
        body.push_str("    </div>\n");
    }

    let mut meta_lines = Vec::new();
    if let Some(p) = meta
        .publisher
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        meta_lines.push(format!("出版社：{}", escape(p)));
    }
    if let Some(d) = meta
        .published_at
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        meta_lines.push(format!("出版日期：{}", escape(d)));
    }
    if let Some(i) = meta
        .isbn
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        meta_lines.push(format!("ISBN：{}", escape(i)));
    }
    if !meta_lines.is_empty() {
        body.push_str("    <div class=\"xmedia-jacket-meta\">\n");
        for line in meta_lines {
            body.push_str(&format!("      <div>{line}</div>\n"));
        }
        body.push_str("    </div>\n");
    }

    if let Some(desc) = meta
        .description
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        body.push_str(
            "    <hr class=\"xmedia-jacket-hr\"/>\n    <div class=\"xmedia-jacket-desc\">\n",
        );
        for para in desc.lines() {
            let t = para.trim();
            if !t.is_empty() {
                body.push_str(&format!("      <p>{}</p>\n", escape(t)));
            }
        }
        body.push_str("    </div>\n");
    }

    body.push_str("  </section>");

    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE html>\n<html xmlns=\"http://www.w3.org/1999/xhtml\" xmlns:epub=\"http://www.idpf.org/2007/ops\" xml:lang=\"{l}\" lang=\"{l}\">\n<head>\n  <meta charset=\"utf-8\"/>\n  <title>书籍简介 - {t}</title>\n{css_link}{jacket_css}\n</head>\n<body>\n{body}\n</body>\n</html>\n",
        l = escape(lang),
        t = escape(meta.title.trim()),
    )
}
