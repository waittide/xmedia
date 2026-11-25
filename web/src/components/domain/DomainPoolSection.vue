<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useRouter } from 'vue-router'
import { LuFilm, LuTv } from 'vue-icons-plus/lu'
import { api as coreApi } from '@/api/client'
import { MdEmptyState, MdSkeleton, type SelectOption } from '@/components/ui'
import { useDelayedLoading } from '@/composables/useDelayedLoading'
import {
  getDomainPlugin,
  itemRoute,
  type DomainPoolFetchResult,
  type PoolPosterCardVM,
  type PoolTrackItemVM,
} from '@/sources'
import { useAuthStore } from '@/stores/auth'
import { useSnackbarStore } from '@/stores/snackbar'
import type { BindMediaItemsResultDto, CoreLibraryDto, MediaKind, UserDto } from '@/types/api'
import MediaErrorState from './MediaErrorState.vue'
import PaginationBar from './PaginationBar.vue'
import PoolControlsBar from './PoolControlsBar.vue'
import PoolMusicTrackList from './PoolMusicTrackList.vue'
import PoolPosterGrid from './PoolPosterGrid.vue'
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

const page = ref(1)
const total = ref(0)
const poolResult = ref<DomainPoolFetchResult>({ kind: 'posters', items: [], total: 0 })

let reqSeq = 0
let searchTimer: ReturnType<typeof setTimeout> | null = null

const activePlugin = computed(() => getDomainPlugin(props.domain))
const pageSize = computed(() => activePlugin.value.pool.pageSize)
const totalPages = computed(() => Math.max(1, Math.ceil(total.value / pageSize.value)))

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
  const res = poolResult.value
  if (res.kind === 'posters' || res.kind === 'tracks') return res.items.length === 0
  return res.shows.length === 0 && res.movies.length === 0
})

const visibleUnboundIds = computed<string[]>(() => {
  const filterUnbound = <T extends { id: string; bound: boolean }>(list: readonly T[]) =>
    list.filter((entry) => !isTargetingSelf.value || !entry.bound).map((entry) => entry.id)

  const res = poolResult.value
  if (res.kind === 'posters') return filterUnbound(res.items)
  if (res.kind === 'tracks') return filterUnbound(res.items)
  return [...filterUnbound(res.shows), ...filterUnbound(res.movies)]
})

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
    const res = await activePlugin.value.pool.fetchPool(
      {
        query: q,
        libraryId: libId,
        page: page.value,
        pageSize: pageSize.value,
      },
      (key, named) => (named ? t(key, named) : t(key)),
    )
    if (seq !== reqSeq) return
    poolResult.value = res
    total.value = res.total
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

function markLocalBound(ids: ReadonlySet<string>, bound: boolean): void {
  if (!isTargetingSelf.value) return
  const patchPoster = (item: PoolPosterCardVM): PoolPosterCardVM =>
    ids.has(item.id) ? { ...item, bound } : item
  const patchTrack = (item: PoolTrackItemVM): PoolTrackItemVM =>
    ids.has(item.id) ? { ...item, bound } : item

  const res = poolResult.value
  if (res.kind === 'posters') {
    poolResult.value = { ...res, items: res.items.map(patchPoster) }
  } else if (res.kind === 'tracks') {
    poolResult.value = { ...res, items: res.items.map(patchTrack) }
  } else {
    poolResult.value = {
      ...res,
      shows: res.shows.map(patchPoster),
      movies: res.movies.map(patchPoster),
    }
  }
}

