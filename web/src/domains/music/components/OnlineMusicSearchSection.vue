<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import {
  LuChevronLeft,
  LuChevronRight,
  LuCompass,
  LuLoader2,
  LuPlay,
  LuPlus,
  LuSearch,
  LuSettings2,
  LuShuffle,
} from 'vue-icons-plus/lu'
import { useAuthStore } from '@/stores/auth'
import { useMusicI18n as useI18n } from '../i18n'
import {
  onlineMusicApi,
  type MusicExploreCategoryDto,
  type MusicSourceSummaryDto,
  type OnlineMusicSearchErrorDto,
  type OnlineMusicTrackDto,
} from '../onlineApi'
import { usePlayerStore } from '../player'
import OnlineTrackTable from './OnlineTrackTable.vue'

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
const playerStore = usePlayerStore()

const loadingInit = ref(true)
const sources = ref<MusicSourceSummaryDto[]>([])
const enabledSources = computed(() => sources.value.filter((s) => s.enabled))

const searchKeyword = ref(props.keyword)
const selectedSourceIds = ref<string[]>([])
const searching = ref(false)
const loadingMore = ref(false)
const searchPage = ref(1)
const hasMoreResults = ref(false)
const hasSearched = ref(false)
const lastSearchKeyword = ref('')
const lastSearchSourceIds = ref<string[]>([])
const searchResults = ref<OnlineMusicTrackDto[]>([])
const searchErrors = ref<OnlineMusicSearchErrorDto[]>([])
const searchToasts = ref<string[]>([])

// Strictly user-triggered single-source explore state (never auto-called on mount)
const exploreMode = ref(false)
const exploring = ref(false)
const exploreCategories = ref<MusicExploreCategoryDto[]>([])
const activeExploreUrl = ref<string | null>(null)
const exploreError = ref<string | null>(null)

let initSeq = 0
let searchSeq = 0
let exploreSeq = 0
let unmounted = false
let debounceTimer: ReturnType<typeof setTimeout> | null = null
let pendingSourceEmit: string | null | undefined = undefined

const pageLimit = 20

function sameIds(a: string[], b: string[]): boolean {
  if (a.length !== b.length) return false
  for (let i = 0; i < a.length; i++) {
    if (a[i] !== b[i]) return false
  }
  return true
}

function trackDedupeKey(tr: OnlineMusicTrackDto): string {
  return `${tr.source_id}:${tr.track_id}:${tr.detail_url || tr.play_url || tr.title}`
}

function clearDebounce(): void {
  if (debounceTimer) {
    clearTimeout(debounceTimer)
    debounceTimer = null
  }
}

function scheduleDebouncedSearch(): void {
  clearDebounce()
  const kw = searchKeyword.value.trim()
  if (!kw || loadingInit.value || enabledSources.value.length === 0) return
  debounceTimer = setTimeout(() => {
    debounceTimer = null
    if (unmounted) return
    const currentKw = searchKeyword.value.trim()
    if (!currentKw || enabledSources.value.length === 0) return
    if (
      hasSearched.value &&
      !exploreMode.value &&
      currentKw === lastSearchKeyword.value &&
      sameIds(selectedSourceIds.value, lastSearchSourceIds.value)
    ) {
      return
    }
    emit('update:keyword', currentKw)
    void runSearch(1, 'replace', true)
  }, 250)
}

function emitSourceId(nextSourceId: string | null): void {
  const currentProp = props.initialSourceId ?? null
  pendingSourceEmit = currentProp !== nextSourceId ? nextSourceId : undefined
  emit('update:sourceId', nextSourceId)
}

watch(
  () => props.initialSourceId,
  (nextId, prevId) => {
    const normalizedNext = nextId ?? null
    if (pendingSourceEmit !== undefined && normalizedNext === pendingSourceEmit) {
      pendingSourceEmit = undefined
      return
    }
    pendingSourceEmit = undefined

    if (nextId && enabledSources.value.some((s) => s.id === nextId)) {
      if (!sameIds(selectedSourceIds.value, [nextId])) {
        selectedSourceIds.value = [nextId]
        resetExploreState()
        if (searchKeyword.value.trim() && !loadingInit.value) {
          void runSearch(1, 'replace', true)
        }
      }
    } else if (
      !nextId &&
      prevId &&
      selectedSourceIds.value.length === 1 &&
      selectedSourceIds.value[0] === prevId
    ) {
      selectedSourceIds.value = []
      resetExploreState()
      if (searchKeyword.value.trim() && !loadingInit.value) {
        void runSearch(1, 'replace', true)
      }
    }
  },
)

