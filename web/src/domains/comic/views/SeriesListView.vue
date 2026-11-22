<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useComicI18n as useI18n } from '@/domains/comic/i18n'
import { LuArrowUpDown, LuGlobe, LuLayers, LuX } from 'vue-icons-plus/lu'
import { api } from '@/domains/comic/api'
import { resolveLibrarySeriesCount, useSidebarStore } from '@/domains/comic/store'
import { useSnackbarStore } from '@/stores/snackbar'
import { useUploadStore } from '@/stores/uploads'
import AppSelect from '@/components/ui/AppSelect.vue'
import MdSegmentedButton, { type SegmentedOption } from '@/components/ui/MdSegmentedButton.vue'
import RefreshButton from '@/components/ui/RefreshButton.vue'
import OnlineComicSearchSection from '@/domains/comic/components/OnlineComicSearchSection.vue'
import SeriesCard from '@/domains/comic/components/SeriesCard.vue'
import type { SeriesStatus, SeriesSummaryDto } from '@/domains/comic/types'

type ComicSearchScope = 'local' | 'online'

const props = defineProps<{
  presetMode?: 'all' | 'continue' | 'favorites'
}>()

const { t } = useI18n()
const route = useRoute()
const router = useRouter()
const upload = useUploadStore()
const sidebarStore = useSidebarStore()
const snackbarStore = useSnackbarStore()

const canUseOnlineScope = computed(
  () => props.presetMode !== 'continue' && props.presetMode !== 'favorites',
)
const searchScope = ref<ComicSearchScope>(
  canUseOnlineScope.value && route.query.scope === 'online' ? 'online' : 'local',
)
const onlineKeyword = ref<string>(typeof route.query.q === 'string' ? route.query.q : '')
const onlineSourceId = computed(() =>
  typeof route.query.source === 'string' && route.query.source.trim()
    ? route.query.source.trim()
    : null,
)
let syncingRouteQuery = false
let syncRouteSeq = 0
let pendingRouteQuery: Record<string, string | string[]> | null = null

const searchScopeOptions = computed<SegmentedOption<ComicSearchScope>[]>(() => [
  { value: 'local', label: t('online.scopeLocal'), icon: LuLayers },
  { value: 'online', label: t('online.scopeOnline'), icon: LuGlobe },
])

const seriesList = ref<SeriesSummaryDto[]>([])
const loading = ref(true)
let fetchSeq = 0

const statusFilter = ref<string>('')
const readStatusFilter = ref<string>('')
const sortBy = ref<'updated_at' | 'created_at' | 'title' | 'chapter_count'>('updated_at')
const sortDir = ref<'desc' | 'asc'>('desc')

const selectedLibraryId = computed(() =>
  typeof route.query.library_id === 'string' ? route.query.library_id : ''
)

const activeQueryTags = computed(() => {
  const badges: Array<{ key: string; label: string }> = []
  if (route.query.q) {
    badges.push({ key: 'q', label: String(route.query.q) })
  }
  if (route.query.shelf_id) {
    const shelfId = String(route.query.shelf_id)
    const shelfName =
      sidebarStore.shelves.find((s) => s.shelf.id === shelfId)?.shelf.name || shelfId
    badges.push({ key: 'shelf_id', label: t('nav.shelves') + ': ' + shelfName })
  }
  if (route.query.writer) {
    badges.push({ key: 'writer', label: t('series.writers') + ': ' + String(route.query.writer) })
  }
  if (route.query.penciller) {
    badges.push({
      key: 'penciller',
      label: t('series.pencillers') + ': ' + String(route.query.penciller),
    })
  }
  if (route.query.tag) {
    badges.push({ key: 'tag', label: '#' + String(route.query.tag) })
  }
  if (route.query.genre) {
    badges.push({ key: 'genre', label: t('series.genres') + ': ' + String(route.query.genre) })
  }
  return badges
})

const statusOptions = computed(() => [
  { value: '', label: t('series.filterStatus') + ': ' + t('series.all') },
  { value: 'ongoing', label: t('series.statusOngoing') },
  { value: 'completed', label: t('series.statusCompleted') },
  { value: 'hiatus', label: t('series.statusHiatus') },
])

const readOptions = computed(() => [
  { value: '', label: t('series.filterReadStatus') + ': ' + t('series.all') },
  { value: 'unread', label: t('series.readUnread') },
  { value: 'in_progress', label: t('series.readInProgress') },
  { value: 'completed', label: t('series.readCompleted') },
])

const libraryOptions = computed(() => [
  { value: '', label: t('series.filterLibrary') + ': ' + t('series.all') },
  ...sidebarStore.libraries.map((l) => ({
    value: l.id,
    label: l.name,
    badge: resolveLibrarySeriesCount(l),
  })),
])

