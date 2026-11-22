<script setup lang="ts">
import { computed, ref, toRef } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  LuArrowRight,
  LuBookOpen,
  LuImage,
  LuList,
  LuSearch,
  LuTerminal,
} from 'vue-icons-plus/lu'
import OnlineSourceDebugShell, {
  type DebugResultTabItem,
} from '@/components/online-sources/OnlineSourceDebugShell.vue'
import { useOnlineSourceDebug } from '@/components/online-sources/useOnlineSourceDebug'
import {
  MdButton,
  MdCard,
  MdChip,
  MdTextField,
  MediaCoverImage,
  type SegmentedOption,
} from '@/components/ui'
import { useComicI18n } from '@/domains/comic/i18n'
import { useSnackbarStore } from '@/stores/snackbar'
import {
  buildOnlineComicImageUrl,
  onlineComicApi,
  resolveOnlineComicCoverUrl,
  type ComicSearchResult,
  type ComicSourceSummaryDto,
  type ComicSourceTestInput,
  type ComicSourceTestMode,
  type ComicSourceTestReportDto,
  type OnlineComicChapter,
} from '../onlineApi'

const props = defineProps<{
  open: boolean
  source: ComicSourceSummaryDto | null
}>()

const emit = defineEmits<{
  close: []
  tested: []
}>()

const { t } = useI18n()
const { t: comicT } = useComicI18n()
const snackbar = useSnackbarStore()

type ResultTab = 'timeline' | 'search' | 'detail' | 'chapters' | 'pages'

const modeOptions = computed<readonly SegmentedOption<ComicSourceTestMode>[]>(() => [
  { value: 'full', label: t('onlineSources.debug.modeFull') },
  { value: 'search', label: t('onlineSources.debug.modeSearch') },
  { value: 'explore', label: t('onlineSources.debug.modeExplore') },
  { value: 'detail', label: t('onlineSources.debug.modeDetail') },
  { value: 'chapters', label: t('onlineSources.debug.modeChapters') },
  { value: 'pages', label: t('onlineSources.debug.modePages') },
])

const defaultKeyword = computed(() => comicT('online.defaultDebugKeyword'))

const chaptersFilter = ref('')
const lastComicUrl = ref('')
const lastTocUrl = ref('')
const lastChapterUrl = ref('')
const lastExploreUrl = ref('')
const pendingPayload = ref<ComicSourceTestInput | null>(null)
const pendingPreserve = ref(false)

const {
  mode,
  queryInput,
  running,
  requestError,
  report,
  activeTab,
  abortActiveRun,
  executeTest,
} = useOnlineSourceDebug<
  ComicSourceSummaryDto,
  ComicSourceTestMode,
  ComicSourceTestReportDto,
  ResultTab
