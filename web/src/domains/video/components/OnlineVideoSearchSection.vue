<script setup lang="ts">
import { computed, toRef } from 'vue'
import OnlineMediaSearchShell from '@/components/online-sources/OnlineMediaSearchShell.vue'
import {
  useOnlineSearchBrowse,
  type OnlineBrowseCategory,
} from '@/components/online-sources/useOnlineSearchBrowse'
import type { MediaItemVM } from '@/sources'
import {
  buildOnlineVideoDetailRoute,
  buildOnlineVideoTrackedRef,
  writeStoredDetailContext,
} from '../composables/useOnlineVideoContext'
import { useVideoI18n as useI18n } from '../i18n'
import {
  onlineVideoApi,
  resolveOnlineVideoPosterUrl,
  type OnlineVideoSearchItemDto,
  type VideoSourceSummaryDto,
} from '../onlineApi'

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

const sourceNameById = new Map<string, string>()

const browse = useOnlineSearchBrowse<OnlineVideoSearchItemDto, VideoSourceSummaryDto>(
  {
    domain: 'video',
    listSources: async () => {
      const list = await onlineVideoApi.listSources()
      sourceNameById.clear()
      for (const s of list) {
        sourceNameById.set(s.id, s.name)
      }
      return list
    },
    search: async ({ keyword, sourceIds, page, signal }) => {
      const res = await onlineVideoApi.search(keyword, sourceIds, page, { signal })
      return {
        results: res.results ?? [],
        page: res.page || page,
        hasMore: (res.results?.length ?? 0) > 0,
        errors: res.errors ?? [],
        toasts: res.toasts ?? [],
      }
    },
    listCategories: async (sourceId, signal) => {
      const res = await onlineVideoApi.categories(sourceId, { signal })
      const categories: OnlineBrowseCategory[] = (res.categories ?? []).map((cat) => ({
        id: cat.id,
        name: cat.name,
      }))
      return {
        categories,
        hasExplore: res.has_explore,
      }
    },
    browse: async ({ sourceId, category, page, signal }) => {
      const res = await onlineVideoApi.browse(sourceId, category?.id ?? null, page, { signal })
      const fallbackName = sourceNameById.get(sourceId) || ''
      const resolvedSourceId = res.source_id || sourceId
      const resolvedSourceName = res.source_name || fallbackName
      const results: OnlineVideoSearchItemDto[] = (res.results ?? []).map((item) => ({
        ...item,
        source_id: resolvedSourceId,
        source_name: resolvedSourceName,
      }))
      const currentPage = res.page || page
      return {
        results,
        page: currentPage,
        pageCount: res.page_count,
        hasMore: res.page_count != null ? currentPage < res.page_count : results.length > 0,
        sourceId: resolvedSourceId,
        sourceName: resolvedSourceName,
      }
    },
    getItemKey: (item) => `${item.source_id}:${item.detail_url || item.vod_id}`,
    toTrackedRef: (item) =>
      buildOnlineVideoTrackedRef({
        sourceId: item.source_id,
        detailUrl: item.detail_url,
        videoId: item.vod_id,
        title: item.title,
        cover: item.cover_url,
        sourceName: item.source_name,
      }),
    toMediaItemVM: (item): MediaItemVM => {
      const tracked = buildOnlineVideoTrackedRef({
        sourceId: item.source_id,
        detailUrl: item.detail_url,
        videoId: item.vod_id,
        title: item.title,
        cover: item.cover_url,
        sourceName: item.source_name,
      })
      const subtitle = [item.remarks, item.type_name, item.year, item.area]
        .filter(Boolean)
        .join(' · ')
      return {
        ref: tracked ?? {
          scope: 'online',
          domain: 'video',
          sourceId: item.source_id,
          ref: item.detail_url || item.vod_id,
        },
        title: item.title,
        subtitle: subtitle || undefined,
        cover:
          resolveOnlineVideoPosterUrl(item.proxy_cover_url, item.cover_url, item.source_id) ||
          undefined,
        badge: {
          source: item.source_name || undefined,
          sourceKind: 'online',
        },
        to: buildOnlineVideoDetailRoute({
          sourceId: item.source_id,
          detailUrl: item.detail_url,
          videoId: item.vod_id,
          title: item.title,
          cover: item.cover_url,
          sourceName: item.source_name,
          remarks: item.remarks,
          year: item.year,
          area: item.area,
        }),
      }
    },
    autoExploreOnEmpty: true,
    defaultExploreFirstSource: true,
  },
  {
    keyword: toRef(props, 'keyword'),
    initialSourceId: toRef(props, 'initialSourceId'),
    onKeywordChange: (val) => emit('update:keyword', val),
    onSourceIdChange: (id) => emit('update:sourceId', id),
  },
)

