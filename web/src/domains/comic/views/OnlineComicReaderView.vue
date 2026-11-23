<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import ComicReaderShell, {
  type ComicReaderChapterItem,
} from '@/domains/comic/components/ComicReaderShell.vue'
import { useComicI18n as useI18n } from '@/domains/comic/i18n'
import {
  ONLINE_COMIC_READER_CTX_KEY,
  onlineComicApi,
  resolveOnlineComicCoverUrl,
  type ComicReadingDirection,
  type OnlineComicChapter,
  type OnlineComicPageDto,
  type OnlineComicReaderContextStorage,
} from '@/domains/comic/onlineApi'
import type { ReaderFitMode, ReaderMode } from '@/domains/comic/types'
import {
  itemRoute,
  libraryRoute,
  parseItemRef,
  playRoute,
  useOnlineItemState,
  type OnlineItemMetadataInput,
  type TrackedItemRef,
} from '@/sources'
import { useSnackbarStore } from '@/stores/snackbar'
import { usePreferencesStore } from '@/stores/preferences'

const { t } = useI18n()
const route = useRoute()
const router = useRouter()
const preferences = usePreferencesStore()
const snackbar = useSnackbarStore()

const ctx = ref<OnlineComicReaderContextStorage | null>(null)
const chapters = ref<OnlineComicChapter[]>([])
const pages = ref<OnlineComicPageDto[]>([])
const activeChapterIndex = ref(0)
const activeChapterUrl = ref('')
const activeSourceName = ref('')
const activeDirection = ref<ComicReadingDirection | null>(null)
const initialPage = ref(1)

const loading = ref(true)
const loadError = ref<string | null>(null)
const hasContent = ref(false)

const readerMode = ref<ReaderMode>(preferences.comic.default_reader_mode)
const fitMode = ref<ReaderFitMode>(preferences.comic.default_fit_mode)
const offsetCoverPage = ref<boolean>(preferences.comic.offset_cover_page)
const dimImages = ref<boolean>(preferences.comic.dim_images_in_dark)
const webtoonWidth = ref<number>(preferences.comic.webtoon_width)

let initSeq = 0
let loadSeq = 0
let detailSeq = 0
let pageAbortController: AbortController | null = null
let detailAbortController: AbortController | null = null
let unmounted = false
let userOverrodeMode = false
let syncingUrl = false
let syncUrlSeq = 0

function isAbortError(err: unknown): boolean {
  return err instanceof DOMException
    ? err.name === 'AbortError'
    : err instanceof Error && err.name === 'AbortError'
}

function readQueryParam(keys: readonly string[]): string {
  for (const k of keys) {
    const val = route.query[k]
    if (typeof val === 'string' && val.trim()) {
      return val.trim()
    }
  }
  return ''
}

function readStoredReaderCtx(): OnlineComicReaderContextStorage | null {
  try {
    const raw = sessionStorage.getItem(ONLINE_COMIC_READER_CTX_KEY)
    if (!raw) return null
    const parsed: unknown = JSON.parse(raw)
    if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) return null
    return parsed as OnlineComicReaderContextStorage
  } catch {
    return null
  }
}

function saveReaderCtxToStorage(next: OnlineComicReaderContextStorage): void {
  try {
    sessionStorage.setItem(ONLINE_COMIC_READER_CTX_KEY, JSON.stringify(next))
  } catch {
    // ignore storage errors
  }
}

function mapDirectionToReaderMode(dir: ComicReadingDirection | null | undefined): ReaderMode {
  if (dir === 'webtoon') return 'webtoon'
  if (dir === 'ltr') {
    return preferences.comic.default_reader_mode === 'ltr_double' ? 'ltr_double' : 'ltr_single'
  }
  if (dir === 'rtl') return 'rtl_double'
  return preferences.comic.default_reader_mode
}

function hydratePreferencesFromTheme(dir?: ComicReadingDirection | null): void {
  fitMode.value = preferences.comic.default_fit_mode
  offsetCoverPage.value = preferences.comic.offset_cover_page
  dimImages.value = preferences.comic.dim_images_in_dark
  webtoonWidth.value = preferences.comic.webtoon_width
  if (!userOverrodeMode) {
    readerMode.value = mapDirectionToReaderMode(dir)
  }
}

function parseExplicitPageQuery(): number | null {
  const raw = readQueryParam(['page'])
  if (!raw) return null
  const num = Number.parseInt(raw, 10)
  return Number.isFinite(num) && num >= 1 ? num : null
}