>({
  open: toRef(props, 'open'),
  source: toRef(props, 'source'),
  defaultMode: 'full',
  defaultTab: 'timeline',
  defaultKeyword: defaultKeyword.value,
  runTest: async (sourceId, signal) => {
    const val = queryInput.value.trim()
    const payload: ComicSourceTestInput = pendingPayload.value ?? { mode: mode.value }
    if (!pendingPayload.value) {
      if (mode.value === 'full' || mode.value === 'search') {
        payload.keyword = val || null
      } else if (mode.value === 'explore') {
        payload.explore_url = val || null
      } else if (mode.value === 'detail') {
        payload.comic_url = val || null
      } else if (mode.value === 'chapters') {
        payload.toc_url = val || null
      } else if (mode.value === 'pages') {
        payload.chapter_url = val || null
      }
    }
    return onlineComicApi.testSource(sourceId, payload, { signal })
  },
  onReset: () => {
    chaptersFilter.value = ''
    lastComicUrl.value = ''
    lastTocUrl.value = ''
    lastChapterUrl.value = ''
    lastExploreUrl.value = ''
    pendingPayload.value = null
    pendingPreserve.value = false
  },
  onReportReady: (res, preferredTab) => {
    const prev = pendingPreserve.value ? report.value : null
    const prevComicUrl = prev?.comic_detail?.comic_url || lastComicUrl.value
    const ranStages = new Set(res.steps.map((s) => s.stage))
    const nextComicUrl = res.comic_detail?.comic_url || pendingPayload.value?.comic_url?.trim() || ''
    const comicUrlChanged = Boolean(nextComicUrl && prevComicUrl && nextComicUrl !== prevComicUrl)

    const merged: ComicSourceTestReportDto = prev
      ? {
          ...res,
          result_count: ranStages.has('search') ? res.result_count : prev.result_count,
          search_items: ranStages.has('search') ? res.search_items : prev.search_items,
          sample_titles: ranStages.has('search') ? res.sample_titles : prev.sample_titles,
          explore_categories: ranStages.has('explore')
            ? res.explore_categories
            : prev.explore_categories,
          comic_detail: ranStages.has('detail') ? res.comic_detail : prev.comic_detail,
          chapters: ranStages.has('chapters')
            ? res.chapters
            : comicUrlChanged
              ? []
              : prev.chapters,
          chapters_total_count: ranStages.has('chapters')
            ? res.chapters_total_count
            : comicUrlChanged
              ? 0
              : prev.chapters_total_count,
          chapter_index: ranStages.has('pages')
            ? res.chapter_index
            : comicUrlChanged
              ? null
              : prev.chapter_index,
          chapter_title: ranStages.has('pages')
            ? res.chapter_title
            : comicUrlChanged
              ? null
              : prev.chapter_title,
          chapter_url: ranStages.has('pages')
            ? res.chapter_url
            : comicUrlChanged
              ? null
              : prev.chapter_url,
          pages: ranStages.has('pages') ? res.pages : comicUrlChanged ? [] : prev.pages,
          page_count: ranStages.has('pages') ? res.page_count : comicUrlChanged ? 0 : prev.page_count,
        }
      : res

    report.value = merged
    if (comicUrlChanged) {
      lastTocUrl.value = merged.comic_detail?.toc_url || ''
      lastChapterUrl.value = merged.chapter_url || merged.chapters[0]?.url || ''
    } else {
      if (merged.comic_detail?.toc_url) lastTocUrl.value = merged.comic_detail.toc_url
      if (merged.chapter_url) lastChapterUrl.value = merged.chapter_url
      else if (merged.chapters[0]?.url) lastChapterUrl.value = merged.chapters[0].url
    }
    if (merged.comic_detail?.comic_url) lastComicUrl.value = merged.comic_detail.comic_url
    else if (merged.search_items[0]?.comic_url) lastComicUrl.value = merged.search_items[0].comic_url
    if (merged.explore_categories[0]?.url) lastExploreUrl.value = merged.explore_categories[0].url

    if (preferredTab) {
      activeTab.value = preferredTab
    } else if (res.mode === 'search' || res.mode === 'explore') {
      activeTab.value = res.search_items.length > 0 ? 'search' : 'timeline'
    } else if (res.mode === 'detail') {
      activeTab.value = res.comic_detail ? 'detail' : 'timeline'
    } else if (res.mode === 'chapters') {
      activeTab.value = res.chapters.length > 0 ? 'chapters' : 'timeline'
    } else if (res.mode === 'pages') {
      activeTab.value = res.pages.length > 0 ? 'pages' : 'timeline'
    } else {
      activeTab.value = 'timeline'
    }
  },
  onTested: () => emit('tested'),
})

function handleModeChange(nextMode: ComicSourceTestMode): void {
  mode.value = nextMode
  if (nextMode === 'detail' && lastComicUrl.value) {
    queryInput.value = lastComicUrl.value
  } else if (nextMode === 'chapters' && lastTocUrl.value) {
    queryInput.value = lastTocUrl.value
  } else if (nextMode === 'pages' && lastChapterUrl.value) {
    queryInput.value = lastChapterUrl.value
  } else if (nextMode === 'explore' && lastExploreUrl.value) {
    queryInput.value = lastExploreUrl.value
  } else if (
    (nextMode === 'full' || nextMode === 'search') &&
    queryInput.value.trim().startsWith('http') &&
    props.source
  ) {
    queryInput.value = props.source.check_keyword?.trim() || defaultKeyword.value
  }
}

