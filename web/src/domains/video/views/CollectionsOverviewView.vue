<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useI18n as useGlobalI18n } from 'vue-i18n'
import {
  LuLibrary,
  LuFolderHeart,
  LuFolderPlus,
  LuTag,
  LuSearch,
  LuRefreshCw,
  LuSettings,
  LuPencil,
  LuTrash2,
  LuArrowUpRight,
  LuX,
} from 'vue-icons-plus/lu'
import AppSelect, { type SelectOption } from '@/components/ui/AppSelect.vue'
import { useAuthStore } from '@/stores/auth'
import { useDialogStore } from '@/stores/dialog'
import { api } from '../api'
import PosterImage from '../components/PosterImage.vue'
import { useVideoI18n as useI18n } from '../i18n'
import { useVideoStore } from '../store'
import type { VideoLibraryDto, VideoShelfDto } from '../types'

type CollectionKind = 'libraries' | 'shelves' | 'tags' | 'genres'
type SortKey = 'count_desc' | 'count_asc' | 'name_asc'

const { t } = useI18n()
const { t: globalT } = useGlobalI18n()
const route = useRoute()
const router = useRouter()
const authStore = useAuthStore()
const dialogStore = useDialogStore()
const videoStore = useVideoStore()

const kind = computed<CollectionKind>(() => {
  if (route.name === 'video-libraries-overview') return 'libraries'
  if (route.name === 'video-shelves-overview') return 'shelves'
  if (route.name === 'video-genres-overview') return 'genres'
  return 'tags'
})

const filterKeyword = ref('')
const sortKey = ref<SortKey>('count_desc')
const newShelfName = ref('')
const creatingShelf = ref(false)
const scanningId = ref<string | null>(null)

watch(kind, () => {
  filterKeyword.value = ''
  sortKey.value = 'count_desc'
})

const sortOptions = computed<SelectOption[]>(() => [
  { value: 'count_desc', label: t('collections.sortCountDesc') },
  { value: 'count_asc', label: t('collections.sortCountAsc') },
  { value: 'name_asc', label: t('collections.sortNameAsc') },
])

const headerMeta = computed(() => {
  if (kind.value === 'libraries') {
    return {
      title: t('collections.librariesTitle'),
      subtitle: t('collections.librariesSubtitle'),
      total: videoStore.libraries.length,
    }
  }
  if (kind.value === 'shelves') {
    return {
      title: t('collections.shelvesTitle'),
      subtitle: t('collections.shelvesSubtitle'),
      total: videoStore.shelves.length,
    }
  }
  if (kind.value === 'genres') {
    return {
      title: globalT('layoutCustomizer.sidebar.video.genres'),
      subtitle: t('collections.tagsSubtitle'),
      total: videoStore.genres.length,
    }
  }
  return {
    title: t('collections.tagsTitle'),
    subtitle: t('collections.tagsSubtitle'),
    total: videoStore.tags.length,
  }
})

const filteredLibraries = computed(() => {
  const q = filterKeyword.value.trim().toLowerCase()
  const list = videoStore.libraries.filter((lib) => {
    if (!q) return true
    const path = lib.kind.type === 'local_mount' ? lib.kind.root_path.toLowerCase() : ''
    return lib.name.toLowerCase().includes(q) || path.includes(q)
  })
  return [...list].sort((a, b) => {
    if (sortKey.value === 'count_desc') return b.video_count - a.video_count
    if (sortKey.value === 'count_asc') return a.video_count - b.video_count
    return a.name.localeCompare(b.name)
  })
})

const filteredShelves = computed(() => {
  const q = filterKeyword.value.trim().toLowerCase()
  const list = videoStore.shelves.filter((sh) => !q || sh.name.toLowerCase().includes(q))
  return [...list].sort((a, b) => {
    if (sortKey.value === 'count_desc') return b.video_count - a.video_count
    if (sortKey.value === 'count_asc') return a.video_count - b.video_count
    return a.name.localeCompare(b.name)
  })
})

const filteredTags = computed(() => {
  const src = kind.value === 'genres' ? videoStore.genres : videoStore.tags
  const q = filterKeyword.value.trim().toLowerCase()
  const list = src.filter((g) => !q || g.name.toLowerCase().includes(q))
  return [...list].sort((a, b) => {
    if (sortKey.value === 'count_desc') return b.count - a.count
    if (sortKey.value === 'count_asc') return a.count - b.count
    return a.name.localeCompare(b.name)
  })
})