const sortOptions = computed(() => [
  { value: 'updated_at', label: t('series.sortUpdated') },
  { value: 'created_at', label: t('series.sortCreated') },
  { value: 'title', label: t('series.sortTitle') },
  { value: 'chapter_count', label: t('series.sortChapters') },
])

function onSelectLibrary(val: string | number): void {
  const next = { ...route.query }
  const id = String(val)
  if (id) {
    next.library_id = id
  } else {
    delete next.library_id
  }
  void router.replace({ query: next })
}

function applyScopeFromRoute(): void {
  if (!canUseOnlineScope.value) {
    searchScope.value = 'local'
    return
  }
  searchScope.value = route.query.scope === 'online' ? 'online' : 'local'
  onlineKeyword.value = typeof route.query.q === 'string' ? route.query.q : ''
}

function queryValuesEqual(a: unknown, b: unknown): boolean {
  if (Array.isArray(a) || Array.isArray(b)) {
    const la = Array.isArray(a) ? a : a ? [a] : []
    const lb = Array.isArray(b) ? b : b ? [b] : []
    return la.length === lb.length && la.every((v, i) => v === lb[i])
  }
  return a === b
}

function syncOnlineScopeToRoute(
  nextScope: ComicSearchScope,
  nextKw: string,
  nextSourceId?: string | null,
): void {
  const nextQuery: Record<string, string | string[]> = {}
  if (nextScope === 'online') {
    nextQuery.scope = 'online'
    if (nextSourceId) {
      nextQuery.source = nextSourceId
    }
    if (nextKw.trim()) {
      nextQuery.q = nextKw.trim()
    }
  } else {
    for (const [k, v] of Object.entries(route.query)) {
      if (k === 'scope' || k === 'source') continue
      if (typeof v === 'string') {
        nextQuery[k] = v
      } else if (Array.isArray(v)) {
        const cleaned = v.filter((item): item is string => typeof item === 'string')
        if (cleaned.length > 0) nextQuery[k] = cleaned
      }
    }
    if (nextKw.trim()) {
      nextQuery.q = nextKw.trim()
    } else {
      delete nextQuery.q
    }
  }

  const cur = syncingRouteQuery && pendingRouteQuery ? pendingRouteQuery : route.query
  const same =
    Object.keys(nextQuery).length === Object.keys(cur).length &&
    Object.entries(nextQuery).every(([k, v]) => queryValuesEqual(cur[k], v))
  if (same) return

  const seq = ++syncRouteSeq
  syncingRouteQuery = true
  pendingRouteQuery = nextQuery
  void router.replace({ path: route.path, query: nextQuery }).finally(() => {
    if (seq === syncRouteSeq) {
      syncingRouteQuery = false
      pendingRouteQuery = null
    }
  })
}

function switchSearchScope(next: ComicSearchScope): void {
  if (!canUseOnlineScope.value) return
  searchScope.value = next
  if (next === 'online') {
    fetchSeq += 1
    loading.value = false
    onlineKeyword.value = typeof route.query.q === 'string' ? route.query.q : ''
  }
  syncOnlineScopeToRoute(next, onlineKeyword.value, next === 'online' ? onlineSourceId.value : null)
  if (next === 'local') {
    void fetchSeries()
  }
}

function handleOnlineKeywordUpdate(nextKw: string): void {
  onlineKeyword.value = nextKw
  syncOnlineScopeToRoute('online', nextKw, onlineSourceId.value)
}

function handleOnlineSourceUpdate(nextSourceId: string | null): void {
  syncOnlineScopeToRoute('online', onlineKeyword.value, nextSourceId)
}

async function fetchSeries(): Promise<void> {
  if (searchScope.value === 'online') {
    loading.value = false
    return
  }
  const seq = ++fetchSeq
  loading.value = true
  try {
    const effectiveRead =
      props.presetMode === 'continue'
        ? 'in_progress'
        : (readStatusFilter.value as '' | 'unread' | 'in_progress' | 'completed')
    const list = await api.listSeries({
      search: (route.query.q as string) || undefined,
      library_id: selectedLibraryId.value || undefined,
      shelf_id: (route.query.shelf_id as string) || undefined,
      status: (statusFilter.value as SeriesStatus | '') || undefined,
      read_status: effectiveRead || undefined,
      favorites_only: props.presetMode === 'favorites',
      writer: (route.query.writer as string) || undefined,
      penciller: (route.query.penciller as string) || undefined,
      tag: (route.query.tag as string) || undefined,
      genre: (route.query.genre as string) || undefined,
      sort_by: sortBy.value,
      sort_dir: sortDir.value,
    })
    if (seq !== fetchSeq) return
    seriesList.value = list
  } catch (err) {
    if (seq !== fetchSeq) return
    snackbarStore.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  } finally {
    if (seq === fetchSeq) {
      loading.value = false
    }
  }
}

function removeQueryKey(key: string): void {
  const next = { ...route.query }
  delete next[key]
  void router.replace({ query: next })
}

