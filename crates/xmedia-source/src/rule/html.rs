use scraper::{ElementRef, Html, Selector};

use crate::http::resolve_relative_url;

fn strip_css_prefix(rule: &str) -> &str {
    if rule
        .as_bytes()
        .get(..5)
        .is_some_and(|b| b.eq_ignore_ascii_case(b"@css:"))
    {
        rule[5..].trim()
    } else {
        rule.trim()
    }
}

pub(super) fn eval_html_list(html_str: &str, rule: &str) -> Vec<String> {
    let doc = Html::parse_fragment(html_str);
    let clean = strip_css_prefix(rule);

    let elements = select_elements_chained(&doc, clean);
    elements.into_iter().map(|el| el.html()).collect()
}

pub(super) fn eval_html_single(html_str: &str, rule: &str, base_url: &str) -> String {
    let doc = Html::parse_fragment(html_str);
    let clean = strip_css_prefix(rule);

    let (selector_part, attr_part) = if let Some((left, right)) = clean.rsplit_once('@') {
        if is_attr_extractor(right.trim()) {
            (left.trim(), right.trim())
        } else {
            (clean, "text")
        }
    } else if is_attr_extractor(clean) {
        ("", clean)
    } else {
        (clean, "text")
    };

    let elements: Vec<ElementRef<'_>> = if selector_part.is_empty() {
        doc.root_element()
            .children()
            .filter_map(ElementRef::wrap)
            .collect()
    } else {
        select_elements_chained(&doc, selector_part)
    };

    if elements.is_empty() {
        return String::new();
    }

    let values: Vec<String> = elements
        .into_iter()
        .filter_map(|el| extract_attr_from_element(el, attr_part, base_url))
        .filter(|s| !s.is_empty())
        .collect();

    values.join("\n")
}

fn is_attr_extractor(s: &str) -> bool {
    if s.is_empty() || s.contains(' ') || s.contains('.') || s.contains('#') || s.contains('>') {
        return false;
    }
    let lower = s.to_ascii_lowercase();
    matches!(
        lower.as_str(),
        "text"
            | "textnodes"
            | "owntext"
            | "html"
            | "all"
            | "href"
            | "src"
            | "srcset"
            | "content"
            | "title"
            | "alt"
            | "value"
            | "id"
            | "class"
            | "name"
            | "rel"
            | "type"
            | "style"
            | "poster"
            | "action"
            | "target"
            | "placeholder"
            | "datetime"
            | "lang"
            | "property"
    ) || lower.starts_with("data-")
        || lower.starts_with("aria-")
}

fn extract_attr_from_element(el: ElementRef<'_>, attr: &str, base_url: &str) -> Option<String> {
    let lower = attr.to_ascii_lowercase();
    match lower.as_str() {
        "text" => {
            let txt = el.text().collect::<Vec<_>>().join("");
            Some(txt.trim().to_string())
        }
        "textnodes" => {
            let lines: Vec<String> = el
                .text()
                .map(|t| t.trim())
                .filter(|t| !t.is_empty())
                .map(str::to_owned)
                .collect();
            Some(lines.join("\n"))
        }
        "owntext" => {
            let txt: String = el
                .children()
                .filter_map(|c| c.value().as_text().map(|t| t.text.as_ref()))
                .collect::<Vec<_>>()
                .join("");
            Some(txt.trim().to_string())
        }
        "html" => Some(el.inner_html()),
        "all" => Some(el.html()),
        "href" | "src" | "data-src" | "data-original" | "data-url" => el
            .value()
            .attr(lower.as_str())
            .map(|v| resolve_relative_url(v, base_url)),
        other => el.value().attr(other).map(|v| v.trim().to_string()),
    }
}

fn select_elements_chained<'a>(doc: &'a Html, chain: &str) -> Vec<ElementRef<'a>> {
    let segments: Vec<&str> = chain
        .split('@')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    if segments.is_empty() {
        return Vec::new();
    }

    let mut current: Vec<ElementRef<'a>> = vec![doc.root_element()];

    for seg in segments {
        let mut next = Vec::new();
        let (css_expr, index_filter) = parse_legado_html_segment(seg);
        if css_expr == "__children__" {
            for parent in &current {
                for child in parent.children().filter_map(ElementRef::wrap) {
                    next.push(child);
                }
            }
        } else if let Ok(selector) = Selector::parse(&css_expr) {
            for parent in &current {
                for matched in parent.select(&selector) {
                    next.push(matched);
                }
            }
        }
        current = apply_index_filter(next, index_filter);
        if current.is_empty() {
            break;
        }
    }

    current
}

#[derive(Debug, Clone)]
enum IndexFilter {
    None,
    Single(isize),
    Exclude(Vec<isize>),
    Slice(Option<isize>, Option<isize>),
}

