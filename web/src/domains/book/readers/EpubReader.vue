<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, watch } from 'vue'
import ePub, { type Book, type Rendition } from 'epubjs'
import { LuChevronLeft, LuChevronRight } from 'vue-icons-plus/lu'
import { MediaErrorState } from '@/components/domain'
import { MdCircularProgress, MdIconButton } from '@/components/ui'
import { api } from '@/domains/book/api'
import { useBookI18n as useI18n } from '@/domains/book/i18n'
import type { ReadingLocation } from '@/domains/book/types'
import { usePreferencesStore } from '@/stores/preferences'
import { useThemeStore } from '@/stores/theme'
import { resolveFontStack } from '@/utils/format'

const LOCATIONS_CACHE_PREFIX = 'xmedia.epub.locations:'
const LOCATIONS_INDEX_KEY = 'xmedia.epub.locations.__index'
const MAX_CACHED_BOOKS = 12
const MAX_LOCATIONS_GENERATE_BYTES = 2 * 1024 * 1024
const SWIPE_THRESHOLD_PX = 48

const props = withDefaults(
  defineProps<{
    bookId: string
    initialLocation: ReadingLocation | null
    hudVisible?: boolean
  }>(),
  {
    hudVisible: true,
  },
)

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
  'toggle-hud': []
}>()

const { t } = useI18n()
const preferencesStore = usePreferencesStore()
const themeStore = useThemeStore()

const mountEl = ref<HTMLElement | null>(null)
const loading = ref(true)
const errorMsg = ref('')

let book: Book | null = null
let rendition: Rendition | null = null
let locationsReady = false
let lastKnownCfi = ''
let unmounted = false
let renderSeq = 0
let fetchAbortController: AbortController | null = null
let touchStartX = 0
let touchStartY = 0

interface EpubLocationsWithCache {
  generate(chars?: number): Promise<string[]>
  percentageFromCfi(cfi: string): number
  load?: (locations: string) => void
  save?: () => string
}

interface EpubBookWithSpine extends Book {
  spine?: {
    spineItems?: unknown[]
    length?: number
  }
}

interface EpubRenditionWithOff extends Rendition {
  off?: (event: string, handler: (...args: unknown[]) => void) => void
  currentLocation?: () => EpubRelocatedLocation | undefined
}

function buildLocationsCacheKey(bookId: string, byteLength: number): string {
  return `${LOCATIONS_CACHE_PREFIX}${bookId}:${byteLength}`
}

function readCachedLocations(key: string): string | null {
  try {
    return localStorage.getItem(key)
  } catch {
    return null
  }
}

function writeCachedLocations(key: string, serialized: string): void {
  try {
    let index: string[] = []
    try {
      const raw = localStorage.getItem(LOCATIONS_INDEX_KEY)
      if (raw) {
        const parsed = JSON.parse(raw)
        if (Array.isArray(parsed)) {
          index = parsed.filter((k): k is string => typeof k === 'string')
        }
      }
    } catch {
      index = []
    }

    const nextIndex = [key, ...index.filter((k) => k !== key)]
    while (nextIndex.length > MAX_CACHED_BOOKS) {
      const evicted = nextIndex.pop()
      if (evicted) {
        try {
          localStorage.removeItem(evicted)
        } catch {
          // ignore
        }
      }
    }

    try {
      localStorage.setItem(key, serialized)
      localStorage.setItem(LOCATIONS_INDEX_KEY, JSON.stringify(nextIndex))
    } catch {
      while (nextIndex.length > 1) {
        const evicted = nextIndex.pop()
        if (evicted) {
          try {
            localStorage.removeItem(evicted)
          } catch {
            // ignore
          }
        }
      }
      try {
        localStorage.setItem(key, serialized)
        localStorage.setItem(LOCATIONS_INDEX_KEY, JSON.stringify(nextIndex))
      } catch {
        // ignore if still too large for localStorage
      }
    }
  } catch {
    // ignore storage errors
  }
}

function applyRenditionTheme(): void {
  if (!rendition) return
  const scheme = themeStore.activeScheme
  const r = preferencesStore.reader
  const verticalPad = r.flow === 'scrolled' ? 64 : 16
  const fontStack = resolveFontStack(r.font_family)
  rendition.themes.default({
    body: {
      background: `${scheme.surface} !important`,
      color: `${scheme['on-surface']} !important`,
      'font-family': `${fontStack} !important`,
      'font-size': `${r.font_size}px !important`,
      'line-height': `${r.line_height} !important`,
      padding: `${verticalPad}px ${r.margin_px}px !important`,
    },
    'p, li, blockquote, dd, dt': {
      'font-family': `${fontStack} !important`,
      'font-size': `${r.font_size}px !important`,
      'line-height': `${r.line_height} !important`,
    },
    'div, span, section, article': {
      'font-family': `${fontStack} !important`,
      'line-height': `${r.line_height} !important`,
    },
    a: {
      color: `${scheme.primary} !important`,
    },
  })
}

