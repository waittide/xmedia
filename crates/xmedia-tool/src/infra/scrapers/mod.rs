pub mod bangumi;
pub mod http_client;
pub mod itunes;
pub mod lrclib;
pub mod musicbrainz;
pub mod openlibrary;
pub mod tmdb;

use std::sync::Arc;

use crate::domain::ports::ScraperProviderPort;

/// Builds the default registry of pluggable `ScraperProviderPort` implementations.
///
/// Extending `xmedia-tool` with a new scraper source requires only:
/// 1. Implementing `ScraperProviderPort` in `infra/scrapers/<new_source>.rs`;
/// 2. Appending `Arc::new(...)` here.
///
/// Both the HTTP API (`/api/v1/tool/scrapers/providers`) and the Web UI discover new sources automatically.
pub fn build_default_scraper_providers() -> Vec<Arc<dyn ScraperProviderPort>> {
    vec![
        Arc::new(itunes::ItunesMusicProvider),
        Arc::new(musicbrainz::MusicBrainzProvider),
        Arc::new(lrclib::LrclibProvider),
        Arc::new(openlibrary::OpenLibraryProvider),
        Arc::new(bangumi::BangumiProvider),
        Arc::new(tmdb::TmdbProvider),
    ]
}

#[cfg(test)]
mod tests {
    use crate::domain::config::ProviderCredentials;
    use crate::domain::scraper::ScrapeTargetKind;
    use crate::infra::scrapers::bangumi::{BgmSubject, map_bgm_detail};
    use crate::infra::scrapers::build_default_scraper_providers;
    use crate::infra::scrapers::itunes::{ItunesItem, map_itunes_detail};
    use crate::infra::scrapers::lrclib::LrclibRecord;
    use crate::infra::scrapers::musicbrainz::{MbRecordingItem, map_recording_detail};
    use crate::infra::scrapers::openlibrary::{OlSearchDoc, map_ol_doc_to_candidate};
    use crate::infra::scrapers::tmdb::{
        TmdbDetailBody, TmdbEpisodeBody, map_tmdb_detail, parse_tmdb_external_id,
    };

    #[test]
    fn default_registry_exposes_six_providers_and_checks_tmdb_credentials() {
        let providers = build_default_scraper_providers();
        assert_eq!(providers.len(), 6);

        let empty_creds = ProviderCredentials::default();
        let configured_ids: Vec<String> = providers
            .iter()
            .filter(|p| p.is_configured(&empty_creds))
            .map(|p| p.descriptor().id)
            .collect();
        assert_eq!(
            configured_ids,
            vec!["itunes", "musicbrainz", "lrclib", "openlibrary", "bangumi"]
        );

        let with_tmdb = ProviderCredentials {
            tmdb_api_key: Some("0123456789abcdef0123456789abcdef".into()),
            ..Default::default()
        };
        assert!(providers.iter().all(|p| p.is_configured(&with_tmdb)));
    }

