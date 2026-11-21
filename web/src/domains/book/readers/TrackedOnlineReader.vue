<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
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
const activePos = ref(0)
const chapterTitle = ref('')
const paragraphs = ref<string[]>([])
const chapterCached = ref(false)
const cachedCount = ref(0)
const totalCount = ref(0)
const loadingToc = ref(true)
const tocLoadFailed = ref(false)
const loadingChapter = ref(false)
const errorMsg = ref<string | null>(null)

const preloadedSet = new Set<number>()
let scrollTimer: ReturnType<typeof setTimeout> | null = null
let loadSeq = 0
let unmounted = false

const activeChapter = computed<TrackedChapterMetaDto | null>(
  () => chapters.value[activePos.value] ?? null,
)
const activeChapterIndex = computed<number>(
  () => activeChapter.value?.index ?? activePos.value,
)

function clearScrollTimer(): void {
  if (scrollTimer) {
    clearTimeout(scrollTimer)
    scrollTimer = null
  }
}

function findPosByChapterIndex(chapterIdx: number): number {
  if (chapters.value.length === 0) return 0
  const exact = chapters.value.findIndex((c) => c.index === chapterIdx)
  if (exact >= 0) return exact
  return Math.min(chapters.value.length - 1, Math.max(0, chapterIdx))
}

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

function computeProgressPercent(pos: number, scrollRatio: number): number {
  const total = Math.max(1, chapters.value.length || totalCount.value || 1)
  const ratio = Math.min(1, Math.max(0, scrollRatio))
  return Math.min(100, Math.max(0, ((pos + ratio) / total) * 100))
}

