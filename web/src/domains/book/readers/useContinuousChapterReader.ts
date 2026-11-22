import {
  computed,
  nextTick,
  onBeforeUnmount,
  onMounted,
  ref,
  type ComputedRef,
  type Ref,
} from 'vue'

export const CONTINUOUS_AUTO_LOAD_THRESHOLD_PX = 400

export interface ContinuousChapterSection {
  pos: number
  chapterIndex: number
  title: string
  volumeTitle?: string | null
  paragraphs: string[]
  cached?: boolean
}

export interface UseContinuousChapterReaderOptions {
  scrollContainerRef: Ref<HTMLElement | null>
  /** Return total readable/navigable position count or max index info */
  getPrevPos: (fromPos: number) => number | null
  getNextPos: (fromPos: number) => number | null
  /** Fetch and normalize a chapter at `pos`. */
  fetchChapter: (
    pos: number,
    opts: { forceRefresh: boolean },
  ) => Promise<ContinuousChapterSection | null>
  /** Called right after opening or scrolling into a chapter to prefetch upcoming chapters. */
  onPrefetch?: (pos: number) => void
  /** Called when active chapter changes or scroll settles. */
  onProgress?: (info: {
    pos: number
    chapter: ContinuousChapterSection
    scrollRatio: number
    immediate: boolean
  }) => void
  /** Optional external guard disabling scroll/keyboard interactions (e.g. while TOC is loading). */
  isBlocked?: () => boolean
  /** Debounce ms for scroll progress reporting. Default: 220ms. */
  progressDebounceMs?: number
}

export interface UseContinuousChapterReaderReturn {
  activePos: Ref<number>
  loadedChapters: Ref<ContinuousChapterSection[]>
  loadingChapter: Ref<boolean>
  appendingNext: Ref<boolean>
  appendError: Ref<string | null>
  errorMsg: Ref<string | null>
  activeChapter: ComputedRef<ContinuousChapterSection | null>
  prevPos: ComputedRef<number | null>
  nextPos: ComputedRef<number | null>
  nextAppendablePos: ComputedRef<number | null>
  openChapter: (
    pos: number,
    opts?: { forceRefresh?: boolean; scrollRatio?: number; silentLoading?: boolean },
  ) => Promise<void>
  appendNextChapter: () => Promise<void>
  handleScroll: () => void
  flushProgress: () => void
  resetState: () => void
}

export function parseTextParagraphs(content: string): string[] {
  return content
    .split(/\r?\n+/)
    .map((line) => line.trim())
    .filter((line) => line.length > 0)
}

function isEditableTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false
  return (
    target.tagName === 'INPUT' ||
    target.tagName === 'TEXTAREA' ||
    target.tagName === 'SELECT' ||
    target.isContentEditable
  )
}

