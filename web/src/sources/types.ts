import type { RouteLocationRaw } from 'vue-router'
import type { MediaKind } from '@/types/api'

/** Media domains that mount `/api/v1/{domain}/online/items` on the backend. */
export type OnlineItemDomain = 'comic' | 'music' | 'video'

export function isOnlineItemDomain(domain: MediaKind): domain is OnlineItemDomain {
  return domain === 'comic' || domain === 'music' || domain === 'video'
}

/**
 * Source origin of a persisted online/remote library item.
 * Mirrors Rust `OnlineItemSourceKind` (`#[serde(rename_all = "snake_case")]`).
 */
export type OnlineItemSourceKind = 'online' | 'remote'

/**
 * Server-computed source availability status for an online/remote library item.
 * Mirrors Rust `OnlineItemAvailability` (`#[serde(rename_all = "snake_case")]`).
 */
export type OnlineItemAvailability = 'ok' | 'source_disabled' | 'source_missing' | 'unverified'

/**
 * Filter view for listing online library items.
 * Mirrors Rust `OnlineItemView` (`#[serde(rename_all = "snake_case")]`).
 */
export type OnlineItemView = 'all' | 'continue' | 'favorites'

/**
 * Persisted reading/playback position inside an online or remote item.
 * Mirrors Rust `OnlineItemProgress` in `crates/xmedia-core/src/domain/online_item.rs`:
 * - comic: `unit_*` = chapter, `position` = 0-based page, `duration` = page count
 * - video: `unit_*` = episode (`unit_group` = play line), `position`/`duration` = seconds
 * - music: no unit, `position`/`duration` = seconds
 */
export interface OnlineProgress {
  unit_index?: number | null
  unit_key?: string | null
  unit_title?: string | null
  unit_group?: string | null
  position: number
  duration?: number | null
  total_units?: number | null
  completed: boolean
}

/** Alias matching the exact Rust struct name `OnlineItemProgress`. */
export type OnlineItemProgress = OnlineProgress

/**
 * Persisted per-user online/remote item returned by `/api/v1/{domain}/online/items`.
 * Mirrors Rust `OnlineItemDto` in `crates/xmedia-core/src/application/online_items.rs`.
 */
export interface OnlineItem {
  id: string
  media_kind: MediaKind
  source_kind: OnlineItemSourceKind
  source_id: string
  source_name: string | null
  availability: OnlineItemAvailability
  item_key: string
  title: string
  subtitle: string | null
  cover_url: string | null
  intro: string | null
  kind: string | null
  extra: Record<string, unknown>
  favorite: boolean
  favorited_at: string | null
  progress: OnlineProgress | null
  progress_ratio: number | null
  progress_at: string | null
  created_at: string
  updated_at: string
}

/** Alias matching the exact Rust DTO name `OnlineItemDto`. */
export type OnlineItemDto = OnlineItem

/** Mirrors Rust `OnlineItemListDto`. */
export interface OnlineItemListDto {
  items: OnlineItem[]
  total: number
}

/** Mirrors Rust `OnlineItemQuery`. */
export interface OnlineItemQuery {
  view?: OnlineItemView
  source_kind?: OnlineItemSourceKind
  source_id?: string
  q?: string
  limit?: number
  offset?: number
}

/** Mirrors Rust `OnlineItemRefInput`. */
export interface OnlineItemRefInput {
  source_kind?: OnlineItemSourceKind
  source_id: string
  item_key: string
}

/** Mirrors Rust `LookupOnlineItemsInput`. */
export interface LookupOnlineItemsInput {
  items: OnlineItemRefInput[]
}

/** Mirrors Rust `UpsertOnlineItemInput`. */
export interface UpsertOnlineItemInput {
  source_kind?: OnlineItemSourceKind
  source_id: string
  item_key: string
  title: string
  subtitle?: string | null
  cover_url?: string | null
  intro?: string | null
  kind?: string | null
  extra?: Record<string, unknown> | null
  favorite?: boolean
  progress?: OnlineProgress | null
}

