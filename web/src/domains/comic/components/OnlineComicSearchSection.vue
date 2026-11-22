<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { useComicI18n as useI18n } from '@/domains/comic/i18n'
import {
  LuBookOpen,
  LuChevronLeft,
  LuChevronRight,
  LuCompass,
  LuLoader2,
  LuPlus,
  LuSearch,
  LuSettings2,
} from 'vue-icons-plus/lu'
import MediaCoverImage from '@/components/ui/MediaCoverImage.vue'
import { useAuthStore } from '@/stores/auth'
import { useSnackbarStore } from '@/stores/snackbar'
import {
  ONLINE_COMIC_CTX_KEY,
  onlineComicApi,
  resolveOnlineComicCoverUrl,
  type ComicExploreCategoryItem,
  type ComicSearchResultItem,
  type ComicSourceSummaryDto,
  type OnlineComicContextStorage,
  type OnlineComicSearchErrorDto,
} from '@/domains/comic/onlineApi'

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
const sources = ref<ComicSourceSummaryDto[]>([])
const sourcesLoadError = ref<string | null>(null)
const searchKeyword = ref(props.keyword)
const selectedSourceIds = ref<string[]>([])

const searching = ref(false)
const loadingMore = ref(false)
const searchPage = ref(1)
const hasMoreResults = ref(false)
const lastSearchKeyword = ref('')
const lastSearchSourceIds = ref<string[]>([])
const searchResults = ref<ComicSearchResultItem[]>([])
const searchErrors = ref<OnlineComicSearchErrorDto[]>([])
const searchToasts = ref<string[]>([])
const hasSearched = ref(false)

const exploring = ref(false)
const loadingMoreExplore = ref(false)
const explorePage = ref(1)
const hasMoreExplore = ref(false)
const activeExploreUrl = ref<string | null>(null)
const exploreCategories = ref<ComicExploreCategoryItem[]>([])
const exploreItems = ref<ComicSearchResultItem[]>([])
const exploreError = ref<string | null>(null)

let initSeq = 0
let searchSeq = 0
let exploreSeq = 0
let unmounted = false
let pendingEmittedSourceId: string | null | undefined = undefined

const enabledSources = computed(() => sources.value.filter((s) => s.enabled))

const activeExploreSourceId = computed<string | null>(() => {
  if (selectedSourceIds.value.length === 1) {
    return selectedSourceIds.value[0] ?? null
  }
  if (selectedSourceIds.value.length === 0 && enabledSources.value.length === 1) {
    return enabledSources.value[0]?.id ?? null
  }
  return null
})

const showExploreView = computed(
  () =>
    !hasSearched.value &&
    searchKeyword.value.trim() === '' &&
    enabledSources.value.length > 0 &&
    activeExploreSourceId.value !== null,
)

const displayItems = computed<ComicSearchResultItem[]>(() =>
  hasSearched.value ? searchResults.value : exploreItems.value,
)

const isPaginationBusy = computed(
  () => searching.value || loadingMore.value || exploring.value || loadingMoreExplore.value,
)

const activePage = computed(() => (hasSearched.value ? searchPage.value : explorePage.value))
const activeHasMore = computed(() =>
  hasSearched.value ? hasMoreResults.value : hasMoreExplore.value,
)
const activeLoadingMore = computed(() =>
  hasSearched.value ? loadingMore.value : loadingMoreExplore.value,
)

function emitSourceIdUpdate(nextId: string | null): void {
  const currentProp = props.initialSourceId ?? null
  pendingEmittedSourceId = nextId !== currentProp ? nextId : undefined
  emit('update:sourceId', nextId)
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
        activeExploreUrl.value = null
        void runExplore(normalizedNext, null, 1, 'replace')
      }
    } else if (!normalizedNext && selectedSourceIds.value.length > 0) {
      selectedSourceIds.value = []
      if (searchKeyword.value.trim() === '' && !hasSearched.value) {
        activeExploreUrl.value = null
        if (activeExploreSourceId.value) {
          void runExplore(activeExploreSourceId.value, null, 1, 'replace')
        } else {
          exploreSeq += 1
          exploreCategories.value = []
          exploreItems.value = []
          exploreError.value = null
          hasMoreExplore.value = false
        }
      }
    }
  },
)

