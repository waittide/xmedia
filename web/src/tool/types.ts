import type { FfmpegStatusDto, MediaKind } from '@/types/api'

export type ScrapeTargetKind =
  | 'book'
  | 'comic'
  | 'music_track'
  | 'music_album'
  | 'lyrics'
  | 'movie'
  | 'tv_show'

export interface ToolStatusDto {
  version: string
  requires_password: boolean
  authenticated: boolean
  ffmpeg: FfmpegStatusDto
}

export interface ManagedRootDto {
  id: string
  name: string
  path: string
  default_kind: MediaKind | null
  read_only?: boolean
}

export interface InboxConfigDto {
  enabled: boolean
  inbox_path: string | null
  target_root_id: string | null
  use_hardlink: boolean
  delete_source: boolean
  auto_scrape: boolean
  organize_by_template: boolean
}

export interface InboxImportedItemDto {
  source_rel: string
  target_rel: string
  mode: string
  scraped: boolean
  error: string | null
}

export interface InboxScanReportDto {
  imported_count: number
  skipped_count: number
  failed_count: number
  items: InboxImportedItemDto[]
}

export interface WebhookTestReportDto {
  ok: boolean
  matched_libraries: string[]
  total_libraries: number
  message: string
}

export interface RenameTemplatesDto {
  book: string
  comic: string
  music: string
  video_movie: string
  video_episode: string
}

export interface ScraperPrioritiesDto {
  book: string[]
  comic: string[]
  music: string[]
  video: string[]
}

export interface PublicSettingsDto {
  roots: ManagedRootDto[]
  has_tmdb_api_key: boolean
  tmdb_api_key_masked: string | null
  has_bangumi_token: boolean
  bangumi_token_masked: string | null
  http_proxy: string | null
  enable_adult_scrapers?: boolean
  rename_templates: RenameTemplatesDto
  scraper_priorities: ScraperPrioritiesDto
  default_scrape_locale: string
  scrape_cache_ttl_ms: number
  inbox?: InboxConfigDto
  xmedia_webhook_enabled?: boolean
  xmedia_webhook_base_url?: string
  has_xmedia_webhook_api_key?: boolean
  xmedia_webhook_api_key_masked?: string | null
  xmedia_webhook_notify_on_rename?: boolean
  xmedia_webhook_notify_on_scrape?: boolean
  xmedia_webhook_notify_on_inbox?: boolean
}

export interface UpdateToolSettingsInput {
  tmdb_api_key?: string
  bangumi_access_token?: string
  http_proxy?: string
  enable_adult_scrapers?: boolean
  rename_templates?: RenameTemplatesDto
  scraper_priorities?: ScraperPrioritiesDto
  default_scrape_locale?: string
  scrape_cache_ttl_ms?: number
  inbox?: InboxConfigDto
  xmedia_webhook_enabled?: boolean
  xmedia_webhook_base_url?: string
  xmedia_webhook_api_key?: string
  xmedia_webhook_notify_on_rename?: boolean
  xmedia_webhook_notify_on_scrape?: boolean
  xmedia_webhook_notify_on_inbox?: boolean
}

export interface ScrapeCacheStatsDto {
  total_entries: number
  valid_entries: number
  total_bytes: number
}

export type AutoScrapeMatchMode = 'strict' | 'standard' | 'loose'

export interface AutoScrapeOptionsDto {
  match_mode?: AutoScrapeMatchMode
  only_fill_missing?: boolean
  apply_cover?: boolean
  write_sidecar_lrc?: boolean
  organize_by_template?: boolean
  providers?: string[]
  field_groups?: string[]
}

export interface BatchTextReplaceRuleDto {
  find: string
  replace: string
  use_regex: boolean
  fields: string[]
}

