use std::path::Path;

use xmedia_core::domain::error::DomainResult;
use xmedia_formats::music::resolve_hybrid_lyrics;

use crate::application::ports::LyricsResolverPort;
use crate::domain::catalog::StructuredLyrics;

#[derive(Default)]
pub struct HybridLyricsResolver;

impl LyricsResolverPort for HybridLyricsResolver {
    fn resolve_lyrics(
        &self,
        audio_path: Option<&str>,
        sidecar_lrc_bytes: Option<&[u8]>,
    ) -> DomainResult<StructuredLyrics> {
        let resolved = resolve_hybrid_lyrics(audio_path.map(Path::new), sidecar_lrc_bytes);
        Ok(StructuredLyrics::from(resolved))
    }
}
