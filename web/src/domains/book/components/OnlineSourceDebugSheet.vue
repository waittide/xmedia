<script setup lang="ts">
import { computed, ref, toRef, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  LuArrowRight,
  LuBookOpen,
  LuCopy,
  LuFileText,
  LuList,
  LuSearch,
  LuTerminal,
} from 'vue-icons-plus/lu'
import OnlineSourceDebugShell, {
  type DebugResultTabItem,
} from '@/components/online-sources/OnlineSourceDebugShell.vue'
import { useOnlineSourceDebug } from '@/components/online-sources/useOnlineSourceDebug'
import { MdButton, MdChip, MdTextField, type SegmentedOption } from '@/components/ui'
import MediaCoverImage from '@/components/ui/MediaCoverImage.vue'
import SourceDebugFieldsCard from '@/domains/shared/source-debug/SourceDebugFieldsCard.vue'
import {
  copyDebugTextToClipboard,
  copySourceDebugReport,
  type SourceDebugFieldRow,
} from '@/domains/shared/source-debug/useSourceDebugHelpers'
import { useSnackbarStore } from '@/stores/snackbar'
import { ensureBookLocale, useBookI18n } from '../i18n'
import {
  buildOnlineCoverUrl,
  onlineBookApi,
  type BookSourceSummaryDto,
  type OnlineChapterItem,
  type SearchBookResult,
  type SourceTestInput,
  type SourceTestMode,
  type SourceTestReportDto,
} from '../onlineApi'

const props = defineProps<{
  open: boolean
  source: BookSourceSummaryDto | null
}>()

const emit = defineEmits<{
  close: []
  tested: []
}>()

const { t } = useI18n()
const { t: bt } = useBookI18n()
const snackbar = useSnackbarStore()

void ensureBookLocale()
watch(
  () => props.open,
  (isOpen) => {
    if (isOpen) void ensureBookLocale()
  },
)

type ResultTab = 'timeline' | 'search' | 'book_info' | 'toc' | 'content'

const modeOptions = computed<readonly SegmentedOption<SourceTestMode>[]>(() => [
  { value: 'full', label: t('onlineSources.debug.modeFull') },
  { value: 'search', label: t('onlineSources.debug.modeSearch') },
  { value: 'book_info', label: t('onlineSources.debug.modeDetail') },
  { value: 'toc', label: t('onlineSources.debug.modeChapters') },
  { value: 'content', label: bt('sourceEditor.debugModeContent') },
])

const tocFilter = ref('')
const lastBookUrl = ref('')
const lastTocUrl = ref('')
const lastChapterUrl = ref('')
const pendingPayload = ref<SourceTestInput | null>(null)
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
  BookSourceSummaryDto,
  SourceTestMode,
  SourceTestReportDto,
  ResultTab
