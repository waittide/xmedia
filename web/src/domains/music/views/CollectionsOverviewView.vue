<script setup lang="ts">
import { computed } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useI18n as useGlobalI18n } from 'vue-i18n'
import { LuListMusic, LuTag } from 'vue-icons-plus/lu'
import DomainCollectionsView, {
  type CollectionChipItem,
  type CollectionGroupItem,
  type CollectionLibraryItem,
  type CollectionViewMode,
} from '@/components/domain/DomainCollectionsView.vue'
import type { SelectOption } from '@/components/ui/AppSelect.vue'
import { useAuthStore } from '@/stores/auth'
import { useDialogStore } from '@/stores/dialog'
import { api } from '../api'
import CoverImage from '../components/CoverImage.vue'
import { useMusicI18n as useI18n } from '../i18n'
import { useMusicStore } from '../store'
import type { LibraryDto, PlaylistSummaryDto } from '../types'

type CollectionKind = 'libraries' | 'playlists' | 'genres' | 'tags'

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

const viewMode = computed<CollectionViewMode>(() => {
  if (kind.value === 'libraries') return 'libraries'
  if (kind.value === 'playlists') return 'collections'
  return 'chips'
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

function canEditPlaylist(pl: PlaylistSummaryDto): boolean {
  return pl.owner_id === authStore.user?.id || authStore.isAdmin
}

function formatLibraryScanText(lib: LibraryDto): string {
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
  if (s.state === 'failed') {
    return t('admin.scanFailed', { message: s.message })
  }
  return t('admin.scanIdle')
}

const libraryItems = computed<CollectionLibraryItem[]>(() =>
  musicStore.libraries.map((lib) => ({
    id: lib.id,
    name: lib.name,
    kindBadge:
      lib.kind.type === 'managed' ? t('admin.managedBadge') : t('admin.localMountBadge'),
    countLabel: t('common.tracksCount', { count: lib.track_count }),
    count: lib.track_count,
    updatedAt: lib.updated_at,
    rootPath: lib.kind.type === 'local_mount' ? lib.kind.root_path : null,
    scanText: lib.kind.type === 'local_mount' ? formatLibraryScanText(lib) : null,
    isMounted: lib.kind.type === 'local_mount',
    isScanning: lib.scan_status.state === 'scanning',
  })),
)

const playlistMap = computed(() => new Map(musicStore.playlists.map((pl) => [pl.id, pl])))

const playlistItems = computed<CollectionGroupItem[]>(() =>
  musicStore.playlists.map((pl) => ({
    id: pl.id,
    name: pl.name,
    countLabel: t('common.tracksCount', { count: pl.track_count }),
    count: pl.track_count,
    updatedAt: pl.updated_at,
    canEdit: canEditPlaylist(pl),
  })),
)

const chipItems = computed<CollectionChipItem[]>(() =>
  kind.value === 'tags'
    ? musicStore.tags.map((tg) => ({
        key: tg.name,
        name: tg.name,
        count: tg.song_count,
      }))
    : musicStore.genres.map((g) => ({
        key: g.name,
        name: g.name,
        count: g.song_count,
        sublabel: t('common.albumsCount', { count: g.album_count }),
      })),
)

async function openLibrary(id: string): Promise<void> {
  await router.push({ path: '/music/songs', query: { library: id } })
}

async function openPlaylist(id: string): Promise<void> {
  await router.push(`/music/playlists/${id}`)
}

async function openChip(name: string): Promise<void> {
  await router.push({
    path: '/music/songs',
    query: kind.value === 'tags' ? { tag: name } : { genre: name },
  })
}

async function triggerLibraryScan(id: string): Promise<void> {
  await api.scanLibrary(id)
  await musicStore.refreshSidebar()
  musicStore.bumpCatalogVersion()
}

async function createPlaylistInline(name: string): Promise<void> {
  await api.createPlaylist({ name })
  await musicStore.refreshSidebar()
}

async function renamePlaylistPrompt(id: string): Promise<void> {
  const pl = playlistMap.value.get(id)
  if (!pl) return
  const nextName = await dialogStore.prompt({
    title: t('playlist.renameTitle'),
    message: t('playlist.renamePrompt'),
    initialValue: pl.name,
  })
  if (!nextName || !nextName.trim() || nextName.trim() === pl.name) return
  await api.updatePlaylist(pl.id, { name: nextName.trim() })
  await musicStore.refreshSidebar()
}

async function removePlaylist(id: string): Promise<void> {
  const pl = playlistMap.value.get(id)
  if (!pl) return
  const ok = await dialogStore.confirm({
    title: t('playlist.deleteTitle'),
    message: t('playlist.confirmDelete', { name: pl.name }),
    danger: true,
  })
  if (!ok) return
  await api.deletePlaylist(pl.id)
  await musicStore.refreshSidebar()
}
</script>

<template>
  <DomainCollectionsView
    max-width-class="max-w-6xl"
    :mode="viewMode"
    :reset-key="kind"
    :title="headerMeta.title"
    :subtitle="headerMeta.subtitle"
    :total="headerMeta.total"
    :search-placeholder="t('collections.searchPlaceholder')"
    :empty-text="t('collections.emptyResult')"
    :see-all-label="t('collections.seeAll')"
    :sort-options="sortOptions"
    :on-refresh="() => musicStore.refreshSidebar()"
    :libraries="libraryItems"
    :show-manage-libraries="authStore.isAdmin"
    :manage-libraries-label="t('collections.manageLibraries')"
    :rescan-label="t('admin.rescanBtn')"
    :show-rescan="authStore.isAdmin"
    :on-scan-library="triggerLibraryScan"
    :collections="playlistItems"
    :collection-icon="LuListMusic"
    :create-placeholder="t('nav.playlistNamePlaceholder')"
    :create-button-label="t('nav.newPlaylist')"
    :rename-title="t('playlist.renameTitle')"
    :delete-title="t('common.delete')"
    :on-create-collection="createPlaylistInline"
    :chips="chipItems"
    :chip-icon="LuTag"
    @open-library="openLibrary"
    @open-collection="openPlaylist"
    @rename-collection="renamePlaylistPrompt"
    @delete-collection="removePlaylist"
    @open-chip="openChip"
  >
    <template #collection-leading="{ item }">
      <CoverImage
        v-if="playlistMap.get(item.id)?.cover_key"
        :cover-id="item.id"
        :has-cover="true"
        :size="120"
        rounded="rounded-xl"
        class="h-11 w-11 shrink-0"
      />
      <div
        v-else
        class="flex h-11 w-11 shrink-0 items-center justify-center rounded-xl bg-[var(--md-sys-color-primary-container)] text-[var(--md-sys-color-on-primary-container)]"
      >
        <LuListMusic class="h-5 w-5" />
      </div>
    </template>
  </DomainCollectionsView>
</template>
