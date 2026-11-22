<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from 'vue'
import * as pdfjsLib from 'pdfjs-dist'
import pdfWorkerUrl from 'pdfjs-dist/build/pdf.worker.min.mjs?url'
import { MediaErrorState } from '@/components/domain'
import { MdCircularProgress } from '@/components/ui'
import { api } from '@/domains/book/api'
import { useBookI18n as useI18n } from '@/domains/book/i18n'
import type { ReadingLocation } from '@/domains/book/types'

pdfjsLib.GlobalWorkerOptions.workerSrc = pdfWorkerUrl

export type PdfFitMode = 'manual' | 'width' | 'page'

export interface PdfReaderState {
  currentPage: number
  totalPages: number
  scale: number
  fitMode: PdfFitMode
}

const SWIPE_THRESHOLD_PX = 48

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
  'state-change': [state: PdfReaderState]
}>()

const { t } = useI18n()

const scrollAreaEl = ref<HTMLElement | null>(null)
const canvasEl = ref<HTMLCanvasElement | null>(null)
const loading = ref(true)
const errorMsg = ref('')
const currentPage = ref(1)
const totalPages = ref(1)
const scale = ref(1.0)
// Default to fit-width so mobile viewports never horizontally overflow on initial open
const fitMode = ref<PdfFitMode>('width')

let loadingTask: pdfjsLib.PDFDocumentLoadingTask | null = null
let pdfDoc: pdfjsLib.PDFDocumentProxy | null = null
let activeRenderTask: pdfjsLib.RenderTask | null = null
let renderSeq = 0
let unmounted = false
let resizeTimer: ReturnType<typeof setTimeout> | null = null
let touchStartX = 0
let touchStartY = 0

function emitState(): void {
  emit('state-change', {
    currentPage: currentPage.value,
    totalPages: totalPages.value,
    scale: scale.value,
    fitMode: fitMode.value,
  })
}

function clampScale(value: number): number {
  return Math.min(3.0, Math.max(0.5, Number(value.toFixed(2))))
}

function computeFitScale(page: pdfjsLib.PDFPageProxy, mode: 'width' | 'page'): number {
  const container = scrollAreaEl.value
  if (!container) return scale.value
  const baseViewport = page.getViewport({ scale: 1 })
  const padX = container.clientWidth < 600 ? 24 : 48
  const padY = 48
  const availWidth = Math.max(220, container.clientWidth - padX)
  const availHeight = Math.max(220, container.clientHeight - padY)
  const widthScale = availWidth / baseViewport.width
  if (mode === 'width') {
    return clampScale(widthScale)
  }
  const heightScale = availHeight / baseViewport.height
  return clampScale(Math.min(widthScale, heightScale))
}

async function renderPage(pageNum: number): Promise<void> {
  if (!pdfDoc || !canvasEl.value || unmounted) return
  if (!Number.isFinite(pageNum)) return
  const prevPage = currentPage.value
  const clamped = Math.min(totalPages.value, Math.max(1, Math.trunc(pageNum)))

  const seq = ++renderSeq

  if (activeRenderTask) {
    const prevTask = activeRenderTask
    activeRenderTask = null
    prevTask.cancel()
    try {
      await prevTask.promise
    } catch {
      // expected RenderingCancelledException
    }
  }
  if (seq !== renderSeq || unmounted || !pdfDoc || !canvasEl.value) return

  let page: pdfjsLib.PDFPageProxy | null = null
  try {
    page = await pdfDoc.getPage(clamped)
    if (seq !== renderSeq || unmounted || !canvasEl.value) return

    if (fitMode.value === 'width' || fitMode.value === 'page') {
      scale.value = computeFitScale(page, fitMode.value)
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
    } finally {
      if (activeRenderTask === task) {
        activeRenderTask = null
      }
    }

    if (seq !== renderSeq || unmounted) return
    errorMsg.value = ''
    currentPage.value = clamped
    emitState()

    const pct = (clamped / Math.max(1, totalPages.value)) * 100
    emit('progress', {
      progress_percent: Number(pct.toFixed(2)),
      locator: `page:${clamped}`,
      page_number: clamped,
    })
  } catch (err) {
    if (seq !== renderSeq || unmounted) return
    const name = err instanceof Error ? err.name : ''
    if (name === 'RenderingCancelledException') return
    currentPage.value = prevPage
    errorMsg.value = err instanceof Error ? err.message : String(err)
    emitState()
  } finally {
    try {
      page?.cleanup()
    } catch {
      // ignore page cleanup failure
    }
  }
}

async function navigateTo(target: string): Promise<void> {
  const match = /^page:(\d+)$/.exec(target)
  const page = match ? Number.parseInt(match[1] ?? '1', 10) : 1
  await renderPage(page)
}

