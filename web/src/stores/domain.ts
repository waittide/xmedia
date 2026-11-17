import { ref } from 'vue'
import { defineStore } from 'pinia'
import type { MediaKind } from '@/types/api'

const DOMAIN_KEY = 'xmedia.active_domain'

export const useDomainStore = defineStore('domain', () => {
  const saved = localStorage.getItem(DOMAIN_KEY) as MediaKind | null
  const activeDomain = ref<MediaKind>(
    saved && ['book', 'comic', 'music', 'video'].includes(saved) ? saved : 'book',
  )

  function setActiveDomain(next: MediaKind): void {
    activeDomain.value = next
    localStorage.setItem(DOMAIN_KEY, next)
  }

  function syncFromPath(path: string): void {
    if (path.startsWith('/book')) setActiveDomain('book')
    else if (path.startsWith('/comic')) setActiveDomain('comic')
    else if (path.startsWith('/music')) setActiveDomain('music')
    else if (path.startsWith('/video')) setActiveDomain('video')
  }

  return {
    activeDomain,
    setActiveDomain,
    syncFromPath,
  }
})