watch(
  () => props.keyword,
  (next) => {
    searchKeyword.value = next
    const trimmed = next.trim()
    if (!trimmed) {
      clearDebounce()
      return
    }
    if (loadingInit.value || enabledSources.value.length === 0) return
    if (
      trimmed !== lastSearchKeyword.value ||
      !sameIds(selectedSourceIds.value, lastSearchSourceIds.value)
    ) {
      scheduleDebouncedSearch()
    }
  },
)

function resetExploreState(): void {
  exploreSeq += 1
  exploreMode.value = false
  exploring.value = false
  exploreCategories.value = []
  activeExploreUrl.value = null
  exploreError.value = null
}

function onLocalInput(): void {
  const trimmed = searchKeyword.value.trim()
  if (!trimmed) {
    clearDebounce()
    return
  }
  scheduleDebouncedSearch()
}

function handleSearchSubmit(): void {
  clearDebounce()
  const kw = searchKeyword.value.trim()
  if (!kw || enabledSources.value.length === 0) return
  emit('update:keyword', kw)
  void runSearch(1, 'replace', true)
}

function selectAllSources(): void {
  selectedSourceIds.value = []
  resetExploreState()
  emitSourceId(null)
  if (searchKeyword.value.trim() && enabledSources.value.length > 0) {
    void runSearch(1, 'replace', true)
  }
}

function toggleSourceFilter(sourceId: string): void {
  const exists = selectedSourceIds.value.includes(sourceId)
  selectedSourceIds.value = exists
    ? selectedSourceIds.value.filter((id) => id !== sourceId)
    : [...selectedSourceIds.value, sourceId]
  resetExploreState()
  emitSourceId(selectedSourceIds.value.length === 1 ? selectedSourceIds.value[0]! : null)
  if (searchKeyword.value.trim() && enabledSources.value.length > 0) {
    void runSearch(1, 'replace', true)
  }
}

async function runSearch(
  page = 1,
  mode: 'replace' | 'append' = 'replace',
  force = false,
): Promise<void> {
  const kw = searchKeyword.value.trim()
  if (!kw || enabledSources.value.length === 0) return

  const sourceScope = [...selectedSourceIds.value]
  if (
    mode === 'replace' &&
    !force &&
    hasSearched.value &&
    !exploreMode.value &&
    page === searchPage.value &&
    kw === lastSearchKeyword.value &&
    sameIds(sourceScope, lastSearchSourceIds.value)
  ) {
    return
  }

  exploreSeq += 1
  exploreMode.value = false
  exploring.value = false
  exploreError.value = null

  const seq = ++searchSeq
  if (mode === 'append') {
    loadingMore.value = true
  } else {
    searching.value = true
    searchErrors.value = []
    searchToasts.value = []
  }

  try {
    const res = await onlineMusicApi.search(
      kw,
      sourceScope.length > 0 ? sourceScope : undefined,
      page,
      pageLimit,
    )
    if (unmounted || seq !== searchSeq || exploreMode.value) return

    hasSearched.value = true
    searchPage.value = res.page || page
    lastSearchKeyword.value = kw
    lastSearchSourceIds.value = sourceScope
    searchErrors.value = res.errors ?? []
    searchToasts.value = res.toasts ?? []

    const incoming = res.results ?? []
    if (mode === 'append') {
      const seen = new Set(searchResults.value.map(trackDedupeKey))
      const next = [...searchResults.value]
      for (const item of incoming) {
        const k = trackDedupeKey(item)
        if (!seen.has(k)) {
          seen.add(k)
          next.push(item)
        }
      }
      searchResults.value = next
    } else {
      searchResults.value = incoming
    }

    const activeSourceCount = sourceScope.length > 0 ? sourceScope.length : enabledSources.value.length
    hasMoreResults.value = incoming.length >= pageLimit || (activeSourceCount > 1 && incoming.length > 0 && incoming.length >= Math.min(10, pageLimit))
  } catch (err) {
    if (unmounted || seq !== searchSeq || exploreMode.value) return
    hasSearched.value = true
    searchErrors.value = [
      {
        source_id: 'search',
        source_name: t('online.scopeOnline'),
        message: err instanceof Error ? err.message : String(err),
      },
    ]
  } finally {
    if (!unmounted && seq === searchSeq) {
      searching.value = false
      loadingMore.value = false
    }
  }
}

