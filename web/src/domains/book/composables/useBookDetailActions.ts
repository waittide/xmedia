import { computed, onMounted, ref, watch } from 'vue'
import { useI18n as useGlobalI18n } from 'vue-i18n'
import { useRoute, useRouter } from 'vue-router'
import { api } from '@/domains/book/api'
import type { OnlineDownloadTarget } from '@/domains/book/components/OnlineDownloadModal.vue'
import { useBookI18n as useI18n } from '@/domains/book/i18n'
import { onlineBookApi, type AggregatedSearchItemDto } from '@/domains/book/onlineApi'
import { useOnlineDownloadsStore } from '@/domains/book/onlineDownloads'
import { useCatalogStore } from '@/domains/book/store'
import type { Bookmark, BookSummaryDto, TocItem } from '@/domains/book/types'
import { libraryRoute } from '@/sources'
import { useDialogStore } from '@/stores/dialog'
import { useSnackbarStore } from '@/stores/snackbar'

export function useBookDetailActions() {
  const { t } = useI18n()
  const { t: globalT } = useGlobalI18n()
  const route = useRoute()
  const router = useRouter()
  const catalogStore = useCatalogStore()
  const onlineDownloadsStore = useOnlineDownloadsStore()
  const snackbarStore = useSnackbarStore()
  const dialogStore = useDialogStore()

  const book = ref<BookSummaryDto | null>(null)
  const toc = ref<TocItem[]>([])
  const bookmarks = ref<Bookmark[]>([])
  const loading = ref(true)
  const loadError = ref<string | null>(null)
  const trackedSourceName = ref('')

  const refreshingToc = ref(false)
  const syncingEpub = ref(false)
  const togglingFavorite = ref(false)
  const togglingShelfIds = ref<Set<string>>(new Set())
  const deletingBook = ref(false)
  const unbindingBook = ref(false)
  const deletingBookmarkIds = ref<Set<string>>(new Set())

  const editModalOpen = ref(false)
  const switchSourceOpen = ref(false)
  const switchingSourceKey = ref<string | null>(null)
  const onlineDownloadTarget = ref<OnlineDownloadTarget | null>(null)
  const onlineDownloadJobId = ref<string | null>(null)

  let detailSeq = 0
  let refreshTocSeq = 0
  let syncEpubSeq = 0

  const trackedDownloadJob = computed(() =>
    onlineDownloadJobId.value
      ? (onlineDownloadsStore.jobs.find((j) => j.id === onlineDownloadJobId.value) ?? null)
      : null,
  )

  async function loadDetail(id: string): Promise<void> {
    const seq = ++detailSeq
    refreshTocSeq += 1
    syncEpubSeq += 1
    loading.value = true
    loadError.value = null
    trackedSourceName.value = ''
    refreshingToc.value = false
    syncingEpub.value = false
    try {
      const [b, bms] = await Promise.all([
        api.getBook(id),
        api.listBookmarks(id).catch(() => []),
      ])
      if (seq !== detailSeq) return
      book.value = b
      bookmarks.value = bms
      catalogStore.activeBookId = b.id

      if (catalogStore.shelves.length === 0) {
        void catalogStore.refreshSidebarData().catch(() => {})
      }

      if (b.online_tracking) {
        const trackedToc = await onlineBookApi.getTrackedToc(id, false).catch(() => null)
        if (seq !== detailSeq) return
        if (trackedToc) {
          trackedSourceName.value = trackedToc.source_name
          toc.value = trackedToc.chapters.map((ch) => ({
            index: ch.index,
            title: ch.title,
            href: `chapter:${ch.index}`,
            level: 0,
          }))
        } else {
          toc.value = []
        }
      } else {
        const loadedToc = await api.getToc(id).catch(() => [])
        if (seq !== detailSeq) return
        toc.value = loadedToc
      }
    } catch (err) {
      if (seq !== detailSeq) return
      book.value = null
      loadError.value = err instanceof Error ? err.message : t('detail.notFound')
    } finally {
      if (seq === detailSeq) {
        loading.value = false
      }
    }
  }

  onMounted(() => {
    void loadDetail(String(route.params.id))
  })

  watch(
    () => route.params.id,
    (nextId) => {
      if (nextId) {
        void loadDetail(String(nextId))
      }
    },
  )

  function goBack(): void {
    const prev = (window.history.state as { back?: string | null } | null)?.back
    if (
      typeof prev === 'string' &&
      !prev.startsWith('/book/play/') &&
      !prev.startsWith('/book/read/')
    ) {
      router.back()
    } else {
      void router.push(libraryRoute('book'))
    }
  }

  function triggerFileDownload(): void {
    if (!book.value || book.value.is_missing) return
    const link = document.createElement('a')
    link.href = api.fileUrl(book.value.id, true)
    link.rel = 'noopener'
    document.body.appendChild(link)
    link.click()
    document.body.removeChild(link)
  }

  async function toggleFavorite(): Promise<void> {
    if (!book.value || togglingFavorite.value) return
    const targetId = book.value.id
    const next = !book.value.is_favorite
    togglingFavorite.value = true
    try {
      await api.setFavorite(targetId, next)
      if (book.value?.id === targetId) {
        book.value = { ...book.value, is_favorite: next }
        catalogStore.updateBookInList(book.value)
      }
    } catch (err) {
      snackbarStore.show({
        message: err instanceof Error ? err.message : String(err),
        tone: 'error',
      })
    } finally {
      togglingFavorite.value = false
    }
  }

  async function toggleShelf(shelfId: string, inShelf: boolean): Promise<void> {
    if (!book.value || togglingShelfIds.value.has(shelfId)) return
    const targetId = book.value.id
    const nextSet = new Set(togglingShelfIds.value)
    nextSet.add(shelfId)
    togglingShelfIds.value = nextSet
    try {
      if (inShelf) {
        await api.removeBookFromShelf(shelfId, targetId)
      } else {
        await api.addBookToShelf(shelfId, targetId)
      }
      await catalogStore.refreshSidebarData()
    } catch (err) {
      snackbarStore.show({
        message: err instanceof Error ? err.message : String(err),
        tone: 'error',
      })
    } finally {
      const done = new Set(togglingShelfIds.value)
      done.delete(shelfId)
      togglingShelfIds.value = done
    }
  }

  function handleMetadataSaved(updated: BookSummaryDto): void {
    book.value = updated
    catalogStore.updateBookInList(updated)
    void catalogStore.refreshSidebarData()
    snackbarStore.show(t('detail.savedSuccess'))
  }

  async function onCoverChange(ev: Event): Promise<void> {
    if (!book.value) return
    const input = ev.target as HTMLInputElement
    const file = input.files?.[0]
    if (!file) return
    const targetId = book.value.id
    try {
      const updated = await api.replaceCover(targetId, file)
      if (book.value?.id === targetId) {
        book.value = updated
        catalogStore.updateBookInList(updated)
        snackbarStore.show(t('detail.coverUpdated'))
      }
    } catch (err) {
      snackbarStore.show({
        message: err instanceof Error ? err.message : String(err),
        tone: 'error',
      })
    } finally {
      input.value = ''
    }
  }

  async function handleRefreshTrackedToc(): Promise<void> {
    const targetBook = book.value
    if (!targetBook?.online_tracking || refreshingToc.value) return
    const targetId = targetBook.id
    const seq = ++refreshTocSeq
    refreshingToc.value = true
    try {
      const res = await onlineBookApi.getTrackedToc(targetId, true)
      if (seq !== refreshTocSeq || book.value?.id !== targetId) return
      trackedSourceName.value = res.source_name
      toc.value = res.chapters.map((ch) => ({
        index: ch.index,
        title: ch.title,
        href: `chapter:${ch.index}`,
        level: 0,
      }))
      const refreshedBook = await api.getBook(targetId)
      if (seq !== refreshTocSeq || book.value?.id !== targetId) return
      book.value = refreshedBook
      snackbarStore.show(t('online.trackedTocRefreshed', { count: res.total_chapters }))
    } catch (err) {
      if (seq !== refreshTocSeq || book.value?.id !== targetId) return
      snackbarStore.show({
        message: err instanceof Error ? err.message : String(err),
        tone: 'error',
      })
    } finally {
      if (seq === refreshTocSeq) {
        refreshingToc.value = false
      }
    }
  }

  async function handleSyncTrackedEpub(): Promise<void> {
    const targetBook = book.value
    if (!targetBook?.online_tracking || syncingEpub.value) return
    const targetId = targetBook.id
    const seq = ++syncEpubSeq
    syncingEpub.value = true
    try {
      const synced = await onlineBookApi.syncTrackedEpub(targetId)
      if (seq !== syncEpubSeq || book.value?.id !== targetId) return
      book.value = synced
      snackbarStore.show(t('online.trackedEpubSynced'))
    } catch (err) {
      if (seq !== syncEpubSeq || book.value?.id !== targetId) return
      snackbarStore.show({
        message: err instanceof Error ? err.message : String(err),
        tone: 'error',
      })
    } finally {
      if (seq === syncEpubSeq) {
        syncingEpub.value = false
      }
    }
  }

  async function handleSwitchTrackedSource(
    item: AggregatedSearchItemDto,
    clearCache: boolean,
  ): Promise<void> {
    const targetBook = book.value
    if (!targetBook?.online_tracking || switchingSourceKey.value !== null) return
    const targetId = targetBook.id
    const key = `${item.source_id}:${item.book.book_url}`
    switchingSourceKey.value = key
    try {
      const detailRes = await onlineBookApi.fetchBookInfo(item.source_id, item.book.book_url, {
        name: item.book.name,
        author: item.book.author,
        book_url: item.book.book_url,
        toc_url: '',
        cover_url: item.book.cover_url,
        intro: item.book.intro,
      })
      const tocUrl = detailRes.detail.toc_url?.trim() || ''
      if (!tocUrl) {
        throw new Error(t('online.switchSourceNoToc'))
      }
      const res = await onlineBookApi.switchTrackedSource(targetId, {
        source_id: item.source_id,
        book_url: item.book.book_url,
        toc_url: tocUrl,
        cover_url: detailRes.detail.cover_url ?? item.book.cover_url,
        intro: detailRes.detail.intro ?? item.book.intro,
        last_chapter: detailRes.detail.last_chapter ?? item.book.last_chapter,
        clear_cache: clearCache,
      })
      if (book.value?.id !== targetId) return
      trackedSourceName.value = res.source_name || item.source_name
      toc.value = res.chapters.map((ch) => ({
        index: ch.index,
        title: ch.title,
        href: `chapter:${ch.index}`,
        level: 0,
      }))
      const refreshed = await api.getBook(targetId)
      if (book.value?.id === targetId) {
        book.value = refreshed
        catalogStore.updateBookInList(refreshed)
      }
      switchSourceOpen.value = false
      snackbarStore.show(
        t('online.trackedSwitchSuccess', {
          source: res.source_name || item.source_name,
          count: res.total_chapters,
        }),
      )
    } catch (err) {
      snackbarStore.show({
        message: err instanceof Error ? err.message : String(err),
        tone: 'error',
      })
    } finally {
      switchingSourceKey.value = null
    }
  }

  function openTrackedBatchDownload(): void {
    const b = book.value
    const tr = b?.online_tracking
    if (!b || !tr) return
    onlineDownloadTarget.value = {
      bookId: b.id,
      sourceId: tr.source_id,
      sourceName: trackedSourceName.value || t('online.sourceLabel'),
      bookName: b.metadata.title,
      author: b.metadata.authors.join(', '),
      bookUrl: tr.book_url,
      tocUrl: tr.toc_url,
      intro: b.metadata.description,
      totalChapters: tr.total_chapters || undefined,
    }
  }

  function closeTrackedDownloadModal(): void {
    onlineDownloadTarget.value = null
    onlineDownloadJobId.value = null
    if (book.value) {
      void loadDetail(book.value.id)
    }
  }

  async function handleUnbindBook(): Promise<void> {
    if (!book.value || unbindingBook.value) return
    const targetId = book.value.id
    const targetTitle = book.value.metadata.title
    const isOnline = Boolean(book.value.online_tracking)
    const confirmed = await dialogStore.confirm({
      title: globalT('pool.unbindConfirmTitle'),
      message: isOnline
        ? globalT('pool.unbindOnlineConfirmMessage', { title: targetTitle })
        : globalT('pool.unbindConfirmMessage', { title: targetTitle }),
      danger: true,
      confirmText: globalT('pool.unbindBtn'),
    })
    if (!confirmed) return
    unbindingBook.value = true
    try {
      await api.unbindItem(targetId)
      snackbarStore.show(globalT('pool.unboundToast'))
      await Promise.all([catalogStore.refreshSidebarData(), catalogStore.fetchBooks()])
      await router.replace(libraryRoute('book'))
    } catch (err) {
      snackbarStore.show({
        message: err instanceof Error ? err.message : String(err),
        tone: 'error',
      })
    } finally {
      unbindingBook.value = false
    }
  }

  async function handleDeleteBook(): Promise<void> {
    if (!book.value || deletingBook.value) return
    const targetId = book.value.id
    const targetTitle = book.value.metadata.title
    const confirmed = await dialogStore.confirm({
      title: t('detail.deleteBook'),
      message: t('detail.confirmDeleteBook', { title: targetTitle }),
      danger: true,
    })
    if (!confirmed) return
    deletingBook.value = true
    try {
      await api.deleteBook(targetId)
      await Promise.all([catalogStore.refreshSidebarData(), catalogStore.fetchBooks()])
      await router.replace(libraryRoute('book'))
    } catch (err) {
      snackbarStore.show({
        message: err instanceof Error ? err.message : String(err),
        tone: 'error',
      })
    } finally {
      deletingBook.value = false
    }
  }

  async function deleteBookmark(bm: Bookmark): Promise<void> {
    if (deletingBookmarkIds.value.has(bm.id) || !book.value) return
    const targetBookId = book.value.id
    const nextSet = new Set(deletingBookmarkIds.value)
    nextSet.add(bm.id)
    deletingBookmarkIds.value = nextSet
    try {
      await api.deleteBookmark(bm.id)
      bookmarks.value = bookmarks.value.filter((item) => item.id !== bm.id)
      void catalogStore.refreshSidebarData()
      snackbarStore.show({
        message: t('catalog.bookmarkDeleted'),
        actionLabel: t('catalog.undo'),
        onAction: () => {
          void api
            .createBookmark(targetBookId, {
              title: bm.title,
              note: bm.note ?? undefined,
              progress_percent: bm.location.progress_percent,
              locator: bm.location.locator,
              chapter_index: bm.location.chapter_index,
              page_number: bm.location.page_number,
              scroll_ratio: bm.location.scroll_ratio,
            })
            .then((restored) => {
              if (book.value?.id === targetBookId) {
                bookmarks.value = [restored, ...bookmarks.value]
              }
              void catalogStore.refreshSidebarData()
              snackbarStore.show(t('catalog.bookRestored'))
            })
            .catch(() => {})
        },
      })
    } catch (err) {
      snackbarStore.show({
        message: err instanceof Error ? err.message : String(err),
        tone: 'error',
      })
    } finally {
      const done = new Set(deletingBookmarkIds.value)
      done.delete(bm.id)
      deletingBookmarkIds.value = done
    }
  }

  function openTocTargetInReader(href: string): void {
    if (!book.value) return
    const chMatch = /^chapter:(\d+)$/.exec(href)
    const pgMatch = /^page:(\d+)$/.exec(href)
    void router.push({
      path: `/book/play/${encodeURIComponent(book.value.id)}`,
      query: {
        locator: href,
        chapter: chMatch ? chMatch[1] : undefined,
        page: pgMatch ? pgMatch[1] : undefined,
      },
    })
  }

  function openBookmarkInReader(bm: Bookmark): void {
    if (!book.value) return
    void router.push({
      path: `/book/play/${encodeURIComponent(book.value.id)}`,
      query: {
        locator: bm.location.locator,
        chapter: bm.location.chapter_index !== null ? String(bm.location.chapter_index) : undefined,
        page: bm.location.page_number !== null ? String(bm.location.page_number) : undefined,
        ratio: bm.location.scroll_ratio !== null ? String(bm.location.scroll_ratio) : undefined,
      },
    })
  }

  return {
    book,
    toc,
    bookmarks,
    loading,
    loadError,
    trackedSourceName,
    refreshingToc,
    syncingEpub,
    togglingFavorite,
    togglingShelfIds,
    deletingBook,
    unbindingBook,
    deletingBookmarkIds,
    editModalOpen,
    switchSourceOpen,
    switchingSourceKey,
    onlineDownloadTarget,
    onlineDownloadJobId,
    trackedDownloadJob,
    onlineDownloadsStore,
    loadDetail,
    goBack,
    triggerFileDownload,
    toggleFavorite,
    toggleShelf,
    handleMetadataSaved,
    onCoverChange,
    handleRefreshTrackedToc,
    handleSyncTrackedEpub,
    handleSwitchTrackedSource,
    openTrackedBatchDownload,
    closeTrackedDownloadModal,
    handleUnbindBook,
    handleDeleteBook,
    deleteBookmark,
    openTocTargetInReader,
    openBookmarkInReader,
  }
}
