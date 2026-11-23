<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useI18n as useGlobalI18n } from 'vue-i18n'
import { RouterLink, useRoute, useRouter } from 'vue-router'
import {
  LuBookmark,
  LuBookOpen,
  LuChevronDown,
  LuChevronUp,
  LuCloud,
  LuDownload,
  LuFileCheck,
  LuFolder,
  LuImage,
  LuList,
  LuPencil,
  LuRefreshCw,
  LuRepeat,
  LuTrash2,
  LuX,
} from 'vue-icons-plus/lu'
import {
  MediaDetailTemplate,
  type MediaDetailSpec,
  type MediaDetailTag,
} from '@/components/domain'
import { MdIconButton, MdMenu, MdMenuItem } from '@/components/ui'
import { api } from '@/domains/book/api'
import BookMetadataModal from '@/domains/book/components/BookMetadataModal.vue'
import BookSourceSwitchDialog from '@/domains/book/components/BookSourceSwitchDialog.vue'
import OnlineDownloadModal, {
  type OnlineDownloadTarget,
} from '@/domains/book/components/OnlineDownloadModal.vue'
import TocTree from '@/domains/book/components/TocTree.vue'
import { useBookI18n as useI18n } from '@/domains/book/i18n'
import { onlineBookApi, type AggregatedSearchItemDto } from '@/domains/book/onlineApi'
import { useCatalogStore } from '@/domains/book/store'
import type { Bookmark, BookSummaryDto, TocItem } from '@/domains/book/types'
import {
  formatProgressPercentValue,
  hasBookProgress,
  normalizeBookProgressPercent,
} from '@/domains/book/utils/bookVm'
import { libraryRoute, playRoute } from '@/sources'
import { useAuthStore } from '@/stores/auth'
import { useDialogStore } from '@/stores/dialog'
import { useOnlineDownloadsStore } from '@/stores/onlineDownloads'
import { useSnackbarStore } from '@/stores/snackbar'
import { useThemeStore } from '@/stores/theme'
import { formatBytes } from '@/utils/format'

const { t } = useI18n()
const { t: globalT } = useGlobalI18n()
const route = useRoute()
const router = useRouter()
const authStore = useAuthStore()
const catalogStore = useCatalogStore()
const onlineDownloadsStore = useOnlineDownloadsStore()
const snackbarStore = useSnackbarStore()
const themeStore = useThemeStore()
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
const deletingBookmarkIds = ref<Set<string>>(new Set())

const editModalOpen = ref(false)
const switchSourceOpen = ref(false)
const switchingSourceKey = ref<string | null>(null)
const onlineDownloadTarget = ref<OnlineDownloadTarget | null>(null)
const onlineDownloadJobId = ref<string | null>(null)

const tocExpanded = ref(false)
const bookmarksExpanded = ref(false)
const coverInputRef = ref<HTMLInputElement | null>(null)

let detailSeq = 0
let refreshTocSeq = 0
let syncEpubSeq = 0

const trackedDownloadJob = computed(() =>
  onlineDownloadJobId.value
    ? (onlineDownloadsStore.jobs.find((j) => j.id === onlineDownloadJobId.value) ?? null)
    : null,
)

const hasProgress = computed(() => hasBookProgress(book.value?.progress))

const progressPercent = computed<number | null>(() =>
  normalizeBookProgressPercent(book.value?.progress),
)

const progressLabel = computed(() =>
  progressPercent.value !== null
    ? t('catalog.progressLabel', { percent: formatProgressPercentValue(progressPercent.value) })
    : '',
)

const formattedLanguage = computed(() => {
  const raw = book.value?.metadata.language
  if (!raw) return null
  try {
    const dn = new Intl.DisplayNames([themeStore.locale], { type: 'language' })
    return dn.of(raw) || raw
  } catch {
    return raw
  }
})

const detailTags = computed<MediaDetailTag[]>(() =>
  (book.value?.metadata.tags ?? []).map((tg) => ({
    label: `#${tg}`,
    value: tg,
    to: libraryRoute('book', { tag: tg }),
  })),
)

