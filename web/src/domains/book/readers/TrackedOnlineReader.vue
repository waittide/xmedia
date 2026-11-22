<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useBookI18n as useI18n } from '@/domains/book/i18n'
import { onlineBookApi, type TrackedChapterMetaDto } from '@/domains/book/onlineApi'
import type { ReadingLocation, TocItem } from '@/domains/book/types'
import { useAuthStore } from '@/stores/auth'
import ContinuousTextArticle from './ContinuousTextArticle.vue'
import {
  parseTextParagraphs,
  useContinuousChapterReader,
  type ContinuousChapterSection,
} from './useContinuousChapterReader'

const props = defineProps<{
  bookId: string
  initialLocation: ReadingLocation | null
}>()

const emit = defineEmits<{
  progress: [
    payload: {
      progress_percent: number
      locator: string
      chapter_index?: number | null
      scroll_ratio?: number | null
    },
  ]
  'toc-loaded': [items: TocItem[]]
  'switch-source': []
}>()

const { t } = useI18n()
const authStore = useAuthStore()

const articleRef = ref<InstanceType<typeof ContinuousTextArticle> | null>(null)
const scrollContainerRef = computed<HTMLElement | null>(
  () => articleRef.value?.scrollContainerRef ?? null,
)

const chapters = ref<TrackedChapterMetaDto[]>([])
const cachedCount = ref(0)
const totalCount = ref(0)
const loadingToc = ref(true)
const tocLoadFailed = ref(false)
const tocErrorMsg = ref<string | null>(null)

const preloadedSet = new Set<number>()
let tocSeq = 0
let unmounted = false

function toTocItems(list: TrackedChapterMetaDto[]): TocItem[] {
  return list.map((ch) => ({
    index: ch.index,
    title: ch.title,
    href: `chapter:${ch.index}`,
    level: 0,
  }))
}

function findPosByChapterIndex(chapterIdx: number): number {
  if (chapters.value.length === 0) return 0
  const exact = chapters.value.findIndex((c) => c.index === chapterIdx)
  if (exact >= 0) return exact
  return Math.min(chapters.value.length - 1, Math.max(0, chapterIdx))
}

