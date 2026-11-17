<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import * as pdfjsLib from 'pdfjs-dist'
import pdfWorkerUrl from 'pdfjs-dist/build/pdf.worker.min.mjs?url'
import { LuChevronLeft, LuChevronRight, LuZoomIn, LuZoomOut } from 'vue-icons-plus/lu'
import { api } from '@/api/client'
import type { ReadingLocation } from '@/types/api'

pdfjsLib.GlobalWorkerOptions.workerSrc = pdfWorkerUrl

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
const canvasEl = ref<HTMLCanvasElement | null>(null)
const loading = ref(true)
const errorMsg = ref('')
const currentPage = ref(1)
const totalPages = ref(1)
const scale = ref(1.3)

let loadingTask: pdfjsLib.PDFDocumentLoadingTask | null = null
let pdfDoc: pdfjsLib.PDFDocumentProxy | null = null
let activeRenderTask: pdfjsLib.RenderTask | null = null

async function renderPage(pageNum: number): Promise<void> {
  if (!pdfDoc || !canvasEl.value) return
  const clamped = Math.min(totalPages.value, Math.max(1, pageNum))
  currentPage.value = clamped

  if (activeRenderTask) {
    activeRenderTask.cancel()
    activeRenderTask = null
  }

  const page = await pdfDoc.getPage(clamped)
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

  const task = page.render({ canvasContext: ctx, canvas, viewport })
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
  const m = /^page:(\d+)$/.exec(target)
  const page = m ? Number.parseInt(m[1] ?? '1', 10) : 1
  await renderPage(page)
}

function adjustZoom(delta: number): void {
  scale.value = Math.min(3.0, Math.max(0.6, Number((scale.value + delta).toFixed(2))))
  void renderPage(currentPage.value)
}

function handleKeydown(ev: KeyboardEvent): void {
  if (ev.key === 'ArrowLeft') void renderPage(currentPage.value - 1)
  if (ev.key === 'ArrowRight') void renderPage(currentPage.value + 1)
}

defineExpose({ navigateTo })

onMounted(async () => {
  window.addEventListener('keydown', handleKeydown)
  try {
    loadingTask = pdfjsLib.getDocument({
      url: api.bookFileUrl(props.bookId),
      withCredentials: true,
    })
    pdfDoc = await loadingTask.promise
    totalPages.value = pdfDoc.numPages || 1
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
  activeRenderTask?.cancel()
  void loadingTask?.destroy()
})
</script>

<template>
  <div class="flex h-full w-full flex-col overflow-hidden bg-[var(--ctp-canvas)]">
    <div class="flex items-center justify-center gap-3 border-b border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)] py-2 text-xs">
      <button
        type="button"
        class="rounded px-2 py-1 hover:bg-[var(--ctp-surface0)]"
        :disabled="currentPage <= 1"
        @click="renderPage(currentPage - 1)"
      >
        <LuChevronLeft class="h-4 w-4" />
      </button>
      <span class="tabular-nums">{{ currentPage }} / {{ totalPages }}</span>
      <button
        type="button"
        class="rounded px-2 py-1 hover:bg-[var(--ctp-surface0)]"
        :disabled="currentPage >= totalPages"
        @click="renderPage(currentPage + 1)"
      >
        <LuChevronRight class="h-4 w-4" />
      </button>
      <span class="mx-2 text-[var(--ctp-surface1)]">|</span>
      <button type="button" class="rounded p-1 hover:bg-[var(--ctp-surface0)]" @click="adjustZoom(-0.15)">
        <LuZoomOut class="h-4 w-4" />
      </button>
      <span class="tabular-nums">{{ Math.round(scale * 100) }}%</span>
      <button type="button" class="rounded p-1 hover:bg-[var(--ctp-surface0)]" @click="adjustZoom(0.15)">
        <LuZoomIn class="h-4 w-4" />
      </button>
    </div>

    <div class="flex-1 overflow-auto flex justify-center p-4">
      <div v-if="loading" class="py-20 text-sm text-[var(--ctp-subtext0)]">{{ t('reader.loadingBook') }}</div>
      <div v-else-if="errorMsg" class="py-20 text-sm text-[var(--ctp-red)]">{{ errorMsg }}</div>
      <canvas v-show="!loading && !errorMsg" ref="canvasEl" class="shadow-lg" />
    </div>
  </div>
</template>
