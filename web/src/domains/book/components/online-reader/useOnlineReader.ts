import { computed, onBeforeUnmount, onMounted, ref, type Ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useBookI18n as useI18n } from '@/domains/book/i18n'
import {
  parseTextParagraphs,
  useContinuousChapterReader,
  type ContinuousChapterSection,
} from '@/domains/book/readers/useContinuousChapterReader'
import {
  libraryRoute,
  parseItemRef,
  playRoute,
  useOnlineItemState,
  type OnlineItemMetadataInput,
  type TrackedItemRef,
} from '@/sources'
import { useSnackbarStore } from '@/stores/snackbar'
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
  getOnlineBookReaderContext,
  matchChapterIndexByTitle,
  resolveChapterPos,
  setOnlineBookReaderContext,
  type ReaderContext,
} from './chapterMatch'

export type OnlineReaderOverlay = 'toc' | 'typography' | 'switch-source' | 'download' | null

export function useOnlineReader(contentScrollRef: Ref<HTMLElement | null>) {
  const { t } = useI18n()
  const route = useRoute()
  const router = useRouter()
  const catalogStore = useCatalogStore()
  const snackbar = useSnackbarStore()

  const readerCtx = ref<ReaderContext | null>(null)
  const chapters = ref<OnlineChapterItem[]>([])
  const loadingToc = ref(false)
  const tocError = ref<string | null>(null)
  const savingShelf = ref(false)

  // Single mutually-exclusive overlay state (fixes overlapping drawers/dialogs)
  const activeOverlay = ref<OnlineReaderOverlay>(null)
  const tocQuery = ref('')
  const switchingKey = ref<string | null>(null)

  let tocSeq = 0
  let unmounted = false
  let progressSyncTimer: ReturnType<typeof setTimeout> | null = null
  let lastPersistedPos = -1

  function findPrevReadablePos(fromPos: number): number | null {
    for (let i = fromPos - 1; i >= 0; i -= 1) {
      const ch = chapters.value[i]
      if (ch && !ch.is_volume && ch.url) return i
    }
    return null
  }

  function findNextReadablePos(fromPos: number): number | null {
    for (let i = fromPos + 1; i < chapters.value.length; i += 1) {
      const ch = chapters.value[i]
      if (ch && !ch.is_volume && ch.url) return i
    }
    return null
  }

  function clearProgressSyncTimer(): void {
    if (progressSyncTimer) {
      clearTimeout(progressSyncTimer)
      progressSyncTimer = null
    }
  }

  const trackedRef = computed<TrackedItemRef | null>(() => {
    const ctx = readerCtx.value
    if (!ctx?.sourceId || !ctx.bookUrl) return null
    return {
      scope: 'online',
      domain: 'book',
      sourceId: ctx.sourceId,
      ref: ctx.bookUrl,
      hints: {
        title: ctx.name || undefined,
        author: ctx.author || undefined,
        cover: ctx.coverUrl ?? undefined,
        extra: ctx.tocUrl ? { toc: ctx.tocUrl } : undefined,
      },
    }
  })

  const onlineItemMeta = computed<OnlineItemMetadataInput | null>(() => {
    const ctx = readerCtx.value
    if (!ctx) return null
    return {
      title: ctx.name || ctx.bookUrl,
      subtitle: ctx.author || null,
      coverUrl: ctx.coverUrl ?? null,
      intro: ctx.intro ?? null,
      extra: {
        toc_url: ctx.tocUrl,
        last_chapter: chapters.value[chapters.value.length - 1]?.title ?? null,
      },
    }
  })

  const onlineItemState = useOnlineItemState(trackedRef, onlineItemMeta)

  function readQueryStr(key: string): string {
    const val = route.query[key]
    return typeof val === 'string' && val.trim() ? val.trim() : ''
  }

  function buildCanonicalRouteQuery(ctx: ReaderContext): Record<string, string> {
    const q: Record<string, string> = {
      source: ctx.sourceId,
      ref: ctx.bookUrl,
      toc: ctx.tocUrl,
      chapter: String(ctx.currentChapterIndex || 0),
    }
    if (ctx.name) q.title = ctx.name
    if (ctx.author) q.author = ctx.author
    if (ctx.coverUrl) q.cover = ctx.coverUrl
    if (ctx.sourceName) q.sourceName = ctx.sourceName
    return q
  }

  function syncContextToUrlAndMemory(ctx: ReaderContext): void {
    setOnlineBookReaderContext(ctx)
    const nextQuery = buildCanonicalRouteQuery(ctx)
    const same =
      route.path === '/book/play/online' &&
      Object.keys(nextQuery).length === Object.keys(route.query).length &&
      Object.entries(nextQuery).every(([k, v]) => route.query[k] === v)
    if (!same) {
      void router.replace({ path: '/book/play/online', query: nextQuery })
    }
  }

  function persistReadingProgress(
    pos: number,
    title: string,
    scrollRatio: number,
    immediate = false,
  ): void {
    const ctx = readerCtx.value
    if (!ctx || unmounted) return
    if (onlineItemState.record.value) {
      onlineItemState.saveProgress(
        {
          unitIndex: pos,
          unitTitle: title || undefined,
          position: scrollRatio,
          duration: 1,
          totalUnits: chapters.value.length > 0 ? chapters.value.length : undefined,
        },
        { immediate },
      )
    }
    if (pos === lastPersistedPos && !immediate) return

    const commit = () => {
      const curCtx = readerCtx.value
      if (!curCtx || unmounted) return
      lastPersistedPos = pos
      curCtx.currentChapterIndex = pos
      syncContextToUrlAndMemory(curCtx)
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

  const reader = useContinuousChapterReader({
    scrollContainerRef: contentScrollRef,
    isBlocked: () => loadingToc.value,
    getPrevPos: findPrevReadablePos,
    getNextPos: findNextReadablePos,
    fetchChapter: async (pos, { forceRefresh }): Promise<ContinuousChapterSection | null> => {
      const ctx = readerCtx.value
      const ch = chapters.value[pos]
      if (!ctx || !ch || ch.is_volume || !ch.url) return null
      const sourceId = ctx.sourceId
      const bookUrl = ctx.bookUrl
      const res = await onlineBookApi.fetchContent(
        sourceId,
        ch.url,
        buildBookJsCtxFrom(ctx, ch.index, ch.title),
        { index: ch.index, title: ch.title, url: ch.url },
        forceRefresh,
      )
      if (
        unmounted ||
        readerCtx.value?.sourceId !== sourceId ||
        readerCtx.value?.bookUrl !== bookUrl
      ) {
        return null
      }
      return {
        pos,
        chapterIndex: ch.index,
        title: res.content.title || ch.title,
        paragraphs: parseTextParagraphs(res.content.content),
        cached: res.cached,
      }
    },
    onPrefetch: (pos) => {
      if (readerCtx.value) {
        prefetchReadableChapter(findNextReadablePos(pos), readerCtx.value.sourceId)
      }
    },
    onProgress: ({ pos, chapter, scrollRatio, immediate }) => {
      persistReadingProgress(pos, chapter.title, scrollRatio, immediate)
    },
    progressDebounceMs: 250,
  })

  const currentChapter = computed<OnlineChapterItem | null>(
    () => chapters.value[reader.activePos.value] ?? null,
  )

  const chapterTitle = computed(
    () => reader.activeChapter.value?.title || currentChapter.value?.title || '',
  )

  const contentCached = computed(() => Boolean(reader.activeChapter.value?.cached))

  const readableChapters = computed(() =>
    chapters.value.filter((c) => !c.is_volume && Boolean(c.url)),
  )

  const downloadTarget = computed<OnlineDownloadTarget | null>(() => {
    const c = readerCtx.value
    if (!c) return null
    return {
      sourceId: c.sourceId,
      sourceName: c.sourceName || t('online.sourceLabel'),
      bookName: c.name,
      author: c.author,
      bookUrl: c.bookUrl,
      tocUrl: c.tocUrl,
      coverUrl: c.coverUrl,
      intro: c.intro,
      totalChapters: readableChapters.value.length,
    }
  })

  function openOverlay(next: OnlineReaderOverlay): void {
    activeOverlay.value = activeOverlay.value === next ? null : next
  }

  function closeOverlay(): void {
    activeOverlay.value = null
  }

  async function openChapter(pos: number, forceRefresh = false): Promise<void> {
    const ch = chapters.value[pos]
    if (!ch || ch.is_volume || !ch.url) return
    if (activeOverlay.value === 'toc') {
      activeOverlay.value = null
    }
    await reader.openChapter(pos, { forceRefresh })
  }

  async function loadTocAndOpen(targetIdx: number, matchTitle?: string): Promise<void> {
    const ctx = readerCtx.value
    if (!ctx) return
    const seq = ++tocSeq
    reader.resetState()
    loadingToc.value = true
    tocError.value = null
    try {
      const tocRes = await onlineBookApi.fetchToc(
        ctx.sourceId,
        ctx.tocUrl,
        buildBookJsCtxFrom(ctx, targetIdx, matchTitle || ''),
      )
      if (unmounted || seq !== tocSeq) return
      chapters.value = tocRes.chapters
      if (tocRes.chapters.length === 0) {
        tocError.value = t('reader.onlineEmptyToc')
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
      tocError.value = err instanceof Error ? err.message : String(err)
    } finally {
      if (!unmounted && seq === tocSeq) {
        loadingToc.value = false
      }
    }
  }

  async function initReader(): Promise<void> {
    const parsedRef = parseItemRef(route, 'book')
    const parsedOnline = parsedRef && parsedRef.scope !== 'local' ? parsedRef : null
    const sourceNameQuery = readQueryStr('sourceName')
    const sourceIdQuery = parsedOnline?.sourceId || readQueryStr('source')
    const bookUrlQuery = parsedOnline?.ref || readQueryStr('ref')
    const tocUrlQuery = parsedOnline?.hints?.extra?.toc ?? readQueryStr('toc')
    const nameQuery = parsedOnline?.hints?.title || readQueryStr('title')
    const authorQuery = parsedOnline?.hints?.author || readQueryStr('author')
    const coverUrlQuery = parsedOnline?.hints?.cover || readQueryStr('cover')
    const chapterQuery = readQueryStr('chapter')
    const parsedChapter = chapterQuery ? Number.parseInt(chapterQuery, 10) : NaN

    const storedCtx = getOnlineBookReaderContext()

    if (sourceIdQuery && bookUrlQuery) {
      const matchesStored =
        storedCtx &&
        storedCtx.sourceId === sourceIdQuery &&
        storedCtx.bookUrl === bookUrlQuery

      let resolvedTocUrl = tocUrlQuery || (matchesStored ? storedCtx.tocUrl : '')
      let resolvedName = nameQuery || (matchesStored ? storedCtx.name : '')
      let resolvedAuthor = authorQuery || (matchesStored ? storedCtx.author : '')
      let resolvedSourceName = sourceNameQuery || (matchesStored ? storedCtx.sourceName : '')
      let resolvedCoverUrl = coverUrlQuery || (matchesStored ? storedCtx.coverUrl : null)
      let resolvedIntro = matchesStored ? storedCtx.intro : null

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
                ? storedCtx.currentChapterIndex
                : 0,
        }
      }
    }

    if (!readerCtx.value && storedCtx) {
      readerCtx.value = storedCtx
    }

    if (unmounted) return
    if (!readerCtx.value) {
      void router.replace(libraryRoute('book', { scope: 'online' }))
      return
    }

    if (!Number.isFinite(parsedChapter)) {
      const persisted = await onlineItemState.refresh()
      if (
        !unmounted &&
        readerCtx.value &&
        persisted?.progress?.unitIndex !== undefined &&
        persisted.progress.unitIndex >= 0
      ) {
        readerCtx.value.currentChapterIndex = persisted.progress.unitIndex
      }
    }

    syncContextToUrlAndMemory(readerCtx.value)
    await loadTocAndOpen(readerCtx.value.currentChapterIndex || 0)
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
          progress_percent: Math.min(100, (reader.activePos.value / total) * 100),
          locator: `chapter:${reader.activePos.value}`,
          chapter_index: reader.activePos.value,
          scroll_ratio: 0,
        })
        .catch(() => {})
      void catalogStore.refreshSidebarData()
      snackbar.show({ message: t('online.trackedSuccess', { title: created.metadata.title }) })
      void router.replace(playRoute({ scope: 'local', domain: 'book', id: created.id }))
    } catch (err) {
      snackbar.show({
        message: err instanceof Error ? err.message : String(err),
        tone: 'error',
      })
    } finally {
      savingShelf.value = false
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
        throw new Error(t('online.switchSourceNoToc'))
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
        buildBookJsCtxFrom(nextCtx, reader.activePos.value, previousTitle),
      )
      if (unmounted) return
      if (tocRes.chapters.length === 0) {
        throw new Error(t('online.switchSourceEmptyToc'))
      }
      const clampedFallback = Math.max(
        0,
        Math.min(reader.activePos.value, tocRes.chapters.length - 1),
      )
      let targetPos = previousTitle
        ? matchChapterIndexByTitle(previousTitle, tocRes.chapters, clampedFallback)
        : clampedFallback
      if (tocRes.chapters[targetPos]?.is_volume || !tocRes.chapters[targetPos]?.url) {
        const firstReadable = tocRes.chapters.findIndex((c) => !c.is_volume && Boolean(c.url))
        if (firstReadable < 0) {
          throw new Error(t('online.switchSourceNoReadable'))
        }
        targetPos = firstReadable
      }

      tocSeq += 1
      nextCtx.currentChapterIndex = targetPos
      readerCtx.value = nextCtx
      chapters.value = tocRes.chapters
      syncContextToUrlAndMemory(nextCtx)
      activeOverlay.value = null

      await openChapter(targetPos, false)
      if (reader.errorMsg.value) {
        snackbar.show({
          message: t('online.switchSourcePartialErr', {
            source: item.source_name,
            error: reader.errorMsg.value,
          }),
          tone: 'error',
        })
        return
      }
      const matchedTitle = currentChapter.value?.title || chapterTitle.value
      snackbar.show({
        message: matchedTitle
          ? t('online.switchSourceMatched', { source: item.source_name, chapter: matchedTitle })
          : t('online.switchSourceSuccess', { source: item.source_name }),
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
    const prev = (window.history.state as { back?: string | null } | null)?.back
    if (typeof prev === 'string' && prev.startsWith('/book/library')) {
      router.back()
    } else {
      void router.push(libraryRoute('book', { scope: 'online' }))
    }
  }

  onMounted(() => {
    unmounted = false
    void initReader()
  })

  onBeforeUnmount(() => {
    clearProgressSyncTimer()
    unmounted = true
    tocSeq += 1
  })

  return {
    readerCtx,
    chapters,
    activeIndex: reader.activePos,
    loadedChapters: reader.loadedChapters,
    loadingToc,
    loadingContent: reader.loadingChapter,
    appendingNext: reader.appendingNext,
    appendError: reader.appendError,
    errorMsg: computed(() => tocError.value || reader.errorMsg.value),
    savingShelf,
    favorite: onlineItemState.favorite,
    togglingFavorite: onlineItemState.loading,
    toggleFavorite: onlineItemState.toggleFavorite,
    activeOverlay,
    tocQuery,
    switchingKey,
    currentChapter,
    chapterTitle,
    contentCached,
    prevReadableIndex: reader.prevPos,
    nextAppendableIndex: reader.nextAppendablePos,
    downloadTarget,
    openOverlay,
    closeOverlay,
    loadTocAndOpen,
    openChapter,
    appendNextChapter: reader.appendNextChapter,
    handleContentScroll: reader.handleScroll,
    handleSaveToShelf,
    handleSelectAltSource,
    handleBackToCatalog,
  }
}
