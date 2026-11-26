use serde_json::{Value, json};
use uuid::Uuid;
use xmedia_core::application::dto::LibraryDto;
use xmedia_core::domain::user::User;

pub use crate::application::dto::season_uuid;
use crate::application::dto::{
    SeasonGroupDto, ShowSummaryDto, SubtitleTrackDto, VideoDetailDto, VideoSummaryDto,
};
use crate::domain::id::VideoId;
use crate::domain::online::OnlineVideoPlayResult;
use crate::domain::video::{VideoContainer, VideoItem};
use crate::http::online::encode_query_component;

pub const JELLYFIN_SERVER_ID: &str = "00000000000040008000786d65646961";
pub const JELLYFIN_SERVER_NAME: &str = "XMedia";
pub const JELLYFIN_SERVER_VERSION: &str = "10.9.11";
pub const SECS_TO_TICKS: i64 = 10_000_000;

pub fn format_jf_id(id: Uuid) -> String {
    id.simple().to_string()
}

pub fn secs_to_ticks(secs: u32) -> Option<i64> {
    (secs > 0).then_some(i64::from(secs).saturating_mul(SECS_TO_TICKS))
}

pub fn pos_secs_to_ticks(pos_secs: f64) -> i64 {
    if !pos_secs.is_finite() || pos_secs <= 0.0 {
        return 0;
    }
    (pos_secs * (SECS_TO_TICKS as f64)).round() as i64
}

pub fn ticks_to_secs(ticks: i64) -> f64 {
    if ticks <= 0 {
        return 0.0;
    }
    (ticks as f64) / (SECS_TO_TICKS as f64)
}

pub fn build_user_dto(user: &User) -> Value {
    let is_admin = user.role.is_admin();
    json!({
        "Name": user.username,
        "ServerId": JELLYFIN_SERVER_ID,
        "Id": format_jf_id(user.id.0),
        "HasPassword": true,
        "HasConfiguredPassword": true,
        "HasConfiguredEasyPassword": false,
        "EnableAutoLogin": false,
        "Configuration": {
            "PlayDefaultAudioTrack": true,
            "SubtitleLanguagePreference": "chi",
            "DisplayMissingEpisodes": false,
            "SubtitleMode": "Default",
            "EnableLocalPassword": false,
            "OrderedViews": [],
            "LatestItemsExcludes": [],
            "MyMediaExcludes": [],
            "HidePlayedInLatest": false,
            "RememberAudioSelections": true,
            "RememberSubtitleSelections": true,
            "EnableNextEpisodeAutoPlay": true
        },
        "Policy": {
            "IsAdministrator": is_admin,
            "IsHidden": false,
            "IsDisabled": !user.is_active,
            "EnableAllFolders": true,
            "EnableMediaPlayback": true,
            "EnableAudioPlaybackTranscoding": true,
            "EnableVideoPlaybackTranscoding": true,
            "EnablePlaybackRemuxing": true,
            "EnableContentDownloading": true,
            "EnableRemoteAccess": true,
            "EnableUserPreferenceAccess": true
        }
    })
}

pub fn build_library_view_item(
    lib: &LibraryDto,
    collection_type: Option<&str>,
    child_count: usize,
) -> Value {
    let id_str = format_jf_id(lib.id.0);
    let mut obj = json!({
        "Name": lib.name,
        "ServerId": JELLYFIN_SERVER_ID,
        "Id": id_str,
        "Etag": id_str,
        "DateCreated": lib.created_at.to_string(),
        "CanDelete": false,
        "CanDownload": false,
        "SortName": lib.name.to_lowercase(),
        "ExternalUrls": [],
        "Path": lib.name,
        "EnableMediaSourceDisplay": true,
        "ChannelId": Value::Null,
        "Taglines": [],
        "Genres": [],
        "PlayAccess": "Full",
        "RemoteTrailers": [],
        "ProviderIds": {},
        "IsFolder": true,
        "Type": "CollectionFolder",
        "ChildCount": child_count,
        "UserData": {
            "PlaybackPositionTicks": 0,
            "PlayCount": 0,
            "IsFavorite": false,
            "Played": false,
            "Key": id_str
        },
        "DisplayPreferencesId": id_str,
        "ImageTags": {},
        "BackdropImageTags": [],
        "LocationType": "FileSystem",
        "MediaType": "Unknown"
    });
    if let Some(ct) = collection_type {
        obj["CollectionType"] = json!(ct);
    }
    obj
}

