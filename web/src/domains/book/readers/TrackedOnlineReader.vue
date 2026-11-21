<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref } from 'vue'
import {
  LuChevronLeft,
  LuChevronRight,
  LuCheckCircle2,
  LuLoader2,
  LuRefreshCw,
} from 'vue-icons-plus/lu'
import { useThemeStore } from '@/stores/theme'
import { resolveFontStack } from '@/utils/format'
import {
  onlineBookApi,
  type TrackedChapterMetaDto,
} from '@/domains/book/onlineApi'
import type { ReadingLocation, TocItem } from '@/domains/book/types'

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
}>()

const themeStore = useThemeStore()

const scrollContainerRef = ref<HTMLElement | null>(null)
const chapters = ref<TrackedChapterMetaDto[]>([])
const activeIndex = ref(0)
const chapterTitle = ref('')
const paragraphs = ref<string[]>([])
const chapterCached = ref(false)
const cachedCount = ref(0)
const totalCount = ref(0)
const loadingToc = ref(true)
const loadingChapter = ref(false)
const errorMsg = ref<string | null>(null)

const preloadedSet = new Set<number>()
let scrollTimer: ReturnType<typeof setTimeout> | null = null
let loadSeq = 0

const fontFamilyStyle = computed(() => resolveFontStack(themeStore.reader.font_family))

const contentStyle = computed(() => ({
  fontFamily: fontFamilyStyle.value,
  fontSize: `${themeStore.reader.font_size || 18}px`,
  lineHeight: String(themeStore.reader.line_height || 1.85),
  maxWidth: `${Math.max(520, 920 - (themeStore.reader.margin_px || 48) * 2)}px`,
}))

