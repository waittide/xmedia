<script setup lang="ts">
import { computed, ref } from 'vue'
import { useRouter } from 'vue-router'
import { LuBookOpen, LuCheck, LuDownload, LuPlus } from 'vue-icons-plus/lu'
import OnlineMediaSearchShell from '@/components/online-sources/OnlineMediaSearchShell.vue'
import { useOnlineSearchBrowse } from '@/components/online-sources/useOnlineSearchBrowse'
import { MdMenuItem } from '@/components/ui'
import { useBookI18n as useI18n } from '@/domains/book/i18n'
import { itemRoute, playRoute, type MediaItemVM, type TrackedItemRef } from '@/sources'
import { useOnlineDownloadsStore } from '@/stores/onlineDownloads'
import { useSnackbarStore } from '@/stores/snackbar'
import {
  buildOnlineCoverUrl,
  onlineBookApi,
  type AggregatedSearchItemDto,
  type BookSourceSummaryDto,
  type OnlineDownloadJobDto,
} from '../onlineApi'
import { useCatalogStore } from '../store'
import OnlineDownloadModal, { type OnlineDownloadTarget } from './OnlineDownloadModal.vue'

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
  'update:sourceId': [sourceId: string | null]
}>()

const { t } = useI18n()
const router = useRouter()
const catalogStore = useCatalogStore()
const snackbar = useSnackbarStore()
const onlineDownloadsStore = useOnlineDownloadsStore()

// Lightweight session map for newly tracked books (avoids fetching up to 10,000 books on mount)
const trackedBookIdByKey = ref<Record<string, string>>({})
const busyActionKeys = ref<Set<string>>(new Set())
const downloadTarget = ref<OnlineDownloadTarget | null>(null)
const modalTrackedJobId = ref<string | null>(null)

function setBusyKey(key: string, active: boolean): void {
  const next = new Set(busyActionKeys.value)
  if (active) next.add(key)
  else next.delete(key)
  busyActionKeys.value = next
}

function itemNaturalKey(sourceId: string, bookUrl: string): string {
  return `${sourceId}:${bookUrl}`
}

function resolveTrackedBookId(sourceId: string, bookUrl: string): string | undefined {
  const key = itemNaturalKey(sourceId, bookUrl)
  if (trackedBookIdByKey.value[key]) return trackedBookIdByKey.value[key]
  const cachedInCatalog = catalogStore.books.find(
    (b) => b.online_tracking?.source_id === sourceId && b.online_tracking?.book_url === bookUrl,
  )
  return cachedInCatalog?.id
}

const searchState = useOnlineSearchBrowse<AggregatedSearchItemDto, BookSourceSummaryDto>(
  {
    domain: 'book',
    listSources: () => onlineBookApi.listSources(),
    search: async ({ keyword, sourceIds, page, signal }) => {
      const res = await onlineBookApi.search(keyword, sourceIds, page, { signal })
      return {
        results: res.items,
        page: res.page || page,
        hasMore: res.items.length > 0,
        errors: res.errors,
        toasts: res.toasts,
      }
    },
    getItemKey: (item) => itemNaturalKey(item.source_id, item.book.book_url),
    toTrackedRef: (item): TrackedItemRef => ({
      scope: 'online',
      domain: 'book',
      sourceId: item.source_id,
      ref: item.book.book_url,
      hints: {
        title: item.book.name,
        cover: item.book.cover_url ? buildOnlineCoverUrl(item.book.cover_url) || undefined : undefined,
        author: item.book.author || undefined,
      },
    }),
    toMediaItemVM: (item): MediaItemVM => {
      const cover = item.book.cover_url ? buildOnlineCoverUrl(item.book.cover_url) || undefined : undefined
      const trackedId = resolveTrackedBookId(item.source_id, item.book.book_url)
      const ref: TrackedItemRef = {
        scope: 'online',
        domain: 'book',
        sourceId: item.source_id,
        ref: item.book.book_url,
        hints: {
          title: item.book.name,
          cover,
          author: item.book.author || undefined,
        },
      }
      const subtitleParts = [
        item.book.author || t('catalog.unknownAuthor'),
        item.book.word_count || '',
      ].filter(Boolean)

      return {
        ref,
        title: item.book.name,
        subtitle: subtitleParts.join(' · '),
        cover,
        badge: {
          source: item.source_name,
          sourceKind: 'online',
          status: trackedId ? t('online.trackedBadge') : item.book.last_chapter || undefined,
          availability: 'ok',
          blocked: false,
        },
        to: trackedId
          ? itemRoute({ scope: 'local', domain: 'book', id: trackedId })
          : playRoute(ref),
        playTo: trackedId
          ? playRoute({ scope: 'local', domain: 'book', id: trackedId })
          : playRoute(ref),
      }
    },
    autoExploreOnEmpty: false,
  },
  {
    keyword: () => props.keyword,
    initialSourceId: () => props.initialSourceId,
    onKeywordChange: (kw) => emit('update:keyword', kw),
    onSourceIdChange: (id) => emit('update:sourceId', id),
  },
)

