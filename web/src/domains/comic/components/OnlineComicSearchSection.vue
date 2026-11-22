<script setup lang="ts">
import { ref } from 'vue'
import { useI18n as useGlobalI18n } from 'vue-i18n'
import OnlineMediaSearchShell from '@/components/online-sources/OnlineMediaSearchShell.vue'
import {
  useOnlineSearchBrowse,
  type OnlineBrowseCategory,
} from '@/components/online-sources/useOnlineSearchBrowse'
import { useComicI18n as useI18n } from '@/domains/comic/i18n'
import {
  onlineComicApi,
  type ComicSearchResultItem,
  type ComicSourceSummaryDto,
  type OnlineComicExploreResponseDto,
} from '@/domains/comic/onlineApi'
import {
  onlineSearchItemToMediaItemVM,
  onlineSearchItemToTrackedRef,
  saveOnlineComicContext,
} from '@/domains/comic/utils/mappers'
import {
  onlineItemsApi,
  sanitizeCoverUrlForApi,
  seedOnlineItemStateCache,
  type MediaItemVM,
} from '@/sources'
import { useSnackbarStore } from '@/stores/snackbar'

const props = withDefaults(
  defineProps<{
    keyword: string
    initialSourceId?: string | null
    showSearchInput?: boolean
  }>(),
  {
    initialSourceId: null,
    showSearchInput: false,
  },
)

const emit = defineEmits<{
  'update:keyword': [value: string]
  'update:sourceId': [value: string | null]
}>()

const { t } = useI18n()
const { locale } = useGlobalI18n()
const snackbar = useSnackbarStore()

const togglingKeys = ref(new Set<string>())
let cachedInitialExplore: {
  sourceId: string
  res: OnlineComicExploreResponseDto
  ts: number
} | null = null

const browse = useOnlineSearchBrowse<ComicSearchResultItem, ComicSourceSummaryDto>(
  {
    domain: 'comic',
    listSources: () => onlineComicApi.listSources(),
    search: async ({ keyword, sourceIds, page, signal }) => {
      const res = await onlineComicApi.search(keyword, sourceIds, page, { signal })
      return {
        results: res.results ?? [],
        page: res.page || page,
        hasMore: (res.results?.length ?? 0) > 0,
        errors: res.errors ?? [],
        toasts: res.toasts ?? [],
      }
    },
    listCategories: async (sourceId, signal) => {
      const res = await onlineComicApi.explore(sourceId, null, 1, { signal })
      cachedInitialExplore = { sourceId, res, ts: Date.now() }
      const categories: OnlineBrowseCategory[] = (res.categories ?? []).map((cat, idx) => ({
        id: cat.url || `cat-${idx}-${cat.title}`,
        name: cat.title,
        url: cat.url,
      }))
      const hasExplore = categories.length > 0 || (res.items?.length ?? 0) > 0
      return { categories, hasExplore }
    },
    browse: async ({ sourceId, category, page, signal }) => {
      if (
        !category &&
        page === 1 &&
        cachedInitialExplore &&
        cachedInitialExplore.sourceId === sourceId &&
        Date.now() - cachedInitialExplore.ts < 5000
      ) {
        const res = cachedInitialExplore.res
        cachedInitialExplore = null
        return {
          results: res.items ?? [],
          page: res.page || 1,
          hasMore: (res.items?.length ?? 0) > 0,
          sourceId,
        }
      }
      cachedInitialExplore = null
      const res = await onlineComicApi.explore(sourceId, category?.url ?? null, page, { signal })
      return {
        results: res.items ?? [],
        page: res.page || page,
        hasMore: (res.items?.length ?? 0) > 0,
        sourceId,
      }
    },
    getItemKey: (item) => `${item.source_id}:${item.comic_url}`,
    toTrackedRef: onlineSearchItemToTrackedRef,
    toMediaItemVM: onlineSearchItemToMediaItemVM,
    autoExploreOnEmpty: true,
    defaultExploreFirstSource: true,
  },
  {
    keyword: () => props.keyword,
    initialSourceId: () => props.initialSourceId,
    onKeywordChange: (next) => emit('update:keyword', next),
    onSourceIdChange: (next) => emit('update:sourceId', next),
  },
)

function findRawItem(vm: MediaItemVM): ComicSearchResultItem | undefined {
  if (vm.ref.scope !== 'online') return undefined
  const { sourceId, ref } = vm.ref
  return browse.displayItems.value.find(
    (item) => item.source_id === sourceId && item.comic_url === ref,
  )
}