async function goToPage(page: number): Promise<void> {
  await renderPage(page)
}

async function prevPage(): Promise<void> {
  if (currentPage.value > 1) {
    await renderPage(currentPage.value - 1)
  }
}

async function nextPage(): Promise<void> {
  if (currentPage.value < totalPages.value) {
    await renderPage(currentPage.value + 1)
  }
}

async function changeScale(delta: number): Promise<void> {
  fitMode.value = 'manual'
  scale.value = clampScale(scale.value + delta)
  await renderPage(currentPage.value)
}

async function zoomIn(): Promise<void> {
  await changeScale(0.2)
}

async function zoomOut(): Promise<void> {
  await changeScale(-0.2)
}

async function applyFitMode(mode: 'width' | 'page'): Promise<void> {
  fitMode.value = mode
  await renderPage(currentPage.value)
}

function handleTouchStart(ev: TouchEvent): void {
  const touch = ev.changedTouches[0]
  if (!touch) return
  touchStartX = touch.screenX
  touchStartY = touch.screenY
}

function handleTouchEnd(ev: TouchEvent): void {
  const touch = ev.changedTouches[0]
  if (!touch) return
  const dx = touch.screenX - touchStartX
  const dy = touch.screenY - touchStartY
  if (Math.abs(dx) >= SWIPE_THRESHOLD_PX && Math.abs(dx) > Math.abs(dy) * 1.4) {
    if (dx > 0) {
      void prevPage()
    } else {
      void nextPage()
    }
  }
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
    void prevPage()
  } else if (ev.key === 'ArrowRight') {
    ev.preventDefault()
    void nextPage()
  }
}

function handleResize(): void {
  if (fitMode.value === 'manual' || !pdfDoc) return
  if (resizeTimer) clearTimeout(resizeTimer)
  resizeTimer = setTimeout(() => {
    resizeTimer = null
    void renderPage(currentPage.value)
  }, 160)
}

async function loadPdf(): Promise<void> {
  loading.value = true
  errorMsg.value = ''
  try {
    const task = pdfjsLib.getDocument({
      url: api.fileUrl(props.bookId),
      withCredentials: true,
    })
    loadingTask = task

    const doc = await task.promise
    if (unmounted) {
      void doc.cleanup()
      void task.destroy()
      return
    }
    pdfDoc = doc
    totalPages.value = doc.numPages
    loading.value = false
    emitState()

    const startPage = props.initialLocation?.page_number ?? 1
    await renderPage(startPage)
  } catch (err) {
    if (unmounted) return
    errorMsg.value = err instanceof Error ? err.message : String(err)
    loading.value = false
  }
}

defineExpose({
  navigateTo,
  goToPage,
  prevPage,
  nextPage,
  zoomIn,
  zoomOut,
  applyFitMode,
})

onMounted(() => {
  unmounted = false
  window.addEventListener('keydown', handleKeydown)
  window.addEventListener('resize', handleResize)
  void loadPdf()
})

onBeforeUnmount(() => {
  unmounted = true
  renderSeq += 1
  window.removeEventListener('keydown', handleKeydown)
  window.removeEventListener('resize', handleResize)
  if (resizeTimer) clearTimeout(resizeTimer)
  activeRenderTask?.cancel()
  activeRenderTask = null
  if (canvasEl.value) {
    canvasEl.value.width = 0
    canvasEl.value.height = 0
  }
  void pdfDoc?.cleanup()
  pdfDoc = null
  void loadingTask?.destroy()
  loadingTask = null
})
</script>

<template>
  <div
    ref="scrollAreaEl"
    class="flex h-full w-full justify-center overflow-auto bg-surface-container px-3 pt-16 pb-20 sm:px-6"
    @touchstart.passive="handleTouchStart"
    @touchend.passive="handleTouchEnd"
  >
    <div
      v-if="loading"
      class="flex flex-col items-center justify-center gap-3 self-center text-on-surface-variant"
    >
      <MdCircularProgress indeterminate :size="32" :stroke-width="3" />
      <span class="text-body-small">{{ t('reader.loadingBook') }}</span>
    </div>

    <MediaErrorState
      v-else-if="errorMsg"
      :title="t('reader.loadFailedTitle')"
      :message="errorMsg"
      :retry-label="t('reader.retry')"
      class="max-w-md self-center"
      @retry="loadPdf"
    />

    <canvas
      v-show="!loading && !errorMsg"
      ref="canvasEl"
      class="self-start rounded-corner-md border border-outline-variant/50 bg-surface-container-lowest shadow-elevation-2"
    />
  </div>
</template>
