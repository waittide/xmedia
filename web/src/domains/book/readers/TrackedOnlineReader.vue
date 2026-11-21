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

interface LoadedTrackedChapter {
  pos: number
  chapterIndex: number
  title: string
  paragraphs: string[]
  cached: boolean
}

const scrollContainerRef = ref<HTMLElement | null>(null)
const chapters = ref<TrackedChapterMetaDto[]>([])
const activePos = ref(0)
const loadedChapters = ref<LoadedTrackedChapter[]>([])
const cachedCount = ref(0)
const totalCount = ref(0)
const loadingToc = ref(true)
const tocLoadFailed = ref(false)
const loadingChapter = ref(false)
const appendingNext = ref(false)
const appendError = ref<string | null>(null)
const errorMsg = ref<string | null>(null)

const AUTO_LOAD_THRESHOLD_PX = 400
const preloadedSet = new Set<number>()
let scrollTimer: ReturnType<typeof setTimeout> | null = null
let loadSeq = 0
let tocSeq = 0
let unmounted = false

const activeChapter = computed<TrackedChapterMetaDto | null>(
  () => chapters.value[activePos.value] ?? null,
)
const activeChapterIndex = computed<number>(
  () => activeChapter.value?.index ?? activePos.value,
)
const lastLoadedPos = computed<number | null>(() => {
  const last = loadedChapters.value[loadedChapters.value.length - 1]
  return last ? last.pos : null
})
const nextAppendablePos = computed<number | null>(() => {
  const basePos = lastLoadedPos.value ?? activePos.value
  return basePos + 1 < chapters.value.length ? basePos + 1 : null
})

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

function computeActiveChapterScrollRatio(): number {
  const el = scrollContainerRef.value
  if (!el) return 0
  const activeSection = el.querySelector<HTMLElement>(
    `[data-chapter-pos="${activePos.value}"]`,
  )
  if (activeSection) {
    const containerRect = el.getBoundingClientRect()
    const secRect = activeSection.getBoundingClientRect()
    const scrollable = Math.max(1, secRect.height - containerRect.height)
    const scrolledInside = containerRect.top - secRect.top
    return Math.min(1, Math.max(0, scrolledInside / scrollable))
  }
  return el.scrollHeight > el.clientHeight
    ? el.scrollTop / (el.scrollHeight - el.clientHeight)
    : 0
}

function emitCurrentProgress(scrollRatio?: number): void {
  if (unmounted) return
  const ratio = scrollRatio !== undefined ? scrollRatio : computeActiveChapterScrollRatio()
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
  loadedChapters.value = []
  appendingNext.value = false
  appendError.value = null
  loadingChapter.value = true
  errorMsg.value = null

  try {
    const res = await onlineBookApi.getTrackedChapter(
      targetBookId,
      targetChapterIdx,
      Boolean(opts.forceRefresh),
    )
    if (unmounted || seq !== loadSeq || props.bookId !== targetBookId) return

    const resolvedTitle = res.title || targetChapter.title || `第 ${targetPos + 1} 章`
    const parsedParagraphs = res.content
      .split(/\r?\n/)
      .map((line) => line.trim())
      .filter((line) => line.length > 0)

    loadedChapters.value = [
      {
        pos: targetPos,
        chapterIndex: targetChapterIdx,
        title: resolvedTitle,
        paragraphs: parsedParagraphs,
        cached: res.cached,
      },
    ]
    cachedCount.value = res.cached_chapters
    totalCount.value = res.total_chapters
    const entry = chapters.value.find((c) => c.index === targetChapterIdx)
    if (entry) entry.cached = true
    loadingChapter.value = false

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

    // Auto-append one chapter if a short non-empty chapter doesn't fill the viewport
    if (
      parsedParagraphs.length > 0 &&
      targetPos + 1 < chapters.value.length &&
      el &&
      el.scrollHeight <= el.clientHeight + 120
    ) {
      void appendNextChapter()
    }
  } catch (err) {
    if (unmounted || seq !== loadSeq || props.bookId !== targetBookId) return
    errorMsg.value = err instanceof Error ? err.message : String(err)
  } finally {
    if (!unmounted && seq === loadSeq) {
      loadingChapter.value = false
    }
  }
}

