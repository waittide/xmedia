use scraper::{Html, Selector};
use xmedia_source::http::resolve_relative_url;
use xmedia_source::rule::cached_regex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PageSegment<'a> {
    Html(&'a str),
    TopLevelText(&'a str),
}

fn is_html_token_start(slice: &str) -> bool {
    let b = slice.as_bytes();
    if b.len() < 2 || b[0] != b'<' {
        return false;
    }
    if slice.starts_with("<!--") || b[1] == b'!' || b[1] == b'?' {
        return true;
    }
    if b[1] == b'/' {
        return b
            .get(2)
            .is_some_and(|c| c.is_ascii_alphabetic() || *c == b'>');
    }
    b[1].is_ascii_alphabetic()
}

fn is_void_html_element(tag: &str) -> bool {
    matches!(
        tag,
        "area"
            | "base"
            | "basefont"
            | "bgsound"
            | "br"
            | "col"
            | "embed"
            | "frame"
            | "hr"
            | "img"
            | "input"
            | "keygen"
            | "link"
            | "meta"
            | "param"
            | "source"
            | "track"
            | "wbr"
    )
}

fn is_raw_text_html_element(tag: &str) -> bool {
    matches!(
        tag,
        "script" | "style" | "textarea" | "title" | "xmp" | "iframe" | "noembed" | "noframes"
    )
}

fn apply_html5_implicit_close(stack: &mut Vec<String>, next_tag: &str) {
    if matches!(
        next_tag,
        "address"
            | "article"
            | "aside"
            | "blockquote"
            | "details"
            | "dialog"
            | "dd"
            | "div"
            | "dl"
            | "dt"
            | "fieldset"
            | "figcaption"
            | "figure"
            | "footer"
            | "form"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "header"
            | "hgroup"
            | "hr"
            | "li"
            | "main"
            | "menu"
            | "nav"
            | "ol"
            | "p"
            | "pre"
            | "search"
            | "section"
            | "table"
            | "ul"
    ) && stack.last().is_some_and(|t| t == "p")
    {
        stack.pop();
    }
    if (next_tag == "li" && stack.last().is_some_and(|t| t == "li"))
        || (matches!(next_tag, "dt" | "dd") && stack.last().is_some_and(|t| t == "dt" || t == "dd"))
        || (matches!(next_tag, "td" | "th") && stack.last().is_some_and(|t| t == "td" || t == "th"))
    {
        stack.pop();
    } else if next_tag == "tr" {
        while stack
            .last()
            .is_some_and(|t| matches!(t.as_str(), "td" | "th" | "tr"))
        {
            stack.pop();
        }
    }
}

fn parse_tag_name(bytes: &[u8], start: usize) -> (String, usize) {
    let mut end = start;
    while end < bytes.len()
        && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'-' || bytes[end] == b':')
    {
        end += 1;
    }
    let name = std::str::from_utf8(&bytes[start..end])
        .unwrap_or("")
        .to_ascii_lowercase();
    (name, end)
}

fn find_raw_text_end(bytes: &[u8], from: usize, tag_name: &str) -> Option<usize> {
    let tag_bytes = tag_name.as_bytes();
    let needle_len = 2 + tag_bytes.len();
    let mut i = from;
    while i + needle_len <= bytes.len() {
        if bytes[i] == b'<'
            && bytes[i + 1] == b'/'
            && bytes[i + 2..i + needle_len].eq_ignore_ascii_case(tag_bytes)
        {
            let next_b = bytes.get(i + needle_len).copied();
            if next_b.is_some_and(|b| b.is_ascii_whitespace() || b == b'>' || b == b'/') {
                let mut j = i + needle_len;
                while j < bytes.len() {
                    if bytes[j] == b'>' {
                        return Some(j + 1);
                    }
                    j += 1;
                }
                return Some(bytes.len());
            }
        }
        i += 1;
    }
    None
}

