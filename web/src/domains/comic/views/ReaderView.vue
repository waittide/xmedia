<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useI18n as useGlobalI18n } from 'vue-i18n'
import { useComicI18n as useI18n } from '@/domains/comic/i18n'
import { LuBookmark } from 'vue-icons-plus/lu'
import { api, chapterPageUrl } from '@/domains/comic/api'
import ComicReaderShell, {
  type ComicReaderChapterItem,
} from '@/domains/comic/components/ComicReaderShell.vue'
import { useSidebarStore } from '@/domains/comic/store'
import { useSnackbarStore } from '@/stores/snackbar'
import { useThemeStore } from '@/stores/theme'
import type {
  ComicBookmark,
  ReaderContextDto,
  ReaderFitMode,
  ReaderMode,
} from '@/domains/comic/types'

const { t } = useI18n()
const { t: globalT } = useGlobalI18n()
const route = useRoute()
const router = useRouter()
const theme = useThemeStore()
const sidebarStore = useSidebarStore()
const snackbar = useSnackbarStore()

const ctx = ref<ReaderContextDto | null>(null)
const chapterBookmarks = ref<ComicBookmark[]>([])
const loading = ref(true)
const loadError = ref<string | null>(null)

const currentPage = ref(1)
const initialPage = ref(1)
const readerMode = ref<ReaderMode>('rtl_double')
const fitMode = ref<ReaderFitMode>('fit_height')
const offsetCoverPage = ref(true)
const dimImages = ref(false)
const webtoonWidth = ref(860)

let progressTimer: ReturnType<typeof setTimeout> | null = null
let pendingProgress: { chapterId: string; page: number; completed?: boolean } | null = null
let chapterLoadSeq = 0
let bookmarkBusy = false

const totalPages = computed(() => ctx.value?.chapter.page_count || 1)

const chapterItems = computed<ComicReaderChapterItem[]>(() =>
  (ctx.value?.all_chapters ?? []).map((ch) => ({
    key: ch.id,
    label: `#${ch.number_label} ${ch.title}`,
    metaLabel: `${ch.page_count}P`,
  })),
)

const nextChapterToastLabel = computed(() => {
  const next = ctx.value?.next_chapter
  return next ? `#${next.number_label} ${next.title}` : ''
})

function getPageSrc(pageNum: number): string {
  if (!ctx.value) return ''
  return chapterPageUrl(ctx.value.chapter.id, pageNum)
}

async function loadChapter(
  chapterId: string,
  queryPage?: number,
  seamlessToast = false,
): Promise<void> {
  const seq = ++chapterLoadSeq
  flushPendingProgress()
  loading.value = true
  loadError.value = null

  try {
    const [data, bms] = await Promise.all([
      api.getReaderContext(chapterId),
      api.listChapterBookmarks(chapterId).catch(() => []),
    ])
    if (seq !== chapterLoadSeq) return
    ctx.value = data
    chapterBookmarks.value = bms

    readerMode.value = data.effective_pref.reader_mode
    fitMode.value = data.effective_pref.fit_mode
    offsetCoverPage.value = data.effective_pref.offset_cover_page
    dimImages.value = data.effective_pref.dim_images_in_dark
    webtoonWidth.value = data.effective_pref.webtoon_width

    const targetInitial = queryPage || data.chapter.current_page || 1
    const clamped = Math.min(Math.max(1, targetInitial), Math.max(1, data.chapter.page_count))
    initialPage.value = clamped
    currentPage.value = clamped

    scheduleSaveProgress(clamped, clamped >= Math.max(1, data.chapter.page_count) ? true : undefined)

    if (seamlessToast) {
      snackbar.show(
        t('reader.switchedToChapter', {
          title: `#${data.chapter.number_label} ${data.chapter.title}`,
        }),
      )
    }

    loading.value = false
  } catch (err) {
    if (seq !== chapterLoadSeq) return
    loadError.value = err instanceof Error ? err.message : String(err)
    loading.value = false
  }
}

function handleComicPageHide(): void {
  flushPendingProgress(true)
}

onMounted(() => {
  const qPage = route.query.page ? Number(route.query.page) : undefined
  void loadChapter(String(route.params.chapterId), qPage, false)
  window.addEventListener('pagehide', handleComicPageHide)
})

onBeforeUnmount(() => {
  chapterLoadSeq += 1
  window.removeEventListener('pagehide', handleComicPageHide)
  flushPendingProgress()
})

watch(
  () => route.params.chapterId,
  (newId, oldId) => {
    if (newId && newId !== oldId) {
      const qPage = route.query.page ? Number(route.query.page) : 1
      void loadChapter(String(newId), qPage, true)
    }
  },
)

function flushPendingProgress(useKeepalive = false): void {
  if (progressTimer) {
    clearTimeout(progressTimer)
    progressTimer = null
  }
  if (!pendingProgress) return
  const payload = pendingProgress
  pendingProgress = null
  if (useKeepalive) {
    try {
      void fetch(`/api/v1/comic/chapters/${encodeURIComponent(payload.chapterId)}/progress`, {
        method: 'PUT',
        credentials: 'include',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          current_page: payload.page,
          completed: payload.completed,
        }),
        keepalive: true,
      })
    } catch {
      // ignore unload errors
    }
    return
  }
  void api.updateProgress(payload.chapterId, payload.page, payload.completed).catch(() => {})
}

