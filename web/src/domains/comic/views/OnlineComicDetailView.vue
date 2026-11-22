<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useComicI18n as useI18n } from '@/domains/comic/i18n'
import {
  LuArrowUpDown,
  LuBookOpen,
  LuChevronLeft,
  LuPlay,
  LuSearch,
} from 'vue-icons-plus/lu'
import MediaCoverImage from '@/components/ui/MediaCoverImage.vue'
import RefreshButton from '@/components/ui/RefreshButton.vue'
import {
  ONLINE_COMIC_CTX_KEY,
  ONLINE_COMIC_READER_CTX_KEY,
  onlineComicApi,
  resolveOnlineComicCoverUrl,
  type ComicReadingDirection,
  type OnlineComicChapter,
  type OnlineComicContextStorage,
  type OnlineComicDetailDto,
  type OnlineComicReaderContextStorage,
} from '@/domains/comic/onlineApi'

const { t } = useI18n()
const route = useRoute()
const router = useRouter()

const loading = ref(true)
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
let unmounted = false

function readQueryParam(keys: string[]): string {
  for (const k of keys) {
    const val = route.query[k]
    if (typeof val === 'string' && val.trim()) {
      return val.trim()
    }
  }
  return ''
}

function readStoredContext(): OnlineComicContextStorage | null {
  try {
    const raw = sessionStorage.getItem(ONLINE_COMIC_CTX_KEY)
    if (!raw) return null
    const parsed: unknown = JSON.parse(raw)
    if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) return null
    return parsed as OnlineComicContextStorage
  } catch {
    return null
  }
}

function hydrateParams(): { sourceId: string; comicUrl: string } {
  const qSourceId = readQueryParam(['sourceId', 'source_id'])
  const qComicUrl = readQueryParam(['comicUrl', 'comic_url'])
  const qSourceName = readQueryParam(['sourceName', 'source_name'])
  const qTitle = readQueryParam(['title', 'name'])
  const qAuthor = readQueryParam(['author'])
  const qCoverUrl = readQueryParam(['coverUrl', 'cover_url'])
  const qTocUrl = readQueryParam(['tocUrl', 'toc_url'])

  const stored = readStoredContext()
  const matchesStored =
    stored &&
    ((qSourceId && qComicUrl && stored.sourceId === qSourceId && stored.comicUrl === qComicUrl) ||
      (!qSourceId && !qComicUrl && Boolean(stored.sourceId && stored.comicUrl)))

  const sourceId = qSourceId || (matchesStored ? stored.sourceId : '')
  const comicUrl = qComicUrl || (matchesStored ? stored.comicUrl : '')

  fallbackCtx.value = {
    sourceId,
    comicUrl,
    sourceName: qSourceName || (matchesStored ? stored.sourceName : undefined),
    title: qTitle || (matchesStored ? stored.title : undefined),
    author: qAuthor || (matchesStored ? stored.author : undefined),
    coverUrl: qCoverUrl || (matchesStored ? stored.coverUrl : undefined),
    tocUrl: qTocUrl || (matchesStored ? stored.tocUrl : undefined),
    intro: matchesStored ? stored.intro : undefined,
    readingDirection: matchesStored ? stored.readingDirection : undefined,
  }

  resolvedSourceId.value = sourceId
  resolvedSourceName.value = fallbackCtx.value.sourceName || ''
  resolvedComicUrl.value = comicUrl

  return { sourceId, comicUrl }
}