async function runExplore(
  sourceId: string,
  exploreUrl: string | null = null,
  page = 1,
): Promise<void> {
  const seq = ++exploreSeq
  searchSeq += 1
  clearDebounce()
  searching.value = false
  loadingMore.value = false
  exploreMode.value = true
  exploring.value = true
  exploreError.value = null
  searchErrors.value = []
  searchToasts.value = []
  activeExploreUrl.value = exploreUrl

  try {
    const res = await onlineMusicApi.explore(sourceId, exploreUrl, page, pageLimit)
    if (unmounted || seq !== exploreSeq || !exploreMode.value) return
    searchPage.value = page
    hasSearched.value = true
    exploreCategories.value = res.categories ?? []
    searchResults.value = res.results ?? []
    hasMoreResults.value = (res.results?.length ?? 0) >= pageLimit
  } catch (err) {
    if (unmounted || seq !== exploreSeq || !exploreMode.value) return
    exploreError.value = err instanceof Error ? err.message : String(err)
    searchResults.value = []
    hasMoreResults.value = false
  } finally {
    if (!unmounted && seq === exploreSeq) {
      exploring.value = false
    }
  }
}

function handleSwitchPage(nextPage: number): void {
  if (nextPage < 1 || searching.value || loadingMore.value || exploring.value) return
  if (exploreMode.value && selectedSourceIds.value.length === 1) {
    void runExplore(selectedSourceIds.value[0]!, activeExploreUrl.value, nextPage)
  } else {
    void runSearch(nextPage, 'replace', true)
  }
}

function handleLoadMore(): void {
  if (!hasMoreResults.value || searching.value || loadingMore.value) return
  void runSearch(searchPage.value + 1, 'append', true)
}

function goManageDataSources(): void {
  void router.push({
    path: '/settings',
    query: { tab: 'domain-datasources', domain: 'music' },
  })
}

async function initSources(): Promise<void> {
  const seq = ++initSeq
  loadingInit.value = true
  try {
    const list = await onlineMusicApi.listSources()
    if (unmounted || seq !== initSeq) return
    sources.value = list
    if (props.initialSourceId && list.some((s) => s.enabled && s.id === props.initialSourceId)) {
      selectedSourceIds.value = [props.initialSourceId]
    }
  } catch {
    if (unmounted || seq !== initSeq) return
    sources.value = []
  } finally {
    if (!unmounted && seq === initSeq) {
      loadingInit.value = false
      if (searchKeyword.value.trim() && enabledSources.value.length > 0) {
        void runSearch(1, 'replace', true)
      }
    }
  }
}

onMounted(() => {
  void initSources()
})

