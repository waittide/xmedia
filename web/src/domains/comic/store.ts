import { defineStore } from 'pinia'
import { ref, watch } from 'vue'
import { api } from './api'
import { useEventsStore } from '@/stores/events'
import { useUploadStore } from '@/stores/uploads'
import type { Library, ShelfDetailDto, TaxonomyCountDto } from './types'

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
  const loading = ref(false)

  async function refresh(): Promise<void> {
    loading.value = true
    try {
      const [libs, sh, tax] = await Promise.all([
        api.listLibraries(),
        api.listShelves(),
        api.taxonomyOverview(),
      ])
      libraries.value = libs
      shelves.value = sh
      tags.value = tax.tags
    } finally {
      loading.value = false
    }
  }

  watch(
    () => [uploadStore.catalogVersion, eventsStore.domainRefreshTick.comic],
    () => {
      void refresh().catch(() => {})
    },
  )

  return {
    libraries,
    shelves,
    tags,
    loading,
    refresh,
  }
})
