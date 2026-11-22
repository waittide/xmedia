import { ref } from 'vue'
import { defineStore } from 'pinia'
import { isMediaKind } from '@/sources/routes'
import type { MediaKind } from '@/types/api'

const DOMAIN_KEY = 'xmedia.active_domain'

export const useDomainStore = defineStore('domain', () => {
  let saved: string | null = null
  try {
    saved = localStorage.getItem(DOMAIN_KEY)
  } catch {
    saved = null
  }
  const activeDomain = ref<MediaKind>(isMediaKind(saved) ? saved : 'book')

  function setActiveDomain(next: MediaKind): void {
    activeDomain.value = next
    try {
      localStorage.setItem(DOMAIN_KEY, next)
    } catch {
      // ignore storage errors
    }
  }

  /**
   * Switching domains always enters that domain's canonical home page (`/${kind}`)
   * rather than restoring stale filtered sub-routes (see IA contract §1.2).
   */
  function getLastRoute(kind: MediaKind): string {
    return `/${kind}`
  }

  function syncFromPath(fullPath: string): void {
    const cleanPath = (fullPath.split('?')[0] ?? fullPath).split('#')[0] ?? fullPath
    const firstSegment = cleanPath.split('/').filter(Boolean)[0]
    if (isMediaKind(firstSegment)) {
      setActiveDomain(firstSegment)
    }
  }

  function resetRoutes(): void {
    activeDomain.value = 'book'
    try {
      localStorage.removeItem(DOMAIN_KEY)
    } catch {
      // ignore storage errors
    }
  }

  return {
    activeDomain,
    setActiveDomain,
    getLastRoute,
    syncFromPath,
    resetRoutes,
  }
})