function toTocItems(list: TrackedChapterMetaDto[]): TocItem[] {
  return list.map((ch) => ({
    index: ch.index,
    title: ch.title,
    href: `chapter:${ch.index}`,
    level: 0,
  }))
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

function computeProgressPercent(idx: number, scrollRatio: number): number {
  const total = Math.max(1, chapters.value.length || totalCount.value || 1)
  const ratio = Math.min(1, Math.max(0, scrollRatio))
  return Math.min(100, Math.max(0, ((idx + ratio) / total) * 100))
}

function emitCurrentProgress(scrollRatio?: number): void {
  const el = scrollContainerRef.value
  const ratio =
    scrollRatio !== undefined
      ? scrollRatio
      : el && el.scrollHeight > el.clientHeight
        ? el.scrollTop / (el.scrollHeight - el.clientHeight)
        : 0
  const clamped = Math.min(1, Math.max(0, ratio))
  emit('progress', {
    progress_percent: computeProgressPercent(activeIndex.value, clamped),
    locator: `chapter:${activeIndex.value}`,
    chapter_index: activeIndex.value,
    scroll_ratio: clamped,
  })
}

function preloadUpcoming(currentIdx: number): void {
  const total = chapters.value.length
  for (const offset of [1, 2]) {
    const nextIdx = currentIdx + offset
    if (nextIdx >= total || preloadedSet.has(nextIdx)) continue
    if (chapters.value[nextIdx]?.cached) {
      preloadedSet.add(nextIdx)
      continue
    }
    preloadedSet.add(nextIdx)
    void onlineBookApi
      .getTrackedChapter(props.bookId, nextIdx, false)
      .then((res) => {
        cachedCount.value = res.cached_chapters
        totalCount.value = res.total_chapters
        const entry = chapters.value[nextIdx]
        if (entry) entry.cached = true
      })
      .catch(() => {
        preloadedSet.delete(nextIdx)
      })
  }
}

async function openChapter(
  index: number,
  opts: { forceRefresh?: boolean; scrollRatio?: number } = {},
): Promise<void> {
  const maxIdx = Math.max(0, chapters.value.length - 1)
  const targetIdx = Math.min(maxIdx, Math.max(0, index))
  const seq = ++loadSeq

  activeIndex.value = targetIdx
  loadingChapter.value = true
  errorMsg.value = null

  try {
    const res = await onlineBookApi.getTrackedChapter(
      props.bookId,
      targetIdx,
      Boolean(opts.forceRefresh),
    )
    if (seq !== loadSeq) return

    chapterTitle.value = res.title || chapters.value[targetIdx]?.title || `第 ${targetIdx + 1} 章`
    paragraphs.value = res.content
      .split(/\r?\n/)
      .map((line) => line.trim())
      .filter((line) => line.length > 0)
    chapterCached.value = res.cached
    cachedCount.value = res.cached_chapters
    totalCount.value = res.total_chapters
    const entry = chapters.value[targetIdx]
    if (entry) entry.cached = true

    await nextTick()
    const el = scrollContainerRef.value
    if (el) {
      const targetRatio = opts.scrollRatio ?? 0
      el.scrollTop =
        targetRatio > 0 && el.scrollHeight > el.clientHeight
          ? Math.round(targetRatio * (el.scrollHeight - el.clientHeight))
          : 0
    }
    emitCurrentProgress(opts.scrollRatio ?? 0)
    preloadUpcoming(targetIdx)
  } catch (err) {
    if (seq !== loadSeq) return
    errorMsg.value = err instanceof Error ? err.message : String(err)
  } finally {
    if (seq === loadSeq) {
      loadingChapter.value = false
    }
  }
}

async function loadInitial(): Promise<void> {
  loadingToc.value = true
  errorMsg.value = null
  try {
    const tocRes = await onlineBookApi.getTrackedToc(props.bookId, false)
    chapters.value = tocRes.chapters
    cachedCount.value = tocRes.cached_chapters
    totalCount.value = tocRes.total_chapters
    emit('toc-loaded', toTocItems(tocRes.chapters))

    if (tocRes.chapters.length === 0) {
      errorMsg.value = '目录为空，请返回书籍详情刷新目录或切换书源'
      return
    }

    const initialIdx = parseChapterIndexFromLocator(
      props.initialLocation?.locator,
      props.initialLocation?.chapter_index,
    )
    await openChapter(initialIdx, {
      scrollRatio: props.initialLocation?.scroll_ratio ?? 0,
    })
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
  } finally {
    loadingToc.value = false
  }
}

function handleScroll(): void {
  if (scrollTimer) clearTimeout(scrollTimer)
  scrollTimer = setTimeout(() => {
    scrollTimer = null
    emitCurrentProgress()
  }, 250)
}

function handleKeydown(ev: KeyboardEvent): void {
  const target = ev.target
  if (
    target instanceof HTMLElement &&
    (target.tagName === 'INPUT' || target.tagName === 'TEXTAREA' || target.isContentEditable)
  ) {
    return
  }
  if (ev.key === 'ArrowLeft' && activeIndex.value > 0 && !loadingChapter.value) {
    ev.preventDefault()
    void openChapter(activeIndex.value - 1)
  } else if (
    ev.key === 'ArrowRight' &&
    activeIndex.value + 1 < chapters.value.length &&
    !loadingChapter.value
  ) {
    ev.preventDefault()
    void openChapter(activeIndex.value + 1)
  }
}

async function navigateTo(target: string, scrollRatio = 0): Promise<void> {
  const idx = parseChapterIndexFromLocator(target, null)
  await openChapter(idx, { scrollRatio })
}

defineExpose({ navigateTo })

onMounted(() => {
  window.addEventListener('keydown', handleKeydown)
  void loadInitial()
})

onBeforeUnmount(() => {
  window.removeEventListener('keydown', handleKeydown)
  if (scrollTimer) clearTimeout(scrollTimer)
})
</script>

<template>
  <div
    ref="scrollContainerRef"
    class="flex-1 overflow-y-auto bg-[var(--md-sys-color-surface)] px-5 py-8 sm:px-10"
    @scroll.passive="handleScroll"
  >
    <div v-if="loadingToc" class="flex flex-col items-center justify-center gap-2 py-24 text-xs text-[var(--md-sys-color-on-surface-variant)]">
      <LuLoader2 class="h-5 w-5 animate-spin text-[var(--md-sys-color-primary)]" />
      <span>正在加载目录…</span>
    </div>

    <div v-else class="mx-auto w-full pb-20" :style="contentStyle">
      <!-- Chapter Header -->
      <div class="mb-6 flex flex-wrap items-center justify-between gap-2 border-b border-[var(--md-sys-color-outline-variant)]/45 pb-4">
        <div class="min-w-0 flex-1">
          <h2 class="text-xl font-bold leading-snug text-[var(--md-sys-color-on-surface)] sm:text-2xl">
            {{ chapterTitle || chapters[activeIndex]?.title }}
          </h2>
          <p class="mt-1 flex items-center gap-2 text-xs text-[var(--md-sys-color-on-surface-variant)]">
            <span class="font-mono">
              {{ activeIndex + 1 }} / {{ Math.max(1, chapters.length) }}
            </span>
            <span v-if="totalCount > 0" class="inline-flex items-center gap-1">
              <LuCheckCircle2 class="h-3.5 w-3.5 text-[var(--md-sys-color-primary)]" />
              <span>已缓存 {{ cachedCount }}/{{ totalCount }}</span>
            </span>
          </p>
        </div>

        <button
          type="button"
          class="md-icon-btn md-icon-btn-sm md-state"
          :disabled="loadingChapter"
          title="重新抓取本章"
          @click="openChapter(activeIndex, { forceRefresh: true })"
        >
          <LuRefreshCw class="h-4 w-4" :class="loadingChapter ? 'animate-spin' : ''" />
        </button>
      </div>

      <!-- Loading Chapter -->
      <div
        v-if="loadingChapter"
        class="flex flex-col items-center justify-center gap-2 py-20 text-xs text-[var(--md-sys-color-on-surface-variant)]"
      >
        <LuLoader2 class="h-5 w-5 animate-spin text-[var(--md-sys-color-primary)]" />
        <span>正在获取章节正文…</span>
      </div>

      <!-- Error State -->
      <div
        v-else-if="errorMsg"
        class="rounded-2xl bg-[var(--md-sys-color-error-container)] px-5 py-6 text-center text-xs text-[var(--md-sys-color-on-error-container)]"
      >
        <p>{{ errorMsg }}</p>
        <button
          type="button"
          class="md-btn md-btn-sm md-btn-filled mt-4"
          @click="openChapter(activeIndex, { forceRefresh: true })"
        >
          重试
        </button>
      </div>

      <!-- Chapter Paragraphs -->
      <article v-else class="space-y-4 text-[var(--md-sys-color-on-surface)]">
        <p
          v-for="(para, idx) in paragraphs"
          :key="idx"
          class="indent-[2em] break-words"
        >
          {{ para }}
        </p>
      </article>

      <!-- Bottom Chapter Navigation -->
      <div
        v-if="!loadingChapter && chapters.length > 0"
        class="mt-12 flex items-center justify-between gap-3 border-t border-[var(--md-sys-color-outline-variant)]/45 pt-6"
      >
        <button
          type="button"
          :disabled="activeIndex <= 0"
          class="md-btn md-btn-tonal md-state"
          @click="openChapter(activeIndex - 1)"
        >
          <LuChevronLeft class="h-4 w-4" />
          <span>上一章</span>
        </button>

        <span class="font-mono text-xs text-[var(--md-sys-color-on-surface-variant)]">
          {{ activeIndex + 1 }} / {{ chapters.length }}
        </span>

        <button
          type="button"
          :disabled="activeIndex + 1 >= chapters.length"
          class="md-btn md-btn-filled md-state"
          @click="openChapter(activeIndex + 1)"
        >
          <span>下一章</span>
          <LuChevronRight class="h-4 w-4" />
        </button>
      </div>
    </div>
  </div>
</template>
