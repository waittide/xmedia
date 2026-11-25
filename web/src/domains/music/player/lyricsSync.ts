import { computed, ref, watch, type ComputedRef, type Ref } from 'vue'
import { api } from '../api'
import { onlineMusicApi } from '../onlineApi'
import type { StructuredLyricsDto } from '../types'
import type { PlayerQueueItem } from './queueHelpers'

export function createLyricsSyncController(options: {
  currentItem: ComputedRef<PlayerQueueItem | null>
  currentTimeSecs: Ref<number>
  onlineResolving: Ref<boolean>
}) {
  const lyricsOpen = ref(false)
  const lyricsLoading = ref(false)
  const lyrics = ref<StructuredLyricsDto | null>(null)
  const translatedLyrics = ref<StructuredLyricsDto | null>(null)

  let lyricsAbort: AbortController | null = null
  let lyricsLoadedTrackId: string | null = null

  function abortLyricsFetch(): void {
    lyricsAbort?.abort()
    lyricsAbort = null
  }

  const activeLyricIndex = computed<number>(() => {
    const lyr = lyrics.value
    if (!lyr || !lyr.synced || lyr.lines.length === 0) return -1
    const nowMs = options.currentTimeSecs.value * 1000 + 120 + (lyr.offset_ms || 0)
    let idx = -1
    for (let i = 0; i < lyr.lines.length; i++) {
      if (lyr.lines[i]!.start_ms <= nowMs) {
        idx = i
      } else {
        break
      }
    }
    return idx
  })

  function isLoadedFor(targetKey: string): boolean {
    return lyricsLoadedTrackId === targetKey
  }

  function prepareForTrackSwitch(targetKey: string): void {
    if (lyricsLoadedTrackId !== targetKey) {
      abortLyricsFetch()
    }
  }

  function clearForPendingTrack(targetKey: string): void {
    if (lyricsLoadedTrackId !== targetKey) {
      lyrics.value = null
      translatedLyrics.value = null
      lyricsLoadedTrackId = null
      lyricsLoading.value = false
    }
  }

  function markNoLyricsForKey(targetKey: string): void {
    if (lyricsLoadedTrackId !== targetKey) {
      lyrics.value = null
      translatedLyrics.value = null
      lyricsLoadedTrackId = targetKey
      lyricsLoading.value = false
    }
  }

  function applyResolvedLyrics(
    targetKey: string,
    nextLyrics: StructuredLyricsDto | null | undefined,
    nextTranslated: StructuredLyricsDto | null | undefined,
  ): void {
    lyrics.value = nextLyrics ?? null
    translatedLyrics.value = nextTranslated ?? null
    lyricsLoadedTrackId = targetKey
    lyricsLoading.value = false
  }

  function resetLyrics(closePanel = false): void {
    abortLyricsFetch()
    if (closePanel) {
      lyricsOpen.value = false
    }
    lyricsLoading.value = false
    lyrics.value = null
    translatedLyrics.value = null
    lyricsLoadedTrackId = null
  }

  async function fetchLyricsFor(item: PlayerQueueItem): Promise<void> {
    abortLyricsFetch()
    const targetKey = item.key
    if (item.kind === 'remote') {
      if (options.currentItem.value?.key === targetKey) {
        applyResolvedLyrics(targetKey, null, null)
      }
      return
    }
    if (item.kind === 'online' && item.resolved) {
      if (options.currentItem.value?.key === targetKey) {
        applyResolvedLyrics(targetKey, item.resolved.lyrics, item.resolved.translated_lyrics)
      }
      return
    }

    const controller = new AbortController()
    lyricsAbort = controller
    lyricsLoading.value = true
    try {
      const res =
        item.kind === 'local'
          ? await api.getTrackLyrics(item.track.id, { signal: controller.signal })
          : await onlineMusicApi.fetchLyrics(
              {
                source_id: item.source_id,
                track_id: item.track.track_id || null,
                lyric_id: item.track.lyric_id || null,
                title: item.track.title || null,
                artist: item.track.artist || null,
                detail_url: item.track.detail_url || null,
              },
              { signal: controller.signal },
            )
      if (controller.signal.aborted) return
      if (options.currentItem.value?.key === targetKey) {
        applyResolvedLyrics(targetKey, res, null)
      }
    } catch (err) {
      if (controller.signal.aborted || (err instanceof Error && err.name === 'AbortError')) return
      if (options.currentItem.value?.key === targetKey) {
        applyResolvedLyrics(targetKey, null, null)
      }
    } finally {
      if (lyricsAbort === controller) {
        lyricsAbort = null
      }
      if (!controller.signal.aborted && options.currentItem.value?.key === targetKey) {
        lyricsLoading.value = false
      }
    }
  }

  watch(lyricsOpen, (isOpen) => {
    const item = options.currentItem.value
    if (
      isOpen &&
      item &&
      lyricsLoadedTrackId !== item.key &&
      !lyricsLoading.value &&
      !(item.kind === 'online' && options.onlineResolving.value)
    ) {
      void fetchLyricsFor(item)
    }
  })

  return {
    lyricsOpen,
    lyricsLoading,
    lyrics,
    translatedLyrics,
    activeLyricIndex,
    isLoadedFor,
    prepareForTrackSwitch,
    clearForPendingTrack,
    markNoLyricsForKey,
    applyResolvedLyrics,
    abortLyricsFetch,
    fetchLyricsFor,
    resetLyrics,
  }
}
