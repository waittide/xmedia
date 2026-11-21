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
import { withAuthToken } from '@/api/client'
import OnlineSourceDebugShell, {
  type DebugResultTabItem,
} from '@/components/online-sources/OnlineSourceDebugShell.vue'
import { useOnlineSourceDebug } from '@/components/online-sources/useOnlineSourceDebug'
import type { SegmentedOption } from '@/components/ui/MdSegmentedButton.vue'
import { useSnackbarStore } from '@/stores/snackbar'
import {
  buildOnlineComicImageUrl,
  onlineComicApi,
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
  defaultKeyword: '海贼王',
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
    queryInput.value = props.source.check_keyword?.trim() || '海贼王'
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
  const kw = props.source?.check_keyword?.trim() || '海贼王'
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
  if (proxyUrl) return withAuthToken(proxyUrl)
  return buildOnlineComicImageUrl(rawUrl, props.source?.id)
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
      <button
        v-if="source?.check_keyword && (mode === 'full' || mode === 'search')"
        type="button"
        class="md-state rounded-full bg-[var(--md-sys-color-surface-container-high)] px-2.5 py-0.5 text-[var(--md-sys-color-on-surface)]"
        @click="queryInput = source.check_keyword!"
      >
        {{ t('onlineSources.debug.checkKeywordHint', { keyword: source.check_keyword }) }}
      </button>
      <button
        v-if="lastExploreUrl && mode === 'explore'"
        type="button"
        class="md-state max-w-xs truncate rounded-full bg-[var(--md-sys-color-surface-container-high)] px-2.5 py-0.5 font-mono text-[var(--md-sys-color-on-surface)]"
        @click="queryInput = lastExploreUrl"
      >
        {{ t('onlineSources.debug.lastExploreHint') }}
      </button>
      <button
        v-if="lastComicUrl && mode === 'detail'"
        type="button"
        class="md-state max-w-xs truncate rounded-full bg-[var(--md-sys-color-surface-container-high)] px-2.5 py-0.5 font-mono text-[var(--md-sys-color-on-surface)]"
        @click="queryInput = lastComicUrl"
      >
        {{ t('onlineSources.debug.lastDetailHint') }}
      </button>
      <button
        v-if="lastTocUrl && mode === 'chapters'"
        type="button"
        class="md-state max-w-xs truncate rounded-full bg-[var(--md-sys-color-surface-container-high)] px-2.5 py-0.5 font-mono text-[var(--md-sys-color-on-surface)]"
        @click="queryInput = lastTocUrl"
      >
        {{ t('onlineSources.debug.lastTocHint') }}
      </button>
      <button
        v-if="lastChapterUrl && mode === 'pages'"
        type="button"
        class="md-state max-w-xs truncate rounded-full bg-[var(--md-sys-color-surface-container-high)] px-2.5 py-0.5 font-mono text-[var(--md-sys-color-on-surface)]"
        @click="queryInput = lastChapterUrl"
      >
        {{ t('onlineSources.debug.lastChapterHint') }}
      </button>
    </template>

    <template #tab-content>
      <!-- Search Results -->
      <div v-if="activeTab === 'search' && report" class="flex flex-col gap-2.5">
        <div
          v-if="report.search_items.length === 0"
          class="rounded-2xl bg-[var(--md-sys-color-surface-container)] py-10 text-center text-xs text-[var(--md-sys-color-on-surface-variant)]"
        >
          {{ t('onlineSources.debug.emptySearch') }}
        </div>
        <template v-else>
          <p class="text-xs text-[var(--md-sys-color-on-surface-variant)]">
            {{
              t('onlineSources.debug.searchSummary', {
                total: report.result_count,
                shown: report.search_items.length,
              })
            }}
          </p>
          <div
            v-for="(item, idx) in report.search_items"
            :key="`${item.comic_url}-${idx}`"
            class="flex items-start gap-3 rounded-2xl bg-[var(--md-sys-color-surface-container)] p-3.5"
          >
            <div class="h-20 w-14 shrink-0 overflow-hidden rounded-lg bg-[var(--md-sys-color-surface-container-high)]">
              <img
                v-if="item.cover_url"
                :src="buildOnlineComicImageUrl(item.cover_url, source?.id)"
                :alt="item.title"
                class="h-full w-full object-cover"
                loading="lazy"
              />
              <div
                v-else
                class="flex h-full w-full items-center justify-center text-[10px] text-[var(--md-sys-color-on-surface-variant)]"
              >
                {{ t('onlineSources.debug.noCover') }}
              </div>
            </div>

            <div class="min-w-0 flex-1 space-y-1">
              <div class="flex flex-wrap items-start justify-between gap-2">
                <div class="flex flex-wrap items-center gap-1.5">
                  <span class="font-mono text-[11px] text-[var(--md-sys-color-on-surface-variant)]">
                    #{{ idx + 1 }}
                  </span>
                  <span class="text-sm font-semibold text-[var(--md-sys-color-on-surface)]">
                    {{ item.title }}
                  </span>
                  <span v-if="item.author" class="text-xs text-[var(--md-sys-color-on-surface-variant)]">
                    · {{ item.author }}
                  </span>
                </div>
                <button
                  type="button"
                  :disabled="running"
                  class="md-btn md-btn-sm md-btn-tonal md-state shrink-0"
                  @click="handleDebugComicItem(item)"
                >
                  <span>{{ t('onlineSources.debug.debugDetailBtn') }}</span>
                  <LuArrowRight class="h-3.5 w-3.5" />
                </button>
              </div>
              <p v-if="item.intro" class="line-clamp-2 text-xs text-[var(--md-sys-color-on-surface-variant)]">
                {{ item.intro }}
              </p>
              <p class="break-all font-mono text-[11px] text-[var(--md-sys-color-on-surface-variant)]/85">
                {{ item.comic_url }}
              </p>
            </div>
          </div>
        </template>
      </div>

      <!-- Detail -->
      <div v-else-if="activeTab === 'detail' && report" class="flex flex-col gap-3">
        <div
          v-if="!report.comic_detail"
          class="rounded-2xl bg-[var(--md-sys-color-surface-container)] py-10 text-center text-xs text-[var(--md-sys-color-on-surface-variant)]"
        >
          {{ t('onlineSources.debug.emptyDetail') }}
        </div>
        <template v-else>
          <div class="flex flex-wrap items-center justify-between gap-2">
            <span class="text-xs font-medium text-[var(--md-sys-color-on-surface-variant)]">
              {{ report.comic_detail.title }}
            </span>
            <button
              type="button"
              :disabled="running || !report.comic_detail.toc_url"
              class="md-btn md-btn-sm md-btn-tonal md-state"
              @click="handleDebugChaptersFromDetail"
            >
              <span>{{ t('onlineSources.debug.debugChaptersBtn') }}</span>
              <LuArrowRight class="h-3.5 w-3.5" />
            </button>
          </div>
          <div class="overflow-hidden rounded-2xl bg-[var(--md-sys-color-surface-container)]">
            <div
              v-for="(field, fIdx) in detailFields"
              :key="field.label"
              class="grid grid-cols-1 gap-1 px-4 py-3 sm:grid-cols-[11rem_1fr] sm:gap-3"
              :class="fIdx > 0 ? 'border-t border-[var(--md-sys-color-outline-variant)]/40' : ''"
            >
              <div class="text-xs font-medium text-[var(--md-sys-color-on-surface-variant)]">
                {{ field.label }}
              </div>
              <div
                class="min-w-0 text-xs"
                :class="[
                  field.value
                    ? 'text-[var(--md-sys-color-on-surface)]'
                    : 'italic text-[var(--md-sys-color-on-surface-variant)]/60',
                  field.mono ? 'break-all font-mono' : '',
                  field.multiline ? 'leading-relaxed whitespace-pre-wrap' : 'break-words',
                ]"
              >
                {{ field.value || t('onlineSources.debug.emptyValue') }}
              </div>
            </div>
          </div>
        </template>
      </div>

      <!-- Chapters -->
      <div v-else-if="activeTab === 'chapters' && report" class="flex flex-col gap-3">
        <div
          v-if="report.chapters.length === 0"
          class="rounded-2xl bg-[var(--md-sys-color-surface-container)] py-10 text-center text-xs text-[var(--md-sys-color-on-surface-variant)]"
        >
          {{ t('onlineSources.debug.emptyChapters') }}
        </div>
        <template v-else>
          <div class="flex flex-wrap items-center justify-between gap-2">
            <span class="text-xs text-[var(--md-sys-color-on-surface-variant)]">
              {{ report.chapters_total_count }}
            </span>
            <input
              v-model="chaptersFilter"
              type="text"
              :placeholder="t('onlineSources.debug.filterChaptersPlaceholder')"
              class="md-input w-56 py-1.5 text-xs"
            />
          </div>
          <div class="divide-y divide-[var(--md-sys-color-outline-variant)]/40 overflow-hidden rounded-2xl bg-[var(--md-sys-color-surface-container)]">
            <div
              v-for="ch in filteredChapters"
              :key="`${ch.index}-${ch.url}`"
              class="flex items-center justify-between gap-3 px-4 py-2.5"
            >
              <div class="min-w-0 flex-1">
                <div class="flex items-center gap-2">
                  <span class="w-10 shrink-0 font-mono text-[11px] text-[var(--md-sys-color-on-surface-variant)]">
                    #{{ ch.index + 1 }}
                  </span>
                  <span
                    v-if="ch.is_volume"
                    class="rounded-full bg-[var(--md-sys-color-tertiary-container)] px-2 py-0.5 text-[10px] font-medium text-[var(--md-sys-color-on-tertiary-container)]"
                  >
                    {{ t('onlineSources.debug.volumeBadge') }}
                  </span>
                  <span class="truncate text-xs font-medium text-[var(--md-sys-color-on-surface)]">
                    {{ ch.title }}
                  </span>
                </div>
                <p class="mt-0.5 truncate pl-12 font-mono text-[10px] text-[var(--md-sys-color-on-surface-variant)]">
                  {{ ch.url || t('onlineSources.debug.noUrl') }}
                </p>
              </div>
              <button
                v-if="ch.url && !ch.is_volume"
                type="button"
                :disabled="running"
                class="md-btn md-btn-sm md-btn-outlined md-state shrink-0"
                @click="handleDebugChapterPages(ch)"
              >
                <span>{{ t('onlineSources.debug.debugPagesBtn') }}</span>
              </button>
            </div>
          </div>
        </template>
      </div>

      <!-- Pages Preview -->
      <div v-else-if="activeTab === 'pages' && report" class="flex flex-col gap-3">
        <div
          v-if="report.pages.length === 0"
          class="rounded-2xl bg-[var(--md-sys-color-surface-container)] py-10 text-center text-xs text-[var(--md-sys-color-on-surface-variant)]"
        >
          {{ t('onlineSources.debug.emptyPages') }}
        </div>
        <template v-else>
          <div class="flex flex-wrap items-center justify-between gap-2 rounded-2xl bg-[var(--md-sys-color-surface-container)] px-4 py-3 text-xs">
            <span class="font-semibold text-[var(--md-sys-color-on-surface)]">
              {{ report.chapter_title || `#${(report.chapter_index ?? 0) + 1}` }}
            </span>
            <span class="text-[var(--md-sys-color-on-surface-variant)]">
              {{ report.page_count }}
            </span>
          </div>
          <div class="grid grid-cols-2 gap-3 sm:grid-cols-3">
            <div
              v-for="page in report.pages"
              :key="page.index"
              class="overflow-hidden rounded-2xl bg-[var(--md-sys-color-surface-container)] p-2"
            >
              <img
                :src="pagePreviewSrc(page.proxy_url, page.image_url)"
                :alt="`Page ${page.index + 1}`"
                class="aspect-[3/4] w-full rounded-xl bg-[var(--md-sys-color-surface-container-high)] object-contain"
                loading="lazy"
              />
              <p class="mt-1.5 truncate font-mono text-[10px] text-[var(--md-sys-color-on-surface-variant)]">
                #{{ page.index + 1 }} · {{ page.image_url }}
              </p>
            </div>
          </div>
        </template>
      </div>
    </template>
  </OnlineSourceDebugShell>
</template>
