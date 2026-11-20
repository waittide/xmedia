<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useBookI18n as useI18n } from '@/domains/book/i18n'
import { LuChevronLeft, LuChevronRight } from 'vue-icons-plus/lu'
import { api } from '@/domains/book/api'
import { useThemeStore } from '@/stores/theme'
import { resolveFontStack } from '@/utils/format'
import type { ReadingLocation, TxtChapterContent } from '@/domains/book/types'

const MAX_CACHED_CHAPTERS = 24

const props = defineProps<{
  bookId: string
  initialLocation: ReadingLocation | null
}>()

const emit = defineEmits<{
  progress: [payload: {
    progress_percent: number
    locator: string
    chapter_index?: number | null
    page_number?: number | null
    scroll_ratio?: number | null
  }]
}>()

const { t } = useI18n()
const themeStore = useThemeStore()

const scrollContainer = ref<HTMLElement | null>(null)
const chapter = ref<TxtChapterContent | null>(null)
const loading = ref(true)
const errorMsg = ref('')

// Lightweight in-memory chapter cache + in-flight request deduplication
const chapterCache = new Map<number, TxtChapterContent>()
const inflightFetches = new Map<number, Promise<TxtChapterContent>>()
let loadSeq = 0

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
    // Refresh LRU order
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

function prefetchAdjacentChapter(index: number, totalChapters: number): void {
  const nextIdx = index + 1
  if (nextIdx >= totalChapters || chapterCache.has(nextIdx) || inflightFetches.has(nextIdx)) {
    return
  }
  void fetchChapterWithCache(nextIdx).catch(() => {
    // Silent background prefetch
  })
}

const paragraphs = computed(() => {
  if (!chapter.value) return []
  return chapter.value.content
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter((line) => line.length > 0)
})

const fontFamilyStyle = computed(() => resolveFontStack(themeStore.reader.font_family))

async function loadChapter(index: number, restoreScrollRatio = 0): Promise<void> {
  const seq = ++loadSeq
  const hasCached = chapterCache.has(index)
  if (!hasCached) {
    loading.value = true
  }
  errorMsg.value = ''
  try {
    const data = await fetchChapterWithCache(index)
    if (seq !== loadSeq) return
    chapter.value = data
    loading.value = false
    await nextTick()
    if (scrollContainer.value) {
      const maxScroll =
        scrollContainer.value.scrollHeight - scrollContainer.value.clientHeight
      scrollContainer.value.scrollTop = maxScroll > 0 ? maxScroll * restoreScrollRatio : 0
    }
    reportCurrentProgress(restoreScrollRatio)
    prefetchAdjacentChapter(data.index, data.total_chapters)
  } catch (err) {
    if (seq !== loadSeq) return
    errorMsg.value = err instanceof Error ? err.message : String(err)
    loading.value = false
  }
}

function reportCurrentProgress(explicitRatio?: number): void {
  if (!chapter.value || !scrollContainer.value) return
  const el = scrollContainer.value
  const maxScroll = el.scrollHeight - el.clientHeight
  const ratio =
    explicitRatio !== undefined
      ? explicitRatio
      : maxScroll > 0
        ? Math.min(1, Math.max(0, el.scrollTop / maxScroll))
        : 0

  const total = Math.max(1, chapter.value.total_chapters)
  const pct = ((chapter.value.index + ratio) / total) * 100

  emit('progress', {
    progress_percent: Math.min(100, Math.max(0, Number(pct.toFixed(2)))),
    locator: `chapter:${chapter.value.index}`,
    chapter_index: chapter.value.index,
    scroll_ratio: Number(ratio.toFixed(4)),
  })
}

async function navigateTo(target: string, scrollRatio = 0): Promise<void> {
  const match = /^chapter:(\d+)$/.exec(target)
  const idx = match ? Number.parseInt(match[1] ?? '0', 10) : 0
  await loadChapter(idx, scrollRatio)
}

