import { computed, nextTick, onBeforeUnmount, onMounted, ref, type Ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useSnackbarStore } from '@/stores/snackbar'
import { useThemeStore } from '@/stores/theme'
import { resolveFontStack } from '@/utils/format'
import { api as bookApi } from '../../api'
import { useCatalogStore } from '../../store'
import type { OnlineDownloadTarget } from '../OnlineDownloadModal.vue'
import {
  onlineBookApi,
  type AggregatedSearchItemDto,
  type OnlineChapterItem,
} from '../../onlineApi'
import {
  buildBookJsCtxFrom,
  matchChapterIndexByTitle,
  resolveChapterPos,
  type LoadedOnlineChapter,
  type ReaderContext,
} from './chapterMatch'

const AUTO_LOAD_THRESHOLD_PX = 400

export function useOnlineReader(contentScrollRef: Ref<HTMLElement | null>) {
  const route = useRoute()
  const router = useRouter()
  const catalogStore = useCatalogStore()
  const themeStore = useThemeStore()
  const snackbar = useSnackbarStore()

  const readerCtx = ref<ReaderContext | null>(null)
  const chapters = ref<OnlineChapterItem[]>([])
  const activeIndex = ref(0)
  const loadedChapters = ref<LoadedOnlineChapter[]>([])
  const loadingToc = ref(false)
  const loadingContent = ref(false)
  const appendingNext = ref(false)
  const appendError = ref<string | null>(null)
  const errorMsg = ref<string | null>(null)
  const savingShelf = ref(false)

  const tocOpen = ref(false)
  const tocQuery = ref('')
  const typographyOpen = ref(false)
  const switchSourceOpen = ref(false)
  const searchingAlts = ref(false)
  const altCandidates = ref<AggregatedSearchItemDto[]>([])
  const switchingKey = ref<string | null>(null)
  const downloadOpen = ref(false)

  let loadSeq = 0
  let tocSeq = 0
  let unmounted = false
  let progressSyncTimer: ReturnType<typeof setTimeout> | null = null
  let lastPersistedPos = -1

  const maxWidthPx = computed(() => Math.max(520, 920 - (themeStore.reader.margin_px || 48) * 2))

  const fontSize = computed<number>({
    get: () => themeStore.reader.font_size || 18,
    set: (v) => {
      const clamped = Math.min(30, Math.max(13, Math.round(v)))
      themeStore.updateReaderPreferences({ font_size: clamped })
    },
  })

  const lineHeight = computed<number>({
    get: () => themeStore.reader.line_height || 1.85,
    set: (v) => {
      const clamped = Math.min(2.4, Math.max(1.3, Number(v.toFixed(2))))
      themeStore.updateReaderPreferences({ line_height: clamped })
    },
  })

  const fontFamily = computed<string>({
    get: () => themeStore.reader.font_family,
    set: (v) => {
      themeStore.updateReaderPreferences({ font_family: v })
    },
  })

  const fontFamilyStyle = computed(() => resolveFontStack(themeStore.reader.font_family))

  const currentChapter = computed<OnlineChapterItem | null>(
    () => chapters.value[activeIndex.value] ?? null,
  )

  const activeLoadedChapter = computed<LoadedOnlineChapter | null>(
    () =>
      loadedChapters.value.find((c) => c.pos === activeIndex.value) ??
      loadedChapters.value[0] ??
      null,
  )

  const chapterTitle = computed(
    () => activeLoadedChapter.value?.title || currentChapter.value?.title || '',
  )

  const contentCached = computed(() => activeLoadedChapter.value?.cached ?? false)

  const readableChapters = computed(() =>
    chapters.value.filter((c) => !c.is_volume && Boolean(c.url)),
  )

  const prevReadableIndex = computed<number | null>(() => {
    for (let i = activeIndex.value - 1; i >= 0; i -= 1) {
      const ch = chapters.value[i]
      if (ch && !ch.is_volume && ch.url) return i
    }
    return null
  })

  function findNextReadablePos(fromPos: number): number | null {
    for (let i = fromPos + 1; i < chapters.value.length; i += 1) {
      const ch = chapters.value[i]
      if (ch && !ch.is_volume && ch.url) return i
    }
    return null
  }

  const nextReadableIndex = computed<number | null>(() => findNextReadablePos(activeIndex.value))

  const lastLoadedPos = computed<number | null>(() => {
    const last = loadedChapters.value[loadedChapters.value.length - 1]
    return last ? last.pos : null
  })

  const nextAppendableIndex = computed<number | null>(() =>
    lastLoadedPos.value === null
      ? nextReadableIndex.value
      : findNextReadablePos(lastLoadedPos.value),
  )

  const downloadTarget = computed<OnlineDownloadTarget | null>(() => {
    const c = readerCtx.value
    if (!c) return null
    return {
      sourceId: c.sourceId,
      sourceName: c.sourceName,
      bookName: c.name,
      author: c.author,
      bookUrl: c.bookUrl,
      tocUrl: c.tocUrl,
      coverUrl: c.coverUrl,
      intro: c.intro,
      totalChapters: readableChapters.value.length,
    }
  })

  function readQueryStr(key: string): string {
    const val = route.query[key]
    return typeof val === 'string' ? val : ''
  }

  function buildRouteQueryFromCtx(ctx: ReaderContext): Record<string, string> {
    const q: Record<string, string> = {
      sourceId: ctx.sourceId,
      sourceName: ctx.sourceName,
      bookUrl: ctx.bookUrl,
      tocUrl: ctx.tocUrl,
      name: ctx.name,
      author: ctx.author,
      chapter: String(ctx.currentChapterIndex || 0),
    }
    if (ctx.shelfId) q.shelfId = ctx.shelfId
    if (ctx.coverUrl) q.coverUrl = ctx.coverUrl
    return q
  }

  function syncContextToUrlAndStorage(ctx: ReaderContext): void {
    try {
      sessionStorage.setItem('xmedia_online_reader_ctx', JSON.stringify(ctx))
    } catch {
      // ignore storage errors
    }
    const nextQuery = buildRouteQueryFromCtx(ctx)
    const same =
      Object.keys(nextQuery).length === Object.keys(route.query).length &&
      Object.entries(nextQuery).every(([k, v]) => route.query[k] === v)
    if (!same) {
      void router.replace({ path: route.path, query: nextQuery })
    }
  }

  function clearProgressSyncTimer(): void {
    if (progressSyncTimer) {
      clearTimeout(progressSyncTimer)
      progressSyncTimer = null
    }
  }

  function persistReadingProgress(pos: number, title: string, immediate = false): void {
    const ctx = readerCtx.value
    if (!ctx || unmounted) return
    if (pos === lastPersistedPos && !immediate) return

    const commit = () => {
      const curCtx = readerCtx.value
      if (!curCtx || unmounted) return
      lastPersistedPos = pos
      curCtx.currentChapterIndex = pos
      syncContextToUrlAndStorage(curCtx)
      if (curCtx.shelfId) {
        void onlineBookApi
          .updateShelfBook(curCtx.shelfId, {
            current_chapter_index: pos,
            current_chapter_title: title,
            total_chapters: chapters.value.length,
            last_chapter: chapters.value[chapters.value.length - 1]?.title ?? null,
          })
          .catch(() => {})
      }
    }

    clearProgressSyncTimer()
    if (immediate) {
      commit()
    } else {
      progressSyncTimer = setTimeout(() => {
        progressSyncTimer = null
        commit()
      }, 300)
    }
  }

  function prefetchReadableChapter(targetPos: number | null, sourceId: string): void {
    const ctx = readerCtx.value
    if (!ctx || targetPos === null) return
    const nextCh = chapters.value[targetPos]
    if (!nextCh || nextCh.is_volume || !nextCh.url) return
    void onlineBookApi
      .fetchContent(
        sourceId,
        nextCh.url,
        buildBookJsCtxFrom(ctx, nextCh.index, nextCh.title),
        { index: nextCh.index, title: nextCh.title, url: nextCh.url },
        false,
      )
      .catch(() => {})
  }

  async function initReader(): Promise<void> {
    const shelfIdQuery = readQueryStr('shelfId')
    const sourceIdQuery = readQueryStr('sourceId')
    const sourceNameQuery = readQueryStr('sourceName')
    const bookUrlQuery = readQueryStr('bookUrl')
    const tocUrlQuery = readQueryStr('tocUrl')
    const nameQuery = readQueryStr('name')
    const authorQuery = readQueryStr('author')
    const coverUrlQuery = readQueryStr('coverUrl')
    const chapterQuery = readQueryStr('chapter')
    const parsedChapter = chapterQuery ? Number.parseInt(chapterQuery, 10) : NaN

    let storedCtx: ReaderContext | null = null
    try {
      const rawStored = sessionStorage.getItem('xmedia_online_reader_ctx')
      if (rawStored) {
        storedCtx = JSON.parse(rawStored) as ReaderContext
      }
    } catch {
      storedCtx = null
    }

    if (sourceIdQuery && bookUrlQuery) {
      const matchesStored =
        storedCtx &&
        storedCtx.sourceId === sourceIdQuery &&
        storedCtx.bookUrl === bookUrlQuery

      let resolvedTocUrl = tocUrlQuery || (matchesStored ? storedCtx!.tocUrl : '')
      let resolvedName = nameQuery || (matchesStored ? storedCtx!.name : '')
      let resolvedAuthor = authorQuery || (matchesStored ? storedCtx!.author : '')
      let resolvedSourceName = sourceNameQuery || (matchesStored ? storedCtx!.sourceName : '')
      let resolvedCoverUrl = coverUrlQuery || (matchesStored ? storedCtx!.coverUrl : null)
      let resolvedIntro = matchesStored ? storedCtx!.intro : null

      if (!resolvedTocUrl) {
        try {
          const info = await onlineBookApi.fetchBookInfo(sourceIdQuery, bookUrlQuery, {
            name: resolvedName,
            author: resolvedAuthor,
            book_url: bookUrlQuery,
            toc_url: '',
            cover_url: resolvedCoverUrl,
            intro: resolvedIntro,
          })
          if (unmounted) return
          resolvedTocUrl = info.detail.toc_url
          resolvedName = info.detail.name || resolvedName
          resolvedAuthor = info.detail.author || resolvedAuthor
          resolvedSourceName = info.source_name || resolvedSourceName
          resolvedCoverUrl = info.detail.cover_url ?? resolvedCoverUrl
          resolvedIntro = info.detail.intro ?? resolvedIntro
        } catch {
          // handled below if tocUrl still missing
        }
      }

      if (resolvedTocUrl) {
        readerCtx.value = {
          shelfId: shelfIdQuery || (matchesStored ? storedCtx!.shelfId : undefined),
          sourceId: sourceIdQuery,
          sourceName: resolvedSourceName,
          name: resolvedName,
          author: resolvedAuthor,
          bookUrl: bookUrlQuery,
          tocUrl: resolvedTocUrl,
          coverUrl: resolvedCoverUrl,
          intro: resolvedIntro,
          currentChapterIndex:
            Number.isFinite(parsedChapter) && parsedChapter >= 0
              ? parsedChapter
              : matchesStored
                ? storedCtx!.currentChapterIndex
                : 0,
        }
      }
    }

    if (!readerCtx.value && shelfIdQuery) {
      if (storedCtx && storedCtx.shelfId === shelfIdQuery) {
        readerCtx.value = storedCtx
      } else {
        try {
          const shelf = await onlineBookApi.listShelf()
          if (unmounted) return
          const found = shelf.find((b) => b.id === shelfIdQuery)
          if (found) {
            readerCtx.value = {
              shelfId: found.id,
              sourceId: found.source_id,
              sourceName: found.source_name,
              name: found.name,
              author: found.author,
              bookUrl: found.book_url,
              tocUrl: found.toc_url,
              coverUrl: found.cover_url,
              intro: found.intro,
              currentChapterIndex:
                Number.isFinite(parsedChapter) && parsedChapter >= 0
                  ? parsedChapter
                  : found.current_chapter_index,
            }
          }
        } catch {
          // ignore
        }
      }
    }

    if (!readerCtx.value && storedCtx && !shelfIdQuery) {
      readerCtx.value = storedCtx
    }

    if (unmounted) return
    if (!readerCtx.value) {
      void router.replace({ path: '/book/catalog', query: { scope: 'online' } })
      return
    }

    syncContextToUrlAndStorage(readerCtx.value)
    await loadTocAndOpen(readerCtx.value.currentChapterIndex || 0)
  }

  async function loadTocAndOpen(targetIdx: number, matchTitle?: string): Promise<void> {
    const ctx = readerCtx.value
    if (!ctx) return
    const seq = ++tocSeq
    loadingToc.value = true
    errorMsg.value = null
    try {
      const tocRes = await onlineBookApi.fetchToc(
        ctx.sourceId,
        ctx.tocUrl,
        buildBookJsCtxFrom(ctx, targetIdx, matchTitle || ''),
      )
      if (unmounted || seq !== tocSeq) return
      chapters.value = tocRes.chapters
      if (tocRes.chapters.length === 0) {
        errorMsg.value = '目录为空，请检查书源状态或尝试换源'
        return
      }
      const clampedFallback = resolveChapterPos(tocRes.chapters, targetIdx)
      let idx = matchTitle
        ? matchChapterIndexByTitle(matchTitle, tocRes.chapters, clampedFallback)
        : clampedFallback

      if (tocRes.chapters[idx]?.is_volume || !tocRes.chapters[idx]?.url) {
        const firstReadable = tocRes.chapters.findIndex((c) => !c.is_volume && Boolean(c.url))
        if (firstReadable >= 0) idx = firstReadable
      }
      loadingToc.value = false
      await openChapter(idx, false)
    } catch (err) {
      if (unmounted || seq !== tocSeq) return
      errorMsg.value = err instanceof Error ? err.message : String(err)
    } finally {
      if (!unmounted && seq === tocSeq) {
        loadingToc.value = false
      }
    }
  }

  async function openChapter(pos: number, forceRefresh = false): Promise<void> {
    const ctx = readerCtx.value
    const ch = chapters.value[pos]
    if (!ctx || !ch || ch.is_volume || !ch.url) return

    clearProgressSyncTimer()
    const seq = ++loadSeq
    const sourceId = ctx.sourceId
    const bookUrl = ctx.bookUrl

    activeIndex.value = pos
    loadedChapters.value = []
    appendingNext.value = false
    appendError.value = null
    tocOpen.value = false
    loadingContent.value = true
    errorMsg.value = null

    try {
      const res = await onlineBookApi.fetchContent(
        sourceId,
        ch.url,
        buildBookJsCtxFrom(ctx, ch.index, ch.title),
        { index: ch.index, title: ch.title, url: ch.url },
        forceRefresh,
      )
      if (
        unmounted ||
        seq !== loadSeq ||
        readerCtx.value?.sourceId !== sourceId ||
        readerCtx.value?.bookUrl !== bookUrl
      ) {
        return
      }

      const resolvedTitle = res.content.title || ch.title
      const parsedParagraphs = res.content.content
        .split(/\n+/)
        .map((s) => s.trim())
        .filter(Boolean)

      loadedChapters.value = [
        {
          pos,
          chapterIndex: ch.index,
          title: resolvedTitle,
          paragraphs: parsedParagraphs,
          cached: res.cached,
        },
      ]
      loadingContent.value = false

      await nextTick()
      if (unmounted || seq !== loadSeq) return
      if (contentScrollRef.value) {
        contentScrollRef.value.scrollTop = 0
      }

      persistReadingProgress(pos, resolvedTitle, true)

      const nextPos = findNextReadablePos(pos)
      prefetchReadableChapter(nextPos, sourceId)

      if (
        parsedParagraphs.length > 0 &&
        nextPos !== null &&
        contentScrollRef.value &&
        contentScrollRef.value.scrollHeight <= contentScrollRef.value.clientHeight + 120
      ) {
        void appendNextChapter()
      }
    } catch (err) {
      if (unmounted || seq !== loadSeq) return
      errorMsg.value = err instanceof Error ? err.message : String(err)
    } finally {
      if (!unmounted && seq === loadSeq) {
        loadingContent.value = false
      }
    }
  }

  async function appendNextChapter(): Promise<void> {
    const ctx = readerCtx.value
    const targetPos = nextAppendableIndex.value
    if (!ctx || targetPos === null) return
    if (loadingToc.value || loadingContent.value || appendingNext.value) return
    if (loadedChapters.value.some((c) => c.pos === targetPos)) return

    const ch = chapters.value[targetPos]
    if (!ch || ch.is_volume || !ch.url) return

    const seq = loadSeq
    const sourceId = ctx.sourceId
    const bookUrl = ctx.bookUrl
    appendingNext.value = true
    appendError.value = null

    try {
      const res = await onlineBookApi.fetchContent(
        sourceId,
        ch.url,
        buildBookJsCtxFrom(ctx, ch.index, ch.title),
        { index: ch.index, title: ch.title, url: ch.url },
        false,
      )
      if (
        unmounted ||
        seq !== loadSeq ||
        readerCtx.value?.sourceId !== sourceId ||
        readerCtx.value?.bookUrl !== bookUrl
      ) {
        return
      }
      if (loadedChapters.value.some((c) => c.pos === targetPos)) return

      const resolvedTitle = res.content.title || ch.title
      const parsedParagraphs = res.content.content
        .split(/\n+/)
        .map((s) => s.trim())
        .filter(Boolean)

      loadedChapters.value.push({
        pos: targetPos,
        chapterIndex: ch.index,
        title: resolvedTitle,
        paragraphs: parsedParagraphs,
        cached: res.cached,
      })

      prefetchReadableChapter(findNextReadablePos(targetPos), sourceId)
    } catch (err) {
      if (unmounted || seq !== loadSeq) return
      appendError.value = err instanceof Error ? err.message : String(err)
    } finally {
      if (!unmounted && seq === loadSeq) {
        appendingNext.value = false
      }
    }
  }

  function handleContentScroll(): void {
    const el = contentScrollRef.value
    if (!el || loadingContent.value || loadingToc.value || loadedChapters.value.length === 0) return

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

    if (visiblePos !== activeIndex.value) {
      activeIndex.value = visiblePos
      const item = loadedChapters.value.find((c) => c.pos === visiblePos)
      const title = item?.title || chapters.value[visiblePos]?.title || ''
      persistReadingProgress(visiblePos, title, false)
      if (readerCtx.value) {
        prefetchReadableChapter(findNextReadablePos(visiblePos), readerCtx.value.sourceId)
      }
    }

    const remaining = el.scrollHeight - el.scrollTop - el.clientHeight
    if (
      remaining <= AUTO_LOAD_THRESHOLD_PX &&
      !appendingNext.value &&
      !appendError.value &&
      nextAppendableIndex.value !== null
    ) {
      void appendNextChapter()
    }
  }

  async function handleSaveToShelf(): Promise<void> {
    const ctx = readerCtx.value
    if (!ctx || savingShelf.value) return
    savingShelf.value = true
    try {
      const created = await onlineBookApi.trackOnlineBook({
        source_id: ctx.sourceId,
        name: ctx.name,
        author: ctx.author,
        book_url: ctx.bookUrl,
        toc_url: ctx.tocUrl,
        cover_url: ctx.coverUrl ?? null,
        intro: ctx.intro ?? null,
        last_chapter: chapters.value[chapters.value.length - 1]?.title ?? null,
      })
      const total = Math.max(1, chapters.value.length)
      await bookApi
        .saveProgress(created.id, {
          progress_percent: Math.min(100, (activeIndex.value / total) * 100),
          locator: `chapter:${activeIndex.value}`,
          chapter_index: activeIndex.value,
          scroll_ratio: 0,
        })
        .catch(() => {})
      void catalogStore.refreshSidebarData()
      snackbar.show({ message: `已将《${created.metadata.title}》加入书库（标准 EPUB 3.0）` })
      void router.replace(`/book/read/${created.id}`)
    } catch (err) {
      snackbar.show({
        message: err instanceof Error ? err.message : String(err),
        tone: 'error',
      })
    } finally {
      savingShelf.value = false
    }
  }

  async function openSwitchSource(): Promise<void> {
    const ctx = readerCtx.value
    if (!ctx) return
    switchSourceOpen.value = true
    searchingAlts.value = true
    altCandidates.value = []
    try {
      const res = await onlineBookApi.search(ctx.name, undefined, 1)
      altCandidates.value = res.items
    } catch (err) {
      snackbar.show({
        message: err instanceof Error ? err.message : String(err),
        tone: 'error',
      })
    } finally {
      searchingAlts.value = false
    }
  }

  async function handleSelectAltSource(item: AggregatedSearchItemDto): Promise<void> {
    const ctx = readerCtx.value
    if (!ctx || switchingKey.value !== null) return
    const key = `${item.source_id}:${item.book.book_url}`
    switchingKey.value = key
    const previousTitle = currentChapter.value?.title || chapterTitle.value || ''
    try {
      const detailRes = await onlineBookApi.fetchBookInfo(item.source_id, item.book.book_url, {
        name: item.book.name,
        author: item.book.author,
        book_url: item.book.book_url,
        toc_url: '',
        cover_url: item.book.cover_url,
        intro: item.book.intro,
      })
      if (unmounted) return
      const nextTocUrl = detailRes.detail.toc_url?.trim()
      if (!nextTocUrl) {
        throw new Error('目标书源未解析到目录地址，未切换')
      }
      const nextCtx: ReaderContext = {
        ...ctx,
        sourceId: item.source_id,
        sourceName: item.source_name,
        bookUrl: item.book.book_url,
        tocUrl: nextTocUrl,
        coverUrl: detailRes.detail.cover_url ?? item.book.cover_url ?? ctx.coverUrl,
        intro: detailRes.detail.intro ?? item.book.intro ?? ctx.intro,
      }

      const tocRes = await onlineBookApi.fetchToc(
        nextCtx.sourceId,
        nextCtx.tocUrl,
        buildBookJsCtxFrom(nextCtx, activeIndex.value, previousTitle),
      )
      if (unmounted) return
      if (tocRes.chapters.length === 0) {
        throw new Error('目标书源目录为空，未切换')
      }
      const clampedFallback = Math.max(0, Math.min(activeIndex.value, tocRes.chapters.length - 1))
      let targetPos = previousTitle
        ? matchChapterIndexByTitle(previousTitle, tocRes.chapters, clampedFallback)
        : clampedFallback
      if (tocRes.chapters[targetPos]?.is_volume || !tocRes.chapters[targetPos]?.url) {
        const firstReadable = tocRes.chapters.findIndex((c) => !c.is_volume && Boolean(c.url))
        if (firstReadable < 0) {
          throw new Error('目标书源无可读章节，未切换')
        }
        targetPos = firstReadable
      }

      if (nextCtx.shelfId) {
        await onlineBookApi.updateShelfBook(nextCtx.shelfId, {
          source_id: nextCtx.sourceId,
          book_url: nextCtx.bookUrl,
          toc_url: nextCtx.tocUrl,
        })
        if (unmounted) return
      }

      tocSeq += 1
      nextCtx.currentChapterIndex = targetPos
      readerCtx.value = nextCtx
      chapters.value = tocRes.chapters
      syncContextToUrlAndStorage(nextCtx)
      switchSourceOpen.value = false

      await openChapter(targetPos, false)
      if (errorMsg.value) {
        snackbar.show({
          message: `已切换至书源「${item.source_name}」，但正文拉取失败：${errorMsg.value}`,
          tone: 'error',
        })
        return
      }
      const matchedTitle = currentChapter.value?.title || chapterTitle.value
      snackbar.show({
        message: matchedTitle
          ? `已切换至书源「${item.source_name}」· 已定位至「${matchedTitle}」`
          : `已切换至书源「${item.source_name}」`,
      })
    } catch (err) {
      snackbar.show({
        message: err instanceof Error ? err.message : String(err),
        tone: 'error',
      })
    } finally {
      switchingKey.value = null
    }
  }

  function handleBackToCatalog(): void {
    void router.push({ path: '/book/catalog', query: { scope: 'online' } })
  }

  function handleKeydown(ev: KeyboardEvent): void {
    if (ev.metaKey || ev.ctrlKey || ev.altKey) return
    if (loadingContent.value || loadingToc.value) return
    const target = ev.target
    if (
      target instanceof HTMLElement &&
      (target.tagName === 'INPUT' ||
        target.tagName === 'TEXTAREA' ||
        target.tagName === 'SELECT' ||
        target.isContentEditable)
    ) {
      return
    }
    if (ev.key === 'ArrowLeft' && prevReadableIndex.value !== null) {
      ev.preventDefault()
      void openChapter(prevReadableIndex.value)
    } else if (ev.key === 'ArrowRight' && nextReadableIndex.value !== null) {
      ev.preventDefault()
      void openChapter(nextReadableIndex.value)
    }
  }

  onMounted(() => {
    unmounted = false
    window.addEventListener('keydown', handleKeydown)
    void initReader()
  })

  onBeforeUnmount(() => {
    clearProgressSyncTimer()
    unmounted = true
    loadSeq += 1
    tocSeq += 1
    window.removeEventListener('keydown', handleKeydown)
  })

  return {
    readerCtx,
    chapters,
    activeIndex,
    loadedChapters,
    loadingToc,
    loadingContent,
    appendingNext,
    appendError,
    errorMsg,
    savingShelf,
    tocOpen,
    tocQuery,
    typographyOpen,
    switchSourceOpen,
    searchingAlts,
    altCandidates,
    switchingKey,
    downloadOpen,
    maxWidthPx,
    fontSize,
    lineHeight,
    fontFamily,
    fontFamilyStyle,
    currentChapter,
    chapterTitle,
    contentCached,
    prevReadableIndex,
    nextAppendableIndex,
    downloadTarget,
    loadTocAndOpen,
    openChapter,
    appendNextChapter,
    handleContentScroll,
    handleSaveToShelf,
    openSwitchSource,
    handleSelectAltSource,
    handleBackToCatalog,
  }
}