const detailSpecs = computed<MediaDetailSpec[]>(() => {
  const b = book.value
  if (!b) return []
  const items: MediaDetailSpec[] = [
    { label: t('detail.library'), value: b.library_name || '—' },
    {
      label: t('detail.sourceMode'),
      value: b.is_managed ? t('catalog.managedBadge') : t('catalog.mountedBadge'),
    },
    { label: t('detail.format'), value: b.format.toUpperCase(), mono: true },
    { label: t('detail.fileSize'), value: formatBytes(b.file_size) },
  ]
  if (b.online_tracking) {
    if (trackedSourceName.value) {
      items.push({ label: t('online.sourceLabel'), value: trackedSourceName.value })
    }
    items.push({
      label: t('reader.toc'),
      value: t('online.trackedBadgeStatus', {
        cached: b.online_tracking.cached_chapters,
        total: b.online_tracking.total_chapters || '?',
      }),
    })
  }
  if (b.metadata.page_count) {
    items.push({
      label: t('detail.structure'),
      value: t('detail.pages', { count: b.metadata.page_count }),
    })
  } else if (b.metadata.chapter_count) {
    items.push({
      label: t('detail.structure'),
      value: t('detail.chapters', { count: b.metadata.chapter_count }),
    })
  }
  if (formattedLanguage.value) {
    items.push({ label: t('detail.language'), value: formattedLanguage.value })
  }
  if (b.metadata.publisher) {
    items.push({ label: t('detail.publisher'), value: b.metadata.publisher })
  }
  if (b.metadata.published_at) {
    items.push({ label: t('detail.publishedAt'), value: b.metadata.published_at })
  }
  if (b.metadata.isbn) {
    items.push({ label: t('detail.isbn'), value: b.metadata.isbn, mono: true })
  }
  return items
})

const readRoute = computed(() =>
  book.value ? playRoute({ scope: 'local', domain: 'book', id: book.value.id }) : undefined,
)

const bookCoverUrl = computed(() =>
  book.value?.has_cover ? api.coverUrl(book.value.id, book.value.updated_at) : null,
)

const isBookInAnyShelf = computed(() => {
  const id = book.value?.id
  if (!id) return false
  return catalogStore.shelves.some((sh) => sh.book_ids.includes(id))
})

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
      tocExpanded.value = false
      bookmarksExpanded.value = false
      void loadDetail(String(nextId))
    }
  },
)