function resetAllFilters(): void {
  statusFilter.value = ''
  readStatusFilter.value = ''
  void router.replace({ query: {} })
}

async function refreshPage(): Promise<void> {
  await Promise.all([sidebarStore.refresh(), fetchSeries()])
}

onMounted(async () => {
  applyScopeFromRoute()
  if (sidebarStore.libraries.length === 0) {
    await sidebarStore.refresh()
  }
  if (searchScope.value !== 'online') {
    await fetchSeries()
  } else {
    loading.value = false
  }
})

watch(
  [() => route.query, () => props.presetMode],
  () => {
    if (syncingRouteQuery) return
    applyScopeFromRoute()
    if (searchScope.value !== 'online') {
      void fetchSeries()
    } else {
      loading.value = false
    }
  },
  { deep: true },
)

watch([statusFilter, readStatusFilter, sortBy, sortDir, () => upload.catalogVersion], () => {
  if (searchScope.value !== 'online') {
    void fetchSeries()
  }
})
</script>

<template>
  <div class="mx-auto max-w-7xl space-y-6 px-6 py-7 sm:px-8">
    <div
      v-if="canUseOnlineScope"
      class="flex flex-wrap items-center justify-between gap-3"
    >
      <MdSegmentedButton
        :model-value="searchScope"
        :options="searchScopeOptions"
        size="sm"
        :show-check="false"
        @update:model-value="switchSearchScope($event)"
      />
    </div>

    <OnlineComicSearchSection
      v-if="searchScope === 'online' && canUseOnlineScope"
      :keyword="onlineKeyword"
      :initial-source-id="onlineSourceId"
      @update:keyword="handleOnlineKeywordUpdate"
      @update:source-id="handleOnlineSourceUpdate"
    />

    <template v-else>
    <div class="flex flex-wrap items-center justify-between gap-3">
      <div class="flex flex-wrap items-center gap-2.5">
        <div class="w-44">
          <AppSelect
            variant="compact"
            :model-value="statusFilter"
            :options="statusOptions"
            @update:model-value="statusFilter = String($event)"
          />
        </div>
        <div v-if="presetMode !== 'continue'" class="w-44">
          <AppSelect
            variant="compact"
            :model-value="readStatusFilter"
            :options="readOptions"
            @update:model-value="readStatusFilter = String($event)"
          />
        </div>
        <div class="w-48">
          <AppSelect
            variant="compact"
            :model-value="selectedLibraryId"
            :options="libraryOptions"
            @update:model-value="onSelectLibrary"
          />
        </div>

        <button
          v-for="badge in activeQueryTags"
          :key="badge.key"
          type="button"
          class="md-chip md-chip-selected md-state !h-8 !rounded-full"
          @click="removeQueryKey(badge.key)"
        >
          <span>{{ badge.label }}</span>
          <LuX class="h-3 w-3" />
        </button>

        <button
          v-if="
            statusFilter || readStatusFilter || selectedLibraryId || activeQueryTags.length > 0
          "
          type="button"
          class="md-btn md-btn-sm md-btn-text md-state"
          @click="resetAllFilters"
        >
          {{ t('series.clearFilters') }}
        </button>
      </div>

      <div class="flex items-center gap-2">
        <div
          class="inline-flex h-8 items-center gap-1.5 rounded-full bg-[var(--md-sys-color-surface-container)] px-3 text-xs font-medium text-[var(--md-sys-color-on-surface-variant)]"
        >
          <LuArrowUpDown class="h-3.5 w-3.5 text-[var(--md-sys-color-on-surface-variant)]" />
          <AppSelect
            variant="inline"
            :model-value="sortBy"
            :options="sortOptions"
            @update:model-value="sortBy = $event as typeof sortBy"
          />
          <button
            type="button"
            :title="sortDir === 'desc' ? t('series.sortDesc') : t('series.sortAsc')"
            class="md-state ml-0.5 rounded-full px-1.5 py-0.5 text-[11px] font-semibold text-[var(--md-sys-color-primary)]"
            @click="sortDir = sortDir === 'desc' ? 'asc' : 'desc'"
          >
            {{ sortDir === 'desc' ? '↓' : '↑' }}
          </button>
        </div>

        <RefreshButton :action="refreshPage" :busy="loading" />
      </div>
    </div>

    <div
      v-if="!loading && seriesList.length === 0"
      class="md-card-outlined rounded-3xl border-dashed py-20 text-center text-sm text-[var(--md-sys-color-on-surface-variant)]"
    >
      {{ t('series.emptyFiltered') }}
    </div>

    <div
      v-else
      class="grid grid-cols-2 gap-5 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 xl:grid-cols-6"
    >
      <SeriesCard v-for="s in seriesList" :key="s.id" :series="s" />
    </div>
    </template>
  </div>
</template>
