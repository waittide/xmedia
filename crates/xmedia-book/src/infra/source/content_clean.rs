use std::sync::LazyLock;

use regex::Regex;

use crate::infra::source::rule::cached_regex;

static RE_COMMENT_IMG: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?is)<div\s+rs-native[^>]*>.*?</div>|<img[^>]+(?:data:image/svg\+xml|qtbzs|showCmt|chapter_review_svg)[^>]*>"#,
    )
    .expect("valid comment img regex")
});
static RE_SCRIPT_STYLE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?is)<(?:script|style|comment)[^>]*>.*?</(?:script|style|comment)>|<comment[^>]*/>",
    )
    .expect("valid script style regex")
});
static RE_HTML_BREAKS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)<br\s*/?>|</p>|</div>|</h[1-6]>|</li>|</blockquote>")
        .expect("valid html breaks regex")
});
static RE_HTML_TAGS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?is)<[^>]+>").expect("valid html tags regex"));

pub(super) fn apply_content_replace_regex(content: &str, replace_rule: &str) -> String {
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

pub fn clean_chapter_html_to_text(raw: &str) -> String {
    if raw.trim().is_empty() {
        return String::new();
    }

    let s = RE_COMMENT_IMG.replace_all(raw, "");
    let s = RE_SCRIPT_STYLE.replace_all(&s, "");
    let s = RE_HTML_BREAKS.replace_all(&s, "\n");
    let s = RE_HTML_TAGS.replace_all(&s, "");

    let decoded = s
        .replace("&nbsp;", " ")
        .replace("&#160;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'");

    decoded
        .lines()
        .map(|l| l.trim().trim_start_matches('\u{3000}').trim())
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}