fn build_image_tags(poster_key: Option<&str>) -> Value {
    match poster_key {
        Some(key) if !key.trim().is_empty() => json!({ "Primary": key }),
        _ => json!({}),
    }
}

pub fn build_video_user_data(v: &VideoSummaryDto) -> Value {
    let pos_ticks = v
        .progress
        .as_ref()
        .map(|p| pos_secs_to_ticks(p.position_secs))
        .unwrap_or(0);
    let played = v.progress.as_ref().is_some_and(|p| p.completed);
    let played_pct = if v.duration_secs > 0 && pos_ticks > 0 {
        let total = i64::from(v.duration_secs) * SECS_TO_TICKS;
        Some(((pos_ticks as f64 / total as f64) * 100.0).clamp(0.0, 100.0))
    } else {
        None
    };
    let mut ud = json!({
        "PlaybackPositionTicks": pos_ticks,
        "PlayCount": if played { 1 } else { 0 },
        "IsFavorite": v.is_favorite,
        "Played": played,
        "Key": format_jf_id(v.id.0)
    });
    if let Some(pct) = played_pct {
        ud["PlayedPercentage"] = json!(pct);
    }
    if let Some(ref prog) = v.progress {
        ud["LastPlayedDate"] = json!(prog.updated_at.to_string());
    }
    ud
}

pub fn build_media_source_local(
    video: &VideoItem,
    subtitles: &[SubtitleTrackDto],
    api_key: &str,
) -> Value {
    let id_str = format_jf_id(video.id.0);
    let container = video.container.as_ref();
    let mut streams: Vec<Value> = Vec::new();
    let mut next_idx = 0i64;

    streams.push(json!({
        "Codec": video.video_codec.as_deref().unwrap_or("h264"),
        "Type": "Video",
        "Index": next_idx,
        "IsDefault": true,
        "IsForced": false,
        "Width": video.width.unwrap_or(1920),
        "Height": video.height.unwrap_or(1080),
        "BitRate": video.bit_rate_kbps.map(|b| u64::from(b) * 1000)
    }));
    next_idx += 1;

    if video.audio_streams.is_empty() {
        streams.push(json!({
            "Codec": "aac",
            "Type": "Audio",
            "Index": next_idx,
            "IsDefault": true,
            "IsForced": false,
            "Channels": 2
        }));
        next_idx += 1;
    } else {
        for (i, a) in video.audio_streams.iter().enumerate() {
            streams.push(json!({
                "Codec": a.codec,
                "Language": a.language,
                "DisplayTitle": a.title.clone().or_else(|| a.language.clone()).unwrap_or_else(|| format!("Audio {}", i + 1)),
                "Type": "Audio",
                "Index": next_idx,
                "IsDefault": i == 0,
                "IsForced": false,
                "Channels": if a.channels > 0 { a.channels } else { 2 }
            }));
            next_idx += 1;
        }
    }

    for (sub_pos, sub) in subtitles.iter().enumerate() {
        let delivery_url = format!(
            "/Videos/{id_str}/{id_str}/Subtitles/{sub_pos}/Stream.vtt?api_key={}",
            encode_query_component(api_key)
        );
        streams.push(json!({
            "Codec": "vtt",
            "Language": sub.language.as_deref().unwrap_or("und"),
            "DisplayTitle": sub.label,
            "Title": sub.label,
            "Type": "Subtitle",
            "Index": next_idx,
            "IsDefault": sub.is_default,
            "IsForced": false,
            "IsExternal": true,
            "IsTextSubtitleStream": true,
            "SupportsExternalStream": true,
            "DeliveryMethod": "External",
            "DeliveryUrl": delivery_url
        }));
        next_idx += 1;
    }

    let direct_url = format!(
        "/Videos/{id_str}/stream?Static=true&MediaSourceId={id_str}&api_key={}",
        encode_query_component(api_key)
    );
    let mut ms = json!({
        "Protocol": "File",
        "Id": id_str,
        "Path": video.source.display_path(),
        "Type": "Default",
        "Container": container,
        "Size": video.file_size,
        "Name": video.title,
        "IsRemote": false,
        "SupportsTranscoding": false,
        "SupportsDirectStream": true,
        "SupportsDirectPlay": true,
        "IsInfiniteStream": false,
        "RequiresOpening": false,
        "RequiresClosing": false,
        "DirectStreamUrl": direct_url,
        "MediaStreams": streams,
        "DefaultAudioStreamIndex": 1
    });
    if let Some(ticks) = secs_to_ticks(video.duration_secs) {
        ms["RunTimeTicks"] = json!(ticks);
    }
    if let Some(br) = video.bit_rate_kbps {
        ms["Bitrate"] = json!(u64::from(br) * 1000);
    }
    ms
}

