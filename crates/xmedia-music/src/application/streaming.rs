use std::sync::Arc;

use uuid::Uuid;
use xmedia_core::application::bindings::MediaBindingService;
use xmedia_core::application::ports::{BinaryPayload, ByteRangeSpec, ByteRangeStream};
use xmedia_core::domain::error::{DomainError, DomainResult};
use xmedia_core::domain::library::MediaKind;
use xmedia_core::domain::repository::LibraryRepository;
use xmedia_core::domain::user::{User, UserId};

use crate::application::ports::{
    AudioTranscoderPort, FfmpegStatusDto, LyricsResolverPort, MusicStoragePort, TranscodeFormat,
    TranscodeRequest, TranscodedAudioStream,
};
use crate::domain::catalog::{
    AlbumId, ArtistId, AudioFormat, SortOrder, StructuredLyrics, Track, TrackId, TrackQuery,
    TrackSortField, TrackSource,
};
use crate::domain::collection::PlaylistId;
use crate::domain::repository::{
    AlbumScope, ArtistScope, MusicCatalogRepository, MusicCollectionRepository, PlaylistQuery,
    TrackScope,
};

pub enum ResolvedAudioStream {
    RawRange {
        track: Track,
        range_stream: ByteRangeStream,
        is_partial: bool,
    },
    Transcoded {
        track: Track,
        transcoded: TranscodedAudioStream,
    },
}

#[derive(Clone)]
pub struct MusicStreamingService {
    libraries: Arc<dyn LibraryRepository>,
    catalog: Arc<dyn MusicCatalogRepository>,
    collections: Arc<dyn MusicCollectionRepository>,
    bindings: Arc<MediaBindingService>,
    storage: Arc<dyn MusicStoragePort>,
    lyrics: Arc<dyn LyricsResolverPort>,
    transcoder: Arc<dyn AudioTranscoderPort>,
}

impl MusicStreamingService {
    pub(crate) fn new(
        libraries: Arc<dyn LibraryRepository>,
        catalog: Arc<dyn MusicCatalogRepository>,
        collections: Arc<dyn MusicCollectionRepository>,
        bindings: Arc<MediaBindingService>,
        storage: Arc<dyn MusicStoragePort>,
        lyrics: Arc<dyn LyricsResolverPort>,
        transcoder: Arc<dyn AudioTranscoderPort>,
    ) -> Self {
        Self {
            libraries,
            catalog,
            collections,
            bindings,
            storage,
            lyrics,
            transcoder,
        }
    }

    pub async fn get_ffmpeg_status(&self) -> FfmpegStatusDto {
        self.transcoder.check_status().await
    }

    pub async fn open_raw_download(
        &self,
        user_id: UserId,
        track_id: TrackId,
        spec: ByteRangeSpec,
    ) -> DomainResult<ResolvedAudioStream> {
        let (track, library) = self.load_bound_track_and_library(user_id, track_id).await?;
        let range_stream = self
            .storage
            .open_audio_stream(&library, &track, spec)
            .await?;
        Ok(ResolvedAudioStream::RawRange {
            track,
            range_stream,
            is_partial: spec.is_partial(),
        })
    }

    pub async fn open_stream(
        &self,
        user_id: UserId,
        track_id: TrackId,
        requested_format: Option<&str>,
        max_bit_rate_kbps: Option<u32>,
        time_offset_secs: Option<u32>,
        spec: ByteRangeSpec,
    ) -> DomainResult<ResolvedAudioStream> {
        let (track, library) = self.load_bound_track_and_library(user_id, track_id).await?;

        let norm_format = requested_format
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_ascii_lowercase);
        let max_br = max_bit_rate_kbps.filter(|&b| b > 0);
        let offset = time_offset_secs.filter(|&t| t > 0);

        let bitrate_fits = max_br.is_none_or(|limit_kbps| {
            !track.format.is_lossless() && track.bit_rate_kbps.is_some_and(|src| src <= limit_kbps)
        });

        let wants_raw = match norm_format.as_deref() {
            Some("raw") => offset.is_none(),
            Some(fmt) => {
                offset.is_none() && format_matches_source(fmt, track.format) && bitrate_fits
            }
            None => offset.is_none() && bitrate_fits,
        };

        if wants_raw {
            let range_stream = self
                .storage
                .open_audio_stream(&library, &track, spec)
                .await?;
            return Ok(ResolvedAudioStream::RawRange {
                track,
                range_stream,
                is_partial: spec.is_partial(),
            });
        }

        let target_format = match norm_format.as_deref() {
            Some(fmt) if fmt != "raw" => TranscodeFormat::parse(fmt)?,
            _ => TranscodeFormat::Mp3,
        };

        let source_path = self.storage.resolve_audio_fs_path(&library, &track).await?;
        let transcoded = self
            .transcoder
            .transcode_stream(
                &source_path,
                track.duration_secs,
                &TranscodeRequest {
                    target_format,
                    max_bit_rate_kbps: max_br,
                    time_offset_secs: offset,
                },
            )
            .await?;

