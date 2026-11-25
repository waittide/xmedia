import type { Component } from 'vue'
import type { RemoteProtocol } from '@/api/appHost'
import type {
  OnlineSourceManagerAdapter,
  OnlineSourceSummaryDto,
} from '@/components/online-sources/types'
import type { BindMediaItemsResultDto, MediaKind } from '@/types/api'
import type { MediaItemVM } from './types'

export type MergedShelfView = 'continue' | 'favorites'

export type TranslateFn = (key: string, named?: Record<string, unknown>) => string

export interface DomainUploadEnqueueOptions {
  comicTargetSeriesId?: string
  comicSeriesTitle?: string
  targetSeriesId?: string
  seriesTitle?: string
  fromDirectory?: boolean
}

export interface DomainUploadExecutionContext {
  options?: DomainUploadEnqueueOptions
  sendXhr: <T>(url: string, formData: FormData) => Promise<T>
}

export interface DomainUploadConfig {
  extensions: readonly string[]
  sidecarExtensions?: readonly string[]
  batchable?: boolean
  allowStemPrefixSidecars?: boolean
  executeUpload: (
    files: File[],
    ctx: DomainUploadExecutionContext,
  ) => Promise<{ targetRoute?: string }>
}

export interface PoolPosterCardVM {
  id: string
  title: string
  subtitle: string
  coverUrl: string | null
  bound: boolean
  badgeText?: string
  openKind?: 'movie' | 'show'
}

export interface PoolTrackItemVM {
  id: string
  title: string
  subtitle: string
  coverUrl: string | null
  formatLabel: string
  durationSecs: number
  albumId: string | null
  bound: boolean
}

export type DomainPoolFetchResult =
  | { kind: 'posters'; items: PoolPosterCardVM[]; total: number }
  | { kind: 'tracks'; items: PoolTrackItemVM[]; total: number }
  | {
      kind: 'video-sections'
      shows: PoolPosterCardVM[]
      movies: PoolPosterCardVM[]
      total: number
    }

export interface DomainPoolQueryParams {
  query?: string
  libraryId?: string
  page: number
  pageSize: number
}

export interface DomainPoolBindInput {
  item_ids?: string[]
  library_id?: string
  user_id?: string | null
}

export interface DomainPoolAdapter {
  pageSize: number
  paginated: boolean
  skeletonMode: 'grid' | 'list'
  fetchPool: (params: DomainPoolQueryParams, t: TranslateFn) => Promise<DomainPoolFetchResult>
  bindItems: (payload: DomainPoolBindInput) => Promise<BindMediaItemsResultDto>
  unbindItem: (id: string, userId: string | null) => Promise<unknown>
  bindAlbum?: (albumId: string, userId: string | null) => Promise<BindMediaItemsResultDto>
}

export interface DomainOnlineSourceComponentsAdapter {
  remoteProtocol: RemoteProtocol | null
  getAdapter: () => OnlineSourceManagerAdapter<OnlineSourceSummaryDto>
  loadEditSheet: () => Promise<{ default: Component }>
  loadDebugSheet: () => Promise<{ default: Component }>
}

export interface DomainBackgroundDownloadTask {
  id: string
  domain: MediaKind
  status: 'queued' | 'running' | 'completed' | 'failed' | 'cancelled'
  isActive: boolean
  sortTime: number
  title: string
  badge?: string
  metaText: string
  secondaryText?: string
  warningText?: string
  errorText?: string
  progress: number | null
  targetRoute?: string
  downloadFileUrl?: string
  downloadFileName?: string
  canCancel: boolean
  canDelete: boolean
  busy: boolean
}

export interface DomainBackgroundTasksProvider {
  getTasks: (t: TranslateFn) => DomainBackgroundDownloadTask[]
  getActiveCount: () => number
  getFailedCount: () => number
  getFinishedCount: () => number
  refresh: () => Promise<void>
  cancelTask: (id: string) => Promise<unknown>
  deleteTask: (id: string) => Promise<unknown>
  clearFinished: () => Promise<void>
  resetState: () => void
}

export interface DomainPlugin {
  kind: MediaKind
  onlineSearchI18n: { titleKey: string; subtitleKey: string }
  ensureLocale: () => Promise<void>
  resetSessionState?: () => void
  fetchHomeOverview: () => Promise<unknown>
  fetchLocalShelfItems: (view: MergedShelfView, limit: number) => Promise<MediaItemVM[]>
  searchLocal: (params: {
    query: string
    limit: number
    signal: AbortSignal
    t: TranslateFn
  }) => Promise<{ items: MediaItemVM[]; total?: number }>
  upload: DomainUploadConfig
  pool: DomainPoolAdapter
  onlineSources: DomainOnlineSourceComponentsAdapter
  getBackgroundTasks?: () => DomainBackgroundTasksProvider
}

export const ALL_DOMAIN_KINDS: readonly MediaKind[] = ['book', 'comic', 'music', 'video']

const registry = new Map<MediaKind, DomainPlugin>()

export function registerDomainPlugin(plugin: DomainPlugin): void {
  registry.set(plugin.kind, plugin)
}

export function tryGetDomainPlugin(kind: MediaKind): DomainPlugin | undefined {
  return registry.get(kind)
}

export function getDomainPlugin(kind: MediaKind): DomainPlugin {
  const plugin = registry.get(kind)
  if (!plugin) {
    throw new Error(`DomainPlugin not registered for media kind "${kind}"`)
  }
  return plugin
}

export function listDomainPlugins(): DomainPlugin[] {
  const ordered: DomainPlugin[] = []
  for (const kind of ALL_DOMAIN_KINDS) {
    const found = registry.get(kind)
    if (found) ordered.push(found)
  }
  return ordered
}
