<script setup lang="ts">
import { computed, ref, toRef } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  LuArrowRight,
  LuFilm,
  LuFolderTree,
  LuPlay,
  LuSearch,
  LuTerminal,
} from 'vue-icons-plus/lu'
import OnlineSourceDebugShell, {
  type DebugResultTabItem,
} from '@/components/online-sources/OnlineSourceDebugShell.vue'
import { useOnlineSourceDebug } from '@/components/online-sources/useOnlineSourceDebug'
import type { SegmentedOption } from '@/components/ui/MdSegmentedButton.vue'
import { useSnackbarStore } from '@/stores/snackbar'
import {
  onlineVideoApi,
  resolveOnlineVideoPosterUrl,
  type OnlineVideoCategory,
  type OnlineVideoEpisode,
  type OnlineVideoSummary,
  type VideoSourceSummaryDto,
  type VideoSourceTestInput,
  type VideoSourceTestMode,
  type VideoSourceTestReportDto,
} from '../onlineApi'

const props = defineProps<{
  open: boolean
  source: VideoSourceSummaryDto | null
}>()

const emit = defineEmits<{
  close: []
  tested: []
}>()

const { t } = useI18n()
const snackbar = useSnackbarStore()

type ResultTab = 'timeline' | 'categories' | 'search' | 'detail' | 'play'

const modeOptions = computed<readonly SegmentedOption<VideoSourceTestMode>[]>(() => [
  { value: 'full', label: t('onlineSources.debug.modeFull') },
  { value: 'categories', label: t('onlineSources.debug.modeCategories') },
  { value: 'search', label: t('onlineSources.debug.modeSearch') },
  { value: 'browse', label: t('onlineSources.debug.modeBrowse') },
  { value: 'detail', label: t('onlineSources.debug.modeDetail') },
  { value: 'play', label: t('onlineSources.debug.modePlay') },
])

const lastCategoryId = ref('')
const lastVideoId = ref('')
const lastDetailUrl = ref('')
const lastPlayUrl = ref('')
const lastGroupName = ref('')
const pendingPayload = ref<VideoSourceTestInput | null>(null)
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
  VideoSourceSummaryDto,
  VideoSourceTestMode,
  VideoSourceTestReportDto,
  ResultTab
>({
  open: toRef(props, 'open'),
  source: toRef(props, 'source'),
  defaultMode: 'full',
  defaultTab: 'timeline',
  defaultKeyword: '电影',
  runTest: async (sourceId, signal) => {
    const val = queryInput.value.trim()
    const payload: VideoSourceTestInput = pendingPayload.value ?? { mode: mode.value }
    if (!pendingPayload.value) {
      if (mode.value === 'full' || mode.value === 'search') {
        payload.keyword = val || null
      } else if (mode.value === 'browse') {
        payload.category_id = val || null
      } else if (mode.value === 'detail') {
        if (val.startsWith('http')) payload.detail_url = val
        else payload.video_id = val || null
      } else if (mode.value === 'play') {
        payload.play_url = val || null
        payload.group_name = lastGroupName.value || null
      }
    }
    return onlineVideoApi.testSource(sourceId, payload, { signal })
  },
  onReset: () => {
    lastCategoryId.value = ''
    lastVideoId.value = ''
    lastDetailUrl.value = ''
    lastPlayUrl.value = ''
    lastGroupName.value = ''
    pendingPayload.value = null
    pendingPreserve.value = false
  },
  onReportReady: (res, preferredTab) => {
    const prev = pendingPreserve.value ? report.value : null
    const ranStages = new Set(res.steps.map((s) => s.stage))
    const merged: VideoSourceTestReportDto = prev
      ? {
          ...res,
          categories: ranStages.has('categories') ? res.categories : prev.categories,
          result_count:
            ranStages.has('search') || ranStages.has('browse')
              ? res.result_count
              : prev.result_count,
          search_items:
            ranStages.has('search') || ranStages.has('browse')
              ? res.search_items
              : prev.search_items,
          sample_titles:
            ranStages.has('search') || ranStages.has('browse')
              ? res.sample_titles
              : prev.sample_titles,
          video_detail: ranStages.has('detail') ? res.video_detail : prev.video_detail,
          play_result: ranStages.has('play') ? res.play_result : prev.play_result,
        }
      : res

    report.value = merged
    if (merged.categories[0]?.id) lastCategoryId.value = merged.categories[0].id
    if (merged.video_detail?.id) lastVideoId.value = merged.video_detail.id
    if (merged.search_items[0]?.detail_url) lastDetailUrl.value = merged.search_items[0].detail_url
    if (merged.video_detail?.play_groups[0]) {
      lastGroupName.value = merged.video_detail.play_groups[0].name
      if (merged.video_detail.play_groups[0].episodes[0]?.play_url) {
        lastPlayUrl.value = merged.video_detail.play_groups[0].episodes[0].play_url
      }
    }

    if (preferredTab) {
      activeTab.value = preferredTab
    } else if (res.mode === 'categories') {
      activeTab.value = res.categories.length > 0 ? 'categories' : 'timeline'
    } else if (res.mode === 'search' || res.mode === 'browse') {
      activeTab.value = res.search_items.length > 0 ? 'search' : 'timeline'
    } else if (res.mode === 'detail') {
      activeTab.value = res.video_detail ? 'detail' : 'timeline'
    } else if (res.mode === 'play') {
      activeTab.value = res.play_result ? 'play' : 'timeline'
    } else {
      activeTab.value = 'timeline'
    }
  },
  onTested: () => emit('tested'),
})

