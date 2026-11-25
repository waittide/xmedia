<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useI18n as useGlobalI18n } from 'vue-i18n'
import { useRoute, useRouter } from 'vue-router'
import {
  LuArrowUpDown,
  LuBookOpen,
  LuLibrary,
  LuPlay,
  LuRefreshCw,
} from 'vue-icons-plus/lu'
import {
  MediaDetailTemplate,
  type MediaDetailSpec,
  type MediaDetailTag,
} from '@/components/domain'
import {
  MdButton,
  MdCard,
  MdEmptyState,
  MdMenuItem,
  MdTextField,
} from '@/components/ui'
import ComicChapterRow from '@/domains/comic/components/ComicChapterRow.vue'
import { useComicI18n as useI18n } from '@/domains/comic/i18n'
import {
  onlineComicApi,
  resolveOnlineComicCoverUrl,
  type ComicReadingDirection,
  type OnlineComicChapter,
  type OnlineComicContextStorage,
  type OnlineComicDetailDto,
  type OnlineComicReaderContextStorage,
} from '@/domains/comic/onlineApi'
import { useSidebarStore } from '@/domains/comic/store'
import {
  readOnlineComicContext,
  saveOnlineComicContext,
  saveOnlineComicReaderContext,
} from '@/domains/comic/utils/mappers'
import {
  itemRoute,
  libraryRoute,
  parseItemRef,
  playRoute,
  type TrackedItemRef,
} from '@/sources'
import { useSnackbarStore } from '@/stores/snackbar'
import { useUploadStore } from '@/stores/uploads'

const { t } = useI18n()
const { t: globalT } = useGlobalI18n()
const route = useRoute()
const router = useRouter()
const snackbar = useSnackbarStore()
const sidebarStore = useSidebarStore()
const uploadStore = useUploadStore()

const loading = ref(true)
const trackingBusy = ref(false)
const errorMessage = ref<string | null>(null)
const detail = ref<OnlineComicDetailDto | null>(null)
const chapters = ref<OnlineComicChapter[]>([])
const resolvedSourceId = ref('')
const resolvedSourceName = ref('')
const resolvedComicUrl = ref('')
const fallbackCtx = ref<OnlineComicContextStorage | null>(null)

const orderAsc = ref(true)
const chapterQuery = ref('')

let loadSeq = 0
let detailAbortController: AbortController | null = null
let unmounted = false

function isAbortError(err: unknown): boolean {
  return err instanceof DOMException
    ? err.name === 'AbortError'
    : err instanceof Error && err.name === 'AbortError'
}

function readQueryParam(key: string): string {
  const val = route.query[key]
  return typeof val === 'string' && val.trim() ? val.trim() : ''
}

function hydrateParams(): { sourceId: string; comicUrl: string } {
  const parsed = parseItemRef(route, 'comic')
  const parsedSourceId = parsed && parsed.scope !== 'local' ? parsed.sourceId : ''
  const parsedComicUrl = parsed && parsed.scope !== 'local' ? parsed.ref : ''
  const parsedHints = parsed && parsed.scope !== 'local' ? parsed.hints : undefined

  const qSourceName = readQueryParam('sourceName')
  const qTocUrl = parsedHints?.extra?.toc || readQueryParam('toc')

  const stored = readOnlineComicContext()
  const matchesStored = Boolean(
    stored &&
      ((parsedSourceId &&
        parsedComicUrl &&
        stored.sourceId === parsedSourceId &&
        stored.comicUrl === parsedComicUrl) ||
        (!parsedSourceId && !parsedComicUrl && Boolean(stored.sourceId && stored.comicUrl))),
  )

  const sourceId = parsedSourceId || (matchesStored && stored ? stored.sourceId : '')
  const comicUrl = parsedComicUrl || (matchesStored && stored ? stored.comicUrl : '')

  fallbackCtx.value = {
    sourceId,
    comicUrl,
    sourceName: qSourceName || (matchesStored ? stored?.sourceName : undefined),
    title: parsedHints?.title || (matchesStored ? stored?.title : undefined),
    author: parsedHints?.author || (matchesStored ? stored?.author : undefined),
    coverUrl: parsedHints?.cover || (matchesStored ? stored?.coverUrl : undefined),
    tocUrl: qTocUrl || (matchesStored ? stored?.tocUrl : undefined),
    intro: matchesStored ? stored?.intro : undefined,
    readingDirection: matchesStored ? stored?.readingDirection : undefined,
    returnKeyword: matchesStored ? stored?.returnKeyword : undefined,
    returnSourceId: matchesStored ? stored?.returnSourceId : undefined,
  }

  resolvedSourceId.value = sourceId
  resolvedSourceName.value = fallbackCtx.value.sourceName || ''
  resolvedComicUrl.value = comicUrl

  return { sourceId, comicUrl }
}

