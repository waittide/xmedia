<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useComicI18n as useI18n } from '@/domains/comic/i18n'
import { LuArrowUpDown, LuX } from 'vue-icons-plus/lu'
import { api } from '@/domains/comic/api'
import { resolveLibrarySeriesCount, useSidebarStore } from '@/domains/comic/store'
import { useUploadStore } from '@/stores/uploads'
import AppSelect from '@/components/ui/AppSelect.vue'
import SeriesCard from '@/domains/comic/components/SeriesCard.vue'
import type { SeriesStatus, SeriesSummaryDto } from '@/domains/comic/types'

const props = defineProps<{
  presetMode?: 'all' | 'continue' | 'favorites'
}>()

const { t } = useI18n()
const route = useRoute()
const router = useRouter()
const upload = useUploadStore()
const sidebarStore = useSidebarStore()

const seriesList = ref<SeriesSummaryDto[]>([])
const loading = ref(true)

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

async function fetchSeries(): Promise<void> {
  loading.value = true
  try {
    const effectiveRead =
      props.presetMode === 'continue'
        ? 'in_progress'
        : (readStatusFilter.value as '' | 'unread' | 'in_progress' | 'completed')
    seriesList.value = await api.listSeries({
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
  } finally {
    loading.value = false
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

onMounted(async () => {
  if (sidebarStore.libraries.length === 0) {
    await sidebarStore.refresh()
  }
  await fetchSeries()
})

watch(
  [
    statusFilter,
    readStatusFilter,
    sortBy,
    sortDir,
    () => route.query,
    () => props.presetMode,
    () => upload.catalogVersion,
  ],
  fetchSeries,
  { deep: true }
)
</script>

<template>
  <div class="mx-auto max-w-7xl space-y-6 p-6 sm:p-8">
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
          class="inline-flex h-7 items-center gap-1 rounded-full bg-[var(--accent-soft)] px-2.5 text-xs font-medium text-[var(--accent)] transition hover:opacity-85"
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
          class="text-xs font-semibold text-[var(--accent)] hover:underline"
          @click="resetAllFilters"
        >
          {{ t('series.clearFilters') }}
        </button>
      </div>

      <div class="flex items-center gap-2">
        <div
          class="inline-flex h-8 items-center gap-1.5 rounded-full bg-[var(--bg-mantle)] px-3 text-xs font-medium text-[var(--text-muted)]"
        >
          <LuArrowUpDown class="h-3.5 w-3.5 text-[var(--text-secondary)]" />
          <AppSelect
            variant="inline"
            :model-value="sortBy"
            :options="sortOptions"
            @update:model-value="sortBy = $event as typeof sortBy"
          />
          <button
            type="button"
            :title="sortDir === 'desc' ? t('series.sortDesc') : t('series.sortAsc')"
            class="ml-0.5 rounded px-1 text-[11px] font-semibold text-[var(--accent)] transition hover:bg-[var(--sidebar-hover)]"
            @click="sortDir = sortDir === 'desc' ? 'asc' : 'desc'"
          >
            {{ sortDir === 'desc' ? '↓' : '↑' }}
          </button>
        </div>
      </div>
    </div>

    <div
      v-if="!loading && seriesList.length === 0"
      class="rounded-3xl border border-dashed border-[var(--border-subtle)] bg-[var(--bg-surface)]/50 py-20 text-center text-sm text-[var(--text-secondary)]"
    >
      {{ t('series.emptyFiltered') }}
    </div>

    <div
      v-else
      class="grid grid-cols-2 gap-5 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 xl:grid-cols-6"
    >
      <SeriesCard v-for="s in seriesList" :key="s.id" :series="s" />
    </div>
  </div>
</template>
