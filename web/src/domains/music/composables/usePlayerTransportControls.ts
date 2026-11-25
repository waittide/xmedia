import { computed, ref } from 'vue'
import { useSnackbarStore } from '@/stores/snackbar'
import { api } from '../api'
import { downloadQueueItem } from '../download'
import { useMusicI18n } from '../i18n'
import { usePlayerStore } from '../player'
import { useMusicStore } from '../store'
import type { TranscodeProfile } from '../types'

export function usePlayerTransportControls() {
  const { t } = useMusicI18n()
  const snackbar = useSnackbarStore()
  const playerStore = usePlayerStore()
  const musicStore = useMusicStore()

  const scrubPreviewSecs = ref<number | null>(null)
  const savingBookmark = ref(false)
  const downloadingCurrent = ref(false)

  const displayTimeSecs = computed(() => scrubPreviewSecs.value ?? playerStore.currentTimeSecs)
  const isCurrentStarred = computed(() => Boolean(playerStore.currentLocalTrack?.starred_at))

  const playModeTitle = computed(() => {
    if (playerStore.playMode === 'loop_one') return t('player.modeLoopOne')
    if (playerStore.playMode === 'shuffle') return t('player.modeShuffle')
    return t('player.modeSequence')
  })

  const qualityOptions = computed(() => [
    {
      value: 'raw' as TranscodeProfile,
      label: t('player.qualityRaw'),
      trailing:
        playerStore.currentLocalTrack && playerStore.transcodeProfile === 'raw'
          ? playerStore.qualitySummary
          : '',
    },
    { value: 'opus128' as TranscodeProfile, label: t('player.qualityOpus128'), trailing: '' },
    { value: 'mp3192' as TranscodeProfile, label: t('player.qualityMp3192'), trailing: '' },
    { value: 'mp3320' as TranscodeProfile, label: t('player.qualityMp3320'), trailing: '' },
    { value: 'aac256' as TranscodeProfile, label: t('player.qualityAac256'), trailing: '' },
  ])

  const onlineQualityOptions = computed(() => {
    const tr = playerStore.currentTrack
    if (!tr || tr.kind !== 'online') return []
    return tr.qualities.map((q) => ({
      value: q,
      label: /^\d+$/.test(q) ? `${q}k` : q.toUpperCase(),
    }))
  })

  const hasQualityMenuItems = computed(() => {
    if (playerStore.currentLocalTrack) return true
    return playerStore.isCurrentOnline && onlineQualityOptions.value.length > 0
  })

  async function handleToggleFavorite(): Promise<void> {
    if (playerStore.currentLocalTrack) {
      await musicStore.toggleTrackStar(playerStore.currentLocalTrack)
    }
  }

  async function saveCurrentBookmark(): Promise<void> {
    const tr = playerStore.currentLocalTrack
    if (!tr || savingBookmark.value) return
    savingBookmark.value = true
    try {
      await api.saveBookmark(tr.id, Math.round(playerStore.currentTimeSecs * 1000))
      snackbar.show({ message: t('player.bookmarkSaved') })
    } catch (err) {
      snackbar.show({
        message: err instanceof Error ? err.message : t('player.playbackError'),
        tone: 'error',
      })
    } finally {
      savingBookmark.value = false
    }
  }

  async function handleDownloadCurrent(): Promise<void> {
    const item = playerStore.currentItem
    if (!item || downloadingCurrent.value) return
    downloadingCurrent.value = true
    const title = playerStore.currentTrack?.title || ''
    try {
      await downloadQueueItem(item)
      snackbar.show({ message: t('online.downloadStartedToast', { title }) })
    } catch (err) {
      snackbar.show({
        message: t('online.downloadFailedToast', {
          message: err instanceof Error ? err.message : String(err),
        }),
        tone: 'error',
      })
    } finally {
      downloadingCurrent.value = false
    }
  }

  return {
    playerStore,
    scrubPreviewSecs,
    savingBookmark,
    downloadingCurrent,
    displayTimeSecs,
    isCurrentStarred,
    playModeTitle,
    qualityOptions,
    onlineQualityOptions,
    hasQualityMenuItems,
    handleToggleFavorite,
    saveCurrentBookmark,
    handleDownloadCurrent,
  }
}
