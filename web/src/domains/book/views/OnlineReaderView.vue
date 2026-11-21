<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import {
  LuArrowLeft,
  LuBookMarked,
  LuChevronLeft,
  LuChevronRight,
  LuDownload,
  LuList,
  LuLoader2,
  LuMinus,
  LuPlus,
  LuRefreshCw,
  LuRepeat,
  LuSearch,
  LuSlidersHorizontal,
} from 'vue-icons-plus/lu'
import AppSlider from '@/components/ui/AppSlider.vue'
import MdDialog from '@/components/ui/MdDialog.vue'
import MdSegmentedButton, { type SegmentedOption } from '@/components/ui/MdSegmentedButton.vue'
import MdSideSheet from '@/components/ui/MdSideSheet.vue'
import ThemePopover from '@/components/ThemePopover.vue'
import { useSnackbarStore } from '@/stores/snackbar'
import { useThemeStore } from '@/stores/theme'
import { resolveFontStack } from '@/utils/format'
import { api as bookApi } from '../api'
import { useCatalogStore } from '../store'
import OnlineDownloadModal, {
  type OnlineDownloadTarget,
} from '../components/OnlineDownloadModal.vue'
import {
  onlineBookApi,
  type AggregatedSearchItemDto,
  type BookJsContextInput,
  type OnlineChapterItem,
} from '../onlineApi'

interface ReaderContext {
  shelfId?: string
  sourceId: string
  sourceName: string
  name: string
  author: string
  bookUrl: string
  tocUrl: string
  coverUrl?: string | null
  intro?: string | null
  currentChapterIndex: number
}

const route = useRoute()
const router = useRouter()
const catalogStore = useCatalogStore()
const themeStore = useThemeStore()
const snackbar = useSnackbarStore()

const readerCtx = ref<ReaderContext | null>(null)
const chapters = ref<OnlineChapterItem[]>([])
const activeIndex = ref(0)
const chapterTitle = ref('')
const paragraphs = ref<string[]>([])
const loadingToc = ref(false)
const loadingContent = ref(false)
const contentCached = ref(false)
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

const maxWidthPx = computed(() => Math.max(520, 920 - (themeStore.reader.margin_px || 48) * 2))
const contentScrollRef = ref<HTMLElement | null>(null)
let loadSeq = 0
let tocSeq = 0
let unmounted = false

// Sync typography preferences directly with themeStore (shared with local book reader)
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

const fontFamilyStyle = computed(() => resolveFontStack(themeStore.reader.font_family))

const fontOptions: SegmentedOption<string>[] = [
  { value: 'system-ui', label: '无衬线' },
  { value: 'serif', label: '衬线体' },
  { value: 'mono', label: '等宽' },
]

const readableChapters = computed(() =>
  chapters.value.filter((c) => !c.is_volume && Boolean(c.url)),
)

const filteredToc = computed(() => {
  const q = tocQuery.value.trim().toLowerCase()
  const entries = chapters.value.map((chapter, pos) => ({ chapter, pos }))
  if (!q) return entries
  return entries.filter((entry) => entry.chapter.title.toLowerCase().includes(q))
})

const currentChapter = computed<OnlineChapterItem | null>(
  () => chapters.value[activeIndex.value] ?? null,
)

const prevReadableIndex = computed<number | null>(() => {
  for (let i = activeIndex.value - 1; i >= 0; i -= 1) {
    const ch = chapters.value[i]
    if (ch && !ch.is_volume && ch.url) return i
  }
  return null
})

const nextReadableIndex = computed<number | null>(() => {
  for (let i = activeIndex.value + 1; i < chapters.value.length; i += 1) {
    const ch = chapters.value[i]
    if (ch && !ch.is_volume && ch.url) return i
  }
  return null
})

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

// Chinese numeral -> Arabic digit helper for fuzzy chapter matching
const CN_DIGITS: Record<string, number> = {
  零: 0,
  〇: 0,
  一: 1,
  二: 2,
  两: 2,
  三: 3,
  四: 4,
  五: 5,
  六: 6,
  七: 7,
  八: 8,
  九: 9,
}
const CN_UNITS: Record<string, number> = {
  十: 10,
  百: 100,
  千: 1000,
  万: 10000,
}