function goBack(): void {
  const prev = (window.history.state as { back?: string | null } | null)?.back
  if (typeof prev === 'string' && !prev.startsWith('/book/play/') && !prev.startsWith('/book/read/')) {
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

const unbindingBook = ref(false)

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
</script>

<template>
  <div class="px-4 py-5 sm:px-6 sm:py-6">
    <MediaDetailTemplate
      :title="book?.metadata.title || ''"
      :cover="bookCoverUrl"
      aspect="poster"
      cover-style="book"
      :fallback-icon="LuBookOpen"
      :back-label="t('detail.back')"
      :progress-percent="progressPercent"
      :progress-label="progressLabel"
      :primary-action-label="hasProgress ? t('catalog.continueReading') : t('catalog.startReading')"
      :primary-action-icon="LuBookOpen"
      :primary-action-to="readRoute"
      :primary-action-disabled="Boolean(book?.is_missing)"
      :favorite="Boolean(book?.is_favorite)"
      :show-favorite="Boolean(book)"
      :favorite-loading="togglingFavorite"
      :description="book?.metadata.description"
      :tags="detailTags"
      :specs="detailSpecs"
      :specs-title="t('detail.specifications')"
      :loading="loading"
      :error="loadError || (!loading && !book ? t('detail.notFound') : null)"
      :availability-warning="book?.is_missing ? t('catalog.missingBadge') : null"
      @back="goBack"
      @toggle-favorite="toggleFavorite"
      @retry="loadDetail(String(route.params.id))"
    >
      <template #hero-meta>
        <div v-if="book" class="space-y-1">
          <p class="text-body-medium text-on-surface-variant">
            <template v-if="book.metadata.authors.length > 0">
              <template v-for="(author, idx) in book.metadata.authors" :key="author">
                <RouterLink
                  :to="libraryRoute('book', { author })"
                  class="transition-colors duration-150 ease-standard hover:text-primary hover:underline"
                >
                  {{ author }}
                </RouterLink>
                <span v-if="idx < book.metadata.authors.length - 1">, </span>
              </template>
            </template>
            <span v-else>{{ t('catalog.unknownAuthor') }}</span>
          </p>

          <p class="text-label-medium flex flex-wrap items-center gap-x-2 gap-y-0.5 text-on-surface-variant">
            <span class="font-mono uppercase">{{ book.format }}</span>
            <template v-if="book.online_tracking">
              <span>·</span>
              <span>
                {{
                  t('online.trackedBadgeStatus', {
                    cached: book.online_tracking.cached_chapters,
                    total: book.online_tracking.total_chapters || '?',
                  })
                }}
              </span>
              <span
                v-if="book.online_tracking.epub_dirty"
                class="rounded-corner-xs bg-tertiary-container px-1.5 py-0.5 text-label-small text-on-tertiary-container"
              >
                {{ t('online.epubDirtyBadge') }}
              </span>
            </template>
          </p>
        </div>
      </template>

      <template #secondary-actions>
        <MdMenu
          v-if="book"
          placement="bottom-start"
          :min-width="208"
          :aria-label="t('detail.shelvesMenu')"
        >
          <template #trigger="{ toggle, triggerAttrs }">
            <MdIconButton
              v-bind="triggerAttrs"
              variant="tonal"
              :selected="isBookInAnyShelf"
              :icon="LuFolder"
              :aria-label="t('detail.shelvesMenu')"
              :tooltip="t('detail.shelvesMenu')"
              @click="toggle"
            />
          </template>

          <div
            v-if="catalogStore.shelves.length === 0"
            class="text-body-small px-3 py-2 text-on-surface-variant"
          >
            {{ t('detail.noShelvesYet') }}
          </div>
          <MdMenuItem
            v-for="sh in catalogStore.shelves"
            :key="sh.id"
            :label="sh.name"
            :selected="sh.book_ids.includes(book.id)"
            :disabled="togglingShelfIds.has(sh.id)"
            keep-open
            @select="toggleShelf(sh.id, sh.book_ids.includes(book.id))"
          />
        </MdMenu>
      </template>

      <template v-if="book" #overflow-menu>
        <MdMenuItem
          v-if="!book.is_missing"
          :icon="LuDownload"
          :label="book.online_tracking ? t('online.downloadEpub3') : t('catalog.download')"
          @select="triggerFileDownload"
        />

        <template v-if="book.online_tracking">
          <MdMenuItem
            :icon="LuRefreshCw"
            :label="t('online.refreshToc')"
            :disabled="refreshingToc"
            @select="handleRefreshTrackedToc"
          />
          <MdMenuItem
            v-if="book.online_tracking.epub_dirty"
            :icon="LuFileCheck"
            :label="syncingEpub ? t('online.syncingEpub') : t('online.syncEpub')"
            :disabled="syncingEpub"
            @select="handleSyncTrackedEpub"
          />
          <template v-if="authStore.isAdmin">
            <MdMenuItem
              :icon="LuRepeat"
              :label="t('online.switchSource')"
              @select="switchSourceOpen = true"
            />
            <MdMenuItem
              :icon="LuCloud"
              :label="t('online.cacheChapters')"
              @select="openTrackedBatchDownload"
            />
          </template>
        </template>

        <template v-if="authStore.isAdmin">
          <MdMenuItem
            :icon="LuPencil"
            :label="t('detail.editMetadata')"
            @select="editModalOpen = true"
          />
          <MdMenuItem
            :icon="LuImage"
            :label="t('detail.replaceCover')"
            @select="coverInputRef?.click()"
          />
        </template>
      </template>

      <template v-if="book" #overflow-danger>
        <MdMenuItem
          :icon="LuX"
          :label="globalT('pool.unbindBtn')"
          :disabled="unbindingBook"
          destructive
          @select="handleUnbindBook"
        />
        <MdMenuItem
          v-if="authStore.isAdmin"
          :icon="LuTrash2"
          :label="t('detail.deleteBook')"
          :disabled="deletingBook"
          destructive
          @select="handleDeleteBook"
        />
      </template>

      <!-- Collapsible TOC & Bookmarks Sections -->
      <div v-if="book && (toc.length > 0 || bookmarks.length > 0)" class="space-y-2">
        <section v-if="toc.length > 0" class="border-t border-outline-variant pt-3">
          <button
            type="button"
            :aria-expanded="tocExpanded"
            class="md-state text-title-small inline-flex cursor-pointer items-center gap-2 rounded-corner-full px-3 py-2 text-on-surface-variant hover:text-on-surface"
            @click="tocExpanded = !tocExpanded"
          >
            <LuList class="h-4 w-4" aria-hidden="true" />
            <span>{{ t('reader.toc') }} ({{ toc.length }})</span>
            <LuChevronUp v-if="tocExpanded" class="h-4 w-4" aria-hidden="true" />
            <LuChevronDown v-else class="h-4 w-4" aria-hidden="true" />
          </button>

          <div
            v-if="tocExpanded"
            class="mt-2 max-h-96 overflow-y-auto rounded-corner-md bg-surface-container-low p-3"
          >
            <TocTree
              :items="toc"
              :active-href="book.progress?.locator"
              searchable
              @select="openTocTargetInReader"
            />
          </div>
        </section>

        <section v-if="bookmarks.length > 0" class="border-t border-outline-variant pt-3">
          <button
            type="button"
            :aria-expanded="bookmarksExpanded"
            class="md-state text-title-small inline-flex cursor-pointer items-center gap-2 rounded-corner-full px-3 py-2 text-on-surface-variant hover:text-on-surface"
            @click="bookmarksExpanded = !bookmarksExpanded"
          >
            <LuBookmark class="h-4 w-4" aria-hidden="true" />
            <span>{{ t('detail.bookmarksSection') }} ({{ bookmarks.length }})</span>
            <LuChevronUp v-if="bookmarksExpanded" class="h-4 w-4" aria-hidden="true" />
            <LuChevronDown v-else class="h-4 w-4" aria-hidden="true" />
          </button>

          <div v-if="bookmarksExpanded" class="mt-2 space-y-1.5">
            <div
              v-for="bm in bookmarks"
              :key="bm.id"
              class="flex items-center justify-between gap-2 rounded-corner-md bg-surface-container-low px-3.5 py-2.5"
            >
              <button
                type="button"
                class="md-state flex min-w-0 flex-1 cursor-pointer items-center gap-3 rounded-corner-sm py-0.5 text-left"
                @click="openBookmarkInReader(bm)"
              >
                <LuBookmark class="h-4 w-4 shrink-0 text-primary" aria-hidden="true" />
                <div class="min-w-0">
                  <p class="text-body-medium truncate text-on-surface">
                    {{ bm.title }}
                  </p>
                  <p class="text-label-small tabular-nums text-on-surface-variant">
                    {{ Math.round(bm.location.progress_percent) }}%
                  </p>
                </div>
              </button>

              <MdIconButton
                size="sm"
                variant="standard"
                :icon="LuTrash2"
                :disabled="deletingBookmarkIds.has(bm.id)"
                :aria-label="t('common.delete')"
                @click="deleteBookmark(bm)"
              />
            </div>
          </div>
        </section>
      </div>
    </MediaDetailTemplate>

    <input
      v-if="authStore.isAdmin"
      ref="coverInputRef"
      type="file"
      accept="image/*"
      class="hidden"
      @change="onCoverChange"
    />

    <BookMetadataModal
      :open="editModalOpen"
      :book="book"
      @close="editModalOpen = false"
      @saved="handleMetadataSaved"
    />

    <BookSourceSwitchDialog
      v-if="book?.online_tracking"
      :open="switchSourceOpen"
      :book-title="book.metadata.title"
      :current-source-id="book.online_tracking.source_id"
      :current-book-url="book.online_tracking.book_url"
      :switching-key="switchingSourceKey"
      show-clear-cache-option
      @close="switchSourceOpen = false"
      @select="handleSwitchTrackedSource"
    />

    <OnlineDownloadModal
      :open="onlineDownloadTarget !== null"
      :target="onlineDownloadTarget"
      :external-job="trackedDownloadJob"
      @close="closeTrackedDownloadModal"
      @job-updated="
        onlineDownloadJobId = $event.id;
        onlineDownloadsStore.trackJob($event)
      "
    />
  </div>
</template>
