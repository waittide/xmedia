import { clearHomeCache } from '@/config/homeCache'
import { useCatalogStore } from '@/domains/book/store'
import { useSidebarStore as useComicSidebarStore } from '@/domains/comic/store'
import { usePlayerStore } from '@/domains/music/player'
import { useMusicStore } from '@/domains/music/store'
import { useVideoStore } from '@/domains/video/store'
import { useDialogStore } from '@/stores/dialog'
import { useDomainStore } from '@/stores/domain'
import { useEventsStore } from '@/stores/events'
import { useOnlineDownloadsStore } from '@/stores/onlineDownloads'
import { usePreferencesStore } from '@/stores/preferences'
import { useSnackbarStore } from '@/stores/snackbar'
import { useUploadStore } from '@/stores/uploads'

const ONLINE_SESSION_CTX_KEYS = [
  'xmedia_online_reader_ctx',
  'xmedia_online_comic_ctx',
  'xmedia_online_comic_reader_ctx',
  'xmedia_online_video_ctx',
  'xmedia_online_video_player_ctx',
] as const

function clearScopedBrowserCaches(): void {
  try {
    for (const key of ONLINE_SESSION_CTX_KEYS) {
      sessionStorage.removeItem(key)
    }
  } catch {
    // ignore
  }
  try {
    const keysToRemove: string[] = []
    for (let i = 0; i < localStorage.length; i += 1) {
      const k = localStorage.key(i)
      if (k && k.startsWith('xmedia.epub.locations:')) {
        keysToRemove.push(k)
      }
    }
    for (const k of keysToRemove) {
      localStorage.removeItem(k)
    }
  } catch {
    // ignore
  }
}

export function resetAllSessionState(): void {
  clearHomeCache()
  clearScopedBrowserCaches()
  useDialogStore().resetState()
  useSnackbarStore().resetState()
  usePreferencesStore().resetState()
  useEventsStore().disconnect()
  usePlayerStore().resetForLogout()
  useUploadStore().resetState()
  useOnlineDownloadsStore().resetState()
  useDomainStore().resetRoutes()
  useCatalogStore().resetState()
  useComicSidebarStore().resetState()
  useMusicStore().resetState()
  useVideoStore().resetState()
}