const displayTitle = computed(
  () => detail.value?.title || fallbackCtx.value?.title || resolvedComicUrl.value || 'Comic',
)

const rawAuthor = computed(() => detail.value?.author || fallbackCtx.value?.author || '')

const displayAuthor = computed(() => rawAuthor.value || t('online.unknownAuthor'))

const displaySummary = computed(() => detail.value?.intro || fallbackCtx.value?.intro || null)

const displayCoverUrl = computed(() =>
  resolveOnlineComicCoverUrl(
    detail.value?.proxy_cover_url,
    detail.value?.cover_url ?? fallbackCtx.value?.coverUrl,
    resolvedSourceId.value,
  ),
)

const displayDirection = computed<ComicReadingDirection>(
  () => detail.value?.reading_direction || fallbackCtx.value?.readingDirection || 'rtl',
)

const effectiveSourceName = computed(() => resolvedSourceName.value)

const directionLabel = computed(() => {
  if (displayDirection.value === 'ltr') return t('series.directionLtr')
  if (displayDirection.value === 'webtoon') return t('series.directionWebtoon')
  return t('series.directionRtl')
})

const heroSubtitle = computed(() => {
  const parts: string[] = [displayAuthor.value]
  if (effectiveSourceName.value) parts.push(effectiveSourceName.value)
  if (detail.value?.status) parts.push(detail.value.status)
  return parts.join(' · ')
})

const kindTags = computed<MediaDetailTag[]>(() => {
  const raw = detail.value?.kind
  if (!raw) return []
  return raw
    .split(/[,，|/]/)
    .map((s) => s.trim())
    .filter(Boolean)
    .map((label) => ({ label, value: label }))
})

const detailSpecs = computed<MediaDetailSpec[]>(() => {
  const specs: MediaDetailSpec[] = []
  if (effectiveSourceName.value) {
    specs.push({ label: t('online.specSource'), value: effectiveSourceName.value })
  }
  if (detail.value?.status) {
    specs.push({ label: t('online.specStatus'), value: detail.value.status })
  }
  specs.push({ label: t('online.readingDirection'), value: directionLabel.value })
  if (detail.value?.last_chapter) {
    specs.push({ label: t('online.specLatestChapter'), value: detail.value.last_chapter })
  }
  if (chapters.value.length > 0) {
    specs.push({
      label: t('online.specChapterCount'),
      value: t('online.chaptersCount', { count: chapters.value.length }),
    })
  }
  return specs
})

const readableChapters = computed(() =>
  chapters.value.filter((ch) => !ch.is_volume && Boolean(ch.url?.trim())),
)

const displayedChapters = computed(() => {
  const q = chapterQuery.value.trim().toLowerCase()
  const base = orderAsc.value ? [...chapters.value] : [...chapters.value].reverse()
  if (!q) return base
  return base.filter((ch) => ch.title.toLowerCase().includes(q))
})

const backRoute = computed(() =>
  libraryRoute('comic', {
    scope: 'online',
    q: fallbackCtx.value?.returnKeyword || null,
    sourceId: fallbackCtx.value?.returnSourceId || null,
  }),
)