export interface BatchMetadataEditInputDto {
  paths: string[]
  media_kind: MediaKind
  title_template?: string
  creator?: string
  series_or_album?: string
  publisher_or_studio?: string
  year?: number | null
  language?: string
  genre?: string
  add_tags?: string[]
  remove_tags?: string[]
  replace_rule?: BatchTextReplaceRuleDto
  zh_direction?: 't2s' | 's2t' | null
}

export interface BatchMetadataEditReportDto {
  batch_id: string | null
  updated_count: number
  skipped_count: number
  failed_count: number
  errors: string[]
}

export interface MetadataSnapshotRecordDto {
  rel_path: string
  file_name: string
}

export interface MetadataHistoryBatchDto {
  batch_id: string
  root_id: string
  media_kind: MediaKind
  label: string
  created_at_unix: number
  updated_count: number
  records: MetadataSnapshotRecordDto[]
}

export interface UndoMetadataReportDto {
  batch_id: string
  restored_count: number
  failed_paths: string[]
}

export interface AutoScrapeResultDto {
  matched: boolean
  confidence?: number
  detail: MediaFileDetail
}

export type ChapterEditMode = 'plain' | 'xhtml'

export interface EditableBookChapterDto {
  id: string
  title: string
  level: number
  plain_text: string
  xhtml_body: string
  has_rich_media: boolean
  source_href: string | null
}

export interface BookContentDocumentDto {
  format: 'txt' | 'epub'
  title: string
  authors: string[]
  stylesheets: string[]
  chapters: EditableBookChapterDto[]
}

export interface SaveBookChapterInput {
  title: string
  level: number
  mode: ChapterEditMode
  plain_text: string
  xhtml_body: string
  source_href?: string | null
}

export interface ConvertTxtToEpubResponseDto {
  detail: MediaFileDetail
  deleted_source_txt: boolean
}

export type ZhConvertDirection = 't2s' | 's2t'

export interface BookTextTransformOptions {
  zh_direction?: ZhConvertDirection | null
  convert_quotes_to_horizontal?: boolean
  vertical_to_horizontal?: boolean
  clean_webnovel_ads?: boolean
  normalize_punctuation?: boolean
  custom_strip_patterns?: string[]
}

export interface TransformBookContentResultDto {
  chapters: EditableBookChapterDto[]
  stylesheets: string[]
  changed_chapters: number
}

export interface InsertedEpubImageDto {
  asset_path: string
  chapter_rel_href: string
  mime_type: string
}

export interface PolishBookResponseDto {
  jacket_updated: boolean
  removed_orphan_files: number
  detail: MediaFileDetail
}

export type BookImageCompressFormat = 'webp' | 'jpeg' | 'keep'

export interface NormalizeBookToEpubOptionsDto {
  keep_source: boolean
  compress_images: boolean
  image_format: BookImageCompressFormat
  max_image_dimension: number | null
  image_quality: number
  generate_jacket: boolean
  remove_orphans: boolean
  pdf_scanned_fallback: boolean
}

export interface NormalizeBookItemReportDto {
  source_rel_path: string
  output_rel_path: string | null
  backup_rel_path: string | null
  source_format: string
  before_bytes: number
  after_bytes: number
  chapter_count: number
  compressed_images: number
  removed_orphans: number
  deleted_source: boolean
  error: string | null
}

export interface NormalizeBooksBatchReportDto {
  total: number
  succeeded: number
  failed: number
  total_before_bytes: number
  total_after_bytes: number
  items: NormalizeBookItemReportDto[]
}

export interface CompletenessFlags {
  has_title: boolean
  has_creator: boolean
  has_cover: boolean
  has_summary_or_lyrics: boolean
  has_nfo_or_tag_block: boolean
}

export interface FolderEntryDto {
  name: string
  rel_path: string
  child_dir_count: number
  media_file_count: number
}

export interface MediaFileEntry {
  root_id: string
  rel_path: string
  file_name: string
  extension: string
  media_kind: MediaKind
  file_size: number
  modified_unix_secs: number
  display_title: string
  display_subtitle: string
  year: number | null
  completeness: CompletenessFlags
}

