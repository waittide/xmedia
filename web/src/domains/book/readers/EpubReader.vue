<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useBookI18n as useI18n } from '@/domains/book/i18n'
import ePub, { type Book, type Rendition } from 'epubjs'
import { LuChevronLeft, LuChevronRight } from 'vue-icons-plus/lu'
import { api } from '@/domains/book/api'
import { useThemeStore } from '@/stores/theme'
import type { ReadingLocation } from '@/domains/book/types'

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

const mountEl = ref<HTMLElement | null>(null)
const loading = ref(true)
const errorMsg = ref('')

let book: Book | null = null
let rendition: Rendition | null = null
let locationsReady = false

function resolveFontStack(key: string): string {
  if (key === 'serif') return '"Noto Serif SC", "Source Han Serif SC", Georgia, serif'
  if (key === 'mono') return '"JetBrains Mono", ui-monospace, SFMono-Regular, monospace'
  return '"Inter", "Noto Sans SC", system-ui, sans-serif'
}

function applyRenditionTheme(): void {
  if (!rendition) return
  const palette = themeStore.activePalette
  const r = themeStore.reader
  rendition.themes.default({
    body: {
      background: `${palette.canvas} !important`,
      color: `${palette.text} !important`,
      'font-family': `${resolveFontStack(r.font_family)} !important`,
      'font-size': `${r.font_size}px !important`,
      'line-height': `${r.line_height} !important`,
      padding: `0 ${r.margin_px}px !important`,
    },
    a: {
      color: `${palette.accent} !important`,
    },
  })
}

async function navigateTo(hrefOrCfi: string): Promise<void> {
  if (!rendition) return
  const clean = hrefOrCfi.replace(/^OEBPS\//i, '')
  try {
    await rendition.display(hrefOrCfi)
  } catch {
    await rendition.display(clean)
  }
}

async function prevPage(): Promise<void> {
  await rendition?.prev()
}

async function nextPage(): Promise<void> {
  await rendition?.next()
}

function handleKeydown(ev: KeyboardEvent): void {
  if (ev.key === 'ArrowLeft') void prevPage()
  if (ev.key === 'ArrowRight') void nextPage()
}

defineExpose({ navigateTo })

onMounted(async () => {
  window.addEventListener('keydown', handleKeydown)
  try {
    const resp = await fetch(api.fileUrl(props.bookId), {
      credentials: 'include',
    })
    if (!resp.ok) {
      throw new Error(`HTTP ${resp.status}`)
    }
    const buffer = await resp.arrayBuffer()
    if (!mountEl.value) return

    book = ePub(buffer)
    await book.ready

    const flowMode = themeStore.reader.flow === 'scrolled' ? 'scrolled-doc' : 'paginated'
    rendition = book.renderTo(mountEl.value, {
      width: '100%',
      height: '100%',
      flow: flowMode,
      spread: 'none',
    })

    applyRenditionTheme()

    const startTarget = props.initialLocation?.locator || undefined
    await rendition.display(startTarget)
    loading.value = false

    void book.locations.generate(1200).then(() => {
      locationsReady = true
    })

    rendition.on('relocated', (loc) => {
      const cfi = loc.start.cfi
      let pct = loc.start.percentage * 100
      if (locationsReady && book) {
        pct = book.locations.percentageFromCfi(cfi) * 100
      }
      if (!Number.isFinite(pct)) pct = 0
      emit('progress', {
        progress_percent: Math.min(100, Math.max(0, Number(pct.toFixed(2)))),
        locator: cfi,
      })
    })
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
    loading.value = false
  }
})

watch(
  () => [
    themeStore.activePalette,
    themeStore.reader.font_family,
    themeStore.reader.font_size,
    themeStore.reader.line_height,
    themeStore.reader.margin_px,
  ],
  () => {
    applyRenditionTheme()
  },
)

watch(
  () => themeStore.reader.flow,
  (nextFlow) => {
    if (!rendition) return
    rendition.flow(nextFlow === 'scrolled' ? 'scrolled-doc' : 'paginated')
  },
)

onBeforeUnmount(() => {
  window.removeEventListener('keydown', handleKeydown)
  rendition?.destroy()
  book?.destroy()
})
</script>

<template>
  <div class="relative flex h-full w-full items-center justify-center overflow-hidden bg-[var(--ctp-canvas)]">
    <div v-if="loading" class="text-sm text-[var(--ctp-subtext0)]">
      {{ t('reader.loadingBook') }}
    </div>
    <div v-else-if="errorMsg" class="text-sm text-[var(--ctp-red)]">
      {{ errorMsg }}
    </div>

    <div ref="mountEl" class="h-full w-full max-w-5xl" />

    <!-- Paginated Prev / Next Floating Buttons -->
    <template v-if="themeStore.reader.flow === 'paginated' && !loading">
      <button
        type="button"
        class="absolute left-3 top-1/2 flex h-11 w-11 -translate-y-1/2 items-center justify-center rounded-full border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)]/85 text-[var(--ctp-subtext1)] shadow-md backdrop-blur-sm transition hover:bg-[var(--ctp-accent)] hover:text-[var(--ctp-on-accent)]"
        :title="t('reader.prevPage')"
        @click="prevPage"
      >
        <LuChevronLeft class="h-5 w-5" />
      </button>

      <button
        type="button"
        class="absolute right-3 top-1/2 flex h-11 w-11 -translate-y-1/2 items-center justify-center rounded-full border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)]/85 text-[var(--ctp-subtext1)] shadow-md backdrop-blur-sm transition hover:bg-[var(--ctp-accent)] hover:text-[var(--ctp-on-accent)]"
        :title="t('reader.nextPage')"
        @click="nextPage"
      >
        <LuChevronRight class="h-5 w-5" />
      </button>
    </template>
  </div>
</template>