function hydrateContextFromRoute(): OnlineComicReaderContextStorage | null {
  const parsed = parseItemRef(route, 'comic')
  const parsedSourceId = parsed && parsed.scope !== 'local' ? parsed.sourceId : ''
  const parsedHints = parsed && parsed.scope !== 'local' ? parsed.hints : undefined

  const qSourceId = parsedSourceId || readQueryParam(['source', 'sourceId', 'source_id'])
  const qChapterUrl =
    parsedHints?.extra?.chapter || readQueryParam(['chapter', 'chapterUrl', 'chapter_url'])
  const rawRef = readQueryParam(['ref', 'comicUrl', 'comic_url'])
  // parseItemRef may fall back to chapterUrl when only chapter_url is provided (deep-link test)
  const qComicUrl = rawRef || (parsed && parsed.scope !== 'local' && parsed.ref !== qChapterUrl ? parsed.ref : '')
  const qTocUrl = parsedHints?.extra?.toc || readQueryParam(['toc', 'tocUrl', 'toc_url'])
  const qChapterIdxRaw = readQueryParam(['chapterIndex', 'chapter_index'])
  const qTitle = parsedHints?.title || readQueryParam(['title', 'name'])
  const qSourceName = readQueryParam(['sourceName', 'source_name'])
  const qAuthor = parsedHints?.author || readQueryParam(['author'])
  const qCoverUrl = parsedHints?.cover || readQueryParam(['cover', 'coverUrl', 'cover_url'])
  const qDirRaw = readQueryParam(['readingDirection', 'reading_direction'])
  const qDir: ComicReadingDirection | null =
    qDirRaw === 'rtl' || qDirRaw === 'ltr' || qDirRaw === 'webtoon' ? qDirRaw : null

  const stored = readStoredReaderCtx()
  const matchesStored = Boolean(
    stored &&
      ((!qSourceId &&
        !qComicUrl &&
        !qChapterUrl &&
        stored.sourceId &&
        (stored.chapterUrl || stored.comicUrl)) ||
        (qSourceId &&
          stored.sourceId === qSourceId &&
          (!qComicUrl || !stored.comicUrl || stored.comicUrl === qComicUrl))),
  )

  const sourceId = qSourceId || (matchesStored && stored ? stored.sourceId : '')
  const comicUrl = qComicUrl || (matchesStored && stored ? stored.comicUrl : '')
  const chapterUrl = qChapterUrl || (matchesStored && stored ? stored.chapterUrl : '')
  if (!sourceId || (!chapterUrl && !comicUrl)) {
    return null
  }

  const parsedIdx = qChapterIdxRaw ? Number.parseInt(qChapterIdxRaw, 10) : Number.NaN
  const storedChapters = matchesStored && Array.isArray(stored?.chapters) ? stored!.chapters : []
  let chapterIndex =
    Number.isFinite(parsedIdx) && parsedIdx >= 0
      ? parsedIdx
      : matchesStored && stored
        ? stored.chapterIndex
        : 0
  if (chapterUrl && storedChapters.length > 0) {
    const matchedIdx = storedChapters.findIndex((c) => c.url === chapterUrl)
    if (matchedIdx >= 0) chapterIndex = matchedIdx
  }

  return {
    sourceId,
    sourceName: qSourceName || (matchesStored ? stored?.sourceName : undefined),
    comicUrl,
    tocUrl: qTocUrl || (matchesStored ? stored?.tocUrl : undefined),
    title: qTitle || (matchesStored ? stored?.title : undefined),
    author: qAuthor || (matchesStored ? stored?.author : undefined),
    coverUrl: qCoverUrl || (matchesStored ? stored?.coverUrl : undefined),
    readingDirection: qDir || (matchesStored ? stored?.readingDirection : undefined) || null,
    chapterUrl,
    chapterIndex,
    chapterTitle:
      storedChapters[chapterIndex]?.title ||
      (matchesStored && stored?.chapterUrl === chapterUrl ? stored?.chapterTitle : undefined),
    chapters: storedChapters,
  }
}

// Persistent online reading progress via `/api/v1/comic/online/items`
const trackedRef = computed<TrackedItemRef | null>(() => {
  const c = ctx.value
  if (!c?.sourceId || !c.comicUrl) return null
  return {
    scope: 'online',
    domain: 'comic',
    sourceId: c.sourceId,
    ref: c.comicUrl,
    hints: {
      title: c.title,
      cover: c.coverUrl ?? undefined,
      author: c.author,
    },
  }
})

