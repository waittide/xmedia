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

export function resetAllSessionState(): void {
  clearHomeCache()
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