async function appendNextChapter(): Promise<void> {
  const targetPos = nextAppendablePos.value
  if (targetPos === null) return
  if (loadingToc.value || loadingChapter.value || appendingNext.value) return
  if (loadedChapters.value.some((c) => c.pos === targetPos)) return

  const targetChapter = chapters.value[targetPos]
  if (!targetChapter) return

  const targetChapterIdx = targetChapter.index
  const targetBookId = props.bookId
  const seq = loadSeq
  appendingNext.value = true
  appendError.value = null

  try {
    const res = await onlineBookApi.getTrackedChapter(targetBookId, targetChapterIdx, false)
    if (unmounted || seq !== loadSeq || props.bookId !== targetBookId) return
    if (loadedChapters.value.some((c) => c.pos === targetPos)) return

    const resolvedTitle = res.title || targetChapter.title || `第 ${targetPos + 1} 章`
    const parsedParagraphs = res.content
      .split(/\r?\n/)
      .map((line) => line.trim())
      .filter((line) => line.length > 0)

    loadedChapters.value.push({
      pos: targetPos,
      chapterIndex: targetChapterIdx,
      title: resolvedTitle,
      paragraphs: parsedParagraphs,
      cached: res.cached,
    })
    cachedCount.value = res.cached_chapters
    totalCount.value = res.total_chapters
    const entry = chapters.value.find((c) => c.index === targetChapterIdx)
    if (entry) entry.cached = true

    preloadUpcoming(targetPos, targetBookId)
  } catch (err) {
    if (unmounted || seq !== loadSeq || props.bookId !== targetBookId) return
    appendError.value = err instanceof Error ? err.message : String(err)
  } finally {
    if (!unmounted && seq === loadSeq) {
      appendingNext.value = false
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
  const seq = ++tocSeq
  loadSeq += 1
  loadingToc.value = true
  tocLoadFailed.value = false
  errorMsg.value = null
  try {
    const tocRes = await onlineBookApi.getTrackedToc(targetBookId, false)
    if (unmounted || seq !== tocSeq || props.bookId !== targetBookId) return

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
    loadingToc.value = false
    await openChapterByPos(initialPos, {
      scrollRatio: props.initialLocation?.scroll_ratio ?? 0,
    })
  } catch (err) {
    if (unmounted || seq !== tocSeq || props.bookId !== targetBookId) return
    tocLoadFailed.value = true
    errorMsg.value = err instanceof Error ? err.message : String(err)
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
    void openChapterByPos(activePos.value, { forceRefresh: true })
  }
}

function handleScroll(): void {
  const el = scrollContainerRef.value
  if (el && !loadingToc.value && !loadingChapter.value && loadedChapters.value.length > 0) {
    // Sync activePos to the chapter currently occupying the top reading zone
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
    if (visiblePos !== activePos.value) {
      activePos.value = visiblePos
      preloadUpcoming(visiblePos, props.bookId)
    }

    // Auto-append next chapter when nearing the bottom
    const remaining = el.scrollHeight - el.scrollTop - el.clientHeight
    if (
      remaining <= AUTO_LOAD_THRESHOLD_PX &&
      !appendingNext.value &&
      !appendError.value &&
      nextAppendablePos.value !== null
    ) {
      void appendNextChapter()
    }
  }

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
    if (scrollTimer) {
      clearScrollTimer()
      emitCurrentProgress()
    }
    preloadedSet.clear()
    chapters.value = []
    loadedChapters.value = []
    appendingNext.value = false
    appendError.value = null
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
  if (scrollTimer) {
    clearScrollTimer()
    emitCurrentProgress()
  }
  unmounted = true
  loadSeq += 1
  tocSeq += 1
  window.removeEventListener('keydown', handleKeydown)
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

      <!-- Continuous Chapter Sections -->
      <div v-else>
        <section
          v-for="(item, itemIdx) in loadedChapters"
          :key="`tracked-${item.pos}-${item.chapterIndex}`"
          :data-chapter-pos="item.pos"
          :class="itemIdx > 0 ? 'mt-14 border-t border-[var(--md-sys-color-outline-variant)]/45 pt-10' : ''"
        >
          <!-- Chapter Header -->
          <div class="mb-6 flex flex-wrap items-center justify-between gap-2 border-b border-[var(--md-sys-color-outline-variant)]/45 pb-4">
            <div class="min-w-0 flex-1">
              <h2 class="text-xl font-bold leading-snug text-[var(--md-sys-color-on-surface)] sm:text-2xl">
                {{ item.title }}
              </h2>
              <p class="mt-1 flex items-center gap-2 text-xs text-[var(--md-sys-color-on-surface-variant)]">
                <span class="font-mono">
                  {{ item.pos + 1 }} / {{ Math.max(1, chapters.length) }}
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
              @click="openChapterByPos(item.pos, { forceRefresh: true })"
            >
              <LuRefreshCw class="h-4 w-4" :class="loadingChapter ? 'animate-spin' : ''" />
            </button>
          </div>

          <article class="space-y-4 text-[var(--md-sys-color-on-surface)]">
            <p
              v-for="(para, idx) in item.paragraphs"
              :key="`tp-${item.pos}-${idx}`"
              class="indent-[2em] break-words"
            >
              {{ para }}
            </p>
          </article>

          <p
            v-if="item.paragraphs.length === 0"
            class="py-10 text-center text-xs text-[var(--md-sys-color-on-surface-variant)]"
          >
            本章正文为空，可点击右上角重新抓取本章。
          </p>
        </section>

        <!-- Continuous Scroll Footer Status -->
        <div class="mt-10 flex flex-col items-center gap-3 text-xs text-[var(--md-sys-color-on-surface-variant)]">
          <div v-if="appendingNext" class="flex items-center gap-2 py-2">
            <LuLoader2 class="h-4 w-4 animate-spin text-[var(--md-sys-color-primary)]" />
            <span>正在自动加载下一章…</span>
          </div>
          <div
            v-else-if="appendError"
            class="flex flex-wrap items-center justify-center gap-2 rounded-2xl bg-[var(--md-sys-color-error-container)] px-4 py-2.5 text-[var(--md-sys-color-on-error-container)]"
          >
            <span>自动加载下一章失败：{{ appendError }}</span>
            <button
              type="button"
              class="md-btn md-btn-sm md-btn-filled md-state"
              @click="appendNextChapter()"
            >
              重试
            </button>
          </div>
          <span v-else-if="nextAppendablePos === null && loadedChapters.length > 0" class="py-1 opacity-75">
            已读至最后一章
          </span>
        </div>
      </div>

      <!-- Bottom Chapter Navigation -->
      <div
        v-if="!loadingChapter && chapters.length > 0"
        class="mt-6 flex items-center justify-between gap-3 border-t border-[var(--md-sys-color-outline-variant)]/45 pt-6"
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
          :disabled="nextAppendablePos === null || appendingNext"
          class="md-btn md-btn-filled md-state"
          @click="nextAppendablePos !== null && openChapterByPos(nextAppendablePos)"
        >
          <span>下一章</span>
          <LuChevronRight class="h-4 w-4" />
        </button>
      </div>
    </div>
  </div>
</template>
