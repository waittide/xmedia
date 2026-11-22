<script setup lang="ts">
import { computed, reactive, ref, toRef } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  LuArrowRight,
  LuChevronDown,
  LuChevronUp,
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
import {
  MdButton,
  MdChip,
  MediaCoverImage,
  type SegmentedOption,
} from '@/components/ui'
import { useSnackbarStore } from '@/stores/snackbar'
import { useVideoI18n } from '../i18n'
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

const EPISODE_PREVIEW_LIMIT = 12

const props = defineProps<{
  open: boolean
  source: VideoSourceSummaryDto | null
}>()

const emit = defineEmits<{
  close: []
  tested: []
}>()

const { t, locale } = useI18n()
const { t: videoT } = useVideoI18n()
const snackbar = useSnackbarStore()
const isZh = computed(() => locale.value.startsWith('zh'))

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
const expandedGroups = reactive<Record<string, boolean>>({})

const defaultDebugKeyword = computed(() => videoT('player.debugDefaultKeyword'))

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
  defaultKeyword: defaultDebugKeyword.value,
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
    for (const k of Object.keys(expandedGroups)) {
      delete expandedGroups[k]
    }
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
    queryInput.value = props.source.check_keyword?.trim() || defaultDebugKeyword.value
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
  const kw = props.source?.check_keyword?.trim() || defaultDebugKeyword.value
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

