<script setup lang="ts">
import { computed } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { LuFolder, LuTag, LuUser } from 'vue-icons-plus/lu'
import DomainCollectionsView, {
  type CollectionChipItem,
  type CollectionGroupItem,
  type CollectionLibraryItem,
  type CollectionViewMode,
} from '@/components/domain/DomainCollectionsView.vue'
import type { SelectOption } from '@/components/ui/AppSelect.vue'
import { api } from '@/domains/book/api'
import { useBookI18n as useI18n } from '@/domains/book/i18n'
import { useCatalogStore } from '@/domains/book/store'
import type { LibraryDto } from '@/domains/book/types'
import { useAuthStore } from '@/stores/auth'
import { useDialogStore } from '@/stores/dialog'

type CollectionKind = 'libraries' | 'shelves' | 'tags' | 'authors'

const { t } = useI18n()
const route = useRoute()
const router = useRouter()
const authStore = useAuthStore()
const catalogStore = useCatalogStore()
const dialogStore = useDialogStore()

const kind = computed<CollectionKind>(() => {
  if (route.name === 'book-libraries-overview') return 'libraries'
  if (route.name === 'book-shelves-overview') return 'shelves'
  if (route.name === 'book-authors-overview') return 'authors'
  return 'tags'
})

const viewMode = computed<CollectionViewMode>(() => {
  if (kind.value === 'libraries') return 'libraries'
  if (kind.value === 'shelves') return 'collections'
  return 'chips'
})

const sortOptions = computed<SelectOption[]>(() => {
  const base: SelectOption[] = [
    { value: 'count_desc', label: t('collections.sortCountDesc') },
    { value: 'count_asc', label: t('collections.sortCountAsc') },
    { value: 'name_asc', label: t('collections.sortNameAsc') },
  ]
  if (kind.value === 'libraries' || kind.value === 'shelves') {
    base.push({ value: 'updated_desc', label: t('collections.sortUpdatedDesc') })
  }
  return base
})

const headerMeta = computed(() => {
  if (kind.value === 'libraries') {
    return {
      title: t('collections.librariesTitle'),
      subtitle: t('collections.librariesSubtitle'),
      total: catalogStore.libraries.length,
    }
  }
  if (kind.value === 'shelves') {
    return {
      title: t('collections.shelvesTitle'),
      subtitle: t('collections.shelvesSubtitle'),
      total: catalogStore.shelves.length,
    }
  }
  if (kind.value === 'authors') {
    return {
      title: t('collections.authorsTitle'),
      subtitle: t('collections.authorsSubtitle'),
      total: catalogStore.authors.length,
    }
  }
  return {
    title: t('collections.tagsTitle'),
    subtitle: t('collections.tagsSubtitle'),
    total: catalogStore.tags.length,
  }
})

function formatLibraryScanText(lib: LibraryDto): string {
  const s = lib.scan_status
  if (s.state === 'scanning') {
    if (s.total && s.total > 0) {
      return t('admin.scanningProgress', { processed: s.processed ?? 0, total: s.total })
    }
    return t('admin.scanning')
  }
  if (s.state === 'completed') {
    return t('admin.scanCompleted', { added: s.added, updated: s.updated, missing: s.missing })
  }
  if (s.state === 'failed') {
    return t('admin.scanFailed', { message: s.message })
  }
  return t('admin.scanIdle')
}

const libraryItems = computed<CollectionLibraryItem[]>(() =>
  catalogStore.libraries.map((lib) => ({
    id: lib.id,
    name: lib.name,
    kindBadge: lib.kind.type === 'managed' ? t('catalog.managedBadge') : t('catalog.mountedBadge'),
    countLabel: t('collections.bookCountUnit', { count: lib.book_count }),
    count: lib.book_count,
    updatedAt: lib.updated_at,
    rootPath: lib.kind.type === 'mounted' ? lib.kind.root_path : null,
    scanText: lib.kind.type === 'mounted' ? formatLibraryScanText(lib) : null,
    isMounted: lib.kind.type === 'mounted',
    isScanning: lib.scan_status.state === 'scanning',
  })),
)

const shelfItems = computed<CollectionGroupItem[]>(() =>
  catalogStore.shelves.map((shelf) => ({
    id: shelf.id,
    name: shelf.name,
    countLabel: t('collections.bookCountUnit', { count: shelf.book_count }),
    count: shelf.book_count,
    updatedAt: shelf.updated_at,
    badge: shelf.is_favorite ? t('collections.defaultFavoriteBadge') : null,
    canEdit: !shelf.is_favorite,
  })),
)

const chipItems = computed<CollectionChipItem[]>(() => {
  const src = kind.value === 'authors' ? catalogStore.authors : catalogStore.tags
  return src.map((item) => ({
    key: item.name,
    name: item.name,
    count: item.count,
  }))
})

async function openLibrary(id: string): Promise<void> {
  catalogStore.selectSection('library', id)
  await router.push('/book/catalog')
}

async function openShelf(id: string): Promise<void> {
  catalogStore.selectSection('shelf', id)
  await router.push('/book/catalog')
}

async function openChip(name: string): Promise<void> {
  catalogStore.selectSection(kind.value === 'authors' ? 'author' : 'tag', name)
  await router.push('/book/catalog')
}

async function triggerLibraryScan(id: string): Promise<void> {
  const started = await api.scanLibrary(id)
  catalogStore.applyLibraryUpdate(started)
}

async function createShelfInline(name: string): Promise<void> {
  await api.createShelf(name)
  await catalogStore.refreshSidebarData()
}

async function renameShelfPrompt(id: string): Promise<void> {
  const shelf = catalogStore.shelves.find((s) => s.id === id)
  if (!shelf) return
  const nextName = await dialogStore.prompt({
    title: t('collections.renameShelf'),
    message: t('collections.renameShelfPrompt'),
    defaultValue: shelf.name,
  })
  if (!nextName || !nextName.trim() || nextName.trim() === shelf.name) return
  await api.renameShelf(shelf.id, nextName.trim())
  await catalogStore.refreshSidebarData()
}

async function removeShelf(id: string): Promise<void> {
  const shelf = catalogStore.shelves.find((s) => s.id === id)
  if (!shelf) return
  const ok = await dialogStore.confirm({
    title: shelf.name,
    message: `${t('common.delete')} "${shelf.name}"?`,
    danger: true,
  })
  if (!ok) return
  await api.deleteShelf(shelf.id)
  await catalogStore.refreshSidebarData()
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
    :see-all-label="t('home.seeAll')"
    :sort-options="sortOptions"
    :on-refresh="() => catalogStore.refreshSidebarData()"
    :libraries="libraryItems"
    :show-manage-libraries="authStore.isAdmin"
    :manage-libraries-label="t('collections.manageLibraries')"
    :rescan-label="t('admin.rescan')"
    :show-rescan="authStore.isAdmin"
    :on-scan-library="triggerLibraryScan"
    :collections="shelfItems"
    :collection-icon="LuFolder"
    :create-placeholder="t('nav.shelfNamePlaceholder')"
    :create-button-label="t('nav.newShelf')"
    :rename-title="t('collections.renameShelf')"
    :delete-title="t('common.delete')"
    :on-create-collection="createShelfInline"
    :chips="chipItems"
    :chip-icon="kind === 'authors' ? LuUser : LuTag"
    @open-library="openLibrary"
    @open-collection="openShelf"
    @rename-collection="renameShelfPrompt"
    @delete-collection="removeShelf"
    @open-chip="openChip"
  />
</template>
