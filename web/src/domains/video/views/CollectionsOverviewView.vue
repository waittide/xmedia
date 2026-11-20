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
import RefreshButton from '@/components/ui/RefreshButton.vue'
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
    await Promise.all([
      (async () => {
        await api.scanLibrary(lib.id)
        await videoStore.refreshSidebar()
        videoStore.bumpCatalogVersion()
      })(),
      new Promise((resolve) => setTimeout(resolve, 450)),
    ])
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
  if (s.state === 'scanning') {
    if (s.total && s.total > 0) {
      const pct = Math.min(100, Math.round((s.scanned / s.total) * 100))
      return t('admin.scanningWithTotal', { scanned: s.scanned, total: s.total, pct })
    }
    return t('admin.scanning', { scanned: s.scanned })
  }
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
          <h1 class="text-2xl font-extrabold tracking-tight text-[var(--md-sys-color-on-surface)]">
            {{ headerMeta.title }}
          </h1>
          <span
            class="rounded-full bg-[var(--md-sys-color-primary-container)] px-2.5 py-0.5 text-xs font-bold text-[var(--md-sys-color-on-primary-container)]"
          >
            {{ headerMeta.total }}
          </span>
        </div>
        <p class="mt-1 text-xs text-[var(--md-sys-color-on-surface-variant)]">
          {{ headerMeta.subtitle }}
        </p>
      </div>

      <div class="flex flex-wrap items-center gap-2">
        <RefreshButton size="md" :action="() => videoStore.refreshSidebar()" />

        <RouterLink
          v-if="kind === 'libraries' && authStore.isAdmin"
          to="/settings?tab=libraries"
          class="md-btn md-btn-sm md-btn-filled md-state"
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
            class="md-input h-9 w-52 text-xs"
          />
          <button
            type="submit"
            :disabled="!newShelfName.trim() || creatingShelf"
            class="md-btn md-btn-sm md-btn-filled md-state"
          >
            <LuFolderPlus class="h-3.5 w-3.5" />
            <span>{{ t('nav.newShelf') }}</span>
          </button>
        </form>
      </div>
    </div>

    <!-- Search + Sort Toolbar -->
    <div class="md-card-filled flex flex-wrap items-center justify-between gap-3 p-3">
      <div class="relative min-w-[220px] max-w-md flex-1">
        <LuSearch
          class="pointer-events-none absolute top-1/2 left-3.5 h-4 w-4 -translate-y-1/2 text-[var(--md-sys-color-on-surface-variant)]"
        />
        <input
          v-model="filterKeyword"
          type="text"
          :placeholder="t('collections.searchPlaceholder')"
          class="md-input h-9 w-full pl-9 pr-8 text-xs placeholder-[var(--md-sys-color-on-surface-variant)]"
        />
        <button
          v-if="filterKeyword"
          type="button"
          class="absolute top-1/2 right-2.5 -translate-y-1/2 text-[var(--md-sys-color-on-surface-variant)] hover:text-[var(--md-sys-color-on-surface)]"
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
        class="md-card-filled py-14 text-center text-xs text-[var(--md-sys-color-on-surface-variant)]"
      >
        {{ t('collections.emptyResult') }}
      </div>

      <div v-else class="grid grid-cols-1 gap-4 md:grid-cols-2 xl:grid-cols-3">
        <div
          v-for="lib in filteredLibraries"
          :key="lib.id"
          class="md-card-filled group flex flex-col justify-between p-5 transition hover:shadow-[var(--md-sys-elevation-1)]"
        >
          <div>
            <div class="flex items-start justify-between gap-3">
              <button
                type="button"
                class="flex min-w-0 items-center gap-3 text-left"
                @click="router.push({ path: '/video/catalog', query: { library: lib.id } })"
              >
                <div
                  class="flex h-10 w-10 shrink-0 items-center justify-center rounded-xl bg-[var(--md-sys-color-primary-container)] text-[var(--md-sys-color-on-primary-container)]"
                >
                  <LuLibrary class="h-5 w-5" />
                </div>
                <div class="min-w-0">
                  <h3
                    class="truncate text-sm font-bold text-[var(--md-sys-color-on-surface)] group-hover:text-[var(--md-sys-color-primary)]"
                  >
                    {{ lib.name }}
                  </h3>
                  <span
                    class="mt-0.5 inline-block rounded bg-[var(--md-sys-color-surface-container-high)] px-1.5 py-0.5 text-[10px] font-medium text-[var(--md-sys-color-on-surface-variant)]"
                  >
                    {{
                      lib.kind.type === 'managed'
                        ? t('admin.managedBadge')
                        : t('admin.localMountBadge')
                    }}
                  </span>
                </div>
              </button>

              <span
                class="shrink-0 rounded-full bg-[var(--md-sys-color-surface-container-high)] px-2.5 py-1 text-xs font-bold text-[var(--md-sys-color-on-surface)]"
              >
                {{ t('common.videosCount', { count: lib.video_count }) }}
              </span>
            </div>

            <p
              v-if="lib.kind.type === 'local_mount'"
              class="mt-3 truncate font-mono text-[11px] text-[var(--md-sys-color-on-surface-variant)]"
              :title="lib.kind.root_path"
            >
              {{ lib.kind.root_path }}
            </p>
            <p
              v-if="lib.kind.type === 'local_mount'"
              class="mt-1 text-xs text-[var(--md-sys-color-on-surface-variant)]"
            >
              {{ formatLibraryScanText(lib) }}
            </p>
          </div>

          <div
            class="mt-4 flex items-center justify-between border-t border-[var(--md-sys-color-outline-variant)]/50 pt-3"
          >
            <button
              type="button"
              class="md-btn md-btn-sm md-btn-text md-state !px-2"
              @click="router.push({ path: '/video/catalog', query: { library: lib.id } })"
            >
              <span>{{ t('collections.seeAll') }}</span>
              <LuArrowUpRight class="h-3.5 w-3.5" />
            </button>

            <button
              v-if="lib.kind.type === 'local_mount' && authStore.isAdmin"
              type="button"
              :disabled="scanningId === lib.id || lib.scan_status.state === 'scanning'"
              class="md-btn md-btn-sm md-btn-outlined md-state"
              @click="triggerLibraryScan(lib)"
            >
              <LuRefreshCw
                class="h-3 w-3"
                :class="
                  scanningId === lib.id || lib.scan_status.state === 'scanning' ? 'animate-spin' : ''
                "
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
        class="md-card-filled py-14 text-center text-xs text-[var(--md-sys-color-on-surface-variant)]"
      >
        {{ t('collections.emptyResult') }}
      </div>

      <div v-else class="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4">
        <div
          v-for="sh in filteredShelves"
          :key="sh.id"
          class="md-card-filled group flex flex-col justify-between p-5 transition hover:shadow-[var(--md-sys-elevation-1)]"
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
              class="flex h-11 w-11 shrink-0 items-center justify-center rounded-xl bg-[var(--md-sys-color-primary-container)] text-[var(--md-sys-color-on-primary-container)]"
            >
              <LuFolderHeart class="h-5 w-5" />
            </div>

            <div class="min-w-0">
              <h3
                class="truncate text-sm font-bold text-[var(--md-sys-color-on-surface)] group-hover:text-[var(--md-sys-color-primary)]"
              >
                {{ sh.name }}
              </h3>
              <p class="mt-0.5 text-xs text-[var(--md-sys-color-on-surface-variant)]">
                {{ t('common.videosCount', { count: sh.video_count }) }}
              </p>
            </div>
          </button>

          <div
            class="mt-4 flex items-center justify-between border-t border-[var(--md-sys-color-outline-variant)]/50 pt-3"
          >
            <button
              type="button"
              class="md-btn md-btn-sm md-btn-text md-state !px-2"
              @click="router.push(`/video/shelves/${sh.id}`)"
            >
              <span>{{ t('collections.seeAll') }}</span>
              <LuArrowUpRight class="h-3.5 w-3.5" />
            </button>

            <div class="flex items-center gap-1">
              <button
                type="button"
                class="md-icon-btn md-icon-btn-sm md-state"
                @click="renameShelfPrompt(sh)"
              >
                <LuPencil class="h-3.5 w-3.5" />
              </button>
              <button
                type="button"
                class="md-icon-btn md-icon-btn-sm md-state hover:bg-[var(--md-sys-color-error-container)] hover:text-[var(--md-sys-color-on-error-container)]"
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
        class="md-card-filled py-14 text-center text-xs text-[var(--md-sys-color-on-surface-variant)]"
      >
        {{ t('collections.emptyResult') }}
      </div>

      <div v-else class="grid grid-cols-2 gap-3.5 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5">
        <button
          v-for="g in filteredTags"
          :key="g.name"
          type="button"
          class="md-card-filled md-state group flex items-center justify-between gap-2.5 px-4 py-3.5 text-left transition hover:-translate-y-0.5 hover:shadow-[var(--md-sys-elevation-1)]"
          @click="
            router.push({
              path: '/video/catalog',
              query: kind === 'genres' ? { genre: g.name } : { tag: g.name },
            })
          "
        >
          <span class="flex min-w-0 items-center gap-2.5">
            <span
              class="flex h-8 w-8 shrink-0 items-center justify-center rounded-xl bg-[var(--md-sys-color-secondary-container)] text-[var(--md-sys-color-on-secondary-container)]"
            >
              <LuTag class="h-4 w-4" />
            </span>
            <span
              class="truncate text-xs font-bold text-[var(--md-sys-color-on-surface)] group-hover:text-[var(--md-sys-color-primary)]"
            >
              {{ g.name }}
            </span>
          </span>
          <span
            class="shrink-0 rounded-full bg-[var(--md-sys-color-surface-container-high)] px-2 py-0.5 text-[11px] font-semibold text-[var(--md-sys-color-on-surface-variant)]"
          >
            {{ g.count }}
          </span>
        </button>
      </div>
    </template>
  </div>
</template>