pub fn build_media_source_online(
    video: &VideoItem,
    play: &OnlineVideoPlayResult,
    api_key: &str,
) -> Value {
    let id_str = format_jf_id(video.id.0);
    let encoded_key = encode_query_component(api_key);
    let is_hls = play.stream_type.eq_ignore_ascii_case("hls")
        || play.stream_url.to_ascii_lowercase().contains(".m3u8");

    let mut streams: Vec<Value> = vec![
        json!({
            "Codec": "h264",
            "Type": "Video",
            "Index": 0,
            "IsDefault": true,
            "IsForced": false,
            "Width": video.width.unwrap_or(1920),
            "Height": video.height.unwrap_or(1080)
        }),
        json!({
            "Codec": "aac",
            "Type": "Audio",
            "Index": 1,
            "IsDefault": true,
            "IsForced": false,
            "Channels": 2
        }),
    ];

    for (idx, sub) in play.subtitles.iter().enumerate() {
        let delivery_url =
            format!("/Videos/{id_str}/{id_str}/Subtitles/{idx}/Stream.vtt?api_key={encoded_key}");
        streams.push(json!({
            "Codec": "vtt",
            "Language": sub.lang.as_deref().unwrap_or("und"),
            "DisplayTitle": sub.label,
            "Title": sub.label,
            "Type": "Subtitle",
            "Index": (idx + 2) as i64,
            "IsDefault": idx == 0,
            "IsForced": false,
            "IsExternal": true,
            "IsTextSubtitleStream": true,
            "SupportsExternalStream": true,
            "DeliveryMethod": "External",
            "DeliveryUrl": delivery_url
        }));
    }

    if is_hls {
        let hls_url = format!("/Videos/{id_str}/hls/master.m3u8?api_key={encoded_key}");
        let mut ms = json!({
            "Protocol": "Http",
            "Id": id_str,
            "Path": hls_url,
            "Type": "Default",
            "Container": "hls",
            "Name": video.title,
            "IsRemote": true,
            "SupportsTranscoding": true,
            "SupportsDirectStream": true,
            "SupportsDirectPlay": false,
            "IsInfiniteStream": false,
            "RequiresOpening": false,
            "RequiresClosing": false,
            "DirectStreamUrl": hls_url,
            "TranscodingUrl": hls_url,
            "TranscodingSubProtocol": "hls",
            "TranscodingContainer": "ts",
            "MediaStreams": streams,
            "DefaultAudioStreamIndex": 1
        });
        if let Some(ticks) = secs_to_ticks(video.duration_secs) {
            ms["RunTimeTicks"] = json!(ticks);
        }
        ms
    } else {
        let container = infer_online_container(&play.stream_url);
        let direct_url = format!(
            "/Videos/{id_str}/stream?Static=true&MediaSourceId={id_str}&api_key={encoded_key}"
        );
        let mut ms = json!({
            "Protocol": "Http",
            "Id": id_str,
            "Path": direct_url,
            "Type": "Default",
            "Container": container.as_ref(),
            "Name": video.title,
            "IsRemote": true,
            "SupportsTranscoding": false,
            "SupportsDirectStream": true,
            "SupportsDirectPlay": true,
            "IsInfiniteStream": false,
            "RequiresOpening": false,
            "RequiresClosing": false,
            "DirectStreamUrl": direct_url,
            "MediaStreams": streams,
            "DefaultAudioStreamIndex": 1
        });
        if let Some(ticks) = secs_to_ticks(video.duration_secs) {
            ms["RunTimeTicks"] = json!(ticks);
        }
        ms
    }
}

fn infer_online_container(url: &str) -> VideoContainer {
    if let Ok(parsed) = url::Url::parse(url)
        && let Some(ext) = std::path::Path::new(parsed.path())
            .extension()
            .and_then(|s| s.to_str())
        && let Ok(c) = ext.parse::<VideoContainer>()
    {
        return c;
    }
    VideoContainer::Mp4
}