async function loadDetail(): Promise<void> {
  const { sourceId, comicUrl } = hydrateParams()
  detailAbortController?.abort()
  const controller = new AbortController()
  detailAbortController = controller
  const seq = ++loadSeq
  errorMessage.value = null

  if (!sourceId || !comicUrl) {
    detail.value = null
    chapters.value = []
    loading.value = false
    errorMessage.value = t('online.missingDetailContext')
    return
  }

  loading.value = true
  try {
    const res = await onlineComicApi.fetchDetail(
      sourceId,
      comicUrl,
      {
        title: fallbackCtx.value?.title,
        author: fallbackCtx.value?.author,
        comic_url: comicUrl,
        cover_url: fallbackCtx.value?.coverUrl ?? null,
        intro: fallbackCtx.value?.intro ?? null,
      },
      { signal: controller.signal },
    )
    if (unmounted || seq !== loadSeq) return

    let loadedChapters = res.chapters ?? []
    if (loadedChapters.length === 0 && res.detail.toc_url?.trim()) {
      const tocRes = await onlineComicApi.fetchChapters(sourceId, res.detail.toc_url, res.detail, {
        signal: controller.signal,
      })
      if (unmounted || seq !== loadSeq) return
      loadedChapters = tocRes.chapters ?? []
    }

    detail.value = res.detail
    chapters.value = loadedChapters
    resolvedSourceName.value = res.source_name || resolvedSourceName.value

    saveOnlineComicContext({
      sourceId,
      sourceName: resolvedSourceName.value,
      comicUrl: res.detail.comic_url || comicUrl,
      tocUrl: res.detail.toc_url,
      title: res.detail.title || fallbackCtx.value?.title,
      author: res.detail.author || fallbackCtx.value?.author,
      coverUrl: res.detail.cover_url ?? fallbackCtx.value?.coverUrl ?? null,
      intro: res.detail.intro ?? fallbackCtx.value?.intro ?? null,
      readingDirection: res.detail.reading_direction,
      returnKeyword: fallbackCtx.value?.returnKeyword,
      returnSourceId: fallbackCtx.value?.returnSourceId,
    })
  } catch (err) {
    if (controller.signal.aborted || isAbortError(err) || unmounted || seq !== loadSeq) return
    errorMessage.value = err instanceof Error ? err.message : String(err)
  } finally {
    if (detailAbortController === controller) {
      detailAbortController = null
    }
    if (!unmounted && seq === loadSeq) {
      loading.value = false
    }
  }
}

onMounted(() => {
  unmounted = false
  void loadDetail()
})

onBeforeUnmount(() => {
  unmounted = true
  loadSeq += 1
  detailAbortController?.abort()
  detailAbortController = null
})

watch(
  () => [route.query.source, route.query.ref],
  () => {
    void loadDetail()
  },
)

function openChapter(ch: OnlineComicChapter, initialPage = 1): void {
  if (ch.is_volume || !ch.url?.trim() || !resolvedSourceId.value) return
  const idxByUrlAndIndex = chapters.value.findIndex(
    (c) => c.url === ch.url && c.index === ch.index,
  )
  const chapterIndex =
    idxByUrlAndIndex >= 0
      ? idxByUrlAndIndex
      : Math.max(
          0,
          chapters.value.findIndex((c) => c.url === ch.url),
        )

  const comicUrl = detail.value?.comic_url || resolvedComicUrl.value
  const tocUrl = detail.value?.toc_url || fallbackCtx.value?.tocUrl || ''
  const title = displayTitle.value
  const sourceName = effectiveSourceName.value
  const readingDirection = displayDirection.value

  const readerCtx: OnlineComicReaderContextStorage = {
    sourceId: resolvedSourceId.value,
    sourceName,
    comicUrl,
    tocUrl,
    title,
    author: displayAuthor.value,
    coverUrl: detail.value?.cover_url ?? fallbackCtx.value?.coverUrl ?? null,
    readingDirection,
    chapterUrl: ch.url,
    chapterIndex,
    chapterTitle: ch.title,
    chapters: chapters.value,
  }
  saveOnlineComicReaderContext(readerCtx)

  const refObj: TrackedItemRef = {
    scope: 'online',
    domain: 'comic',
    sourceId: resolvedSourceId.value,
    ref: comicUrl,
    hints: {
      title,
      cover: detail.value?.cover_url ?? fallbackCtx.value?.coverUrl ?? undefined,
      author: rawAuthor.value || undefined,
      extra: {
        chapter: ch.url,
        ...(tocUrl ? { toc: tocUrl } : {}),
        ...(initialPage > 1 ? { page: String(initialPage) } : {}),
      },
    },
  }

  void router.push(
    playRoute(refObj, {
      unitKey: ch.url,
      unitIndex: chapterIndex,
      unitTitle: ch.title,
      position: initialPage,
    }),
  )
}

function handlePrimaryRead(): void {
  const first = readableChapters.value[0] ?? chapters.value[0]
  if (first) {
    openChapter(first, 1)
  }
}

