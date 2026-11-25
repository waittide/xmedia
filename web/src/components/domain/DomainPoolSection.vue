<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useRouter } from 'vue-router'
import {
  LuCheck,
  LuDatabase,
  LuDisc3,
  LuExternalLink,
  LuFilm,
  LuFolderPlus,
  LuLibrary,
  LuPlus,
  LuTv,
  LuUserCheck,
  LuX,
} from 'vue-icons-plus/lu'
import { api as coreApi, withAuthToken } from '@/api/client'
import {
  MdButton,
  MdChip,
  MdEmptyState,
  MdSelect,
  MdSkeleton,
  MediaCoverImage,
  type SelectOption,
} from '@/components/ui'
import { useDelayedLoading } from '@/composables/useDelayedLoading'
import { api as bookApi } from '@/domains/book/api'
import type { PoolBookSummaryDto } from '@/domains/book/types'
import { api as comicApi } from '@/domains/comic/api'
import type { PoolSeriesSummaryDto } from '@/domains/comic/types'
import { api as musicApi, buildCoverUrl as buildMusicCoverUrl } from '@/domains/music/api'
import type { PoolTrackDto } from '@/domains/music/types'
import { api as videoApi, buildPosterUrl as buildVideoPosterUrl } from '@/domains/video/api'
import type { PoolShowSummaryDto, PoolVideoSummaryDto } from '@/domains/video/types'
import { itemRoute } from '@/sources'
import { useAuthStore } from '@/stores/auth'
import { useSnackbarStore } from '@/stores/snackbar'
import type { BindMediaItemsResultDto, CoreLibraryDto, MediaKind, UserDto } from '@/types/api'
import MediaErrorState from './MediaErrorState.vue'
import PaginationBar from './PaginationBar.vue'
import { mediaGridClassFor, resolveDomainIcon } from './mediaLayout'

const props = withDefaults(
  defineProps<{
    domain: MediaKind
    searchQuery?: string
  }>(),
  {
    searchQuery: '',
  },
)

const emit = defineEmits<{
  changed: []
}>()

const { t } = useI18n()
const router = useRouter()
const authStore = useAuthStore()
const snackbar = useSnackbarStore()

const libraries = ref<CoreLibraryDto[]>([])
const selectedLibraryId = ref<string | null>(null)
const users = ref<UserDto[]>([])
const targetUserId = ref<string>('')

const loading = ref(true)
const error = ref<string | null>(null)
const batchBusy = ref(false)
const busyIds = ref<Set<string>>(new Set())

// Pagination (Book & Music)
const page = ref(1)
const total = ref(0)
const pageSize = computed(() => (props.domain === 'music' ? 50 : 24))
const totalPages = computed(() => Math.max(1, Math.ceil(total.value / pageSize.value)))

// Domain items
const poolBooks = ref<PoolBookSummaryDto[]>([])
const poolSeries = ref<PoolSeriesSummaryDto[]>([])
const poolTracks = ref<PoolTrackDto[]>([])
const poolShows = ref<PoolShowSummaryDto[]>([])
const poolMovies = ref<PoolVideoSummaryDto[]>([])

let reqSeq = 0
let searchTimer: ReturnType<typeof setTimeout> | null = null

const showSkeleton = useDelayedLoading(() => loading.value)
const domainIcon = computed(() => resolveDomainIcon(props.domain))
const gridClass = computed(() => mediaGridClassFor('poster'))

const isTargetingSelf = computed(
  () => !targetUserId.value || targetUserId.value === authStore.user?.id,
)

const effectiveTargetUserId = computed<string | null>(() =>
  authStore.isAdmin && targetUserId.value && targetUserId.value !== authStore.user?.id
    ? targetUserId.value
    : null,
)

const userSelectOptions = computed<SelectOption[]>(() => {
  const opts: SelectOption[] = [{ value: '', label: t('pool.assignSelf') }]
  for (const u of users.value) {
    if (u.id === authStore.user?.id) continue
    opts.push({
      value: u.id,
      label: u.display_name ? `${u.display_name} (@${u.username})` : `@${u.username}`,
    })
  }
  return opts
})