function handleRenditionClick(...args: unknown[]): void {
  const ev = args[0] as MouseEvent | undefined
  if (!ev) return
  const target = ev.target as HTMLElement | null
  if (target?.closest('a, button, input, select, textarea')) return
  const doc = target?.ownerDocument
  const sel = doc?.getSelection?.()
  if (sel && sel.toString().trim().length > 0) return

  if (preferencesStore.reader.flow !== 'paginated') {
    emit('toggle-hud')
    return
  }

  const width = doc?.defaultView?.innerWidth || window.innerWidth
  if (width <= 0) {
    emit('toggle-hud')
    return
  }
  const ratio = ev.clientX / width
  if (ratio < 0.25) {
    void prevPage()
  } else if (ratio > 0.75) {
    void nextPage()
  } else {
    emit('toggle-hud')
  }
}

function handleRenditionTouchStart(...args: unknown[]): void {
  const ev = args[0] as TouchEvent | undefined
  const touch = ev?.changedTouches?.[0]
  if (!touch) return
  touchStartX = touch.screenX
  touchStartY = touch.screenY
}

function handleRenditionTouchEnd(...args: unknown[]): void {
  if (preferencesStore.reader.flow !== 'paginated') return
  const ev = args[0] as TouchEvent | undefined
  const touch = ev?.changedTouches?.[0]
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

function destroyRendition(): void {
  const r = rendition as EpubRenditionWithOff | null
  if (r && typeof r.off === 'function') {
    r.off('keydown', handleRenditionKeydown)
    r.off('relocated', handleRelocated)
    r.off('click', handleRenditionClick)
    r.off('touchstart', handleRenditionTouchStart)
    r.off('touchend', handleRenditionTouchEnd)
  }
  rendition?.destroy()
  rendition = null
}

async function mountRendition(target?: string): Promise<void> {
  if (!book || !mountEl.value || unmounted) return
  const seq = ++renderSeq
  destroyRendition()

  const isScrolled = preferencesStore.reader.flow === 'scrolled'
  const localRendition = book.renderTo(mountEl.value, {
    width: '100%',
    height: '100%',
    manager: isScrolled ? 'continuous' : 'default',
    flow: isScrolled ? 'scrolled' : 'paginated',
    spread: 'none',
    allowScriptedContent: false,
  })
  rendition = localRendition

  localRendition.on('keydown', handleRenditionKeydown)
  localRendition.on('click', handleRenditionClick)
  localRendition.on('touchstart', handleRenditionTouchStart)
  localRendition.on('touchend', handleRenditionTouchEnd)
  applyRenditionTheme()

  await localRendition.display(target || undefined)
  if (unmounted || seq !== renderSeq) {
    if (rendition === localRendition) {
      destroyRendition()
    } else {
      const r = localRendition as EpubRenditionWithOff
      if (typeof r.off === 'function') {
        r.off('keydown', handleRenditionKeydown)
        r.off('click', handleRenditionClick)
        r.off('touchstart', handleRenditionTouchStart)
        r.off('touchend', handleRenditionTouchEnd)
      }
      localRendition.destroy()
    }
    return
  }

  localRendition.on('relocated', handleRelocated)
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

function isEditableEventTarget(target: EventTarget | null): boolean {
  if (!target || typeof target !== 'object') return false
  const el = target as { tagName?: string; isContentEditable?: boolean }
  const tag = el.tagName?.toUpperCase()
  return tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT' || Boolean(el.isContentEditable)
}

function handleKeydown(ev: KeyboardEvent): void {
  if (ev.metaKey || ev.ctrlKey || ev.altKey) return
  if (isEditableEventTarget(ev.target)) return
  if (document.querySelector('[aria-modal="true"]')) return
  if (ev.key === 'ArrowLeft') {
    ev.preventDefault()
    void prevPage()
  } else if (ev.key === 'ArrowRight') {
    ev.preventDefault()
    void nextPage()
  }
}

function handleRenditionKeydown(...args: unknown[]): void {
  const ev = args[0]
  if (ev && typeof ev === 'object' && 'key' in ev) {
    handleKeydown(ev as KeyboardEvent)
  }
}

interface EpubRelocatedLocation {
  start?: {
    cfi?: string
    index?: number
    percentage?: number
    displayed?: {
      page?: number
      total?: number
    }
  }
}

function getSpineLength(): number {
  const spine = (book as EpubBookWithSpine | null)?.spine
  if (!spine) return 0
  if (Array.isArray(spine.spineItems) && spine.spineItems.length > 0) {
    return spine.spineItems.length
  }
  return typeof spine.length === 'number' && spine.length > 0 ? spine.length : 0
}

function estimateSpineProgress(loc: EpubRelocatedLocation): number {
  const spineLength = getSpineLength()
  if (spineLength <= 0) return 0
  const rawIdx = loc.start?.index
  const chapterIdx =
    typeof rawIdx === 'number' && Number.isFinite(rawIdx)
      ? Math.min(spineLength - 1, Math.max(0, rawIdx))
      : 0
  const displayed = loc.start?.displayed
  const page =
    displayed && typeof displayed.page === 'number' && displayed.page >= 1 ? displayed.page : 1
  const total =
    displayed && typeof displayed.total === 'number' && displayed.total >= 1 ? displayed.total : 1
  const chapterFraction = Math.min(1, Math.max(0, page / total))
  const raw = ((chapterIdx + chapterFraction) / spineLength) * 100
  if (!Number.isFinite(raw) || raw <= 0) return 0
  return Math.min(100, Math.max(0.01, Number(raw.toFixed(2))))
}

function emitLocationProgress(loc: EpubRelocatedLocation | undefined): void {
  const cfi = loc?.start?.cfi
  if (!cfi) return
  lastKnownCfi = cfi
  const chapterIndex =
    typeof loc?.start?.index === 'number' && Number.isFinite(loc.start.index)
      ? loc.start.index
      : null

  let pct = 0
  if (locationsReady && book) {
    const fromCfi = book.locations.percentageFromCfi(cfi) * 100
    if (Number.isFinite(fromCfi) && fromCfi > 0) {
      pct = fromCfi
    }
  }
  if (pct <= 0) {
    const rawPct = (loc?.start?.percentage ?? 0) * 100
    if (Number.isFinite(rawPct) && rawPct > 0) {
      pct = rawPct
    }
  }
  if (pct <= 0) {
    pct = estimateSpineProgress(loc ?? {})
  }
  if (!Number.isFinite(pct) || pct < 0) pct = 0
  if (pct > 0 && pct < 0.01) pct = 0.01

  emit('progress', {
    progress_percent: Math.min(100, Math.max(0, Number(pct.toFixed(2)))),
    locator: cfi,
    chapter_index: chapterIndex,
  })
}

function handleRelocated(...args: unknown[]): void {
  if (unmounted) return
  emitLocationProgress(args[0] as EpubRelocatedLocation | undefined)
}

async function loadEpub(): Promise<void> {
  fetchAbortController?.abort()
  const controller = new AbortController()
  fetchAbortController = controller
  loading.value = true
  errorMsg.value = ''
  try {
    const resp = await fetch(api.fileUrl(props.bookId), {
      credentials: 'include',
      signal: controller.signal,
    })
    if (unmounted || controller.signal.aborted) return
    if (!resp.ok) {
      throw new Error(`HTTP ${resp.status}`)
    }
    const buffer = await resp.arrayBuffer()
    if (unmounted || !mountEl.value) return

    const localBook = ePub(buffer)
    await localBook.ready
    if (unmounted || !mountEl.value) {
      localBook.destroy()
      return
    }
    book = localBook

    const cacheKey = buildLocationsCacheKey(props.bookId, buffer.byteLength)
    const cachedLocations = readCachedLocations(cacheKey)
    const locationsApi = localBook.locations as unknown as EpubLocationsWithCache
    if (cachedLocations && typeof locationsApi.load === 'function') {
      try {
        locationsApi.load(cachedLocations)
        locationsReady = true
      } catch {
        locationsReady = false
      }
    }

    const startTarget = props.initialLocation?.locator || undefined
    await mountRendition(startTarget)
    if (unmounted) {
      destroyRendition()
      localBook.destroy()
      if (book === localBook) book = null
      return
    }
    loading.value = false

    // Self-heal saved progress records that have a locator but 0% progress (e.g. large EPUBs)
    if (startTarget && !(props.initialLocation && props.initialLocation.progress_percent > 0)) {
      const current = (rendition as EpubRenditionWithOff | null)?.currentLocation?.()
      if (current?.start?.cfi) {
        emitLocationProgress(current)
      }
    }

    // Skip full-book locations.generate on huge EPUBs (> 2 MiB) to avoid freezing UI / overflowing localStorage;
    // spine-based progress estimation already reports smooth instant progress.
    if (!locationsReady && buffer.byteLength <= MAX_LOCATIONS_GENERATE_BYTES) {
      const currentBook = localBook
      const currentLocations = currentBook.locations as unknown as EpubLocationsWithCache
      void currentBook.locations
        .generate(1200)
        .then(() => {
          if (unmounted || book !== currentBook) return
          locationsReady = true
          try {
            const serialized =
              typeof currentLocations.save === 'function' ? currentLocations.save() : ''
            if (serialized) {
              writeCachedLocations(cacheKey, serialized)
            }
          } catch {
            // ignore
          }
          if (lastKnownCfi) {
            const pct = currentBook.locations.percentageFromCfi(lastKnownCfi) * 100
            if (Number.isFinite(pct) && pct > 0) {
              const current = (rendition as EpubRenditionWithOff | null)?.currentLocation?.()
              const chapterIndex =
                typeof current?.start?.index === 'number' && Number.isFinite(current.start.index)
                  ? current.start.index
                  : null
              emit('progress', {
                progress_percent: Math.min(100, Math.max(0.01, Number(pct.toFixed(2)))),
                locator: lastKnownCfi,
                chapter_index: chapterIndex,
              })
            }
          }
        })
        .catch(() => {})
    }
  } catch (err) {
    destroyRendition()
    book?.destroy()
    book = null
    if (
      unmounted ||
      controller.signal.aborted ||
      (err instanceof Error && err.name === 'AbortError')
    ) {
      return
    }
    errorMsg.value = err instanceof Error ? err.message : String(err)
    loading.value = false
  }
}

defineExpose({ navigateTo, prevPage, nextPage })

onMounted(() => {
  unmounted = false
  window.addEventListener('keydown', handleKeydown)
  void loadEpub()
})

watch(
  () => [
    themeStore.activeScheme,
    preferencesStore.reader.font_family,
    preferencesStore.reader.font_size,
    preferencesStore.reader.line_height,
    preferencesStore.reader.margin_px,
  ],
  () => {
    applyRenditionTheme()
  },
)

watch(
  () => preferencesStore.reader.flow,
  () => {
    if (!book || !mountEl.value || loading.value) return
    const resumeTarget = lastKnownCfi || props.initialLocation?.locator || undefined
    void mountRendition(resumeTarget).catch(() => {})
  },
)

onBeforeUnmount(() => {
  unmounted = true
  renderSeq += 1
  fetchAbortController?.abort()
  fetchAbortController = null
  window.removeEventListener('keydown', handleKeydown)
  destroyRendition()
  book?.destroy()
  book = null
})
</script>

<template>
  <div
    class="safe-pt safe-pb relative flex h-full w-full items-center justify-center overflow-hidden bg-surface pt-14 pb-10"
  >
    <div
      v-if="loading"
      class="flex flex-col items-center justify-center gap-3 text-on-surface-variant"
    >
      <MdCircularProgress indeterminate :size="32" :stroke-width="3" />
      <span class="text-body-small">{{ t('reader.loadingBook') }}</span>
    </div>

    <MediaErrorState
      v-else-if="errorMsg"
      :title="t('reader.loadFailedTitle')"
      :message="errorMsg"
      :retry-label="t('reader.retry')"
      class="max-w-md"
      @retry="loadEpub"
    />

    <div ref="mountEl" class="h-full w-full max-w-5xl" />

    <!-- Subtle Desktop Edge Page-Turn Controls (hidden on mobile & when HUD is idle) -->
    <template v-if="preferencesStore.reader.flow === 'paginated' && !loading && !errorMsg">
      <div
        class="pointer-events-none fixed inset-y-0 left-3 hidden items-center transition-opacity duration-200 ease-standard sm:flex"
        :class="hudVisible ? 'opacity-100' : 'opacity-0'"
      >
        <MdIconButton
          variant="tonal"
          :icon="LuChevronLeft"
          :aria-label="t('reader.prevPage')"
          :tooltip="t('reader.prevPage')"
          class="pointer-events-auto shadow-elevation-1"
          @click.stop="prevPage"
        />
      </div>

      <div
        class="pointer-events-none fixed inset-y-0 right-3 hidden items-center transition-opacity duration-200 ease-standard sm:flex"
        :class="hudVisible ? 'opacity-100' : 'opacity-0'"
      >
        <MdIconButton
          variant="tonal"
          :icon="LuChevronRight"
          :aria-label="t('reader.nextPage')"
          :tooltip="t('reader.nextPage')"
          class="pointer-events-auto shadow-elevation-1"
          @click.stop="nextPage"
        />
      </div>
    </template>
  </div>
</template>