export interface DirectorySnapshot {
  root_id: string
  root_name: string
  current_rel_path: string
  parent_rel_path: string | null
  folders: FolderEntryDto[]
  files: MediaFileEntry[]
}

export interface BookFileMeta {
  format: string
  title: string
  authors: string[]
  series: string | null
  subseries: string | null
  series_index: string | null
  description: string | null
  publisher: string | null
  published_at: string | null
  language: string | null
  isbn: string | null
  tags: string[]
  locked_fields: string[]
  has_cover: boolean
  writable: boolean
}

export interface ComicFileMeta {
  format: string
  series: string | null
  series_group: string | null
  title: string | null
  number: string | null
  volume: number | null
  summary: string | null
  writers: string[]
  pencillers: string[]
  publisher: string | null
  genres: string[]
  tags: string[]
  year: number | null
  language_iso: string | null
  reading_direction: string
  page_count: number
  has_comic_info_xml: boolean
  has_cover: boolean
  has_series_cover: boolean
}

export interface MusicFileMeta {
  format: string
  title: string
  artist: string
  album_artist: string | null
  album: string
  track_number: number | null
  disc_number: number | null
  year: number | null
  genre: string | null
  comment: string | null
  duration_secs: number
  bit_rate_kbps: number | null
  sample_rate_hz: number | null
  bit_depth: number | null
  channels: number | null
  has_embedded_cover: boolean
  has_directory_cover: boolean
  embedded_lyrics: string | null
  sidecar_lrc: string | null
  sidecar_lrc_filename: string
}

export interface ProbedAudioStreamDto {
  stream_index: number
  codec: string
  channels: number
  language: string | null
  title: string | null
  is_default: boolean
}

export interface ProbedSubtitleStreamDto {
  stream_index: number
  codec: string
  language: string | null
  title: string | null
  is_text_based: boolean
  is_default: boolean
}

export interface VideoFileMeta {
  container: string
  is_episode: boolean
  title: string
  original_title: string | null
  show_title: string | null
  season_number: number | null
  episode_number: number | null
  year: number | null
  rating: number | null
  summary: string | null
  genres: string[]
  tags: string[]
  actors: string[]
  directors: string[]
  studio: string | null
  collection_set?: string | null
  tmdb_id: string | null
  bangumi_id: string | null
  locked_fields?: string[]
  has_nfo: boolean
  nfo_filename: string
  has_poster: boolean
  has_fanart: boolean
  has_thumb: boolean
  has_clearlogo?: boolean
  duration_secs: number
  width: number | null
  height: number | null
  video_codec: string | null
  bit_rate_kbps: number | null
  audio_streams: ProbedAudioStreamDto[]
  subtitle_streams: ProbedSubtitleStreamDto[]
  sidecar_subtitles: string[]
}

export type MediaMetadataPayload =
  | { kind: 'book'; data: BookFileMeta }
  | { kind: 'comic'; data: ComicFileMeta }
  | { kind: 'music'; data: MusicFileMeta }
  | { kind: 'video'; data: VideoFileMeta }

export interface MediaFileDetail {
  entry: MediaFileEntry
  metadata: MediaMetadataPayload
}

export interface BookMetaPatch {
  title?: string
  authors?: string[]
  series?: string
  subseries?: string
  series_index?: string
  description?: string
  publisher?: string
  published_at?: string
  language?: string
  isbn?: string
  tags?: string[]
  locked_fields?: string[]
}

export interface ComicMetaPatch {
  series?: string
  series_group?: string
  title?: string
  number?: string
  volume?: number
  summary?: string
  writers?: string[]
  pencillers?: string[]
  publisher?: string
  genres?: string[]
  tags?: string[]
  year?: number
  language_iso?: string
  reading_direction?: string
}

