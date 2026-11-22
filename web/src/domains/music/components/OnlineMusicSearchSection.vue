<script setup lang="ts">
import { computed, toRef } from 'vue'
import { LuCompass, LuMusic, LuPlay, LuShuffle } from 'vue-icons-plus/lu'
import OnlineMediaSearchShell from '@/components/online-sources/OnlineMediaSearchShell.vue'
import {
  useOnlineSearchBrowse,
  type OnlineBrowseCategory,
  type UseOnlineSearchBrowseReturn,
} from '@/components/online-sources/useOnlineSearchBrowse'
import { MdButton, MdEmptyState } from '@/components/ui'
import { useMusicI18n as useI18n } from '../i18n'
import {
  onlineMusicApi,
  type MusicSourceSummaryDto,
  type OnlineMusicTrackDto,
} from '../onlineApi'
import { onlineTrackToTrackedRef } from '../onlineTrackRef'
import { buildOnlineTrackKey, usePlayerStore } from '../player'
import OnlineTrackTable from './OnlineTrackTable.vue'

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
const playerStore = usePlayerStore()

const pageLimit = 20
let cachedEnabledCount = 0

const browse: UseOnlineSearchBrowseReturn<OnlineMusicTrackDto, MusicSourceSummaryDto> =
  useOnlineSearchBrowse<OnlineMusicTrackDto, MusicSourceSummaryDto>(
    {
      domain: 'music',
      autoExploreOnEmpty: false,
      defaultExploreFirstSource: false,
      listSources: async () => {
        const list = await onlineMusicApi.listSources()
        cachedEnabledCount = list.filter((s) => s.enabled).length
        return list
      },
      search: async ({ keyword, sourceIds, page, signal }) => {
        const res = await onlineMusicApi.search(
          keyword,
          sourceIds.length > 0 ? sourceIds : undefined,
          page,
          pageLimit,
          { signal },
        )
        const incoming = res.results ?? []
        const activeSourceCount = sourceIds.length > 0 ? sourceIds.length : cachedEnabledCount
        const hasMore =
          incoming.length >= pageLimit ||
          (activeSourceCount > 1 && incoming.length >= Math.min(10, pageLimit))
        return {
          results: incoming,
          page: res.page || page,
          hasMore,
          errors: (res.errors ?? []).map((e) => ({
            source_id: e.source_id,
            source_name: e.source_name,
            error: e.message,
          })),
          toasts: res.toasts ?? [],
        }
      },
      browse: async ({ sourceId, category, page, signal }) => {
        const res = await onlineMusicApi.explore(sourceId, category?.url ?? null, page, pageLimit, {
          signal,
        })
        if (res.categories && res.categories.length > 0) {
          const mapped: OnlineBrowseCategory[] = res.categories.map((cat) => ({
            id: cat.url,
            name: cat.title,
            url: cat.url,
          }))
          browse.browseCategories.value = mapped
        }
        const incoming = res.results ?? []
        return {
          results: incoming,
          page,
          hasMore: incoming.length >= pageLimit,
          sourceId: res.source_id,
        }
      },
      getItemKey: (tr) => buildOnlineTrackKey(tr),
      toTrackedRef: (tr) => onlineTrackToTrackedRef(tr),
    },
    {
      keyword: toRef(props, 'keyword'),
      initialSourceId: toRef(props, 'initialSourceId'),
      onKeywordChange: (next) => emit('update:keyword', next),
      onSourceIdChange: (next) => emit('update:sourceId', next),
    },
  )

const displayTracks = computed(() => browse.displayItems.value)
const exploreTargetSourceId = computed(
  () =>
    (browse.selectedSourceIds.value.length === 1
      ? browse.selectedSourceIds.value[0]
      : browse.enabledSources.value[0]?.id) ?? null,
)

function handleTriggerExplore(): void {
  const target = exploreTargetSourceId.value
  if (!target) return
  void browse.triggerExplore(target)
}

function submitSearch(kw?: string): Promise<void> {
  return browse.submitSearch(kw)
}

function clearSearch(): void {
  browse.clearSearch()
}

defineExpose({
  submitSearch,
  clearSearch,
  retryInit: browse.retryInit,
})
</script>

<template>
  <OnlineMediaSearchShell
    domain="music"
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
    :item-count="displayTracks.length"
    :has-more="browse.activeHasMore.value"
    :loading-more="browse.activeLoadingMore.value"
    :idle-hint="t('online.promptToSearch')"
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
  >
    <template #idle>
      <MdEmptyState compact :icon="LuMusic" :title="t('online.promptToSearch')">
        <template v-if="exploreTargetSourceId" #action>
          <MdButton
            variant="tonal"
            size="sm"
            :icon="LuCompass"
            :loading="browse.browsing.value"
            @click="handleTriggerExplore"
          >
            {{ browse.browsing.value ? t('online.exploringBtn') : t('online.exploreBtn') }}
          </MdButton>
        </template>
      </MdEmptyState>
    </template>

    <template #summary-actions>
      <MdButton
        v-if="exploreTargetSourceId"
        variant="text"
        size="sm"
        :icon="LuCompass"
        :loading="browse.browsing.value"
        @click="handleTriggerExplore"
      >
        {{ t('online.exploreBtn') }}
      </MdButton>
      <MdButton
        variant="tonal"
        size="sm"
        :icon="LuPlay"
        @click="playerStore.playOnlineTracks(displayTracks, 0)"
      >
        {{ t('common.playAll') }}
      </MdButton>
      <MdButton
        variant="text"
        size="sm"
        :icon="LuShuffle"
        @click="playerStore.shuffleAndPlayOnline(displayTracks)"
      >
        {{ t('common.shufflePlay') }}
      </MdButton>
    </template>

    <template #results>
      <OnlineTrackTable :tracks="displayTracks" />
    </template>
  </OnlineMediaSearchShell>
</template>
