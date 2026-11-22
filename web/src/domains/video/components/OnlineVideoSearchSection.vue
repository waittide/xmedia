<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import {
  LuChevronLeft,
  LuChevronRight,
  LuCompass,
  LuFilm,
  LuLoader2,
  LuPlus,
  LuSearch,
  LuSettings2,
} from 'vue-icons-plus/lu'
import MediaCoverImage from '@/components/ui/MediaCoverImage.vue'
import { useAuthStore } from '@/stores/auth'
import { useSnackbarStore } from '@/stores/snackbar'
import { useVideoI18n as useI18n } from '../i18n'
import {
  ONLINE_VIDEO_CTX_KEY,
  onlineVideoApi,
  resolveOnlineVideoPosterUrl,
  type OnlineVideoCategory,
  type OnlineVideoContextStorage,
  type OnlineVideoSearchErrorDto,
  type OnlineVideoSearchItemDto,
  type VideoSourceSummaryDto,
} from '../onlineApi'

const props = defineProps<{
  keyword: string
  initialSourceId?: string | null
}>()

const emit = defineEmits<{
  'update:keyword': [value: string]
  'update:sourceId': [value: string | null]
}>()

const { t } = useI18n()
const router = useRouter()
const authStore = useAuthStore()
const snackbar = useSnackbarStore()

const loadingInit = ref(false)
const sources = ref<VideoSourceSummaryDto[]>([])
const sourcesLoadError = ref<string | null>(null)
const searchKeyword = ref(props.keyword)
const selectedSourceIds = ref<string[]>([])

const searching = ref(false)
const loadingMore = ref(false)
const searchPage = ref(1)
const hasMoreResults = ref(false)
const lastSearchKeyword = ref('')
const lastSearchSourceIds = ref<string[]>([])
const searchResults = ref<OnlineVideoSearchItemDto[]>([])
const searchErrors = ref<OnlineVideoSearchErrorDto[]>([])
const searchToasts = ref<string[]>([])
const hasSearched = ref(false)

const browsing = ref(false)
const loadingMoreBrowse = ref(false)
const browsePage = ref(1)
const hasMoreBrowse = ref(false)
const activeCategoryId = ref<string | null>(null)
const browseCategories = ref<OnlineVideoCategory[]>([])
const categoriesExpanded = ref(false)
const browseItems = ref<OnlineVideoSearchItemDto[]>([])
const browseError = ref<string | null>(null)

const CATEGORY_COLLAPSE_LIMIT = 12
const canCollapseCategories = computed(
  () => browseCategories.value.length > CATEGORY_COLLAPSE_LIMIT,
)
const visibleBrowseCategories = computed<OnlineVideoCategory[]>(() => {
  if (!canCollapseCategories.value || categoriesExpanded.value) {
    return browseCategories.value
  }
  const head = browseCategories.value.slice(0, CATEGORY_COLLAPSE_LIMIT)
  if (
    activeCategoryId.value &&
    !head.some((c) => c.id === activeCategoryId.value)
  ) {
    const activeCat = browseCategories.value.find((c) => c.id === activeCategoryId.value)
    if (activeCat) {
      return [...head, activeCat]
    }
  }
  return head
})

let initSeq = 0
let searchSeq = 0
let browseSeq = 0
let unmounted = false
let inputDebounceTimer: ReturnType<typeof setTimeout> | null = null
let pendingEmittedSourceId: string | null | undefined = undefined
let pendingEmittedKeyword: string | undefined = undefined

const enabledSources = computed(() => sources.value.filter((s) => s.enabled))

const activeBrowseSourceId = computed<string | null>(() => {
  if (selectedSourceIds.value.length === 1) {
    return selectedSourceIds.value[0] ?? null
  }
  if (selectedSourceIds.value.length === 0) {
    return enabledSources.value[0]?.id ?? null
  }
  return null
})

