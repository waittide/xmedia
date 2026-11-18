<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useI18n as useGlobalI18n } from 'vue-i18n'
import {
  LuLibrary,
  LuListMusic,
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
import CoverImage from '../components/CoverImage.vue'
import { useMusicI18n as useI18n } from '../i18n'
import { useMusicStore } from '../store'
import type { LibraryDto, PlaylistSummaryDto } from '../types'

type CollectionKind = 'libraries' | 'playlists' | 'genres' | 'tags'
type SortKey = 'count_desc' | 'count_asc' | 'name_asc' | 'updated_desc'

const { t } = useI18n()
const { t: globalT } = useGlobalI18n()
const route = useRoute()
const router = useRouter()
const authStore = useAuthStore()
const dialogStore = useDialogStore()
const musicStore = useMusicStore()

const kind = computed<CollectionKind>(() => {
  if (route.name === 'music-libraries-overview') return 'libraries'
  if (route.name === 'music-playlists-overview') return 'playlists'
  if (route.name === 'music-tags-overview') return 'tags'
  return 'genres'
})

const filterKeyword = ref('')
const sortKey = ref<SortKey>('count_desc')
const newPlaylistName = ref('')
const creatingPlaylist = ref(false)
const scanningId = ref<string | null>(null)

watch(kind, () => {
  filterKeyword.value = ''
  sortKey.value = 'count_desc'
})

const sortOptions = computed<SelectOption[]>(() => {
  const base: SelectOption[] = [
    { value: 'count_desc', label: t('collections.sortCountDesc') },
    { value: 'count_asc', label: t('collections.sortCountAsc') },
    { value: 'name_asc', label: t('collections.sortNameAsc') },
  ]
  if (kind.value === 'libraries' || kind.value === 'playlists') {
    base.push({ value: 'updated_desc', label: t('collections.sortUpdatedDesc') })
  }
  return base
})

const headerMeta = computed(() => {
  if (kind.value === 'libraries') {
    return {
      title: t('collections.librariesTitle'),
      subtitle: t('collections.librariesSubtitle'),
      total: musicStore.libraries.length,
    }
  }
  if (kind.value === 'playlists') {
    return {
      title: t('collections.playlistsTitle'),
      subtitle: t('collections.playlistsSubtitle'),
      total: musicStore.playlists.length,
    }
  }
  if (kind.value === 'tags') {
    return {
      title: globalT('musicTags.allTitle'),
      subtitle: globalT('musicTags.allSubtitle'),
      total: musicStore.tags.length,
    }
  }
  return {
    title: t('collections.genresTitle'),
    subtitle: t('collections.genresSubtitle'),
    total: musicStore.genres.length,
  }
})

const filteredLibraries = computed(() => {
  const q = filterKeyword.value.trim().toLowerCase()
  const list = musicStore.libraries.filter((lib) => {
    if (!q) return true
    const path = lib.kind.type === 'local_mount' ? lib.kind.root_path.toLowerCase() : ''
    return lib.name.toLowerCase().includes(q) || path.includes(q)
  })
  return [...list].sort((a, b) => {
    if (sortKey.value === 'count_desc') return b.track_count - a.track_count
    if (sortKey.value === 'count_asc') return a.track_count - b.track_count
    if (sortKey.value === 'updated_desc') return b.updated_at.localeCompare(a.updated_at)
    return a.name.localeCompare(b.name)
  })
})

const filteredPlaylists = computed(() => {
  const q = filterKeyword.value.trim().toLowerCase()
  const list = musicStore.playlists.filter((pl) => !q || pl.name.toLowerCase().includes(q))
  return [...list].sort((a, b) => {
    if (sortKey.value === 'count_desc') return b.track_count - a.track_count
    if (sortKey.value === 'count_asc') return a.track_count - b.track_count
    if (sortKey.value === 'updated_desc') return b.updated_at.localeCompare(a.updated_at)
    return a.name.localeCompare(b.name)
  })
})

const filteredGenres = computed(() => {
  const q = filterKeyword.value.trim().toLowerCase()
  const rawList =
    kind.value === 'tags'
      ? musicStore.tags.map((tg) => ({ name: tg.name, song_count: tg.song_count, album_count: null }))
      : musicStore.genres.map((g) => ({ name: g.name, song_count: g.song_count, album_count: g.album_count }))
  const list = rawList.filter((g) => !q || g.name.toLowerCase().includes(q))
  return [...list].sort((a, b) => {
    if (sortKey.value === 'count_desc') return b.song_count - a.song_count
    if (sortKey.value === 'count_asc') return a.song_count - b.song_count
    return a.name.localeCompare(b.name)
  })
})

function canEditPlaylist(pl: PlaylistSummaryDto): boolean {
  return pl.owner_id === authStore.user?.id || authStore.isAdmin
}

async function openLibrary(id: string): Promise<void> {
  await router.push({ path: '/music/songs', query: { library: id } })
}

async function openPlaylist(id: string): Promise<void> {
  await router.push(`/music/playlists/${id}`)
}

async function openGenre(name: string): Promise<void> {
  if (kind.value === 'tags') {
    await router.push({ path: '/music/songs', query: { tag: name } })
  } else {
    await router.push({ path: '/music/songs', query: { genre: name } })
  }
}

async function triggerLibraryScan(lib: LibraryDto): Promise<void> {
  scanningId.value = lib.id
  try {
    await Promise.all([
      (async () => {
        await api.scanLibrary(lib.id)
        await musicStore.refreshSidebar()
        musicStore.bumpCatalogVersion()
      })(),
      new Promise((resolve) => setTimeout(resolve, 450)),
    ])
  } finally {
    scanningId.value = null
  }
}

async function createPlaylistInline(): Promise<void> {
  const name = newPlaylistName.value.trim()
  if (!name) return
  creatingPlaylist.value = true
  try {
    await api.createPlaylist({ name })
    newPlaylistName.value = ''
    await musicStore.refreshSidebar()
  } finally {
    creatingPlaylist.value = false
  }
}

async function renamePlaylistPrompt(pl: PlaylistSummaryDto): Promise<void> {
  const nextName = await dialogStore.prompt({
    title: t('playlist.renameTitle'),
    message: t('playlist.renamePrompt'),
    initialValue: pl.name,
  })
  if (!nextName || !nextName.trim() || nextName.trim() === pl.name) return
  await api.updatePlaylist(pl.id, { name: nextName.trim() })
  await musicStore.refreshSidebar()
}

async function removePlaylist(pl: PlaylistSummaryDto): Promise<void> {
  const ok = await dialogStore.confirm({
    title: t('playlist.deleteTitle'),
    message: t('playlist.confirmDelete', { name: pl.name }),
    danger: true,
  })
  if (!ok) return
  await api.deletePlaylist(pl.id)
  await musicStore.refreshSidebar()
}

function formatLibraryScanText(lib: LibraryDto): string {
  const s = lib.scan_status
  if (s.state === 'scanning') {
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
  if (s.state === 'failed') {
    return t('admin.scanFailed', { message: s.message })
  }
  return t('admin.scanIdle')
}
</script>

<template>
  <div class="mx-auto max-w-6xl space-y-6 px-6 py-7 sm:px-8">
    <!-- Top Header + Actions -->
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

      <!-- Contextual Quick Action + Refresh -->
      <div class="flex flex-wrap items-center gap-2">
        <RefreshButton size="md" :action="() => musicStore.refreshSidebar()" />

        <RouterLink
          v-if="kind === 'libraries' && authStore.isAdmin"
          to="/settings?tab=libraries"
          class="inline-flex h-9 items-center gap-1.5 rounded-xl bg-[var(--ctp-accent)] px-3.5 text-xs font-semibold text-[var(--ctp-on-accent)] shadow-sm transition hover:opacity-90"
        >
          <LuSettings class="h-3.5 w-3.5" />
          <span>{{ t('collections.manageLibraries') }}</span>
        </RouterLink>

        <form
          v-else-if="kind === 'playlists'"
          class="flex items-center gap-2"
          @submit.prevent="createPlaylistInline"
        >
          <input
            v-model="newPlaylistName"
            type="text"
            :placeholder="t('nav.playlistNamePlaceholder')"
            class="h-9 w-52 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-shell)] px-3 text-xs outline-none focus:border-[var(--ctp-accent)]"
          />
          <button
            type="submit"
            :disabled="!newPlaylistName.trim() || creatingPlaylist"
            class="inline-flex h-9 items-center gap-1.5 rounded-xl bg-[var(--ctp-accent)] px-3.5 text-xs font-semibold text-[var(--ctp-on-accent)] shadow-sm transition hover:opacity-90 disabled:opacity-50"
          >
            <LuFolderPlus class="h-3.5 w-3.5" />
            <span>{{ t('nav.newPlaylist') }}</span>
          </button>
        </form>
      </div>
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

    <!-- 1. Libraries Grid -->
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
                @click="openLibrary(lib.id)"
              >
                <div
                  class="flex h-10 w-10 shrink-0 items-center justify-center rounded-xl bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]"
                >
                  <LuLibrary class="h-5 w-5" />
                </div>
                <div class="min-w-0">
                  <h3
                    class="truncate text-sm font-bold text-[var(--ctp-text)] group-hover:text-[var(--ctp-accent)]"
                  >
                    {{ lib.name }}
                  </h3>
                  <span
                    class="mt-0.5 inline-block rounded bg-[var(--ctp-surface0)]/65 px-1.5 py-0.5 text-[10px] font-medium text-[var(--ctp-subtext1)]"
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
                class="shrink-0 rounded-full bg-[var(--ctp-canvas)] px-2.5 py-1 text-xs font-bold text-[var(--ctp-text)]"
              >
                {{ t('common.tracksCount', { count: lib.track_count }) }}
              </span>
            </div>

            <p
              v-if="lib.kind.type === 'local_mount'"
              class="mt-3 truncate font-mono text-[11px] text-[var(--ctp-subtext0)]"
              :title="lib.kind.root_path"
            >
              {{ lib.kind.root_path }}
            </p>
            <p
              v-if="lib.kind.type === 'local_mount'"
              class="mt-1 text-xs text-[var(--ctp-subtext1)]"
            >
              {{ formatLibraryScanText(lib) }}
            </p>
          </div>

          <div
            class="mt-4 flex items-center justify-between border-t border-[var(--ctp-surface0)]/60 pt-3"
          >
            <button
              type="button"
              class="inline-flex items-center gap-1 text-xs font-semibold text-[var(--ctp-accent)] hover:underline"
              @click="openLibrary(lib.id)"
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

    <!-- 2. Playlists Grid -->
    <template v-else-if="kind === 'playlists'">
      <div
        v-if="filteredPlaylists.length === 0"
        class="card-soft rounded-2xl bg-[var(--ctp-shell)] py-14 text-center text-xs text-[var(--ctp-subtext0)]"
      >
        {{ t('collections.emptyResult') }}
      </div>

      <div v-else class="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4">
        <div
          v-for="pl in filteredPlaylists"
          :key="pl.id"
          class="group card-soft flex flex-col justify-between rounded-2xl bg-[var(--ctp-shell)] p-5 transition hover:border-[var(--ctp-accent)]/60"
        >
          <div class="flex items-start justify-between gap-3">
            <button
              type="button"
              class="flex min-w-0 items-center gap-3 text-left"
              @click="openPlaylist(pl.id)"
            >
              <CoverImage
                v-if="pl.cover_key"
                :cover-id="pl.id"
                :has-cover="true"
                :size="120"
                rounded="rounded-xl"
                class="h-11 w-11 shrink-0"
              />
              <div
                v-else
                class="flex h-11 w-11 shrink-0 items-center justify-center rounded-xl bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]"
              >
                <LuListMusic class="h-5 w-5" />
              </div>

              <div class="min-w-0">
                <h3
                  class="truncate text-sm font-bold text-[var(--ctp-text)] group-hover:text-[var(--ctp-accent)]"
                >
                  {{ pl.name }}
                </h3>
                <p class="mt-0.5 text-xs text-[var(--ctp-subtext0)]">
                  {{ t('common.tracksCount', { count: pl.track_count }) }}
                </p>
              </div>
            </button>
          </div>

          <div
            class="mt-4 flex items-center justify-between border-t border-[var(--ctp-surface0)]/60 pt-3"
          >
            <button
              type="button"
              class="inline-flex items-center gap-1 text-xs font-semibold text-[var(--ctp-accent)] hover:underline"
              @click="openPlaylist(pl.id)"
            >
              <span>{{ t('collections.seeAll') }}</span>
              <LuArrowUpRight class="h-3.5 w-3.5" />
            </button>

            <div v-if="canEditPlaylist(pl)" class="flex items-center gap-1">
              <button
                type="button"
                class="rounded-lg p-1.5 text-[var(--ctp-subtext0)] hover:bg-[var(--ctp-surface0)] hover:text-[var(--ctp-text)]"
                :title="t('playlist.renameTitle')"
                @click="renamePlaylistPrompt(pl)"
              >
                <LuPencil class="h-3.5 w-3.5" />
              </button>
              <button
                type="button"
                class="rounded-lg p-1.5 text-[var(--ctp-subtext0)] hover:bg-[var(--ctp-red)]/10 hover:text-[var(--ctp-red)]"
                :title="t('common.delete')"
                @click="removePlaylist(pl)"
              >
                <LuTrash2 class="h-3.5 w-3.5" />
              </button>
            </div>
          </div>
        </div>
      </div>
    </template>

    <!-- 3. Genres Grid -->
    <template v-else>
      <div
        v-if="filteredGenres.length === 0"
        class="card-soft rounded-2xl bg-[var(--ctp-shell)] py-14 text-center text-xs text-[var(--ctp-subtext0)]"
      >
        {{ t('collections.emptyResult') }}
      </div>

      <div v-else class="grid grid-cols-2 gap-3.5 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5">
        <button
          v-for="g in filteredGenres"
          :key="g.name"
          type="button"
          class="group card-soft flex items-center justify-between gap-2.5 rounded-2xl bg-[var(--ctp-shell)] px-4 py-3.5 text-left transition hover:-translate-y-0.5 hover:border-[var(--ctp-accent)]"
          @click="openGenre(g.name)"
        >
          <span class="flex min-w-0 items-center gap-2.5">
            <span
              class="flex h-8 w-8 shrink-0 items-center justify-center rounded-xl bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]"
            >
              <LuTag class="h-4 w-4" />
            </span>
            <span class="min-w-0">
              <span
                class="block truncate text-xs font-bold text-[var(--ctp-text)] group-hover:text-[var(--ctp-accent)]"
              >
                {{ g.name }}
              </span>
              <span v-if="g.album_count !== null" class="block text-[10px] text-[var(--ctp-subtext0)]">
                {{ t('common.albumsCount', { count: g.album_count }) }}
              </span>
            </span>
          </span>
          <span
            class="shrink-0 rounded-full bg-[var(--ctp-canvas)] px-2 py-0.5 text-[11px] font-semibold text-[var(--ctp-subtext0)]"
          >
            {{ g.song_count }}
          </span>
        </button>
      </div>
    </template>
  </div>
</template>