/// Translates Legado shorthand (`class.item.0`, `tag.a.-1`, `id.content`, `children`, `div.list[0:5]`, `tr[!0]`)
/// into a standard CSS selector plus optional index filter.
fn parse_legado_html_segment(seg: &str) -> (String, IndexFilter) {
    let mut s = seg.trim();
    let mut filter = IndexFilter::None;

    if let Some(open) = s.rfind('[')
        && s.ends_with(']')
    {
        let inside = s[open + 1..s.len() - 1].trim();
        if let Ok(idx) = inside.parse::<isize>() {
            filter = IndexFilter::Single(idx);
            s = s[..open].trim();
        } else if let Some(excl) = inside.strip_prefix('!')
            && !excl.trim().is_empty()
            && excl.split(',').all(|p| p.trim().parse::<isize>().is_ok())
        {
            let ids = excl
                .split(',')
                .filter_map(|p| p.trim().parse::<isize>().ok())
                .collect();
            filter = IndexFilter::Exclude(ids);
            s = s[..open].trim();
        } else if !inside.contains('=')
            && !inside.contains('"')
            && !inside.contains('\'')
            && let Some((a, b)) = inside.split_once(':')
            && (a.trim().is_empty() || a.trim().parse::<isize>().is_ok())
            && (b.trim().is_empty() || b.trim().parse::<isize>().is_ok())
        {
            filter = IndexFilter::Slice(
                a.trim().parse::<isize>().ok(),
                b.trim().parse::<isize>().ok(),
            );
            s = s[..open].trim();
        }
    }

    if s.eq_ignore_ascii_case("children") {
        return ("__children__".to_string(), filter);
    }

    if let Some(rest) = s.strip_prefix("class.") {
        let (name, idx_opt) = split_trailing_dot_index(rest);
        if let Some(idx) = idx_opt {
            filter = IndexFilter::Single(idx);
        }
        let css = name
            .split_whitespace()
            .filter(|c| !c.is_empty())
            .map(|c| format!(".{c}"))
            .collect::<String>();
        return (css, filter);
    }

    if let Some(rest) = s.strip_prefix("id.") {
        let (name, idx_opt) = split_trailing_dot_index(rest);
        if let Some(idx) = idx_opt {
            filter = IndexFilter::Single(idx);
        }
        return (format!("#{name}"), filter);
    }

    if let Some(rest) = s.strip_prefix("tag.") {
        let (name, idx_opt) = split_trailing_dot_index(rest);
        if let Some(idx) = idx_opt {
            filter = IndexFilter::Single(idx);
        }
        return (name.to_string(), filter);
    }

    let (clean_css, idx_opt) = split_trailing_dot_index(s);
    if let Some(idx) = idx_opt {
        filter = IndexFilter::Single(idx);
    }
    (clean_css.to_string(), filter)
}

fn split_trailing_dot_index(s: &str) -> (&str, Option<isize>) {
    if let Some((head, tail)) = s.rsplit_once('.')
        && !head.is_empty()
        && let Ok(idx) = tail.parse::<isize>()
    {
        return (head, Some(idx));
    }
    (s, None)
}

fn apply_index_filter<T>(items: Vec<T>, filter: IndexFilter) -> Vec<T> {
    let len = items.len();
    match filter {
        IndexFilter::None => items,
        IndexFilter::Single(idx) => {
            let resolved = if idx < 0 {
                len.checked_sub(idx.unsigned_abs())
            } else {
                Some(idx as usize)
            };
            resolved
                .and_then(|i| items.into_iter().nth(i))
                .into_iter()
                .collect()
        }
        IndexFilter::Exclude(excluded) => {
            let mut bad = std::collections::HashSet::new();
            for idx in excluded {
                let resolved = if idx < 0 {
                    len.checked_sub(idx.unsigned_abs())
                } else {
                    Some(idx as usize)
                };
                if let Some(i) = resolved {
                    bad.insert(i);
                }
            }
            items
                .into_iter()
                .enumerate()
                .filter_map(|(i, v)| (!bad.contains(&i)).then_some(v))
                .collect()
        }
        IndexFilter::Slice(start, end) => {
            let ilen = len as isize;
            let s = start
                .map(|v| {
                    if v < 0 {
                        (ilen + v).max(0)
                    } else {
                        v.min(ilen)
                    }
                })
                .unwrap_or(0) as usize;
            let e = end
                .map(|v| {
                    if v < 0 {
                        (ilen + v).max(0)
                    } else {
                        v.min(ilen)
                    }
                })
                .unwrap_or(ilen) as usize;
            items
                .into_iter()
                .enumerate()
                .filter_map(|(i, v)| (i >= s && i < e).then_some(v))
                .collect()
        }
    }
}