const itemLookupByKey = computed(() => {
  const map = new Map<string, OnlineVideoSearchItemDto>()
  for (const item of browse.displayItems.value) {
    const key = `${item.source_id}:${item.detail_url || item.vod_id}`
    map.set(key, item)
  }
  return map
})

function findRawItem(vm: MediaItemVM): OnlineVideoSearchItemDto | null {
  if (vm.ref.scope === 'local') return null
  return itemLookupByKey.value.get(`${vm.ref.sourceId}:${vm.ref.ref}`) ?? null
}

function handleSelectItem(vm: MediaItemVM): void {
  const raw = findRawItem(vm)
  if (!raw) return
  const activeKw = (browse.lastSearchKeyword.value || browse.keyword.value).trim()
  const activeSrc =
    browse.selectedSourceIds.value.length === 1
      ? (browse.selectedSourceIds.value[0] ?? null)
      : null
  writeStoredDetailContext({
    sourceId: raw.source_id,
    sourceName: raw.source_name,
    videoId: raw.vod_id,
    detailUrl: raw.detail_url,
    title: raw.title,
    coverUrl: raw.cover_url,
    proxyCoverUrl: raw.proxy_cover_url,
    remarks: raw.remarks,
    year: raw.year,
    area: raw.area,
    typeName: raw.type_name,
    returnKeyword: activeKw || undefined,
    returnSourceId: activeSrc,
  })
}

function submitKeyword(nextKw: string): void {
  browse.setKeyword(nextKw, { debounce: false })
  void browse.submitSearch()
}

function clearKeyword(): void {
  browse.clearSearch()
}

defineExpose({
  submitKeyword,
  clearKeyword,
  refresh: browse.retryInit,
})
</script>

<template>
  <OnlineMediaSearchShell
    domain="video"
    :keyword="browse.keyword.value"
    :show-search-input="showSearchInput"
    :search-placeholder="t('online.searchPlaceholder')"
    :loading-init="browse.loadingInit.value"
    :sources-load-error="browse.sourcesLoadError.value"
    :enabled-sources="browse.enabledSources.value"
    :selected-source-ids="browse.selectedSourceIds.value"
    :source-collapse-limit="6"
    :searching="browse.searching.value"
    :browsing="browse.browsing.value"
    :has-searched="browse.hasSearched.value"
    :last-search-keyword="browse.lastSearchKeyword.value"
    :search-errors="browse.searchErrors.value"
    :search-toasts="browse.searchToasts.value"
    :show-browse-view="browse.showBrowseView.value"
    :browse-categories="browse.browseCategories.value"
    :active-category-id="browse.activeCategoryId.value"
    :category-collapse-limit="10"
    :browse-error="browse.browseError.value"
    :items="browse.displayVMs.value"
    :item-states="browse.itemStates"
    item-aspect="poster"
    item-cover-style="plain"
    :show-favorite-on-cards="false"
    :has-more="browse.activeHasMore.value"
    :loading-more="browse.activeLoadingMore.value"
    :idle-hint="t('online.browseEmpty')"
    @update:keyword="browse.setKeyword($event, { debounce: true })"
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
  />
</template>