export function useContinuousChapterReader(
  options: UseContinuousChapterReaderOptions,
): UseContinuousChapterReaderReturn {
  const debounceMs = options.progressDebounceMs ?? 220

  const activePos = ref(0)
  const loadedChapters = ref<ContinuousChapterSection[]>([])
  const loadingChapter = ref(false)
  const appendingNext = ref(false)
  const appendError = ref<string | null>(null)
  const errorMsg = ref<string | null>(null)

  let loadSeq = 0
  let unmounted = false
  let scrollTimer: ReturnType<typeof setTimeout> | null = null

  const activeChapter = computed<ContinuousChapterSection | null>(
    () =>
      loadedChapters.value.find((c) => c.pos === activePos.value) ??
      loadedChapters.value[0] ??
      null,
  )

  const prevPos = computed<number | null>(() => options.getPrevPos(activePos.value))
  const nextPos = computed<number | null>(() => options.getNextPos(activePos.value))

  const lastLoadedPos = computed<number | null>(() => {
    const last = loadedChapters.value[loadedChapters.value.length - 1]
    return last ? last.pos : null
  })

  const nextAppendablePos = computed<number | null>(() =>
    lastLoadedPos.value === null ? nextPos.value : options.getNextPos(lastLoadedPos.value),
  )

  function clearScrollTimer(): void {
    if (scrollTimer !== null) {
      clearTimeout(scrollTimer)
      scrollTimer = null
    }
  }

  function computeActiveChapterScrollRatio(): number {
    const el = options.scrollContainerRef.value
    const cur = activeChapter.value
    if (!el || !cur) return 0
    const activeSection = el.querySelector<HTMLElement>(`[data-chapter-pos="${cur.pos}"]`)
    if (activeSection) {
      const containerRect = el.getBoundingClientRect()
      const secRect = activeSection.getBoundingClientRect()
      const scrollable = Math.max(1, secRect.height - containerRect.height)
      const scrolledInside = containerRect.top - secRect.top
      return Math.min(1, Math.max(0, scrolledInside / scrollable))
    }
    const maxScroll = el.scrollHeight - el.clientHeight
    return maxScroll > 0 ? Math.min(1, Math.max(0, el.scrollTop / maxScroll)) : 0
  }

  function reportProgress(explicitRatio?: number, immediate = false): void {
    if (unmounted) return
    const cur = activeChapter.value
    if (!cur) return
    const ratio = explicitRatio !== undefined ? explicitRatio : computeActiveChapterScrollRatio()
    const clamped = Math.min(1, Math.max(0, Number(ratio.toFixed(4))))
    options.onProgress?.({
      pos: cur.pos,
      chapter: cur,
      scrollRatio: clamped,
      immediate,
    })
  }

  function flushProgress(): void {
    if (scrollTimer !== null) {
      clearScrollTimer()
      reportProgress(undefined, true)
    }
  }

  function resetState(): void {
    clearScrollTimer()
    loadSeq += 1
    activePos.value = 0
    loadedChapters.value = []
    loadingChapter.value = false
    appendingNext.value = false
    appendError.value = null
    errorMsg.value = null
  }

  async function openChapter(
    pos: number,
    opts: { forceRefresh?: boolean; scrollRatio?: number; silentLoading?: boolean } = {},
  ): Promise<void> {
    clearScrollTimer()
    const seq = ++loadSeq
    activePos.value = pos
    loadedChapters.value = []
    appendingNext.value = false
    appendError.value = null
    errorMsg.value = null
    if (!opts.silentLoading) {
      loadingChapter.value = true
    }

    try {
      const section = await options.fetchChapter(pos, {
        forceRefresh: Boolean(opts.forceRefresh),
      })
      if (unmounted || seq !== loadSeq) return
      if (!section) {
        loadingChapter.value = false
        return
      }

      activePos.value = section.pos
      loadedChapters.value = [section]
      loadingChapter.value = false

      await nextTick()
      if (unmounted || seq !== loadSeq) return

      const el = options.scrollContainerRef.value
      const targetRatio = opts.scrollRatio ?? 0
      if (el) {
        const maxScroll = el.scrollHeight - el.clientHeight
        el.scrollTop = targetRatio > 0 && maxScroll > 0 ? Math.round(maxScroll * targetRatio) : 0
      }

      reportProgress(targetRatio, true)
      options.onPrefetch?.(section.pos)

      // Auto-append one chapter if a short non-empty chapter doesn't fill the viewport
      const upcoming = options.getNextPos(section.pos)
      if (
        section.paragraphs.length > 0 &&
        upcoming !== null &&
        el &&
        el.scrollHeight <= el.clientHeight + 120
      ) {
        void appendNextChapter()
      }
    } catch (err) {
      if (unmounted || seq !== loadSeq) return
      errorMsg.value = err instanceof Error ? err.message : String(err)
    } finally {
      if (!unmounted && seq === loadSeq) {
        loadingChapter.value = false
      }
    }
  }

  async function appendNextChapter(): Promise<void> {
    const targetPos = nextAppendablePos.value
    if (targetPos === null) return
    if (options.isBlocked?.() || loadingChapter.value || appendingNext.value) return
    if (loadedChapters.value.some((c) => c.pos === targetPos)) return

    const seq = loadSeq
    appendingNext.value = true
    appendError.value = null
    try {
      const section = await options.fetchChapter(targetPos, { forceRefresh: false })
      if (unmounted || seq !== loadSeq || !section) return
      if (loadedChapters.value.some((c) => c.pos === section.pos)) return

      loadedChapters.value.push(section)
      options.onPrefetch?.(section.pos)
    } catch (err) {
      if (unmounted || seq !== loadSeq) return
      appendError.value = err instanceof Error ? err.message : String(err)
    } finally {
      if (!unmounted && seq === loadSeq) {
        appendingNext.value = false
      }
    }
  }

  function handleScroll(): void {
    const el = options.scrollContainerRef.value
    if (!el || options.isBlocked?.() || loadingChapter.value || loadedChapters.value.length === 0) {
      return
    }

    const containerRect = el.getBoundingClientRect()
    const anchorY = containerRect.top + Math.min(200, containerRect.height * 0.35)
    const sections = el.querySelectorAll<HTMLElement>('[data-chapter-pos]')
    let visiblePos = loadedChapters.value[0]!.pos

    for (const sec of sections) {
      const rect = sec.getBoundingClientRect()
      if (rect.top <= anchorY) {
        const parsed = Number.parseInt(sec.dataset.chapterPos ?? '', 10)
        if (Number.isFinite(parsed)) {
          visiblePos = parsed
        }
      } else {
        break
      }
    }

    if (visiblePos !== activePos.value) {
      activePos.value = visiblePos
      options.onPrefetch?.(visiblePos)
    }

    const remaining = el.scrollHeight - el.scrollTop - el.clientHeight
    if (
      remaining <= CONTINUOUS_AUTO_LOAD_THRESHOLD_PX &&
      !appendingNext.value &&
      !appendError.value &&
      nextAppendablePos.value !== null
    ) {
      void appendNextChapter()
    }

    clearScrollTimer()
    scrollTimer = setTimeout(() => {
      scrollTimer = null
      reportProgress(undefined, false)
    }, debounceMs)
  }

  function handleKeydown(ev: KeyboardEvent): void {
    if (ev.metaKey || ev.ctrlKey || ev.altKey) return
    if (options.isBlocked?.() || loadingChapter.value) return
    if (isEditableTarget(ev.target)) return
    if (ev.key === 'ArrowLeft' && prevPos.value !== null) {
      ev.preventDefault()
      void openChapter(prevPos.value)
    } else if (ev.key === 'ArrowRight' && nextPos.value !== null) {
      ev.preventDefault()
      void openChapter(nextPos.value)
    }
  }

  onMounted(() => {
    unmounted = false
    window.addEventListener('keydown', handleKeydown)
  })

  onBeforeUnmount(() => {
    flushProgress()
    unmounted = true
    loadSeq += 1
    window.removeEventListener('keydown', handleKeydown)
  })

  return {
    activePos,
    loadedChapters,
    loadingChapter,
    appendingNext,
    appendError,
    errorMsg,
    activeChapter,
    prevPos,
    nextPos,
    nextAppendablePos,
    openChapter,
    appendNextChapter,
    handleScroll,
    flushProgress,
    resetState,
  }
}