async function handleTrackToLibrary(): Promise<void> {
  if (trackingBusy.value || !resolvedSourceId.value || !resolvedComicUrl.value) {
    return
  }
  trackingBusy.value = true
  try {
    const genres = kindTags.value.map((tg) => tg.label).filter(Boolean)
    const created = await onlineComicApi.trackOnlineComic({
      source_id: resolvedSourceId.value,
      comic_url: detail.value?.comic_url || resolvedComicUrl.value,
      toc_url: detail.value?.toc_url || fallbackCtx.value?.tocUrl || null,
      title: displayTitle.value || null,
      author: rawAuthor.value || null,
      cover_url: detail.value?.cover_url ?? fallbackCtx.value?.coverUrl ?? null,
      summary: displaySummary.value,
      reading_direction: displayDirection.value,
      genres: genres.length > 0 ? genres : null,
      status: detail.value?.status ?? null,
    })
    uploadStore.notifyCatalogChanged()
    void sidebarStore.refresh()
    snackbar.show({ message: t('online.trackedToLibraryToast') })
    await router.push(itemRoute({ scope: 'local', domain: 'comic', id: created.series.id }))
  } catch (err) {
    snackbar.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  } finally {
    trackingBusy.value = false
  }
}
</script>

<template>
  <div class="px-4 py-6 sm:px-6 md:px-8">
    <MediaDetailTemplate
      :title="displayTitle"
      :subtitle="heroSubtitle"
      :cover="displayCoverUrl || null"
      aspect="poster"
      cover-style="book"
      :fallback-icon="LuBookOpen"
      :show-back="true"
      :back-label="t('online.backToSearch')"
      :back-to="backRoute"
      :primary-action-label="t('series.startReading')"
      :primary-action-icon="LuPlay"
      :primary-action-disabled="readableChapters.length === 0"
      :secondary-action-label="
        detail ? (trackingBusy ? t('online.trackingBtn') : t('online.trackToLibraryBtn')) : ''
      "
      :secondary-action-icon="LuLibrary"
      :secondary-action-disabled="trackingBusy"
      :show-favorite="false"
      :description="displaySummary"
      :tags="kindTags"
      :specs="detailSpecs"
      :specs-title="t('series.specsTitle')"
      :loading="loading && !detail"
      :error="errorMessage"
      @primary-action="handlePrimaryRead"
      @secondary-action="handleTrackToLibrary"
      @retry="loadDetail"
    >
      <template v-if="resolvedSourceId && resolvedComicUrl" #overflow-menu="{ close }">
        <MdMenuItem
          :label="globalT('common.refresh')"
          :icon="LuRefreshCw"
          @select="
            close();
            loadDetail()
          "
        />
      </template>

      <!-- Chapters Section -->
      <section class="space-y-4">
        <div class="flex flex-wrap items-center justify-between gap-3">
          <div class="flex items-center gap-2.5">
            <h2 class="text-title-large text-on-surface">
              {{ t('series.chaptersHeader') }}
            </h2>
            <span
              class="text-label-medium rounded-corner-full bg-surface-container-high px-2.5 py-0.5 tabular-nums text-on-surface-variant"
            >
              {{ t('online.chaptersCount', { count: chapters.length }) }}
            </span>
          </div>

          <div class="flex flex-wrap items-center gap-2.5">
            <div v-if="chapters.length > 10" class="w-48">
              <MdTextField
                v-model="chapterQuery"
                type="search"
                size="sm"
                clearable
                :placeholder="t('online.filterChaptersPlaceholder')"
              />
            </div>

            <MdButton
              variant="outlined"
              :icon="LuArrowUpDown"
              @click="orderAsc = !orderAsc"
            >
              {{ orderAsc ? t('series.orderAsc') : t('series.orderDesc') }}
            </MdButton>
          </div>
        </div>

        <MdEmptyState
          v-if="!loading && chapters.length === 0"
          compact
          :icon="LuBookOpen"
          :title="t('online.emptyChapters')"
        />

        <MdEmptyState
          v-else-if="!loading && displayedChapters.length === 0"
          compact
          :icon="LuBookOpen"
          :title="t('series.noMatchingChapters')"
        />

        <MdCard v-else variant="filled" padding="none" class="overflow-hidden">
          <div class="divide-y divide-outline-variant/50">
            <ComicChapterRow
              v-for="ch in displayedChapters"
              :key="`${ch.index}-${ch.url}`"
              :badge="ch.is_volume ? t('online.volumeBadge') : `#${ch.index + 1}`"
              :title="ch.title"
              :meta-text="ch.update_time || ''"
              :disabled="ch.is_volume || !ch.url"
              @open="openChapter(ch, 1)"
            />
          </div>
        </MdCard>
      </section>
    </MediaDetailTemplate>
  </div>
</template>