async function loadDetail(): Promise<void> {
  const { sourceId, comicUrl } = hydrateParams()
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
    const res = await onlineComicApi.fetchDetail(sourceId, comicUrl, {
      title: fallbackCtx.value?.title,
      author: fallbackCtx.value?.author,
      comic_url: comicUrl,
      cover_url: fallbackCtx.value?.coverUrl ?? null,
      intro: fallbackCtx.value?.intro ?? null,
    })
    if (unmounted || seq !== loadSeq) return

    let loadedChapters = res.chapters ?? []
    if (loadedChapters.length === 0 && res.detail.toc_url?.trim()) {
      const tocRes = await onlineComicApi.fetchChapters(sourceId, res.detail.toc_url, res.detail)
      if (unmounted || seq !== loadSeq) return
      loadedChapters = tocRes.chapters ?? []
    }

    detail.value = res.detail
    chapters.value = loadedChapters
    resolvedSourceName.value = res.source_name || resolvedSourceName.value

    const nextStored: OnlineComicContextStorage = {
      sourceId,
      sourceName: resolvedSourceName.value,
      comicUrl: res.detail.comic_url || comicUrl,
      tocUrl: res.detail.toc_url,
      title: res.detail.title || fallbackCtx.value?.title,
      author: res.detail.author || fallbackCtx.value?.author,
      coverUrl: res.detail.cover_url ?? fallbackCtx.value?.coverUrl ?? null,
      intro: res.detail.intro ?? fallbackCtx.value?.intro ?? null,
      readingDirection: res.detail.reading_direction,
    }
    try {
      sessionStorage.setItem(ONLINE_COMIC_CTX_KEY, JSON.stringify(nextStored))
    } catch {
      // ignore storage errors
    }
  } catch (err) {
    if (unmounted || seq !== loadSeq) return
    errorMessage.value = err instanceof Error ? err.message : String(err)
  } finally {
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
})

watch(
  () => [route.query.sourceId, route.query.source_id, route.query.comicUrl, route.query.comic_url],
  () => {
    void loadDetail()
  },
)

const displayTitle = computed(
  () => detail.value?.title || fallbackCtx.value?.title || resolvedComicUrl.value || 'Comic',
)

const displayAuthor = computed(
  () => detail.value?.author || fallbackCtx.value?.author || t('online.unknownAuthor'),
)

const displaySummary = computed(
  () => detail.value?.intro || fallbackCtx.value?.intro || t('series.summaryEmpty'),
)

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

const directionLabel = computed(() => {
  if (displayDirection.value === 'ltr') return t('series.directionLtr')
  if (displayDirection.value === 'webtoon') return t('series.directionWebtoon')
  return t('series.directionRtl')
})