pub fn build_video_summary_item(
    v: &VideoSummaryDto,
    detail: Option<&VideoDetailDto>,
    raw_video: Option<&VideoItem>,
    api_key: &str,
) -> Value {
    let id_str = format_jf_id(v.id.0);
    let is_episode = v.show_id.is_some();
    let item_type = if is_episode { "Episode" } else { "Movie" };

    let mut obj = json!({
        "Name": v.title,
        "ServerId": JELLYFIN_SERVER_ID,
        "Id": id_str,
        "Etag": id_str,
        "DateCreated": v.created_at.to_string(),
        "SortName": v.title.to_lowercase(),
        "Overview": v.summary,
        "Container": v.container.as_ref(),
        "Genres": v.genres,
        "Tags": v.tags,
        "IsFolder": false,
        "Type": item_type,
        "MediaType": "Video",
        "VideoType": "VideoFile",
        "LocationType": "FileSystem",
        "CanDownload": true,
        "CanDelete": false,
        "PlayAccess": "Full",
        "ImageTags": build_image_tags(v.poster_key.as_deref()),
        "BackdropImageTags": [],
        "PrimaryImageAspectRatio": if is_episode { 1.7778 } else { 0.6667 },
        "UserData": build_video_user_data(v)
    });

    if let Some(year) = v.year {
        obj["ProductionYear"] = json!(year);
        obj["PremiereDate"] = json!(format!("{year}-01-01T00:00:00.0000000Z"));
    }
    if let Some(ticks) = secs_to_ticks(v.duration_secs) {
        obj["RunTimeTicks"] = json!(ticks);
    }
    if let Some(w) = v.width {
        obj["Width"] = json!(w);
    }
    if let Some(h) = v.height {
        obj["Height"] = json!(h);
    }

    if let Some(show_id) = v.show_id {
        let season_num = v.season_number.unwrap_or(1);
        let s_uuid = season_uuid(show_id, season_num);
        obj["SeriesId"] = json!(format_jf_id(show_id.0));
        obj["SeriesName"] = json!(v.show_title.as_deref().unwrap_or(""));
        obj["SeasonId"] = json!(format_jf_id(s_uuid));
        obj["SeasonName"] = json!(format!("Season {season_num}"));
        obj["ParentId"] = json!(format_jf_id(s_uuid));
        obj["ParentIndexNumber"] = json!(season_num);
        if let Some(ep_num) = v.episode_number {
            obj["IndexNumber"] = json!(ep_num);
        }
    } else {
        obj["ParentId"] = json!(format_jf_id(v.library_id.0));
    }

    if let Some(raw) = raw_video {
        if raw.source.is_cached() {
            let subs = detail.map(|d| d.subtitles.as_slice()).unwrap_or(&[]);
            obj["MediaSources"] = json!([build_media_source_local(raw, subs, api_key)]);
        } else {
            // Lightweight placeholder source for catalog listing (real URL resolved in PlaybackInfo)
            let direct_url = format!(
                "/Videos/{id_str}/stream?Static=true&MediaSourceId={id_str}&api_key={}",
                encode_query_component(api_key)
            );
            obj["MediaSources"] = json!([{
                "Protocol": "Http",
                "Id": id_str,
                "Path": direct_url,
                "Type": "Default",
                "Container": "mp4",
                "Name": v.title,
                "IsRemote": true,
                "SupportsDirectPlay": true,
                "SupportsDirectStream": true,
                "SupportsTranscoding": true,
                "DirectStreamUrl": direct_url,
                "MediaStreams": []
            }]);
        }
    }

    obj
}

