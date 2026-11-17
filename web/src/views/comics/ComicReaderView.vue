<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { LuArrowLeft, LuChevronLeft, LuChevronRight } from 'vue-icons-plus/lu'
import { api } from '@/api/client'
import type { ComicFitMode, ComicReaderContextDto, ComicReaderMode } from '@/types/api'

const route = useRoute()
const router = useRouter()

const ctx = ref<ComicReaderContextDto | null>(null)
const currentPage = ref(1)
const readerMode = ref<ComicReaderMode>('rtl_double')
const fitMode = ref<ComicFitMode>('fit_height')
const offsetCover = ref(true)

const totalPages = computed(() => ctx.value?.chapter.page_count ?? 1)

const spreads = computed<number[][]>(() => {
  const total = totalPages.value
  if (readerMode.value === 'ltr_single' || readerMode.value === 'webtoon') {
    return Array.from({ length: total }, (_, i) => [i + 1])
  }
  const out: number[][] = []
  let p = 1
  while (p <= total) {
    if (p === 1 && offsetCover.value) {
      out.push([1])
      p += 1
    } else if (p + 1 <= total) {
      out.push([p, p + 1])
      p += 2
    } else {
      out.push([p])
      p += 1
    }
  }
  return out
})

const currentSpread = computed(() => {
  const found = spreads.value.find((sp) => sp.includes(currentPage.value))
  return found ?? [1]
})

const displaySpread = computed(() =>
  readerMode.value === 'rtl_double' ? [...currentSpread.value].reverse() : currentSpread.value,
)

async function loadChapter(chapterId: string): Promise<void> {
  const data = await api.getComicReaderContext(chapterId)
  ctx.value = data
  readerMode.value = data.effective_pref.reader_mode
  fitMode.value = data.effective_pref.fit_mode
  offsetCover.value = data.effective_pref.offset_cover_page
  currentPage.value = Math.max(1, Math.min(data.chapter.page_count, data.chapter.current_page || 1))
}

function syncProgress(): void {
  if (!ctx.value) return
  const lastInSpread = Math.max(...currentSpread.value)
  const completed = lastInSpread >= totalPages.value ? true : undefined
  void api.updateComicProgress(ctx.value.chapter.id, lastInSpread, completed).catch(() => {})
}

function stepSpread(delta: number): void {
  if (!ctx.value) return
  const idx = spreads.value.findIndex((sp) => sp.includes(currentPage.value))
  const nextIdx = idx + delta
  if (nextIdx >= 0 && nextIdx < spreads.value.length) {
    currentPage.value = spreads.value[nextIdx]![0]!
    syncProgress()
  } else if (delta > 0 && ctx.value.next_chapter) {
    void router.push(`/comics/read/${ctx.value.next_chapter.id}`)
  } else if (delta < 0 && ctx.value.prev_chapter) {
    void router.push(`/comics/read/${ctx.value.prev_chapter.id}`)
  }
}

function onKeydown(ev: KeyboardEvent): void {
  if (readerMode.value === 'webtoon') return
  const rtl = readerMode.value === 'rtl_double'
  if (ev.key === 'ArrowLeft') stepSpread(rtl ? 1 : -1)
  if (ev.key === 'ArrowRight') stepSpread(rtl ? -1 : 1)
}

onMounted(() => {
  window.addEventListener('keydown', onKeydown)
  void loadChapter(String(route.params.chapterId))
})

watch(
  () => route.params.chapterId,
  (nextId) => {
    if (nextId) void loadChapter(String(nextId))
  },
)

onBeforeUnmount(() => {
  window.removeEventListener('keydown', onKeydown)
})
</script>

<template>
  <div v-if="ctx" class="flex h-full w-full flex-col overflow-hidden bg-black text-white">
    <!-- Top HUD -->
    <div class="flex h-12 shrink-0 items-center justify-between bg-zinc-900/90 px-4 text-xs border-b border-white/10">
      <div class="flex items-center gap-3 min-w-0">
        <button
          type="button"
          class="inline-flex items-center gap-1 rounded px-2 py-1 hover:bg-white/10"
          @click="router.push(`/comics/series/${ctx.series_id}`)"
        >
          <LuArrowLeft class="h-4 w-4" /> {{ ctx.series_title }}
        </button>
        <span class="text-zinc-400">Ch.{{ ctx.chapter.number_label }}</span>
      </div>

      <div class="flex items-center gap-2">
        <select v-model="readerMode" class="rounded bg-zinc-800 px-2 py-1 text-xs">
          <option value="rtl_double">RTL Manga Spread</option>
          <option value="ltr_single">LTR Single</option>
          <option value="ltr_double">LTR Spread</option>
          <option value="webtoon">Webtoon Scroll</option>
        </select>
        <select v-model="fitMode" class="rounded bg-zinc-800 px-2 py-1 text-xs">
          <option value="fit_height">Fit Height</option>
          <option value="fit_width">Fit Width</option>
          <option value="original">Original</option>
        </select>
        <span class="tabular-nums text-zinc-300">P{{ currentPage }} / {{ totalPages }}</span>
      </div>
    </div>

    <!-- Webtoon Mode -->
    <div
      v-if="readerMode === 'webtoon'"
      class="flex-1 overflow-y-auto flex flex-col items-center"
    >
      <img
        v-for="p in totalPages"
        :key="p"
        :src="api.comicPageUrl(ctx.chapter.id, p)"
        loading="lazy"
        alt=""
        class="w-full max-w-4xl select-none"
        @load="p === totalPages && api.updateComicProgress(ctx.chapter.id, totalPages, true)"
      />
    </div>

    <!-- Paginated / Spread Mode -->
    <div v-else class="relative flex flex-1 items-center justify-center overflow-hidden">
      <div class="flex items-center justify-center h-full w-full">
        <img
          v-for="p in displaySpread"
          :key="p"
          :src="api.comicPageUrl(ctx.chapter.id, p)"
          alt=""
          class="select-none object-contain"
          :class="fitMode === 'fit_height' ? 'max-h-full' : fitMode === 'fit_width' ? 'w-full' : ''"
        />
      </div>

      <button
        type="button"
        class="absolute left-3 top-1/2 -translate-y-1/2 rounded-full bg-zinc-900/80 p-2.5 hover:bg-zinc-700"
        @click="stepSpread(readerMode === 'rtl_double' ? 1 : -1)"
      >
        <LuChevronLeft class="h-5 w-5" />
      </button>
      <button
        type="button"
        class="absolute right-3 top-1/2 -translate-y-1/2 rounded-full bg-zinc-900/80 p-2.5 hover:bg-zinc-700"
        @click="stepSpread(readerMode === 'rtl_double' ? -1 : 1)"
      >
        <LuChevronRight class="h-5 w-5" />
      </button>
    </div>
  </div>
</template>
