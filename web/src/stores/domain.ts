import { ref } from 'vue'
import { defineStore } from 'pinia'
import type { MediaKind } from '@/types/api'

const DOMAIN_KEY = 'xmedia.active_domain'

const DEFAULT_ROUTES: Record<MediaKind, string> = {
  book: '/book',
  comic: '/comic',
  music: '/music',
  video: '/video',
}

export const useDomainStore = defineStore('domain', () => {
  const saved = localStorage.getItem(DOMAIN_KEY) as MediaKind | null
  const activeDomain = ref<MediaKind>(
    saved && ['book', 'comic', 'music', 'video'].includes(saved) ? saved : 'book',
  )
  const lastDomainRoute = ref<Record<MediaKind, string>>({ ...DEFAULT_ROUTES })

  function setActiveDomain(next: MediaKind): void {
    activeDomain.value = next
    localStorage.setItem(DOMAIN_KEY, next)
  }

  function getLastRoute(kind: MediaKind): string {
    return lastDomainRoute.value[kind] || `/${kind}`
  }

  function syncFromPath(fullPath: string): void {
    const cleanPath = fullPath.split('?')[0] ?? fullPath
    if (cleanPath === '/book' || cleanPath.startsWith('/book/')) {
      setActiveDomain('book')
      if (!cleanPath.startsWith('/book/read/')) {
        lastDomainRoute.value.book = fullPath
      }
    } else if (cleanPath === '/comic' || cleanPath.startsWith('/comic/')) {
      setActiveDomain('comic')
      if (!cleanPath.startsWith('/comic/read/')) {
        lastDomainRoute.value.comic = fullPath
      }
    } else if (cleanPath === '/music' || cleanPath.startsWith('/music/')) {
      setActiveDomain('music')
    } else if (cleanPath === '/video' || cleanPath.startsWith('/video/')) {
      setActiveDomain('video')
      if (!cleanPath.startsWith('/video/watch/')) {
        lastDomainRoute.value.video = fullPath
      }
    }
    if (cleanPath === '/music' || cleanPath.startsWith('/music/')) {
      lastDomainRoute.value.music = fullPath
    }
  }

  function resetRoutes(): void {
    lastDomainRoute.value = { ...DEFAULT_ROUTES }
  }

  return {
    activeDomain,
    lastDomainRoute,
    setActiveDomain,
    getLastRoute,
    syncFromPath,
    resetRoutes,
  }
})