const showBrowseView = computed(
  () =>
    !hasSearched.value &&
    searchKeyword.value.trim() === '' &&
    enabledSources.value.length > 0 &&
    activeBrowseSourceId.value !== null,
)

const displayItems = computed<OnlineVideoSearchItemDto[]>(() =>
  hasSearched.value ? searchResults.value : browseItems.value,
)

const isPaginationBusy = computed(
  () => searching.value || loadingMore.value || browsing.value || loadingMoreBrowse.value,
)

const activePage = computed(() => (hasSearched.value ? searchPage.value : browsePage.value))
const activeHasMore = computed(() =>
  hasSearched.value ? hasMoreResults.value : hasMoreBrowse.value,
)
const activeLoadingMore = computed(() =>
  hasSearched.value ? loadingMore.value : loadingMoreBrowse.value,
)

function clearInputDebounce(): void {
  if (inputDebounceTimer !== null) {
    clearTimeout(inputDebounceTimer)
    inputDebounceTimer = null
  }
}

function emitKeywordUpdate(nextKw: string): void {
  pendingEmittedKeyword = nextKw !== props.keyword ? nextKw : undefined
  emit('update:keyword', nextKw)
}

function emitSourceIdUpdate(nextId: string | null): void {
  const currentProp = props.initialSourceId ?? null
  pendingEmittedSourceId = nextId !== currentProp ? nextId : undefined
  emit('update:sourceId', nextId)
}

function itemDedupeKey(item: OnlineVideoSearchItemDto): string {
  return `${item.source_id}:${item.detail_url || item.vod_id}`
}

function resetSearchState(): void {
  searchSeq += 1
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
  browsing.value = false
  loadingMoreBrowse.value = false
  browsePage.value = 1
  hasMoreBrowse.value = false
  activeCategoryId.value = null
  browseCategories.value = []
  browseItems.value = []
  browseError.value = null
}

