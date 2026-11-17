<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref } from 'vue'
import { useBookI18n as useI18n } from '@/domains/book/i18n'
import { LuChevronLeft, LuChevronRight } from 'vue-icons-plus/lu'
import { api } from '@/domains/book/api'
import { useThemeStore } from '@/stores/theme'
import type { ReadingLocation, TxtChapterContent } from '@/domains/book/types'

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

const paragraphs = computed(() => {
  if (!chapter.value) return []
  return chapter.value.content
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter((line) => line.length > 0)
})

const fontFamilyStyle = computed(() => {
  const key = themeStore.reader.font_family
  if (key === 'serif') return '"Noto Serif SC", "Source Han Serif SC", Georgia, serif'
  if (key === 'mono') return '"JetBrains Mono", ui-monospace, SFMono-Regular, monospace'
  return '"Inter", "Noto Sans SC", system-ui, sans-serif'
})

async function loadChapter(index: number, restoreScrollRatio = 0): Promise<void> {
  loading.value = true
  errorMsg.value = ''
  try {
    const data = await api.getTxtChapter(props.bookId, index)
    chapter.value = data
    loading.value = false
    await nextTick()
    if (scrollContainer.value) {
      const maxScroll =
        scrollContainer.value.scrollHeight - scrollContainer.value.clientHeight
      scrollContainer.value.scrollTop = maxScroll > 0 ? maxScroll * restoreScrollRatio : 0
    }
    reportCurrentProgress(restoreScrollRatio)
  } catch (err) {
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
  const tag = (ev.target as HTMLElement | null)?.tagName
  if (tag === 'INPUT' || tag === 'TEXTAREA') return
  if (!chapter.value || loading.value) return
  if (ev.key === 'ArrowLeft' && chapter.value.index > 0) {
    void loadChapter(chapter.value.index - 1, 0)
  } else if (ev.key === 'ArrowRight' && chapter.value.index + 1 < chapter.value.total_chapters) {
    void loadChapter(chapter.value.index + 1, 0)
  }
}

defineExpose({ navigateTo })

onMounted(async () => {
  window.addEventListener('keydown', handleKeydown)
  const initIdx = props.initialLocation?.chapter_index ?? 0
  const initRatio = props.initialLocation?.scroll_ratio ?? 0
  await loadChapter(initIdx, initRatio)
})

onBeforeUnmount(() => {
  window.removeEventListener('keydown', handleKeydown)
})
</script>

<template>
  <div
    ref="scrollContainer"
    class="h-full w-full overflow-y-auto bg-[var(--ctp-canvas)] transition-colors"
    @scroll.passive="reportCurrentProgress()"
  >
    <div
      v-if="loading"
      class="flex h-full items-center justify-center text-sm text-[var(--ctp-subtext0)]"
    >
      {{ t('reader.loadingBook') }}
    </div>

    <div
      v-else-if="errorMsg"
      class="flex h-full items-center justify-center text-sm text-[var(--ctp-red)]"
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
      <header class="mb-8 border-b border-[var(--ctp-surface0)] pb-4">
        <div class="flex flex-wrap items-center gap-2 text-xs font-semibold text-[var(--ctp-accent)]">
          <span
            v-if="chapter.volume_title"
            class="rounded-md bg-[var(--ctp-accent-soft)] px-2 py-0.5"
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
        <h1 class="mt-2 text-2xl font-bold text-[var(--ctp-text)]">{{ chapter.title }}</h1>
      </header>

      <div class="space-y-4 text-[var(--ctp-text)]">
        <p v-for="(para, i) in paragraphs" :key="i" class="indent-8 leading-relaxed">
          {{ para }}
        </p>
      </div>

      <footer class="mt-12 flex items-center justify-between border-t border-[var(--ctp-surface0)] pt-6 text-sm">
        <button
          type="button"
          :disabled="chapter.index <= 0"
          class="flex items-center gap-1.5 rounded-full border border-[var(--ctp-surface1)] px-5 py-2 text-xs font-semibold text-[var(--ctp-text)] transition hover:bg-[var(--ctp-surface0)] disabled:opacity-35"
          @click="loadChapter(chapter.index - 1, 0)"
        >
          <LuChevronLeft class="h-4 w-4" />
          <span>{{ t('reader.prevChapter') }}</span>
        </button>

        <span class="text-xs text-[var(--ctp-subtext0)]">
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
          class="flex items-center gap-1.5 rounded-full bg-[var(--ctp-accent)] px-5 py-2 text-xs font-semibold text-[var(--ctp-on-accent)] shadow-sm transition hover:opacity-95 disabled:opacity-35"
          @click="loadChapter(chapter.index + 1, 0)"
        >
          <span>{{ t('reader.nextChapter') }}</span>
          <LuChevronRight class="h-4 w-4" />
        </button>
      </footer>
    </article>
  </div>
</template>