function parseChineseInt(token: string): number | null {
  if (!token) return null
  if (/^\d+$/.test(token)) return Number.parseInt(token, 10)
  let total = 0
  let section = 0
  let number = 0
  let matched = false
  for (const ch of token) {
    if (ch in CN_DIGITS) {
      number = CN_DIGITS[ch]!
      matched = true
    } else if (ch in CN_UNITS) {
      const unit = CN_UNITS[ch]!
      matched = true
      if (unit === 10000) {
        section = (section + number) * unit
        total += section
        section = 0
        number = 0
      } else {
        if (number === 0 && unit === 10) number = 1
        section += number * unit
        number = 0
      }
    } else {
      return null
    }
  }
  return matched ? total + section + number : null
}

function normalizeChapterTitle(raw: string): string {
  return raw
    .normalize('NFKC')
    .toLowerCase()
    .replace(/[\s\p{P}\p{S}_-]+/gu, '')
}

function extractChapterSignature(raw: string): { num: number | null; body: string; norm: string } {
  const cleaned = raw.normalize('NFKC').trim()
  const norm = normalizeChapterTitle(cleaned)
  const m = cleaned.match(/^(?:第\s*)?([0-9零〇一二两三四五六七八九十百千万]+)\s*(?:[章回节话卷集幕篇]|[\s.:：、\-]+)(.*)$/i)
  if (m) {
    const num = parseChineseInt(m[1] ?? '')
    const body = normalizeChapterTitle(m[2] ?? '')
    return { num, body, norm }
  }
  return { num: null, body: norm, norm }
}

function diceSimilarity(a: string, b: string): number {
  if (!a || !b) return 0
  if (a === b) return 1
  if (a.length < 2 || b.length < 2) {
    return a.includes(b) || b.includes(a) ? 0.6 : 0
  }
  const bigrams = new Map<string, number>()
  for (let i = 0; i < a.length - 1; i += 1) {
    const bg = a.slice(i, i + 2)
    bigrams.set(bg, (bigrams.get(bg) ?? 0) + 1)
  }
  let overlap = 0
  for (let i = 0; i < b.length - 1; i += 1) {
    const bg = b.slice(i, i + 2)
    const cnt = bigrams.get(bg) ?? 0
    if (cnt > 0) {
      overlap += 1
      bigrams.set(bg, cnt - 1)
    }
  }
  return (2 * overlap) / (a.length - 1 + (b.length - 1))
}

function matchChapterIndexByTitle(
  targetTitle: string,
  list: OnlineChapterItem[],
  fallbackIdx: number,
): number {
  const srcSig = extractChapterSignature(targetTitle)
  if (!srcSig.norm) return fallbackIdx

  let bestIdx = -1
  let bestScore = 0

  for (let i = 0; i < list.length; i += 1) {
    const ch = list[i]
    if (!ch || ch.is_volume || !ch.url) continue
    const candSig = extractChapterSignature(ch.title)
    if (!candSig.norm) continue

    let score = 0
    if (candSig.norm === srcSig.norm) {
      score = 1.0
    } else if (srcSig.body && candSig.body && srcSig.body === candSig.body) {
      score = srcSig.num !== null && candSig.num === srcSig.num ? 0.98 : 0.9
    } else if (
      srcSig.norm.length >= 3 &&
      candSig.norm.length >= 3 &&
      (candSig.norm.includes(srcSig.norm) || srcSig.norm.includes(candSig.norm))
    ) {
      const ratio = Math.min(srcSig.norm.length, candSig.norm.length) / Math.max(srcSig.norm.length, candSig.norm.length)
      score = 0.72 + ratio * 0.2
    } else {
      const simFull = diceSimilarity(srcSig.norm, candSig.norm)
      const simBody = srcSig.body && candSig.body ? diceSimilarity(srcSig.body, candSig.body) : 0
      score = Math.max(simFull, simBody)
      if (srcSig.num !== null && candSig.num !== null && srcSig.num === candSig.num) {
        score = Math.max(score, 0.7 + simBody * 0.25)
      }
    }

    // Small proximity bonus to break ties in favor of nearby index
    const dist = Math.abs(i - fallbackIdx)
    const tieBreaker = Math.max(0, 0.02 - dist * 0.0001)
    const finalScore = score + tieBreaker

    if (finalScore > bestScore) {
      bestScore = finalScore
      bestIdx = i
    }
  }

  return bestIdx >= 0 && bestScore >= 0.45 ? bestIdx : fallbackIdx
}