const onlineMeta = computed<OnlineItemMetadataInput>(() => {
  const c = ctx.value
  return {
    title: c?.title || activeSourceName.value || c?.comicUrl || 'Comic',
    subtitle: c?.author || null,
    coverUrl: c?.coverUrl ?? null,
    extra: {
      toc_url: c?.tocUrl || undefined,
      reading_direction: activeDirection.value || c?.readingDirection || undefined,
      source_name: activeSourceName.value || c?.sourceName || undefined,
    },
  }
})

const onlineState = useOnlineItemState(trackedRef, onlineMeta, {
  autoLoad: false,
  progressDebounceMs: 2500,
})

const readableChapterIndices = computed<number[]>(() => {
  const result: number[] = []
  chapters.value.forEach((ch, idx) => {
    if (!ch.is_volume && Boolean(ch.url?.trim())) {
      result.push(idx)
    }
  })
  return result
})

const currentReadablePos = computed(() => {
  const indices = readableChapterIndices.value
  if (indices.length === 0) return -1
  const byIdx = indices.indexOf(activeChapterIndex.value)
  if (byIdx >= 0) return byIdx
  if (activeChapterUrl.value) {
    const byUrl = indices.findIndex((idx) => chapters.value[idx]?.url === activeChapterUrl.value)
    if (byUrl >= 0) return byUrl
  }
  return -1
})

const prevChapterIndex = computed<number | null>(() => {
  const pos = currentReadablePos.value
  if (pos <= 0) return null
  return readableChapterIndices.value[pos - 1] ?? null
})

const nextChapterIndex = computed<number | null>(() => {
  const pos = currentReadablePos.value
  if (pos < 0 || pos + 1 >= readableChapterIndices.value.length) return null
  return readableChapterIndices.value[pos + 1] ?? null
})

const currentChapterObj = computed<OnlineComicChapter | null>(() => {
  if (chapters.value[activeChapterIndex.value]) {
    return chapters.value[activeChapterIndex.value] ?? null
  }
  if (activeChapterUrl.value) {
    return chapters.value.find((c) => c.url === activeChapterUrl.value) ?? null
  }
  return null
})

const nextChapterObj = computed<OnlineComicChapter | null>(() =>
  nextChapterIndex.value !== null ? (chapters.value[nextChapterIndex.value] ?? null) : null,
)

const activeChapterKey = computed(
  () => `${ctx.value?.sourceId ?? ''}:${activeChapterIndex.value}:${activeChapterUrl.value}`,
)

const chapterItems = computed<ComicReaderChapterItem[]>(() =>
  chapters.value.map((ch, idx) => ({
    key: `${ctx.value?.sourceId ?? ''}:${idx}:${ch.url}`,
    label: ch.is_volume ? `${t('online.volumeBadge')} · ${ch.title}` : `#${idx + 1} ${ch.title}`,
    metaLabel: ch.update_time ?? undefined,
    disabled: ch.is_volume || !ch.url,
  })),
)

const headerTitle = computed(() => ctx.value?.title || activeSourceName.value || '')
const headerSubtitle = computed(() => {
  const ch = currentChapterObj.value
  if (ch) return `#${activeChapterIndex.value + 1} ${ch.title}`
  return ctx.value?.chapterTitle || ''
})

function getPageSrc(pageNum: number): string {
  const page = pages.value[pageNum - 1]
  if (!page) return ''
  return resolveOnlineComicCoverUrl(page.proxy_url, page.image_url, ctx.value?.sourceId)
}

let lastReportedPage: { page: number; reachedEnd: boolean } | null = null

function buildCurrentComicProgress(): {
  unitIndex: number
  unitKey: string
  unitTitle: string
  position: number
  duration: number
  totalUnits?: number
  completed: boolean
} | null {
  if (!trackedRef.value || !activeChapterUrl.value || pages.value.length === 0 || !lastReportedPage) {
    return null
  }
  const unitTitle =
    currentChapterObj.value?.title ||
    ctx.value?.chapterTitle ||
    `#${activeChapterIndex.value + 1}`
  const isFinalChapter = chapters.value.length > 0 && nextChapterIndex.value === null
  return {
    unitIndex: activeChapterIndex.value,
    unitKey: activeChapterUrl.value,
    unitTitle,
    position: lastReportedPage.page,
    duration: pages.value.length,
    totalUnits: chapters.value.length > 0 ? chapters.value.length : undefined,
    completed: lastReportedPage.reachedEnd && isFinalChapter,
  }
}