const hasQuickHint = computed(() => {
  if ((mode.value === 'full' || mode.value === 'search') && props.source?.check_keyword) return true
  if (mode.value === 'explore' && Boolean(lastExploreUrl.value)) return true
  if (mode.value === 'detail' && Boolean(lastComicUrl.value)) return true
  if (mode.value === 'chapters' && Boolean(lastTocUrl.value)) return true
  if (mode.value === 'pages' && Boolean(lastChapterUrl.value)) return true
  return false
})

const inputPlaceholder = computed(() => {
  const kw = props.source?.check_keyword?.trim() || defaultKeyword.value
  if (mode.value === 'full' || mode.value === 'search') {
    return t('onlineSources.debug.keywordPlaceholder', { keyword: kw })
  }
  return t('onlineSources.debug.urlPlaceholder')
})

const domainLabel = computed(() => t('onlineSources.domainLabel.comic'))

const resultTabs = computed<DebugResultTabItem<ResultTab>[]>(() => [
  { id: 'timeline', label: t('onlineSources.debug.tabTimeline'), icon: LuTerminal },
  {
    id: 'search',
    label: t('onlineSources.debug.tabSearch', { count: report.value?.result_count ?? 0 }),
    icon: LuSearch,
  },
  { id: 'detail', label: t('onlineSources.debug.tabDetail'), icon: LuBookOpen },
  {
    id: 'chapters',
    label: t('onlineSources.debug.tabChapters', { count: report.value?.chapters_total_count ?? 0 }),
    icon: LuList,
  },
  {
    id: 'pages',
    label: t('onlineSources.debug.tabPages', { count: report.value?.page_count ?? 0 }),
    icon: LuImage,
  },
])

function stageToTab(stage: string): ResultTab {
  if (stage === 'search' || stage === 'explore') return 'search'
  if (stage === 'detail') return 'detail'
  if (stage === 'chapters') return 'chapters'
  if (stage === 'pages') return 'pages'
  return 'timeline'
}

const filteredChapters = computed(() => {
  const list = report.value?.chapters ?? []
  const q = chaptersFilter.value.trim().toLowerCase()
  if (!q) return list
  return list.filter(
    (ch) =>
      ch.title.toLowerCase().includes(q) ||
      String(ch.index + 1).includes(q) ||
      ch.url.toLowerCase().includes(q),
  )
})

const detailFields = computed(() => {
  const d = report.value?.comic_detail
  if (!d) return []
  return [
    { label: 'title', value: d.title },
    { label: 'author', value: d.author },
    { label: 'kind', value: d.kind ?? '' },
    { label: 'status', value: d.status ?? '' },
    { label: 'readingDirection', value: d.reading_direction ?? '' },
    { label: 'lastChapter', value: d.last_chapter ?? '' },
    { label: 'comicUrl', value: d.comic_url, mono: true },
    { label: 'tocUrl', value: d.toc_url, mono: true },
    { label: 'coverUrl', value: d.cover_url ?? '', mono: true },
    { label: 'intro', value: d.intro ?? '', multiline: true },
  ]
})

async function handleRun(): Promise<void> {
  pendingPayload.value = null
  pendingPreserve.value = mode.value !== 'full' && mode.value !== 'search'
  await executeTest()
}

async function handleDebugComicItem(item: ComicSearchResult): Promise<void> {
  if (item.comic_url && item.comic_url !== lastComicUrl.value) {
    lastTocUrl.value = ''
    lastChapterUrl.value = ''
  }
  mode.value = 'full'
  queryInput.value = item.comic_url
  pendingPayload.value = { mode: 'full', comic_url: item.comic_url }
  pendingPreserve.value = true
  await executeTest('timeline')
}

async function handleDebugChaptersFromDetail(): Promise<void> {
  const tocUrl = report.value?.comic_detail?.toc_url || lastTocUrl.value
  if (!tocUrl) return
  mode.value = 'chapters'
  queryInput.value = tocUrl
  pendingPayload.value = { mode: 'chapters', toc_url: tocUrl }
  pendingPreserve.value = true
  await executeTest('chapters')
}

