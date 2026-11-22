<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { api } from '@/domains/book/api'
import { useBookI18n as useI18n } from '@/domains/book/i18n'
import type { ReadingLocation, TxtChapterContent } from '@/domains/book/types'
import ContinuousTextArticle from './ContinuousTextArticle.vue'
import {
  parseTextParagraphs,
  useContinuousChapterReader,
  type ContinuousChapterSection,
} from './useContinuousChapterReader'

const MAX_CACHED_CHAPTERS = 24

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
      page_number?: number | null
      scroll_ratio?: number | null
    },
  ]
}>()

const { t } = useI18n()

const articleRef = ref<InstanceType<typeof ContinuousTextArticle> | null>(null)
const scrollContainerRef = computed<HTMLElement | null>(
  () => articleRef.value?.scrollContainerRef ?? null,
)
const totalChapters = ref(0)

const chapterCache = new Map<number, TxtChapterContent>()
const inflightFetches = new Map<number, Promise<TxtChapterContent>>()

function cacheChapter(index: number, data: TxtChapterContent): void {
  if (chapterCache.has(index)) {
    chapterCache.delete(index)
  }
  chapterCache.set(index, data)
  if (chapterCache.size > MAX_CACHED_CHAPTERS) {
    const oldestKey = chapterCache.keys().next().value
    if (oldestKey !== undefined) {
      chapterCache.delete(oldestKey)
    }
  }
}

function fetchChapterWithCache(index: number): Promise<TxtChapterContent> {
  const cached = chapterCache.get(index)
  if (cached) {
    chapterCache.delete(index)
    chapterCache.set(index, cached)
    return Promise.resolve(cached)
  }
  const inflight = inflightFetches.get(index)
  if (inflight) return inflight

  const currentBookId = props.bookId
  const req = api
    .getTxtChapter(currentBookId, index)
    .then((data) => {
      if (props.bookId === currentBookId) {
        cacheChapter(index, data)
      }
      return data
    })
    .finally(() => {
      inflightFetches.delete(index)
    })

  inflightFetches.set(index, req)
  return req
}

function prefetchAdjacent(index: number): void {
  const nextIdx = index + 1
  if (
    nextIdx >= totalChapters.value ||
    chapterCache.has(nextIdx) ||
    inflightFetches.has(nextIdx)
  ) {
    return
  }
  void fetchChapterWithCache(nextIdx).catch(() => {})
}

const reader = useContinuousChapterReader({
  scrollContainerRef,
  getPrevPos: (fromPos) => (fromPos > 0 ? fromPos - 1 : null),
  getNextPos: (fromPos) => (fromPos + 1 < totalChapters.value ? fromPos + 1 : null),
  fetchChapter: async (pos): Promise<ContinuousChapterSection> => {
    const data = await fetchChapterWithCache(pos)
    totalChapters.value = data.total_chapters
    return {
      pos: data.index,
      chapterIndex: data.index,
      title: data.title,
      volumeTitle: data.volume_title,
      paragraphs: parseTextParagraphs(data.content),
    }
  },
  onPrefetch: (pos) => prefetchAdjacent(pos),
  onProgress: ({ chapter, scrollRatio }) => {
    const total = Math.max(1, totalChapters.value)
    const pct = ((chapter.chapterIndex + scrollRatio) / total) * 100
    emit('progress', {
      progress_percent: Math.min(100, Math.max(0, Number(pct.toFixed(2)))),
      locator: `chapter:${chapter.chapterIndex}`,
      chapter_index: chapter.chapterIndex,
      scroll_ratio: scrollRatio,
    })
  },
})

async function openAt(index: number, scrollRatio = 0): Promise<void> {
  await reader.openChapter(index, {
    scrollRatio,
    silentLoading: chapterCache.has(index),
  })
}

async function navigateTo(target: string, scrollRatio = 0): Promise<void> {
  const match = /^chapter:(\d+)$/.exec(target)
  const idx = match ? Number.parseInt(match[1] ?? '0', 10) : 0
  await openAt(idx, scrollRatio)
}

watch(
  () => props.bookId,
  (nextId, prevId) => {
    if (nextId === prevId) return
    reader.flushProgress()
    chapterCache.clear()
    inflightFetches.clear()
    totalChapters.value = 0
    reader.resetState()
    const initIdx = props.initialLocation?.chapter_index ?? 0
    const initRatio = props.initialLocation?.scroll_ratio ?? 0
    void openAt(initIdx, initRatio)
  },
)

onMounted(() => {
  const initIdx = props.initialLocation?.chapter_index ?? 0
  const initRatio = props.initialLocation?.scroll_ratio ?? 0
  void openAt(initIdx, initRatio)
})

onBeforeUnmount(() => {
  chapterCache.clear()
  inflightFetches.clear()
})

defineExpose({ navigateTo })
</script>

<template>
  <ContinuousTextArticle
    ref="articleRef"
    :chapters="reader.loadedChapters.value"
    :active-pos="reader.activePos.value"
    :total-chapters="totalChapters"
    :loading="reader.loadingChapter.value"
    :loading-label="t('reader.loadingBook')"
    :error-msg="reader.errorMsg.value"
    :appending-next="reader.appendingNext.value"
    :append-error="reader.appendError.value"
    :has-prev="reader.prevPos.value !== null"
    :has-next="reader.nextAppendablePos.value !== null"
    @scroll="reader.handleScroll()"
    @retry="openAt(reader.activePos.value)"
    @retry-append="reader.appendNextChapter()"
    @prev-chapter="reader.prevPos.value !== null && openAt(reader.prevPos.value, 0)"
    @next-chapter="
      reader.nextAppendablePos.value !== null && openAt(reader.nextAppendablePos.value, 0)
    "
  />
</template>
