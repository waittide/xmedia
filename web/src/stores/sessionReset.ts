import { clearHomeCache } from '@/config/homeCache'
import { useCatalogStore } from '@/domains/book/store'
import { useSidebarStore as useComicSidebarStore } from '@/domains/comic/store'
import { usePlayerStore } from '@/domains/music/player'
import { useMusicStore } from '@/domains/music/store'
import { useVideoStore } from '@/domains/video/store'
import { useDomainStore } from '@/stores/domain'
import { useEventsStore } from '@/stores/events'
import { useUploadStore } from '@/stores/uploads'

export function resetAllSessionState(): void {
  clearHomeCache()
  useEventsStore().disconnect()
  usePlayerStore().resetForLogout()
  useUploadStore().resetState()
  useDomainStore().resetRoutes()
  useCatalogStore().resetState()
  useComicSidebarStore().resetState()
  useMusicStore().resetState()
  useVideoStore().resetState()
}