>({
  open: toRef(props, 'open'),
  source: toRef(props, 'source'),
  defaultMode: 'full',
  defaultTab: 'timeline',
  defaultKeyword: '我的',
  runTest: async (sourceId, signal) => {
    const val = queryInput.value.trim()
    const payload: SourceTestInput = pendingPayload.value ?? { mode: mode.value }
    if (!pendingPayload.value) {
      if (mode.value === 'full' || mode.value === 'search') {
        payload.keyword = val || null
      } else if (mode.value === 'book_info') {
        payload.book_url = val || null
      } else if (mode.value === 'toc') {
        payload.toc_url = val || null
      } else if (mode.value === 'content') {
        payload.chapter_url = val || null
      }
    }
    return onlineBookApi.testSource(sourceId, payload, { signal })
  },
  onReset: () => {
    tocFilter.value = ''
    lastBookUrl.value = ''
    lastTocUrl.value = ''
    lastChapterUrl.value = ''
    pendingPayload.value = null
    pendingPreserve.value = false
  },
  onReportReady: (res, preferredTab) => {
    const prev = pendingPreserve.value ? report.value : null
    const prevBookUrl = prev?.book_detail?.book_url || lastBookUrl.value
    const ranStages = new Set(res.steps.map((s) => s.stage))
    const nextBookUrl = res.book_detail?.book_url || pendingPayload.value?.book_url?.trim() || ''
    const bookUrlChanged = Boolean(nextBookUrl && prevBookUrl && nextBookUrl !== prevBookUrl)

    const merged: SourceTestReportDto = prev
      ? {
          ...res,
          result_count: ranStages.has('search') ? res.result_count : prev.result_count,
          search_items: ranStages.has('search') ? res.search_items : prev.search_items,
          sample_titles: ranStages.has('search') ? res.sample_titles : prev.sample_titles,
          book_detail: ranStages.has('book_info') ? res.book_detail : prev.book_detail,
          toc_chapters: ranStages.has('toc')
            ? res.toc_chapters
            : bookUrlChanged
              ? []
              : prev.toc_chapters,
          toc_total_count: ranStages.has('toc')
            ? res.toc_total_count
            : bookUrlChanged
              ? 0
              : prev.toc_total_count,
          content_chapter_index: ranStages.has('content')
            ? res.content_chapter_index
            : bookUrlChanged
              ? null
              : prev.content_chapter_index,
          content_chapter_title: ranStages.has('content')
            ? res.content_chapter_title
            : bookUrlChanged
              ? null
              : prev.content_chapter_title,
          content_chapter_url: ranStages.has('content')
            ? res.content_chapter_url
            : bookUrlChanged
              ? null
              : prev.content_chapter_url,
          content_text: ranStages.has('content')
            ? res.content_text
            : bookUrlChanged
              ? null
              : prev.content_text,
          content_char_count: ranStages.has('content')
            ? res.content_char_count
            : bookUrlChanged
              ? 0
              : prev.content_char_count,
        }
      : res

    report.value = merged
    if (bookUrlChanged) {
      lastTocUrl.value = merged.book_detail?.toc_url || ''
      lastChapterUrl.value = merged.content_chapter_url || merged.toc_chapters[0]?.url || ''
    } else {
      if (merged.book_detail?.toc_url) lastTocUrl.value = merged.book_detail.toc_url
      if (merged.content_chapter_url) lastChapterUrl.value = merged.content_chapter_url
      else if (merged.toc_chapters[0]?.url) lastChapterUrl.value = merged.toc_chapters[0].url
    }
    if (merged.book_detail?.book_url) lastBookUrl.value = merged.book_detail.book_url
    else if (merged.search_items[0]?.book_url) lastBookUrl.value = merged.search_items[0].book_url

    if (preferredTab) {
      activeTab.value = preferredTab
    } else if (res.mode === 'search') {
      activeTab.value = res.search_items.length > 0 ? 'search' : 'timeline'
    } else if (res.mode === 'book_info') {
      activeTab.value = res.book_detail ? 'book_info' : 'timeline'
    } else if (res.mode === 'toc') {
      activeTab.value = res.toc_chapters.length > 0 ? 'toc' : 'timeline'
    } else if (res.mode === 'content') {
      activeTab.value = res.content_text ? 'content' : 'timeline'
    } else {
      activeTab.value = 'timeline'
    }
  },
  onTested: () => emit('tested'),
})

function handleModeChange(nextMode: SourceTestMode): void {
  mode.value = nextMode
  if (nextMode === 'book_info' && lastBookUrl.value) {
    queryInput.value = lastBookUrl.value
  } else if (nextMode === 'toc' && lastTocUrl.value) {
    queryInput.value = lastTocUrl.value
  } else if (nextMode === 'content' && lastChapterUrl.value) {
    queryInput.value = lastChapterUrl.value
  } else if (
    (nextMode === 'full' || nextMode === 'search') &&
    queryInput.value.trim().startsWith('http') &&
    props.source
  ) {
    queryInput.value = props.source.check_keyword?.trim() || '我的'
  }
}