function handleModeChange(nextMode: VideoSourceTestMode): void {
  mode.value = nextMode
  if (nextMode === 'browse' && lastCategoryId.value) {
    queryInput.value = lastCategoryId.value
  } else if (nextMode === 'detail' && (lastDetailUrl.value || lastVideoId.value)) {
    queryInput.value = lastDetailUrl.value || lastVideoId.value
  } else if (nextMode === 'play' && lastPlayUrl.value) {
    queryInput.value = lastPlayUrl.value
  } else if ((nextMode === 'full' || nextMode === 'search') && props.source) {
    queryInput.value = props.source.check_keyword?.trim() || '电影'
  }
}

const hasQuickHint = computed(() => {
  if ((mode.value === 'full' || mode.value === 'search') && props.source?.check_keyword) return true
  if (mode.value === 'browse' && Boolean(lastCategoryId.value)) return true
  if (mode.value === 'detail' && Boolean(lastDetailUrl.value || lastVideoId.value)) return true
  if (mode.value === 'play' && Boolean(lastPlayUrl.value)) return true
  return false
})

const inputPlaceholder = computed(() => {
  const kw = props.source?.check_keyword?.trim() || '电影'
  if (mode.value === 'full' || mode.value === 'search') {
    return t('onlineSources.debug.keywordPlaceholder', { keyword: kw })
  }
  return t('onlineSources.debug.urlPlaceholder')
})

const domainLabel = computed(() => t('onlineSources.domainLabel.video'))

const resultTabs = computed<DebugResultTabItem<ResultTab>[]>(() => [
  { id: 'timeline', label: t('onlineSources.debug.tabTimeline'), icon: LuTerminal },
  {
    id: 'categories',
    label: t('onlineSources.debug.tabCategories', { count: report.value?.categories.length ?? 0 }),
    icon: LuFolderTree,
  },
  {
    id: 'search',
    label: t('onlineSources.debug.tabSearch', { count: report.value?.result_count ?? 0 }),
    icon: LuSearch,
  },
  { id: 'detail', label: t('onlineSources.debug.tabDetail'), icon: LuFilm },
  { id: 'play', label: t('onlineSources.debug.tabPlay'), icon: LuPlay },
])

function stageToTab(stage: string): ResultTab {
  if (stage === 'categories') return 'categories'
  if (stage === 'search' || stage === 'browse') return 'search'
  if (stage === 'detail') return 'detail'
  if (stage === 'play') return 'play'
  return 'timeline'
}

async function handleRun(): Promise<void> {
  pendingPayload.value = null
  pendingPreserve.value = mode.value !== 'full' && mode.value !== 'search'
  await executeTest()
}

async function handleBrowseCategory(cat: OnlineVideoCategory): Promise<void> {
  lastCategoryId.value = cat.id
  mode.value = 'browse'
  queryInput.value = cat.id
  pendingPayload.value = { mode: 'browse', category_id: cat.id }
  pendingPreserve.value = true
  await executeTest('search')
}

async function handleDebugVideoItem(item: OnlineVideoSummary): Promise<void> {
  lastVideoId.value = item.vod_id
  lastDetailUrl.value = item.detail_url
  mode.value = 'detail'
  queryInput.value = item.detail_url || item.vod_id
  pendingPayload.value = {
    mode: 'detail',
    video_id: item.vod_id || null,
    detail_url: item.detail_url || null,
  }
  pendingPreserve.value = true
  await executeTest('detail')
}

