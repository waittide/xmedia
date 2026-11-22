import { computed, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useI18n } from 'vue-i18n'
import { useEscapeStack } from '@/composables/useEscapeStack'
import { useAuthStore } from '@/stores/auth'
import { useDialogStore } from '@/stores/dialog'
import { useDomainStore } from '@/stores/domain'
import { useSnackbarStore } from '@/stores/snackbar'
import { usePreferencesStore } from '@/stores/preferences'
import { useBookI18n } from '@/domains/book/i18n'
import { useCatalogStore } from '@/domains/book/store'
import { api as bookApi } from '@/domains/book/api'
import type { BookBookmarkItemDto } from '@/domains/book/types'
import { useComicI18n } from '@/domains/comic/i18n'
import { useSidebarStore as useComicSidebarStore } from '@/domains/comic/store'
import { api as comicApi } from '@/domains/comic/api'
import { useMusicI18n } from '@/domains/music/i18n'
import { useMusicStore } from '@/domains/music/store'
import { api as musicApi } from '@/domains/music/api'
import type { PlaylistSummaryDto } from '@/domains/music/types'
import { useVideoI18n } from '@/domains/video/i18n'
import { useVideoStore } from '@/domains/video/store'
import { api as videoApi } from '@/domains/video/api'
import type { VideoShelfDto } from '@/domains/video/types'
import type { MediaKind } from '@/types/api'
import {
  buildBookSidebarGroup,
  buildComicSidebarGroup,
} from './sidebar/buildBookComicGroups'
import {
  buildMusicSidebarGroup,
  buildVideoSidebarGroup,
} from './sidebar/buildMusicVideoGroups'
import {
  FOLDED_STORAGE_KEY,
  SIDEBAR_PREVIEW_LIMIT,
  type PrimaryNavItem,
  type SidebarGroupDescriptor,
  type SidebarSubItem,
} from './sidebar/types'
import { usePrimaryNavItems } from './sidebar/usePrimaryNavItems'

export { usePrimaryNavItems, type PrimaryNavItem }