/** Mirrors Rust `PatchOnlineItemInput`. */
export interface PatchOnlineItemInput {
  favorite?: boolean
  title?: string
  subtitle?: string | null
  cover_url?: string | null
  intro?: string | null
  kind?: string | null
  extra?: Record<string, unknown> | null
}

/**
 * Only `source_disabled` and `source_missing` block playback/reading.
 * Both `ok` and `unverified` (used by remote OPDS/Subsonic items) are playable.
 */
export function isAvailabilityBlocked(
  availability: OnlineItemAvailability | null | undefined,
): boolean {
  return availability === 'source_disabled' || availability === 'source_missing'
}

// ---------------------------------------------------------------------------
// Unified Frontend Source & Item Contract (§3.1 / §3.2)
// ---------------------------------------------------------------------------

export type SourceScope = 'local' | 'online' | 'remote'
export type SourceHealth = 'ok' | 'failing' | 'untested'
export type SourceProtocol = 'rule' | 'opds' | 'subsonic'

/** URL-backed source filter: omitted `scope` defaults to `'local'`. */
export interface SourceFilter {
  scope: SourceScope
  sourceId: string | null
}

export interface SourceCapabilities {
  search: boolean
  /** Can browse categories / recommendations when `q` is empty. */
  explore: boolean
  /** Supports persisted favorites and reading/playback progress. */
  track: boolean
  /** Supports importing / downloading into a local library. */
  import: boolean
}

export interface SourceDescriptor {
  scope: SourceScope
  /** Always `'local'` for the local library scope. */
  id: string
  domain: MediaKind
  name: string
  enabled: boolean
  health?: SourceHealth
  protocol?: SourceProtocol
  capabilities: SourceCapabilities
}

export interface ItemHints {
  title?: string
  cover?: string
  author?: string
  extra?: Record<string, string>
}

export interface LocalItemRef {
  scope: 'local'
  domain: MediaKind
  id: string
}

export interface OnlineItemRef {
  scope: 'online'
  domain: MediaKind
  sourceId: string
  ref: string
  hints?: ItemHints
}

export interface RemoteItemRef {
  scope: 'remote'
  domain: MediaKind
  sourceId: string
  ref: string
  hints?: ItemHints
}

export type TrackedItemRef = OnlineItemRef | RemoteItemRef
export type ItemRef = LocalItemRef | OnlineItemRef | RemoteItemRef

/** CamelCase playback/reading position for ergonomic frontend usage. */
export interface PlayPosition {
  unitIndex?: number
  unitKey?: string
  unitTitle?: string
  unitGroup?: string
  /** comic: 0-based page; video/music: seconds. */
  position: number
  /** comic: total pages; video/music: total seconds. */
  duration?: number
  totalUnits?: number
  completed?: boolean
}

/** CamelCase snapshot of a persisted `OnlineItem`. */
export interface OnlineItemSnapshot {
  id: string
  mediaKind: MediaKind
  sourceKind: OnlineItemSourceKind
  sourceId: string
  sourceName: string
  availability: OnlineItemAvailability
  blocked: boolean
  itemKey: string
  title: string
  subtitle?: string
  coverUrl?: string
  intro?: string
  kind?: string
  extra?: Record<string, unknown>
  favorite: boolean
  favoritedAt?: string
  progress: PlayPosition | null
  progressRatio: number | null
  progressAt?: string
  createdAt: string
  updatedAt: string
}

/** Metadata passed to `useOnlineItemState` when upserting a newly tracked item. */
export interface OnlineItemMetadataInput {
  title: string
  subtitle?: string | null
  coverUrl?: string | null
  intro?: string | null
  kind?: string | null
  extra?: Record<string, unknown> | null
}

export interface MediaItemBadge {
  source?: string
  sourceKind?: SourceScope
  progressPercent?: number
  status?: string
  availability?: OnlineItemAvailability
  blocked?: boolean
}

/** Unified card / list-item view model consumed by all 4 media domains. */
export interface MediaItemVM {
  ref: ItemRef
  title: string
  subtitle?: string
  cover?: string
  badge?: MediaItemBadge
  to: RouteLocationRaw
  playTo?: RouteLocationRaw
  /** Epoch milliseconds used for cross-source chronological merging. */
  sortTimestamp?: number
}