function parseChapterIndexFromLocator(
  locator: string | null | undefined,
  fallbackIndex: number | null | undefined,
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

function preloadUpcoming(currentPos: number, targetBookId: string): void {
  const total = chapters.value.length
  for (const offset of [1, 2]) {
    const nextPos = currentPos + offset
    if (nextPos >= total) continue
    const nextCh = chapters.value[nextPos]
    if (!nextCh) continue
    const nextChapterIdx = nextCh.index
    if (preloadedSet.has(nextChapterIdx)) continue
    if (nextCh.cached) {
      preloadedSet.add(nextChapterIdx)
      continue
    }
    preloadedSet.add(nextChapterIdx)
    void onlineBookApi
      .getTrackedChapter(targetBookId, nextChapterIdx, false)
      .then((res) => {
        if (unmounted || props.bookId !== targetBookId) return
        cachedCount.value = res.cached_chapters
        totalCount.value = res.total_chapters
        const entry = chapters.value.find((c) => c.index === nextChapterIdx)
        if (entry) entry.cached = true
      })
      .catch(() => {
        if (props.bookId === targetBookId) {
          preloadedSet.delete(nextChapterIdx)
        }
      })
  }
}

const reader = useContinuousChapterReader({
  scrollContainerRef,
  isBlocked: () => loadingToc.value,
  getPrevPos: (fromPos) => (fromPos > 0 ? fromPos - 1 : null),
  getNextPos: (fromPos) => (fromPos + 1 < chapters.value.length ? fromPos + 1 : null),
  fetchChapter: async (pos, { forceRefresh }): Promise<ContinuousChapterSection | null> => {
    const targetChapter = chapters.value[pos]
    if (!targetChapter) return null
    const targetBookId = props.bookId
    const res = await onlineBookApi.getTrackedChapter(
      targetBookId,
      targetChapter.index,
      forceRefresh,
    )
    if (unmounted || props.bookId !== targetBookId) return null

    cachedCount.value = res.cached_chapters
    totalCount.value = res.total_chapters
    const entry = chapters.value.find((c) => c.index === targetChapter.index)
    if (entry) entry.cached = true

    return {
      pos,
      chapterIndex: targetChapter.index,
      title: res.title || targetChapter.title || t('reader.chapterTitleFallback', { index: pos + 1 }),
      paragraphs: parseTextParagraphs(res.content),
      cached: res.cached,
    }
  },
  onPrefetch: (pos) => preloadUpcoming(pos, props.bookId),
  onProgress: ({ pos, chapter, scrollRatio }) => {
    const total = Math.max(1, chapters.value.length || totalCount.value || 1)
    const pct = Math.min(100, Math.max(0, ((pos + scrollRatio) / total) * 100))
    emit('progress', {
      progress_percent: pct,
      locator: `chapter:${chapter.chapterIndex}`,
      chapter_index: chapter.chapterIndex,
      scroll_ratio: scrollRatio,
    })
  },
  progressDebounceMs: 250,
})

const cachedSummary = computed(() =>
  totalCount.value > 0
    ? t('reader.cachedProgress', { cached: cachedCount.value, total: totalCount.value })
    : null,
)

async function loadInitial(): Promise<void> {
  const targetBookId = props.bookId
  const seq = ++tocSeq
  reader.resetState()
  loadingToc.value = true
  tocLoadFailed.value = false
  tocErrorMsg.value = null
  try {
    const tocRes = await onlineBookApi.getTrackedToc(targetBookId, false)
    if (unmounted || seq !== tocSeq || props.bookId !== targetBookId) return

    chapters.value = tocRes.chapters
    cachedCount.value = tocRes.cached_chapters
    totalCount.value = tocRes.total_chapters
    emit('toc-loaded', toTocItems(tocRes.chapters))

    if (tocRes.chapters.length === 0) {
      tocLoadFailed.value = true
      tocErrorMsg.value = t('reader.trackedEmptyToc')
      return
    }

    const initialChapterIdx = parseChapterIndexFromLocator(
      props.initialLocation?.locator,
      props.initialLocation?.chapter_index,
    )
    const initialPos = findPosByChapterIndex(initialChapterIdx)
    loadingToc.value = false
    await reader.openChapter(initialPos, {
      scrollRatio: props.initialLocation?.scroll_ratio ?? 0,
    })
  } catch (err) {
    if (unmounted || seq !== tocSeq || props.bookId !== targetBookId) return
    tocLoadFailed.value = true
    tocErrorMsg.value = err instanceof Error ? err.message : String(err)
  } finally {
    if (!unmounted && seq === tocSeq) {
      loadingToc.value = false
    }
  }
}

function handleRetry(): void {
  if (tocLoadFailed.value || chapters.value.length === 0) {
    void loadInitial()
  } else {
    void reader.openChapter(reader.activePos.value, { forceRefresh: true })
  }
}

async function navigateTo(target: string, scrollRatio = 0): Promise<void> {
  const idx = parseChapterIndexFromLocator(target, null)
  const pos = findPosByChapterIndex(idx)
  await reader.openChapter(pos, { scrollRatio })
}

async function reload(): Promise<void> {
  preloadedSet.clear()
  await loadInitial()
}

watch(
  () => props.bookId,
  (nextId, prevId) => {
    if (nextId === prevId) return
    reader.flushProgress()
    preloadedSet.clear()
    chapters.value = []
    void loadInitial()
  },
)

onMounted(() => {
  unmounted = false
  void loadInitial()
})

onBeforeUnmount(() => {
  unmounted = true
  tocSeq += 1
})

defineExpose({ navigateTo, reload })
</script>

<template>
  <ContinuousTextArticle
    ref="articleRef"
    :chapters="reader.loadedChapters.value"
    :active-pos="reader.activePos.value"
    :total-chapters="chapters.length"
    :loading="loadingToc || reader.loadingChapter.value"
    :loading-label="loadingToc ? t('reader.loadingToc') : t('reader.loadingContent')"
    :error-msg="tocErrorMsg || reader.errorMsg.value"
    :appending-next="reader.appendingNext.value"
    :append-error="reader.appendError.value"
    :has-prev="reader.prevPos.value !== null"
    :has-next="reader.nextAppendablePos.value !== null"
    :cached-summary="cachedSummary"
    show-chapter-refetch
    :show-switch-source-on-error="authStore.isAdmin"
    @scroll="reader.handleScroll()"
    @retry="handleRetry"
    @retry-append="reader.appendNextChapter()"
    @prev-chapter="reader.prevPos.value !== null && reader.openChapter(reader.prevPos.value)"
    @next-chapter="
      reader.nextAppendablePos.value !== null && reader.openChapter(reader.nextAppendablePos.value)
    "
    @refetch-chapter="reader.openChapter($event, { forceRefresh: true })"
    @switch-source="emit('switch-source')"
  />
</template>
