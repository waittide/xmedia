use std::path::Path;

use xmedia_core::domain::error::DomainResult;
use xmedia_formats::music::{
    StructuredLyrics as FormatStructuredLyrics, parse_lyrics_str as format_parse_lyrics_str,
    resolve_hybrid_lyrics,
};

use crate::application::ports::LyricsResolverPort;
use crate::domain::catalog::{LyricLine, StructuredLyrics};

#[derive(Default)]
pub struct HybridLyricsResolver;

impl LyricsResolverPort for HybridLyricsResolver {
    fn resolve_lyrics(
        &self,
        audio_path: Option<&str>,
        sidecar_lrc_bytes: Option<&[u8]>,
    ) -> DomainResult<StructuredLyrics> {
        let resolved = resolve_hybrid_lyrics(audio_path.map(Path::new), sidecar_lrc_bytes);
        Ok(map_structured_lyrics(resolved))
    }
}

pub fn parse_lyrics_str(input: &str) -> StructuredLyrics {
    map_structured_lyrics(format_parse_lyrics_str(input))
}

fn map_structured_lyrics(src: FormatStructuredLyrics) -> StructuredLyrics {
    StructuredLyrics {
        lang: src.lang,
        synced: src.synced,
        display_artist: src.display_artist,
        display_title: src.display_title,
        offset_ms: src.offset_ms,
        lines: src
            .lines
            .into_iter()
            .map(|l| LyricLine {
                start_ms: l.start_ms,
                text: l.text,
            })
            .collect(),
        plain_text: src.plain_text,
    }
}