function buildBookJsCtxFrom(c: ReaderContext, idx: number, title: string): BookJsContextInput {
  return {
    name: c.name,
    author: c.author,
    book_url: c.bookUrl,
    toc_url: c.tocUrl,
    cover_url: c.coverUrl ?? null,
    intro: c.intro ?? null,
    dur_chapter_index: idx,
    dur_chapter_title: title,
  }
}

function resolveChapterPos(list: OnlineChapterItem[], rawIdx: number): number {
  if (list.length === 0) return 0
  const byIndex = list.findIndex((c) => c.index === rawIdx)
  if (byIndex >= 0) return byIndex
  return Math.max(0, Math.min(rawIdx, list.length - 1))
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

  // 1. Prefer explicit route query parameters (supports multi-tab & refresh for non-shelf books)
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
        // will be handled below if tocUrl still missing
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
        currentChapterIndex: Number.isFinite(parsedChapter) && parsedChapter >= 0
          ? parsedChapter
          : (matchesStored ? storedCtx!.currentChapterIndex : 0),
      }
    }
  }

  // 2. Resolve from shelfId if not yet constructed
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

  // 3. Fallback to sessionStorage if no query params were provided
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

  const seq = ++loadSeq
  const sourceId = ctx.sourceId
  const bookUrl = ctx.bookUrl

  activeIndex.value = pos
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

    chapterTitle.value = res.content.title || ch.title
    paragraphs.value = res.content.content
      .split(/\n+/)
      .map((s) => s.trim())
      .filter(Boolean)
    contentCached.value = res.cached

    await nextTick()
    if (unmounted || seq !== loadSeq) return
    if (contentScrollRef.value) {
      contentScrollRef.value.scrollTop = 0
    }

    // Persist reading progress in URL query, sessionStorage, and online shelf
    ctx.currentChapterIndex = pos
    syncContextToUrlAndStorage(ctx)
    if (ctx.shelfId) {
      void onlineBookApi
        .updateShelfBook(ctx.shelfId, {
          current_chapter_index: pos,
          current_chapter_title: chapterTitle.value,
          total_chapters: chapters.value.length,
          last_chapter: chapters.value[chapters.value.length - 1]?.title ?? null,
        })
        .catch(() => {})
    }

    // Prefetch next chapter silently into server disk cache
    const nextPos = nextReadableIndex.value
    if (nextPos !== null) {
      const nextCh = chapters.value[nextPos]
      if (nextCh && nextCh.url) {
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

    // Validate new source TOC BEFORE committing context & shelf update
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

    // Commit switched source atomically
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
  unmounted = true
  loadSeq += 1
  tocSeq += 1
  window.removeEventListener('keydown', handleKeydown)
})
</script>

