<script setup lang="ts">
import { computed } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useI18n as useGlobalI18n } from 'vue-i18n'
import { LuFolderHeart, LuTag } from 'vue-icons-plus/lu'
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
import PosterImage from '../components/PosterImage.vue'
import { useVideoI18n as useI18n } from '../i18n'
import { useVideoStore } from '../store'
import type { VideoLibraryDto } from '../types'

type CollectionKind = 'libraries' | 'shelves' | 'tags' | 'genres'

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

const viewMode = computed<CollectionViewMode>(() => {
  if (kind.value === 'libraries') return 'libraries'
  if (kind.value === 'shelves') return 'collections'
  return 'chips'
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

const libraryItems = computed<CollectionLibraryItem[]>(() =>
  videoStore.libraries.map((lib) => ({
    id: lib.id,
    name: lib.name,
    kindBadge:
      lib.kind.type === 'managed' ? t('admin.managedBadge') : t('admin.localMountBadge'),
    countLabel: t('common.videosCount', { count: lib.video_count }),
    count: lib.video_count,
    rootPath: lib.kind.type === 'local_mount' ? lib.kind.root_path : null,
    scanText: lib.kind.type === 'local_mount' ? formatLibraryScanText(lib) : null,
    isMounted: lib.kind.type === 'local_mount',
    isScanning: lib.scan_status.state === 'scanning',
  })),
)

const shelfMap = computed(() => new Map(videoStore.shelves.map((sh) => [sh.id, sh])))

const shelfItems = computed<CollectionGroupItem[]>(() =>
  videoStore.shelves.map((sh) => ({
    id: sh.id,
    name: sh.name,
    countLabel: t('common.videosCount', { count: sh.video_count }),
    count: sh.video_count,
    canEdit: true,
  })),
)

const chipItems = computed<CollectionChipItem[]>(() => {
  const src = kind.value === 'genres' ? videoStore.genres : videoStore.tags
  return src.map((g) => ({
    key: g.name,
    name: g.name,
    count: g.count,
  }))
})

async function triggerLibraryScan(id: string): Promise<void> {
  await api.scanLibrary(id)
  await videoStore.refreshSidebar()
  videoStore.bumpCatalogVersion()
}

async function createShelfInline(name: string): Promise<void> {
  await api.createShelf({ name })
  await videoStore.refreshSidebar()
}

async function renameShelfPrompt(id: string): Promise<void> {
  const sh = shelfMap.value.get(id)
  if (!sh) return
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

async function removeShelf(id: string): Promise<void> {
  const sh = shelfMap.value.get(id)
  if (!sh) return
  const ok = await dialogStore.confirm({
    title: t('shelf.deleteTitle'),
    message: t('shelf.confirmDelete', { name: sh.name }),
    danger: true,
  })
  if (!ok) return
  await api.deleteShelf(sh.id)
  await videoStore.refreshSidebar()
}
</script>

<template>
  <DomainCollectionsView
    :mode="viewMode"
    :reset-key="kind"
    :title="headerMeta.title"
    :subtitle="headerMeta.subtitle"
    :total="headerMeta.total"
    :search-placeholder="t('collections.searchPlaceholder')"
    :empty-text="t('collections.emptyResult')"
    :see-all-label="t('collections.seeAll')"
    :sort-options="sortOptions"
    :on-refresh="() => videoStore.refreshSidebar()"
    :libraries="libraryItems"
    :show-manage-libraries="authStore.isAdmin"
    :manage-libraries-label="t('collections.manageLibraries')"
    :rescan-label="t('admin.rescanBtn')"
    :show-rescan="authStore.isAdmin"
    :on-scan-library="triggerLibraryScan"
    :collections="shelfItems"
    :collection-icon="LuFolderHeart"
    :create-placeholder="t('nav.shelfNamePlaceholder')"
    :create-button-label="t('nav.newShelf')"
    :rename-title="t('shelf.renameTitle')"
    :delete-title="t('common.delete')"
    :on-create-collection="createShelfInline"
    :chips="chipItems"
    :chip-icon="LuTag"
    chip-icon-tone="secondary"
    @open-library="(id) => router.push({ path: '/video/catalog', query: { library: id } })"
    @open-collection="(id) => router.push(`/video/shelves/${id}`)"
    @rename-collection="renameShelfPrompt"
    @delete-collection="removeShelf"
    @open-chip="
      (name) =>
        router.push({
          path: '/video/catalog',
          query: kind === 'genres' ? { genre: name } : { tag: name },
        })
    "
  >
    <template #collection-leading="{ item }">
      <PosterImage
        v-if="shelfMap.get(item.id)?.poster_key"
        :poster-key="shelfMap.get(item.id)?.poster_key"
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
    </template>
  </DomainCollectionsView>
</template>