const isEmpty = computed(() => {
  if (props.domain === 'book') return poolBooks.value.length === 0
  if (props.domain === 'comic') return poolSeries.value.length === 0
  if (props.domain === 'music') return poolTracks.value.length === 0
  return poolShows.value.length === 0 && poolMovies.value.length === 0
})

const visibleUnboundIds = computed<string[]>(() => {
  if (props.domain === 'book') {
    return poolBooks.value
      .filter((b) => !isTargetingSelf.value || !b.bound)
      .map((b) => b.id)
  }
  if (props.domain === 'comic') {
    return poolSeries.value
      .filter((s) => !isTargetingSelf.value || !s.bound)
      .map((s) => s.id)
  }
  if (props.domain === 'music') {
    return poolTracks.value
      .filter((tr) => !isTargetingSelf.value || !tr.bound)
      .map((tr) => tr.id)
  }
  return [
    ...poolShows.value.filter((s) => !isTargetingSelf.value || !s.bound).map((s) => s.id),
    ...poolMovies.value.filter((m) => !isTargetingSelf.value || !m.bound).map((m) => m.id),
  ]
})

function formatDuration(secs: number): string {
  if (!Number.isFinite(secs) || secs <= 0) return '0:00'
  const m = Math.floor(secs / 60)
  const s = Math.floor(secs % 60)
  return `${m}:${String(s).padStart(2, '0')}`
}

function setBusy(id: string, busy: boolean): void {
  const next = new Set(busyIds.value)
  if (busy) next.add(id)
  else next.delete(id)
  busyIds.value = next
}

async function loadMeta(): Promise<void> {
  try {
    libraries.value = await coreApi.listLibraries(props.domain)
  } catch {
    libraries.value = []
  }
  if (authStore.isAdmin) {
    try {
      users.value = await coreApi.listUsers()
    } catch {
      users.value = []
    }
  }
}

async function fetchPool(): Promise<void> {
  const seq = ++reqSeq
  loading.value = true
  error.value = null
  const q = props.searchQuery?.trim() || undefined
  const libId = selectedLibraryId.value || undefined

  try {
    if (props.domain === 'book') {
      const res = await bookApi.listPool({
        keyword: q,
        library_id: libId,
        page: page.value,
        page_size: pageSize.value,
        sort_by: 'created_at',
        sort_order: 'desc',
      })
      if (seq !== reqSeq) return
      poolBooks.value = res.items
      total.value = res.total
    } else if (props.domain === 'comic') {
      const list = await comicApi.listPool({
        search: q,
        library_id: libId,
      })
      if (seq !== reqSeq) return
      poolSeries.value = list
      total.value = list.length
    } else if (props.domain === 'music') {
      const res = await musicApi.listPool({
        search: q,
        library_id: libId,
        page: page.value,
        page_size: pageSize.value,
        sort_by: 'created_at',
        sort_order: 'desc',
      })
      if (seq !== reqSeq) return
      poolTracks.value = res.items
      total.value = res.total
    } else {
      const res = await videoApi.listPool({
        search: q,
        library_id: libId,
      })
      if (seq !== reqSeq) return
      poolShows.value = res.shows
      poolMovies.value = res.movies
      total.value = res.shows.length + res.movies.length
    }
  } catch (err) {
    if (seq !== reqSeq) return
    error.value = err instanceof Error ? err.message : String(err)
  } finally {
    if (seq === reqSeq) {
      loading.value = false
    }
  }
}

function reportBindResult(res: BindMediaItemsResultDto): void {
  if (res.bound_count > 0) {
    snackbar.show(t('pool.boundToast', { count: res.bound_count }))
    emit('changed')
  } else {
    snackbar.show(t('pool.alreadyBoundToast'))
  }
}

function markLocalBound(ids: Set<string>, bound: boolean): void {
  if (!isTargetingSelf.value) return
  if (props.domain === 'book') {
    poolBooks.value = poolBooks.value.map((b) => (ids.has(b.id) ? { ...b, bound } : b))
  } else if (props.domain === 'comic') {
    poolSeries.value = poolSeries.value.map((s) => (ids.has(s.id) ? { ...s, bound } : s))
  } else if (props.domain === 'music') {
    poolTracks.value = poolTracks.value.map((tr) => (ids.has(tr.id) ? { ...tr, bound } : tr))
  } else {
    poolShows.value = poolShows.value.map((s) => (ids.has(s.id) ? { ...s, bound } : s))
    poolMovies.value = poolMovies.value.map((m) => (ids.has(m.id) ? { ...m, bound } : m))
  }
}