export interface MusicMetaPatch {
  title?: string
  artist?: string
  album_artist?: string
  album?: string
  track_number?: number
  disc_number?: number
  year?: number
  genre?: string
  comment?: string
  embedded_lyrics?: string
  sidecar_lrc?: string
}

export interface VideoMetaPatch {
  is_episode?: boolean
  title?: string
  original_title?: string
  show_title?: string
  season_number?: number
  episode_number?: number
  year?: number
  rating?: number
  summary?: string
  genres?: string[]
  tags?: string[]
  actors?: string[]
  directors?: string[]
  studio?: string
  collection_set?: string
  tmdb_id?: string
  bangumi_id?: string
  locked_fields?: string[]
}

export type MediaMetadataPatchEnvelope =
  | { kind: 'book'; patch: BookMetaPatch }
  | { kind: 'comic'; patch: ComicMetaPatch }
  | { kind: 'music'; patch: MusicMetaPatch }
  | { kind: 'video'; patch: VideoMetaPatch }

export interface RenamePlanItem {
  from_rel_path: string
  to_rel_path: string
  companion_moves: [string, string][]
  conflict: string | null
  unchanged: boolean
}

export interface ProviderStatusDto {
  id: string
  name: string
  description: string
  website: string
  supported_targets: ScrapeTargetKind[]
  requires_api_key: boolean
  configured: boolean
}

export interface ScrapeQuery {
  target?: ScrapeTargetKind
  providers?: string[]
  keyword: string
  artist_or_author?: string
  album_or_series?: string
  year?: number | null
  season?: number | null
  episode?: number | null
  duration_secs?: number | null
  isbn?: string
  locale?: string
}

export interface ScrapeCandidate {
  provider_id: string
  provider_name: string
  external_id: string
  target: ScrapeTargetKind
  title: string
  subtitle: string | null
  year: number | null
  summary: string | null
  cover_url: string | null
  synced_lyrics: string | null
  plain_lyrics: string | null
}

export interface ScrapedMetadata {
  provider_id: string
  external_id: string
  target: ScrapeTargetKind | null
  title: string | null
  original_title: string | null
  series_or_album: string | null
  creators: string[]
  illustrators_or_directors: string[]
  cast: string[]
  publisher_or_studio: string | null
  year: number | null
  published_date: string | null
  language: string | null
  isbn: string | null
  rating: number | null
  track_or_episode_number: number | null
  disc_or_season_number: number | null
  summary: string | null
  show_summary?: string | null
  season_title?: string | null
  season_summary?: string | null
  season_poster_url?: string | null
  episode_still_url?: string | null
  genres: string[]
  tags: string[]
  cover_url: string | null
  backdrop_url: string | null
  synced_lyrics: string | null
  plain_lyrics: string | null
  external_ids?: Record<string, string>
}

export interface FieldSourceBadge {
  provider_id: string
  provider_name: string
}

export interface MergedScrapedResult {
  metadata: ScrapedMetadata
  field_sources: Record<string, FieldSourceBadge>
}

export interface ProviderBriefDto {
  id: string
  name: string
}

export interface AggregateScrapeResponse {
  provider_order: ProviderBriefDto[]
  candidates: ScrapeCandidate[]
  selected_by_provider: Record<string, ScrapeCandidate>
  details_by_provider: Record<string, ScrapedMetadata>
  merged: MergedScrapedResult
}

export interface ComicPageInfoDto {
  index: number
  entry_name: string
  file_size: number
  width: number | null
  height: number | null
  page_type: 'Story' | 'FrontCover' | 'InnerCover' | 'BackCover' | 'Advertisement' | string
  double_page: boolean
}

export interface ComicPagesSnapshotDto {
  format: string
  total_pages: number
  total_size_bytes: number
  pages: ComicPageInfoDto[]
}

export interface SaveComicPageInput {
  entry_name: string
  page_type: string
  double_page: boolean
}

export type ComicImageReencodeFormat = 'keep' | 'jpeg' | 'webp'

