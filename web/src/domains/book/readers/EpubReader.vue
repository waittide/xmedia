<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useBookI18n as useI18n } from '@/domains/book/i18n'
import ePub, { type Book, type Rendition } from 'epubjs'
import { LuChevronLeft, LuChevronRight } from 'vue-icons-plus/lu'
import { api } from '@/domains/book/api'
import { useThemeStore } from '@/stores/theme'
import { resolveFontStack } from '@/utils/format'
import type { ReadingLocation } from '@/domains/book/types'

const LOCATIONS_CACHE_PREFIX = 'xmedia.epub.locations:'
const LOCATIONS_INDEX_KEY = 'xmedia.epub.locations.__index'
const MAX_CACHED_BOOKS = 12

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
let lastKnownCfi = ''
let unmounted = false

interface EpubLocationsWithCache {
  generate(chars?: number): Promise<string[]>
  percentageFromCfi(cfi: string): number
  load?: (locations: string) => void
  save?: () => string
}

interface EpubRenditionWithOff extends Rendition {
  off?: (event: string, handler: (...args: unknown[]) => void) => void
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
      // On QuotaExceededError, evict older entries and retry once
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

function isEditableEventTarget(target: EventTarget | null): boolean {
  if (!target || typeof target !== 'object') return false
  const el = target as { tagName?: string; isContentEditable?: boolean }
  const tag = el.tagName?.toUpperCase()
  return tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT' || Boolean(el.isContentEditable)
}

function handleKeydown(ev: KeyboardEvent): void {
  if (ev.metaKey || ev.ctrlKey || ev.altKey) return
  if (isEditableEventTarget(ev.target)) return
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

function handleRelocated(...args: unknown[]): void {
  if (unmounted) return
  const loc = args[0] as { start?: { cfi?: string; percentage?: number } } | undefined
  const cfi = loc?.start?.cfi
  if (!cfi) return
  lastKnownCfi = cfi
  let pct = (loc?.start?.percentage ?? 0) * 100
  if (locationsReady && book) {
    pct = book.locations.percentageFromCfi(cfi) * 100
  }
  if (!Number.isFinite(pct)) pct = 0
  emit('progress', {
    progress_percent: Math.min(100, Math.max(0, Number(pct.toFixed(2)))),
    locator: cfi,
  })
}

defineExpose({ navigateTo })

onMounted(async () => {
  unmounted = false
  window.addEventListener('keydown', handleKeydown)
  try {
    const resp = await fetch(api.fileUrl(props.bookId), {
      credentials: 'include',
    })
    if (unmounted) return
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

    const flowMode = themeStore.reader.flow === 'scrolled' ? 'scrolled-doc' : 'paginated'
    const localRendition = localBook.renderTo(mountEl.value, {
      width: '100%',
      height: '100%',
      flow: flowMode,
      spread: 'none',
      allowScriptedContent: false,
    })
    rendition = localRendition

    // Ensure arrow-key paging still works after clicking inside the epub iframe
    localRendition.on('keydown', handleRenditionKeydown)

    applyRenditionTheme()

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
    await localRendition.display(startTarget)
    if (unmounted) {
      const r = localRendition as EpubRenditionWithOff
      if (typeof r.off === 'function') {
        r.off('keydown', handleRenditionKeydown)
      }
      localRendition.destroy()
      localBook.destroy()
      if (rendition === localRendition) rendition = null
      if (book === localBook) book = null
      return
    }
    loading.value = false

    localRendition.on('relocated', handleRelocated)

    if (!locationsReady) {
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
            if (Number.isFinite(pct)) {
              emit('progress', {
                progress_percent: Math.min(100, Math.max(0, Number(pct.toFixed(2)))),
                locator: lastKnownCfi,
              })
            }
          }
        })
        .catch(() => {
          // ignore background location indexing errors
        })
    }
  } catch (err) {
    const r = rendition as EpubRenditionWithOff | null
    if (r && typeof r.off === 'function') {
      r.off('keydown', handleRenditionKeydown)
      r.off('relocated', handleRelocated)
    }
    rendition?.destroy()
    book?.destroy()
    rendition = null
    book = null
    if (unmounted) return
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
    applyRenditionTheme()
  },
)

onBeforeUnmount(() => {
  unmounted = true
  window.removeEventListener('keydown', handleKeydown)
  const r = rendition as EpubRenditionWithOff | null
  if (r && typeof r.off === 'function') {
    r.off('keydown', handleRenditionKeydown)
    r.off('relocated', handleRelocated)
  }
  rendition?.destroy()
  book?.destroy()
  rendition = null
  book = null
})
</script>

<template>
  <div class="relative flex h-full w-full items-center justify-center overflow-hidden bg-[var(--md-sys-color-surface)]">
    <div v-if="loading" class="text-sm text-[var(--md-sys-color-on-surface-variant)]">
      {{ t('reader.loadingBook') }}
    </div>
    <div v-else-if="errorMsg" class="text-sm text-[var(--md-sys-color-error)]">
      {{ errorMsg }}
    </div>

    <div ref="mountEl" class="h-full w-full max-w-5xl" />

    <!-- Paginated Prev / Next Floating Buttons -->
    <template v-if="themeStore.reader.flow === 'paginated' && !loading">
      <button
        type="button"
        class="md-icon-btn md-state absolute left-3 top-1/2 !h-11 !w-11 -translate-y-1/2 border border-[var(--md-sys-color-outline-variant)]/70 bg-[var(--md-sys-color-surface-container-high)]/90 text-[var(--md-sys-color-on-surface)] shadow-[var(--md-sys-elevation-2)] backdrop-blur-sm hover:bg-[var(--md-sys-color-primary)] hover:text-[var(--md-sys-color-on-primary)]"
        :title="t('reader.prevPage')"
        @click="prevPage"
      >
        <LuChevronLeft class="h-5 w-5" />
      </button>

      <button
        type="button"
        class="md-icon-btn md-state absolute right-3 top-1/2 !h-11 !w-11 -translate-y-1/2 border border-[var(--md-sys-color-outline-variant)]/70 bg-[var(--md-sys-color-surface-container-high)]/90 text-[var(--md-sys-color-on-surface)] shadow-[var(--md-sys-elevation-2)] backdrop-blur-sm hover:bg-[var(--md-sys-color-primary)] hover:text-[var(--md-sys-color-on-primary)]"
        :title="t('reader.nextPage')"
        @click="nextPage"
      >
        <LuChevronRight class="h-5 w-5" />
      </button>
    </template>
  </div>
</template>
