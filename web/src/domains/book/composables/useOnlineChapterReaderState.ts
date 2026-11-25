import { computed, onBeforeUnmount, onMounted, ref, type Ref } from 'vue'
import type { OnlineChapterItem, TrackedChapterMetaDto } from '@/domains/book/onlineApi'
import {
  useContinuousChapterReader,
  type ContinuousChapterSection,
  type UseContinuousChapterReaderOptions,
} from '@/domains/book/readers/useContinuousChapterReader'
import type { TocItem } from '@/domains/book/types'

export function parseChapterIndexFromLocator(
  locator: string | null | undefined,
  fallbackIndex?: number | null,
): number {
  if (typeof fallbackIndex === 'number' && Number.isFinite(fallbackIndex) && fallbackIndex >= 0) {
    return Math.floor(fallbackIndex)
  }
  if (locator) {
    const match = locator.match(/^chapter:(\d+)/)
    if (match) return Number.parseInt(match[1]!, 10)
    const numeric = Number.parseInt(locator, 10)
    if (Number.isFinite(numeric) && numeric >= 0) return numeric
  }
  return 0
}

export function trackedChaptersToTocItems(list: readonly TrackedChapterMetaDto[]): TocItem[] {
  return list.map((ch) => ({
    index: ch.index,
    title: ch.title,
    href: `chapter:${ch.index}`,
    level: 0,
  }))
}

export function onlineChaptersToTocItems(list: readonly OnlineChapterItem[]): TocItem[] {
  let hasSeenVolume = false
  return list.map((ch, pos) => {
    if (ch.is_volume) {
      hasSeenVolume = true
      return {
        index: pos,
        title: ch.title,
        href: '',
        level: 0,
      }
    }
    return {
      index: pos,
      title: ch.title,
      href: ch.url ? `chapter:${pos}` : '',
      level: hasSeenVolume ? 1 : 0,
    }
  })
}

export interface UseOnlineChapterReaderStateOptions {
  scrollContainerRef: Ref<HTMLElement | null>
  getPrevPos: (fromPos: number) => number | null
  getNextPos: (fromPos: number) => number | null
  fetchChapter: (
    pos: number,
    options: { forceRefresh: boolean },
  ) => Promise<ContinuousChapterSection | null>
  onPrefetch?: (pos: number) => void
  onProgress?: UseContinuousChapterReaderOptions['onProgress']
  progressDebounceMs?: number
}

export function useOnlineChapterReaderState(options: UseOnlineChapterReaderStateOptions) {
  const loadingToc = ref(false)
  const tocError = ref<string | null>(null)
  let tocSeq = 0
  let unmounted = false

  const reader = useContinuousChapterReader({
    scrollContainerRef: options.scrollContainerRef,
    isBlocked: () => loadingToc.value,
    getPrevPos: options.getPrevPos,
    getNextPos: options.getNextPos,
    fetchChapter: options.fetchChapter,
    onPrefetch: options.onPrefetch,
    onProgress: options.onProgress,
    progressDebounceMs: options.progressDebounceMs ?? 250,
  })

  const combinedLoading = computed(() => loadingToc.value || reader.loadingChapter.value)
  const combinedErrorMsg = computed(() => tocError.value || reader.errorMsg.value)

  function beginTocLoad(): number {
    const seq = ++tocSeq
    reader.resetState()
    loadingToc.value = true
    tocError.value = null
    return seq
  }

  function bumpTocSeq(): number {
    tocSeq += 1
    return tocSeq
  }

  function isCurrentTocSeq(seq: number): boolean {
    return !unmounted && seq === tocSeq
  }

  function isUnmounted(): boolean {
    return unmounted
  }

  onMounted(() => {
    unmounted = false
  })

  onBeforeUnmount(() => {
    unmounted = true
    tocSeq += 1
  })

  return {
    reader,
    loadingToc,
    tocError,
    combinedLoading,
    combinedErrorMsg,
    beginTocLoad,
    bumpTocSeq,
    isCurrentTocSeq,
    isUnmounted,
  }
}