function handleSelectItem(vm: MediaItemVM): void {
  if (vm.ref.scope !== 'online') return
  const raw = findRawItem(vm)
  const activeKw = (browse.lastSearchKeyword.value || browse.keyword.value).trim()
  const activeSrc =
    browse.selectedSourceIds.value.length === 1
      ? (browse.selectedSourceIds.value[0] ?? null)
      : null

  saveOnlineComicContext({
    sourceId: vm.ref.sourceId,
    sourceName: raw?.source_name || vm.badge?.source || undefined,
    comicUrl: vm.ref.ref,
    title: raw?.title || vm.title,
    author: raw?.author || vm.ref.hints?.author,
    coverUrl: raw?.cover_url ?? vm.ref.hints?.cover ?? null,
    intro: raw?.intro ?? null,
    returnKeyword: activeKw || undefined,
    returnSourceId: activeSrc,
  })
}

async function applyFavorite(vm: MediaItemVM, nextFavorite: boolean, showUndo: boolean): Promise<void> {
  if (vm.ref.scope !== 'online') return
  const lockKey = `${vm.ref.sourceId}:${vm.ref.ref}`
  if (togglingKeys.value.has(lockKey)) return
  const nextLock = new Set(togglingKeys.value)
  nextLock.add(lockKey)
  togglingKeys.value = nextLock

  const raw = findRawItem(vm)
  try {
    const saved = await onlineItemsApi.upsert('comic', {
      source_kind: 'online',
      source_id: vm.ref.sourceId,
      item_key: vm.ref.ref,
      title: raw?.title || vm.title,
      subtitle: raw?.author || vm.ref.hints?.author || null,
      cover_url: sanitizeCoverUrlForApi(raw?.cover_url ?? vm.ref.hints?.cover ?? null),
      intro: raw?.intro ?? null,
      kind: raw?.kind ?? null,
      extra: {
        source_name: raw?.source_name || vm.badge?.source || undefined,
      },
      favorite: nextFavorite,
    })
    seedOnlineItemStateCache([saved])

    if (!nextFavorite && showUndo) {
      const isZh = locale.value.startsWith('zh')
      snackbar.show({
        message: isZh ? '已移除收藏' : 'Removed from favorites',
        actionLabel: isZh ? '撤销' : 'Undo',
        onAction: async () => {
          await applyFavorite(vm, true, false)
        },
      })
    }
  } catch (err) {
    snackbar.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  } finally {
    const cleaned = new Set(togglingKeys.value)
    cleaned.delete(lockKey)
    togglingKeys.value = cleaned
  }
}

async function handleToggleFavorite(vm: MediaItemVM): Promise<void> {
  if (vm.ref.scope !== 'online') return
  const currentFav = browse.itemStates.isFavorite(vm.ref)
  await applyFavorite(vm, !currentFav, true)
}

defineExpose({
  submitSearch: (kw?: string) => browse.submitSearch(kw),
  clearSearch: () => browse.clearSearch(),
  refresh: () => browse.retryInit(),
})
</script>

<template>
  <OnlineMediaSearchShell
    domain="comic"
    :keyword="browse.keyword.value"
    :show-search-input="showSearchInput"
    :search-placeholder="t('online.searchPlaceholder')"
    :loading-init="browse.loadingInit.value"
    :sources-load-error="browse.sourcesLoadError.value"
    :enabled-sources="browse.enabledSources.value"
    :selected-source-ids="browse.selectedSourceIds.value"
    :searching="browse.searching.value"
    :browsing="browse.browsing.value"
    :has-searched="browse.hasSearched.value"
    :last-search-keyword="browse.lastSearchKeyword.value"
    :search-errors="browse.searchErrors.value"
    :search-toasts="browse.searchToasts.value"
    :show-browse-view="browse.showBrowseView.value"
    :browse-categories="browse.browseCategories.value"
    :active-category-id="browse.activeCategoryId.value"
    :browse-error="browse.browseError.value"
    :items="browse.displayVMs.value"
    :item-states="browse.itemStates"
    item-aspect="poster"
    item-cover-style="book"
    :show-favorite-on-cards="true"
    :has-more="browse.activeHasMore.value"
    :loading-more="browse.activeLoadingMore.value"
    :settings-to="{ path: '/settings/sources', query: { domain: 'comic' } }"
    :idle-hint="t('online.exploreEmpty')"
    @update:keyword="browse.setKeyword($event)"
    @search-submit="browse.submitSearch()"
    @search-clear="browse.clearSearch()"
    @toggle-source="browse.toggleSource($event)"
    @clear-sources="browse.clearSourceFilter()"
    @select-category="browse.selectCategory($event)"
    @retry-init="browse.retryInit()"
    @retry-browse="browse.retryBrowse()"
    @retry-search="browse.retrySearch()"
    @load-more="browse.loadMore()"
    @select-item="handleSelectItem"
    @toggle-favorite="handleToggleFavorite"
  />
</template>