function handlePageChange(payload: {
  page: number
  highestInSpread: number
  reachedEnd: boolean
}): void {
  lastReportedPage = { page: payload.page, reachedEnd: payload.reachedEnd }
  const pos = buildCurrentComicProgress()
  if (!pos) return
  onlineState.saveProgress(pos)
}

function flushComicProgressOnPageHide(): void {
  const pos = buildCurrentComicProgress()
  onlineState.flushProgressKeepalive(pos ?? undefined)
}

async function hydrateChaptersInBackground(
  sourceId: string,
  comicUrl: string,
  tocUrlHint?: string,
): Promise<void> {
  detailAbortController?.abort()
  const controller = new AbortController()
  detailAbortController = controller
  const seq = ++detailSeq
  try {
    const detailRes = await onlineComicApi.fetchDetail(
      sourceId,
      comicUrl,
      {
        title: ctx.value?.title,
        author: ctx.value?.author,
        comic_url: comicUrl,
        cover_url: ctx.value?.coverUrl ?? null,
      },
      { signal: controller.signal },
    )
    if (unmounted || seq !== detailSeq) return

    let loadedChapters = detailRes.chapters ?? []
    const effectiveTocUrl = detailRes.detail.toc_url?.trim() || tocUrlHint?.trim() || ''
    if (loadedChapters.length === 0 && effectiveTocUrl) {
      const tocRes = await onlineComicApi
        .fetchChapters(sourceId, effectiveTocUrl, detailRes.detail, { signal: controller.signal })
        .catch(() => null)
      if (unmounted || seq !== detailSeq) return
      if (tocRes) loadedChapters = tocRes.chapters ?? []
    }

    chapters.value = loadedChapters
    if (detailRes.source_name) {
      activeSourceName.value = detailRes.source_name
    }
    if (!activeDirection.value && detailRes.detail.reading_direction) {
      activeDirection.value = detailRes.detail.reading_direction
      if (!userOverrodeMode) {
        readerMode.value = mapDirectionToReaderMode(detailRes.detail.reading_direction)
      }
    }

    if (ctx.value) {
      const matchedIdx = loadedChapters.findIndex((c) => c.url === activeChapterUrl.value)
      if (matchedIdx >= 0) {
        activeChapterIndex.value = matchedIdx
      }
      const nextCtx: OnlineComicReaderContextStorage = {
        ...ctx.value,
        sourceName: activeSourceName.value || ctx.value.sourceName,
        title: detailRes.detail.title || ctx.value.title,
        author: detailRes.detail.author || ctx.value.author,
        coverUrl: detailRes.detail.cover_url ?? ctx.value.coverUrl,
        tocUrl: effectiveTocUrl || ctx.value.tocUrl,
        readingDirection: activeDirection.value ?? ctx.value.readingDirection,
        chapterIndex: activeChapterIndex.value,
        chapterTitle: loadedChapters[activeChapterIndex.value]?.title || ctx.value.chapterTitle,
        chapters: loadedChapters,
      }
      ctx.value = nextCtx
      saveReaderCtxToStorage(nextCtx)
    }
  } catch {
    // Background metadata enrichment must never disrupt direct chapter reading
  } finally {
    if (detailAbortController === controller) {
      detailAbortController = null
    }
  }
}

