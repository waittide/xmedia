import { computed, nextTick, onBeforeUnmount, onMounted, reactive, ref, watch } from 'vue'
import type { ReaderFitMode, ReaderMode } from '@/domains/comic/types'
import { useThemeStore } from '@/stores/theme'

export interface UseComicStageControllerOptions {
  getChapterKey: () => string
  getTotalPages: () => number
  getInitialPage: () => number
  getReaderMode: () => ReaderMode
  getFitMode: () => ReaderFitMode
  getOffsetCoverPage: () => boolean
  getDimImages: () => boolean
  getPreloadPages: () => number
  getLoading: () => boolean
  getHasContent: () => boolean
  getPageSrc: (pageNum: number) => string
  getWebtoonContainer: () => HTMLDivElement | null
  onHideHud: () => void
  onShowOsd: (text: string) => void
  canAutoHideHud: () => boolean
  onPageChange: (payload: { page: number; highestInSpread: number; reachedEnd: boolean }) => void
}

export function preferredComicScrollBehavior(): ScrollBehavior {
  if (
    typeof window !== 'undefined' &&
    window.matchMedia?.('(prefers-reduced-motion: reduce)').matches
  ) {
    return 'auto'
  }
  return 'smooth'
}

export function useComicStageController(options: UseComicStageControllerOptions) {
  const theme = useThemeStore()

  const currentPage = ref(1)
  const zoomPercent = ref(100)
  const pendingNextChapterTap = ref(false)
  const isFullscreen = ref(false)

  const widePages = reactive<Record<number, boolean>>({})
  const naturalWidths = reactive<Record<number, number>>({})
  const brokenPages = reactive<Record<number, boolean>>({})
  const retryTokens = reactive<Record<number, number>>({})
  const preloadedSet = new Set<string>()

  let webtoonObserver: IntersectionObserver | null = null
  const visibleWebtoonPages = new Set<number>()
  let suppressWebtoonObserverUntil = 0
  let lastWebtoonScrollTop = 0
  let unmounted = false

  const safeTotalPages = computed(() => Math.max(1, options.getTotalPages()))
  const isSinglePage = computed(() => safeTotalPages.value <= 1)
  const isRtl = computed(() => options.getReaderMode() === 'rtl_double')

  const spreads = computed<number[][]>(() => {
    const total = safeTotalPages.value
    const mode = options.getReaderMode()
    if (mode === 'ltr_single' || mode === 'webtoon') {
      return Array.from({ length: total }, (_, i) => [i + 1])
    }

    const groups: number[][] = []
    const offsetCover = options.getOffsetCoverPage()
    let p = 1
    while (p <= total) {
      if (p === 1 && offsetCover) {
        groups.push([1])
        p += 1
        continue
      }
      if (widePages[p]) {
        groups.push([p])
        p += 1
        continue
      }
      if (p + 1 <= total && !widePages[p + 1]) {
        groups.push([p, p + 1])
        p += 2
      } else {
        groups.push([p])
        p += 1
      }
    }
    return groups
  })

  const currentSpreadIndex = computed(() => {
    const idx = spreads.value.findIndex((sp) => sp.includes(currentPage.value))
    return idx >= 0 ? idx : 0
  })

  const currentSpread = computed<number[]>(
    () => spreads.value[currentSpreadIndex.value] || [1],
  )

  const pageCounterText = computed(() => {
    const total = options.getTotalPages()
    if (currentSpread.value.length > 1) {
      return `${currentSpread.value[0]}-${currentSpread.value[1]} / ${total}`
    }
    return `${currentPage.value} / ${total}`
  })

  const eyeCareClass = computed(() => {
    if (!options.getDimImages()) return ''
    return theme.isDark
      ? 'brightness-[0.82] contrast-[0.95]'
      : 'brightness-[0.94] sepia-[0.14] contrast-[0.96]'
  })

  function pageImageStyle(pageNum: number): Record<string, string> {
    const scale = zoomPercent.value / 100
    const fit = options.getFitMode()
    if (fit === 'fit_height') {
      return {
        maxHeight: `${Math.round(100 * scale)}dvh`,
        width: 'auto',
        maxWidth:
          currentSpread.value.length > 1
            ? `${Math.round(50 * scale)}vw`
            : `${Math.round(100 * scale)}vw`,
      }
    }
    if (fit === 'fit_width') {
      return {
        width:
          currentSpread.value.length > 1
            ? `${Math.round(50 * scale)}vw`
            : `${Math.round(96 * scale)}vw`,
        height: 'auto',
      }
    }
    const natW = naturalWidths[pageNum]
    if (natW && natW > 0) {
      return {
        width: `${Math.round(natW * scale)}px`,
        height: 'auto',
        maxWidth: 'none',
      }
    }
    return {
      width: `${Math.round(100 * scale)}%`,
      height: 'auto',
      maxWidth: 'none',
    }
  }

  function pageSrc(pageNum: number): string {
    const base = options.getPageSrc(pageNum)
    if (!base) return ''
    const token = retryTokens[pageNum]
    if (!token || base.startsWith('data:')) return base
    const sep = base.includes('?') ? '&' : '?'
    return `${base}${sep}_retry=${token}`
  }

  function teardownWebtoonObserver(): void {
    if (webtoonObserver) {
      webtoonObserver.disconnect()
      webtoonObserver = null
    }
    visibleWebtoonPages.clear()
  }

  function resetStageState(): void {
    teardownWebtoonObserver()
    pendingNextChapterTap.value = false
    preloadedSet.clear()
    for (const k of Object.keys(widePages)) delete widePages[Number(k)]
    for (const k of Object.keys(naturalWidths)) delete naturalWidths[Number(k)]
    for (const k of Object.keys(brokenPages)) delete brokenPages[Number(k)]
    for (const k of Object.keys(retryTokens)) delete retryTokens[Number(k)]
  }

  function onImageLoaded(pageNum: number, ev: Event): void {
    const img = ev.target
    delete brokenPages[pageNum]
    if (img instanceof HTMLImageElement && img.naturalWidth > 0 && img.naturalHeight > 0) {
      naturalWidths[pageNum] = img.naturalWidth
      widePages[pageNum] = img.naturalWidth > img.naturalHeight * 1.12
    }
  }

  function onImageError(pageNum: number): void {
    brokenPages[pageNum] = true
  }

  function retryPageImage(pageNum: number): void {
    delete brokenPages[pageNum]
    retryTokens[pageNum] = (retryTokens[pageNum] || 0) + 1
  }

  function preloadAround(centerPage: number): void {
    const maxPage = options.getTotalPages()
    if (!options.getHasContent() || options.getLoading() || maxPage <= 0) return
    const ahead = Math.max(3, options.getPreloadPages() || 3)
    const keyPrefix = options.getChapterKey()

    const candidates: number[] = []
    for (let offset = 0; offset <= ahead + 1; offset++) {
      if (centerPage + offset <= maxPage) candidates.push(centerPage + offset)
    }
    if (centerPage - 1 >= 1) candidates.push(centerPage - 1)

    for (const p of candidates) {
      const src = options.getPageSrc(p)
      if (!src) continue
      const key = `${keyPrefix}:${p}`
      if (preloadedSet.has(key)) continue
      preloadedSet.add(key)
      const img = new Image()
      img.onload = () => {
        if (
          !unmounted &&
          img.naturalWidth > 0 &&
          img.naturalHeight > 0 &&
          options.getChapterKey() === keyPrefix
        ) {
          naturalWidths[p] = img.naturalWidth
          widePages[p] = img.naturalWidth > img.naturalHeight * 1.12
        }
      }
      img.onerror = () => {}
      img.src = src
    }
  }

  function emitPageChange(pageVal: number, forceReachedEnd = false): void {
    if (!options.getHasContent() || options.getLoading()) return
    const highestInSpread = Math.max(...currentSpread.value, pageVal)
    const reachedEnd = forceReachedEnd || highestInSpread >= options.getTotalPages()
    options.onPageChange({ page: pageVal, highestInSpread, reachedEnd })
  }

  function scrollToWebtoonPage(pageNum: number): void {
    const container = options.getWebtoonContainer()
    if (!container) return
    suppressWebtoonObserverUntil = Date.now() + 450
    const el = container.querySelector<HTMLElement>(`[data-page="${pageNum}"]`)
    if (el) {
      el.scrollIntoView({ behavior: 'instant', block: 'start' })
      lastWebtoonScrollTop = container.scrollTop
    }
  }

  function setupWebtoonObserver(): void {
    teardownWebtoonObserver()
    const container = options.getWebtoonContainer()
    if (
      !container ||
      options.getReaderMode() !== 'webtoon' ||
      options.getLoading() ||
      !options.getHasContent()
    ) {
      return
    }
    webtoonObserver = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) {
          const target = entry.target
          if (!(target instanceof HTMLElement)) continue
          const p = Number(target.dataset.page)
          if (!p) continue
          if (entry.isIntersecting) visibleWebtoonPages.add(p)
          else visibleWebtoonPages.delete(p)
        }
        if (Date.now() < suppressWebtoonObserverUntil) return
        if (visibleWebtoonPages.size > 0) {
          const minVisible = Math.min(...visibleWebtoonPages)
          if (minVisible !== currentPage.value) {
            currentPage.value = minVisible
          }
        }
      },
      {
        root: container,
        rootMargin: '-35% 0px -55% 0px',
        threshold: 0,
      },
    )
    const nodes = container.querySelectorAll<HTMLElement>('[data-page]')
    for (const node of nodes) {
      webtoonObserver.observe(node)
    }
  }

  function onWebtoonScroll(): void {
    const container = options.getWebtoonContainer()
    if (!container) return
    const cur = container.scrollTop
    if (Math.abs(cur - lastWebtoonScrollTop) > 48 && options.canAutoHideHud()) {
      options.onHideHud()
      lastWebtoonScrollTop = cur
    }
  }

  function adjustZoom(delta: number): void {
    const next = Math.max(50, Math.min(200, zoomPercent.value + delta))
    zoomPercent.value = next
    options.onShowOsd(`${next}%`)
  }

  function resetZoom(): void {
    zoomPercent.value = 100
    options.onShowOsd('100%')
  }

  async function toggleFullscreen(): Promise<void> {
    try {
      if (!document.fullscreenElement) {
        await document.documentElement.requestFullscreen()
      } else {
        await document.exitFullscreen()
      }
    } catch {
      // ignore fullscreen permission restrictions
    }
  }

  function handleFullscreenChange(): void {
    isFullscreen.value = Boolean(document.fullscreenElement)
  }

  watch(currentPage, (p) => {
    pendingNextChapterTap.value = false
    preloadAround(p)
    emitPageChange(p)
  })

  watch(
    () =>
      [
        options.getReaderMode(),
        options.getChapterKey(),
        options.getLoading(),
        options.getHasContent(),
      ] as const,
    async ([mode, newKey, isLoading, ready], oldVal) => {
      const oldMode = oldVal?.[0]
      const oldKey = oldVal?.[1]
      const oldLoading = oldVal?.[2]
      const oldReady = oldVal?.[3]

      const keyChanged = newKey !== oldKey
      const contentJustReady =
        keyChanged || oldVal === undefined || Boolean(oldLoading) || !oldReady

      if (keyChanged) {
        resetStageState()
      } else {
        teardownWebtoonObserver()
      }

      if (isLoading || !ready) return

      if (contentJustReady) {
        const targetInitial = Math.min(
          Math.max(1, options.getInitialPage() || 1),
          Math.max(1, options.getTotalPages()),
        )
        currentPage.value = targetInitial
        preloadAround(targetInitial)
        suppressWebtoonObserverUntil = Date.now() + 600
        emitPageChange(targetInitial)
      } else if (mode !== oldMode && mode === 'webtoon') {
        suppressWebtoonObserverUntil = Date.now() + 600
      }

      if (mode === 'webtoon' && newKey) {
        await nextTick()
        if (
          unmounted ||
          options.getChapterKey() !== newKey ||
          options.getReaderMode() !== 'webtoon' ||
          options.getLoading() ||
          !options.getHasContent()
        ) {
          return
        }
        scrollToWebtoonPage(currentPage.value)
        setupWebtoonObserver()
      }
    },
    { immediate: true },
  )

  onMounted(() => {
    unmounted = false
    isFullscreen.value = Boolean(document.fullscreenElement)
    document.addEventListener('fullscreenchange', handleFullscreenChange)
  })

  onBeforeUnmount(() => {
    unmounted = true
    document.removeEventListener('fullscreenchange', handleFullscreenChange)
    teardownWebtoonObserver()
    preloadedSet.clear()
  })

  return {
    currentPage,
    zoomPercent,
    pendingNextChapterTap,
    isFullscreen,
    brokenPages,
    safeTotalPages,
    isSinglePage,
    isRtl,
    spreads,
    currentSpreadIndex,
    currentSpread,
    pageCounterText,
    eyeCareClass,
    pageImageStyle,
    pageSrc,
    onImageLoaded,
    onImageError,
    retryPageImage,
    emitPageChange,
    scrollToWebtoonPage,
    onWebtoonScroll,
    adjustZoom,
    resetZoom,
    toggleFullscreen,
  }
}