async function handleDebugEpisode(ep: OnlineVideoEpisode, groupName: string): Promise<void> {
  lastPlayUrl.value = ep.play_url
  lastGroupName.value = groupName
  mode.value = 'play'
  queryInput.value = ep.play_url
  pendingPayload.value = {
    mode: 'play',
    play_url: ep.play_url,
    group_name: groupName,
  }
  pendingPreserve.value = true
  await executeTest('play')
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
        v-if="lastCategoryId && mode === 'browse'"
        type="button"
        class="md-state rounded-full bg-[var(--md-sys-color-surface-container-high)] px-2.5 py-0.5 font-mono text-[var(--md-sys-color-on-surface)]"
        @click="queryInput = lastCategoryId"
      >
        {{ t('onlineSources.debug.lastCategoryHint') }}
      </button>
      <button
        v-if="(lastDetailUrl || lastVideoId) && mode === 'detail'"
        type="button"
        class="md-state max-w-xs truncate rounded-full bg-[var(--md-sys-color-surface-container-high)] px-2.5 py-0.5 font-mono text-[var(--md-sys-color-on-surface)]"
        @click="queryInput = lastDetailUrl || lastVideoId"
      >
        {{ t('onlineSources.debug.lastDetailHint') }}
      </button>
      <button
        v-if="lastPlayUrl && mode === 'play'"
        type="button"
        class="md-state max-w-xs truncate rounded-full bg-[var(--md-sys-color-surface-container-high)] px-2.5 py-0.5 font-mono text-[var(--md-sys-color-on-surface)]"
        @click="queryInput = lastPlayUrl"
      >
        {{ t('onlineSources.debug.lastPlayHint') }}
      </button>
    </template>

    <template #tab-content>
      <!-- Categories -->
      <div v-if="activeTab === 'categories' && report" class="flex flex-col gap-2">
        <div
          v-if="report.categories.length === 0"
          class="rounded-2xl bg-[var(--md-sys-color-surface-container)] py-10 text-center text-xs text-[var(--md-sys-color-on-surface-variant)]"
        >
          {{ t('onlineSources.debug.emptyCategories') }}
        </div>
        <div v-else class="flex flex-wrap gap-2">
          <button
            v-for="cat in report.categories"
            :key="cat.id"
            type="button"
            :disabled="running"
            class="md-btn md-btn-sm md-btn-tonal md-state"
            @click="handleBrowseCategory(cat)"
          >
            <span>{{ cat.name }}</span>
            <span class="font-mono text-[10px] opacity-70">#{{ cat.id }}</span>
          </button>
        </div>
      </div>

      <!-- Search / Browse Results -->
      <div v-else-if="activeTab === 'search' && report" class="flex flex-col gap-2.5">
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
            :key="`${item.vod_id}-${item.detail_url}-${idx}`"
            class="flex items-start gap-3 rounded-2xl bg-[var(--md-sys-color-surface-container)] p-3.5"
          >
            <div class="h-20 w-14 shrink-0 overflow-hidden rounded-lg bg-[var(--md-sys-color-surface-container-high)]">
              <img
                v-if="resolveOnlineVideoPosterUrl(item.proxy_cover_url, item.cover_url, source?.id)"
                :src="resolveOnlineVideoPosterUrl(item.proxy_cover_url, item.cover_url, source?.id)"
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
                  <span v-if="item.remarks" class="text-xs text-[var(--md-sys-color-on-surface-variant)]">
                    · {{ item.remarks }}
                  </span>
                </div>
                <button
                  type="button"
                  :disabled="running"
                  class="md-btn md-btn-sm md-btn-tonal md-state shrink-0"
                  @click="handleDebugVideoItem(item)"
                >
                  <span>{{ t('onlineSources.debug.debugDetailBtn') }}</span>
                  <LuArrowRight class="h-3.5 w-3.5" />
                </button>
              </div>
              <div class="flex flex-wrap gap-1.5 text-[11px] text-[var(--md-sys-color-on-surface-variant)]">
                <span v-if="item.type_name">{{ item.type_name }}</span>
                <span v-if="item.year">{{ item.year }}</span>
                <span v-if="item.area">{{ item.area }}</span>
              </div>
              <p class="break-all font-mono text-[11px] text-[var(--md-sys-color-on-surface-variant)]/85">
                {{ item.detail_url || item.vod_id }}
              </p>
            </div>
          </div>
        </template>
      </div>

      <!-- Detail & Play Groups -->
      <div v-else-if="activeTab === 'detail' && report" class="flex flex-col gap-3">
        <div
          v-if="!report.video_detail"
          class="rounded-2xl bg-[var(--md-sys-color-surface-container)] py-10 text-center text-xs text-[var(--md-sys-color-on-surface-variant)]"
        >
          {{ t('onlineSources.debug.emptyDetail') }}
        </div>
        <template v-else>
          <div class="flex flex-col gap-2 rounded-2xl bg-[var(--md-sys-color-surface-container)] p-4 text-xs">
            <div class="text-sm font-semibold text-[var(--md-sys-color-on-surface)]">
              {{ report.video_detail.title }}
            </div>
            <div class="flex flex-wrap gap-2 text-[var(--md-sys-color-on-surface-variant)]">
              <span v-if="report.video_detail.year">{{ report.video_detail.year }}</span>
              <span v-if="report.video_detail.area">{{ report.video_detail.area }}</span>
              <span v-if="report.video_detail.director">{{ report.video_detail.director }}</span>
              <span v-if="report.video_detail.actors">{{ report.video_detail.actors }}</span>
            </div>
            <p v-if="report.video_detail.intro" class="leading-relaxed text-[var(--md-sys-color-on-surface-variant)]">
              {{ report.video_detail.intro }}
            </p>
          </div>

          <div
            v-if="report.video_detail.play_groups.length > 0"
            class="flex flex-col gap-3 rounded-2xl bg-[var(--md-sys-color-surface-container)] p-4"
          >
            <h4 class="text-xs font-semibold text-[var(--md-sys-color-on-surface)]">
              {{ t('onlineSources.debug.playGroupsTitle') }}
            </h4>
            <div
              v-for="group in report.video_detail.play_groups"
              :key="group.name"
              class="flex flex-col gap-2"
            >
              <div class="text-xs font-medium text-[var(--md-sys-color-primary)]">
                {{ group.name }} ({{ group.episodes.length }})
              </div>
              <div class="flex flex-wrap gap-1.5">
                <button
                  v-for="ep in group.episodes.slice(0, 40)"
                  :key="`${group.name}-${ep.index}-${ep.play_url}`"
                  type="button"
                  :disabled="running"
                  class="md-btn md-btn-sm md-btn-outlined md-state"
                  @click="handleDebugEpisode(ep, group.name)"
                >
                  {{ ep.name }}
                </button>
              </div>
            </div>
          </div>
        </template>
      </div>

      <!-- Play Result -->
      <div v-else-if="activeTab === 'play' && report" class="flex flex-col gap-3">
        <div
          v-if="!report.play_result"
          class="rounded-2xl bg-[var(--md-sys-color-surface-container)] py-10 text-center text-xs text-[var(--md-sys-color-on-surface-variant)]"
        >
          {{ t('onlineSources.debug.emptyPlay') }}
        </div>
        <div
          v-else
          class="flex flex-col gap-2.5 rounded-2xl bg-[var(--md-sys-color-surface-container)] p-4 text-xs"
        >
          <div class="flex flex-wrap gap-3">
            <span>
              {{ t('onlineSources.debug.streamTypeLabel') }}:
              <strong class="font-mono">{{ report.play_result.stream_type }}</strong>
            </span>
            <span>
              {{ t('onlineSources.debug.headersAppliedLabel') }}:
              <strong>
                {{
                  report.play_result.headers_applied
                    ? t('onlineSources.debug.yes')
                    : t('onlineSources.debug.no')
                }}
              </strong>
            </span>
          </div>
          <div class="break-all font-mono text-[11px] text-[var(--md-sys-color-on-surface-variant)]">
            {{ t('onlineSources.debug.streamUrlLabel') }}: {{ report.play_result.stream_url }}
          </div>
          <div class="break-all font-mono text-[11px] text-[var(--md-sys-color-on-surface-variant)]">
            {{ t('onlineSources.debug.proxyStreamUrlLabel') }}: {{ report.play_result.proxy_stream_url }}
          </div>
          <div v-if="report.play_result.subtitles.length > 0" class="pt-1">
            <div class="font-semibold">{{ t('onlineSources.debug.subtitlesTitle') }}</div>
            <ul class="mt-1 space-y-1 font-mono text-[11px] text-[var(--md-sys-color-on-surface-variant)]">
              <li v-for="(sub, sIdx) in report.play_result.subtitles" :key="sIdx">
                [{{ sub.lang || 'sub' }}] {{ sub.label }}: {{ sub.url }}
              </li>
            </ul>
          </div>
        </div>
      </div>
    </template>
  </OnlineSourceDebugShell>
</template>