async function handleBindSingle(id: string): Promise<void> {
  if (busyIds.value.has(id)) return
  setBusy(id, true)
  try {
    const payload = { item_ids: [id], user_id: effectiveTargetUserId.value }
    let res: BindMediaItemsResultDto
    if (props.domain === 'book') res = await bookApi.bindItems(payload)
    else if (props.domain === 'comic') res = await comicApi.bindItems(payload)
    else if (props.domain === 'music') res = await musicApi.bindItems(payload)
    else res = await videoApi.bindItems(payload)

    markLocalBound(new Set([id]), true)
    reportBindResult(res)
  } catch (err) {
    snackbar.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  } finally {
    setBusy(id, false)
  }
}

async function handleUnbindSingle(id: string): Promise<void> {
  if (busyIds.value.has(id)) return
  setBusy(id, true)
  try {
    const uid = effectiveTargetUserId.value
    if (props.domain === 'book') await bookApi.unbindItem(id, uid)
    else if (props.domain === 'comic') await comicApi.unbindItem(id, uid)
    else if (props.domain === 'music') await musicApi.unbindItem(id, uid)
    else await videoApi.unbindItem(id, uid)

    markLocalBound(new Set([id]), false)
    snackbar.show(t('pool.unboundToast'))
    emit('changed')
  } catch (err) {
    snackbar.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  } finally {
    setBusy(id, false)
  }
}

async function handleBindAlbum(albumId: string): Promise<void> {
  const key = `album:${albumId}`
  if (busyIds.value.has(key)) return
  setBusy(key, true)
  try {
    const res = await musicApi.bindItems({
      album_id: albumId,
      user_id: effectiveTargetUserId.value,
    })
    if (isTargetingSelf.value) {
      poolTracks.value = poolTracks.value.map((tr) =>
        tr.album_id === albumId ? { ...tr, bound: true } : tr,
      )
    }
    reportBindResult(res)
  } catch (err) {
    snackbar.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  } finally {
    setBusy(key, false)
  }
}

async function handleBindWholeLibrary(): Promise<void> {
  if (!selectedLibraryId.value || batchBusy.value) return
  batchBusy.value = true
  try {
    const payload = {
      library_id: selectedLibraryId.value,
      user_id: effectiveTargetUserId.value,
    }
    let res: BindMediaItemsResultDto
    if (props.domain === 'book') res = await bookApi.bindItems(payload)
    else if (props.domain === 'comic') res = await comicApi.bindItems(payload)
    else if (props.domain === 'music') res = await musicApi.bindItems(payload)
    else res = await videoApi.bindItems(payload)

    reportBindResult(res)
    await fetchPool()
  } catch (err) {
    snackbar.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  } finally {
    batchBusy.value = false
  }
}

async function handleBindAllVisible(): Promise<void> {
  const ids = visibleUnboundIds.value
  if (ids.length === 0 || batchBusy.value) return
  batchBusy.value = true
  try {
    const payload = {
      item_ids: ids,
      user_id: effectiveTargetUserId.value,
    }
    let res: BindMediaItemsResultDto
    if (props.domain === 'book') res = await bookApi.bindItems(payload)
    else if (props.domain === 'comic') res = await comicApi.bindItems(payload)
    else if (props.domain === 'music') res = await musicApi.bindItems(payload)
    else res = await videoApi.bindItems(payload)

    markLocalBound(new Set(ids), true)
    reportBindResult(res)
  } catch (err) {
    snackbar.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  } finally {
    batchBusy.value = false
  }
}

function selectLibrary(id: string | null): void {
  if (selectedLibraryId.value === id) return
  selectedLibraryId.value = id
  page.value = 1
  void fetchPool()
}

function openBoundItem(id: string, kind?: 'movie' | 'show'): void {
  if (props.domain === 'video' && kind === 'movie') {
    void router.push({
      path: `/video/item/${encodeURIComponent(id)}`,
      query: { kind: 'movie' },
    })
    return
  }
  void router.push(itemRoute({ scope: 'local', domain: props.domain, id }))
}

