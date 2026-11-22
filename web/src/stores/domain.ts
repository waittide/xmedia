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
  let saved: MediaKind | null = null
  try {
    saved = localStorage.getItem(DOMAIN_KEY) as MediaKind | null
  } catch {
    saved = null
  }
  const activeDomain = ref<MediaKind>(
    saved && ['book', 'comic', 'music', 'video'].includes(saved) ? saved : 'book',
  )
  const lastDomainRoute = ref<Record<MediaKind, string>>({ ...DEFAULT_ROUTES })

  function setActiveDomain(next: MediaKind): void {
    activeDomain.value = next
    try {
      localStorage.setItem(DOMAIN_KEY, next)
    } catch {
      // ignore storage errors
    }
  }

  function getLastRoute(kind: MediaKind): string {
    return lastDomainRoute.value[kind] || `/${kind}`
  }

  function syncFromPath(fullPath: string): void {
    const cleanPath = fullPath.split('?')[0] ?? fullPath
    if (cleanPath === '/book' || cleanPath.startsWith('/book/')) {
      setActiveDomain('book')
      if (
        !cleanPath.startsWith('/book/read/') &&
        cleanPath !== '/book/online/read' &&
        !cleanPath.startsWith('/book/online/read/')
      ) {
        lastDomainRoute.value.book = fullPath
      }
    } else if (cleanPath === '/comic' || cleanPath.startsWith('/comic/')) {
      setActiveDomain('comic')
      if (
        !cleanPath.startsWith('/comic/read/') &&
        cleanPath !== '/comic/online/read' &&
        !cleanPath.startsWith('/comic/online/read/')
      ) {
        lastDomainRoute.value.comic = fullPath
      }
    } else if (cleanPath === '/music' || cleanPath.startsWith('/music/')) {
      setActiveDomain('music')
      lastDomainRoute.value.music = fullPath
    } else if (cleanPath === '/video' || cleanPath.startsWith('/video/')) {
      setActiveDomain('video')
      if (
        !cleanPath.startsWith('/video/watch/') &&
        cleanPath !== '/video/online/watch' &&
        !cleanPath.startsWith('/video/online/watch/')
      ) {
        lastDomainRoute.value.video = fullPath
      }
    }
  }

  function resetRoutes(): void {
    activeDomain.value = 'book'
    lastDomainRoute.value = { ...DEFAULT_ROUTES }
    try {
      localStorage.removeItem(DOMAIN_KEY)
    } catch {
      // ignore storage errors
    }
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