pub fn build_series_item(s: &ShowSummaryDto) -> Value {
    let id_str = format_jf_id(s.id.0);
    let unplayed = s.episode_count.saturating_sub(s.watched_episode_count);
    let played = s.episode_count > 0 && unplayed == 0;
    let played_pct = if s.episode_count > 0 {
        (f64::from(s.watched_episode_count) / f64::from(s.episode_count)) * 100.0
    } else {
        0.0
    };

    let mut obj = json!({
        "Name": s.title,
        "ServerId": JELLYFIN_SERVER_ID,
        "Id": id_str,
        "Etag": id_str,
        "DateCreated": s.created_at.to_string(),
        "SortName": s.title.to_lowercase(),
        "Overview": s.summary,
        "Genres": s.genres,
        "Tags": s.tags,
        "ParentId": format_jf_id(s.library_id.0),
        "IsFolder": true,
        "Type": "Series",
        "ChildCount": s.season_count.max(1),
        "RecursiveItemCount": s.episode_count,
        "LocationType": "FileSystem",
        "CanDownload": false,
        "CanDelete": false,
        "PlayAccess": "Full",
        "ImageTags": build_image_tags(s.poster_key.as_deref()),
        "BackdropImageTags": [],
        "PrimaryImageAspectRatio": 0.6667,
        "UserData": {
            "UnplayedItemCount": unplayed,
            "PlaybackPositionTicks": 0,
            "PlayCount": if played { 1 } else { 0 },
            "IsFavorite": s.is_favorite,
            "Played": played,
            "PlayedPercentage": played_pct,
            "Key": id_str
        }
    });

    if let Some(year) = s.year {
        obj["ProductionYear"] = json!(year);
        obj["PremiereDate"] = json!(format!("{year}-01-01T00:00:00.0000000Z"));
    }

    obj
}

pub fn build_season_item(show: &ShowSummaryDto, season: &SeasonGroupDto) -> Value {
    let s_uuid = season_uuid(show.id, season.season_number);
    let id_str = format_jf_id(s_uuid);
    let total = season.episodes.len() as u32;
    let watched = season
        .episodes
        .iter()
        .filter(|e| e.progress.as_ref().is_some_and(|p| p.completed))
        .count() as u32;
    let unplayed = total.saturating_sub(watched);
    let played = total > 0 && unplayed == 0;
    let name = if season.season_number == 0 {
        "Specials".to_string()
    } else {
        format!("Season {}", season.season_number)
    };
    let poster = season
        .episodes
        .iter()
        .find_map(|e| e.poster_key.as_deref())
        .or(show.poster_key.as_deref());

    json!({
        "Name": name,
        "ServerId": JELLYFIN_SERVER_ID,
        "Id": id_str,
        "Etag": id_str,
        "DateCreated": show.created_at.to_string(),
        "SortName": format!("{:04}", season.season_number),
        "SeriesId": format_jf_id(show.id.0),
        "SeriesName": show.title,
        "ParentId": format_jf_id(show.id.0),
        "IndexNumber": season.season_number,
        "IsFolder": true,
        "Type": "Season",
        "ChildCount": total,
        "LocationType": "FileSystem",
        "ImageTags": build_image_tags(poster),
        "BackdropImageTags": [],
        "PrimaryImageAspectRatio": 0.6667,
        "UserData": {
            "UnplayedItemCount": unplayed,
            "PlaybackPositionTicks": 0,
            "PlayCount": if played { 1 } else { 0 },
            "IsFavorite": false,
            "Played": played,
            "Key": id_str
        }
    })
}

pub fn build_search_hint_video(v: &VideoSummaryDto) -> Value {
    let is_ep = v.show_id.is_some();
    let mut obj = json!({
        "ItemId": format_jf_id(v.id.0),
        "Id": format_jf_id(v.id.0),
        "Name": v.title,
        "Type": if is_ep { "Episode" } else { "Movie" },
        "MediaType": "Video",
        "IsFolder": false,
        "ProductionYear": v.year,
        "PrimaryImageTag": v.poster_key,
        "RunTimeTicks": secs_to_ticks(v.duration_secs)
    });
    if let Some(ref st) = v.show_title {
        obj["Series"] = json!(st);
    }
    if let Some(ep) = v.episode_number {
        obj["IndexNumber"] = json!(ep);
    }
    if let Some(s) = v.season_number {
        obj["ParentIndexNumber"] = json!(s);
    }
    obj
}

pub fn build_search_hint_series(s: &ShowSummaryDto) -> Value {
    json!({
        "ItemId": format_jf_id(s.id.0),
        "Id": format_jf_id(s.id.0),
        "Name": s.title,
        "Type": "Series",
        "IsFolder": true,
        "ProductionYear": s.year,
        "PrimaryImageTag": s.poster_key
    })
}

pub fn build_playback_info_response(media_source: Value, play_session_id: VideoId) -> Value {
    json!({
        "MediaSources": [media_source],
        "PlaySessionId": format_jf_id(play_session_id.0)
    })
}