async function openChapter(
  targetChapterUrl: string,
  targetIndex: number,
  targetInitialPage = 1,
  seamlessToast = false,
): Promise<void> {
  if (!ctx.value) return
  pageAbortController?.abort()
  const controller = new AbortController()
  pageAbortController = controller
  const seq = ++loadSeq
  loading.value = true
  loadError.value = null
  hasContent.value = false
  activeChapterUrl.value = targetChapterUrl
  activeChapterIndex.value = targetIndex
  initialPage.value = Math.max(1, Math.floor(targetInitialPage))

  const chapterObj = chapters.value[targetIndex] ?? null
  const updatedCtx: OnlineComicReaderContextStorage = {
    ...ctx.value,
    chapterUrl: targetChapterUrl,
    chapterIndex: targetIndex,
    chapterTitle: chapterObj?.title || ctx.value.chapterTitle,
    chapters: chapters.value,
  }
  ctx.value = updatedCtx
  saveReaderCtxToStorage(updatedCtx)

  try {
    const pagesRes = await onlineComicApi.fetchPages(
      updatedCtx.sourceId,
      targetChapterUrl,
      updatedCtx.comicUrl || null,
      {
        comic: updatedCtx.title
          ? {
              title: updatedCtx.title,
              author: updatedCtx.author,
              comic_url: updatedCtx.comicUrl,
              toc_url: updatedCtx.tocUrl,
              cover_url: updatedCtx.coverUrl ?? null,
              reading_direction: updatedCtx.readingDirection ?? null,
            }
          : undefined,
        chapter: chapterObj ?? undefined,
        signal: controller.signal,
      },
    )
    if (unmounted || seq !== loadSeq) return

    if (pagesRes.source_name) {
      activeSourceName.value = pagesRes.source_name
    }
    const resolvedDir =
      pagesRes.reading_direction || activeDirection.value || updatedCtx.readingDirection
    activeDirection.value = resolvedDir ?? null
    if (!userOverrodeMode) {
      readerMode.value = mapDirectionToReaderMode(resolvedDir)
    }

    pages.value = pagesRes.pages ?? []
    if (pages.value.length === 0) {
      hasContent.value = false
      loadError.value = t('online.emptyPages')
      loading.value = false
      return
    }

    initialPage.value = Math.min(Math.max(1, initialPage.value), pages.value.length)
    hasContent.value = true
    loading.value = false

    if (seamlessToast) {
      const label = chapterObj?.title || `#${targetIndex + 1}`
      snackbar.show(t('reader.switchedToChapter', { title: label }))
    }
  } catch (err) {
    if (controller.signal.aborted || isAbortError(err) || unmounted || seq !== loadSeq) return
    pages.value = []
    hasContent.value = false
    loadError.value = err instanceof Error ? err.message : String(err)
    loading.value = false
  } finally {
    if (pageAbortController === controller) {
      pageAbortController = null
    }
  }
}