function visibleGroupEpisodes(groupName: string, episodes: OnlineVideoEpisode[]): OnlineVideoEpisode[] {
  if (expandedGroups[groupName] || episodes.length <= EPISODE_PREVIEW_LIMIT) {
    return episodes
  }
  return episodes.slice(0, EPISODE_PREVIEW_LIMIT)
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
      <MdChip
        v-if="source?.check_keyword && (mode === 'full' || mode === 'search')"
        variant="suggestion"
        :label="t('onlineSources.debug.checkKeywordHint', { keyword: source.check_keyword })"
        @click="queryInput = source.check_keyword!"
      />
      <MdChip
        v-if="lastCategoryId && mode === 'browse'"
        variant="suggestion"
        :label="t('onlineSources.debug.lastCategoryHint')"
        @click="queryInput = lastCategoryId"
      />
      <MdChip
        v-if="(lastDetailUrl || lastVideoId) && mode === 'detail'"
        variant="suggestion"
        :label="t('onlineSources.debug.lastDetailHint')"
        @click="queryInput = lastDetailUrl || lastVideoId"
      />
      <MdChip
        v-if="lastPlayUrl && mode === 'play'"
        variant="suggestion"
        :label="t('onlineSources.debug.lastPlayHint')"
        @click="queryInput = lastPlayUrl"
      />
    </template>

    <template #tab-content>
      <!-- Categories -->
      <div v-if="activeTab === 'categories' && report" class="flex flex-col gap-2">
        <div
          v-if="report.categories.length === 0"
          class="text-body-small rounded-corner-lg bg-surface-container py-10 text-center text-on-surface-variant"
        >
          {{ t('onlineSources.debug.emptyCategories') }}
        </div>
        <div v-else class="flex flex-wrap gap-1.5">
          <MdChip
            v-for="cat in report.categories"
            :key="cat.id"
            variant="assist"
            :label="`${cat.name} (#${cat.id})`"
            :disabled="running"
            @click="handleBrowseCategory(cat)"
          />
        </div>
      </div>

      <!-- Search / Browse Results -->
      <div v-else-if="activeTab === 'search' && report" class="flex flex-col gap-2.5">
        <div
          v-if="report.search_items.length === 0"
          class="text-body-small rounded-corner-lg bg-surface-container py-10 text-center text-on-surface-variant"
        >
          {{ t('onlineSources.debug.emptySearch') }}
        </div>
        <template v-else>
          <p class="text-body-small text-on-surface-variant">
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
            class="flex items-start gap-3 rounded-corner-lg bg-surface-container p-3.5"
          >
            <div class="w-14 shrink-0 overflow-hidden rounded-corner-sm bg-surface-container-high">
              <MediaCoverImage
                :src="resolveOnlineVideoPosterUrl(item.proxy_cover_url, item.cover_url, source?.id) || null"
                :alt="item.title"
                aspect-class="aspect-[2/3]"
                rounded="rounded-corner-sm"
                :fallback-icon="LuFilm"
                fallback-icon-class="h-5 w-5 text-on-surface-variant/50"
              />
            </div>

            <div class="min-w-0 flex-1 space-y-1">
              <div class="flex flex-wrap items-start justify-between gap-2">
                <div class="flex flex-wrap items-baseline gap-1.5">
                  <span class="text-label-small font-mono text-on-surface-variant">
                    #{{ idx + 1 }}
                  </span>
                  <span class="text-title-small text-on-surface">
                    {{ item.title }}
                  </span>
                  <span v-if="item.remarks" class="text-body-small text-on-surface-variant">
                    · {{ item.remarks }}
                  </span>
                </div>
                <MdButton
                  variant="tonal"
                  size="sm"
                  :disabled="running"
                  @click="handleDebugVideoItem(item)"
                >
                  <span>{{ t('onlineSources.debug.debugDetailBtn') }}</span>
                  <template #trailing>
                    <LuArrowRight class="h-3.5 w-3.5" aria-hidden="true" />
                  </template>
                </MdButton>
              </div>
              <div class="text-body-small flex flex-wrap gap-1.5 text-on-surface-variant">
                <span v-if="item.type_name">{{ item.type_name }}</span>
                <span v-if="item.year">{{ item.year }}</span>
                <span v-if="item.area">{{ item.area }}</span>
              </div>
              <p class="text-label-small break-all font-mono text-on-surface-variant">
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
          class="text-body-small rounded-corner-lg bg-surface-container py-10 text-center text-on-surface-variant"
        >
          {{ t('onlineSources.debug.emptyDetail') }}
        </div>
        <template v-else>
          <div class="flex flex-col gap-2 rounded-corner-lg bg-surface-container p-4">
            <div class="text-title-medium text-on-surface">
              {{ report.video_detail.title }}
            </div>
            <div class="text-body-small flex flex-wrap gap-2 text-on-surface-variant">
              <span v-if="report.video_detail.year">{{ report.video_detail.year }}</span>
              <span v-if="report.video_detail.area">{{ report.video_detail.area }}</span>
              <span v-if="report.video_detail.director">{{ report.video_detail.director }}</span>
              <span v-if="report.video_detail.actors">{{ report.video_detail.actors }}</span>
            </div>
            <p v-if="report.video_detail.intro" class="text-body-small text-on-surface-variant">
              {{ report.video_detail.intro }}
            </p>
          </div>

          <div
            v-if="report.video_detail.play_groups.length > 0"
            class="flex flex-col gap-3 rounded-corner-lg bg-surface-container p-4"
          >
            <h4 class="text-title-small text-on-surface">
              {{ t('onlineSources.debug.playGroupsTitle') }}
            </h4>
            <div
              v-for="group in report.video_detail.play_groups"
              :key="group.name"
              class="flex flex-col gap-2"
            >
              <div class="text-label-large text-primary">
                {{ group.name }} ({{ group.episodes.length }})
              </div>
              <div class="flex flex-wrap items-center gap-1.5">
                <MdChip
                  v-for="ep in visibleGroupEpisodes(group.name, group.episodes)"
                  :key="`${group.name}-${ep.index}-${ep.play_url}`"
                  variant="assist"
                  :label="ep.name"
                  :disabled="running"
                  @click="handleDebugEpisode(ep, group.name)"
                />
                <MdButton
                  v-if="group.episodes.length > EPISODE_PREVIEW_LIMIT"
                  variant="text"
                  size="sm"
                  :icon="expandedGroups[group.name] ? LuChevronUp : LuChevronDown"
                  @click="expandedGroups[group.name] = !expandedGroups[group.name]"
                >
                  {{
                    expandedGroups[group.name]
                      ? isZh
                        ? '收起'
                        : 'Show less'
                      : isZh
                        ? `展开全部 (${group.episodes.length})`
                        : `Show all (${group.episodes.length})`
                  }}
                </MdButton>
              </div>
            </div>
          </div>
        </template>
      </div>

      <!-- Play Result -->
      <div v-else-if="activeTab === 'play' && report" class="flex flex-col gap-3">
        <div
          v-if="!report.play_result"
          class="text-body-small rounded-corner-lg bg-surface-container py-10 text-center text-on-surface-variant"
        >
          {{ t('onlineSources.debug.emptyPlay') }}
        </div>
        <div
          v-else
          class="text-body-small flex flex-col gap-2.5 rounded-corner-lg bg-surface-container p-4 text-on-surface"
        >
          <div class="flex flex-wrap gap-4">
            <span>
              {{ t('onlineSources.debug.streamTypeLabel') }}:
              <span class="text-label-large font-mono">{{ report.play_result.stream_type }}</span>
            </span>
            <span>
              {{ t('onlineSources.debug.headersAppliedLabel') }}:
              <span class="text-label-large">
                {{
                  report.play_result.headers_applied
                    ? t('onlineSources.debug.yes')
                    : t('onlineSources.debug.no')
                }}
              </span>
            </span>
          </div>
          <div class="text-label-small break-all font-mono text-on-surface-variant">
            {{ t('onlineSources.debug.streamUrlLabel') }}: {{ report.play_result.stream_url }}
          </div>
          <div class="text-label-small break-all font-mono text-on-surface-variant">
            {{ t('onlineSources.debug.proxyStreamUrlLabel') }}:
            {{ report.play_result.proxy_stream_url }}
          </div>
          <div v-if="report.play_result.subtitles.length > 0" class="pt-1">
            <div class="text-label-large text-on-surface">
              {{ t('onlineSources.debug.subtitlesTitle') }}
            </div>
            <ul class="text-label-small mt-1 space-y-1 font-mono text-on-surface-variant">
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
