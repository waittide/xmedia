use bytes::Bytes;
use xmedia_formats::common::xml_escape_clean as xml_escape;

use crate::application::ports::CoverPayload;

pub fn generate_placeholder_svg_cover(
    title: &str,
    author: Option<&str>,
    badge: &str,
) -> CoverPayload {
    let hash = title.bytes().fold(5381u32, |acc, b| {
        acc.wrapping_mul(33).wrapping_add(u32::from(b))
    });
    let palettes = [
        ("#1e1e2e", "#89b4fa", "#cdd6f4"),
        ("#181825", "#cba6f7", "#cdd6f4"),
        ("#1e2030", "#8aadf4", "#cad3f5"),
        ("#232634", "#81c8be", "#c6d0f5"),
        ("#1e1e2e", "#fab387", "#cdd6f4"),
        ("#1e1e2e", "#a6e3a1", "#cdd6f4"),
    ];
    let (bg, accent, fg) = palettes[(hash as usize) % palettes.len()];

    let clean_title: Vec<String> = title
        .chars()
        .collect::<Vec<_>>()
        .chunks(14)
        .take(3)
        .map(|chunk| xml_escape(&chunk.iter().collect::<String>()))
        .collect();
    let line1 = clean_title
        .first()
        .map(String::as_str)
        .unwrap_or("Untitled");
    let line2 = clean_title.get(1).map(String::as_str).unwrap_or("");
    let line3 = clean_title.get(2).map(String::as_str).unwrap_or("");
    let author_text = xml_escape(author.unwrap_or("xmedia library"));
    let badge_text = xml_escape(badge);

    let svg = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 360 500" width="360" height="500">
  <rect width="360" height="500" rx="18" fill="{bg}"/>
  <rect x="24" y="24" width="8" height="452" rx="4" fill="{accent}" opacity="0.85"/>
  <rect x="270" y="30" width="62" height="28" rx="14" fill="{accent}" opacity="0.2"/>
  <text x="301" y="49" text-anchor="middle" font-family="system-ui, sans-serif" font-size="13" font-weight="700" fill="{accent}">{badge_text}</text>
  <text x="52" y="175" font-family="system-ui, sans-serif" font-size="24" font-weight="700" fill="{fg}">{line1}</text>
  <text x="52" y="212" font-family="system-ui, sans-serif" font-size="24" font-weight="700" fill="{fg}">{line2}</text>
  <text x="52" y="249" font-family="system-ui, sans-serif" font-size="24" font-weight="700" fill="{fg}">{line3}</text>
  <text x="52" y="444" font-family="system-ui, sans-serif" font-size="15" fill="{accent}">{author_text}</text>
</svg>"##
    );

    CoverPayload::trusted_generated_svg(Bytes::from(svg.into_bytes()))
}