export function useWorkspaceSidebar(onCloseMobile: () => void) {
  const route = useRoute()
  const router = useRouter()
  const { t } = useI18n()
  const { t: bookT } = useBookI18n()
  const { t: comicT } = useComicI18n()
  const { t: musicT } = useMusicI18n()
  const { t: videoT } = useVideoI18n()

  const authStore = useAuthStore()
  const dialogStore = useDialogStore()
  const domainStore = useDomainStore()
  const snackbarStore = useSnackbarStore()
  const prefsStore = usePreferencesStore()
  const escapeStack = useEscapeStack()

  const bookStore = useCatalogStore()
  const comicSidebarStore = useComicSidebarStore()
  const musicStore = useMusicStore()
  const videoStore = useVideoStore()

  const creatingGroupId = ref<string | null>(null)
  const newCollectionName = ref('')
  const creatingCollection = ref(false)

  function cancelCreateCollection(): void {
    if (creatingCollection.value) return
    creatingGroupId.value = null
    newCollectionName.value = ''
  }

  escapeStack.register(
    () => creatingGroupId.value !== null,
    () => cancelCreateCollection(),
  )

  watch(
    () => domainStore.activeDomain,
    () => {
      creatingGroupId.value = null
      newCollectionName.value = ''
    },
  )

  function loadInitialFolded(): Record<string, boolean> {
    try {
      const raw = localStorage.getItem(FOLDED_STORAGE_KEY)
      return raw ? (JSON.parse(raw) as Record<string, boolean>) : {}
    } catch {
      return {}
    }
  }

  const foldedMap = ref<Record<string, boolean>>(loadInitialFolded())
  const expandedMap = ref<Record<string, boolean>>({})

  function isGroupFolded(id: string): boolean {
    return Boolean(foldedMap.value[id])
  }

  function toggleGroupFold(id: string): void {
    const next = !foldedMap.value[id]
    foldedMap.value = { ...foldedMap.value, [id]: next }
    localStorage.setItem(FOLDED_STORAGE_KEY, JSON.stringify(foldedMap.value))
  }

  function isGroupExpanded(id: string): boolean {
    return Boolean(expandedMap.value[id])
  }

  function toggleGroupExpanded(id: string): void {
    expandedMap.value = { ...expandedMap.value, [id]: !expandedMap.value[id] }
  }

  const {
    primaryNavItems,
    isBookSectionActive,
    openBookSection,
    openMusicCatalog,
    openVideoCatalog,
  } = usePrimaryNavItems(onCloseMobile)

  function openBookBookmark(item: BookBookmarkItemDto): void {
    onCloseMobile()
    const loc = item.bookmark.location
    const query: Record<string, string> = { locator: loc.locator }
    if (loc.chapter_index !== null) query.chapter = String(loc.chapter_index)
    if (loc.page_number !== null) query.page = String(loc.page_number)
    if (loc.scroll_ratio !== null) query.ratio = String(loc.scroll_ratio)
    void router.push({ path: `/book/read/${item.bookmark.book_id}`, query })
  }

  const activeComicLibraryId = computed(() =>
    route.name === 'comic-series-list' && typeof route.query.library_id === 'string'
      ? route.query.library_id
      : '',
  )
  const activeComicShelfId = computed(() =>
    route.name === 'comic-series-list' && typeof route.query.shelf_id === 'string'
      ? route.query.shelf_id
      : '',
  )
  const activeComicTag = computed(() =>
    route.name === 'comic-series-list' && typeof route.query.tag === 'string'
      ? route.query.tag
      : '',
  )
  const activeComicGenre = computed(() =>
    route.name === 'comic-series-list' && typeof route.query.genre === 'string'
      ? route.query.genre
      : '',
  )
  const activeComicWriter = computed(() =>
    route.name === 'comic-series-list' && typeof route.query.writer === 'string'
      ? route.query.writer
      : '',
  )
  const activeComicPenciller = computed(() =>
    route.name === 'comic-series-list' && typeof route.query.penciller === 'string'
      ? route.query.penciller
      : '',
  )

  function canEditPlaylist(pl: PlaylistSummaryDto): boolean {
    return pl.owner_id === authStore.user?.id || authStore.isAdmin
  }

  function showError(err: unknown): void {
    snackbarStore.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  }

  async function handleCreateCollection(): Promise<void> {
    const name = newCollectionName.value.trim()
    if (!name || creatingCollection.value) return
    const d = domainStore.activeDomain
    creatingCollection.value = true

    try {
      if (d === 'book') {
        const created = await bookApi.createShelf(name)
        await bookStore.refreshSidebarData()
        newCollectionName.value = ''
        creatingGroupId.value = null
        await openBookSection('shelf', created.id)
      } else if (d === 'comic') {
        const created = await comicApi.createShelf({ name })
        await comicSidebarStore.refresh()
        newCollectionName.value = ''
        creatingGroupId.value = null
        onCloseMobile()
        await router.push({ path: '/comic/series', query: { shelf_id: created.id } })
      } else if (d === 'music') {
        const created = await musicApi.createPlaylist({ name })
        await musicStore.refreshSidebar()
        newCollectionName.value = ''
        creatingGroupId.value = null
        onCloseMobile()
        await router.push(`/music/playlists/${created.playlist.id}`)
      } else {
        const created = await videoApi.createShelf({ name })
        await videoStore.refreshSidebar()
        newCollectionName.value = ''
        creatingGroupId.value = null
        onCloseMobile()
        await router.push(`/video/shelves/${created.shelf.id}`)
      }
    } catch (err) {
      showError(err)
    } finally {
      creatingCollection.value = false
    }
  }

  async function handleDeleteBookShelf(id: string, name: string): Promise<void> {
    const ok = await dialogStore.confirm({ title: t('common.delete'), message: name, danger: true })
    if (!ok) return
    try {
      await bookApi.deleteShelf(id)
      await bookStore.refreshSidebarData()
      if (bookStore.activeSection === 'shelf' && bookStore.activeShelfId === id) {
        await openBookSection('all')
      }
    } catch (err) {
      showError(err)
    }
  }

  async function handleDeleteBookBookmark(bm: BookBookmarkItemDto): Promise<void> {
    const ok = await dialogStore.confirm({
      title: t('common.delete'),
      message: `${bm.book_title} · ${bm.bookmark.title}`,
      danger: true,
    })
    if (!ok) return
    try {
      await bookApi.deleteBookmark(bm.bookmark.id)
      await bookStore.refreshSidebarData()
    } catch (err) {
      showError(err)
    }
  }

  async function handleDeleteComicShelf(id: string, name: string): Promise<void> {
    const ok = await dialogStore.confirm({
      title: comicT('shelves.deleteShelf'),
      message: name,
      danger: true,
    })
    if (!ok) return
    try {
      await comicApi.deleteShelf(id)
      await comicSidebarStore.refresh()
      if (activeComicShelfId.value === id) {
        await router.push('/comic/series')
      }
    } catch (err) {
      showError(err)
    }
  }

  async function handleDeleteComicBookmark(id: string, label: string): Promise<void> {
    const ok = await dialogStore.confirm({ title: t('common.delete'), message: label, danger: true })
    if (!ok) return
    try {
      await comicApi.deleteBookmark(id)
      await comicSidebarStore.refresh()
    } catch (err) {
      showError(err)
    }
  }

  async function handleDeleteMusicPlaylist(pl: PlaylistSummaryDto): Promise<void> {
    const ok = await dialogStore.confirm({
      title: musicT('playlist.deleteTitle'),
      message: musicT('playlist.confirmDelete', { name: pl.name }),
      danger: true,
    })
    if (!ok) return
    try {
      await musicApi.deletePlaylist(pl.id)
      await musicStore.refreshSidebar()
      if (route.path === `/music/playlists/${pl.id}`) {
        await router.push('/music/songs')
      }
    } catch (err) {
      showError(err)
    }
  }

  async function handleDeleteVideoShelf(sh: VideoShelfDto): Promise<void> {
    const ok = await dialogStore.confirm({
      title: videoT('shelf.deleteTitle'),
      message: videoT('shelf.confirmDelete', { name: sh.name }),
      danger: true,
    })
    if (!ok) return
    try {
      await videoApi.deleteShelf(sh.id)
      await videoStore.refreshSidebar()
      if (route.path === `/video/shelves/${sh.id}`) {
        await router.push('/video/catalog')
      }
    } catch (err) {
      showError(err)
    }
  }

  function visibleGroupItems(group: SidebarGroupDescriptor): SidebarSubItem[] {
    const all = group.items
    if (isGroupExpanded(group.id) || all.length <= SIDEBAR_PREVIEW_LIMIT) return all
    const head = all.slice(0, SIDEBAR_PREVIEW_LIMIT)
    if (!head.some((it) => it.active)) {
      const activeItem = all.find((it) => it.active)
      if (activeItem) return [...head.slice(0, SIDEBAR_PREVIEW_LIMIT - 1), activeItem]
    }
    return head
  }

  function buildDomainSidebarGroup(domain: MediaKind, key: string): SidebarGroupDescriptor | null {
    const id = `${domain}:${key}`
    const label = t(`layoutCustomizer.sidebar.${domain}.${key}`)
    const go = (target: string | { path: string; query?: Record<string, string> }) => {
      onCloseMobile()
      void router.push(target)
    }

    if (domain === 'book') {
      return buildBookSidebarGroup(id, key, label, {
        route,
        bookStore,
        bookT,
        go,
        isBookSectionActive,
        openBookSection,
        openBookBookmark,
        onDeleteShelf: handleDeleteBookShelf,
        onDeleteBookmark: handleDeleteBookBookmark,
      })
    }
    if (domain === 'comic') {
      return buildComicSidebarGroup(id, key, label, {
        route,
        comicSidebarStore,
        comicT,
        go,
        activeComicLibraryId: activeComicLibraryId.value,
        activeComicShelfId: activeComicShelfId.value,
        activeComicTag: activeComicTag.value,
        activeComicGenre: activeComicGenre.value,
        activeComicWriter: activeComicWriter.value,
        activeComicPenciller: activeComicPenciller.value,
        onDeleteShelf: handleDeleteComicShelf,
        onDeleteBookmark: handleDeleteComicBookmark,
      })
    }
    if (domain === 'music') {
      return buildMusicSidebarGroup(id, key, label, {
        route,
        musicStore,
        musicT,
        go,
        openMusicCatalog,
        canEditPlaylist,
        onDeletePlaylist: handleDeleteMusicPlaylist,
      })
    }
    return buildVideoSidebarGroup(id, key, label, {
      route,
      videoStore,
      videoT,
      go,
      openVideoCatalog,
      onDeleteShelf: handleDeleteVideoShelf,
    })
  }

  const activeSidebarGroups = computed<SidebarGroupDescriptor[]>(() => {
    const dom = domainStore.activeDomain
    const configuredKeys = prefsStore.workspaceLayout[dom].sidebar_groups
    return configuredKeys
      .map((key) => buildDomainSidebarGroup(dom, key))
      .filter(
        (g): g is SidebarGroupDescriptor =>
          g !== null &&
          (g.items.length > 0 || g.overviewActive || creatingGroupId.value === g.id),
      )
  })

  function toggleCreateForGroup(group: SidebarGroupDescriptor): void {
    if (isGroupFolded(group.id)) {
      toggleGroupFold(group.id)
    }
    creatingGroupId.value = creatingGroupId.value === group.id ? null : group.id
    newCollectionName.value = ''
  }

  return {
    t,
    domainStore,
    primaryNavItems,
    activeSidebarGroups,
    creatingGroupId,
    newCollectionName,
    creatingCollection,
    SIDEBAR_PREVIEW_LIMIT,
    isGroupFolded,
    toggleGroupFold,
    isGroupExpanded,
    toggleGroupExpanded,
    visibleGroupItems,
    toggleCreateForGroup,
    cancelCreateCollection,
    handleCreateCollection,
  }
}