    #[test]
    fn provider_payload_mappers_normalize_all_sources_cleanly() {
        // 0. Apple Music / iTunes
        let itunes_item: ItunesItem = serde_json::from_str(
            r#"{
                "wrapperType": "track",
                "trackId": 535824738,
                "collectionName": "葉惠美",
                "trackName": "晴天",
                "artistName": "周杰倫",
                "artworkUrl100": "https://is1-ssl.mzstatic.com/image/thumb/Music115/JAY.jpg/100x100bb.jpg",
                "releaseDate": "2003-07-31T07:00:00Z",
                "trackNumber": 3,
                "discNumber": 1,
                "primaryGenreName": "Mandopop"
            }"#,
        )
        .unwrap();
        let it_meta = map_itunes_detail(itunes_item, "TW:535824738", ScrapeTargetKind::MusicTrack);
        assert_eq!(it_meta.title.as_deref(), Some("晴天"));
        assert_eq!(it_meta.track_or_episode_number, Some(3));
        assert_eq!(
            it_meta.cover_url.as_deref(),
            Some("https://is1-ssl.mzstatic.com/image/thumb/Music115/JAY.jpg/1000x1000bb.jpg")
        );
        // 1. LRCLIB
        let lrc: LrclibRecord = serde_json::from_str(
            r#"{
                "id": 42,
                "trackName": "晴天",
                "artistName": "周杰伦",
                "albumName": "叶惠美",
                "duration": 269.5,
                "syncedLyrics": "[00:28.90]故事的小黄花",
                "plainLyrics": "故事的小黄花"
            }"#,
        )
        .unwrap();
        let lrc_meta = lrc.into_metadata(ScrapeTargetKind::Lyrics);
        assert_eq!(lrc_meta.title.as_deref(), Some("晴天"));
        assert_eq!(lrc_meta.creators, vec!["周杰伦"]);
        assert_eq!(
            lrc_meta.synced_lyrics.as_deref(),
            Some("[00:28.90]故事的小黄花")
        );

        // 2. MusicBrainz + Cover Art Archive
        let mb: MbRecordingItem = serde_json::from_str(
            r#"{
                "id": "rec-123",
                "title": "Blue Bird",
                "first-release-date": "2008-07-09",
                "artist-credit": [{"name": "いきものがかり"}],
                "releases": [{
                    "id": "rel-999",
                    "title": "My song Your song",
                    "media": [{"position": 1, "track": [{"number": "3", "position": 3}]}]
                }],
                "genres": [{"name": "j-pop"}]
            }"#,
        )
        .unwrap();
        let mb_meta = map_recording_detail(mb);
        assert_eq!(
            mb_meta.series_or_album.as_deref(),
            Some("My song Your song")
        );
        assert_eq!(mb_meta.track_or_episode_number, Some(3));
        assert_eq!(mb_meta.year, Some(2008));
        assert_eq!(
            mb_meta.cover_url.as_deref(),
            Some("https://coverartarchive.org/release/rel-999/front-500")
        );

        // 3. OpenLibrary
        let ol: OlSearchDoc = serde_json::from_str(
            r#"{
                "key": "/works/OL17365W",
                "title": "The Three-Body Problem",
                "author_name": ["Cixin Liu"],
                "first_publish_year": 2006,
                "publisher": ["Tor Books"],
                "cover_i": 8231990
            }"#,
        )
        .unwrap();
        let ol_cand = map_ol_doc_to_candidate(ol);
        assert_eq!(ol_cand.external_id, "OL17365W");
        assert_eq!(
            ol_cand.cover_url.as_deref(),
            Some("https://covers.openlibrary.org/b/id/8231990-L.jpg")
        );

        // 4. Bangumi
        let bgm: BgmSubject = serde_json::from_str(
            r#"{
                "id": 400602,
                "type": 2,
                "name": "葬送のフリーレン",
                "name_cn": "葬送的芙莉莲",
                "summary": "打倒魔王之后的故事。",
                "date": "2023-09-29",
                "rating": {"score": 8.9},
                "tags": [{"name": "奇幻"}, {"name": "MADHOUSE"}],
                "images": {"large": "https://lain.bgm.tv/pic/cover/l/frieren.jpg"},
                "infobox": [
                    {"key": "原作", "value": "山田鐘人、アベツカサ"},
                    {"key": "导演", "value": "斎藤圭一郎"},
                    {"key": "动画制作", "value": "MADHOUSE"}
                ]
            }"#,
        )
        .unwrap();
        let bgm_meta = map_bgm_detail(bgm, ScrapeTargetKind::TvShow);
        assert_eq!(bgm_meta.title.as_deref(), Some("葬送的芙莉莲"));
        assert_eq!(bgm_meta.illustrators_or_directors, vec!["斎藤圭一郎"]);
        assert_eq!(bgm_meta.publisher_or_studio.as_deref(), Some("MADHOUSE"));
        assert_eq!(bgm_meta.year, Some(2023));

        // 5. TMDB TV Show + Episode S01E02
        assert_eq!(
            parse_tmdb_external_id("209867:S1E2"),
            ("209867", Some((1, 2)))
        );
        let tmdb_tv: TmdbDetailBody = serde_json::from_str(
            r#"{
                "id": 209867,
                "name": "葬送的芙莉莲",
                "original_name": "葬送のフリーレン",
                "first_air_date": "2023-09-29",
                "poster_path": "/poster.jpg",
                "backdrop_path": "/backdrop.jpg",
                "genres": [{"name": "动画"}, {"name": "奇幻"}],
                "networks": [{"name": "Nippon TV"}],
                "credits": {
                    "cast": [{"name": "种崎敦美"}],
                    "crew": [{"name": "斋藤圭一郎", "job": "Director"}]
                }
            }"#,
        )
        .unwrap();
        let tmdb_ep = TmdbEpisodeBody {
            name: Some("不必是魔法也无妨".into()),
            overview: Some("芙莉莲与菲伦继续旅行。".into()),
            air_date: Some("2023-09-29".into()),
            vote_average: Some(8.8),
            season_number: Some(1),
            episode_number: Some(2),
        };
        let tmdb_meta = map_tmdb_detail(tmdb_tv, ScrapeTargetKind::TvShow, Some(tmdb_ep));
        assert_eq!(tmdb_meta.title.as_deref(), Some("不必是魔法也无妨"));
        assert_eq!(tmdb_meta.series_or_album.as_deref(), Some("葬送的芙莉莲"));
        assert_eq!(tmdb_meta.disc_or_season_number, Some(1));
        assert_eq!(tmdb_meta.track_or_episode_number, Some(2));
        assert_eq!(tmdb_meta.cast, vec!["种崎敦美"]);
        assert_eq!(
            tmdb_meta.cover_url.as_deref(),
            Some("https://image.tmdb.org/t/p/w780/poster.jpg")
        );
    }
}
