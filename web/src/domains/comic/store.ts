import { defineStore } from 'pinia'
import { ref, watch } from 'vue'
import { api } from './api'
import { useAuthStore } from '@/stores/auth'
import { useEventsStore } from '@/stores/events'
import { useUploadStore } from '@/stores/uploads'
import type {
  ComicBookmarkItemDto,
  Library,
  ShelfDetailDto,
  TaxonomyCountDto,
} from './types'

export function resolveLibrarySeriesCount(lib: Library): number {
  if (typeof lib.series_count === 'number') {
    return lib.series_count
  }
  if (lib.scan_state.state === 'completed') {
    return lib.scan_state.series_count
  }
  return 0
}

export const useSidebarStore = defineStore('comicSidebar', () => {
  const uploadStore = useUploadStore()
  const eventsStore = useEventsStore()

  const libraries = ref<Library[]>([])
  const shelves = ref<ShelfDetailDto[]>([])
  const tags = ref<TaxonomyCountDto[]>([])
  const genres = ref<TaxonomyCountDto[]>([])
  const writers = ref<TaxonomyCountDto[]>([])
  const pencillers = ref<TaxonomyCountDto[]>([])
  const bookmarks = ref<ComicBookmarkItemDto[]>([])
  const loading = ref(false)
  let fetchSeq = 0

  async function refresh(): Promise<void> {
    const seq = ++fetchSeq
    loading.value = true
    try {
      const [libs, sh, tax, bms] = await Promise.all([
        api.listLibraries(),
        api.listShelves(),
        api.taxonomyOverview(),
        api.listAllBookmarks().catch(() => []),
      ])
      if (seq !== fetchSeq) return
      libraries.value = libs
      shelves.value = sh
      tags.value = tax.tags
      genres.value = tax.genres
      writers.value = tax.writers
      pencillers.value = tax.pencillers
      bookmarks.value = bms
    } finally {
      if (seq === fetchSeq) {
        loading.value = false
      }
    }
  }

  watch(
    () => [uploadStore.catalogVersion, eventsStore.domainRefreshTick.comic],
    () => {
      if (!useAuthStore().isAuthenticated) return
      void refresh().catch(() => {})
    },
  )

  function resetState(): void {
    fetchSeq += 1
    libraries.value = []
    shelves.value = []
    tags.value = []
    genres.value = []
    writers.value = []
    pencillers.value = []
    bookmarks.value = []
    loading.value = false
  }

  return {
    libraries,
    shelves,
    tags,
    genres,
    writers,
    pencillers,
    bookmarks,
    loading,
    refresh,
    resetState,
  }
})