async function handleDebugChapterPages(ch: OnlineComicChapter): Promise<void> {
  if (!ch.url) return
  mode.value = 'pages'
  queryInput.value = ch.url
  lastChapterUrl.value = ch.url
  pendingPayload.value = {
    mode: 'pages',
    chapter_url: ch.url,
    chapter_index: ch.index,
    chapter_title: ch.title,
    comic_url: lastComicUrl.value || null,
    toc_url: lastTocUrl.value || null,
  }
  pendingPreserve.value = true
  await executeTest('pages')
}

function pagePreviewSrc(proxyUrl: string, rawUrl: string): string {
  return resolveOnlineComicCoverUrl(proxyUrl, rawUrl, props.source?.id)
}

async function copyDebugReport(): Promise<void> {
  if (!report.value || !props.source) return
  const r = report.value
  const lines = [
    `=== ${props.source.name} (${r.mode}) ===`,
    `URL: ${props.source.source_url}`,
    `Result: ${r.ok ? 'OK' : 'FAIL'} | Elapsed: ${r.total_elapsed_ms} ms`,
    ...r.steps.map(
      (s, i) => `${i + 1}. [${s.ok ? 'OK' : 'FAIL'}] ${s.label} (${s.elapsed_ms}ms) - ${s.summary}`,
    ),
  ]
  try {
    await navigator.clipboard.writeText(lines.join('\n'))
    snackbar.show({ message: t('onlineSources.debug.reportCopied') })
  } catch {
    snackbar.show({ message: t('onlineSources.debug.copyFailed'), tone: 'error' })
  }
}
</script>