export interface ComicOptimizeReportDto {
  before_bytes: number
  after_bytes: number
  page_count: number
  reencoded_pages: number
  removed_junk_entries: number
}

export interface MergeComicsReportDto {
  output_rel_path: string
  source_count: number
  total_pages: number
  output_bytes: number
}

export interface BatchComicSeriesSyncInput {
  series?: string
  series_group?: string
  writers?: string[]
  pencillers?: string[]
  publisher?: string
  genres?: string[]
  tags?: string[]
  reading_direction?: string
  numbering_mode?: 'none' | 'chapter' | 'volume'
  start_index?: number
  pad_width?: number
}

export interface BatchComicSeriesSyncReportDto {
  updated_count: number
  failed_paths: string[]
}

export interface MusicLyricsToolInput {
  offset_ms?: number
  strip_credit_noise?: boolean
  zh_direction?: 't2s' | 's2t' | null
  sync_mode?: 'none' | 'sidecar_to_embedded' | 'embedded_to_sidecar' | 'both'
  custom_text?: string
}

export interface MusicLyricsToolReportDto {
  updated_embedded: boolean
  updated_sidecar: boolean
  removed_lines: number
  final_lyrics: string
}

export interface MusicCoverOptimizeReportDto {
  had_cover: boolean
  optimized: boolean
  before_bytes: number
  after_bytes: number
  width: number | null
  height: number | null
}

export interface ReplayGainTrackResultDto {
  rel_path: string
  track_gain_db: number
  track_peak: number
}

export interface ReplayGainReportDto {
  processed: number
  album_gain_db: number | null
  album_peak: number | null
  tracks: ReplayGainTrackResultDto[]
  failed_paths: string[]
}

export interface CueSplitReportDto {
  created_files: string[]
  album_title: string | null
  album_artist: string | null
}

export interface BatchMusicAlbumSyncInput {
  album?: string
  album_artist?: string
  artist?: string
  year?: number | null
  genre?: string
  disc_number?: number | null
  auto_number_tracks?: boolean
  start_track?: number
}

export interface BatchMusicAlbumSyncReportDto {
  updated_count: number
  failed_paths: string[]
}

export interface VideoSubtitleInfoDto {
  filename: string
  extension: string
  file_size: number
  detected_encoding: string
  is_utf8: boolean
  language_tag: string | null
  forced: boolean
}

export interface ProcessSubtitleInput {
  subtitle_filename: string
  convert_utf8?: boolean
  offset_ms?: number
  zh_direction?: 't2s' | 's2t' | null
  rename_language?: string
  forced?: boolean
}

export interface ProcessSubtitleReportDto {
  original_filename: string
  final_filename: string
  original_encoding: string
  shifted_cues: number
  converted_utf8: boolean
}

export interface ExtractSubtitleReportDto {
  created_filename: string
  file_size: number
}

export interface BatchRemapEpisodesInput {
  show_title?: string
  season_number?: number | null
  mode?: 'sequential' | 'absolute_offset'
  start_episode?: number
  episode_offset?: number
  write_season_nfo?: boolean
}

export interface BatchRemapEpisodesReportDto {
  updated_count: number
  failed_paths: string[]
}

export interface RenameMoveRecord {
  from_rel_path: string
  to_rel_path: string
  companion_moves: [string, string][]
}

export interface RenameHistoryBatchDto {
  batch_id: string
  root_id: string
  created_at_unix: number
  moved_count: number
  moves: RenameMoveRecord[]
}

export interface UndoRenameReportDto {
  batch_id: string
  restored_count: number
  skipped_conflicts: string[]
}

export interface DryRunScrapeItemDto {
  rel_path: string
  display_title: string
  query_keyword: string
  matched: boolean
  matched_title: string | null
  matched_creator: string | null
  matched_year: number | null
  matched_provider: string | null
  confidence: number
  low_confidence: boolean
  error: string | null
}