export interface SearchGroup {
  key: string
  domain: MediaKind
  scope: SourceScope
  title: string
  status: 'idle' | 'loading' | 'ok' | 'error'
  error?: string
  items: MediaItemVM[]
  total?: number
  seeAll: RouteLocationRaw
}

export interface OnlineQuickAction {
  domain: MediaKind
  label: string
  subtitle?: string
  to: RouteLocationRaw
}

// ---------------------------------------------------------------------------
// Conversion helpers between Rust snake_case DTOs and camelCase frontend models
// ---------------------------------------------------------------------------

export function playPositionToOnlineProgress(pos: PlayPosition): OnlineProgress {
  const safePosition = Number.isFinite(pos.position) ? Math.max(0, pos.position) : 0
  const safeDuration =
    pos.duration !== undefined && Number.isFinite(pos.duration) && pos.duration > 0
      ? pos.duration
      : null
  const safeUnitIndex =
    pos.unitIndex !== undefined && Number.isFinite(pos.unitIndex)
      ? Math.max(0, Math.floor(pos.unitIndex))
      : null
  const safeTotalUnits =
    pos.totalUnits !== undefined && Number.isFinite(pos.totalUnits) && pos.totalUnits > 0
      ? Math.floor(pos.totalUnits)
      : null

  return {
    unit_index: safeUnitIndex,
    unit_key: pos.unitKey?.trim() || null,
    unit_title: pos.unitTitle?.trim() || null,
    unit_group: pos.unitGroup?.trim() || null,
    position: safePosition,
    duration: safeDuration,
    total_units: safeTotalUnits,
    completed: Boolean(pos.completed),
  }
}

export function onlineProgressToPlayPosition(progress: OnlineProgress): PlayPosition {
  return {
    unitIndex: progress.unit_index ?? undefined,
    unitKey: progress.unit_key ?? undefined,
    unitTitle: progress.unit_title ?? undefined,
    unitGroup: progress.unit_group ?? undefined,
    position: progress.position,
    duration: progress.duration ?? undefined,
    totalUnits: progress.total_units ?? undefined,
    completed: progress.completed,
  }
}

export function toOnlineItemSnapshot(dto: OnlineItem): OnlineItemSnapshot {
  return {
    id: dto.id,
    mediaKind: dto.media_kind,
    sourceKind: dto.source_kind,
    sourceId: dto.source_id,
    sourceName: dto.source_name ?? '',
    availability: dto.availability,
    blocked: isAvailabilityBlocked(dto.availability),
    itemKey: dto.item_key,
    title: dto.title,
    subtitle: dto.subtitle ?? undefined,
    coverUrl: dto.cover_url ?? undefined,
    intro: dto.intro ?? undefined,
    kind: dto.kind ?? undefined,
    extra: dto.extra,
    favorite: dto.favorite,
    favoritedAt: dto.favorited_at ?? undefined,
    progress: dto.progress ? onlineProgressToPlayPosition(dto.progress) : null,
    progressRatio: dto.progress_ratio,
    progressAt: dto.progress_at ?? undefined,
    createdAt: dto.created_at,
    updatedAt: dto.updated_at,
  }
}

/**
 * Validates a cover URL against the backend rules in `validate_cover_url`:
 * accepts only `http(s)://...` or site-relative `/...` (not `//...` or `/\...`),
 * no whitespace or control characters, max 2048 chars.
 */
export function sanitizeCoverUrlForApi(raw: string | null | undefined): string | null {
  if (!raw) return null
  const trimmed = raw.trim()
  if (!trimmed || trimmed.length > 2048) return null
  if (/[\s\u0000-\u001f\u007f]/.test(trimmed)) return null
  const lower = trimmed.toLowerCase()
  const isHttp = lower.startsWith('http://') || lower.startsWith('https://')
  const isLocalPath =
    trimmed.startsWith('/') && !trimmed.startsWith('//') && !trimmed.startsWith('/\\')
  return isHttp || isLocalPath ? trimmed : null
}
