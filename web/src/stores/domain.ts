import { ref } from 'vue'
import { defineStore } from 'pinia'
import { isMediaKind } from '@/sources/routes'
import { readStoredPreferencesRaw } from '@/theme/preferencesStorage'
import type { MediaKind } from '@/types/api'

const DOMAIN_KEY = 'xmedia.active_domain'

function readInitialSavedDomain(): MediaKind {
  try {
    const saved = localStorage.getItem(DOMAIN_KEY)
    if (isMediaKind(saved)) return saved
  } catch {
    // ignore storage errors
  }
  const prefs = readStoredPreferencesRaw()
  if (isMediaKind(prefs.active_domain)) return prefs.active_domain
  return 'book'
}

function hasExplicitDomainInWindowPath(): boolean {
  if (typeof window === 'undefined') return false
  const seg = window.location.pathname.split('/').filter(Boolean)[0]
  return isMediaKind(seg)
}

export const useDomainStore = defineStore('domain', () => {
  const activeDomain = ref<MediaKind>(readInitialSavedDomain())

  function setActiveDomain(next: MediaKind): void {
    activeDomain.value = next
    try {
      localStorage.setItem(DOMAIN_KEY, next)
    } catch {
      // ignore storage errors
    }
  }

  function hydratePreferredDomain(preferred?: MediaKind | null): void {
    if (!isMediaKind(preferred)) return
    if (hasExplicitDomainInWindowPath()) return
    setActiveDomain(preferred)
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
    // Retain locally remembered domain unless overwritten by incoming user preferences on login.
    activeDomain.value = readInitialSavedDomain()
  }

  return {
    activeDomain,
    setActiveDomain,
    hydratePreferredDomain,
    getLastRoute,
    syncFromPath,
    resetRoutes,
  }
})