watch(
  () => props.initialSourceId,
  (nextId) => {
    const normalizedNext = nextId ?? null
    if (pendingEmittedSourceId !== undefined) {
      const expected = pendingEmittedSourceId
      pendingEmittedSourceId = undefined
      if (normalizedNext === expected) {
        return
      }
    }
    if (normalizedNext && enabledSources.value.some((s) => s.id === normalizedNext)) {
      selectedSourceIds.value = [normalizedNext]
      if (searchKeyword.value.trim() === '' && !hasSearched.value) {
        activeCategoryId.value = null
        void runBrowse(normalizedNext, null, 1, 'replace', true)
      }
    } else if (!normalizedNext && selectedSourceIds.value.length > 0) {
      selectedSourceIds.value = []
      if (searchKeyword.value.trim() === '' && !hasSearched.value) {
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

watch(
  () => props.keyword,
  (next) => {
    if (pendingEmittedKeyword !== undefined) {
      const expected = pendingEmittedKeyword
      pendingEmittedKeyword = undefined
      if (next === expected) {
        return
      }
    }
    clearInputDebounce()
    const trimmed = next.trim()
    searchKeyword.value = next
    if (loadingInit.value) return

    if (trimmed) {
      if ((trimmed !== lastSearchKeyword.value || !hasSearched.value) && enabledSources.value.length > 0) {
        void runSearch(1, 'replace')
      }
      return
    }

    if (hasSearched.value || lastSearchKeyword.value) {
      resetSearchState()
      if (activeBrowseSourceId.value) {
        void runBrowse(activeBrowseSourceId.value, activeCategoryId.value, 1, 'replace', true)
      }
    }
  },
)

async function initData(): Promise<void> {
  const seq = ++initSeq
  loadingInit.value = true
  sourcesLoadError.value = null
  try {
    const srcList = await onlineVideoApi.listSources()
    if (unmounted || seq !== initSeq) return
    sources.value = srcList
    if (
      props.initialSourceId &&
      enabledSources.value.some((s) => s.id === props.initialSourceId)
    ) {
      selectedSourceIds.value = [props.initialSourceId]
    }
  } catch (err) {
    if (unmounted || seq !== initSeq) return
    sources.value = []
    sourcesLoadError.value = err instanceof Error ? err.message : String(err)
  } finally {
    if (!unmounted && seq === initSeq) {
      loadingInit.value = false
    }
  }

  if (unmounted || seq !== initSeq || enabledSources.value.length === 0) return

  if (searchKeyword.value.trim()) {
    void runSearch(1, 'replace')
  } else if (activeBrowseSourceId.value) {
    void runBrowse(activeBrowseSourceId.value, null, 1, 'replace', true)
  }
}

onMounted(() => {
  unmounted = false
  void initData()
})

onBeforeUnmount(() => {
  unmounted = true
  clearInputDebounce()
  initSeq += 1
  searchSeq += 1
  browseSeq += 1
})

function toggleSourceFilter(id: string): void {
  const next = selectedSourceIds.value.includes(id)
    ? selectedSourceIds.value.filter((s) => s !== id)
    : [...selectedSourceIds.value, id]
  selectedSourceIds.value = next
  emitSourceIdUpdate(next.length === 1 ? (next[0] ?? null) : null)

  if (searchKeyword.value.trim()) {
    if (hasSearched.value) {
      void runSearch(1, 'replace')
    }
    return
  }

  resetSearchState()
  activeCategoryId.value = null
  if (activeBrowseSourceId.value) {
    void runBrowse(activeBrowseSourceId.value, null, 1, 'replace', true)
  } else {
    resetBrowseState()
  }
}

function clearSourceFilter(): void {
  selectedSourceIds.value = []
  emitSourceIdUpdate(null)

  if (searchKeyword.value.trim()) {
    if (hasSearched.value) {
      void runSearch(1, 'replace')
    }
    return
  }

  resetSearchState()
  activeCategoryId.value = null
  if (activeBrowseSourceId.value) {
    void runBrowse(activeBrowseSourceId.value, null, 1, 'replace', true)
  } else {
    resetBrowseState()
  }
}

async function runBrowse(
  sourceId: string,
  categoryId: string | null,
  page: number,
  mode: 'replace' | 'append',
  fetchCategories = false,
): Promise<void> {
  if (mode === 'append' && (browsing.value || loadingMoreBrowse.value)) return
  const seq = ++browseSeq
  if (mode === 'append') {
    loadingMoreBrowse.value = true
  } else {
    browsing.value = true
    loadingMoreBrowse.value = false
    browseError.value = null
    activeCategoryId.value = categoryId
  }

  try {
    if (fetchCategories) {
      browseCategories.value = []
      categoriesExpanded.value = false
      const catRes = await onlineVideoApi.categories(sourceId)
      if (unmounted || seq !== browseSeq) return
      browseCategories.value = catRes.categories ?? []
      if (browseCategories.value.length === 0 && catRes.has_explore === false && categoryId === null) {
        browseItems.value = []
        hasMoreBrowse.value = false
        browsePage.value = 1
        return
      }
    }

    const res = await onlineVideoApi.browse(sourceId, categoryId, page)
    if (unmounted || seq !== browseSeq) return

    const sourceFallbackName =
      enabledSources.value.find((s) => s.id === sourceId)?.name || ''
    const resolvedSourceId = res.source_id || sourceId
    const resolvedSourceName = res.source_name || sourceFallbackName
    const normalized: OnlineVideoSearchItemDto[] = (res.results ?? []).map((item) => ({
      ...item,
      source_id: resolvedSourceId,
      source_name: resolvedSourceName,
    }))

    const currentPage = res.page || page
    browsePage.value = currentPage
    const pageCountMore =
      res.page_count != null ? currentPage < res.page_count : normalized.length > 0

    if (mode === 'append') {
      const seen = new Set(browseItems.value.map(itemDedupeKey))
      const appended = normalized.filter((item) => !seen.has(itemDedupeKey(item)))
      browseItems.value = [...browseItems.value, ...appended]
      hasMoreBrowse.value = appended.length > 0 && pageCountMore
      if (appended.length === 0) {
        snackbar.show({ message: t('online.noMoreResults') })
      }
    } else {
      browseItems.value = normalized
      hasMoreBrowse.value = pageCountMore
    }
  } catch (err) {
    if (unmounted || seq !== browseSeq) return
    const msg = err instanceof Error ? err.message : String(err)
    if (
      mode === 'replace' &&
      browseCategories.value.length === 0 &&
      categoryId === null &&
      msg.includes('未配置 exploreUrl')
    ) {
      browseItems.value = []
      hasMoreBrowse.value = false
      browseError.value = null
    } else {
      browseError.value = msg
      if (mode === 'replace') {
        browseItems.value = []
        hasMoreBrowse.value = false
      }
    }
  } finally {
    if (!unmounted && seq === browseSeq) {
      browsing.value = false
      loadingMoreBrowse.value = false
    }
  }
}

function retryBrowse(): void {
  if (!activeBrowseSourceId.value) return
  void runBrowse(activeBrowseSourceId.value, activeCategoryId.value, 1, 'replace', true)
}

async function runSearch(page: number, mode: 'replace' | 'append'): Promise<void> {
  const kw = (
    mode === 'replace' && page === 1
      ? searchKeyword.value
      : lastSearchKeyword.value || searchKeyword.value
  ).trim()

  if (!kw || enabledSources.value.length === 0) {
    if (!kw) {
      resetSearchState()
      if (activeBrowseSourceId.value) {
        void runBrowse(activeBrowseSourceId.value, activeCategoryId.value, 1, 'replace', true)
      }
    }
    return
  }
  if (mode === 'append' && (searching.value || loadingMore.value)) return

  const srcIds =
    mode === 'replace' && page === 1 ? [...selectedSourceIds.value] : [...lastSearchSourceIds.value]

  const seq = ++searchSeq
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
    const res = await onlineVideoApi.search(kw, srcIds, page)
    if (unmounted || seq !== searchSeq) return
    searchPage.value = res.page || page

    if (mode === 'append') {
      const seen = new Set(searchResults.value.map(itemDedupeKey))
      const appended = res.results.filter((item) => !seen.has(itemDedupeKey(item)))
      searchResults.value = [...searchResults.value, ...appended]
      hasMoreResults.value = appended.length > 0
      if (appended.length === 0) {
        snackbar.show({ message: t('online.noMoreResults') })
      }
    } else {
      searchResults.value = res.results
      hasMoreResults.value = res.results.length > 0
    }
    searchErrors.value = res.errors ?? []
    searchToasts.value = res.toasts ?? []
  } catch (err) {
    if (unmounted || seq !== searchSeq) return
    const msg = err instanceof Error ? err.message : String(err)
    if (mode === 'replace') {
      searchResults.value = []
      hasMoreResults.value = false
      searchErrors.value = [
        {
          source_id: 'search',
          source_name: t('online.scopeOnline'),
          error: msg,
        },
      ]
    }
    snackbar.show({
      message: msg,
      tone: 'error',
    })
  } finally {
    if (!unmounted && seq === searchSeq) {
      searching.value = false
      loadingMore.value = false
    }
  }
}

function handleKeywordInput(): void {
  clearInputDebounce()
  inputDebounceTimer = setTimeout(() => {
    inputDebounceTimer = null
    if (unmounted) return
    emitKeywordUpdate(searchKeyword.value)
    if (searchKeyword.value.trim()) {
      void runSearch(1, 'replace')
    } else {
      resetSearchState()
      if (activeBrowseSourceId.value) {
        void runBrowse(activeBrowseSourceId.value, activeCategoryId.value, 1, 'replace', true)
      }
    }
  }, 250)
}

async function handleSearchSubmit(): Promise<void> {
  clearInputDebounce()
  emitKeywordUpdate(searchKeyword.value)
  await runSearch(1, 'replace')
}

async function handleLoadMore(): Promise<void> {
  if (hasSearched.value) {
    if (!hasMoreResults.value) return
    await runSearch(searchPage.value + 1, 'append')
  } else if (activeBrowseSourceId.value && hasMoreBrowse.value) {
    await runBrowse(
      activeBrowseSourceId.value,
      activeCategoryId.value,
      browsePage.value + 1,
      'append',
    )
  }
}

async function handleSwitchPage(nextPage: number): Promise<void> {
  if (nextPage < 1) return
  if (hasSearched.value) {
    await runSearch(nextPage, 'replace')
  } else if (activeBrowseSourceId.value) {
    await runBrowse(activeBrowseSourceId.value, activeCategoryId.value, nextPage, 'replace')
  }
}

function selectBrowseCategory(catId: string | null): void {
  if (!activeBrowseSourceId.value) return
  void runBrowse(activeBrowseSourceId.value, catId, 1, 'replace')
}

function formatMetaLine(item: OnlineVideoSearchItemDto): string {
  return [item.type_name, item.year, item.area].filter(Boolean).join(' · ')
}

function openOnlineVideoDetail(item: OnlineVideoSearchItemDto): void {
  const activeKw = (lastSearchKeyword.value || searchKeyword.value).trim()
  const activeSrc =
    selectedSourceIds.value.length === 1 ? (selectedSourceIds.value[0] ?? null) : null
  const ctxPayload: OnlineVideoContextStorage = {
    sourceId: item.source_id,
    sourceName: item.source_name,
    videoId: item.vod_id,
    detailUrl: item.detail_url,
    title: item.title,
    coverUrl: item.cover_url,
    proxyCoverUrl: item.proxy_cover_url,
    remarks: item.remarks,
    year: item.year,
    area: item.area,
    typeName: item.type_name,
    returnKeyword: activeKw || undefined,
    returnSourceId: activeSrc,
  }
  try {
    sessionStorage.setItem(ONLINE_VIDEO_CTX_KEY, JSON.stringify(ctxPayload))
  } catch {
    // ignore storage errors
  }

  const query: Record<string, string> = {
    sourceId: item.source_id,
    sourceName: item.source_name,
    title: item.title,
  }
  if (item.vod_id) query.videoId = item.vod_id
  if (item.detail_url) query.detailUrl = item.detail_url
  if (item.cover_url) query.coverUrl = item.cover_url
  if (item.remarks) query.remarks = item.remarks
  if (item.year) query.year = item.year
  if (item.area) query.area = item.area

  void router.push({ path: '/video/online/detail', query })
}

function goManageDataSources(): void {
  void router.push({ path: '/settings', query: { tab: 'domain-datasources', domain: 'video' } })
}
</script>

<template>
  <div class="space-y-4">
    <div class="md-card-filled space-y-3 p-4">
      <form class="flex flex-wrap items-center gap-2" @submit.prevent="handleSearchSubmit">
        <div class="relative min-w-0 flex-1">
          <LuSearch
            class="pointer-events-none absolute top-1/2 left-3.5 h-4 w-4 -translate-y-1/2 text-[var(--md-sys-color-on-surface-variant)]"
          />
          <input
            v-model="searchKeyword"
            type="search"
            :placeholder="t('online.searchPlaceholder')"
            class="md-input h-10 w-full !rounded-full !pl-10 text-xs"
            @input="handleKeywordInput"
          />
        </div>
        <button
          type="submit"
          :disabled="!searchKeyword.trim() || enabledSources.length === 0"
          class="md-btn md-btn-filled md-state h-10 px-5"
        >
          <LuLoader2 v-if="searching" class="h-4 w-4 animate-spin" />
          <LuSearch v-else class="h-4 w-4" />
          <span>{{ searching ? t('online.searchingBtn') : t('online.searchBtn') }}</span>
        </button>
      </form>

      <!-- Source Filter Chips -->
      <div v-if="enabledSources.length > 0" class="flex flex-wrap items-center gap-1.5">
        <span class="text-[11px] text-[var(--md-sys-color-on-surface-variant)]">
          {{ t('online.sourceFilterLabel') }}
        </span>
        <button
          type="button"
          class="md-chip md-state !h-7 !rounded-full !px-2.5 !text-[11px]"
          :class="selectedSourceIds.length === 0 ? 'md-chip-selected' : ''"
          @click="clearSourceFilter"
        >
          {{ t('online.allSourcesCount', { count: enabledSources.length }) }}
        </button>
        <button
          v-for="src in enabledSources"
          :key="src.id"
          type="button"
          class="md-chip md-state !h-7 !rounded-full !px-2.5 !text-[11px]"
          :class="selectedSourceIds.includes(src.id) ? 'md-chip-selected' : ''"
          @click="toggleSourceFilter(src.id)"
        >
          {{ src.name }}
        </button>
      </div>

      <!-- Sources Load Error Banner -->
      <div
        v-else-if="!loadingInit && sourcesLoadError"
        role="alert"
        class="flex flex-wrap items-center justify-between gap-2 rounded-xl bg-[var(--md-sys-color-error-container)]/70 px-3.5 py-2.5 text-xs text-[var(--md-sys-color-on-error-container)]"
      >
        <span>{{ t('online.sourcesLoadFailed') }}: {{ sourcesLoadError }}</span>
        <button
          type="button"
          class="md-btn md-btn-sm md-btn-tonal md-state"
          @click="initData"
        >
          <span>{{ t('online.retry') }}</span>
        </button>
      </div>

      <!-- Empty / No Enabled Sources Banner -->
      <div
        v-else-if="!loadingInit"
        class="flex flex-wrap items-center justify-between gap-2 text-xs text-[var(--md-sys-color-on-surface-variant)]"
      >
        <span>{{ t('online.noEnabledSources') }}</span>
        <button
          v-if="authStore.isAdmin"
          type="button"
          class="md-btn md-btn-sm md-btn-tonal md-state"
          @click="goManageDataSources"
        >
          <LuSettings2 class="h-3.5 w-3.5" />
          <span>{{ t('online.goSettings') }}</span>
        </button>
      </div>
    </div>

    <!-- Categories Bar (when not searching) -->
    <div
      v-if="showBrowseView && (browseCategories.length > 0 || browsing || browseError)"
      class="md-card-filled space-y-2.5 p-4"
    >
      <div class="flex items-center gap-2 text-xs font-semibold text-[var(--md-sys-color-on-surface)]">
        <LuCompass class="h-4 w-4 text-[var(--md-sys-color-primary)]" />
        <span>{{ t('online.categoriesTitle') }}</span>
        <LuLoader2 v-if="browsing" class="h-3.5 w-3.5 animate-spin text-[var(--md-sys-color-primary)]" />
      </div>

      <div
        v-if="browseCategories.length > 0"
        role="tablist"
        :aria-label="t('online.categoriesTitle')"
        class="flex flex-wrap items-center gap-1.5"
      >
        <button
          type="button"
          role="tab"
          :aria-selected="activeCategoryId === null"
          class="md-chip md-state !h-7 !rounded-full !px-2.5 !text-[11px]"
          :class="activeCategoryId === null ? 'md-chip-selected' : ''"
          @click="selectBrowseCategory(null)"
        >
          {{ t('online.allCategories') }}
        </button>
        <button
          v-for="cat in visibleBrowseCategories"
          :key="cat.id"
          type="button"
          role="tab"
          :aria-selected="activeCategoryId === cat.id"
          class="md-chip md-state !h-7 !rounded-full !px-2.5 !text-[11px]"
          :class="activeCategoryId === cat.id ? 'md-chip-selected' : ''"
          @click="selectBrowseCategory(cat.id)"
        >
          {{ cat.name }}
        </button>
        <button
          v-if="canCollapseCategories"
          type="button"
          class="md-btn md-btn-sm md-btn-text md-state !h-7 !rounded-full !px-2.5 !text-[11px]"
          @click="categoriesExpanded = !categoriesExpanded"
        >
          {{
            categoriesExpanded
              ? t('online.collapseCategories')
              : t('online.expandCategories', { count: browseCategories.length })
          }}
        </button>
      </div>

      <div
        v-if="browseError"
        role="alert"
        class="flex flex-wrap items-center justify-between gap-2 rounded-xl bg-[var(--md-sys-color-error-container)]/70 px-3 py-2 text-xs text-[var(--md-sys-color-on-error-container)]"
      >
        <span>{{ t('online.browseError', { error: browseError }) }}</span>
        <button
          type="button"
          class="md-btn md-btn-sm md-btn-tonal md-state shrink-0"
          @click="retryBrowse"
        >
          <span>{{ t('online.retry') }}</span>
        </button>
      </div>
    </div>

    <!-- Toasts / Per-Source Search Errors -->
    <div v-if="searchErrors.length > 0 || searchToasts.length > 0" class="space-y-1.5 text-xs">
      <div
        v-for="(toast, i) in searchToasts"
        :key="`st-${i}`"
        class="rounded-xl bg-[var(--md-sys-color-surface-container)] px-3.5 py-2 text-[var(--md-sys-color-on-surface-variant)]"
      >
        {{ toast }}
      </div>
      <div
        v-for="e in searchErrors"
        :key="e.source_id"
        role="alert"
        class="rounded-xl bg-[var(--md-sys-color-error-container)]/70 px-3.5 py-2 text-[var(--md-sys-color-on-error-container)]"
      >
        {{ t('online.sourceErrorPrefix', { name: e.source_name, error: e.error }) }}
      </div>
    </div>

    <!-- Loading Indicator -->
    <div
      v-if="searching || (browsing && displayItems.length === 0)"
      class="flex flex-col items-center justify-center rounded-[24px] bg-[var(--md-sys-color-surface-container-low)] py-14 text-xs text-[var(--md-sys-color-on-surface-variant)]"
    >
      <LuLoader2 class="mb-2.5 h-6 w-6 animate-spin text-[var(--md-sys-color-primary)]" />
      <span>{{ searching ? t('online.searchingBtn') : t('online.loadingBrowse') }}</span>
    </div>

    <!-- Empty Search Results -->
    <div
      v-else-if="hasSearched && !searching && searchResults.length === 0 && searchErrors.length === 0"
      class="rounded-[24px] bg-[var(--md-sys-color-surface-container-low)] py-14 text-center text-xs text-[var(--md-sys-color-on-surface-variant)]"
    >
      {{ t('online.noResultsFor', { keyword: lastSearchKeyword || searchKeyword }) }}
    </div>

    <!-- Idle Hint when no Search & no Browse items -->
    <div
      v-else-if="
        !hasSearched &&
        !browseError &&
        displayItems.length === 0 &&
        enabledSources.length > 0 &&
        !loadingInit
      "
      class="rounded-[24px] bg-[var(--md-sys-color-surface-container-low)] py-14 text-center text-xs text-[var(--md-sys-color-on-surface-variant)]"
    >
      {{ t('online.browseEmpty') }}
    </div>

    <!-- Results Grid (Search or Browse) -->
    <template v-else-if="displayItems.length > 0">
      <div class="grid grid-cols-1 gap-3.5 md:grid-cols-2 lg:grid-cols-3">
        <button
          v-for="(item, idx) in displayItems"
          :key="`${item.source_id}-${idx}-${item.detail_url || item.vod_id}`"
          type="button"
          class="md-card-filled md-state group flex w-full cursor-pointer gap-3.5 p-3.5 text-left transition hover:-translate-y-0.5 hover:border-[var(--md-sys-color-outline)]/40 hover:shadow-[var(--md-sys-elevation-1)]"
          @click="openOnlineVideoDetail(item)"
        >
          <MediaCoverImage
            :src="resolveOnlineVideoPosterUrl(item.proxy_cover_url, item.cover_url, item.source_id)"
            :alt="item.title"
            aspect-class="aspect-[2/3]"
            rounded="rounded-xl"
            class="w-20 shrink-0"
            :fallback-icon="LuFilm"
            fallback-icon-class="h-6 w-6 text-[var(--md-sys-color-on-surface-variant)]/50"
          />

          <div class="flex min-w-0 flex-1 flex-col justify-between py-0.5">
            <div class="space-y-1.5">
              <div class="flex items-start justify-between gap-2">
                <h3
                  class="line-clamp-2 text-sm font-semibold text-[var(--md-sys-color-on-surface)] group-hover:text-[var(--md-sys-color-primary)]"
                >
                  {{ item.title }}
                </h3>
                <span
                  class="shrink-0 rounded-full bg-[var(--md-sys-color-surface-container-high)] px-2 py-0.5 text-[10px] font-medium text-[var(--md-sys-color-on-surface-variant)]"
                >
                  {{ item.source_name }}
                </span>
              </div>

              <p
                v-if="item.remarks"
                class="inline-block max-w-full truncate rounded-md bg-[var(--md-sys-color-secondary-container)]/80 px-1.5 py-0.5 text-[10px] font-medium text-[var(--md-sys-color-on-secondary-container)]"
              >
                {{ item.remarks }}
              </p>
            </div>

            <p
              v-if="formatMetaLine(item)"
              class="mt-2 truncate text-xs text-[var(--md-sys-color-on-surface-variant)]"
            >
              {{ formatMetaLine(item) }}
            </p>
          </div>
        </button>
      </div>

      <!-- Pagination & Load More -->
      <div
        class="flex flex-wrap items-center justify-between gap-3 rounded-2xl bg-[var(--md-sys-color-surface-container-low)] px-4 py-3 text-xs"
      >
        <div class="flex items-center gap-2">
          <button
            type="button"
            :disabled="activePage <= 1 || isPaginationBusy"
            class="md-btn md-btn-sm md-btn-outlined md-state"
            @click="handleSwitchPage(activePage - 1)"
          >
            <LuChevronLeft class="h-3.5 w-3.5" />
            <span>{{ t('online.prevPage') }}</span>
          </button>
          <span class="px-2 font-mono text-[var(--md-sys-color-on-surface-variant)]">
            {{ t('online.pageInfo', { page: activePage, count: displayItems.length }) }}
          </span>
          <button
            type="button"
            :disabled="!activeHasMore || isPaginationBusy"
            class="md-btn md-btn-sm md-btn-outlined md-state"
            @click="handleSwitchPage(activePage + 1)"
          >
            <span>{{ t('online.nextPage') }}</span>
            <LuChevronRight class="h-3.5 w-3.5" />
          </button>
        </div>

        <button
          type="button"
          :disabled="!activeHasMore || isPaginationBusy"
          class="md-btn md-btn-sm md-btn-tonal md-state"
          @click="handleLoadMore"
        >
          <LuLoader2 v-if="activeLoadingMore" class="h-3.5 w-3.5 animate-spin" />
          <LuPlus v-else class="h-3.5 w-3.5" />
          <span>
            {{
              activeLoadingMore
                ? t('online.loadingMore')
                : activeHasMore
                  ? t('online.loadMore')
                  : t('online.noMoreResults')
            }}
          </span>
        </button>
      </div>
    </template>
  </div>
</template>