const itemsByKey = computed(() => {
  const map = new Map<string, AggregatedSearchItemDto>()
  for (const entry of searchState.displayItems.value) {
    map.set(itemNaturalKey(entry.source_id, entry.book.book_url), entry)
  }
  return map
})

const modalTrackedJob = computed<OnlineDownloadJobDto | null>(() => {
  if (!modalTrackedJobId.value) return null
  return onlineDownloadsStore.jobs.find((j) => j.id === modalTrackedJobId.value) ?? null
})

function findRawItem(vm: MediaItemVM): AggregatedSearchItemDto | null {
  if (vm.ref.scope === 'local') return null
  return itemsByKey.value.get(itemNaturalKey(vm.ref.sourceId, vm.ref.ref)) ?? null
}

function handleOpenReader(vm: MediaItemVM): void {
  if (vm.ref.scope === 'local') return
  const trackedId = resolveTrackedBookId(vm.ref.sourceId, vm.ref.ref)
  if (trackedId) {
    void router.push(playRoute({ scope: 'local', domain: 'book', id: trackedId }))
    return
  }
  const raw = findRawItem(vm)
  if (raw) {
    try {
      sessionStorage.setItem(
        'xmedia_online_reader_ctx',
        JSON.stringify({
          sourceId: raw.source_id,
          sourceName: raw.source_name,
          name: raw.book.name,
          author: raw.book.author,
          bookUrl: raw.book.book_url,
          tocUrl: '',
          coverUrl: raw.book.cover_url,
          intro: raw.book.intro,
          currentChapterIndex: 0,
        }),
      )
    } catch {
      // ignore storage errors
    }
  }
  void router.push(playRoute(vm.ref))
}

async function handleTrackToLibrary(vm: MediaItemVM): Promise<void> {
  if (vm.ref.scope === 'local') return
  const existingId = resolveTrackedBookId(vm.ref.sourceId, vm.ref.ref)
  if (existingId) {
    void router.push(itemRoute({ scope: 'local', domain: 'book', id: existingId }))
    return
  }
  const raw = findRawItem(vm)
  if (!raw) return
  const key = itemNaturalKey(raw.source_id, raw.book.book_url)
  if (busyActionKeys.value.has(key)) return
  setBusyKey(key, true)
  try {
    const detailRes = await onlineBookApi
      .fetchBookInfo(raw.source_id, raw.book.book_url, {
        name: raw.book.name,
        author: raw.book.author,
        book_url: raw.book.book_url,
        toc_url: '',
        cover_url: raw.book.cover_url,
        intro: raw.book.intro,
      })
      .catch(() => null)
    const created = await onlineBookApi.trackOnlineBook({
      source_id: raw.source_id,
      name: detailRes?.detail.name || raw.book.name,
      author: detailRes?.detail.author || raw.book.author,
      book_url: raw.book.book_url,
      toc_url: detailRes?.detail.toc_url || '',
      cover_url: detailRes?.detail.cover_url ?? raw.book.cover_url,
      intro: detailRes?.detail.intro ?? raw.book.intro,
      kind: detailRes?.detail.kind ?? raw.book.kind,
      last_chapter: detailRes?.detail.last_chapter ?? raw.book.last_chapter,
    })
    trackedBookIdByKey.value = { ...trackedBookIdByKey.value, [key]: created.id }
    void catalogStore.refreshSidebarData()
    snackbar.show({
      message: t('online.trackedSuccess', { title: created.metadata.title }),
      actionLabel: t('online.viewBook'),
      onAction: () => {
        void router.push(itemRoute({ scope: 'local', domain: 'book', id: created.id }))
      },
    })
  } catch (err) {
    snackbar.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  } finally {
    setBusyKey(key, false)
  }
}

