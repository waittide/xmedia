<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useComicI18n as useI18n } from '@/domains/comic/i18n'
import ComicReaderShell, {
  type ComicReaderChapterItem,
} from '@/domains/comic/components/ComicReaderShell.vue'
import {
  ONLINE_COMIC_READER_CTX_KEY,
  onlineComicApi,
  resolveOnlineComicCoverUrl,
  type ComicReadingDirection,
  type OnlineComicChapter,
  type OnlineComicPageDto,
  type OnlineComicReaderContextStorage,
} from '@/domains/comic/onlineApi'
import { useSnackbarStore } from '@/stores/snackbar'
import { useThemeStore } from '@/stores/theme'
import type { ReaderFitMode, ReaderMode } from '@/domains/comic/types'

const { t } = useI18n()
const route = useRoute()
const router = useRouter()
const theme = useThemeStore()
const snackbar = useSnackbarStore()

const ctx = ref<OnlineComicReaderContextStorage | null>(null)
const chapters = ref<OnlineComicChapter[]>([])
const pages = ref<OnlineComicPageDto[]>([])
const activeChapterIndex = ref(0)
const activeChapterUrl = ref('')
const activeSourceName = ref('')
const activeDirection = ref<ComicReadingDirection | null>(null)

const loading = ref(true)
const loadError = ref<string | null>(null)
const hasContent = ref(false)

const readerMode = ref<ReaderMode>(theme.comic.default_reader_mode)
const fitMode = ref<ReaderFitMode>(theme.comic.default_fit_mode)
const offsetCoverPage = ref<boolean>(theme.comic.offset_cover_page)
const dimImages = ref<boolean>(theme.comic.dim_images_in_dark)
const webtoonWidth = ref<number>(theme.comic.webtoon_width)

let loadSeq = 0
let detailSeq = 0
let unmounted = false
let userOverrodeMode = false
let syncingUrl = false
let syncUrlSeq = 0

function readQueryParam(keys: string[]): string {
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
    return theme.comic.default_reader_mode === 'ltr_double' ? 'ltr_double' : 'ltr_single'
  }
  if (dir === 'rtl') return 'rtl_double'
  return theme.comic.default_reader_mode
}

function hydratePreferencesFromTheme(dir?: ComicReadingDirection | null): void {
  fitMode.value = theme.comic.default_fit_mode
  offsetCoverPage.value = theme.comic.offset_cover_page
  dimImages.value = theme.comic.dim_images_in_dark
  webtoonWidth.value = theme.comic.webtoon_width
  if (!userOverrodeMode) {
    readerMode.value = mapDirectionToReaderMode(dir)
  }
}