const kindTags = computed(() => {
  const raw = detail.value?.kind
  if (!raw) return []
  return raw
    .split(/[,，|/]/)
    .map((s) => s.trim())
    .filter(Boolean)
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

function openChapter(ch: OnlineComicChapter): void {
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
  const sourceName = resolvedSourceName.value
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
  try {
    sessionStorage.setItem(ONLINE_COMIC_READER_CTX_KEY, JSON.stringify(readerCtx))
  } catch {
    // ignore storage errors
  }

  const query: Record<string, string> = {
    sourceId: resolvedSourceId.value,
    comicUrl,
    chapterUrl: ch.url,
    chapterIndex: String(chapterIndex),
    readingDirection,
  }
  if (tocUrl) query.tocUrl = tocUrl
  if (title) query.title = title
  if (sourceName) query.sourceName = sourceName

  void router.push({ path: '/comic/online/read', query })
}

function startReadingFirst(): void {
  const first = readableChapters.value[0] ?? chapters.value[0]
  if (first) {
    openChapter(first)
  }
}

function backToOnlineSearch(): void {
  void router.push({ path: '/comic/series', query: { scope: 'online' } })
}
</script>

<template>
  <div class="mx-auto max-w-6xl space-y-8 p-6 sm:p-8">
    <!-- Top Back Bar -->
    <div class="flex items-center justify-between gap-3">
      <button
        type="button"
        class="md-btn md-btn-sm md-btn-tonal md-state"
        @click="backToOnlineSearch"
      >
        <LuChevronLeft class="h-4 w-4" />
        <span>{{ t('online.backToSearch') }}</span>
      </button>

      <RefreshButton
        v-if="resolvedSourceId && resolvedComicUrl"
        size="md"
        :action="loadDetail"
        :busy="loading"
      />
    </div>

    <!-- Error State (invalid params or fetch failed) -->
    <div
      v-if="errorMessage && !loading"
      class="md-card-filled flex flex-col items-center justify-center space-y-4 rounded-3xl p-10 text-center"
    >
      <p class="text-sm font-semibold text-[var(--md-sys-color-error)]">
        {{ t('online.detailLoadFailed') }}
      </p>
      <p class="max-w-md text-xs text-[var(--md-sys-color-on-surface-variant)]">
        {{ errorMessage }}
      </p>
      <div class="flex flex-wrap items-center justify-center gap-2.5">
        <button
          v-if="resolvedSourceId && resolvedComicUrl"
          type="button"
          class="md-btn md-btn-sm md-btn-filled md-state"
          @click="loadDetail"
        >
          {{ t('online.retry') }}
        </button>
        <button
          type="button"
          class="md-btn md-btn-sm md-btn-tonal md-state"
          @click="backToOnlineSearch"
        >
          {{ t('online.backToOnlineList') }}
        </button>
      </div>
    </div>

    <!-- Loading Skeleton -->
    <div
      v-else-if="loading && !detail"
      class="md-card-filled flex flex-col items-center justify-center rounded-3xl py-20 text-xs text-[var(--md-sys-color-on-surface-variant)]"
    >
      <div
        class="mb-3 h-7 w-7 animate-spin rounded-full border-2 border-[var(--md-sys-color-primary)] border-t-transparent"
      />
      <span>{{ t('online.loadingDetail') }}</span>
    </div>

    <!-- Hero & Chapters Content -->
    <template v-else>
      <section
        class="md-card-filled flex flex-col gap-8 rounded-3xl border border-[var(--md-sys-color-outline-variant)]/50 p-6 shadow-[var(--md-sys-elevation-1)] md:flex-row md:p-8"
      >
        <!-- Cover -->
        <div class="mx-auto w-48 shrink-0 md:mx-0 md:w-56">
          <div class="book-shadow overflow-hidden rounded-2xl">
            <MediaCoverImage
              :src="displayCoverUrl"
              :alt="displayTitle"
              aspect-class="aspect-[2/3]"
              rounded="rounded-2xl"
              :fallback-icon="LuBookOpen"
              fallback-icon-class="h-12 w-12 text-[var(--md-sys-color-on-surface-variant)]/40"
            />
          </div>
        </div>

        <!-- Metadata & Actions -->
        <div class="flex min-w-0 flex-1 flex-col justify-between space-y-5">
          <div class="space-y-3.5">
            <!-- Meta Badges -->
            <div class="flex flex-wrap items-center gap-2 text-xs">
              <span
                v-if="resolvedSourceName"
                class="rounded-full bg-[var(--md-sys-color-primary-container)] px-3 py-1 font-semibold text-[var(--md-sys-color-on-primary-container)]"
              >
                {{ t('online.sourceLabel') }}: {{ resolvedSourceName }}
              </span>
              <span
                v-if="detail?.status"
                class="rounded-full bg-[var(--md-sys-color-secondary-container)] px-3 py-1 font-semibold text-[var(--md-sys-color-on-secondary-container)]"
              >
                {{ detail.status }}
              </span>
              <span
                class="rounded-full bg-[var(--md-sys-color-surface-container-high)] px-3 py-1 font-medium text-[var(--md-sys-color-on-surface-variant)]"
              >
                {{ directionLabel }}
              </span>
              <span
                v-if="detail?.word_count"
                class="rounded-full bg-[var(--md-sys-color-surface-container-high)] px-3 py-1 font-medium text-[var(--md-sys-color-on-surface-variant)]"
              >
                {{ detail.word_count }}
              </span>
            </div>

            <div>
              <h1
                class="text-2xl font-extrabold tracking-tight text-[var(--md-sys-color-on-surface)] sm:text-3xl"
              >
                {{ displayTitle }}
              </h1>
              <p class="mt-1 text-xs text-[var(--md-sys-color-on-surface-variant)]">
                {{ t('series.writers') }}: {{ displayAuthor }}
                <span v-if="detail?.last_chapter">
                  · {{ t('online.latestChapter', { chapter: detail.last_chapter }) }}
                </span>
              </p>
            </div>

            <p
              class="text-sm leading-relaxed whitespace-pre-line text-[var(--md-sys-color-on-surface-variant)]"
            >
              {{ displaySummary }}
            </p>

            <div v-if="kindTags.length > 0" class="flex flex-wrap gap-1.5 pt-1">
              <span
                v-for="tag in kindTags"
                :key="tag"
                class="md-chip !h-7 !text-xs"
              >
                {{ tag }}
              </span>
            </div>
          </div>

          <div class="flex flex-wrap items-center gap-2.5 pt-2">
            <button
              type="button"
              :disabled="readableChapters.length === 0"
              class="md-btn md-btn-filled md-state !h-11 !px-6 text-sm shadow-[var(--md-sys-elevation-1)]"
              @click="startReadingFirst"
            >
              <LuPlay class="h-4 w-4 fill-current" />
              <span>{{ t('series.startReading') }}</span>
            </button>
          </div>
        </div>
      </section>

      <!-- Chapters Section -->
      <section class="space-y-4">
        <div class="flex flex-wrap items-center justify-between gap-3">
          <div class="flex items-center gap-3">
            <h2 class="text-lg font-bold text-[var(--md-sys-color-on-surface)]">
              {{ t('series.chaptersHeader') }}
            </h2>
            <span
              class="rounded-full bg-[var(--md-sys-color-surface-container-high)] px-2.5 py-0.5 text-xs font-semibold text-[var(--md-sys-color-on-surface-variant)]"
            >
              {{ t('online.chaptersCount', { count: chapters.length }) }}
            </span>
          </div>

          <div class="flex flex-wrap items-center gap-2.5">
            <div v-if="chapters.length > 10" class="relative w-48">
              <LuSearch
                class="pointer-events-none absolute top-1/2 left-3 h-3.5 w-3.5 -translate-y-1/2 text-[var(--md-sys-color-on-surface-variant)]"
              />
              <input
                v-model="chapterQuery"
                type="text"
                :placeholder="t('online.filterChaptersPlaceholder')"
                class="md-input h-8 w-full !pl-8 text-xs"
              />
            </div>

            <button
              type="button"
              class="md-btn md-btn-sm md-btn-outlined md-state"
              @click="orderAsc = !orderAsc"
            >
              <LuArrowUpDown class="h-3.5 w-3.5" />
              <span>{{ orderAsc ? t('series.orderAsc') : t('series.orderDesc') }}</span>
            </button>
          </div>
        </div>

        <!-- Empty Chapters Notice -->
        <div
          v-if="!loading && chapters.length === 0"
          class="md-card-outlined rounded-3xl border-dashed py-16 text-center text-sm text-[var(--md-sys-color-on-surface-variant)]"
        >
          {{ t('online.emptyChapters') }}
        </div>

        <!-- Chapter Rows -->
        <div
          v-else
          class="md-card-filled divide-y divide-[var(--md-sys-color-outline-variant)]/45 overflow-hidden border border-[var(--md-sys-color-outline-variant)]/50"
        >
          <button
            v-for="ch in displayedChapters"
            :key="`${ch.index}-${ch.url}`"
            type="button"
            :disabled="ch.is_volume || !ch.url"
            class="md-state group flex w-full items-center justify-between gap-4 px-5 py-3.5 text-left transition hover:bg-[var(--md-sys-color-surface-container)] disabled:cursor-default disabled:opacity-65"
            @click="openChapter(ch)"
          >
            <div class="flex min-w-0 flex-1 items-center gap-2.5">
              <span
                class="shrink-0 rounded-md bg-[var(--md-sys-color-surface-container-high)] px-2 py-0.5 text-xs font-semibold text-[var(--md-sys-color-on-surface)]"
              >
                {{ ch.is_volume ? t('online.volumeBadge') : `#${ch.index + 1}` }}
              </span>
              <span
                class="truncate text-sm font-semibold text-[var(--md-sys-color-on-surface)] group-hover:text-[var(--md-sys-color-primary)]"
              >
                {{ ch.title }}
              </span>
            </div>

            <div class="flex shrink-0 items-center gap-3 text-xs text-[var(--md-sys-color-on-surface-variant)]">
              <span v-if="ch.update_time">{{ ch.update_time }}</span>
              <span
                v-if="!ch.is_volume && ch.url"
                class="font-semibold text-[var(--md-sys-color-primary)] opacity-80 group-hover:opacity-100"
              >
                {{ t('online.openChapter') }}
              </span>
            </div>
          </button>
        </div>
      </section>
    </template>
  </div>
</template>