const hasQuickHint = computed(() => {
  if ((mode.value === 'full' || mode.value === 'search') && props.source?.check_keyword) {
    return true
  }
  if (mode.value === 'book_info' && Boolean(lastBookUrl.value)) return true
  if (mode.value === 'toc' && Boolean(lastTocUrl.value)) return true
  if (mode.value === 'content' && Boolean(lastChapterUrl.value)) return true
  return false
})

const inputPlaceholder = computed(() => {
  const kw = props.source?.check_keyword?.trim() || '我的'
  if (mode.value === 'full' || mode.value === 'search') {
    return t('onlineSources.debug.keywordPlaceholder', { keyword: kw })
  }
  return t('onlineSources.debug.urlPlaceholder')
})

const domainLabel = computed(() => t('onlineSources.domainLabel.book'))

const resultTabs = computed<DebugResultTabItem<ResultTab>[]>(() => [
  { id: 'timeline', label: t('onlineSources.debug.tabTimeline'), icon: LuTerminal },
  {
    id: 'search',
    label: t('onlineSources.debug.tabSearch', { count: report.value?.result_count ?? 0 }),
    icon: LuSearch,
  },
  { id: 'book_info', label: t('onlineSources.debug.tabDetail'), icon: LuBookOpen },
  {
    id: 'toc',
    label: t('onlineSources.debug.tabChapters', { count: report.value?.toc_total_count ?? 0 }),
    icon: LuList,
  },
  {
    id: 'content',
    label: bt('sourceEditor.debugTabContent', { count: report.value?.content_char_count ?? 0 }),
    icon: LuFileText,
  },
])

function stageToTab(stage: string): ResultTab {
  if (stage === 'search') return 'search'
  if (stage === 'book_info') return 'book_info'
  if (stage === 'toc') return 'toc'
  if (stage === 'content') return 'content'
  return 'timeline'
}

const filteredToc = computed(() => {
  const list = report.value?.toc_chapters ?? []
  const q = tocFilter.value.trim().toLowerCase()
  if (!q) return list
  return list.filter(
    (ch) =>
      ch.title.toLowerCase().includes(q) ||
      String(ch.index + 1).includes(q) ||
      ch.url.toLowerCase().includes(q),
  )
})

const contentParagraphs = computed(() => {
  const raw = report.value?.content_text ?? ''
  if (!raw.trim()) return []
  return raw
    .split(/\r?\n+/)
    .map((line) => line.trim())
    .filter((line) => line.length > 0)
})