pub(super) fn segment_page_content(input: &str) -> (Vec<PageSegment<'_>>, bool) {
    let bytes = input.as_bytes();
    let mut segments = Vec::new();
    let mut element_stack: Vec<String> = Vec::new();
    let mut pos = 0usize;
    let mut text_start = 0usize;
    let mut html_start: Option<usize> = None;
    let mut has_html_tokens = false;

    while pos < bytes.len() {
        let Some(rel_lt) = input[pos..].find('<') else {
            break;
        };
        let lt_pos = pos + rel_lt;
        if !is_html_token_start(&input[lt_pos..]) {
            pos = lt_pos + 1;
            continue;
        }

        has_html_tokens = true;
        if html_start.is_none() {
            if lt_pos > text_start {
                segments.push(PageSegment::TopLevelText(&input[text_start..lt_pos]));
            }
            html_start = Some(lt_pos);
        }

        let tail = &input[lt_pos..];
        if tail.starts_with("<!--") {
            if tail.starts_with("<!-->") {
                pos = lt_pos + 5;
            } else if tail.starts_with("<!--->") {
                pos = lt_pos + 6;
            } else {
                let comment_body = &input[lt_pos + 4..];
                let end_std = comment_body.find("-->").map(|idx| lt_pos + 4 + idx + 3);
                let end_bang = comment_body.find("--!>").map(|idx| lt_pos + 4 + idx + 4);
                pos = match (end_std, end_bang) {
                    (Some(a), Some(b)) => a.min(b),
                    (Some(a), None) | (None, Some(a)) => a,
                    (None, None) => bytes.len(),
                };
            }
            if element_stack.is_empty()
                && let Some(start) = html_start.take()
            {
                segments.push(PageSegment::Html(&input[start..pos]));
                text_start = pos;
            }
        } else if tail.starts_with("<![CDATA[") {
            pos = if let Some(rel_end) = input[lt_pos + 9..].find("]]>") {
                lt_pos + 9 + rel_end + 3
            } else {
                bytes.len()
            };
            if element_stack.is_empty()
                && let Some(start) = html_start.take()
            {
                segments.push(PageSegment::Html(&input[start..pos]));
                text_start = pos;
            }
        } else if tail.starts_with("<!") || tail.starts_with("<?") {
            pos = if let Some(rel_end) = input[lt_pos + 2..].find('>') {
                lt_pos + 2 + rel_end + 1
            } else {
                bytes.len()
            };
            if element_stack.is_empty()
                && let Some(start) = html_start.take()
            {
                segments.push(PageSegment::Html(&input[start..pos]));
                text_start = pos;
            }
        } else if tail.starts_with("</") {
            let (tag_name, name_end) = parse_tag_name(bytes, lt_pos + 2);
            pos = if let Some(rel_gt) = input[name_end..].find('>') {
                name_end + rel_gt + 1
            } else {
                bytes.len()
            };
            if !tag_name.is_empty()
                && let Some(idx) = element_stack.iter().rposition(|t| t == &tag_name)
            {
                element_stack.truncate(idx);
            }
            if element_stack.is_empty()
                && let Some(start) = html_start.take()
            {
                segments.push(PageSegment::Html(&input[start..pos]));
                text_start = pos;
            }
        } else {
            let (tag_name, attr_start) = parse_tag_name(bytes, lt_pos + 1);
            let mut quote: Option<char> = None;
            let mut last_non_ws: Option<u8> = None;
            let mut closed_at: Option<usize> = None;
            for (rel_idx, ch) in input[attr_start..].char_indices() {
                if let Some(q) = quote {
                    if ch == q {
                        quote = None;
                        last_non_ws = Some(q as u8);
                    }
                } else if ch == '"' || ch == '\'' {
                    quote = Some(ch);
                } else if ch == '>' {
                    closed_at = Some(attr_start + rel_idx + 1);
                    break;
                } else if !ch.is_ascii_whitespace() {
                    last_non_ws = Some(if ch.is_ascii() { ch as u8 } else { b'x' });
                }
            }
            let Some(next_pos) = closed_at else {
                break;
            };
            pos = next_pos;
            let self_closing = last_non_ws == Some(b'/');

            apply_html5_implicit_close(&mut element_stack, &tag_name);
            let in_foreign = element_stack.iter().any(|t| t == "svg" || t == "math");

            if is_void_html_element(&tag_name)
                || (self_closing && (in_foreign || tag_name == "svg" || tag_name == "math"))
            {
                if element_stack.is_empty()
                    && let Some(start) = html_start.take()
                {
                    segments.push(PageSegment::Html(&input[start..pos]));
                    text_start = pos;
                }
            } else if is_raw_text_html_element(&tag_name) {
                element_stack.push(tag_name.clone());
                if let Some(end_pos) = find_raw_text_end(bytes, pos, &tag_name) {
                    pos = end_pos;
                    element_stack.pop();
                } else {
                    pos = bytes.len();
                }
                if element_stack.is_empty()
                    && let Some(start) = html_start.take()
                {
                    segments.push(PageSegment::Html(&input[start..pos]));
                    text_start = pos;
                }
            } else {
                element_stack.push(tag_name);
            }
        }
    }

    if let Some(start) = html_start.take() {
        if start < bytes.len() {
            segments.push(PageSegment::Html(&input[start..]));
        }
    } else if text_start < bytes.len() {
        segments.push(PageSegment::TopLevelText(&input[text_start..]));
    }

    (segments, has_html_tokens)
}