        Ok(ResolvedAudioStream::Transcoded { track, transcoded })
    }

    pub async fn get_lyrics_by_track(
        &self,
        user_id: UserId,
        track_id: TrackId,
    ) -> DomainResult<StructuredLyrics> {
        let (track, library) = self.load_bound_track_and_library(user_id, track_id).await?;

        let sidecar = self
            .storage
            .read_sidecar_lrc_for_track(&library, &track)
            .await?;
        let audio_path = self
            .storage
            .resolve_audio_fs_path(&library, &track)
            .await
            .ok();

        let lyrics_resolver = self.lyrics.clone();
        let mut resolved = tokio::task::spawn_blocking(move || {
            lyrics_resolver.resolve_lyrics(audio_path.as_deref(), sidecar.as_deref())
        })
        .await
        .map_err(|e| DomainError::Storage(format!("lyrics resolve task failed: {e}")))??;
        if resolved.display_artist.is_none() {
            resolved.display_artist = Some(track.artist_name);
        }
        if resolved.display_title.is_none() {
            resolved.display_title = Some(track.title);
        }
        Ok(resolved)
    }

    pub async fn get_lyrics_by_artist_title(
        &self,
        user_id: UserId,
        artist: Option<&str>,
        title: Option<&str>,
    ) -> DomainResult<StructuredLyrics> {
        let title_q = title.unwrap_or_default().trim();
        let artist_q = artist.unwrap_or_default().trim();
        let search_term = if !title_q.is_empty() {
            title_q
        } else if !artist_q.is_empty() {
            artist_q
        } else {
            return Ok(StructuredLyrics::default());
        };
        let allowed = self
            .bindings
            .list_bound_ids(user_id, MediaKind::Music)
            .await?
            .into_iter()
            .map(TrackId)
            .collect();
        let candidates = self
            .catalog
            .query_tracks(TrackScope::Offset {
                allowed: &allowed,
                query: &TrackQuery {
                    search: Some(search_term.to_string()),
                    sort_by: TrackSortField::Title,
                    sort_order: SortOrder::Asc,
                    ..Default::default()
                },
                offset: 0,
                limit: 25,
            })
            .await?;

        let artist_lower = artist_q.to_lowercase();
        let title_lower = title_q.to_lowercase();

        let mut prioritized: Vec<&Track> = candidates
            .iter()
            .filter(|t| {
                t.has_lyrics()
                    && (title_lower.is_empty() || t.title.to_lowercase() == title_lower)
                    && (artist_lower.is_empty()
                        || t.artist_name.to_lowercase().contains(&artist_lower))
            })
            .collect();
        for tr in &candidates {
            if tr.has_lyrics() && !prioritized.iter().any(|p| p.id == tr.id) {
                prioritized.push(tr);
            }
        }

        for track in prioritized {
            let lyr = self.get_lyrics_by_track(user_id, track.id).await?;
            if !lyr.is_empty() {
                return Ok(lyr);
            }
        }
        Ok(StructuredLyrics::default())
    }

    pub async fn get_cover_art(
        &self,
        actor: &User,
        raw_id: &str,
        max_size: Option<u32>,
    ) -> DomainResult<BinaryPayload> {
        let clean = raw_id.trim();

        if let Ok(uuid) = Uuid::parse_str(clean) {
            if let Some(album) = self
                .catalog
                .query_albums(AlbumScope::Id(AlbumId(uuid)))
                .await?
                .items
                .into_iter()
                .next()
            {
                let alb_tracks = self
                    .catalog
                    .query_tracks(TrackScope::Album {
                        album_id: album.id,
                        allowed: None,
                    })
                    .await?;
                self.authorize_entity_cover_tracks(actor.id, &alb_tracks)
                    .await?;
                if let Some(ref key) = album.cover_key {
                    return self.storage.read_cover(key, max_size).await;
                }
            }
            if let Some(track) = self
                .catalog
                .query_tracks(TrackScope::Id(TrackId(uuid)))
                .await?
                .into_iter()
                .find(|t| !t.is_missing)
            {
                if matches!(track.source, TrackSource::OnlineTracked(_)) {
                    self.bindings
                        .require_bound(actor.id, MediaKind::Music, track.id.0, "track not found")
                        .await?;
                }
                if let Some(ref key) = track.cover_key {
                    return self.storage.read_cover(key, max_size).await;
                }
            }
            if let Some(artist) = self
                .catalog
                .query_artists(ArtistScope::Id(ArtistId(uuid)))
                .await?
                .items
                .into_iter()
                .next()
            {
                let ar_tracks = self
                    .catalog
                    .query_tracks(TrackScope::Artist {
                        artist_id: artist.id,
                        allowed: None,
                    })
                    .await?;
                self.authorize_entity_cover_tracks(actor.id, &ar_tracks)
                    .await?;
                if let Some(ref key) = artist.cover_key {
                    return self.storage.read_cover(key, max_size).await;
                }
            }
            if let Some(pl) = self
                .collections
                .query_playlists(PlaylistQuery::Id(PlaylistId(uuid)))
                .await?
                .into_iter()
                .next()
            {
                if !pl.is_public && pl.owner_id != actor.id && !actor.role.is_admin() {
                    return Err(DomainError::Forbidden(
                        "not authorized to access private playlist cover".into(),
                    ));
                }
                let bound = self
                    .bindings
                    .list_bound_ids(actor.id, MediaKind::Music)
                    .await?;
                for &track_id in &pl.track_ids {
                    if !bound.contains(&track_id.0) {
                        continue;
                    }
                    if let Some(track) = self
                        .catalog
                        .query_tracks(TrackScope::Id(track_id))
                        .await?
                        .into_iter()
                        .find(|t| !t.is_missing)
                        && let Some(ref key) = track.cover_key
                    {
                        return self.storage.read_cover(key, max_size).await;
                    }
                }
                return Err(DomainError::NotFound("playlist has no cover art".into()));
            }
        }

        let matched_tracks = self
            .catalog
            .query_tracks(TrackScope::CoverKey(clean))
            .await?;
        let mut matched_pool = false;
        let mut matched_tracked_tracks: Vec<TrackId> = Vec::new();
        for tr in matched_tracks {
            if matches!(tr.source, TrackSource::OnlineTracked(_)) {
                matched_tracked_tracks.push(tr.id);
            } else {
                matched_pool = true;
                break;
            }
        }
        if !matched_pool {
            let matched_albums = self
                .catalog
                .query_albums(AlbumScope::CoverKey(clean))
                .await?
                .items;
            for alb in matched_albums {
                let alb_tracks = self
                    .catalog
                    .query_tracks(TrackScope::Album {
                        album_id: alb.id,
                        allowed: None,
                    })
                    .await?;
                for tr in alb_tracks {
                    if matches!(tr.source, TrackSource::OnlineTracked(_)) {
                        matched_tracked_tracks.push(tr.id);
                    } else {
                        matched_pool = true;
                        break;
                    }
                }
                if matched_pool {
                    break;
                }
            }
        }
        if !matched_pool && !matched_tracked_tracks.is_empty() {
            let bound = self
                .bindings
                .list_bound_ids(actor.id, MediaKind::Music)
                .await?;
            if !matched_tracked_tracks
                .iter()
                .any(|tid| bound.contains(&tid.0))
            {
                return Err(DomainError::NotFound("cover art not found".into()));
            }
        }

        self.storage.read_cover(clean, max_size).await
    }

    async fn load_bound_track_and_library(
        &self,
        user_id: UserId,
        track_id: TrackId,
    ) -> DomainResult<(Track, xmedia_core::domain::library::Library)> {
        self.bindings
            .require_bound(user_id, MediaKind::Music, track_id.0, "track not found")
            .await?;
        let track = self
            .catalog
            .query_tracks(TrackScope::Id(track_id))
            .await?
            .into_iter()
            .find(|t| !t.is_missing)
            .ok_or_else(|| DomainError::NotFound("track not found".into()))?;
        let library = self
            .libraries
            .find_by_id(track.library_id)
            .await?
            .ok_or_else(|| DomainError::NotFound("library not found".into()))?;
        Ok((track, library))
    }

    async fn authorize_entity_cover_tracks(
        &self,
        user_id: UserId,
        tracks: &[Track],
    ) -> DomainResult<()> {
        let has_local_pool_track = tracks
            .iter()
            .any(|t| !matches!(t.source, TrackSource::OnlineTracked(_)));
        if !has_local_pool_track {
            let bound = self
                .bindings
                .list_bound_ids(user_id, MediaKind::Music)
                .await?;
            if !tracks.iter().any(|t| bound.contains(&t.id.0)) {
                return Err(DomainError::NotFound("cover art not found".into()));
            }
        }
        Ok(())
    }
}

fn format_matches_source(fmt: &str, source_fmt: AudioFormat) -> bool {
    matches!(
        (fmt, source_fmt),
        ("mp3", AudioFormat::Mp3)
            | ("flac", AudioFormat::Flac)
            | ("opus", AudioFormat::Opus)
            | ("ogg" | "oga", AudioFormat::Ogg)
            | ("m4a" | "mp4", AudioFormat::M4a)
            | ("alac", AudioFormat::Alac)
            | ("aac", AudioFormat::Aac | AudioFormat::M4a)
            | ("wav" | "wave", AudioFormat::Wav)
            | ("aiff" | "aif" | "aifc", AudioFormat::Aiff)
            | ("ape", AudioFormat::Ape)
            | ("wv" | "wavpack", AudioFormat::Wv)
    )
}