function emitCurrentProgress(scrollRatio?: number): void {
  if (unmounted) return
  const el = scrollContainerRef.value
  const ratio =
    scrollRatio !== undefined
      ? scrollRatio
      : el && el.scrollHeight > el.clientHeight
        ? el.scrollTop / (el.scrollHeight - el.clientHeight)
        : 0
  const clamped = Math.min(1, Math.max(0, ratio))
  const chIdx = activeChapterIndex.value
  emit('progress', {
    progress_percent: computeProgressPercent(activePos.value, clamped),
    locator: `chapter:${chIdx}`,
    chapter_index: chIdx,
    scroll_ratio: clamped,
  })
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

async function openChapterByPos(
  pos: number,
  opts: { forceRefresh?: boolean; scrollRatio?: number } = {},
): Promise<void> {
  clearScrollTimer()
  if (chapters.value.length === 0) return

  const maxPos = Math.max(0, chapters.value.length - 1)
  const targetPos = Math.min(maxPos, Math.max(0, pos))
  const targetChapter = chapters.value[targetPos]
  if (!targetChapter) return

  const targetChapterIdx = targetChapter.index
  const targetBookId = props.bookId
  const seq = ++loadSeq

  activePos.value = targetPos
  loadingChapter.value = true
  errorMsg.value = null

  try {
    const res = await onlineBookApi.getTrackedChapter(
      targetBookId,
      targetChapterIdx,
      Boolean(opts.forceRefresh),
    )
    if (unmounted || seq !== loadSeq || props.bookId !== targetBookId) return

    chapterTitle.value = res.title || targetChapter.title || `第 ${targetPos + 1} 章`
    paragraphs.value = res.content
      .split(/\r?\n/)
      .map((line) => line.trim())
      .filter((line) => line.length > 0)
    chapterCached.value = res.cached
    cachedCount.value = res.cached_chapters
    totalCount.value = res.total_chapters
    const entry = chapters.value.find((c) => c.index === targetChapterIdx)
    if (entry) entry.cached = true

    await nextTick()
    if (unmounted || seq !== loadSeq || props.bookId !== targetBookId) return
    const el = scrollContainerRef.value
    if (el) {
      const targetRatio = opts.scrollRatio ?? 0
      el.scrollTop =
        targetRatio > 0 && el.scrollHeight > el.clientHeight
          ? Math.round(targetRatio * (el.scrollHeight - el.clientHeight))
          : 0
    }
    emitCurrentProgress(opts.scrollRatio ?? 0)
    preloadUpcoming(targetPos, targetBookId)
  } catch (err) {
    if (unmounted || seq !== loadSeq || props.bookId !== targetBookId) return
    errorMsg.value = err instanceof Error ? err.message : String(err)
  } finally {
    if (!unmounted && seq === loadSeq) {
      loadingChapter.value = false
    }
  }
}

async function openChapter(
  chapterIdx: number,
  opts: { forceRefresh?: boolean; scrollRatio?: number } = {},
): Promise<void> {
  const pos = findPosByChapterIndex(chapterIdx)
  await openChapterByPos(pos, opts)
}

async function loadInitial(): Promise<void> {
  clearScrollTimer()
  const targetBookId = props.bookId
  const seq = ++loadSeq
  loadingToc.value = true
  tocLoadFailed.value = false
  errorMsg.value = null
  try {
    const tocRes = await onlineBookApi.getTrackedToc(targetBookId, false)
    if (unmounted || seq !== loadSeq || props.bookId !== targetBookId) return

    chapters.value = tocRes.chapters
    cachedCount.value = tocRes.cached_chapters
    totalCount.value = tocRes.total_chapters
    emit('toc-loaded', toTocItems(tocRes.chapters))

    if (tocRes.chapters.length === 0) {
      tocLoadFailed.value = true
      errorMsg.value = '目录为空，请返回书籍详情刷新目录或切换书源'
      return
    }

    const initialChapterIdx = parseChapterIndexFromLocator(
      props.initialLocation?.locator,
      props.initialLocation?.chapter_index,
    )
    const initialPos = findPosByChapterIndex(initialChapterIdx)
    await openChapterByPos(initialPos, {
      scrollRatio: props.initialLocation?.scroll_ratio ?? 0,
    })
  } catch (err) {
    if (unmounted || seq !== loadSeq || props.bookId !== targetBookId) return
    tocLoadFailed.value = true
    errorMsg.value = err instanceof Error ? err.message : String(err)
  } finally {
    if (!unmounted && seq === loadSeq) {
      loadingToc.value = false
    }
  }
}

function handleRetry(): void {
  if (tocLoadFailed.value || chapters.value.length === 0) {
    void loadInitial()
  } else {
    void openChapterByPos(activePos.value, { forceRefresh: true })
  }
}

function handleScroll(): void {
  clearScrollTimer()
  scrollTimer = setTimeout(() => {
    scrollTimer = null
    emitCurrentProgress()
  }, 250)
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
  if (ev.key === 'ArrowLeft' && activePos.value > 0 && !loadingChapter.value) {
    ev.preventDefault()
    void openChapterByPos(activePos.value - 1)
  } else if (
    ev.key === 'ArrowRight' &&
    activePos.value + 1 < chapters.value.length &&
    !loadingChapter.value
  ) {
    ev.preventDefault()
    void openChapterByPos(activePos.value + 1)
  }
}

async function navigateTo(target: string, scrollRatio = 0): Promise<void> {
  const idx = parseChapterIndexFromLocator(target, null)
  await openChapter(idx, { scrollRatio })
}

defineExpose({ navigateTo })

watch(
  () => props.bookId,
  (nextId, prevId) => {
    if (nextId === prevId) return
    clearScrollTimer()
    preloadedSet.clear()
    chapters.value = []
    paragraphs.value = []
    chapterTitle.value = ''
    activePos.value = 0
    void loadInitial()
  },
)

onMounted(() => {
  unmounted = false
  window.addEventListener('keydown', handleKeydown)
  void loadInitial()
})

onBeforeUnmount(() => {
  unmounted = true
  loadSeq += 1
  window.removeEventListener('keydown', handleKeydown)
  clearScrollTimer()
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
            {{ chapterTitle || activeChapter?.title }}
          </h2>
          <p class="mt-1 flex items-center gap-2 text-xs text-[var(--md-sys-color-on-surface-variant)]">
            <span class="font-mono">
              {{ activePos + 1 }} / {{ Math.max(1, chapters.length) }}
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
          :disabled="loadingChapter || chapters.length === 0"
          title="重新抓取本章"
          @click="openChapterByPos(activePos, { forceRefresh: true })"
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
          @click="handleRetry"
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
          :disabled="activePos <= 0"
          class="md-btn md-btn-tonal md-state"
          @click="openChapterByPos(activePos - 1)"
        >
          <LuChevronLeft class="h-4 w-4" />
          <span>上一章</span>
        </button>

        <span class="font-mono text-xs text-[var(--md-sys-color-on-surface-variant)]">
          {{ activePos + 1 }} / {{ chapters.length }}
        </span>

        <button
          type="button"
          :disabled="activePos + 1 >= chapters.length"
          class="md-btn md-btn-filled md-state"
          @click="openChapterByPos(activePos + 1)"
        >
          <span>下一章</span>
          <LuChevronRight class="h-4 w-4" />
        </button>
      </div>
    </div>
  </div>
</template>