async function handleBindSingle(id: string): Promise<void> {
  if (busyIds.value.has(id)) return
  setBusy(id, true)
  try {
    const res = await activePlugin.value.pool.bindItems({
      item_ids: [id],
      user_id: effectiveTargetUserId.value,
    })
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
    await activePlugin.value.pool.unbindItem(id, effectiveTargetUserId.value)
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
  const bindAlbumFn = activePlugin.value.pool.bindAlbum
  if (!bindAlbumFn) return
  const key = `album:${albumId}`
  if (busyIds.value.has(key)) return
  setBusy(key, true)
  try {
    const res = await bindAlbumFn(albumId, effectiveTargetUserId.value)
    if (isTargetingSelf.value && poolResult.value.kind === 'tracks') {
      poolResult.value = {
        ...poolResult.value,
        items: poolResult.value.items.map((tr) =>
          tr.albumId === albumId ? { ...tr, bound: true } : tr,
        ),
      }
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
    const res = await activePlugin.value.pool.bindItems({
      library_id: selectedLibraryId.value,
      user_id: effectiveTargetUserId.value,
    })
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
    const res = await activePlugin.value.pool.bindItems({
      item_ids: ids,
      user_id: effectiveTargetUserId.value,
    })
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
    <PoolControlsBar
      :libraries="libraries"
      :selected-library-id="selectedLibraryId"
      :is-admin="authStore.isAdmin"
      :user-select-options="userSelectOptions"
      :target-user-id="targetUserId"
      :visible-unbound-count="visibleUnboundIds.length"
      :batch-busy="batchBusy"
      @select-library="selectLibrary"
      @update:target-user-id="targetUserId = $event"
      @bind-library="handleBindWholeLibrary"
      @bind-visible="handleBindAllVisible"
    />

    <!-- Error / Skeleton / Empty -->
    <MediaErrorState v-if="error" :message="error" :retrying="loading" @retry="fetchPool" />

    <div v-else-if="loading" aria-busy="true">
      <template v-if="showSkeleton">
        <div v-if="activePlugin.pool.skeletonMode === 'list'" class="space-y-2">
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

    <!-- Poster Grid (Book / Comic) -->
    <template v-else-if="poolResult.kind === 'posters'">
      <PoolPosterGrid
        :items="poolResult.items"
        :grid-class="gridClass"
        :fallback-icon="domainIcon"
        :is-targeting-self="isTargetingSelf"
        :busy-ids="busyIds"
        @bind="handleBindSingle"
        @unbind="handleUnbindSingle"
        @open="openBoundItem"
      />
      <PaginationBar
        v-if="activePlugin.pool.paginated && totalPages > 1"
        :page="page"
        :page-size="pageSize"
        :total="total"
        :total-pages="totalPages"
        @update:page="handlePageChange"
      />
    </template>

    <!-- Music Track List -->
    <template v-else-if="poolResult.kind === 'tracks'">
      <PoolMusicTrackList
        :items="poolResult.items"
        :is-targeting-self="isTargetingSelf"
        :busy-ids="busyIds"
        @bind="handleBindSingle"
        @unbind="handleUnbindSingle"
        @bind-album="handleBindAlbum"
      />
      <PaginationBar
        v-if="activePlugin.pool.paginated && totalPages > 1"
        :page="page"
        :page-size="pageSize"
        :total="total"
        :total-pages="totalPages"
        @update:page="handlePageChange"
      />
    </template>

    <!-- Video Sections (TV Shows + Movies) -->
    <div v-else class="space-y-6">
      <section v-if="poolResult.shows.length > 0" class="space-y-3">
        <h2 class="text-title-medium flex items-center gap-2 text-on-surface">
          <LuTv class="h-4 w-4 text-primary" aria-hidden="true" />
          <span>{{ t('pool.showsHeading') }}</span>
          <span class="text-body-small text-on-surface-variant">{{ poolResult.shows.length }}</span>
        </h2>
        <PoolPosterGrid
          :items="poolResult.shows"
          :grid-class="gridClass"
          :fallback-icon="LuTv"
          :is-targeting-self="isTargetingSelf"
          :busy-ids="busyIds"
          @bind="handleBindSingle"
          @unbind="handleUnbindSingle"
          @open="openBoundItem"
        />
      </section>

      <section v-if="poolResult.movies.length > 0" class="space-y-3">
        <h2 class="text-title-medium flex items-center gap-2 text-on-surface">
          <LuFilm class="h-4 w-4 text-primary" aria-hidden="true" />
          <span>{{ t('pool.moviesHeading') }}</span>
          <span class="text-body-small text-on-surface-variant">{{ poolResult.movies.length }}</span>
        </h2>
        <PoolPosterGrid
          :items="poolResult.movies"
          :grid-class="gridClass"
          :fallback-icon="LuFilm"
          :is-targeting-self="isTargetingSelf"
          :busy-ids="busyIds"
          @bind="handleBindSingle"
          @unbind="handleUnbindSingle"
          @open="openBoundItem"
        />
      </section>
    </div>
  </div>
</template>
