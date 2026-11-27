import type { ComputedRef, MaybeRefOrGetter, Ref } from 'vue'
import type {
  MediaItemVM,
  OnlineItemSnapshot,
  TrackedItemRef,
  UseOnlineItemStatesReturn,
} from '@/sources'
import type { OnlineDomain, OnlineSourceSummaryDto } from './types'

export interface OnlineBrowseCategory {
  id: string
  name: string
  url?: string
}

export interface OnlineSearchErrorItem {
  source_id: string
  source_name: string
  error: string
}

export interface OnlineSearchResultPage<T> {
  results: T[]
  page?: number
  hasMore?: boolean
  errors?: OnlineSearchErrorItem[]
  toasts?: string[]
}

export interface OnlineExploreCategoriesResult {
  categories: OnlineBrowseCategory[]
  /** Structured backend capability flag; avoids brittle message matching. */
  hasExplore?: boolean
}

export interface OnlineBrowseResultPage<T> {
  results: T[]
  page?: number
  pageCount?: number | null
  hasMore?: boolean
  sourceId?: string
  sourceName?: string
}

export interface OnlineSearchBrowseAdapter<
  T,
  S extends OnlineSourceSummaryDto = OnlineSourceSummaryDto,
> {
  domain: OnlineDomain
  /** Per-user plugin list endpoint (`GET /plugins?domain={domain}`). */
  listSources: () => Promise<S[]>
  /** Aggregated or source-scoped online search. */
  search: (params: {
    keyword: string
    sourceIds: string[]
    page: number
    signal: AbortSignal
  }) => Promise<OnlineSearchResultPage<T>>
  /** Optional explore/category metadata for a source when `q` is empty. */
  listCategories?: (
    sourceId: string,
    signal: AbortSignal,
  ) => Promise<OnlineExploreCategoriesResult>
  /** Optional explore/category browse items for a source when `q` is empty. */
  browse?: (params: {
    sourceId: string
    category: OnlineBrowseCategory | null
    page: number
    signal: AbortSignal
  }) => Promise<OnlineBrowseResultPage<T>>
  /** Stable deduplication key used when appending paginated results. */
  getItemKey: (item: T) => string
  /** Optional mapper to `TrackedItemRef` for batch `useOnlineItemStates` hydration. */
  toTrackedRef?: (item: T) => TrackedItemRef | null
  /** Optional mapper to unified `MediaItemVM` with hydrated favorite/progress state. */
  toMediaItemVM?: (item: T, state: OnlineItemSnapshot | null) => MediaItemVM
  /** Automatically browse explore/categories when keyword is empty. Default: `Boolean(browse)`. */
  autoExploreOnEmpty?: boolean
  /** Pick first enabled source for explore when "All sources" is selected. Default: `true`. */
  defaultExploreFirstSource?: boolean
  /** Keyword input debounce (ms). Default: `250`. */
  debounceMs?: number
  /** Auto-initialize on mount. Default: `true`. */
  autoInit?: boolean
}

export interface UseOnlineSearchBrowseOptions {
  keyword?: MaybeRefOrGetter<string>
  initialSourceId?: MaybeRefOrGetter<string | null | undefined>
  onKeywordChange?: (keyword: string) => void
  onSourceIdChange?: (sourceId: string | null) => void
}

export interface UseOnlineSearchBrowseReturn<
  T,
  S extends OnlineSourceSummaryDto = OnlineSourceSummaryDto,
> {
  loadingInit: Ref<boolean>
  sources: Ref<S[]>
  enabledSources: ComputedRef<S[]>
  sourcesLoadError: Ref<string | null>
  keyword: Ref<string>
  selectedSourceIds: Ref<string[]>
  activeBrowseSourceId: ComputedRef<string | null>
  searching: Ref<boolean>
  loadingMore: Ref<boolean>
  searchPage: Ref<number>
  hasMoreResults: Ref<boolean>
  hasSearched: Ref<boolean>
  lastSearchKeyword: Ref<string>
  searchResults: Ref<T[]>
  searchErrors: Ref<OnlineSearchErrorItem[]>
  searchToasts: Ref<string[]>
  browsing: Ref<boolean>
  loadingMoreBrowse: Ref<boolean>
  browsePage: Ref<number>
  hasMoreBrowse: Ref<boolean>
  browseCategories: Ref<OnlineBrowseCategory[]>
  activeCategoryId: Ref<string | null>
  browseItems: Ref<T[]>
  browseError: Ref<string | null>
  exploreSupported: Ref<boolean>
  showBrowseView: ComputedRef<boolean>
  displayItems: ComputedRef<T[]>
  displayVMs: ComputedRef<MediaItemVM[]>
  itemStates: UseOnlineItemStatesReturn
  isBusy: ComputedRef<boolean>
  isLoadingPrimary: ComputedRef<boolean>
  activeHasMore: ComputedRef<boolean>
  activeLoadingMore: ComputedRef<boolean>
  init: () => Promise<void>
  retryInit: () => Promise<void>
  setKeyword: (value: string, opts?: { debounce?: boolean }) => void
  submitSearch: (overrideKeyword?: string) => Promise<void>
  clearSearch: () => void
  retrySearch: () => Promise<void>
  toggleSource: (sourceId: string) => void
  selectSingleSource: (sourceId: string | null) => void
  clearSourceFilter: () => void
  selectCategory: (categoryId: string | null) => Promise<void>
  triggerExplore: (sourceId?: string | null) => Promise<void>
  retryBrowse: () => Promise<void>
  loadMore: () => Promise<void>
}

export function isAbortError(err: unknown): boolean {
  if (err instanceof DOMException && err.name === 'AbortError') return true
  return err instanceof Error && err.name === 'AbortError'
}

export function dedupeAppendItems<T>(
  existing: readonly T[],
  incoming: readonly T[],
  getKey: (item: T) => string,
): { merged: T[]; appendedCount: number } {
  const seen = new Set(existing.map(getKey))
  const appended = incoming.filter((item) => !seen.has(getKey(item)))
  return {
    merged: [...existing, ...appended],
    appendedCount: appended.length,
  }
}

export function hydrateMediaItemVMWithState(
  vm: MediaItemVM,
  state: OnlineItemSnapshot | null,
): MediaItemVM {
  if (!state) return vm
  const ratioPct =
    state.progressRatio !== null && Number.isFinite(state.progressRatio)
      ? Math.round(state.progressRatio * 100)
      : undefined
  return {
    ...vm,
    badge: {
      ...vm.badge,
      source: vm.badge?.source || state.sourceName || undefined,
      sourceKind: vm.badge?.sourceKind ?? state.sourceKind,
      progressPercent: vm.badge?.progressPercent ?? ratioPct,
      availability: vm.badge?.availability ?? state.availability,
      blocked: vm.badge?.blocked ?? state.blocked,
    },
  }
}