async function initReader(): Promise<void> {
  const runInitSeq = ++initSeq
  pageAbortController?.abort()
  pageAbortController = null
  detailAbortController?.abort()
  detailAbortController = null

  const hydrated = hydrateContextFromRoute()
  const seq = ++loadSeq
  loadError.value = null
  hasContent.value = false
  pages.value = []

  if (!hydrated) {
    ctx.value = null
    chapters.value = []
    loading.value = false
    loadError.value = t('online.invalidReaderContext')
    return
  }

  ctx.value = hydrated
  chapters.value = hydrated.chapters ?? []
  activeSourceName.value = hydrated.sourceName || ''
  activeDirection.value = hydrated.readingDirection ?? null
  activeChapterIndex.value = hydrated.chapterIndex
  activeChapterUrl.value = hydrated.chapterUrl
  hydratePreferencesFromTheme(activeDirection.value)

  // Hydrate persisted online progress (chapter + page) so reload/resume restores position
  const savedSnap = hydrated.comicUrl
    ? await onlineState.refresh(false).catch(() => null)
    : null
  if (unmounted || runInitSeq !== initSeq) return

  const savedProg = savedSnap?.progress ?? null
  const explicitPage = parseExplicitPageQuery()

  if (!hydrated.chapterUrl && hydrated.comicUrl) {
    const controller = new AbortController()
    detailAbortController = controller
    loading.value = true
    try {
      const detailRes = await onlineComicApi.fetchDetail(
        hydrated.sourceId,
        hydrated.comicUrl,
        undefined,
        { signal: controller.signal },
      )
      if (unmounted || seq !== loadSeq) return

      let loadedChapters = detailRes.chapters ?? []
      const effectiveTocUrl = detailRes.detail.toc_url?.trim() || hydrated.tocUrl?.trim() || ''
      if (loadedChapters.length === 0 && effectiveTocUrl) {
        const tocRes = await onlineComicApi.fetchChapters(
          hydrated.sourceId,
          effectiveTocUrl,
          detailRes.detail,
          { signal: controller.signal },
        )
        if (unmounted || seq !== loadSeq) return
        loadedChapters = tocRes.chapters ?? []
      }

      chapters.value = loadedChapters
      activeSourceName.value = detailRes.source_name || activeSourceName.value
      activeDirection.value = detailRes.detail.reading_direction || activeDirection.value

      let targetChapter: OnlineComicChapter | null = null
      let resumePage = explicitPage ?? 1

      if (savedProg?.unitKey) {
        const bySavedUrl = loadedChapters.find(
          (c) => !c.is_volume && c.url === savedProg.unitKey,
        )
        if (bySavedUrl) {
          targetChapter = bySavedUrl
          if (!explicitPage && savedProg.position >= 1) {
            resumePage = Math.floor(savedProg.position)
          }
        }
      }
      if (!targetChapter && savedProg?.unitIndex !== undefined) {
        const bySavedIdx = loadedChapters[savedProg.unitIndex]
        if (bySavedIdx && !bySavedIdx.is_volume && bySavedIdx.url) {
          targetChapter = bySavedIdx
          if (!explicitPage && savedProg.position >= 1) {
            resumePage = Math.floor(savedProg.position)
          }
        }
      }
      if (!targetChapter) {
        targetChapter =
          (loadedChapters[hydrated.chapterIndex] &&
          !loadedChapters[hydrated.chapterIndex]!.is_volume &&
          loadedChapters[hydrated.chapterIndex]!.url
            ? loadedChapters[hydrated.chapterIndex]!
            : loadedChapters.find((c) => !c.is_volume && Boolean(c.url?.trim()))) ?? null
      }

      if (!targetChapter) {
        loading.value = false
        loadError.value = t('online.emptyChapters')
        return
      }

      const resolvedIdx = loadedChapters.findIndex((c) => c.url === targetChapter!.url)
      ctx.value = {
        ...hydrated,
        sourceName: activeSourceName.value,
        title: detailRes.detail.title || hydrated.title,
        author: detailRes.detail.author || hydrated.author,
        coverUrl: detailRes.detail.cover_url ?? hydrated.coverUrl,
        tocUrl: effectiveTocUrl,
        readingDirection: activeDirection.value,
        chapterUrl: targetChapter.url,
        chapterIndex: resolvedIdx >= 0 ? resolvedIdx : 0,
        chapterTitle: targetChapter.title,
        chapters: loadedChapters,
      }
      await openChapter(targetChapter.url, resolvedIdx >= 0 ? resolvedIdx : 0, resumePage, false)
    } catch (err) {
      if (controller.signal.aborted || isAbortError(err) || unmounted || seq !== loadSeq) return
      loading.value = false
      loadError.value = err instanceof Error ? err.message : String(err)
    } finally {
      if (detailAbortController === controller) {
        detailAbortController = null
      }
    }
    return
  }

  let resolvedInitialPage = explicitPage ?? 1
  if (
    !explicitPage &&
    savedProg &&
    savedProg.position >= 1 &&
    (savedProg.unitKey === hydrated.chapterUrl ||
      (savedProg.unitKey === undefined && savedProg.unitIndex === hydrated.chapterIndex))
  ) {
    resolvedInitialPage = Math.floor(savedProg.position)
  }

  await openChapter(hydrated.chapterUrl, hydrated.chapterIndex, resolvedInitialPage, false)
  if (!unmounted && runInitSeq === initSeq && chapters.value.length === 0 && hydrated.comicUrl) {
    void hydrateChaptersInBackground(hydrated.sourceId, hydrated.comicUrl, hydrated.tocUrl)
  }
}

function switchChapterByIndex(targetIdx: number): void {
  const target = chapters.value[targetIdx]
  if (!target || target.is_volume || !target.url?.trim() || !ctx.value) return

  void onlineState.flushPendingProgress()

  const refObj: TrackedItemRef = {
    scope: 'online',
    domain: 'comic',
    sourceId: ctx.value.sourceId,
    ref: ctx.value.comicUrl || target.url,
    hints: {
      title: ctx.value.title,
      cover: ctx.value.coverUrl ?? undefined,
      author: ctx.value.author,
      extra: {
        chapter: target.url,
        ...(ctx.value.tocUrl ? { toc: ctx.value.tocUrl } : {}),
      },
    },
  }

  const seq = ++syncUrlSeq
  syncingUrl = true
  void router
    .replace(
      playRoute(refObj, {
        unitKey: target.url,
        unitIndex: targetIdx,
        unitTitle: target.title,
        position: 1,
      }),
    )
    .finally(() => {
      if (seq === syncUrlSeq) {
        syncingUrl = false
      }
    })
  void openChapter(target.url, targetIdx, 1, true)
}