const detailFields = computed<SourceDebugFieldRow[]>(() => {
  const d = report.value?.book_detail
  if (!d) return []
  return [
    { label: 'name', value: d.name },
    { label: 'author', value: d.author },
    { label: 'kind', value: d.kind ?? '' },
    { label: 'wordCount', value: d.word_count ?? '' },
    { label: 'lastChapter', value: d.last_chapter ?? '' },
    { label: 'bookUrl', value: d.book_url, mono: true },
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

async function handleDebugBookItem(item: SearchBookResult): Promise<void> {
  if (item.book_url && item.book_url !== lastBookUrl.value) {
    lastTocUrl.value = ''
    lastChapterUrl.value = ''
  }
  mode.value = 'full'
  queryInput.value = item.book_url
  pendingPayload.value = { mode: 'full', book_url: item.book_url }
  pendingPreserve.value = true
  await executeTest('timeline')
}

async function handleDebugTocFromDetail(): Promise<void> {
  const tocUrl = report.value?.book_detail?.toc_url || lastTocUrl.value
  if (!tocUrl) return
  mode.value = 'toc'
  queryInput.value = tocUrl
  pendingPayload.value = { mode: 'toc', toc_url: tocUrl }
  pendingPreserve.value = true
  await executeTest('toc')
}

async function handleDebugChapter(ch: OnlineChapterItem): Promise<void> {
  if (!ch.url) return
  mode.value = 'content'
  queryInput.value = ch.url
  lastChapterUrl.value = ch.url
  pendingPayload.value = {
    mode: 'content',
    chapter_url: ch.url,
    chapter_index: ch.index,
    chapter_title: ch.title,
    book_url: lastBookUrl.value || null,
    toc_url: lastTocUrl.value || null,
  }
  pendingPreserve.value = true
  await executeTest('content')
}

async function copyDebugReport(): Promise<void> {
  if (!report.value || !props.source) return
  const r = report.value
  const extraLines: string[] = []
  if (r.book_detail) {
    extraLines.push(
      '',
      '--- Detail ---',
      `name: ${r.book_detail.name}`,
      `author: ${r.book_detail.author}`,
      `tocUrl: ${r.book_detail.toc_url}`,
      `intro: ${r.book_detail.intro ?? ''}`,
    )
  }
  if (r.toc_total_count > 0) {
    extraLines.push('', `--- TOC (${r.toc_total_count}) ---`)
    for (const ch of r.toc_chapters.slice(0, 20)) {
      extraLines.push(`[${ch.index + 1}] ${ch.title} -> ${ch.url}`)
    }
  }
  if (r.content_text) {
    extraLines.push(
      '',
      `--- Content: ${r.content_chapter_title ?? ''} (${r.content_char_count}) ---`,
      r.content_text,
    )
  }
  await copySourceDebugReport({
    source: props.source,
    report: r,
    snackbar,
    copiedMessage: t('onlineSources.debug.reportCopied'),
    failedMessage: t('onlineSources.debug.copyFailed'),
    extraLines,
  })
}

async function copyContentText(): Promise<void> {
  if (!report.value?.content_text) return
  await copyDebugTextToClipboard(
    report.value.content_text,
    snackbar,
    bt('sourceEditor.debugContentCopied'),
    t('onlineSources.debug.copyFailed'),
  )
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
      <MdChip
        v-if="source?.check_keyword && (mode === 'full' || mode === 'search')"
        variant="suggestion"
        :label="t('onlineSources.debug.checkKeywordHint', { keyword: source.check_keyword })"
        @click="queryInput = source.check_keyword!"
      />
      <MdChip
        v-if="lastBookUrl && mode === 'book_info'"
        variant="suggestion"
        :label="t('onlineSources.debug.lastDetailHint')"
        @click="queryInput = lastBookUrl"
      />
      <MdChip
        v-if="lastTocUrl && mode === 'toc'"
        variant="suggestion"
        :label="t('onlineSources.debug.lastTocHint')"
        @click="queryInput = lastTocUrl"
      />
      <MdChip
        v-if="lastChapterUrl && mode === 'content'"
        variant="suggestion"
        :label="t('onlineSources.debug.lastChapterHint')"
        @click="queryInput = lastChapterUrl"
      />
    </template>

    <template #tab-content>
      <!-- TAB 2: Search Results -->
      <div v-if="activeTab === 'search' && report" class="flex flex-col gap-3">
        <div
          v-if="report.search_items.length === 0"
          class="rounded-corner-lg bg-surface-container py-10 text-center text-body-small text-on-surface-variant"
        >
          {{ bt('sourceEditor.debugNoSearch') }}
        </div>

        <template v-else>
          <p class="text-body-small text-on-surface-variant">
            {{
              bt('sourceEditor.debugSearchSummary', {
                total: report.result_count,
                shown: report.search_items.length,
              })
            }}
          </p>
          <div
            v-for="(item, idx) in report.search_items"
            :key="`${item.book_url}-${idx}`"
            class="flex items-start gap-3.5 rounded-corner-lg bg-surface-container p-3.5"
          >
            <div class="w-14 shrink-0">
              <MediaCoverImage
                :src="item.cover_url ? buildOnlineCoverUrl(item.cover_url) : null"
                :alt="item.name"
                aspect-class="aspect-[2/3]"
                rounded="rounded-corner-sm"
                bg-class="bg-surface-container-high"
              >
                <template #fallback>
                  <span class="text-label-small px-1 text-center text-on-surface-variant">
                    {{ bt('sourceEditor.debugNoCover') }}
                  </span>
                </template>
              </MediaCoverImage>
            </div>

            <div class="flex min-w-0 flex-1 flex-col gap-1.5">
              <div class="flex flex-wrap items-start justify-between gap-2">
                <div class="flex min-w-0 flex-wrap items-center gap-1.5">
                  <span class="font-mono text-label-small tabular-nums text-on-surface-variant">
                    #{{ idx + 1 }}
                  </span>
                  <span class="text-title-small text-on-surface">
                    {{ item.name }}
                  </span>
                  <span v-if="item.author" class="text-body-small text-on-surface-variant">
                    · {{ item.author }}
                  </span>
                </div>

                <MdButton
                  variant="tonal"
                  :disabled="running"
                  class="shrink-0"
                  @click="handleDebugBookItem(item)"
                >
                  <span>{{ bt('sourceEditor.debugTestBookDetail') }}</span>
                  <template #trailing>
                    <LuArrowRight class="h-3.5 w-3.5" aria-hidden="true" />
                  </template>
                </MdButton>
              </div>

              <div class="flex flex-wrap items-center gap-1.5 text-label-small">
                <span
                  v-if="item.kind"
                  class="rounded-corner-full bg-surface-container-high px-2 py-0.5 text-on-surface-variant"
                >
                  {{ item.kind }}
                </span>
                <span
                  v-if="item.word_count"
                  class="rounded-corner-full bg-surface-container-high px-2 py-0.5 text-on-surface-variant"
                >
                  {{ item.word_count }}
                </span>
                <span v-if="item.last_chapter" class="text-on-surface-variant">
                  {{ bt('sourceEditor.debugLatestPrefix', { chapter: item.last_chapter }) }}
                </span>
              </div>

              <p
                v-if="item.intro"
                class="text-body-small whitespace-pre-wrap text-on-surface-variant"
              >
                {{ item.intro }}
              </p>

              <p class="break-all font-mono text-label-small text-on-surface-variant">
                {{ item.book_url }}
              </p>
            </div>
          </div>
        </template>
      </div>

      <!-- TAB 3: Book Detail -->
      <div v-else-if="activeTab === 'book_info' && report" class="flex flex-col gap-3">
        <div
          v-if="!report.book_detail"
          class="rounded-corner-lg bg-surface-container py-10 text-center text-body-small text-on-surface-variant"
        >
          {{ bt('sourceEditor.debugNoDetail') }}
        </div>

        <template v-else>
          <div class="flex flex-wrap items-center justify-between gap-2">
            <span class="text-label-medium text-on-surface-variant">
              {{ bt('sourceEditor.debugDetailTitle') }}
            </span>
            <MdButton
              variant="tonal"
              :disabled="running || !report.book_detail.toc_url"
              @click="handleDebugTocFromDetail"
            >
              <span>{{ bt('sourceEditor.debugTestTocFromDetail') }}</span>
              <template #trailing>
                <LuArrowRight class="h-3.5 w-3.5" aria-hidden="true" />
              </template>
            </MdButton>
          </div>

          <SourceDebugFieldsCard
            :fields="detailFields"
            :empty-label="bt('sourceEditor.debugEmptyValue')"
          />
        </template>
      </div>

      <!-- TAB 4: TOC Chapters -->
      <div v-else-if="activeTab === 'toc' && report" class="flex flex-col gap-3">
        <div
          v-if="report.toc_chapters.length === 0"
          class="rounded-corner-lg bg-surface-container py-10 text-center text-body-small text-on-surface-variant"
        >
          {{ bt('sourceEditor.debugNoToc') }}
        </div>

        <template v-else>
          <div class="flex flex-wrap items-center justify-between gap-2">
            <span class="text-body-small text-on-surface-variant">
              {{ bt('sourceEditor.debugTocCount', { total: report.toc_total_count }) }}
              <template v-if="report.toc_total_count > report.toc_chapters.length">
                {{ bt('sourceEditor.debugTocShownHint', { shown: report.toc_chapters.length }) }}
              </template>
            </span>
            <div class="w-full sm:w-60">
              <MdTextField
                v-model="tocFilter"
                type="search"
                size="sm"
                clearable
                :placeholder="bt('sourceEditor.debugTocFilterPlaceholder')"
              />
            </div>
          </div>

          <div class="divide-y divide-outline-variant/50 overflow-hidden rounded-corner-lg bg-surface-container">
            <div
              v-for="ch in filteredToc"
              :key="`${ch.index}-${ch.url}`"
              class="flex items-center justify-between gap-3 px-4 py-2.5"
            >
              <div class="min-w-0 flex-1">
                <div class="flex items-center gap-2">
                  <span class="w-10 shrink-0 font-mono text-label-small tabular-nums text-on-surface-variant">
                    #{{ ch.index + 1 }}
                  </span>
                  <span
                    v-if="ch.is_volume"
                    class="rounded-corner-full bg-tertiary-container px-2 py-0.5 text-label-small text-on-tertiary-container"
                  >
                    {{ bt('sourceEditor.debugVolumeBadge') }}
                  </span>
                  <span class="truncate text-body-small text-on-surface">
                    {{ ch.title }}
                  </span>
                </div>
                <p class="mt-0.5 truncate pl-12 font-mono text-label-small text-on-surface-variant">
                  {{ ch.url || bt('sourceEditor.debugNoLink') }}
                </p>
              </div>

              <MdButton
                v-if="ch.url && !ch.is_volume"
                variant="outlined"
                size="sm"
                :disabled="running"
                class="shrink-0"
                @click="handleDebugChapter(ch)"
              >
                {{ bt('sourceEditor.debugTestChapter') }}
              </MdButton>
            </div>
          </div>
        </template>
      </div>

      <!-- TAB 5: Full Chapter Content -->
      <div v-else-if="activeTab === 'content' && report" class="flex flex-col gap-3">
        <div
          v-if="!report.content_text"
          class="rounded-corner-lg bg-surface-container py-10 text-center text-body-small text-on-surface-variant"
        >
          {{ bt('sourceEditor.debugNoContent') }}
        </div>

        <template v-else>
          <div class="flex flex-wrap items-center justify-between gap-2 rounded-corner-lg bg-surface-container px-4 py-3">
            <div class="min-w-0">
              <div class="text-title-small text-on-surface">
                {{ report.content_chapter_title || bt('sourceEditor.debugContentTitleFallback') }}
              </div>
              <div class="mt-0.5 flex flex-wrap items-center gap-2 text-body-small text-on-surface-variant">
                <span>{{ bt('sourceEditor.debugContentChars', { count: report.content_char_count }) }}</span>
                <span v-if="report.content_chapter_url" class="break-all font-mono text-label-small">
                  · {{ report.content_chapter_url }}
                </span>
              </div>
            </div>
            <MdButton
              variant="tonal"
              :icon="LuCopy"
              class="shrink-0"
              @click="copyContentText"
            >
              {{ bt('sourceEditor.debugCopyContent') }}
            </MdButton>
          </div>

          <article
            class="flex flex-col gap-3 rounded-corner-lg bg-surface-container p-5 text-body-large leading-7 text-on-surface"
          >
            <p
              v-for="(para, pIdx) in contentParagraphs"
              :key="pIdx"
              class="indent-[2em] break-words"
            >
              {{ para }}
            </p>
          </article>
        </template>
      </div>
    </template>
  </OnlineSourceDebugShell>
</template>