function handlePageChange(nextPage: number): void {
  page.value = nextPage
  void fetchPool()
}

watch(
  () => props.domain,
  () => {
    selectedLibraryId.value = null
    page.value = 1
    void loadMeta()
    void fetchPool()
  },
)

watch(
  () => props.searchQuery,
  () => {
    if (searchTimer) clearTimeout(searchTimer)
    searchTimer = setTimeout(() => {
      page.value = 1
      void fetchPool()
    }, 220)
  },
)

onMounted(() => {
  void loadMeta()
  void fetchPool()
})

onBeforeUnmount(() => {
  if (searchTimer) {
    clearTimeout(searchTimer)
    searchTimer = null
  }
})
</script>

<template>
  <div class="space-y-4">
    <!-- Controls Strip: Library filter chips + Admin target user + Batch claim -->
    <div class="flex flex-wrap items-center justify-between gap-2.5">
      <div class="no-scrollbar flex min-w-0 flex-1 items-center gap-1.5 overflow-x-auto py-0.5">
        <MdChip
          variant="filter"
          :icon="LuDatabase"
          :label="t('pool.allLibraries')"
          :selected="selectedLibraryId === null"
          hide-check
          @click="selectLibrary(null)"
        />
        <MdChip
          v-for="lib in libraries"
          :key="lib.id"
          variant="filter"
          :icon="LuLibrary"
          :label="`${lib.name} (${lib.item_count})`"
          :selected="selectedLibraryId === lib.id"
          hide-check
          @click="selectLibrary(lib.id)"
        />
      </div>

      <div class="flex shrink-0 flex-wrap items-center gap-2">
        <div
          v-if="authStore.isAdmin && userSelectOptions.length > 1"
          class="flex items-center gap-1.5"
        >
          <LuUserCheck class="h-4 w-4 text-on-surface-variant" aria-hidden="true" />
          <MdSelect
            v-model="targetUserId"
            :options="userSelectOptions"
            variant="compact"
            size="sm"
            :placeholder="t('pool.assignTargetLabel')"
          />
        </div>

        <MdButton
          v-if="selectedLibraryId"
          variant="tonal"
          size="sm"
          :icon="LuFolderPlus"
          :loading="batchBusy"
          @click="handleBindWholeLibrary"
        >
          {{ t('pool.bindLibraryBtn') }}
        </MdButton>
        <MdButton
          v-else-if="visibleUnboundIds.length > 0"
          variant="tonal"
          size="sm"
          :icon="LuFolderPlus"
          :loading="batchBusy"
          @click="handleBindAllVisible"
        >
          {{ t('pool.bindAllVisibleBtn') }}
        </MdButton>
      </div>
    </div>

    <!-- Error / Skeleton / Empty -->
    <MediaErrorState v-if="error" :message="error" :retrying="loading" @retry="fetchPool" />

    <div v-else-if="loading" aria-busy="true">
      <template v-if="showSkeleton">
        <div v-if="domain === 'music'" class="space-y-2">
          <MdSkeleton v-for="i in 8" :key="`mp-sk-${i}`" variant="rect" height="3.5rem" />
        </div>
        <div v-else :class="gridClass">
          <MdSkeleton v-for="i in 12" :key="`gp-sk-${i}`" variant="card" aspect="poster" />
        </div>
      </template>
    </div>

    <MdEmptyState
      v-else-if="isEmpty"
      :icon="domainIcon"
      :title="t('pool.emptyTitle')"
      :description="t('pool.emptyDesc')"
    />

    <!-- Book Pool Grid -->
    <template v-else-if="domain === 'book'">
      <div :class="gridClass">
        <article
          v-for="book in poolBooks"
          :key="book.id"
          class="group flex flex-col gap-2"
        >
          <div class="relative overflow-hidden rounded-corner-md bg-surface-container-low">
            <MediaCoverImage
              :src="book.has_cover ? bookApi.coverUrl(book.id, book.updated_at) : null"
              :alt="book.metadata.title"
              aspect-class="aspect-[2/3]"
              rounded="rounded-corner-md"
              :fallback-icon="domainIcon"
            />
            <span
              v-if="isTargetingSelf && book.bound"
              class="text-label-small absolute top-2 left-2 inline-flex items-center gap-1 rounded-corner-full bg-primary px-2 py-0.5 text-on-primary shadow-elevation-1"
            >
              <LuCheck class="h-3 w-3" aria-hidden="true" />
              {{ t('pool.boundLabel') }}
            </span>
            <span
              class="text-label-small absolute right-2 bottom-2 rounded-corner-xs bg-surface-container-highest/90 px-1.5 py-0.5 uppercase text-on-surface backdrop-blur-xs"
            >
              {{ book.format }}
            </span>
          </div>

          <div class="min-w-0 space-y-0.5 px-0.5">
            <p class="text-title-small truncate text-on-surface" :title="book.metadata.title">
              {{ book.metadata.title }}
            </p>
            <p class="text-body-small truncate text-on-surface-variant">
              {{ book.metadata.authors.join(', ') || book.library_name || '—' }}
            </p>
          </div>

          <div class="mt-auto flex items-center gap-1.5 pt-0.5">
            <template v-if="isTargetingSelf && book.bound">
              <MdButton
                variant="tonal"
                size="sm"
                class="flex-1"
                :icon="LuExternalLink"
                @click="openBoundItem(book.id)"
              >
                {{ t('pool.openLocalBtn') }}
              </MdButton>
              <MdButton
                variant="text"
                size="sm"
                :icon="LuX"
                :loading="busyIds.has(book.id)"
                :title="t('pool.unbindBtn')"
                @click="handleUnbindSingle(book.id)"
              />
            </template>
            <MdButton
              v-else
              variant="filled"
              size="sm"
              class="w-full"
              :icon="LuPlus"
              :loading="busyIds.has(book.id)"
              @click="handleBindSingle(book.id)"
            >
              {{ t('pool.bindBtn') }}
            </MdButton>
          </div>
        </article>
      </div>

      <PaginationBar
        v-if="totalPages > 1"
        :page="page"
        :page-size="pageSize"
        :total="total"
        :total-pages="totalPages"
        @update:page="handlePageChange"
      />
    </template>

    <!-- Comic Pool Grid -->
    <template v-else-if="domain === 'comic'">
      <div :class="gridClass">
        <article
          v-for="series in poolSeries"
          :key="series.id"
          class="group flex flex-col gap-2"
        >
          <div class="relative overflow-hidden rounded-corner-md bg-surface-container-low">
            <MediaCoverImage
              :src="series.cover_url ? withAuthToken(series.cover_url) : null"
              :alt="series.title"
              aspect-class="aspect-[2/3]"
              rounded="rounded-corner-md"
              :fallback-icon="domainIcon"
            />
            <span
              v-if="isTargetingSelf && series.bound"
              class="text-label-small absolute top-2 left-2 inline-flex items-center gap-1 rounded-corner-full bg-primary px-2 py-0.5 text-on-primary shadow-elevation-1"
            >
              <LuCheck class="h-3 w-3" aria-hidden="true" />
              {{ t('pool.boundLabel') }}
            </span>
          </div>

          <div class="min-w-0 space-y-0.5 px-0.5">
            <p class="text-title-small truncate text-on-surface" :title="series.title">
              {{ series.title }}
            </p>
            <p class="text-body-small truncate text-on-surface-variant">
              {{ t('pool.chaptersCount', { count: series.chapter_count }) }}
            </p>
          </div>

          <div class="mt-auto flex items-center gap-1.5 pt-0.5">
            <template v-if="isTargetingSelf && series.bound">
              <MdButton
                variant="tonal"
                size="sm"
                class="flex-1"
                :icon="LuExternalLink"
                @click="openBoundItem(series.id)"
              >
                {{ t('pool.openLocalBtn') }}
              </MdButton>
              <MdButton
                variant="text"
                size="sm"
                :icon="LuX"
                :loading="busyIds.has(series.id)"
                :title="t('pool.unbindBtn')"
                @click="handleUnbindSingle(series.id)"
              />
            </template>
            <MdButton
              v-else
              variant="filled"
              size="sm"
              class="w-full"
              :icon="LuPlus"
              :loading="busyIds.has(series.id)"
              @click="handleBindSingle(series.id)"
            >
              {{ t('pool.bindBtn') }}
            </MdButton>
          </div>
        </article>
      </div>
    </template>

    <!-- Music Pool Track List -->
    <template v-else-if="domain === 'music'">
      <div class="divide-y divide-outline-variant/50 overflow-hidden rounded-corner-lg bg-surface-container-low">
        <div
          v-for="track in poolTracks"
          :key="track.id"
          class="flex items-center gap-3 px-3 py-2.5 transition-colors hover:bg-surface-container"
        >
          <div class="h-10 w-10 shrink-0 overflow-hidden rounded-corner-sm bg-surface-container-high">
            <MediaCoverImage
              :src="buildMusicCoverUrl(track.cover_key, 120)"
              :alt="track.title"
              aspect-class="aspect-square"
              rounded="rounded-corner-sm"
              :fallback-icon="LuDisc3"
              fallback-icon-class="h-4 w-4 text-on-surface-variant/60"
            />
          </div>

          <div class="min-w-0 flex-1">
            <div class="flex items-center gap-2">
              <p class="text-title-small truncate text-on-surface">{{ track.title }}</p>
              <span
                v-if="isTargetingSelf && track.bound"
                class="text-label-small shrink-0 rounded-corner-full bg-primary-container px-2 py-0.5 text-on-primary-container"
              >
                {{ t('pool.boundLabel') }}
              </span>
            </div>
            <p class="text-body-small truncate text-on-surface-variant">
              {{ [track.artist_name, track.album_title].filter(Boolean).join(' · ') }}
            </p>
          </div>

          <span class="text-label-small hidden shrink-0 tabular-nums text-on-surface-variant sm:inline">
            {{ track.format.toUpperCase() }} · {{ formatDuration(track.duration_secs) }}
          </span>

          <div class="flex shrink-0 items-center gap-1.5">
            <MdButton
              v-if="track.album_id"
              variant="text"
              size="sm"
              class="hidden md:inline-flex"
              :loading="busyIds.has(`album:${track.album_id}`)"
              @click="handleBindAlbum(track.album_id)"
            >
              {{ t('pool.bindAlbumBtn') }}
            </MdButton>

            <MdButton
              v-if="isTargetingSelf && track.bound"
              variant="outlined"
              size="sm"
              :icon="LuX"
              :loading="busyIds.has(track.id)"
              @click="handleUnbindSingle(track.id)"
            >
              {{ t('pool.unbindBtn') }}
            </MdButton>
            <MdButton
              v-else
              variant="filled"
              size="sm"
              :icon="LuPlus"
              :loading="busyIds.has(track.id)"
              @click="handleBindSingle(track.id)"
            >
              {{ t('pool.bindBtn') }}
            </MdButton>
          </div>
        </div>
      </div>

      <PaginationBar
        v-if="totalPages > 1"
        :page="page"
        :page-size="pageSize"
        :total="total"
        :total-pages="totalPages"
        @update:page="handlePageChange"
      />
    </template>

    <!-- Video Pool (TV Shows + Movies) -->
    <template v-else>
      <div class="space-y-6">
        <section v-if="poolShows.length > 0" class="space-y-3">
          <h2 class="text-title-medium flex items-center gap-2 text-on-surface">
            <LuTv class="h-4 w-4 text-primary" aria-hidden="true" />
            <span>{{ t('pool.showsHeading') }}</span>
            <span class="text-body-small text-on-surface-variant">{{ poolShows.length }}</span>
          </h2>
          <div :class="gridClass">
            <article
              v-for="show in poolShows"
              :key="show.id"
              class="group flex flex-col gap-2"
            >
              <div class="relative overflow-hidden rounded-corner-md bg-surface-container-low">
                <MediaCoverImage
                  :src="buildVideoPosterUrl(show.poster_key)"
                  :alt="show.title"
                  aspect-class="aspect-[2/3]"
                  rounded="rounded-corner-md"
                  :fallback-icon="LuTv"
                />
                <span
                  v-if="isTargetingSelf && show.bound"
                  class="text-label-small absolute top-2 left-2 inline-flex items-center gap-1 rounded-corner-full bg-primary px-2 py-0.5 text-on-primary shadow-elevation-1"
                >
                  <LuCheck class="h-3 w-3" aria-hidden="true" />
                  {{ t('pool.boundLabel') }}
                </span>
              </div>

              <div class="min-w-0 space-y-0.5 px-0.5">
                <p class="text-title-small truncate text-on-surface" :title="show.title">
                  {{ show.title }}
                </p>
                <p class="text-body-small truncate text-on-surface-variant">
                  {{
                    [
                      show.year ? String(show.year) : '',
                      t('pool.episodesCount', { count: show.episode_count }),
                    ]
                      .filter(Boolean)
                      .join(' · ')
                  }}
                </p>
              </div>

              <div class="mt-auto flex items-center gap-1.5 pt-0.5">
                <template v-if="isTargetingSelf && show.bound">
                  <MdButton
                    variant="tonal"
                    size="sm"
                    class="flex-1"
                    :icon="LuExternalLink"
                    @click="openBoundItem(show.id)"
                  >
                    {{ t('pool.openLocalBtn') }}
                  </MdButton>
                  <MdButton
                    variant="text"
                    size="sm"
                    :icon="LuX"
                    :loading="busyIds.has(show.id)"
                    :title="t('pool.unbindBtn')"
                    @click="handleUnbindSingle(show.id)"
                  />
                </template>
                <MdButton
                  v-else
                  variant="filled"
                  size="sm"
                  class="w-full"
                  :icon="LuPlus"
                  :loading="busyIds.has(show.id)"
                  @click="handleBindSingle(show.id)"
                >
                  {{ t('pool.bindBtn') }}
                </MdButton>
              </div>
            </article>
          </div>
        </section>

        <section v-if="poolMovies.length > 0" class="space-y-3">
          <h2 class="text-title-medium flex items-center gap-2 text-on-surface">
            <LuFilm class="h-4 w-4 text-primary" aria-hidden="true" />
            <span>{{ t('pool.moviesHeading') }}</span>
            <span class="text-body-small text-on-surface-variant">{{ poolMovies.length }}</span>
          </h2>
          <div :class="gridClass">
            <article
              v-for="movie in poolMovies"
              :key="movie.id"
              class="group flex flex-col gap-2"
            >
              <div class="relative overflow-hidden rounded-corner-md bg-surface-container-low">
                <MediaCoverImage
                  :src="buildVideoPosterUrl(movie.poster_key)"
                  :alt="movie.title"
                  aspect-class="aspect-[2/3]"
                  rounded="rounded-corner-md"
                  :fallback-icon="LuFilm"
                />
                <span
                  v-if="isTargetingSelf && movie.bound"
                  class="text-label-small absolute top-2 left-2 inline-flex items-center gap-1 rounded-corner-full bg-primary px-2 py-0.5 text-on-primary shadow-elevation-1"
                >
                  <LuCheck class="h-3 w-3" aria-hidden="true" />
                  {{ t('pool.boundLabel') }}
                </span>
              </div>

              <div class="min-w-0 space-y-0.5 px-0.5">
                <p class="text-title-small truncate text-on-surface" :title="movie.title">
                  {{ movie.title }}
                </p>
                <p class="text-body-small truncate text-on-surface-variant">
                  {{
                    [movie.year ? String(movie.year) : '', movie.resolution || '']
                      .filter(Boolean)
                      .join(' · ') || '—'
                  }}
                </p>
              </div>

              <div class="mt-auto flex items-center gap-1.5 pt-0.5">
                <template v-if="isTargetingSelf && movie.bound">
                  <MdButton
                    variant="tonal"
                    size="sm"
                    class="flex-1"
                    :icon="LuExternalLink"
                    @click="openBoundItem(movie.id, 'movie')"
                  >
                    {{ t('pool.openLocalBtn') }}
                  </MdButton>
                  <MdButton
                    variant="text"
                    size="sm"
                    :icon="LuX"
                    :loading="busyIds.has(movie.id)"
                    :title="t('pool.unbindBtn')"
                    @click="handleUnbindSingle(movie.id)"
                  />
                </template>
                <MdButton
                  v-else
                  variant="filled"
                  size="sm"
                  class="w-full"
                  :icon="LuPlus"
                  :loading="busyIds.has(movie.id)"
                  @click="handleBindSingle(movie.id)"
                >
                  {{ t('pool.bindBtn') }}
                </MdButton>
              </div>
            </article>
          </div>
        </section>
      </div>
    </template>
  </div>
</template>
