<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { LuChevronLeft, LuChevronRight } from 'vue-icons-plus/lu'
import { api } from '@/api/client'
import { useThemeStore } from '@/stores/theme'
import type { ReadingLocation, TxtChapterContent } from '@/types/api'

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
const themeStore = useThemeStore()
const scrollContainer = ref<HTMLElement | null>(null)
const chapter = ref<TxtChapterContent | null>(null)
const loading = ref(true)
const errorMsg = ref('')

const paragraphs = computed(() => {
  if (!chapter.value) return []
  return chapter.value.content
    .split(/\r?\n/)
    .map((l) => l.trim())
    .filter((l) => l.length > 0)
})

async function loadChapter(index: number, restoreScrollRatio = 0): Promise<void> {
  loading.value = true
  errorMsg.value = ''
  try {
    chapter.value = await api.getTxtChapter(props.bookId, index)
    loading.value = false
    await nextTick()
    if (scrollContainer.value) {
      const maxScroll = scrollContainer.value.scrollHeight - scrollContainer.value.clientHeight
      scrollContainer.value.scrollTop = maxScroll > 0 ? maxScroll * restoreScrollRatio : 0
    }
    reportProgress(restoreScrollRatio)
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
    loading.value = false
  }
}

function reportProgress(explicitRatio?: number): void {
  if (!chapter.value || !scrollContainer.value) return
  const el = scrollContainer.value
  const maxScroll = el.scrollHeight - el.clientHeight
  const ratio =
    explicitRatio !== undefined
      ? explicitRatio
      : maxScroll > 0
        ? Math.min(1, Math.max(0, el.scrollTop / maxScroll))
        : 0
  const pct = ((chapter.value.index + ratio) / Math.max(1, chapter.value.total_chapters)) * 100
  emit('progress', {
    progress_percent: Math.min(100, Math.max(0, Number(pct.toFixed(2)))),
    locator: `chapter:${chapter.value.index}`,
    chapter_index: chapter.value.index,
    scroll_ratio: Number(ratio.toFixed(4)),
  })
}

async function navigateTo(target: string): Promise<void> {
  const m = /^chapter:(\d+)$/.exec(target)
  const idx = m ? Number.parseInt(m[1] ?? '0', 10) : 0
  await loadChapter(idx, 0)
}

function handleKeydown(ev: KeyboardEvent): void {
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
  await loadChapter(
    props.initialLocation?.chapter_index ?? 0,
    props.initialLocation?.scroll_ratio ?? 0,
  )
})

onBeforeUnmount(() => {
  window.removeEventListener('keydown', handleKeydown)
})
</script>

<template>
  <div
    ref="scrollContainer"
    class="h-full w-full overflow-y-auto bg-[var(--ctp-canvas)] px-6 py-8"
    @scroll.passive="reportProgress()"
  >
    <div v-if="loading" class="py-20 text-center text-sm text-[var(--ctp-subtext0)]">
      {{ t('reader.loadingBook') }}
    </div>
    <div v-else-if="errorMsg" class="py-20 text-center text-sm text-[var(--ctp-red)]">
      {{ errorMsg }}
    </div>
    <article
      v-else-if="chapter"
      class="mx-auto max-w-3xl"
      :style="{
        fontSize: `${themeStore.book.font_size}px`,
        lineHeight: String(themeStore.book.line_height),
      }"
    >
      <h2 class="mb-6 text-xl font-bold text-[var(--ctp-text)]">{{ chapter.title }}</h2>
      <p
        v-for="(para, idx) in paragraphs"
        :key="idx"
        class="mb-4 indent-8 text-[var(--ctp-text)]"
      >
        {{ para }}
      </p>

      <div class="mt-10 flex items-center justify-between border-t border-[var(--ctp-surface0)] pt-6 text-sm">
        <button
          type="button"
          class="flex items-center gap-1 rounded-lg border border-[var(--ctp-surface0)] px-4 py-2 disabled:opacity-40"
          :disabled="chapter.index <= 0"
          @click="loadChapter(chapter.index - 1, 0)"
        >
          <LuChevronLeft class="h-4 w-4" /> {{ t('reader.prevPage') }}
        </button>
        <span class="text-xs text-[var(--ctp-subtext0)]">
          {{ chapter.index + 1 }} / {{ chapter.total_chapters }}
        </span>
        <button
          type="button"
          class="flex items-center gap-1 rounded-lg border border-[var(--ctp-surface0)] px-4 py-2 disabled:opacity-40"
          :disabled="chapter.index + 1 >= chapter.total_chapters"
          @click="loadChapter(chapter.index + 1, 0)"
        >
          {{ t('reader.nextPage') }} <LuChevronRight class="h-4 w-4" />
        </button>
      </div>
    </article>
  </div>
</template>