<template>
  <div class="flex h-screen flex-col bg-[var(--md-sys-color-surface)] text-[var(--md-sys-color-on-surface)]">
    <!-- Top Reader Bar -->
    <header
      class="flex h-14 shrink-0 items-center justify-between gap-2 border-b border-[var(--md-sys-color-outline-variant)]/45 bg-[var(--md-sys-color-surface-container)]/90 px-3 sm:px-5"
    >
      <div class="flex min-w-0 items-center gap-2">
        <button
          type="button"
          class="md-icon-btn md-icon-btn-sm md-state"
          title="返回在线搜索"
          aria-label="返回在线搜索"
          @click="router.push({ path: '/book/catalog', query: { scope: 'online' } })"
        >
          <LuArrowLeft class="h-4 w-4" />
        </button>

        <div v-if="readerCtx" class="min-w-0">
          <div class="flex items-center gap-2">
            <span class="truncate text-sm font-semibold">{{ readerCtx.name }}</span>
            <span class="hidden rounded-full bg-[var(--md-sys-color-primary-container)] px-2 py-0.5 text-[10px] font-medium text-[var(--md-sys-color-on-primary-container)] sm:inline">
              {{ readerCtx.sourceName }}
            </span>
          </div>
          <p class="truncate text-[11px] text-[var(--md-sys-color-on-surface-variant)]">
            {{ chapterTitle || '加载中…' }}
            <span v-if="chapters.length > 0">
              · {{ activeIndex + 1 }} / {{ chapters.length }}
            </span>
            <span v-if="contentCached" class="ml-1 opacity-75">· 已缓存</span>
          </p>
        </div>
      </div>

      <div class="flex shrink-0 items-center gap-1">
        <!-- Font size quick controls (synced to themeStore) -->
        <div class="hidden items-center gap-0.5 rounded-full bg-[var(--md-sys-color-surface-container-high)] px-1.5 py-0.5 md:flex">
          <button
            type="button"
            class="md-icon-btn md-icon-btn-sm md-state !h-7 !w-7"
            title="减小字号"
            aria-label="减小字号"
            @click="fontSize = fontSize - 1"
          >
            <LuMinus class="h-3.5 w-3.5" />
          </button>
          <span class="px-1 font-mono text-[11px]">{{ fontSize }}</span>
          <button
            type="button"
            class="md-icon-btn md-icon-btn-sm md-state !h-7 !w-7"
            title="增大字号"
            aria-label="增大字号"
            @click="fontSize = fontSize + 1"
          >
            <LuPlus class="h-3.5 w-3.5" />
          </button>
        </div>

        <button
          type="button"
          class="md-icon-btn md-icon-btn-sm md-state"
          :class="
            typographyOpen
              ? '!bg-[var(--md-sys-color-secondary-container)] !text-[var(--md-sys-color-on-secondary-container)]'
              : ''
          "
          title="排版设置"
          @click="typographyOpen = !typographyOpen"
        >
          <LuSlidersHorizontal class="h-4 w-4" />
        </button>

        <ThemePopover />

        <button
          v-if="readerCtx && !readerCtx.shelfId"
          type="button"
          :disabled="savingShelf"
          class="md-btn md-btn-sm md-btn-tonal md-state"
          @click="handleSaveToShelf"
        >
          <LuBookMarked class="h-3.5 w-3.5" />
          <span class="hidden sm:inline">加入书架</span>
        </button>

        <button
          type="button"
          class="md-icon-btn md-icon-btn-sm md-state"
          title="重新拉取本章正文（忽略缓存）"
          :disabled="loadingContent"
          @click="openChapter(activeIndex, true)"
        >
          <LuRefreshCw class="h-4 w-4" :class="loadingContent ? 'animate-spin' : ''" />
        </button>

        <button
          type="button"
          class="md-icon-btn md-icon-btn-sm md-state"
          title="换源"
          @click="openSwitchSource"
        >
          <LuRepeat class="h-4 w-4" />
        </button>

        <button
          type="button"
          class="md-icon-btn md-icon-btn-sm md-state"
          title="下载 / 打包入库"
          @click="downloadOpen = true"
        >
          <LuDownload class="h-4 w-4" />
        </button>

        <button
          type="button"
          class="md-btn md-btn-sm md-btn-tonal md-state"
          @click="tocOpen = !tocOpen"
        >
          <LuList class="h-3.5 w-3.5" />
          <span class="hidden sm:inline">目录</span>
        </button>
      </div>
    </header>

    <!-- Main Reading Area -->
    <div
      ref="contentScrollRef"
      class="flex-1 overflow-y-auto px-4 py-8 sm:px-8"
    >
      <div
        class="mx-auto"
        :style="{ maxWidth: `${maxWidthPx}px` }"
      >
        <div
          v-if="loadingToc || loadingContent"
          class="flex flex-col items-center justify-center py-24 text-xs text-[var(--md-sys-color-on-surface-variant)]"
        >
          <LuLoader2 class="mb-3 h-6 w-6 animate-spin text-[var(--md-sys-color-primary)]" />
          <span>{{ loadingToc ? '正在获取章节目录…' : '正在拉取章节正文…' }}</span>
        </div>

        <div
          v-else-if="errorMsg"
          class="rounded-2xl bg-[var(--md-sys-color-error-container)] p-5 text-center text-xs text-[var(--md-sys-color-on-error-container)]"
        >
          <p class="font-semibold">加载失败</p>
          <p class="mt-1">{{ errorMsg }}</p>
          <div class="mt-4 flex justify-center gap-2">
            <button
              type="button"
              class="md-btn md-btn-sm md-btn-filled md-state"
              @click="currentChapter ? openChapter(activeIndex, true) : loadTocAndOpen(activeIndex)"
            >
              重试
            </button>
            <button
              type="button"
              class="md-btn md-btn-sm md-btn-tonal md-state"
              @click="openSwitchSource"
            >
              尝试换源
            </button>
          </div>
        </div>

        <article
          v-else
          class="pb-16"
          :style="{ fontFamily: fontFamilyStyle }"
        >
          <h1
            class="mb-8 text-center font-bold tracking-tight"
            :style="{ fontSize: `${Math.round(fontSize * 1.3)}px` }"
          >
            {{ chapterTitle }}
          </h1>

          <div
            class="space-y-4 select-text"
            :style="{
              fontSize: `${fontSize}px`,
              lineHeight: String(lineHeight),
            }"
          >
            <p
              v-for="(para, idx) in paragraphs"
              :key="`p-${idx}`"
              class="indent-[2em] break-words"
            >
              {{ para }}
            </p>
          </div>

          <p
            v-if="paragraphs.length === 0"
            class="py-12 text-center text-xs text-[var(--md-sys-color-on-surface-variant)]"
          >
            本章正文为空，可点击右上角刷新重试或切换书源。
          </p>

          <!-- Chapter Navigation Footer -->
          <div class="mt-12 flex items-center justify-between gap-3 border-t border-[var(--md-sys-color-outline-variant)]/45 pt-6">
            <button
              type="button"
              :disabled="prevReadableIndex === null"
              class="md-btn md-btn-tonal md-state disabled:opacity-40"
              @click="prevReadableIndex !== null && openChapter(prevReadableIndex)"
            >
              <LuChevronLeft class="h-4 w-4" />
              <span>上一章</span>
            </button>

            <button
              type="button"
              class="md-btn md-btn-text md-state"
              @click="tocOpen = true"
            >
              <LuList class="h-4 w-4" />
              <span>目录</span>
            </button>

            <button
              type="button"
              :disabled="nextReadableIndex === null"
              class="md-btn md-btn-filled md-state disabled:opacity-40"
              @click="nextReadableIndex !== null && openChapter(nextReadableIndex)"
            >
              <span>下一章</span>
              <LuChevronRight class="h-4 w-4" />
            </button>
          </div>
        </article>
      </div>
    </div>

    <!-- Typography Side Sheet (synced with themeStore) -->
    <MdSideSheet
      :open="typographyOpen"
      title="排版设置"
      subtitle="与本地阅读器共享字体、字号与行高偏好"
      width-class="w-full max-w-xs"
      @close="typographyOpen = false"
    >
      <div class="space-y-5 text-xs">
        <div>
          <label class="mb-2 block font-semibold text-[var(--md-sys-color-on-surface-variant)]">
            字体
          </label>
          <MdSegmentedButton
            :model-value="themeStore.reader.font_family"
            :options="fontOptions"
            @update:model-value="(v) => themeStore.updateReaderPreferences({ font_family: v })"
          />
        </div>

        <div>
          <div class="mb-1.5 flex justify-between">
            <span class="font-semibold text-[var(--md-sys-color-on-surface-variant)]">字号</span>
            <span class="font-mono">{{ fontSize }} px</span>
          </div>
          <AppSlider
            v-model="fontSize"
            :min="13"
            :max="30"
            :step="1"
            aria-label="字号"
          />
        </div>

        <div>
          <div class="mb-1.5 flex justify-between">
            <span class="font-semibold text-[var(--md-sys-color-on-surface-variant)]">行高</span>
            <span class="font-mono">{{ lineHeight.toFixed(2) }}</span>
          </div>
          <AppSlider
            v-model="lineHeight"
            :min="1.3"
            :max="2.4"
            :step="0.05"
            aria-label="行高"
          />
        </div>
      </div>
    </MdSideSheet>

    <!-- TOC Side Sheet -->
    <MdSideSheet
      :open="tocOpen"
      :title="`目录 (${chapters.length})`"
      width-class="w-full max-w-sm"
      @close="tocOpen = false"
    >
      <template #subheader>
        <div class="relative">
          <LuSearch class="pointer-events-none absolute top-1/2 left-3 h-3.5 w-3.5 -translate-y-1/2 text-[var(--md-sys-color-on-surface-variant)]" />
          <input
            v-model="tocQuery"
            type="text"
            placeholder="筛选章节标题…"
            class="md-input h-9 w-full !pl-9 text-xs"
          />
        </div>
      </template>

      <div class="space-y-0.5">
        <button
          v-for="entry in filteredToc"
          :key="`toc-${entry.pos}-${entry.chapter.index}`"
          type="button"
          :disabled="entry.chapter.is_volume || !entry.chapter.url"
          class="flex w-full items-center justify-between gap-2 rounded-xl px-3 py-2 text-left text-xs transition"
          :class="
            entry.chapter.is_volume
              ? 'mt-2 font-bold text-[var(--md-sys-color-primary)] opacity-90'
              : entry.pos === activeIndex
                ? 'bg-[var(--md-sys-color-secondary-container)] font-semibold text-[var(--md-sys-color-on-secondary-container)]'
                : 'text-[var(--md-sys-color-on-surface-variant)] hover:bg-[var(--md-sys-color-surface-container-high)] hover:text-[var(--md-sys-color-on-surface)]'
          "
          @click="openChapter(entry.pos)"
        >
          <span class="truncate">{{ entry.chapter.title }}</span>
        </button>
      </div>
    </MdSideSheet>

    <!-- Switch Source Dialog -->
    <MdDialog
      :open="switchSourceOpen"
      :title="`换源 · ${readerCtx?.name ?? ''}`"
      subtitle="切换书源后将按当前章节标题自动模糊定位对应章节"
      max-width="max-w-lg"
      @close="switchSourceOpen = false"
    >
      <template #icon>
        <LuRepeat class="h-5 w-5" />
      </template>

      <div class="space-y-2 py-1 text-xs">
        <div v-if="searchingAlts" class="flex items-center justify-center py-10 text-[var(--md-sys-color-on-surface-variant)]">
          <LuLoader2 class="mr-2 h-4 w-4 animate-spin" />
          <span>正在聚合搜索其他书源…</span>
        </div>
        <div v-else-if="altCandidates.length === 0" class="py-10 text-center text-[var(--md-sys-color-on-surface-variant)]">
          暂未搜到其他书源的匹配条目
        </div>
        <button
          v-for="(item, idx) in altCandidates"
          :key="`alt-${item.source_id}-${idx}`"
          type="button"
          :disabled="switchingKey !== null"
          class="md-state flex w-full items-center justify-between gap-3 rounded-2xl bg-[var(--md-sys-color-surface-container)] p-3 text-left transition hover:brightness-105"
          @click="handleSelectAltSource(item)"
        >
          <div class="min-w-0 flex-1">
            <div class="flex items-center gap-2">
              <span class="truncate font-semibold text-[var(--md-sys-color-on-surface)]">
                {{ item.book.name }}
              </span>
              <span class="shrink-0 rounded-full bg-[var(--md-sys-color-primary-container)] px-2 py-0.5 text-[10px] font-medium text-[var(--md-sys-color-on-primary-container)]">
                {{ item.source_name }}
              </span>
            </div>
            <p class="mt-0.5 truncate text-[11px] text-[var(--md-sys-color-on-surface-variant)]">
              {{ item.book.author || '佚名' }}
              <span v-if="item.book.last_chapter"> · {{ item.book.last_chapter }}</span>
            </p>
          </div>
          <LuLoader2
            v-if="switchingKey === `${item.source_id}:${item.book.book_url}`"
            class="h-4 w-4 shrink-0 animate-spin"
          />
        </button>
      </div>
    </MdDialog>

    <OnlineDownloadModal
      :open="downloadOpen"
      :target="downloadTarget"
      @close="downloadOpen = false"
    />
  </div>
</template>