onBeforeUnmount(() => {
  unmounted = true
  clearDebounce()
})
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
            class="md-input h-10 w-full !pl-10 text-xs"
            @input="onLocalInput"
          />
        </div>

        <button
          type="submit"
          :disabled="!searchKeyword.trim() || enabledSources.length === 0 || searching"
          class="md-btn md-btn-filled md-state h-10 px-5"
        >
          <LuLoader2 v-if="searching" class="h-4 w-4 animate-spin" />
          <LuSearch v-else class="h-4 w-4" />
          <span>{{ searching ? t('online.searchingBtn') : t('online.searchBtn') }}</span>
        </button>

        <button
          v-if="selectedSourceIds.length === 1 && !searching"
          type="button"
          :disabled="exploring"
          class="md-btn md-btn-tonal md-state h-10 px-4"
          @click="runExplore(selectedSourceIds[0]!, null, 1)"
        >
          <LuLoader2 v-if="exploring" class="h-4 w-4 animate-spin" />
          <LuCompass v-else class="h-4 w-4" />
          <span>{{ exploring ? t('online.exploringBtn') : t('online.exploreBtn') }}</span>
        </button>
      </form>

      <!-- Source Filter Chips -->
      <div v-if="enabledSources.length > 0" class="flex flex-wrap items-center gap-1.5">
        <span class="text-[11px] text-[var(--md-sys-color-on-surface-variant)]">
          {{ t('online.sourceScopeLabel') }}
        </span>
        <button
          type="button"
          class="md-chip md-state !h-7 !rounded-full !px-2.5 !text-[11px]"
          :class="selectedSourceIds.length === 0 ? 'md-chip-selected' : ''"
          @click="selectAllSources"
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

      <!-- Empty Sources Banner -->
      <div
        v-else-if="!loadingInit"
        class="flex flex-wrap items-center justify-between gap-2 text-xs text-[var(--md-sys-color-on-surface-variant)]"
      >
        <span>{{ t('online.emptySourcesHint') }}</span>
        <button
          v-if="authStore.isAdmin"
          type="button"
          class="md-btn md-btn-sm md-btn-tonal md-state"
          @click="goManageDataSources"
        >
          <LuSettings2 class="h-3.5 w-3.5" />
          <span>{{ t('online.goToSettings') }}</span>
        </button>
      </div>
    </div>

    <!-- Explore Category Chips (when user triggers explore) -->
    <div
      v-if="exploreMode && exploreCategories.length > 0 && selectedSourceIds.length === 1"
      class="flex flex-wrap items-center gap-1.5"
    >
      <button
        v-for="cat in exploreCategories"
        :key="`${cat.title}-${cat.url}`"
        type="button"
        class="md-chip md-state !h-7 !rounded-full !px-3 !text-[11px]"
        :class="activeExploreUrl === cat.url ? 'md-chip-selected' : ''"
        @click="runExplore(selectedSourceIds[0]!, cat.url, 1)"
      >
        {{ cat.title }}
      </button>
    </div>

    <!-- Explore Error -->
    <div
      v-if="exploreError"
      class="rounded-xl bg-[var(--md-sys-color-error-container)]/70 px-3.5 py-2 text-xs text-[var(--md-sys-color-on-error-container)]"
    >
      {{ exploreError }}
    </div>

    <!-- Toasts / Per-Source Errors -->
    <div
      v-if="searchErrors.length > 0 || searchToasts.length > 0"
      class="space-y-1.5 text-xs"
    >
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
        class="rounded-xl bg-[var(--md-sys-color-error-container)]/70 px-3.5 py-2 text-[var(--md-sys-color-on-error-container)]"
      >
        {{ t('online.sourceSearchError', { source: e.source_name, message: e.message }) }}
      </div>
    </div>

    <!-- Prompt before initial search -->
    <div
      v-if="!hasSearched && !searching && !exploring && enabledSources.length > 0"
      class="rounded-[24px] bg-[var(--md-sys-color-surface-container-low)] py-14 text-center text-xs text-[var(--md-sys-color-on-surface-variant)]"
    >
      {{ t('online.promptToSearch') }}
    </div>

    <!-- Empty Results -->
    <div
      v-else-if="hasSearched && !searching && !exploring && searchResults.length === 0"
      class="rounded-[24px] bg-[var(--md-sys-color-surface-container-low)] py-14 text-center text-xs text-[var(--md-sys-color-on-surface-variant)]"
    >
      {{
        exploreMode
          ? t('online.exploreEmpty')
          : lastSearchKeyword || searchKeyword.trim()
            ? t('online.noResultsFor', { q: lastSearchKeyword || searchKeyword.trim() })
            : t('online.noResults')
      }}
    </div>

    <!-- Results Table & Footer -->
    <template v-else-if="searchResults.length > 0">
      <OnlineTrackTable :tracks="searchResults" />

      <div
        class="flex flex-wrap items-center justify-between gap-3 rounded-2xl bg-[var(--md-sys-color-surface-container-low)] px-4 py-3 text-xs"
      >
        <div class="flex items-center gap-2">
          <button
            type="button"
            :disabled="searchPage <= 1 || searching || loadingMore || exploring"
            class="md-btn md-btn-sm md-btn-outlined md-state"
            @click="handleSwitchPage(searchPage - 1)"
          >
            <LuChevronLeft class="h-3.5 w-3.5" />
            <span>{{ t('online.prevPage') }}</span>
          </button>
          <span class="px-2 font-mono text-[var(--md-sys-color-on-surface-variant)]">
            {{ t('online.pageSummary', { page: searchPage, count: searchResults.length }) }}
          </span>
          <button
            type="button"
            :disabled="!hasMoreResults || searching || loadingMore || exploring"
            class="md-btn md-btn-sm md-btn-outlined md-state"
            @click="handleSwitchPage(searchPage + 1)"
          >
            <span>{{ t('online.nextPage') }}</span>
            <LuChevronRight class="h-3.5 w-3.5" />
          </button>
        </div>

        <div class="flex flex-wrap items-center gap-2">
          <button
            type="button"
            class="md-btn md-btn-sm md-btn-tonal md-state"
            @click="playerStore.playOnlineTracks(searchResults, 0)"
          >
            <LuPlay class="h-3.5 w-3.5 fill-current" />
            <span>{{ t('common.playAll') }}</span>
          </button>
          <button
            type="button"
            class="md-btn md-btn-sm md-btn-outlined md-state"
            @click="playerStore.shuffleAndPlayOnline(searchResults)"
          >
            <LuShuffle class="h-3.5 w-3.5" />
            <span>{{ t('common.shufflePlay') }}</span>
          </button>
          <button
            v-if="!exploreMode"
            type="button"
            :disabled="!hasMoreResults || searching || loadingMore"
            class="md-btn md-btn-sm md-btn-tonal md-state"
            @click="handleLoadMore"
          >
            <LuLoader2 v-if="loadingMore" class="h-3.5 w-3.5 animate-spin" />
            <LuPlus v-else class="h-3.5 w-3.5" />
            <span>
              {{
                loadingMore
                  ? t('online.loadingMore')
                  : hasMoreResults
                    ? t('online.loadMore')
                    : t('online.noMoreResults')
              }}
            </span>
          </button>
        </div>
      </div>
    </template>
  </div>
</template>
