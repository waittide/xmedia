<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useBookI18n as useI18n } from '@/domains/book/i18n'
import { LuChevronLeft, LuChevronRight, LuLoader2 } from 'vue-icons-plus/lu'
import { api } from '@/domains/book/api'
import { useThemeStore } from '@/stores/theme'
import { resolveFontStack } from '@/utils/format'
import type { ReadingLocation, TxtChapterContent } from '@/domains/book/types'

const MAX_CACHED_CHAPTERS = 24
const AUTO_LOAD_THRESHOLD_PX = 400

interface LoadedTxtChapter {
  data: TxtChapterContent
  paragraphs: string[]
}

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
const loadedChapters = ref<LoadedTxtChapter[]>([])
const activeChapterIndex = ref(0)
const loading = ref(true)
const appendingNext = ref(false)
const appendError = ref('')
const errorMsg = ref('')

// Lightweight in-memory chapter cache + in-flight request deduplication
const chapterCache = new Map<number, TxtChapterContent>()
const inflightFetches = new Map<number, Promise<TxtChapterContent>>()
let loadSeq = 0
let scrollTimer: ReturnType<typeof setTimeout> | null = null

const activeChapter = computed<TxtChapterContent | null>(
  () =>
    loadedChapters.value.find((c) => c.data.index === activeChapterIndex.value)?.data ??
    loadedChapters.value[0]?.data ??
    null,
)

const lastLoadedChapter = computed<TxtChapterContent | null>(
  () => loadedChapters.value[loadedChapters.value.length - 1]?.data ?? null,
)

const nextAppendableIndex = computed<number | null>(() => {
  const last = lastLoadedChapter.value
  if (!last) return null
  return last.index + 1 < last.total_chapters ? last.index + 1 : null
})

function parseParagraphs(content: string): string[] {
  return content
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter((line) => line.length > 0)
}

function clearScrollTimer(): void {
  if (scrollTimer) {
    clearTimeout(scrollTimer)
    scrollTimer = null
  }
}

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

const fontFamilyStyle = computed(() => resolveFontStack(themeStore.reader.font_family))

let unmounted = false

async function loadChapter(index: number, restoreScrollRatio = 0): Promise<void> {
  clearScrollTimer()
  const seq = ++loadSeq
  const hasCached = chapterCache.has(index)
  if (!hasCached) {
    loading.value = true
  }
  loadedChapters.value = []
  appendingNext.value = false
  appendError.value = ''
  errorMsg.value = ''
  try {
    const data = await fetchChapterWithCache(index)
    if (unmounted || seq !== loadSeq) return
    const parsedParagraphs = parseParagraphs(data.content)
    activeChapterIndex.value = data.index
    loadedChapters.value = [{ data, paragraphs: parsedParagraphs }]
    loading.value = false
    await nextTick()
    if (unmounted || seq !== loadSeq) return
    if (scrollContainer.value) {
      const maxScroll =
        scrollContainer.value.scrollHeight - scrollContainer.value.clientHeight
      scrollContainer.value.scrollTop = maxScroll > 0 ? maxScroll * restoreScrollRatio : 0
    }
    reportCurrentProgress(restoreScrollRatio)
    prefetchAdjacentChapter(data.index, data.total_chapters)

    if (
      parsedParagraphs.length > 0 &&
      data.index + 1 < data.total_chapters &&
      scrollContainer.value &&
      scrollContainer.value.scrollHeight <= scrollContainer.value.clientHeight + 120
    ) {
      void appendNextChapter()
    }
  } catch (err) {
    if (unmounted || seq !== loadSeq) return
    errorMsg.value = err instanceof Error ? err.message : String(err)
    loading.value = false
  }
}