function handleSelectChapterKey(key: string): void {
  const idx = chapterItems.value.findIndex((item) => item.key === key)
  if (idx < 0) return
  switchChapterByIndex(idx)
}

// Keep online reader settings session-scoped so one webtoon never overwrites global local manga defaults
function handleModeChange(mode: ReaderMode): void {
  userOverrodeMode = true
  readerMode.value = mode
}

function handleFitChange(fit: ReaderFitMode): void {
  fitMode.value = fit
}

function handleOffsetCoverChange(val: boolean): void {
  offsetCoverPage.value = val
}

function handleDimImagesChange(val: boolean): void {
  dimImages.value = val
}

function handleWebtoonWidthCommit(val: number): void {
  webtoonWidth.value = val
}

function handleRetry(): void {
  if (ctx.value && activeChapterUrl.value) {
    void openChapter(activeChapterUrl.value, activeChapterIndex.value, initialPage.value, false)
  } else {
    void initReader()
  }
}

function handleBack(): void {
  void onlineState.flushPendingProgress()
  if (ctx.value?.sourceId && ctx.value?.comicUrl) {
    void router.push(
      itemRoute({
        scope: 'online',
        domain: 'comic',
        sourceId: ctx.value.sourceId,
        ref: ctx.value.comicUrl,
        hints: {
          title: ctx.value.title,
          cover: ctx.value.coverUrl ?? undefined,
          author: ctx.value.author,
        },
      }),
    )
    return
  }
  void router.push(libraryRoute('comic', { scope: 'online' }))
}

onMounted(() => {
  unmounted = false
  window.addEventListener('pagehide', flushComicProgressOnPageHide)
  void initReader()
})

onBeforeUnmount(() => {
  unmounted = true
  window.removeEventListener('pagehide', flushComicProgressOnPageHide)
  initSeq += 1
  loadSeq += 1
  detailSeq += 1
  pageAbortController?.abort()
  pageAbortController = null
  detailAbortController?.abort()
  detailAbortController = null
  void onlineState.flushPendingProgress()
})

watch(
  () => [
    route.query.source,
    route.query.sourceId,
    route.query.source_id,
    route.query.ref,
    route.query.comicUrl,
    route.query.comic_url,
    route.query.chapter,
    route.query.chapterUrl,
    route.query.chapter_url,
  ],
  () => {
    if (syncingUrl) return
    userOverrodeMode = false
    void initReader()
  },
)
</script>

<template>
  <ComicReaderShell
    :title="headerTitle"
    :subtitle="headerSubtitle"
    :back-label="ctx?.comicUrl ? t('online.backToDetail') : t('online.backToOnlineList')"
    :chapter-key="activeChapterKey"
    :total-pages="pages.length"
    :initial-page="initialPage"
    :get-page-src="getPageSrc"
    :chapters="chapterItems"
    :has-prev-chapter="prevChapterIndex !== null"
    :has-next-chapter="nextChapterIndex !== null"
    :next-chapter-toast-label="nextChapterObj?.title ?? ''"
    :reader-mode="readerMode"
    :fit-mode="fitMode"
    :offset-cover-page="offsetCoverPage"
    :dim-images="dimImages"
    :webtoon-width="webtoonWidth"
    :preload-pages="preferences.comic.preload_pages"
    :loading="loading"
    :load-error="loadError"
    :has-content="hasContent"
    @update:reader-mode="handleModeChange"
    @update:fit-mode="handleFitChange"
    @update:offset-cover-page="handleOffsetCoverChange"
    @update:dim-images="handleDimImagesChange"
    @update:webtoon-width="webtoonWidth = $event"
    @commit-webtoon-width="handleWebtoonWidthCommit"
    @page-change="handlePageChange"
    @prev-chapter="prevChapterIndex !== null && switchChapterByIndex(prevChapterIndex)"
    @next-chapter="nextChapterIndex !== null && switchChapterByIndex(nextChapterIndex)"
    @select-chapter="handleSelectChapterKey($event)"
    @back="handleBack"
    @retry="handleRetry"
  >
    <template #header-meta>
      <span
        v-if="activeSourceName"
        class="text-label-small hidden shrink-0 rounded-corner-full bg-primary-container px-2 py-0.5 text-on-primary-container sm:inline-flex"
      >
        {{ activeSourceName }}
      </span>
    </template>
  </ComicReaderShell>
</template>
