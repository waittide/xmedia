import { computed, ref, watch } from 'vue'
import { defineStore } from 'pinia'
import { api } from './api'
import { useAuthStore } from '@/stores/auth'
import { useDomainStore } from '@/stores/domain'
import { useEventsStore } from '@/stores/events'
import type {
  BookBookmarkItemDto,
  BookFormat,
  BookSortField,
  BookSummaryDto,
  FacetCount,
  LibraryDto,
  ShelfDto,
  SortOrder,
  TagMatchMode,
} from './types'

export type NavSection = 'all' | 'recent' | 'favorites' | 'library' | 'shelf' | 'tag' | 'author'

function readStoredViewMode(): 'list' | 'grid' {
  try {
    const raw = localStorage.getItem('xmedia.book_view_mode')
    if (raw === 'list' || raw === 'grid') return raw
  } catch {
    // ignore storage errors
  }
  return 'grid'
}

export const useCatalogStore = defineStore('bookCatalog', () => {
  const eventsStore = useEventsStore()

  const libraries = ref<LibraryDto[]>([])
  const shelves = ref<ShelfDto[]>([])
  const authors = ref<FacetCount[]>([])
  const tags = ref<FacetCount[]>([])
  const bookmarks = ref<BookBookmarkItemDto[]>([])

  const books = ref<BookSummaryDto[]>([])
  const total = ref<number>(0)
  const page = ref<number>(1)
  const pageSize = ref<number>(24)
  const loading = ref<boolean>(false)
  let fetchSeq = 0
  let sidebarSeq = 0

  const viewMode = ref<'list' | 'grid'>(readStoredViewMode())
  const activeSection = ref<NavSection>('all')
  const activeLibraryId = ref<string>('')
  const activeShelfId = ref<string>('')

  const keyword = ref<string>('')
  const filterFormats = ref<BookFormat[]>([])
  const filterAuthors = ref<string[]>([])
  const filterTags = ref<string[]>([])
  const filterTagMode = ref<TagMatchMode>('any')
  const filterLibraryIds = ref<string[]>([])
  const includeMissing = ref<boolean>(false)
  const sortBy = ref<BookSortField>('created_at')
  const sortOrder = ref<SortOrder>('desc')

  const selectedBookIds = ref<Set<string>>(new Set())
  const activeBookId = ref<string | null>(null)
  const scanRefreshVersion = computed(() => eventsStore.domainRefreshTick.book)

  const activeBook = computed<BookSummaryDto | null>(
    () => books.value.find((b) => b.id === activeBookId.value) ?? null,
  )

  const scanningLibrary = computed(
    () => libraries.value.find((lib) => lib.scan_status.state === 'scanning') ?? null,
  )

  const anyScanning = computed(() => scanningLibrary.value !== null)

  function setViewMode(mode: 'list' | 'grid'): void {
    viewMode.value = mode
    try {
      localStorage.setItem('xmedia.book_view_mode', mode)
    } catch {
      // ignore storage errors
    }
  }

  async function refreshSidebarData(): Promise<void> {
    const seq = ++sidebarSeq
    const [libs, sh, au, tg, bms] = await Promise.all([
      api.listLibraries(),
      api.listShelves(),
      api.listAuthors(),
      api.listTags(),
      api.listAllBookmarks().catch(() => []),
    ])
    if (seq !== sidebarSeq) return
    libraries.value = libs
    shelves.value = sh
    authors.value = au
    tags.value = tg
    bookmarks.value = bms
  }

  async function fetchBooks(): Promise<void> {
    const seq = ++fetchSeq
    loading.value = true
    selectedBookIds.value = new Set()
    try {
      const hasSecondaryFilters =
        filterFormats.value.length > 0 ||
        filterAuthors.value.length > 0 ||
        filterTags.value.length > 0 ||
        filterLibraryIds.value.length > 0 ||
        includeMissing.value

      if (activeSection.value === 'recent' && !keyword.value.trim() && !hasSecondaryFilters) {
        const recent = await api.listRecent(48)
        if (seq !== fetchSeq) return
        books.value = recent
        total.value = recent.length
        return
      }

      const effectiveLibIds =
        activeSection.value === 'library'
          ? activeLibraryId.value
            ? [activeLibraryId.value]
            : undefined
          : filterLibraryIds.value.length > 0
            ? filterLibraryIds.value
            : undefined

      const res = await api.searchBooks({
        keyword: keyword.value.trim() || undefined,
        formats: filterFormats.value.length > 0 ? filterFormats.value : undefined,
        authors: filterAuthors.value.length > 0 ? filterAuthors.value : undefined,
        tags: filterTags.value.length > 0 ? filterTags.value : undefined,
        tag_mode: filterTags.value.length > 1 ? filterTagMode.value : undefined,
        library_ids: effectiveLibIds,
        favorites_only: activeSection.value === 'favorites' || undefined,
        shelf_id: activeSection.value === 'shelf' ? activeShelfId.value || undefined : undefined,
        include_missing: includeMissing.value || undefined,
        sort_by: sortBy.value,
        sort_order: sortOrder.value,
        page: page.value,
        page_size: pageSize.value,
      })

      if (seq !== fetchSeq) return
      books.value = res.items
      total.value = res.total
    } finally {
      if (seq === fetchSeq) {
        loading.value = false
      }
    }
  }

  function selectSection(section: NavSection, id = ''): void {
    activeSection.value = section
    page.value = 1
    if (section === 'library') {
      activeLibraryId.value = id
      filterTags.value = []
      filterAuthors.value = []
    } else if (section === 'shelf') {
      activeShelfId.value = id
      filterTags.value = []
      filterAuthors.value = []
    } else if (section === 'tag') {
      filterTags.value = id ? [id] : []
      filterAuthors.value = []
    } else if (section === 'author') {
      filterAuthors.value = id ? [id] : []
      filterTags.value = []
    } else {
      filterTags.value = []
      filterAuthors.value = []
    }
    void fetchBooks()
  }

  function resetFilters(): void {
    keyword.value = ''
    filterFormats.value = []
    filterAuthors.value = []
    filterTags.value = []
    filterTagMode.value = 'any'
    filterLibraryIds.value = []
    includeMissing.value = false
    sortBy.value = 'created_at'
    sortOrder.value = 'desc'
    page.value = 1
    void fetchBooks()
  }

  function toggleSelectBook(id: string): void {
    const next = new Set(selectedBookIds.value)
    if (next.has(id)) next.delete(id)
    else next.add(id)
    selectedBookIds.value = next
  }

  function clearSelection(): void {
    selectedBookIds.value = new Set()
  }

  function updateBookInList(updated: BookSummaryDto): void {
    const idx = books.value.findIndex((b) => b.id === updated.id)
    if (idx >= 0) books.value[idx] = updated
  }

  function applyLibraryUpdate(updated: LibraryDto): void {
    const idx = libraries.value.findIndex((l) => l.id === updated.id)
    if (idx >= 0) {
      libraries.value[idx] = updated
    } else {
      libraries.value.push(updated)
    }
  }

  const domainStore = useDomainStore()
  let sseDirty = false

  watch(scanRefreshVersion, () => {
    if (!useAuthStore().isAuthenticated) return
    if (domainStore.activeDomain !== 'book') {
      sseDirty = true
      return
    }
    sseDirty = false
    void Promise.all([refreshSidebarData(), fetchBooks()]).catch(() => {})
  })

  watch(
    () => domainStore.activeDomain,
    (next) => {
      if (next === 'book' && sseDirty && useAuthStore().isAuthenticated) {
        sseDirty = false
        void Promise.all([refreshSidebarData(), fetchBooks()]).catch(() => {})
      }
    },
  )

  function resetState(): void {
    sseDirty = false
    fetchSeq += 1
    sidebarSeq += 1
    loading.value = false
    libraries.value = []
    shelves.value = []
    authors.value = []
    tags.value = []
    bookmarks.value = []
    books.value = []
    total.value = 0
    page.value = 1
    pageSize.value = 24
    sortBy.value = 'created_at'
    sortOrder.value = 'desc'
    activeSection.value = 'all'
    activeLibraryId.value = ''
    activeShelfId.value = ''
    keyword.value = ''
    filterFormats.value = []
    filterAuthors.value = []
    filterTags.value = []
    filterTagMode.value = 'any'
    filterLibraryIds.value = []
    includeMissing.value = false
    selectedBookIds.value = new Set()
    activeBookId.value = null
  }

  return {
    libraries,
    shelves,
    authors,
    tags,
    bookmarks,
    books,
    total,
    page,
    pageSize,
    loading,
    viewMode,
    activeSection,
    activeLibraryId,
    activeShelfId,
    keyword,
    filterFormats,
    filterAuthors,
    filterTags,
    filterTagMode,
    filterLibraryIds,
    includeMissing,
    sortBy,
    sortOrder,
    selectedBookIds,
    activeBookId,
    activeBook,
    scanningLibrary,
    anyScanning,
    scanRefreshVersion,
    setViewMode,
    refreshSidebarData,
    fetchBooks,
    selectSection,
    resetFilters,
    toggleSelectBook,
    clearSelection,
    updateBookInList,
    applyLibraryUpdate,
    resetState,
  }
})