watch(
  () => props.keyword,
  (next) => {
    const trimmed = next.trim()
    searchKeyword.value = next
    if (loadingInit.value) return

    if (trimmed) {
      if (trimmed !== lastSearchKeyword.value && enabledSources.value.length > 0) {
        void runSearch(1, 'replace')
      }
      return
    }

    if (hasSearched.value || lastSearchKeyword.value) {
      hasSearched.value = false
      lastSearchKeyword.value = ''
      searchResults.value = []
      searchErrors.value = []
      searchToasts.value = []
      hasMoreResults.value = false
      if (activeExploreSourceId.value) {
        void runExplore(activeExploreSourceId.value, activeExploreUrl.value, 1, 'replace')
      }
    }
  },
)

async function initData(): Promise<void> {
  const seq = ++initSeq
  loadingInit.value = true
  sourcesLoadError.value = null
  try {
    const srcList = await onlineComicApi.listSources()
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
  } else if (activeExploreSourceId.value) {
    void runExplore(activeExploreSourceId.value, null, 1, 'replace')
  }
}

onMounted(() => {
  unmounted = false
  void initData()
})

onBeforeUnmount(() => {
  unmounted = true
  initSeq += 1
  searchSeq += 1
  exploreSeq += 1
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

  hasSearched.value = false
  searchResults.value = []
  activeExploreUrl.value = null
  if (activeExploreSourceId.value) {
    void runExplore(activeExploreSourceId.value, null, 1, 'replace')
  } else {
    exploreSeq += 1
    exploreCategories.value = []
    exploreItems.value = []
    exploreError.value = null
    hasMoreExplore.value = false
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

  hasSearched.value = false
  searchResults.value = []
  activeExploreUrl.value = null
  if (activeExploreSourceId.value) {
    void runExplore(activeExploreSourceId.value, null, 1, 'replace')
  } else {
    exploreSeq += 1
    exploreCategories.value = []
    exploreItems.value = []
    exploreError.value = null
    hasMoreExplore.value = false
  }
}

async function runExplore(
  sourceId: string,
  exploreUrl: string | null,
  page: number,
  mode: 'replace' | 'append',
): Promise<void> {
  if (mode === 'append' && (exploring.value || loadingMoreExplore.value)) return
  const seq = ++exploreSeq
  if (mode === 'append') {
    loadingMoreExplore.value = true
  } else {
    exploring.value = true
    loadingMoreExplore.value = false
    exploreError.value = null
    activeExploreUrl.value = exploreUrl
  }

  try {
    const res = await onlineComicApi.explore(sourceId, exploreUrl, page)
    if (unmounted || seq !== exploreSeq) return
    explorePage.value = res.page || page
    if (res.categories.length > 0 || !exploreUrl) {
      exploreCategories.value = res.categories
    }
    if (mode === 'append') {
      const seen = new Set(exploreItems.value.map((i) => `${i.source_id}:${i.comic_url}`))
      const appended = res.items.filter((i) => !seen.has(`${i.source_id}:${i.comic_url}`))
      exploreItems.value = [...exploreItems.value, ...appended]
      hasMoreExplore.value = appended.length > 0
      if (appended.length === 0) {
        snackbar.show({ message: t('online.noMoreResults') })
      }
    } else {
      exploreItems.value = res.items
      hasMoreExplore.value = res.items.length > 0
    }
  } catch (err) {
    if (unmounted || seq !== exploreSeq) return
    exploreError.value = err instanceof Error ? err.message : String(err)
    if (mode === 'replace') {
      exploreItems.value = []
      hasMoreExplore.value = false
    }
  } finally {
    if (!unmounted && seq === exploreSeq) {
      exploring.value = false
      loadingMoreExplore.value = false
    }
  }
}

async function runSearch(page: number, mode: 'replace' | 'append'): Promise<void> {
  const kw = (
    mode === 'replace' && page === 1
      ? searchKeyword.value
      : lastSearchKeyword.value || searchKeyword.value
  ).trim()

  if (!kw || enabledSources.value.length === 0) {
    if (!kw) {
      hasSearched.value = false
      lastSearchKeyword.value = ''
      searchResults.value = []
      searchErrors.value = []
      searchToasts.value = []
      hasMoreResults.value = false
      if (activeExploreSourceId.value) {
        void runExplore(activeExploreSourceId.value, activeExploreUrl.value, 1, 'replace')
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
    const res = await onlineComicApi.search(kw, srcIds, page)
    if (unmounted || seq !== searchSeq) return
    searchPage.value = res.page || page

    if (mode === 'append') {
      const seen = new Set(searchResults.value.map((i) => `${i.source_id}:${i.comic_url}`))
      const appended = res.results.filter((i) => !seen.has(`${i.source_id}:${i.comic_url}`))
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

async function handleSearchSubmit(): Promise<void> {
  emit('update:keyword', searchKeyword.value)
  await runSearch(1, 'replace')
}

async function handleLoadMore(): Promise<void> {
  if (hasSearched.value) {
    if (!hasMoreResults.value) return
    await runSearch(searchPage.value + 1, 'append')
  } else if (activeExploreSourceId.value && hasMoreExplore.value) {
    await runExplore(
      activeExploreSourceId.value,
      activeExploreUrl.value,
      explorePage.value + 1,
      'append',
    )
  }
}

async function handleSwitchPage(nextPage: number): Promise<void> {
  if (nextPage < 1) return
  if (hasSearched.value) {
    await runSearch(nextPage, 'replace')
  } else if (activeExploreSourceId.value) {
    await runExplore(activeExploreSourceId.value, activeExploreUrl.value, nextPage, 'replace')
  }
}

function selectExploreCategory(url: string | null): void {
  if (!activeExploreSourceId.value) return
  void runExplore(activeExploreSourceId.value, url, 1, 'replace')
}

function formatKindTags(raw: string | null | undefined): string {
  if (!raw) return ''
  return raw
    .split(/[,，|/]/)
    .map((s) => s.trim())
    .filter(Boolean)
    .join(' · ')
}

function openOnlineComicDetail(item: ComicSearchResultItem): void {
  const ctxPayload: OnlineComicContextStorage = {
    sourceId: item.source_id,
    sourceName: item.source_name,
    comicUrl: item.comic_url,
    title: item.title,
    author: item.author,
    coverUrl: item.cover_url,
    intro: item.intro,
  }
  try {
    sessionStorage.setItem(ONLINE_COMIC_CTX_KEY, JSON.stringify(ctxPayload))
  } catch {
    // ignore storage errors
  }

  const query: Record<string, string> = {
    sourceId: item.source_id,
    comicUrl: item.comic_url,
    sourceName: item.source_name,
    title: item.title,
  }
  if (item.author) query.author = item.author
  if (item.cover_url) query.coverUrl = item.cover_url
  void router.push({ path: '/comic/online/detail', query })
}

function goManageDataSources(): void {
  void router.push({ path: '/settings', query: { tab: 'domain-datasources', domain: 'comic' } })
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
            type="text"
            :placeholder="t('online.searchPlaceholder')"
            class="md-input h-10 w-full !pl-10 text-xs"
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

    <!-- Single-Source Explore Categories (when not searching) -->
    <div
      v-if="showExploreView && (exploreCategories.length > 0 || exploring || exploreError)"
      class="md-card-filled space-y-2.5 p-4"
    >
      <div class="flex items-center gap-2 text-xs font-semibold text-[var(--md-sys-color-on-surface)]">
        <LuCompass class="h-4 w-4 text-[var(--md-sys-color-primary)]" />
        <span>{{ t('online.exploreTitle') }}</span>
        <LuLoader2 v-if="exploring" class="h-3.5 w-3.5 animate-spin text-[var(--md-sys-color-primary)]" />
      </div>

      <div v-if="exploreCategories.length > 0" class="flex flex-wrap items-center gap-1.5">
        <button
          type="button"
          class="md-chip md-state !h-7 !rounded-full !px-2.5 !text-[11px]"
          :class="activeExploreUrl === null ? 'md-chip-selected' : ''"
          @click="selectExploreCategory(null)"
        >
          {{ t('online.exploreAll') }}
        </button>
        <button
          v-for="cat in exploreCategories"
          :key="cat.url || cat.title"
          type="button"
          class="md-chip md-state !h-7 !rounded-full !px-2.5 !text-[11px]"
          :class="activeExploreUrl === cat.url ? 'md-chip-selected' : ''"
          @click="selectExploreCategory(cat.url)"
        >
          {{ cat.title }}
        </button>
      </div>

      <p
        v-if="exploreError"
        role="alert"
        class="rounded-xl bg-[var(--md-sys-color-error-container)]/70 px-3 py-2 text-xs text-[var(--md-sys-color-on-error-container)]"
      >
        {{ t('online.exploreError', { error: exploreError }) }}
      </p>
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
      v-if="searching || (exploring && displayItems.length === 0)"
      class="flex flex-col items-center justify-center rounded-[24px] bg-[var(--md-sys-color-surface-container-low)] py-14 text-xs text-[var(--md-sys-color-on-surface-variant)]"
    >
      <LuLoader2 class="mb-2.5 h-6 w-6 animate-spin text-[var(--md-sys-color-primary)]" />
      <span>{{ searching ? t('online.searchingBtn') : t('online.loadingExplore') }}</span>
    </div>

    <!-- Empty Search Results -->
    <div
      v-else-if="hasSearched && !searching && searchResults.length === 0 && searchErrors.length === 0"
      class="rounded-[24px] bg-[var(--md-sys-color-surface-container-low)] py-14 text-center text-xs text-[var(--md-sys-color-on-surface-variant)]"
    >
      {{ t('online.noResultsFor', { keyword: lastSearchKeyword || searchKeyword }) }}
    </div>

    <!-- Idle Hint when no Search & no Explore items -->
    <div
      v-else-if="
        !hasSearched &&
        !exploreError &&
        displayItems.length === 0 &&
        enabledSources.length > 0 &&
        !loadingInit
      "
      class="rounded-[24px] bg-[var(--md-sys-color-surface-container-low)] py-14 text-center text-xs text-[var(--md-sys-color-on-surface-variant)]"
    >
      {{ t('online.exploreEmpty') }}
    </div>

    <!-- Results Grid (Search or Explore) -->
    <template v-else-if="displayItems.length > 0">
      <div class="grid grid-cols-1 gap-3.5 md:grid-cols-2">
        <button
          v-for="(item, idx) in displayItems"
          :key="`${item.source_id}-${idx}-${item.comic_url}`"
          type="button"
          class="md-card-filled md-state group flex w-full cursor-pointer gap-3.5 p-3.5 text-left transition hover:border-[var(--md-sys-color-primary)]/45"
          @click="openOnlineComicDetail(item)"
        >
          <MediaCoverImage
            :src="resolveOnlineComicCoverUrl(item.proxy_cover_url, item.cover_url, item.source_id)"
            :alt="item.title"
            aspect-class="aspect-[2/3]"
            rounded="rounded-xl"
            class="w-20 shrink-0"
            :fallback-icon="LuBookOpen"
            fallback-icon-class="h-6 w-6 text-[var(--md-sys-color-on-surface-variant)]/50"
          />

          <div class="flex min-w-0 flex-1 flex-col justify-between">
            <div>
              <div class="flex items-start justify-between gap-2">
                <h3
                  class="truncate text-sm font-semibold text-[var(--md-sys-color-on-surface)] group-hover:text-[var(--md-sys-color-primary)]"
                >
                  {{ item.title }}
                </h3>
                <span
                  class="shrink-0 rounded-full bg-[var(--md-sys-color-primary-container)] px-2 py-0.5 text-[10px] font-medium text-[var(--md-sys-color-on-primary-container)]"
                >
                  {{ item.source_name }}
                </span>
              </div>

              <p class="mt-0.5 truncate text-xs text-[var(--md-sys-color-on-surface-variant)]">
                {{ item.author || t('online.unknownAuthor') }}
                <span v-if="item.word_count"> · {{ item.word_count }}</span>
                <span v-if="formatKindTags(item.kind)"> · {{ formatKindTags(item.kind) }}</span>
              </p>

              <p
                v-if="item.intro"
                class="mt-1 line-clamp-2 text-[11px] leading-relaxed text-[var(--md-sys-color-on-surface-variant)]"
              >
                {{ item.intro }}
              </p>

              <p
                v-if="item.last_chapter"
                class="mt-1 truncate text-[10px] text-[var(--md-sys-color-on-surface-variant)] opacity-80"
              >
                {{ t('online.latestChapter', { chapter: item.last_chapter }) }}
              </p>
            </div>

            <div class="mt-2 flex items-center justify-end">
              <span class="text-[11px] font-semibold text-[var(--md-sys-color-primary)]">
                {{ t('online.viewDetail') }} →
              </span>
            </div>
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
