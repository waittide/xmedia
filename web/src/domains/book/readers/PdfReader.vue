<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from 'vue'
import { useBookI18n as useI18n } from '@/domains/book/i18n'
import * as pdfjsLib from 'pdfjs-dist'
import pdfWorkerUrl from 'pdfjs-dist/build/pdf.worker.min.mjs?url'
import {
  LuChevronLeft,
  LuChevronRight,
  LuMaximize2,
  LuMoveHorizontal,
  LuZoomIn,
  LuZoomOut,
} from 'vue-icons-plus/lu'
import { api } from '@/domains/book/api'
import type { ReadingLocation } from '@/domains/book/types'

pdfjsLib.GlobalWorkerOptions.workerSrc = pdfWorkerUrl

type FitMode = 'manual' | 'width' | 'page'

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

const scrollAreaEl = ref<HTMLElement | null>(null)
const canvasEl = ref<HTMLCanvasElement | null>(null)
const loading = ref(true)
const errorMsg = ref('')
const currentPage = ref(1)
const totalPages = ref(1)
const scale = ref(1.35)
const fitMode = ref<FitMode>('manual')
const pageInput = ref(1)

let loadingTask: pdfjsLib.PDFDocumentLoadingTask | null = null
let pdfDoc: pdfjsLib.PDFDocumentProxy | null = null
let activeRenderTask: pdfjsLib.RenderTask | null = null

function clampScale(value: number): number {
  return Math.min(3.0, Math.max(0.5, Number(value.toFixed(2))))
}

async function computeFitScale(page: pdfjsLib.PDFPageProxy, mode: 'width' | 'page'): Promise<number> {
  const container = scrollAreaEl.value
  if (!container) return scale.value
  const baseViewport = page.getViewport({ scale: 1 })
  const availWidth = Math.max(240, container.clientWidth - 48)
  const availHeight = Math.max(240, container.clientHeight - 48)
  const widthScale = availWidth / baseViewport.width
  if (mode === 'width') {
    return clampScale(widthScale)
  }
  const heightScale = availHeight / baseViewport.height
  return clampScale(Math.min(widthScale, heightScale))
}

async function renderPage(pageNum: number): Promise<void> {
  if (!pdfDoc || !canvasEl.value) return
  const clamped = Math.min(totalPages.value, Math.max(1, pageNum))
  currentPage.value = clamped
  pageInput.value = clamped

  if (activeRenderTask) {
    activeRenderTask.cancel()
    activeRenderTask = null
  }

  const page = await pdfDoc.getPage(clamped)
  if (fitMode.value === 'width' || fitMode.value === 'page') {
    scale.value = await computeFitScale(page, fitMode.value)
  }

  const dpr = window.devicePixelRatio || 1
  const viewport = page.getViewport({ scale: scale.value })

  const canvas = canvasEl.value
  const ctx = canvas.getContext('2d')
  if (!ctx) return

  canvas.width = Math.floor(viewport.width * dpr)
  canvas.height = Math.floor(viewport.height * dpr)
  canvas.style.width = `${Math.floor(viewport.width)}px`
  canvas.style.height = `${Math.floor(viewport.height)}px`

  ctx.setTransform(dpr, 0, 0, dpr, 0, 0)

  const task = page.render({
    canvasContext: ctx,
    canvas,
    viewport,
  })
  activeRenderTask = task
  try {
    await task.promise
  } catch {
    return
  }

  const pct = (clamped / Math.max(1, totalPages.value)) * 100
  emit('progress', {
    progress_percent: Number(pct.toFixed(2)),
    locator: `page:${clamped}`,
    page_number: clamped,
  })
}

async function navigateTo(target: string): Promise<void> {
  const match = /^page:(\d+)$/.exec(target)
  const page = match ? Number.parseInt(match[1] ?? '1', 10) : 1
  await renderPage(page)
}

async function changeScale(delta: number): Promise<void> {
  fitMode.value = 'manual'
  scale.value = clampScale(scale.value + delta)
  await renderPage(currentPage.value)
}

async function applyFitMode(mode: 'width' | 'page'): Promise<void> {
  fitMode.value = mode
  await renderPage(currentPage.value)
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
  if (ev.key === 'ArrowLeft') {
    ev.preventDefault()
    void renderPage(currentPage.value - 1)
  } else if (ev.key === 'ArrowRight') {
    ev.preventDefault()
    void renderPage(currentPage.value + 1)
  }
}

let resizeTimer: ReturnType<typeof setTimeout> | null = null
function handleResize(): void {
  if (fitMode.value === 'manual' || !pdfDoc) return
  if (resizeTimer) clearTimeout(resizeTimer)
  resizeTimer = setTimeout(() => {
    resizeTimer = null
    void renderPage(currentPage.value)
  }, 160)
}