pub(super) fn looks_like_mixed_plain_url(line: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.chars().any(char::is_whitespace) {
        return false;
    }
    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("data:") {
        return true;
    }
    if let Some(rest) = trimmed.strip_prefix("//") {
        return !rest.is_empty();
    }
    if let Some(rest) = trimmed.strip_prefix("../") {
        return !rest.is_empty();
    }
    if let Some(rest) = trimmed.strip_prefix("./") {
        return !rest.is_empty();
    }
    if let Some(rest) = trimmed.strip_prefix('/') {
        return !rest.is_empty();
    }
    let path_part = trimmed.split(['?', '#']).next().unwrap_or(trimmed);
    if path_part.contains('/') {
        return true;
    }
    path_part.rsplit_once('.').is_some_and(|(stem, ext)| {
        !stem.is_empty()
            && (2..=8).contains(&ext.len())
            && ext.bytes().all(|b| b.is_ascii_alphanumeric())
    })
}

fn is_invalid_image_candidate(raw: &str) -> bool {
    let t = raw.trim();
    if t.is_empty() || t == "#" || t.starts_with('#') {
        return true;
    }
    let lower = t.to_ascii_lowercase();
    lower.starts_with("javascript:")
        || lower.starts_with("about:")
        || lower.starts_with("data:image/svg+xml")
}

pub(crate) fn extract_img_srcs_from_html(html_fragment: &str, out: &mut Vec<String>) {
    let Ok(img_sel) = Selector::parse("img") else {
        return;
    };
    let doc = Html::parse_fragment(html_fragment);
    for el in doc.select(&img_sel) {
        let attr = ["data-url", "data-original", "data-src", "src"]
            .into_iter()
            .filter_map(|name| el.value().attr(name))
            .map(str::trim)
            .find(|val| !is_invalid_image_candidate(val));
        if let Some(val) = attr {
            out.push(val.to_string());
        }
    }
}

pub(super) fn normalize_extracted_urls(candidates: Vec<String>, base_url: &str) -> Vec<String> {
    let mut out = Vec::with_capacity(candidates.len());
    for raw in candidates {
        let t = raw.trim();
        if is_invalid_image_candidate(t) {
            continue;
        }
        let resolved = resolve_relative_url(t, base_url);
        if !resolved.trim().is_empty() {
            out.push(resolved);
        }
    }
    out
}

pub(super) fn apply_replace_regex(content: &str, replace_rule: &str) -> String {
    let mut out = content.to_string();
    for line in replace_rule.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some((pat, rep)) = trimmed.strip_prefix("##").and_then(|s| s.split_once("##"))
            && let Some(re) = cached_regex(pat)
        {
            out = re.replace_all(&out, rep).into_owned();
        } else if let Some(re) = cached_regex(trimmed) {
            out = re.replace_all(&out, "").into_owned();
        }
    }
    out
}