function handleKeydown(ev: KeyboardEvent): void {
  if (ev.metaKey || ev.ctrlKey || ev.altKey) return
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
  if (!chapter.value || loading.value) return
  if (ev.key === 'ArrowLeft' && chapter.value.index > 0) {
    ev.preventDefault()
    void loadChapter(chapter.value.index - 1, 0)
  } else if (ev.key === 'ArrowRight' && chapter.value.index + 1 < chapter.value.total_chapters) {
    ev.preventDefault()
    void loadChapter(chapter.value.index + 1, 0)
  }
}

watch(
  () => props.bookId,
  () => {
    chapterCache.clear()
    inflightFetches.clear()
  },
)

defineExpose({ navigateTo })

onMounted(async () => {
  window.addEventListener('keydown', handleKeydown)
  const initIdx = props.initialLocation?.chapter_index ?? 0
  const initRatio = props.initialLocation?.scroll_ratio ?? 0
  await loadChapter(initIdx, initRatio)
})

onBeforeUnmount(() => {
  window.removeEventListener('keydown', handleKeydown)
  chapterCache.clear()
  inflightFetches.clear()
})
</script>

<template>
  <div
    ref="scrollContainer"
    class="h-full w-full overflow-y-auto bg-[var(--md-sys-color-surface)] transition-colors"
    @scroll.passive="reportCurrentProgress()"
  >
    <div
      v-if="loading"
      class="flex h-full items-center justify-center text-sm text-[var(--md-sys-color-on-surface-variant)]"
    >
      {{ t('reader.loadingBook') }}
    </div>

    <div
      v-else-if="errorMsg"
      class="flex h-full items-center justify-center text-sm text-[var(--md-sys-color-error)]"
    >
      {{ errorMsg }}
    </div>

    <article
      v-else-if="chapter"
      class="mx-auto max-w-4xl py-10"
      :style="{
        paddingLeft: `${themeStore.reader.margin_px}px`,
        paddingRight: `${themeStore.reader.margin_px}px`,
        fontFamily: fontFamilyStyle,
        fontSize: `${themeStore.reader.font_size}px`,
        lineHeight: String(themeStore.reader.line_height),
      }"
    >
      <header class="mb-8 border-b border-[var(--md-sys-color-outline-variant)]/60 pb-4">
        <div class="flex flex-wrap items-center gap-2 text-xs font-semibold text-[var(--md-sys-color-primary)]">
          <span
            v-if="chapter.volume_title"
            class="rounded-full bg-[var(--md-sys-color-primary-container)] px-2.5 py-0.5 text-[var(--md-sys-color-on-primary-container)]"
          >
            {{ chapter.volume_title }}
          </span>
          <span>
            {{
              t('reader.chapterIndicator', {
                current: chapter.index + 1,
                total: chapter.total_chapters,
              })
            }}
          </span>
        </div>
        <h1 class="mt-2 text-2xl font-bold text-[var(--md-sys-color-on-surface)]">{{ chapter.title }}</h1>
      </header>

      <div class="space-y-4 text-[var(--md-sys-color-on-surface)]">
        <p v-for="(para, i) in paragraphs" :key="i" class="indent-8 leading-relaxed">
          {{ para }}
        </p>
      </div>

      <footer class="mt-12 flex items-center justify-between border-t border-[var(--md-sys-color-outline-variant)]/60 pt-6 text-sm">
        <button
          type="button"
          :disabled="chapter.index <= 0"
          class="md-btn md-btn-sm md-btn-tonal md-state"
          @click="loadChapter(chapter.index - 1, 0)"
        >
          <LuChevronLeft class="h-4 w-4" />
          <span>{{ t('reader.prevChapter') }}</span>
        </button>

        <span class="text-xs text-[var(--md-sys-color-on-surface-variant)]">
          {{
            t('reader.chapterIndicator', {
              current: chapter.index + 1,
              total: chapter.total_chapters,
            })
          }}
        </span>

        <button
          type="button"
          :disabled="chapter.index + 1 >= chapter.total_chapters"
          class="md-btn md-btn-sm md-btn-filled md-state"
          @click="loadChapter(chapter.index + 1, 0)"
        >
          <span>{{ t('reader.nextChapter') }}</span>
          <LuChevronRight class="h-4 w-4" />
        </button>
      </footer>
    </article>
  </div>
</template>