defineExpose({ navigateTo })

onMounted(async () => {
  window.addEventListener('keydown', handleKeydown)
  window.addEventListener('resize', handleResize)
  try {
    loadingTask = pdfjsLib.getDocument({
      url: api.fileUrl(props.bookId),
      withCredentials: true,
    })

    pdfDoc = await loadingTask.promise
    totalPages.value = pdfDoc.numPages
    loading.value = false

    const startPage = props.initialLocation?.page_number ?? 1
    await renderPage(startPage)
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
    loading.value = false
  }
})

onBeforeUnmount(() => {
  window.removeEventListener('keydown', handleKeydown)
  window.removeEventListener('resize', handleResize)
  if (resizeTimer) clearTimeout(resizeTimer)
  activeRenderTask?.cancel()
  void pdfDoc?.cleanup()
  void loadingTask?.destroy()
})
</script>

<template>
  <div class="flex h-full w-full flex-col overflow-hidden bg-[var(--md-sys-color-surface-container)]">
    <!-- PDF Sub-Toolbar -->
    <div class="flex min-h-11 shrink-0 flex-wrap items-center justify-center gap-2 border-b border-[var(--md-sys-color-outline-variant)]/55 bg-[var(--md-sys-color-surface)] px-4 py-1.5 text-xs">
      <button
        type="button"
        :disabled="currentPage <= 1"
        class="md-icon-btn md-icon-btn-sm md-state disabled:opacity-35"
        :title="t('reader.prevPage')"
        @click="renderPage(currentPage - 1)"
      >
        <LuChevronLeft class="h-4 w-4" />
      </button>

      <form class="flex items-center gap-1.5" @submit.prevent="renderPage(pageInput)">
        <input
          v-model.number="pageInput"
          type="number"
          min="1"
          :max="totalPages"
          class="md-input h-8 w-16 !px-2 text-center text-xs"
        />
        <span class="text-[var(--md-sys-color-on-surface-variant)]">/ {{ totalPages }}</span>
      </form>

      <button
        type="button"
        :disabled="currentPage >= totalPages"
        class="md-icon-btn md-icon-btn-sm md-state disabled:opacity-35"
        :title="t('reader.nextPage')"
        @click="renderPage(currentPage + 1)"
      >
        <LuChevronRight class="h-4 w-4" />
      </button>

      <div class="mx-1 h-4 w-px bg-[var(--md-sys-color-outline-variant)]" />

      <button
        type="button"
        class="md-icon-btn md-icon-btn-sm md-state"
        :title="t('reader.zoomOut')"
        @click="changeScale(-0.2)"
      >
        <LuZoomOut class="h-4 w-4" />
      </button>
      <span class="w-12 text-center font-mono text-[var(--md-sys-color-on-surface)]">
        {{ Math.round(scale * 100) }}%
      </span>
      <button
        type="button"
        class="md-icon-btn md-icon-btn-sm md-state"
        :title="t('reader.zoomIn')"
        @click="changeScale(0.2)"
      >
        <LuZoomIn class="h-4 w-4" />
      </button>

      <div class="mx-1 h-4 w-px bg-[var(--md-sys-color-outline-variant)]" />

      <button
        type="button"
        class="md-chip md-state !h-7 !rounded-full !px-2.5 !text-[11px]"
        :class="fitMode === 'width' ? 'md-chip-selected' : ''"
        :title="t('reader.fitWidth')"
        @click="applyFitMode('width')"
      >
        <LuMoveHorizontal class="h-3.5 w-3.5" />
        <span>{{ t('reader.fitWidth') }}</span>
      </button>

      <button
        type="button"
        class="md-chip md-state !h-7 !rounded-full !px-2.5 !text-[11px]"
        :class="fitMode === 'page' ? 'md-chip-selected' : ''"
        :title="t('reader.fitPage')"
        @click="applyFitMode('page')"
      >
        <LuMaximize2 class="h-3.5 w-3.5" />
        <span>{{ t('reader.fitPage') }}</span>
      </button>
    </div>

    <!-- Canvas Scroll Area -->
    <div ref="scrollAreaEl" class="flex flex-1 justify-center overflow-auto p-6">
      <div v-if="loading" class="self-center text-sm text-[var(--md-sys-color-on-surface-variant)]">
        {{ t('reader.loadingBook') }}
      </div>
      <div v-else-if="errorMsg" class="self-center text-sm text-[var(--md-sys-color-error)]">
        {{ errorMsg }}
      </div>
      <canvas
        v-show="!loading && !errorMsg"
        ref="canvasEl"
        class="rounded-xl border border-[var(--md-sys-color-outline-variant)]/45 bg-[var(--md-sys-color-surface-container-lowest)] shadow-[var(--md-sys-elevation-2)]"
      />
    </div>
  </div>
</template>