<template>
  <OnlineSourceDebugShell
    :open="open"
    :fullscreen="true"
    :title="t('onlineSources.debug.title', { name: source?.name ?? '' })"
    :subtitle="source?.source_url"
    :domain-label="domainLabel"
    :mode="mode"
    :mode-options="modeOptions"
    :query-input="queryInput"
    :input-placeholder="inputPlaceholder"
    :running="running"
    :request-error="requestError"
    :report="report"
    :active-tab="activeTab"
    timeline-tab-id="timeline"
    :result-tabs="resultTabs"
    :has-quick-hint="hasQuickHint"
    :stage-to-tab="stageToTab"
    @close="emit('close')"
    @update:mode="handleModeChange"
    @update:query-input="queryInput = $event"
    @update:active-tab="activeTab = $event"
    @run="handleRun"
    @stop="abortActiveRun"
    @copy-report="copyDebugReport"
  >
    <template #quick-hints>
      <MdChip
        v-if="source?.check_keyword && (mode === 'full' || mode === 'search')"
        variant="suggestion"
        :label="t('onlineSources.debug.checkKeywordHint', { keyword: source.check_keyword })"
        @click="queryInput = source.check_keyword!"
      />
      <MdChip
        v-if="lastExploreUrl && mode === 'explore'"
        variant="suggestion"
        :label="t('onlineSources.debug.lastExploreHint')"
        @click="queryInput = lastExploreUrl"
      />
      <MdChip
        v-if="lastComicUrl && mode === 'detail'"
        variant="suggestion"
        :label="t('onlineSources.debug.lastDetailHint')"
        @click="queryInput = lastComicUrl"
      />
      <MdChip
        v-if="lastTocUrl && mode === 'chapters'"
        variant="suggestion"
        :label="t('onlineSources.debug.lastTocHint')"
        @click="queryInput = lastTocUrl"
      />
      <MdChip
        v-if="lastChapterUrl && mode === 'pages'"
        variant="suggestion"
        :label="t('onlineSources.debug.lastChapterHint')"
        @click="queryInput = lastChapterUrl"
      />
    </template>

    <template #tab-content>
      <!-- Search Results -->
      <div v-if="activeTab === 'search' && report" class="flex flex-col gap-3">
        <MdCard v-if="report.search_items.length === 0" variant="filled" padding="lg">
          <p class="text-body-small text-center text-on-surface-variant">
            {{ t('onlineSources.debug.emptySearch') }}
          </p>
        </MdCard>

        <template v-else>
          <p class="text-body-small text-on-surface-variant">
            {{
              t('onlineSources.debug.searchSummary', {
                total: report.result_count,
                shown: report.search_items.length,
              })
            }}
          </p>
          <MdCard
            v-for="(item, idx) in report.search_items"
            :key="`${item.comic_url}-${idx}`"
            variant="filled"
            padding="md"
          >
            <div class="flex items-start gap-3">
              <div class="h-20 w-14 shrink-0 overflow-hidden rounded-corner-sm bg-surface-container-high">
                <MediaCoverImage
                  :src="item.cover_url ? buildOnlineComicImageUrl(item.cover_url, source?.id) : null"
                  :alt="item.title"
                  aspect-class="aspect-[2/3]"
                  rounded="rounded-corner-sm"
                  :fallback-icon="LuBookOpen"
                  fallback-icon-class="h-5 w-5 text-on-surface-variant/50"
                />
              </div>

              <div class="min-w-0 flex-1 space-y-1">
                <div class="flex flex-wrap items-start justify-between gap-2">
                  <div class="flex flex-wrap items-center gap-1.5">
                    <span class="text-label-small font-mono tabular-nums text-on-surface-variant">
                      #{{ idx + 1 }}
                    </span>
                    <span class="text-title-small text-on-surface">
                      {{ item.title }}
                    </span>
                    <span v-if="item.author" class="text-body-small text-on-surface-variant">
                      · {{ item.author }}
                    </span>
                  </div>
                  <MdButton
                    variant="tonal"
                    size="sm"
                    :disabled="running"
                    class="shrink-0"
                    @click="handleDebugComicItem(item)"
                  >
                    <span>{{ t('onlineSources.debug.debugDetailBtn') }}</span>
                    <template #trailing>
                      <LuArrowRight class="h-3.5 w-3.5" aria-hidden="true" />
                    </template>
                  </MdButton>
                </div>
                <p v-if="item.intro" class="text-body-small line-clamp-2 text-on-surface-variant">
                  {{ item.intro }}
                </p>
                <p class="text-label-small break-all font-mono text-on-surface-variant/85">
                  {{ item.comic_url }}
                </p>
              </div>
            </div>
          </MdCard>
        </template>
      </div>

      <!-- Detail -->
      <div v-else-if="activeTab === 'detail' && report" class="flex flex-col gap-3">
        <MdCard v-if="!report.comic_detail" variant="filled" padding="lg">
          <p class="text-body-small text-center text-on-surface-variant">
            {{ t('onlineSources.debug.emptyDetail') }}
          </p>
        </MdCard>

        <template v-else>
          <div class="flex flex-wrap items-center justify-between gap-2">
            <span class="text-title-small text-on-surface">
              {{ report.comic_detail.title }}
            </span>
            <MdButton
              variant="tonal"
              size="sm"
              :disabled="running || !report.comic_detail.toc_url"
              @click="handleDebugChaptersFromDetail"
            >
              <span>{{ t('onlineSources.debug.debugChaptersBtn') }}</span>
              <template #trailing>
                <LuArrowRight class="h-3.5 w-3.5" aria-hidden="true" />
              </template>
            </MdButton>
          </div>

          <MdCard variant="filled" padding="none" class="overflow-hidden">
            <div
              v-for="(field, fIdx) in detailFields"
              :key="field.label"
              class="grid grid-cols-1 gap-1 px-4 py-3 sm:grid-cols-[11rem_1fr] sm:gap-3"
              :class="fIdx > 0 ? 'border-t border-outline-variant/50' : ''"
            >
              <div class="text-label-medium text-on-surface-variant">
                {{ field.label }}
              </div>
              <div
                class="text-body-small min-w-0"
                :class="[
                  field.value ? 'text-on-surface' : 'italic text-on-surface-variant/60',
                  field.mono ? 'break-all font-mono' : '',
                  field.multiline ? 'leading-relaxed whitespace-pre-wrap' : 'break-words',
                ]"
              >
                {{ field.value || t('onlineSources.debug.emptyValue') }}
              </div>
            </div>
          </MdCard>
        </template>
      </div>

      <!-- Chapters -->
      <div v-else-if="activeTab === 'chapters' && report" class="flex flex-col gap-3">
        <MdCard v-if="report.chapters.length === 0" variant="filled" padding="lg">
          <p class="text-body-small text-center text-on-surface-variant">
            {{ t('onlineSources.debug.emptyChapters') }}
          </p>
        </MdCard>

        <template v-else>
          <div class="flex flex-wrap items-center justify-between gap-2">
            <span class="text-body-small tabular-nums text-on-surface-variant">
              {{ report.chapters_total_count }}
            </span>
            <div class="w-56">
              <MdTextField
                v-model="chaptersFilter"
                type="search"
                size="sm"
                clearable
                :placeholder="t('onlineSources.debug.filterChaptersPlaceholder')"
              />
            </div>
          </div>

          <MdCard variant="filled" padding="none" class="overflow-hidden">
            <div class="divide-y divide-outline-variant/50">
              <div
                v-for="ch in filteredChapters"
                :key="`${ch.index}-${ch.url}`"
                class="flex items-center justify-between gap-3 px-4 py-2.5"
              >
                <div class="min-w-0 flex-1">
                  <div class="flex items-center gap-2">
                    <span class="text-label-small w-10 shrink-0 font-mono tabular-nums text-on-surface-variant">
                      #{{ ch.index + 1 }}
                    </span>
                    <span
                      v-if="ch.is_volume"
                      class="text-label-small rounded-corner-full bg-tertiary-container px-2 py-0.5 text-on-tertiary-container"
                    >
                      {{ t('onlineSources.debug.volumeBadge') }}
                    </span>
                    <span class="text-body-medium truncate text-on-surface">
                      {{ ch.title }}
                    </span>
                  </div>
                  <p class="text-label-small mt-0.5 truncate pl-12 font-mono text-on-surface-variant">
                    {{ ch.url || t('onlineSources.debug.noUrl') }}
                  </p>
                </div>

                <MdButton
                  v-if="ch.url && !ch.is_volume"
                  variant="outlined"
                  size="sm"
                  :disabled="running"
                  class="shrink-0"
                  @click="handleDebugChapterPages(ch)"
                >
                  {{ t('onlineSources.debug.debugPagesBtn') }}
                </MdButton>
              </div>
            </div>
          </MdCard>
        </template>
      </div>

      <!-- Pages Preview -->
      <div v-else-if="activeTab === 'pages' && report" class="flex flex-col gap-3">
        <MdCard v-if="report.pages.length === 0" variant="filled" padding="lg">
          <p class="text-body-small text-center text-on-surface-variant">
            {{ t('onlineSources.debug.emptyPages') }}
          </p>
        </MdCard>

        <template v-else>
          <MdCard variant="filled" padding="sm">
            <div class="flex flex-wrap items-center justify-between gap-2 px-1">
              <span class="text-title-small text-on-surface">
                {{ report.chapter_title || `#${(report.chapter_index ?? 0) + 1}` }}
              </span>
              <span class="text-body-small tabular-nums text-on-surface-variant">
                {{ report.page_count }}
              </span>
            </div>
          </MdCard>

          <div class="grid grid-cols-2 gap-3 sm:grid-cols-3">
            <MdCard
              v-for="page in report.pages"
              :key="page.index"
              variant="filled"
              padding="sm"
              class="overflow-hidden"
            >
              <img
                v-if="pagePreviewSrc(page.proxy_url, page.image_url)"
                :src="pagePreviewSrc(page.proxy_url, page.image_url)"
                :alt="`Page ${page.index + 1}`"
                class="aspect-[3/4] w-full rounded-corner-sm bg-surface-container-high object-contain"
                loading="lazy"
              />
              <p class="text-label-small mt-1.5 truncate font-mono text-on-surface-variant">
                #{{ page.index + 1 }} · {{ page.image_url }}
              </p>
            </MdCard>
          </div>
        </template>
      </div>
    </template>
  </OnlineSourceDebugShell>
</template>
