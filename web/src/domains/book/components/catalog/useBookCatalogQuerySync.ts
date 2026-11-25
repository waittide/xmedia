import type { ComputedRef, Ref } from 'vue'
import { useRoute, useRouter, type LocationQueryRaw } from 'vue-router'
import { useCatalogStore } from '@/domains/book/store'
import type { BookFormat } from '@/domains/book/types'
import type { LibraryTab, SourceScope } from '@/sources'

export type BookBrowseView = 'items' | 'authors' | 'tags'

function readQueryString(val: unknown): string {
  if (typeof val === 'string') return val.trim()
  if (Array.isArray(val)) {
    const first = val.find((v): v is string => typeof v === 'string' && v.trim().length > 0)
    return first ? first.trim() : ''
  }
  return ''
}

function readQueryList(val: unknown): string[] {
  if (typeof val === 'string') return val.trim() ? [val.trim()] : []
  if (Array.isArray(val)) {
    return val
      .filter((v): v is string => typeof v === 'string' && v.trim().length > 0)
      .map((v) => v.trim())
  }
  return []
}

export function parseBookTab(raw: string): LibraryTab {
  if (raw === 'continue' || raw === 'favorites' || raw === 'collections') return raw
  return 'all'
}

export function parseBookView(raw: string): BookBrowseView {
  if (raw === 'authors' || raw === 'tags') return raw
  return 'items'
}

function queryValuesEqual(a: unknown, b: unknown): boolean {
  if (Array.isArray(a) || Array.isArray(b)) {
    const la = readQueryList(a)
    const lb = readQueryList(b)
    return la.length === lb.length && la.every((v, i) => v === lb[i])
  }
  return a === b
}

export function useBookCatalogQuerySync(options: {
  activeScope: ComputedRef<SourceScope>
  getSourceId: () => string | null
  activeTab: Ref<LibraryTab>
  activeView: Ref<BookBrowseView>
  activeCollectionId: Ref<string>
  searchInput: Ref<string>
}) {
  const route = useRoute()
  const router = useRouter()
  const catalogStore = useCatalogStore()
  let syncingRouteQuery = false

  function buildCanonicalQuery(): LocationQueryRaw {
    const q: LocationQueryRaw = {}
    const scope = options.activeScope.value

    if (scope !== 'local') {
      q.scope = scope
      const srcId = options.getSourceId()
      if (srcId) q.source = srcId
      if (options.searchInput.value.trim()) q.q = options.searchInput.value.trim()
      return q
    }

    if (options.activeTab.value !== 'all') q.tab = options.activeTab.value
    if (options.activeCollectionId.value) q.collection = options.activeCollectionId.value
    if (
      options.activeTab.value === 'all' &&
      !options.activeCollectionId.value &&
      options.activeView.value !== 'items'
    ) {
      q.view = options.activeView.value
    }
    if (options.searchInput.value.trim()) q.q = options.searchInput.value.trim()

    if (catalogStore.filterLibraryIds.length === 1) {
      q.library = catalogStore.filterLibraryIds[0]!
    } else if (catalogStore.filterLibraryIds.length > 1) {
      q.library = [...catalogStore.filterLibraryIds]
    }
    if (catalogStore.filterAuthors.length === 1) {
      q.author = catalogStore.filterAuthors[0]!
    } else if (catalogStore.filterAuthors.length > 1) {
      q.author = [...catalogStore.filterAuthors]
    }
    if (catalogStore.filterTags.length === 1) {
      q.tag = catalogStore.filterTags[0]!
    } else if (catalogStore.filterTags.length > 1) {
      q.tag = [...catalogStore.filterTags]
    }
    if (catalogStore.filterTags.length > 1 && catalogStore.filterTagMode !== 'any') {
      q.tag_mode = catalogStore.filterTagMode
    }
    if (catalogStore.filterFormats.length === 1) {
      q.format = catalogStore.filterFormats[0]!
    } else if (catalogStore.filterFormats.length > 1) {
      q.format = [...catalogStore.filterFormats]
    }
    if (catalogStore.includeMissing) q.missing = '1'
    if (catalogStore.sortBy !== 'created_at') q.sort = catalogStore.sortBy
    if (catalogStore.sortOrder !== 'desc') q.order = catalogStore.sortOrder
    if (catalogStore.page > 1) q.page = String(catalogStore.page)

    return q
  }

  function syncRouteQuery(): void {
    const next = buildCanonicalQuery()
    const cur = route.query
    const same =
      route.path === '/book/library' &&
      Object.keys(next).length === Object.keys(cur).length &&
      Object.entries(next).every(([k, v]) => queryValuesEqual(cur[k], v))
    if (same) return
    syncingRouteQuery = true
    void router.replace({ path: '/book/library', query: next }).finally(() => {
      syncingRouteQuery = false
    })
  }

  function applyRouteQuery(): void {
    const qTab = readQueryString(route.query.tab)
    const qCollection = readQueryString(route.query.collection) || readQueryString(route.query.shelf)
    const qView = readQueryString(route.query.view)
    const qKeyword = readQueryString(route.query.q)
    const qLibraries = readQueryList(route.query.library)
    const qAuthors = readQueryList(route.query.author)
    const qTags = readQueryList(route.query.tag)
    const qTagMode = readQueryString(route.query.tag_mode)
    const qFormats = readQueryList(route.query.format).filter(
      (f): f is BookFormat => f === 'epub' || f === 'txt' || f === 'pdf',
    )
    const qMissing = route.query.missing === '1' || route.query.missing === 'true'
    const qSort = readQueryString(route.query.sort) || readQueryString(route.query.sort_by)
    const qOrder = readQueryString(route.query.order) || readQueryString(route.query.sort_order)
    const qPage = Number.parseInt(readQueryString(route.query.page), 10)

    options.activeTab.value = qCollection && qTab === 'collections' ? 'all' : parseBookTab(qTab)
    options.activeCollectionId.value = qCollection
    options.activeView.value = qCollection ? 'items' : parseBookView(qView)
    options.searchInput.value = qKeyword

    catalogStore.activeSection = qCollection ? 'shelf' : 'all'
    catalogStore.activeLibraryId = ''
    catalogStore.activeShelfId = qCollection
    catalogStore.keyword = qKeyword
    catalogStore.filterLibraryIds = qLibraries
    catalogStore.filterAuthors = qAuthors
    catalogStore.filterTags = qTags
    catalogStore.filterTagMode = qTagMode === 'all' ? 'all' : 'any'
    catalogStore.filterFormats = qFormats
    catalogStore.includeMissing = qMissing
    catalogStore.sortBy =
      qSort === 'created_at' || qSort === 'updated_at' || qSort === 'title' || qSort === 'file_size'
        ? qSort
        : 'created_at'
    catalogStore.sortOrder = qOrder === 'asc' || qOrder === 'desc' ? qOrder : 'desc'
    catalogStore.page = Number.isFinite(qPage) && qPage >= 1 ? qPage : 1
  }

  return {
    isSyncing: () => syncingRouteQuery,
    syncRouteQuery,
    applyRouteQuery,
  }
}
