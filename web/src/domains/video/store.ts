import { ref, watch } from 'vue'
import { defineStore } from 'pinia'
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

  async function refreshSidebar(): Promise<void> {
    const [libsRes, shelvesRes, videosRes, showsRes] = await Promise.allSettled([
      api.listLibraries(),
      api.listShelves(),
      api.listVideos(),
      api.listShows(),
    ])
    if (libsRes.status === 'fulfilled') libraries.value = libsRes.value
    if (shelvesRes.status === 'fulfilled') shelves.value = shelvesRes.value
    if (showsRes.status === 'fulfilled') shows.value = showsRes.value

    const genreCounts = new Map<string, number>()
    const tagCounts = new Map<string, number>()
    if (videosRes.status === 'fulfilled') {
      for (const v of videosRes.value) {
        for (const g of v.genres) {
          const clean = g.trim()
          if (clean) genreCounts.set(clean, (genreCounts.get(clean) ?? 0) + 1)
        }
        for (const t of v.tags) {
          const clean = t.trim()
          if (clean) tagCounts.set(clean, (tagCounts.get(clean) ?? 0) + 1)
        }
      }
    }
    if (showsRes.status === 'fulfilled') {
      for (const s of showsRes.value) {
        for (const g of s.genres) {
          const clean = g.trim()
          if (clean && !genreCounts.has(clean)) genreCounts.set(clean, 1)
        }
        for (const t of s.tags) {
          const clean = t.trim()
          if (clean && !tagCounts.has(clean)) tagCounts.set(clean, 1)
        }
      }
    }
    genres.value = Array.from(genreCounts.entries())
      .map(([name, count]) => ({ name, count }))
      .sort((a, b) => b.count - a.count || a.name.localeCompare(b.name))
    tags.value = Array.from(tagCounts.entries())
      .map(([name, count]) => ({ name, count }))
      .sort((a, b) => b.count - a.count || a.name.localeCompare(b.name))
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
      await refreshSidebar()
    }
  }

  watch(
    () => eventsStore.domainRefreshTick.video,
    () => {
      bumpCatalogVersion()
      void refreshSidebar().catch(() => {})
    },
  )

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
  }
})