async function triggerLibraryScan(lib: VideoLibraryDto): Promise<void> {
  scanningId.value = lib.id
  try {
    await api.scanLibrary(lib.id)
    await videoStore.refreshSidebar()
    videoStore.bumpCatalogVersion()
  } finally {
    scanningId.value = null
  }
}

async function createShelfInline(): Promise<void> {
  const name = newShelfName.value.trim()
  if (!name) return
  creatingShelf.value = true
  try {
    await api.createShelf({ name })
    newShelfName.value = ''
    await videoStore.refreshSidebar()
  } finally {
    creatingShelf.value = false
  }
}

async function renameShelfPrompt(sh: VideoShelfDto): Promise<void> {
  const nextName = await dialogStore.prompt({
    title: t('shelf.renameTitle'),
    message: t('shelf.renamePrompt'),
    initialValue: sh.name,
  })
  if (!nextName || !nextName.trim() || nextName.trim() === sh.name) return
  const detail = await api.getShelf(sh.id)
  await api.updateShelf(sh.id, {
    name: nextName.trim(),
    description: detail.shelf.description,
    video_ids: detail.videos.map((v) => v.id),
  })
  await videoStore.refreshSidebar()
}

async function removeShelf(sh: VideoShelfDto): Promise<void> {
  const ok = await dialogStore.confirm({
    title: t('shelf.deleteTitle'),
    message: t('shelf.confirmDelete', { name: sh.name }),
    danger: true,
  })
  if (!ok) return
  await api.deleteShelf(sh.id)
  await videoStore.refreshSidebar()
}

function formatLibraryScanText(lib: VideoLibraryDto): string {
  const s = lib.scan_status
  if (s.state === 'scanning') return t('admin.scanning', { scanned: s.scanned })
  if (s.state === 'completed') {
    return t('admin.scanCompleted', {
      scanned: s.scanned,
      added: s.added,
      updated: s.updated,
      missing: s.missing,
    })
  }
  if (s.state === 'failed') return t('admin.scanFailed', { message: s.message })
  return t('admin.scanIdle')
}
</script>

