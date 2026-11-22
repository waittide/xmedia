import {
  computed,
  onMounted,
  onScopeDispose,
  ref,
  toValue,
  watch,
  type ComputedRef,
  type MaybeRefOrGetter,
  type Ref,
} from 'vue'
import { useI18n } from 'vue-i18n'
import {
  useOnlineItemStates,
  type MediaItemVM,
  type OnlineItemSnapshot,
  type TrackedItemRef,
  type UseOnlineItemStatesReturn,
} from '@/sources'
import { useSnackbarStore } from '@/stores/snackbar'
import {
  extractApiError,
  type OnlineDomain,
  type OnlineSourceSummaryDto,
} from './types'

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
  /** Member-readable source list endpoint (`GET /{domain}/sources`, never admin-only). */
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

function isAbortError(err: unknown): boolean {
  if (err instanceof DOMException && err.name === 'AbortError') return true
  return err instanceof Error && err.name === 'AbortError'
}

export function useOnlineSearchBrowse<
  T,
  S extends OnlineSourceSummaryDto = OnlineSourceSummaryDto,
>(
  adapter: OnlineSearchBrowseAdapter<T, S>,
  options: UseOnlineSearchBrowseOptions = {},
): UseOnlineSearchBrowseReturn<T, S> {
  const snackbar = useSnackbarStore()
  const { locale } = useI18n()
  const isZh = computed(() => locale.value.startsWith('zh'))

  const autoExploreOnEmpty = adapter.autoExploreOnEmpty ?? Boolean(adapter.browse)
  const defaultExploreFirstSource = adapter.defaultExploreFirstSource ?? true
  const debounceMs = adapter.debounceMs ?? 250
  const autoInit = adapter.autoInit ?? true

  const loadingInit = ref(false)
  const sources = ref<S[]>([]) as Ref<S[]>
  const sourcesLoadError = ref<string | null>(null)

  const keyword = ref(toValue(options.keyword) ?? '')
  const selectedSourceIds = ref<string[]>([])

  const searching = ref(false)
  const loadingMore = ref(false)
  const searchPage = ref(1)
  const hasMoreResults = ref(false)
  const hasSearched = ref(false)
  const lastSearchKeyword = ref('')
  const lastSearchSourceIds = ref<string[]>([])
  const searchResults = ref<T[]>([]) as Ref<T[]>
  const searchErrors = ref<OnlineSearchErrorItem[]>([])
  const searchToasts = ref<string[]>([])

  const browsing = ref(false)
  const loadingMoreBrowse = ref(false)
  const browsePage = ref(1)
  const hasMoreBrowse = ref(false)
  const browseCategories = ref<OnlineBrowseCategory[]>([])
  const activeCategoryId = ref<string | null>(null)
  const browseItems = ref<T[]>([]) as Ref<T[]>
  const browseError = ref<string | null>(null)
  const exploreSupported = ref(Boolean(adapter.browse))
  const exploreActivated = ref(autoExploreOnEmpty)

  let initSeq = 0
  let searchSeq = 0
  let browseSeq = 0
  let disposed = false
  let debounceTimer: ReturnType<typeof setTimeout> | null = null
  let searchAbort: AbortController | null = null
  let browseAbort: AbortController | null = null
  let pendingKeywordEmit: string | undefined = undefined
  let pendingSourceEmit: string | null | undefined = undefined

  const enabledSources = computed(() => sources.value.filter((s) => s.enabled))

  const activeBrowseSourceId = computed<string | null>(() => {
    if (!adapter.browse) return null
    if (selectedSourceIds.value.length === 1) {
      return selectedSourceIds.value[0] ?? null
    }
    if (selectedSourceIds.value.length === 0) {
      if (defaultExploreFirstSource) {
        return enabledSources.value[0]?.id ?? null
      }
      return enabledSources.value.length === 1 ? (enabledSources.value[0]?.id ?? null) : null
    }
    return null
  })

  const showBrowseView = computed(
    () =>
      Boolean(adapter.browse) &&
      exploreActivated.value &&
      !hasSearched.value &&
      keyword.value.trim() === '' &&
      enabledSources.value.length > 0 &&
      activeBrowseSourceId.value !== null,
  )

  const displayItems = computed<T[]>(() =>
    hasSearched.value ? searchResults.value : browseItems.value,
  )

  const trackedRefs = computed<TrackedItemRef[]>(() => {
    if (!adapter.toTrackedRef) return []
    const refs: TrackedItemRef[] = []
    for (const item of displayItems.value) {
      const tracked = adapter.toTrackedRef(item)
      if (tracked) refs.push(tracked)
    }
    return refs
  })

  const itemStates = useOnlineItemStates(trackedRefs, {
    autoLoad: Boolean(adapter.toTrackedRef),
  })

  const displayVMs = computed<MediaItemVM[]>(() => {
    if (!adapter.toMediaItemVM) return []
    return displayItems.value.map((item) => {
      const tracked = adapter.toTrackedRef ? adapter.toTrackedRef(item) : null
      const state = tracked ? itemStates.getState(tracked) : null
      const vm = adapter.toMediaItemVM!(item, state)
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
    })
  })

  const isBusy = computed(
    () => searching.value || loadingMore.value || browsing.value || loadingMoreBrowse.value,
  )
  const isLoadingPrimary = computed(
    () => searching.value || (browsing.value && displayItems.value.length === 0),
  )
  const activeHasMore = computed(() =>
    hasSearched.value ? hasMoreResults.value : hasMoreBrowse.value,
  )
  const activeLoadingMore = computed(() =>
    hasSearched.value ? loadingMore.value : loadingMoreBrowse.value,
  )

  function clearDebounce(): void {
    if (debounceTimer !== null) {
      clearTimeout(debounceTimer)
      debounceTimer = null
    }
  }

  function abortSearch(): void {
    if (searchAbort) {
      searchAbort.abort()
      searchAbort = null
    }
  }

  function abortBrowse(): void {
    if (browseAbort) {
      browseAbort.abort()
      browseAbort = null
    }
  }

  function notifyKeywordChange(next: string): void {
    const propVal = toValue(options.keyword)
    pendingKeywordEmit = propVal !== undefined && next !== propVal ? next : undefined
    options.onKeywordChange?.(next)
  }

  function notifySourceIdChange(next: string | null): void {
    const propVal = toValue(options.initialSourceId) ?? null
    pendingSourceEmit = next !== propVal ? next : undefined
    options.onSourceIdChange?.(next)
  }

  function resetSearchState(): void {
    searchSeq += 1
    abortSearch()
    searching.value = false
    loadingMore.value = false
    hasSearched.value = false
    lastSearchKeyword.value = ''
    lastSearchSourceIds.value = []
    searchPage.value = 1
    searchResults.value = []
    searchErrors.value = []
    searchToasts.value = []
    hasMoreResults.value = false
  }

  function resetBrowseState(): void {
    browseSeq += 1
    abortBrowse()
    browsing.value = false
    loadingMoreBrowse.value = false
    browsePage.value = 1
    hasMoreBrowse.value = false
    activeCategoryId.value = null
    browseCategories.value = []
    browseItems.value = []
    browseError.value = null
  }

  function findCategory(categoryId: string | null): OnlineBrowseCategory | null {
    if (!categoryId) return null
    return browseCategories.value.find((cat) => cat.id === categoryId) ?? null
  }

  async function runBrowse(
    sourceId: string,
    categoryId: string | null,
    page: number,
    mode: 'replace' | 'append',
    fetchCategories = false,
  ): Promise<void> {
    if (!adapter.browse) return
    if (mode === 'append' && (browsing.value || loadingMoreBrowse.value)) return

    const seq = ++browseSeq
    if (mode === 'replace') {
      abortBrowse()
    }
    const controller = new AbortController()
    browseAbort = controller

    exploreActivated.value = true
    if (mode === 'append') {
      loadingMoreBrowse.value = true
    } else {
      browsing.value = true
      loadingMoreBrowse.value = false
      browseError.value = null
      activeCategoryId.value = categoryId
    }

    try {
      if (fetchCategories && adapter.listCategories) {
        browseCategories.value = []
        const catRes = await adapter.listCategories(sourceId, controller.signal)
        if (disposed || seq !== browseSeq) return
        browseCategories.value = catRes.categories ?? []
        const hasExplore = catRes.hasExplore ?? (browseCategories.value.length > 0 || true)
        exploreSupported.value = hasExplore
        if (browseCategories.value.length === 0 && catRes.hasExplore === false && categoryId === null) {
          browseItems.value = []
          hasMoreBrowse.value = false
          browsePage.value = 1
          return
        }
      }

      const targetCategory = findCategory(categoryId)
      const res = await adapter.browse({
        sourceId,
        category: targetCategory,
        page,
        signal: controller.signal,
      })
      if (disposed || seq !== browseSeq) return

      const incoming = res.results ?? []
      const currentPage = res.page ?? page
      browsePage.value = currentPage
      const hasMoreByPage =
        res.hasMore !== undefined
          ? res.hasMore
          : res.pageCount != null
            ? currentPage < res.pageCount
            : incoming.length > 0

      if (mode === 'append') {
        const seen = new Set(browseItems.value.map(adapter.getItemKey))
        const appended = incoming.filter((item) => !seen.has(adapter.getItemKey(item)))
        browseItems.value = [...browseItems.value, ...appended]
        hasMoreBrowse.value = appended.length > 0 && hasMoreByPage
        if (appended.length === 0) {
          snackbar.show({ message: isZh.value ? '没有更多结果了' : 'No more results' })
        }
      } else {
        browseItems.value = incoming
        hasMoreBrowse.value = hasMoreByPage
      }
    } catch (err) {
      if (disposed || seq !== browseSeq || isAbortError(err)) return
      browseError.value = extractApiError(err)
      if (mode === 'replace') {
        browseItems.value = []
        hasMoreBrowse.value = false
      }
    } finally {
      if (browseAbort === controller) {
        browseAbort = null
      }
      if (!disposed && seq === browseSeq) {
        browsing.value = false
        loadingMoreBrowse.value = false
      }
    }
  }

  async function runSearch(page: number, mode: 'replace' | 'append'): Promise<void> {
    const kw = (
      mode === 'replace' && page === 1
        ? keyword.value
        : lastSearchKeyword.value || keyword.value
    ).trim()

    if (!kw || enabledSources.value.length === 0) {
      if (!kw) {
        resetSearchState()
        if (autoExploreOnEmpty && activeBrowseSourceId.value) {
          await runBrowse(activeBrowseSourceId.value, activeCategoryId.value, 1, 'replace', true)
        }
      }
      return
    }

    if (mode === 'append' && (searching.value || loadingMore.value)) return

    const srcIds =
      mode === 'replace' && page === 1 ? [...selectedSourceIds.value] : [...lastSearchSourceIds.value]

    const seq = ++searchSeq
    if (mode === 'replace') {
      abortSearch()
    }
    const controller = new AbortController()
    searchAbort = controller

    lastSearchKeyword.value = kw
    lastSearchSourceIds.value = srcIds

    if (mode === 'append') {
      loadingMore.value = true
    } else {
      searching.value = true
      loadingMore.value = false
      hasSearched.value = true
      searchResults.value = []
      hasMoreResults.value = false
      searchErrors.value = []
      searchToasts.value = []
    }

    try {
      const res = await adapter.search({
        keyword: kw,
        sourceIds: srcIds,
        page,
        signal: controller.signal,
      })
      if (disposed || seq !== searchSeq) return

      searchPage.value = res.page ?? page
      const incoming = res.results ?? []

      if (mode === 'append') {
        const seen = new Set(searchResults.value.map(adapter.getItemKey))
        const appended = incoming.filter((item) => !seen.has(adapter.getItemKey(item)))
        searchResults.value = [...searchResults.value, ...appended]
        hasMoreResults.value = (res.hasMore ?? true) && appended.length > 0
        if (appended.length === 0) {
          snackbar.show({ message: isZh.value ? '没有更多结果了' : 'No more results' })
        }
      } else {
        searchResults.value = incoming
        hasMoreResults.value = res.hasMore ?? incoming.length > 0
      }

      searchErrors.value = res.errors ?? []
      searchToasts.value = res.toasts ?? []
    } catch (err) {
      if (disposed || seq !== searchSeq || isAbortError(err)) return
      const msg = extractApiError(err)
      if (mode === 'replace') {
        searchResults.value = []
        hasMoreResults.value = false
        searchErrors.value = [
          {
            source_id: 'search',
            source_name: isZh.value ? '在线来源' : 'Online sources',
            error: msg,
          },
        ]
      }
      snackbar.show({ message: msg, tone: 'error' })
    } finally {
      if (searchAbort === controller) {
        searchAbort = null
      }
      if (!disposed && seq === searchSeq) {
        searching.value = false
        loadingMore.value = false
      }
    }
  }

  async function init(): Promise<void> {
    const seq = ++initSeq
    loadingInit.value = true
    sourcesLoadError.value = null
    try {
      const list = await adapter.listSources()
      if (disposed || seq !== initSeq) return
      sources.value = list

      const initialId = toValue(options.initialSourceId)
      if (initialId && enabledSources.value.some((s) => s.id === initialId)) {
        selectedSourceIds.value = [initialId]
      } else {
        selectedSourceIds.value = selectedSourceIds.value.filter((id) =>
          enabledSources.value.some((s) => s.id === id),
        )
      }
    } catch (err) {
      if (disposed || seq !== initSeq) return
      sources.value = []
      sourcesLoadError.value = extractApiError(err)
    } finally {
      if (!disposed && seq === initSeq) {
        loadingInit.value = false
      }
    }

    if (disposed || seq !== initSeq || enabledSources.value.length === 0) return

    if (keyword.value.trim()) {
      void runSearch(1, 'replace')
    } else if (autoExploreOnEmpty && activeBrowseSourceId.value) {
      void runBrowse(activeBrowseSourceId.value, null, 1, 'replace', true)
    }
  }

  async function retryInit(): Promise<void> {
    await init()
  }

  function setKeyword(value: string, opts?: { debounce?: boolean }): void {
    keyword.value = value
    clearDebounce()
    if (!opts?.debounce) {
      notifyKeywordChange(value)
      return
    }
    debounceTimer = setTimeout(() => {
      debounceTimer = null
      if (disposed) return
      notifyKeywordChange(keyword.value)
      if (keyword.value.trim()) {
        void runSearch(1, 'replace')
      } else {
        resetSearchState()
        if (autoExploreOnEmpty && activeBrowseSourceId.value) {
          void runBrowse(activeBrowseSourceId.value, activeCategoryId.value, 1, 'replace', true)
        }
      }
    }, debounceMs)
  }

  async function submitSearch(overrideKeyword?: string): Promise<void> {
    clearDebounce()
    if (overrideKeyword !== undefined) {
      keyword.value = overrideKeyword
    }
    notifyKeywordChange(keyword.value)
    if (loadingInit.value) return
    await runSearch(1, 'replace')
  }

  function clearSearch(): void {
    clearDebounce()
    keyword.value = ''
    notifyKeywordChange('')
    resetSearchState()
    if (autoExploreOnEmpty && activeBrowseSourceId.value) {
      void runBrowse(activeBrowseSourceId.value, activeCategoryId.value, 1, 'replace', true)
    }
  }

  async function retrySearch(): Promise<void> {
    await runSearch(1, 'replace')
  }

  function applySourceSelection(next: string[]): void {
    selectedSourceIds.value = next
    notifySourceIdChange(next.length === 1 ? (next[0] ?? null) : null)

    if (keyword.value.trim()) {
      if (hasSearched.value) {
        void runSearch(1, 'replace')
      }
      return
    }

    resetSearchState()
    activeCategoryId.value = null
    if (exploreActivated.value && activeBrowseSourceId.value) {
      void runBrowse(activeBrowseSourceId.value, null, 1, 'replace', true)
    } else {
      resetBrowseState()
    }
  }

  function toggleSource(sourceId: string): void {
    const next = selectedSourceIds.value.includes(sourceId)
      ? selectedSourceIds.value.filter((id) => id !== sourceId)
      : [...selectedSourceIds.value, sourceId]
    applySourceSelection(next)
  }

  function selectSingleSource(sourceId: string | null): void {
    applySourceSelection(sourceId ? [sourceId] : [])
  }

  function clearSourceFilter(): void {
    applySourceSelection([])
  }

  async function selectCategory(categoryId: string | null): Promise<void> {
    if (!activeBrowseSourceId.value) return
    await runBrowse(activeBrowseSourceId.value, categoryId, 1, 'replace', false)
  }

  async function triggerExplore(sourceId?: string | null): Promise<void> {
    const targetId = sourceId ?? activeBrowseSourceId.value ?? enabledSources.value[0]?.id ?? null
    if (!targetId) return
    if (sourceId && (selectedSourceIds.value.length !== 1 || selectedSourceIds.value[0] !== sourceId)) {
      selectedSourceIds.value = [sourceId]
      notifySourceIdChange(sourceId)
    }
    resetSearchState()
    await runBrowse(targetId, null, 1, 'replace', true)
  }

  async function retryBrowse(): Promise<void> {
    if (!activeBrowseSourceId.value) return
    await runBrowse(activeBrowseSourceId.value, activeCategoryId.value, 1, 'replace', true)
  }

  async function loadMore(): Promise<void> {
    if (hasSearched.value) {
      if (!hasMoreResults.value) return
      await runSearch(searchPage.value + 1, 'append')
    } else if (activeBrowseSourceId.value && hasMoreBrowse.value) {
      await runBrowse(
        activeBrowseSourceId.value,
        activeCategoryId.value,
        browsePage.value + 1,
        'append',
        false,
      )
    }
  }

  if (options.keyword !== undefined) {
    watch(
      () => toValue(options.keyword) ?? '',
      (next) => {
        if (pendingKeywordEmit !== undefined) {
          const expected = pendingKeywordEmit
          pendingKeywordEmit = undefined
          if (next === expected) return
        }
        clearDebounce()
        keyword.value = next
        if (loadingInit.value) return

        const trimmed = next.trim()
        if (trimmed) {
          if (
            (trimmed !== lastSearchKeyword.value || !hasSearched.value) &&
            enabledSources.value.length > 0
          ) {
            void runSearch(1, 'replace')
          }
          return
        }

        if (hasSearched.value || lastSearchKeyword.value) {
          resetSearchState()
          if (autoExploreOnEmpty && activeBrowseSourceId.value) {
            void runBrowse(activeBrowseSourceId.value, activeCategoryId.value, 1, 'replace', true)
          }
        }
      },
    )
  }

  if (options.initialSourceId !== undefined) {
    watch(
      () => toValue(options.initialSourceId) ?? null,
      (nextId) => {
        if (pendingSourceEmit !== undefined) {
          const expected = pendingSourceEmit
          pendingSourceEmit = undefined
          if (nextId === expected) return
        }
        if (nextId && enabledSources.value.some((s) => s.id === nextId)) {
          selectedSourceIds.value = [nextId]
          if (keyword.value.trim() === '' && !hasSearched.value && exploreActivated.value) {
            activeCategoryId.value = null
            void runBrowse(nextId, null, 1, 'replace', true)
          }
        } else if (!nextId && selectedSourceIds.value.length > 0) {
          selectedSourceIds.value = []
          if (keyword.value.trim() === '' && !hasSearched.value && exploreActivated.value) {
            activeCategoryId.value = null
            if (activeBrowseSourceId.value) {
              void runBrowse(activeBrowseSourceId.value, null, 1, 'replace', true)
            } else {
              resetBrowseState()
            }
          }
        }
      },
    )
  }

  if (autoInit) {
    onMounted(() => {
      void init()
    })
  }

  onScopeDispose(() => {
    disposed = true
    clearDebounce()
    abortSearch()
    abortBrowse()
    initSeq += 1
    searchSeq += 1
    browseSeq += 1
  })

  return {
    loadingInit,
    sources,
    enabledSources,
    sourcesLoadError,
    keyword,
    selectedSourceIds,
    activeBrowseSourceId,
    searching,
    loadingMore,
    searchPage,
    hasMoreResults,
    hasSearched,
    lastSearchKeyword,
    searchResults,
    searchErrors,
    searchToasts,
    browsing,
    loadingMoreBrowse,
    browsePage,
    hasMoreBrowse,
    browseCategories,
    activeCategoryId,
    browseItems,
    browseError,
    exploreSupported,
    showBrowseView,
    displayItems,
    displayVMs,
    itemStates,
    isBusy,
    isLoadingPrimary,
    activeHasMore,
    activeLoadingMore,
    init,
    retryInit,
    setKeyword,
    submitSearch,
    clearSearch,
    retrySearch,
    toggleSource,
    selectSingleSource,
    clearSourceFilter,
    selectCategory,
    triggerExplore,
    retryBrowse,
    loadMore,
  }
}