async function appendNextChapter(): Promise<void> {
  const targetIdx = nextAppendableIndex.value
  if (targetIdx === null || loading.value || appendingNext.value) return
  if (loadedChapters.value.some((c) => c.data.index === targetIdx)) return

  const seq = loadSeq
  appendingNext.value = true
  appendError.value = ''
  try {
    const data = await fetchChapterWithCache(targetIdx)
    if (unmounted || seq !== loadSeq) return
    if (loadedChapters.value.some((c) => c.data.index === targetIdx)) return

    loadedChapters.value.push({ data, paragraphs: parseParagraphs(data.content) })
    prefetchAdjacentChapter(data.index, data.total_chapters)
  } catch (err) {
    if (unmounted || seq !== loadSeq) return
    appendError.value = err instanceof Error ? err.message : String(err)
  } finally {
    if (!unmounted && seq === loadSeq) {
      appendingNext.value = false
    }
  }
}

function computeActiveChapterScrollRatio(): number {
  const el = scrollContainer.value
  const current = activeChapter.value
  if (!el || !current) return 0
  const activeSection = el.querySelector<HTMLElement>(`[data-chapter-index="${current.index}"]`)
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

function reportCurrentProgress(explicitRatio?: number): void {
  const current = activeChapter.value
  if (!current || !scrollContainer.value) return
  const ratio = explicitRatio !== undefined ? explicitRatio : computeActiveChapterScrollRatio()

  const total = Math.max(1, current.total_chapters)
  const pct = ((current.index + ratio) / total) * 100

  emit('progress', {
    progress_percent: Math.min(100, Math.max(0, Number(pct.toFixed(2)))),
    locator: `chapter:${current.index}`,
    chapter_index: current.index,
    scroll_ratio: Number(ratio.toFixed(4)),
  })
}

function handleScroll(): void {
  const el = scrollContainer.value
  if (el && !loading.value && loadedChapters.value.length > 0) {
    const containerRect = el.getBoundingClientRect()
    const anchorY = containerRect.top + Math.min(200, containerRect.height * 0.35)
    const sections = el.querySelectorAll<HTMLElement>('[data-chapter-index]')
    let visibleIdx = loadedChapters.value[0]!.data.index
    for (const sec of sections) {
      const rect = sec.getBoundingClientRect()
      if (rect.top <= anchorY) {
        const parsed = Number.parseInt(sec.dataset.chapterIndex ?? '', 10)
        if (Number.isFinite(parsed)) {
          visibleIdx = parsed
        }
      } else {
        break
      }
    }
    if (visibleIdx !== activeChapterIndex.value) {
      activeChapterIndex.value = visibleIdx
      const cur = loadedChapters.value.find((c) => c.data.index === visibleIdx)?.data
      if (cur) {
        prefetchAdjacentChapter(cur.index, cur.total_chapters)
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

  clearScrollTimer()
  scrollTimer = setTimeout(() => {
    scrollTimer = null
    reportCurrentProgress()
  }, 200)
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
  const current = activeChapter.value
  if (!current || loading.value) return
  if (ev.key === 'ArrowLeft' && current.index > 0) {
    ev.preventDefault()
    void loadChapter(current.index - 1, 0)
  } else if (ev.key === 'ArrowRight' && current.index + 1 < current.total_chapters) {
    ev.preventDefault()
    void loadChapter(current.index + 1, 0)
  }
}

watch(
  () => props.bookId,
  (nextId, prevId) => {
    if (nextId === prevId) return
    if (scrollTimer) {
      clearScrollTimer()
      reportCurrentProgress()
    }
    chapterCache.clear()
    inflightFetches.clear()
    loadedChapters.value = []
    appendingNext.value = false
    appendError.value = ''
    const initIdx = props.initialLocation?.chapter_index ?? 0
    const initRatio = props.initialLocation?.scroll_ratio ?? 0
    void loadChapter(initIdx, initRatio)
  },
)

defineExpose({ navigateTo })

onMounted(async () => {
  unmounted = false
  window.addEventListener('keydown', handleKeydown)
  const initIdx = props.initialLocation?.chapter_index ?? 0
  const initRatio = props.initialLocation?.scroll_ratio ?? 0
  await loadChapter(initIdx, initRatio)
})

onBeforeUnmount(() => {
  if (scrollTimer) {
    clearScrollTimer()
    reportCurrentProgress()
  }
  unmounted = true
  loadSeq += 1
  window.removeEventListener('keydown', handleKeydown)
  chapterCache.clear()
  inflightFetches.clear()
})
</script>

<template>
  <div
    ref="scrollContainer"
    class="h-full w-full overflow-y-auto bg-[var(--md-sys-color-surface)] transition-colors"
    @scroll.passive="handleScroll"
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
      v-else-if="loadedChapters.length > 0 && activeChapter"
      class="mx-auto max-w-4xl py-10"
      :style="{
        paddingLeft: `${themeStore.reader.margin_px}px`,
        paddingRight: `${themeStore.reader.margin_px}px`,
        fontFamily: fontFamilyStyle,
        fontSize: `${themeStore.reader.font_size}px`,
        lineHeight: String(themeStore.reader.line_height),
      }"
    >
      <section
        v-for="(item, itemIdx) in loadedChapters"
        :key="`txt-ch-${item.data.index}`"
        :data-chapter-index="item.data.index"
        :class="itemIdx > 0 ? 'mt-14 border-t border-[var(--md-sys-color-outline-variant)]/55 pt-10' : ''"
      >
        <header class="mb-8 border-b border-[var(--md-sys-color-outline-variant)]/60 pb-4">
          <div class="flex flex-wrap items-center gap-2 text-xs font-semibold text-[var(--md-sys-color-primary)]">
            <span
              v-if="item.data.volume_title"
              class="rounded-full bg-[var(--md-sys-color-primary-container)] px-2.5 py-0.5 text-[var(--md-sys-color-on-primary-container)]"
            >
              {{ item.data.volume_title }}
            </span>
            <span>
              {{
                t('reader.chapterIndicator', {
                  current: item.data.index + 1,
                  total: item.data.total_chapters,
                })
              }}
            </span>
          </div>
          <h1 class="mt-2 text-2xl font-bold text-[var(--md-sys-color-on-surface)]">{{ item.data.title }}</h1>
        </header>

        <div class="space-y-4 text-[var(--md-sys-color-on-surface)]">
          <p v-for="(para, i) in item.paragraphs" :key="`txt-p-${item.data.index}-${i}`" class="indent-8">
            {{ para }}
          </p>
        </div>
      </section>

      <div class="mt-10 flex flex-col items-center gap-3 text-xs text-[var(--md-sys-color-on-surface-variant)]">
        <div v-if="appendingNext" class="flex items-center gap-2 py-2">
          <LuLoader2 class="h-4 w-4 animate-spin text-[var(--md-sys-color-primary)]" />
          <span>{{ t('common.loading') }}</span>
        </div>
        <div
          v-else-if="appendError"
          class="flex flex-wrap items-center justify-center gap-2 rounded-2xl bg-[var(--md-sys-color-error-container)] px-4 py-2.5 text-[var(--md-sys-color-on-error-container)]"
        >
          <span>{{ appendError }}</span>
          <button
            type="button"
            class="md-btn md-btn-sm md-btn-filled md-state"
            @click="appendNextChapter()"
          >
            重试
          </button>
        </div>
      </div>

      <footer class="mt-6 flex items-center justify-between border-t border-[var(--md-sys-color-outline-variant)]/60 pt-6 text-sm">
        <button
          type="button"
          :disabled="activeChapter.index <= 0"
          class="md-btn md-btn-sm md-btn-tonal md-state"
          @click="loadChapter(activeChapter.index - 1, 0)"
        >
          <LuChevronLeft class="h-4 w-4" />
          <span>{{ t('reader.prevChapter') }}</span>
        </button>

        <span class="text-xs text-[var(--md-sys-color-on-surface-variant)]">
          {{
            t('reader.chapterIndicator', {
              current: activeChapter.index + 1,
              total: activeChapter.total_chapters,
            })
          }}
        </span>

        <button
          type="button"
          :disabled="nextAppendableIndex === null || appendingNext"
          class="md-btn md-btn-sm md-btn-filled md-state"
          @click="nextAppendableIndex !== null && loadChapter(nextAppendableIndex, 0)"
        >
          <span>{{ t('reader.nextChapter') }}</span>
          <LuChevronRight class="h-4 w-4" />
        </button>
      </footer>
    </article>
  </div>
</template>