function hydrateContextFromRoute(): OnlineComicReaderContextStorage | null {
  const qSourceId = readQueryParam(['sourceId', 'source_id'])
  const qComicUrl = readQueryParam(['comicUrl', 'comic_url'])
  const qChapterUrl = readQueryParam(['chapterUrl', 'chapter_url'])
  const qTocUrl = readQueryParam(['tocUrl', 'toc_url'])
  const qChapterIdxRaw = readQueryParam(['chapterIndex', 'chapter_index', 'chapter'])
  const qTitle = readQueryParam(['title', 'name'])
  const qSourceName = readQueryParam(['sourceName', 'source_name'])
  const qAuthor = readQueryParam(['author'])
  const qCoverUrl = readQueryParam(['coverUrl', 'cover_url'])
  const qDirRaw = readQueryParam(['readingDirection', 'reading_direction'])
  const qDir: ComicReadingDirection | null =
    qDirRaw === 'rtl' || qDirRaw === 'ltr' || qDirRaw === 'webtoon' ? qDirRaw : null

  const stored = readStoredReaderCtx()
  const matchesStored = Boolean(
    stored &&
      ((!qSourceId && !qComicUrl && !qChapterUrl && stored.sourceId && (stored.chapterUrl || stored.comicUrl)) ||
        (qSourceId &&
          stored.sourceId === qSourceId &&
          (!qComicUrl || !stored.comicUrl || stored.comicUrl === qComicUrl))),
  )

  const sourceId = qSourceId || (matchesStored ? stored!.sourceId : '')
  const comicUrl = qComicUrl || (matchesStored ? stored!.comicUrl : '')
  const chapterUrl = qChapterUrl || (matchesStored ? stored!.chapterUrl : '')
  if (!sourceId || (!chapterUrl && !comicUrl)) {
    return null
  }

  const parsedIdx = qChapterIdxRaw ? Number.parseInt(qChapterIdxRaw, 10) : NaN
  const storedChapters = matchesStored && Array.isArray(stored?.chapters) ? stored!.chapters : []
  let chapterIndex = Number.isFinite(parsedIdx) && parsedIdx >= 0 ? parsedIdx : matchesStored ? stored!.chapterIndex : 0
  if (chapterUrl && storedChapters.length > 0) {
    const matchedIdx = storedChapters.findIndex((c) => c.url === chapterUrl)
    if (matchedIdx >= 0) chapterIndex = matchedIdx
  }

  const merged: OnlineComicReaderContextStorage = {
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
  return merged
}

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

async function hydrateChaptersInBackground(
  sourceId: string,
  comicUrl: string,
  tocUrlHint?: string,
): Promise<void> {
  const seq = ++detailSeq
  try {
    const detailRes = await onlineComicApi.fetchDetail(sourceId, comicUrl, {
      title: ctx.value?.title,
      author: ctx.value?.author,
      comic_url: comicUrl,
      cover_url: ctx.value?.coverUrl ?? null,
    })
    if (unmounted || seq !== detailSeq) return

    let loadedChapters = detailRes.chapters ?? []
    const effectiveTocUrl = detailRes.detail.toc_url?.trim() || tocUrlHint?.trim() || ''
    if (loadedChapters.length === 0 && effectiveTocUrl) {
      const tocRes = await onlineComicApi
        .fetchChapters(sourceId, effectiveTocUrl, detailRes.detail)
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
  }
}

async function openChapter(
  targetChapterUrl: string,
  targetIndex: number,
  seamlessToast = false,
): Promise<void> {
  if (!ctx.value) return
  const seq = ++loadSeq
  loading.value = true
  loadError.value = null
  hasContent.value = false
  activeChapterUrl.value = targetChapterUrl
  activeChapterIndex.value = targetIndex

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

    hasContent.value = true
    loading.value = false

    if (seamlessToast) {
      const label = chapterObj?.title || `#${targetIndex + 1}`
      snackbar.show(t('reader.switchedToChapter', { title: label }))
    }
  } catch (err) {
    if (unmounted || seq !== loadSeq) return
    pages.value = []
    hasContent.value = false
    loadError.value = err instanceof Error ? err.message : String(err)
    loading.value = false
  }
}

async function initReader(): Promise<void> {
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

  if (!hydrated.chapterUrl && hydrated.comicUrl) {
    loading.value = true
    try {
      const detailRes = await onlineComicApi.fetchDetail(hydrated.sourceId, hydrated.comicUrl)
      if (unmounted || seq !== loadSeq) return

      let loadedChapters = detailRes.chapters ?? []
      const effectiveTocUrl = detailRes.detail.toc_url?.trim() || hydrated.tocUrl?.trim() || ''
      if (loadedChapters.length === 0 && effectiveTocUrl) {
        const tocRes = await onlineComicApi.fetchChapters(
          hydrated.sourceId,
          effectiveTocUrl,
          detailRes.detail,
        )
        if (unmounted || seq !== loadSeq) return
        loadedChapters = tocRes.chapters ?? []
      }

      chapters.value = loadedChapters
      activeSourceName.value = detailRes.source_name || activeSourceName.value
      activeDirection.value = detailRes.detail.reading_direction || activeDirection.value

      const targetChapter =
        (loadedChapters[hydrated.chapterIndex] &&
        !loadedChapters[hydrated.chapterIndex]!.is_volume &&
        loadedChapters[hydrated.chapterIndex]!.url
          ? loadedChapters[hydrated.chapterIndex]!
          : loadedChapters.find((c) => !c.is_volume && Boolean(c.url?.trim()))) ?? null

      if (!targetChapter) {
        loading.value = false
        loadError.value = t('online.emptyChapters')
        return
      }

      const resolvedIdx = loadedChapters.findIndex((c) => c.url === targetChapter.url)
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
      await openChapter(targetChapter.url, resolvedIdx >= 0 ? resolvedIdx : 0, false)
    } catch (err) {
      if (unmounted || seq !== loadSeq) return
      loading.value = false
      loadError.value = err instanceof Error ? err.message : String(err)
    }
    return
  }

  await openChapter(hydrated.chapterUrl, hydrated.chapterIndex, false)
  if (!unmounted && chapters.value.length === 0 && hydrated.comicUrl) {
    void hydrateChaptersInBackground(hydrated.sourceId, hydrated.comicUrl, hydrated.tocUrl)
  }
}

function switchChapterByIndex(targetIdx: number): void {
  const target = chapters.value[targetIdx]
  if (!target || target.is_volume || !target.url?.trim() || !ctx.value) return

  const nextQuery: Record<string, string> = {
    sourceId: ctx.value.sourceId,
    chapterUrl: target.url,
    chapterIndex: String(targetIdx),
  }
  if (ctx.value.comicUrl) nextQuery.comicUrl = ctx.value.comicUrl
  if (ctx.value.tocUrl) nextQuery.tocUrl = ctx.value.tocUrl
  if (ctx.value.title) nextQuery.title = ctx.value.title
  if (activeSourceName.value) nextQuery.sourceName = activeSourceName.value
  if (activeDirection.value) nextQuery.readingDirection = activeDirection.value

  const seq = ++syncUrlSeq
  syncingUrl = true
  void router.replace({ path: '/comic/online/read', query: nextQuery }).finally(() => {
    if (seq === syncUrlSeq) {
      syncingUrl = false
    }
  })
  void openChapter(target.url, targetIdx, true)
}

function handleSelectChapterKey(key: string): void {
  const idx = chapterItems.value.findIndex((item) => item.key === key)
  if (idx < 0) return
  switchChapterByIndex(idx)
}

function handleModeChange(mode: ReaderMode): void {
  userOverrodeMode = true
  readerMode.value = mode
  theme.updateComicPreferences({ default_reader_mode: mode })
}

function handleFitChange(fit: ReaderFitMode): void {
  fitMode.value = fit
  theme.updateComicPreferences({ default_fit_mode: fit })
}

function handleOffsetCoverChange(val: boolean): void {
  offsetCoverPage.value = val
  theme.updateComicPreferences({ offset_cover_page: val })
}

function handleDimImagesChange(val: boolean): void {
  dimImages.value = val
  theme.updateComicPreferences({ dim_images_in_dark: val })
}

function handleWebtoonWidthCommit(val: number): void {
  webtoonWidth.value = val
  theme.updateComicPreferences({ webtoon_width: val })
}

function handleRetry(): void {
  if (ctx.value && activeChapterUrl.value) {
    void openChapter(activeChapterUrl.value, activeChapterIndex.value, false)
  } else {
    void initReader()
  }
}

function handleBack(): void {
  if (ctx.value?.sourceId && ctx.value?.comicUrl) {
    const query: Record<string, string> = {
      sourceId: ctx.value.sourceId,
      comicUrl: ctx.value.comicUrl,
    }
    if (ctx.value.title) query.title = ctx.value.title
    if (activeSourceName.value) query.sourceName = activeSourceName.value
    void router.push({ path: '/comic/online/detail', query })
    return
  }
  void router.push({ path: '/comic/series', query: { scope: 'online' } })
}

onMounted(() => {
  unmounted = false
  void initReader()
})

onBeforeUnmount(() => {
  unmounted = true
  loadSeq += 1
  detailSeq += 1
})

watch(
  () => [
    route.query.sourceId,
    route.query.source_id,
    route.query.chapterUrl,
    route.query.chapter_url,
    route.query.comicUrl,
    route.query.comic_url,
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
    :initial-page="1"
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
    :preload-pages="theme.comic.preload_pages"
    :loading="loading"
    :load-error="loadError"
    :has-content="hasContent"
    @update:reader-mode="handleModeChange"
    @update:fit-mode="handleFitChange"
    @update:offset-cover-page="handleOffsetCoverChange"
    @update:dim-images="handleDimImagesChange"
    @update:webtoon-width="webtoonWidth = $event"
    @commit-webtoon-width="handleWebtoonWidthCommit"
    @prev-chapter="prevChapterIndex !== null && switchChapterByIndex(prevChapterIndex)"
    @next-chapter="nextChapterIndex !== null && switchChapterByIndex(nextChapterIndex)"
    @select-chapter="handleSelectChapterKey($event)"
    @back="handleBack"
    @retry="handleRetry"
  >
    <template #header-meta>
      <span
        v-if="activeSourceName"
        class="hidden shrink-0 rounded-full bg-[var(--md-sys-color-primary-container)] px-2 py-0.5 text-[10px] font-semibold text-[var(--md-sys-color-on-primary-container)] sm:inline-flex"
      >
        {{ activeSourceName }}
      </span>
    </template>
  </ComicReaderShell>
</template>
