import { ref, watch } from 'vue'
import { defineStore } from 'pinia'
import { useAuthStore } from '@/stores/auth'
import { useDomainStore } from '@/stores/domain'
import { useEventsStore } from '@/stores/events'
import { api } from './api'
import type {
  ShowSummaryDto,
  VideoLibraryDto,
  VideoShelfDto,
  VideoSummaryDto,
  VideoTagStat,
} from './types'

export const useVideoStore = defineStore('video', () => {
  const eventsStore = useEventsStore()

  const libraries = ref<VideoLibraryDto[]>([])
  const shelves = ref<VideoShelfDto[]>([])
  const shows = ref<ShowSummaryDto[]>([])
  const genres = ref<VideoTagStat[]>([])
  const tags = ref<VideoTagStat[]>([])
  const catalogVersion = ref(0)

  const SIDEBAR_CACHE_TTL_MS = 5000
  let sidebarInFlight: Promise<void> | null = null
  let lastSidebarLoadedAt = 0
  let sidebarDirtyAfterInFlight = false
  let sidebarDebounceTimer: ReturnType<typeof setTimeout> | null = null
  let sidebarEpoch = 0

  async function runSidebarFetch(epoch: number): Promise<void> {
    const [libsRes, shelvesRes, taxonomyRes, showsRes] = await Promise.allSettled([
      api.listLibraries(),
      api.listShelves(),
      api.getTaxonomy(),
      api.listShows(),
    ])
    if (epoch !== sidebarEpoch) return
    if (libsRes.status === 'fulfilled') libraries.value = libsRes.value
    if (shelvesRes.status === 'fulfilled') shelves.value = shelvesRes.value
    if (showsRes.status === 'fulfilled') shows.value = showsRes.value
    if (taxonomyRes.status === 'fulfilled') {
      genres.value = taxonomyRes.value.genres
      tags.value = taxonomyRes.value.tags
    }
    lastSidebarLoadedAt = Date.now()
  }

  async function refreshSidebar(force = false): Promise<void> {
    if (sidebarInFlight) {
      if (force) sidebarDirtyAfterInFlight = true
      return sidebarInFlight
    }
    if (!force && lastSidebarLoadedAt > 0 && Date.now() - lastSidebarLoadedAt < SIDEBAR_CACHE_TTL_MS) {
      return
    }
    const epoch = sidebarEpoch
    const task = runSidebarFetch(epoch).finally(() => {
      if (sidebarInFlight === task) {
        sidebarInFlight = null
      }
      if (epoch === sidebarEpoch && sidebarDirtyAfterInFlight) {
        sidebarDirtyAfterInFlight = false
        void refreshSidebar(true).catch(() => {})
      }
    })
    sidebarInFlight = task
    return task
  }

  function scheduleRefreshSidebar(): void {
    if (sidebarDebounceTimer) clearTimeout(sidebarDebounceTimer)
    sidebarDebounceTimer = setTimeout(() => {
      sidebarDebounceTimer = null
      void refreshSidebar(true).catch(() => {})
    }, 250)
  }

  function bumpCatalogVersion(): void {
    catalogVersion.value += 1
  }

  async function toggleVideoFavorite(video: VideoSummaryDto): Promise<boolean> {
    const next = !video.is_favorite
    await api.setFavorite('video', video.id, next)
    video.is_favorite = next
    return next
  }

  async function toggleShowFavorite(show: ShowSummaryDto): Promise<boolean> {
    const next = !show.is_favorite
    await api.setFavorite('show', show.id, next)
    show.is_favorite = next
    return next
  }

  async function addVideoToShelf(shelfId: string, videoId: string): Promise<void> {
    const detail = await api.getShelf(shelfId)
    const ids = detail.videos.map((v) => v.id)
    if (!ids.includes(videoId)) {
      ids.push(videoId)
      await api.updateShelf(shelfId, {
        name: detail.shelf.name,
        description: detail.shelf.description,
        video_ids: ids,
      })
      await refreshSidebar(true)
    }
  }

  const domainStore = useDomainStore()
  let sseDirty = false

  watch(
    () => eventsStore.domainRefreshTick.video,
    () => {
      if (!useAuthStore().isAuthenticated) return
      bumpCatalogVersion()
      if (domainStore.activeDomain !== 'video') {
        sseDirty = true
        lastSidebarLoadedAt = 0
        return
      }
      sseDirty = false
      scheduleRefreshSidebar()
    },
  )

  watch(
    () => domainStore.activeDomain,
    (next) => {
      if (next === 'video' && sseDirty && useAuthStore().isAuthenticated) {
        sseDirty = false
        scheduleRefreshSidebar()
      }
    },
  )

  function resetState(): void {
    sseDirty = false
    sidebarEpoch += 1
    sidebarInFlight = null
    if (sidebarDebounceTimer) {
      clearTimeout(sidebarDebounceTimer)
      sidebarDebounceTimer = null
    }
    lastSidebarLoadedAt = 0
    sidebarDirtyAfterInFlight = false
    libraries.value = []
    shelves.value = []
    shows.value = []
    genres.value = []
    tags.value = []
  }

  return {
    libraries,
    shelves,
    shows,
    genres,
    tags,
    catalogVersion,
    refreshSidebar,
    bumpCatalogVersion,
    toggleVideoFavorite,
    toggleShowFavorite,
    addVideoToShelf,
    resetState,
  }
})