<template>
  <div class="mx-auto max-w-6xl space-y-6 px-6 py-7 sm:px-8">
    <!-- Top Header -->
    <div class="flex flex-wrap items-end justify-between gap-4">
      <div>
        <div class="flex items-center gap-2.5">
          <h1 class="text-2xl font-extrabold tracking-tight text-[var(--ctp-text)]">
            {{ headerMeta.title }}
          </h1>
          <span
            class="rounded-full bg-[var(--ctp-accent-soft)] px-2.5 py-0.5 text-xs font-bold text-[var(--ctp-accent)]"
          >
            {{ headerMeta.total }}
          </span>
        </div>
        <p class="mt-1 text-xs text-[var(--ctp-subtext0)]">
          {{ headerMeta.subtitle }}
        </p>
      </div>

      <RouterLink
        v-if="kind === 'libraries' && authStore.isAdmin"
        to="/settings?tab=libraries"
        class="inline-flex h-9 items-center gap-1.5 rounded-xl bg-[var(--ctp-accent)] px-3.5 text-xs font-semibold text-[var(--ctp-on-accent)] shadow-sm transition hover:opacity-90"
      >
        <LuSettings class="h-3.5 w-3.5" />
        <span>{{ t('collections.manageLibraries') }}</span>
      </RouterLink>

      <form
        v-else-if="kind === 'shelves'"
        class="flex items-center gap-2"
        @submit.prevent="createShelfInline"
      >
        <input
          v-model="newShelfName"
          type="text"
          :placeholder="t('nav.shelfNamePlaceholder')"
          class="h-9 w-52 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-shell)] px-3 text-xs outline-none focus:border-[var(--ctp-accent)]"
        />
        <button
          type="submit"
          :disabled="!newShelfName.trim() || creatingShelf"
          class="inline-flex h-9 items-center gap-1.5 rounded-xl bg-[var(--ctp-accent)] px-3.5 text-xs font-semibold text-[var(--ctp-on-accent)] shadow-sm transition hover:opacity-90 disabled:opacity-50"
        >
          <LuFolderPlus class="h-3.5 w-3.5" />
          <span>{{ t('nav.newShelf') }}</span>
        </button>
      </form>
    </div>

    <!-- Search + Sort Toolbar -->
    <div
      class="flex flex-wrap items-center justify-between gap-3 rounded-2xl border border-[var(--ctp-surface0)]/75 bg-[var(--ctp-shell)] p-3"
    >
      <div class="relative min-w-[220px] max-w-md flex-1">
        <LuSearch
          class="pointer-events-none absolute left-3.5 top-1/2 h-4 w-4 -translate-y-1/2 text-[var(--ctp-subtext0)]"
        />
        <input
          v-model="filterKeyword"
          type="text"
          :placeholder="t('collections.searchPlaceholder')"
          class="h-9 w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] pl-9 pr-8 text-xs text-[var(--ctp-text)] placeholder-[var(--ctp-subtext0)] outline-none focus:border-[var(--ctp-accent)]"
        />
        <button
          v-if="filterKeyword"
          type="button"
          class="absolute right-2.5 top-1/2 -translate-y-1/2 text-[var(--ctp-subtext0)] hover:text-[var(--ctp-text)]"
          @click="filterKeyword = ''"
        >
          <LuX class="h-3.5 w-3.5" />
        </button>
      </div>

      <div class="w-56">
        <AppSelect
          :model-value="sortKey"
          :options="sortOptions"
          variant="compact"
          @update:model-value="sortKey = $event as SortKey"
        />
      </div>
    </div>

    <!-- 1. Libraries -->
    <template v-if="kind === 'libraries'">
      <div
        v-if="filteredLibraries.length === 0"
        class="card-soft rounded-2xl bg-[var(--ctp-shell)] py-14 text-center text-xs text-[var(--ctp-subtext0)]"
      >
        {{ t('collections.emptyResult') }}
      </div>

      <div v-else class="grid grid-cols-1 gap-4 md:grid-cols-2 xl:grid-cols-3">
        <div
          v-for="lib in filteredLibraries"
          :key="lib.id"
          class="group card-soft flex flex-col justify-between rounded-2xl bg-[var(--ctp-shell)] p-5 transition hover:border-[var(--ctp-accent)]/60"
        >
          <div>
            <div class="flex items-start justify-between gap-3">
              <button
                type="button"
                class="flex min-w-0 items-center gap-3 text-left"
                @click="router.push({ path: '/video/catalog', query: { library: lib.id } })"
              >
                <div
                  class="flex h-10 w-10 shrink-0 items-center justify-center rounded-xl bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]"
                >
                  <LuLibrary class="h-5 w-5" />
                </div>
                <div class="min-w-0">
                  <h3 class="truncate text-sm font-bold text-[var(--ctp-text)] group-hover:text-[var(--ctp-accent)]">
                    {{ lib.name }}
                  </h3>
                  <span class="mt-0.5 inline-block rounded bg-[var(--ctp-surface0)]/65 px-1.5 py-0.5 text-[10px] font-medium text-[var(--ctp-subtext1)]">
                    {{
                      lib.kind.type === 'managed'
                        ? t('admin.managedBadge')
                        : t('admin.localMountBadge')
                    }}
                  </span>
                </div>
              </button>

              <span class="shrink-0 rounded-full bg-[var(--ctp-canvas)] px-2.5 py-1 text-xs font-bold text-[var(--ctp-text)]">
                {{ t('common.videosCount', { count: lib.video_count }) }}
              </span>
            </div>

            <p
              v-if="lib.kind.type === 'local_mount'"
              class="mt-3 truncate font-mono text-[11px] text-[var(--ctp-subtext0)]"
              :title="lib.kind.root_path"
            >
              {{ lib.kind.root_path }}
            </p>
            <p v-if="lib.kind.type === 'local_mount'" class="mt-1 text-xs text-[var(--ctp-subtext1)]">
              {{ formatLibraryScanText(lib) }}
            </p>
          </div>

          <div class="mt-4 flex items-center justify-between border-t border-[var(--ctp-surface0)]/60 pt-3">
            <button
              type="button"
              class="inline-flex items-center gap-1 text-xs font-semibold text-[var(--ctp-accent)] hover:underline"
              @click="router.push({ path: '/video/catalog', query: { library: lib.id } })"
            >
              <span>{{ t('collections.seeAll') }}</span>
              <LuArrowUpRight class="h-3.5 w-3.5" />
            </button>

            <button
              v-if="lib.kind.type === 'local_mount' && authStore.isAdmin"
              type="button"
              :disabled="scanningId === lib.id || lib.scan_status.state === 'scanning'"
              class="inline-flex items-center gap-1 rounded-lg border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-2.5 py-1 text-[11px] font-semibold text-[var(--ctp-subtext1)] hover:text-[var(--ctp-text)] disabled:opacity-50"
              @click="triggerLibraryScan(lib)"
            >
              <LuRefreshCw
                class="h-3 w-3"
                :class="scanningId === lib.id || lib.scan_status.state === 'scanning' ? 'animate-spin' : ''"
              />
              <span>{{ t('admin.rescanBtn') }}</span>
            </button>
          </div>
        </div>
      </div>
    </template>

    <!-- 2. Shelves -->
    <template v-else-if="kind === 'shelves'">
      <div
        v-if="filteredShelves.length === 0"
        class="card-soft rounded-2xl bg-[var(--ctp-shell)] py-14 text-center text-xs text-[var(--ctp-subtext0)]"
      >
        {{ t('collections.emptyResult') }}
      </div>

      <div v-else class="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4">
        <div
          v-for="sh in filteredShelves"
          :key="sh.id"
          class="group card-soft flex flex-col justify-between rounded-2xl bg-[var(--ctp-shell)] p-5 transition hover:border-[var(--ctp-accent)]/60"
        >
          <button
            type="button"
            class="flex min-w-0 items-center gap-3 text-left"
            @click="router.push(`/video/shelves/${sh.id}`)"
          >
            <PosterImage
              v-if="sh.poster_key"
              :poster-key="sh.poster_key"
              :size="160"
              aspect="video"
              rounded="rounded-xl"
              class="h-11 w-16 shrink-0"
            />
            <div
              v-else
              class="flex h-11 w-11 shrink-0 items-center justify-center rounded-xl bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]"
            >
              <LuFolderHeart class="h-5 w-5" />
            </div>

            <div class="min-w-0">
              <h3 class="truncate text-sm font-bold text-[var(--ctp-text)] group-hover:text-[var(--ctp-accent)]">
                {{ sh.name }}
              </h3>
              <p class="mt-0.5 text-xs text-[var(--ctp-subtext0)]">
                {{ t('common.videosCount', { count: sh.video_count }) }}
              </p>
            </div>
          </button>

          <div class="mt-4 flex items-center justify-between border-t border-[var(--ctp-surface0)]/60 pt-3">
            <button
              type="button"
              class="inline-flex items-center gap-1 text-xs font-semibold text-[var(--ctp-accent)] hover:underline"
              @click="router.push(`/video/shelves/${sh.id}`)"
            >
              <span>{{ t('collections.seeAll') }}</span>
              <LuArrowUpRight class="h-3.5 w-3.5" />
            </button>

            <div class="flex items-center gap-1">
              <button
                type="button"
                class="rounded-lg p-1.5 text-[var(--ctp-subtext0)] hover:bg-[var(--ctp-surface0)] hover:text-[var(--ctp-text)]"
                @click="renameShelfPrompt(sh)"
              >
                <LuPencil class="h-3.5 w-3.5" />
              </button>
              <button
                type="button"
                class="rounded-lg p-1.5 text-[var(--ctp-subtext0)] hover:bg-[var(--ctp-red)]/10 hover:text-[var(--ctp-red)]"
                @click="removeShelf(sh)"
              >
                <LuTrash2 class="h-3.5 w-3.5" />
              </button>
            </div>
          </div>
        </div>
      </div>
    </template>

    <!-- 3. Tags -->
    <template v-else>
      <div
        v-if="filteredTags.length === 0"
        class="card-soft rounded-2xl bg-[var(--ctp-shell)] py-14 text-center text-xs text-[var(--ctp-subtext0)]"
      >
        {{ t('collections.emptyResult') }}
      </div>

      <div v-else class="grid grid-cols-2 gap-3.5 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5">
        <button
          v-for="g in filteredTags"
          :key="g.name"
          type="button"
          class="group card-soft flex items-center justify-between gap-2.5 rounded-2xl bg-[var(--ctp-shell)] px-4 py-3.5 text-left transition hover:-translate-y-0.5 hover:border-[var(--ctp-accent)]"
          @click="
            router.push({
              path: '/video/catalog',
              query: kind === 'genres' ? { genre: g.name } : { tag: g.name },
            })
          "
        >
          <span class="flex min-w-0 items-center gap-2.5">
            <span class="flex h-8 w-8 shrink-0 items-center justify-center rounded-xl bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]">
              <LuTag class="h-4 w-4" />
            </span>
            <span class="truncate text-xs font-bold text-[var(--ctp-text)] group-hover:text-[var(--ctp-accent)]">
              {{ g.name }}
            </span>
          </span>
          <span class="shrink-0 rounded-full bg-[var(--ctp-canvas)] px-2 py-0.5 text-[11px] font-semibold text-[var(--ctp-subtext0)]">
            {{ g.count }}
          </span>
        </button>
      </div>
    </template>
  </div>
</template>