function scheduleSaveProgress(highestPage: number, forceCompleted?: boolean): void {
  if (!ctx.value) return
  const chapterId = ctx.value.chapter.id
  const reachedEnd = highestPage >= totalPages.value
  const completed = forceCompleted ?? (reachedEnd ? true : undefined)

  pendingProgress = { chapterId, page: highestPage, completed }
  if (progressTimer) clearTimeout(progressTimer)
  progressTimer = setTimeout(() => {
    flushPendingProgress()
  }, 260)
}

function handlePageChange(payload: {
  page: number
  highestInSpread: number
  reachedEnd: boolean
}): void {
  currentPage.value = payload.page
  scheduleSaveProgress(payload.highestInSpread, payload.reachedEnd ? true : undefined)
}

async function persistSeriesReaderPref(): Promise<void> {
  if (!ctx.value) return
  await api.saveSeriesReaderPref(ctx.value.series_id, {
    reader_mode: readerMode.value,
    fit_mode: fitMode.value,
    offset_cover_page: offsetCoverPage.value,
    webtoon_width: webtoonWidth.value,
  })
}

function changeMode(mode: ReaderMode): void {
  readerMode.value = mode
  void persistSeriesReaderPref()
}

function changeFit(fit: ReaderFitMode): void {
  fitMode.value = fit
  void persistSeriesReaderPref()
}

function toggleOffsetCover(val: boolean): void {
  offsetCoverPage.value = val
  void persistSeriesReaderPref()
}

function toggleDimImages(val: boolean): void {
  dimImages.value = val
  theme.updateComicPreferences({ dim_images_in_dark: val })
}

function handleWebtoonWidthCommit(val: number): void {
  webtoonWidth.value = val
  void persistSeriesReaderPref()
}

function goToChapter(chapterId: string): void {
  void router.replace('/comic/read/' + chapterId + '?page=1')
}

function handleBack(): void {
  if (ctx.value) {
    void router.push('/comic/series/' + ctx.value.series_id)
  } else {
    void router.push('/comic')
  }
}

const currentPageBookmark = computed<ComicBookmark | null>(
  () => chapterBookmarks.value.find((b) => b.page_number === currentPage.value) ?? null,
)

async function togglePageBookmark(): Promise<void> {
  if (!ctx.value || bookmarkBusy) return
  bookmarkBusy = true
  try {
    const page = currentPage.value
    const existing = currentPageBookmark.value
    if (existing) {
      await api.deleteBookmark(existing.id)
      chapterBookmarks.value = chapterBookmarks.value.filter((b) => b.id !== existing.id)
      snackbar.show(globalT('bookmarksPage.removedBookmark', { page }))
    } else {
      const created = await api.createBookmark(ctx.value.chapter.id, {
        page_number: page,
        title: `#${ctx.value.chapter.number_label} · ${globalT('bookmarksPage.pageLabel', { page })}`,
      })
      chapterBookmarks.value = [...chapterBookmarks.value, created]
      snackbar.show(globalT('bookmarksPage.addedBookmark', { page }))
    }
    void sidebarStore.refresh()
  } catch (err) {
    snackbar.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  } finally {
    bookmarkBusy = false
  }
}
</script>

<template>
  <ComicReaderShell
    :title="ctx?.series_title ?? ''"
    :subtitle="ctx ? `#${ctx.chapter.number_label} ${ctx.chapter.title}` : ''"
    :back-label="t('reader.backToSeries')"
    :chapter-key="ctx?.chapter.id ?? String(route.params.chapterId)"
    :total-pages="totalPages"
    :initial-page="initialPage"
    :get-page-src="getPageSrc"
    :chapters="chapterItems"
    :has-prev-chapter="Boolean(ctx?.prev_chapter)"
    :has-next-chapter="Boolean(ctx?.next_chapter)"
    :next-chapter-toast-label="nextChapterToastLabel"
    :reader-mode="readerMode"
    :fit-mode="fitMode"
    :offset-cover-page="offsetCoverPage"
    :dim-images="dimImages"
    :webtoon-width="webtoonWidth"
    :preload-pages="ctx?.effective_pref.preload_pages ?? 3"
    :loading="loading"
    :load-error="loadError"
    :has-content="Boolean(ctx)"
    @update:reader-mode="changeMode"
    @update:fit-mode="changeFit"
    @update:offset-cover-page="toggleOffsetCover"
    @update:dim-images="toggleDimImages"
    @update:webtoon-width="webtoonWidth = $event"
    @commit-webtoon-width="handleWebtoonWidthCommit"
    @page-change="handlePageChange"
    @prev-chapter="ctx?.prev_chapter && goToChapter(ctx.prev_chapter.id)"
    @next-chapter="ctx?.next_chapter && goToChapter(ctx.next_chapter.id)"
    @select-chapter="goToChapter($event)"
    @back="handleBack"
    @retry="loadChapter(String(route.params.chapterId), undefined, false)"
  >
    <template #toolbar-actions>
      <button
        v-if="ctx"
        type="button"
        :title="globalT('bookmarksPage.bookmarkPage')"
        :aria-label="globalT('bookmarksPage.bookmarkPage')"
        class="md-icon-btn md-icon-btn-sm md-state"
        :class="
          currentPageBookmark
            ? '!bg-[var(--md-sys-color-primary-container)] !text-[var(--md-sys-color-on-primary-container)]'
            : ''
        "
        @click="togglePageBookmark"
      >
        <LuBookmark class="h-4 w-4" :class="currentPageBookmark ? 'fill-current' : ''" />
      </button>
    </template>
  </ComicReaderShell>
</template>