async function handleOpenDownload(vm: MediaItemVM): Promise<void> {
  if (vm.ref.scope === 'local') return
  const raw = findRawItem(vm)
  if (!raw) return
  const key = `dl:${itemNaturalKey(raw.source_id, raw.book.book_url)}`
  if (busyActionKeys.value.has(key)) return
  setBusyKey(key, true)
  try {
    const detailRes = await onlineBookApi.fetchBookInfo(raw.source_id, raw.book.book_url, {
      name: raw.book.name,
      author: raw.book.author,
      book_url: raw.book.book_url,
      toc_url: '',
      cover_url: raw.book.cover_url,
      intro: raw.book.intro,
    })
    downloadTarget.value = {
      bookId: resolveTrackedBookId(raw.source_id, raw.book.book_url),
      sourceId: raw.source_id,
      sourceName: raw.source_name,
      bookName: detailRes.detail.name || raw.book.name,
      author: detailRes.detail.author || raw.book.author,
      bookUrl: raw.book.book_url,
      tocUrl: detailRes.detail.toc_url,
      coverUrl: detailRes.detail.cover_url ?? raw.book.cover_url,
      intro: detailRes.detail.intro ?? raw.book.intro,
    }
  } catch (err) {
    snackbar.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  } finally {
    setBusyKey(key, false)
  }
}

function handleJobUpdated(job: OnlineDownloadJobDto): void {
  modalTrackedJobId.value = job.id
  onlineDownloadsStore.trackJob(job)
}

function closeDownloadModal(): void {
  downloadTarget.value = null
  modalTrackedJobId.value = null
}

function submitSearch(): Promise<void> {
  return searchState.submitSearch()
}

function clearSearch(): void {
  searchState.clearSearch()
}

defineExpose({
  submitSearch,
  clearSearch,
  refreshSources: () => searchState.init(),
})
</script>

<template>
  <div>
    <OnlineMediaSearchShell
      domain="book"
      :keyword="searchState.keyword.value"
      :show-search-input="showSearchInput"
      :search-placeholder="t('online.searchPlaceholder')"
      :loading-init="searchState.loadingInit.value"
      :sources-load-error="searchState.sourcesLoadError.value"
      :enabled-sources="searchState.enabledSources.value"
      :selected-source-ids="searchState.selectedSourceIds.value"
      :searching="searchState.searching.value"
      :has-searched="searchState.hasSearched.value"
      :last-search-keyword="searchState.lastSearchKeyword.value"
      :search-errors="searchState.searchErrors.value"
      :search-toasts="searchState.searchToasts.value"
      :items="searchState.displayVMs.value"
      item-aspect="poster"
      item-cover-style="book"
      :item-states="searchState.itemStates"
      :show-favorite-on-cards="false"
      :has-more="searchState.activeHasMore.value"
      :loading-more="searchState.activeLoadingMore.value"
      :idle-hint="t('online.idleHint')"
      @update:keyword="searchState.setKeyword($event)"
      @search-submit="searchState.submitSearch()"
      @search-clear="searchState.clearSearch()"
      @toggle-source="searchState.toggleSource($event)"
      @clear-sources="searchState.clearSourceFilter()"
      @retry-init="searchState.retryInit()"
      @retry-search="searchState.retrySearch()"
      @load-more="searchState.loadMore()"
    >
      <template #card-menu="{ item }">
        <MdMenuItem
          :icon="LuBookOpen"
          :label="t('online.readNow')"
          @select="handleOpenReader(item)"
        />
        <MdMenuItem
          :icon="
            item.ref.scope !== 'local' && resolveTrackedBookId(item.ref.sourceId, item.ref.ref)
              ? LuCheck
              : LuPlus
          "
          :label="
            item.ref.scope !== 'local' && resolveTrackedBookId(item.ref.sourceId, item.ref.ref)
              ? t('online.inLibrary')
              : t('online.addToLibrary')
          "
          @select="handleTrackToLibrary(item)"
        />
        <MdMenuItem
          :icon="LuDownload"
          :label="t('online.downloadPack')"
          @select="handleOpenDownload(item)"
        />
      </template>
    </OnlineMediaSearchShell>

    <OnlineDownloadModal
      :open="downloadTarget !== null"
      :target="downloadTarget"
      :external-job="modalTrackedJob"
      @close="closeDownloadModal"
      @job-updated="handleJobUpdated"
    />
  </div>
</template>
