<script setup lang="ts">
import { computed, ref, toRef } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  LuArrowRight,
  LuDisc3,
  LuFileText,
  LuMusic,
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
  buildOnlineMusicCoverUrl,
  onlineMusicApi,
  type MusicSourceSummaryDto,
  type MusicSourceTestInput,
  type MusicSourceTestMode,
  type MusicSourceTestReportDto,
  type OnlineMusicTrackDto,
} from '../onlineApi'

const props = defineProps<{
  open: boolean
  source: MusicSourceSummaryDto | null
}>()

const emit = defineEmits<{
  close: []
  tested: []
}>()

const { t } = useI18n()
const snackbar = useSnackbarStore()

type ResultTab = 'timeline' | 'search' | 'detail' | 'stream' | 'lyric'

const modeOptions = computed<readonly SegmentedOption<MusicSourceTestMode>[]>(() => [
  { value: 'full', label: t('onlineSources.debug.modeFull') },
  { value: 'search', label: t('onlineSources.debug.modeSearch') },
  { value: 'explore', label: t('onlineSources.debug.modeExplore') },
  { value: 'detail', label: t('onlineSources.debug.modeDetail') },
  { value: 'stream', label: t('onlineSources.debug.modeStream') },
  { value: 'lyric', label: t('onlineSources.debug.modeLyric') },
])

const lastTrack = ref<OnlineMusicTrackDto | null>(null)
const lastExploreUrl = ref('')
const qualityInput = ref('')
const pendingPayload = ref<MusicSourceTestInput | null>(null)
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
  MusicSourceSummaryDto,
  MusicSourceTestMode,
  MusicSourceTestReportDto,
  ResultTab
>({
  open: toRef(props, 'open'),
  source: toRef(props, 'source'),
  defaultMode: 'full',
  defaultTab: 'timeline',
  defaultKeyword: 'Mojito',
  runTest: async (sourceId, signal) => {
    const val = queryInput.value.trim()
    let payload: MusicSourceTestInput
    if (pendingPayload.value) {
      payload = pendingPayload.value
    } else if (mode.value === 'full' || mode.value === 'search') {
      payload = {
        mode: mode.value,
        keyword: val || null,
        quality: qualityInput.value.trim() || null,
      }
    } else if (mode.value === 'explore') {
      payload = { mode: 'explore', explore_url: val || null }
    } else {
      const tr = lastTrack.value
      payload = {
        mode: mode.value,
        target_url: val.startsWith('http') ? val : null,
        track_id: tr?.track_id || (!val.startsWith('http') ? val || null : null),
        detail_url: tr?.detail_url ?? null,
        play_url: tr?.play_url ?? null,
        url_id: tr?.url_id ?? null,
        cover_id: tr?.cover_id ?? null,
        lyric_id: tr?.lyric_id ?? null,
        title: tr?.title ?? null,
        artist: tr?.artist ?? null,
        quality: qualityInput.value.trim() || null,
      }
    }
    return onlineMusicApi.testSource(sourceId, payload, { signal })
  },
  onReset: () => {
    lastTrack.value = null
    lastExploreUrl.value = ''
    qualityInput.value = ''
    pendingPayload.value = null
    pendingPreserve.value = false
  },
  onReportReady: (res, preferredTab) => {
    const prev = pendingPreserve.value ? report.value : null
    const ranStages = new Set(res.steps.map((s) => s.stage))
    const merged: MusicSourceTestReportDto = prev
      ? {
          ...res,
          result_count: ranStages.has('search') ? res.result_count : prev.result_count,
          search_items: ranStages.has('search') ? res.search_items : prev.search_items,
          sample_titles: ranStages.has('search') ? res.sample_titles : prev.sample_titles,
          explore_categories: ranStages.has('explore')
            ? res.explore_categories
            : prev.explore_categories,
          resolved_track: res.resolved_track ?? prev.resolved_track,
          stream_probe: res.stream_probe ?? prev.stream_probe,
          lyrics: res.lyrics ?? prev.lyrics,
        }
      : res

    report.value = merged
    if (merged.resolved_track?.track) {
      lastTrack.value = merged.resolved_track.track
    } else if (merged.search_items[0]) {
      lastTrack.value = merged.search_items[0]
    }
    if (merged.explore_categories[0]?.url) {
      lastExploreUrl.value = merged.explore_categories[0].url
    }

    if (preferredTab) {
      activeTab.value = preferredTab
    } else if (res.mode === 'search' || res.mode === 'explore') {
      activeTab.value = res.search_items.length > 0 ? 'search' : 'timeline'
    } else if (res.mode === 'detail') {
      activeTab.value = res.resolved_track ? 'detail' : 'timeline'
    } else if (res.mode === 'stream') {
      activeTab.value = res.stream_probe ? 'stream' : 'timeline'
    } else if (res.mode === 'lyric') {
      activeTab.value = res.lyrics ? 'lyric' : 'timeline'
    } else {
      activeTab.value = 'timeline'
    }
  },
  onTested: () => emit('tested'),
})

function handleModeChange(nextMode: MusicSourceTestMode): void {
  mode.value = nextMode
  if (nextMode === 'explore' && lastExploreUrl.value) {
    queryInput.value = lastExploreUrl.value
  } else if (
    (nextMode === 'detail' || nextMode === 'stream' || nextMode === 'lyric') &&
    lastTrack.value
  ) {
    queryInput.value = lastTrack.value.detail_url || lastTrack.value.track_id || lastTrack.value.title
  } else if ((nextMode === 'full' || nextMode === 'search') && props.source) {
    queryInput.value = props.source.check_keyword?.trim() || 'Mojito'
  }
}

const hasQuickHint = computed(() => {
  if ((mode.value === 'full' || mode.value === 'search') && props.source?.check_keyword) return true
  if (mode.value === 'explore' && Boolean(lastExploreUrl.value)) return true
  return false
})

const inputPlaceholder = computed(() => {
  const kw = props.source?.check_keyword?.trim() || 'Mojito'
  if (mode.value === 'full' || mode.value === 'search') {
    return t('onlineSources.debug.keywordPlaceholder', { keyword: kw })
  }
  return t('onlineSources.debug.urlPlaceholder')
})

const domainLabel = computed(() => t('onlineSources.domainLabel.music'))

const resultTabs = computed<DebugResultTabItem<ResultTab>[]>(() => [
  { id: 'timeline', label: t('onlineSources.debug.tabTimeline'), icon: LuTerminal },
  {
    id: 'search',
    label: t('onlineSources.debug.tabSearch', { count: report.value?.result_count ?? 0 }),
    icon: LuSearch,
  },
  { id: 'detail', label: t('onlineSources.debug.tabDetail'), icon: LuDisc3 },
  { id: 'stream', label: t('onlineSources.debug.tabStream'), icon: LuMusic },
  { id: 'lyric', label: t('onlineSources.debug.tabLyric'), icon: LuFileText },
])

function stageToTab(stage: string): ResultTab {
  if (stage === 'search' || stage === 'explore') return 'search'
  if (stage === 'detail' || stage === 'cover') return 'detail'
  if (stage === 'stream') return 'stream'
  if (stage === 'lyric') return 'lyric'
  return 'timeline'
}

async function handleRun(): Promise<void> {
  pendingPayload.value = null
  pendingPreserve.value = mode.value !== 'full' && mode.value !== 'search'
  await executeTest()
}

async function handleDebugTrack(track: OnlineMusicTrackDto): Promise<void> {
  lastTrack.value = track
  mode.value = 'full'
  pendingPayload.value = {
    mode: 'full',
    keyword: track.title,
    track_id: track.track_id,
    detail_url: track.detail_url,
    play_url: track.play_url,
    url_id: track.url_id,
    cover_id: track.cover_id,
    lyric_id: track.lyric_id,
    title: track.title,
    artist: track.artist,
    quality: qualityInput.value.trim() || null,
  }
  pendingPreserve.value = true
  await executeTest('detail')
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
    <template #extra-controls>
      <div class="flex items-center gap-2 text-xs">
        <span class="text-[var(--md-sys-color-on-surface-variant)]">
          {{ t('onlineSources.debug.qualityLabel') }}
        </span>
        <input
          v-model="qualityInput"
          type="text"
          placeholder="320 / 999 / flac"
          class="md-input w-40 py-1 font-mono text-xs"
        />
      </div>
    </template>

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
            :key="`${item.track_id}-${idx}`"
            class="flex items-start gap-3 rounded-2xl bg-[var(--md-sys-color-surface-container)] p-3.5"
          >
            <div class="h-14 w-14 shrink-0 overflow-hidden rounded-xl bg-[var(--md-sys-color-surface-container-high)]">
              <img
                v-if="item.proxy_cover_url || item.cover_url"
                :src="item.proxy_cover_url ? withAuthToken(item.proxy_cover_url) : buildOnlineMusicCoverUrl(item.cover_url, source?.id)"
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
                <div class="min-w-0">
                  <div class="flex flex-wrap items-center gap-1.5">
                    <span class="font-mono text-[11px] text-[var(--md-sys-color-on-surface-variant)]">
                      #{{ idx + 1 }}
                    </span>
                    <span class="text-sm font-semibold text-[var(--md-sys-color-on-surface)]">
                      {{ item.title }}
                    </span>
                    <span v-if="item.artist" class="text-xs text-[var(--md-sys-color-on-surface-variant)]">
                      · {{ item.artist }}
                    </span>
                  </div>
                  <p v-if="item.album" class="text-xs text-[var(--md-sys-color-on-surface-variant)]">
                    {{ item.album }}
                  </p>
                </div>
                <button
                  type="button"
                  :disabled="running"
                  class="md-btn md-btn-sm md-btn-tonal md-state shrink-0"
                  @click="handleDebugTrack(item)"
                >
                  <span>{{ t('onlineSources.debug.debugTrackBtn') }}</span>
                  <LuArrowRight class="h-3.5 w-3.5" />
                </button>
              </div>
            </div>
          </div>
        </template>
      </div>

      <!-- Resolved Track Detail -->
      <div v-else-if="activeTab === 'detail' && report" class="flex flex-col gap-3">
        <div
          v-if="!report.resolved_track"
          class="rounded-2xl bg-[var(--md-sys-color-surface-container)] py-10 text-center text-xs text-[var(--md-sys-color-on-surface-variant)]"
        >
          {{ t('onlineSources.debug.emptyDetail') }}
        </div>
        <div
          v-else
          class="flex flex-col gap-3 rounded-2xl bg-[var(--md-sys-color-surface-container)] p-4"
        >
          <div class="flex items-center gap-3">
            <img
              v-if="report.resolved_track.proxy_cover_url || report.resolved_track.cover_url"
              :src="
                report.resolved_track.proxy_cover_url
                  ? withAuthToken(report.resolved_track.proxy_cover_url)
                  : buildOnlineMusicCoverUrl(report.resolved_track.cover_url, source?.id)
              "
              :alt="report.resolved_track.track.title"
              class="h-16 w-16 rounded-xl object-cover"
            />
            <div class="min-w-0">
              <div class="text-sm font-semibold text-[var(--md-sys-color-on-surface)]">
                {{ report.resolved_track.track.title }}
              </div>
              <div class="text-xs text-[var(--md-sys-color-on-surface-variant)]">
                {{ report.resolved_track.track.artist }}
                <template v-if="report.resolved_track.track.album">
                  · {{ report.resolved_track.track.album }}
                </template>
              </div>
              <div class="mt-1 flex flex-wrap gap-2 font-mono text-[11px] text-[var(--md-sys-color-on-surface-variant)]">
                <span v-if="report.resolved_track.format">{{ report.resolved_track.format }}</span>
                <span v-if="report.resolved_track.bitrate">{{ report.resolved_track.bitrate }} kbps</span>
              </div>
            </div>
          </div>
        </div>
      </div>

      <!-- Stream Probe & Audio Preview -->
      <div v-else-if="activeTab === 'stream' && report" class="flex flex-col gap-3">
        <div
          v-if="!report.stream_probe"
          class="rounded-2xl bg-[var(--md-sys-color-surface-container)] py-10 text-center text-xs text-[var(--md-sys-color-on-surface-variant)]"
        >
          {{ t('onlineSources.debug.emptyStream') }}
        </div>
        <div
          v-else
          class="flex flex-col gap-3 rounded-2xl bg-[var(--md-sys-color-surface-container)] p-4 text-xs"
        >
          <audio
            v-if="report.stream_probe.proxy_stream_url"
            controls
            preload="none"
            :src="withAuthToken(report.stream_probe.proxy_stream_url)"
            class="w-full"
          />
          <div class="grid grid-cols-1 gap-2 sm:grid-cols-2">
            <div>
              <span class="text-[var(--md-sys-color-on-surface-variant)]">
                {{ t('onlineSources.debug.formatLabel') }}:
              </span>
              <span class="font-mono">{{ report.stream_probe.format || '-' }}</span>
            </div>
            <div>
              <span class="text-[var(--md-sys-color-on-surface-variant)]">
                {{ t('onlineSources.debug.bitrateLabel') }}:
              </span>
              <span class="font-mono">
                {{ report.stream_probe.bitrate ? `${report.stream_probe.bitrate} kbps` : '-' }}
              </span>
            </div>
            <div>
              <span class="text-[var(--md-sys-color-on-surface-variant)]">
                {{ t('onlineSources.debug.expiresLabel') }}:
              </span>
              <span class="font-mono">
                {{
                  report.stream_probe.expires_in_secs != null
                    ? `${report.stream_probe.expires_in_secs}s`
                    : '-'
                }}
              </span>
            </div>
            <div>
              <span class="text-[var(--md-sys-color-on-surface-variant)]">
                {{ t('onlineSources.debug.headersAppliedLabel') }}:
              </span>
              <span>
                {{
                  report.stream_probe.headers_applied
                    ? t('onlineSources.debug.yes')
                    : t('onlineSources.debug.no')
                }}
              </span>
            </div>
          </div>
          <div class="break-all font-mono text-[11px] text-[var(--md-sys-color-on-surface-variant)]">
            {{ t('onlineSources.debug.streamUrlLabel') }}: {{ report.stream_probe.stream_url }}
          </div>
          <div class="break-all font-mono text-[11px] text-[var(--md-sys-color-on-surface-variant)]">
            {{ t('onlineSources.debug.proxyStreamUrlLabel') }}: {{ report.stream_probe.proxy_stream_url }}
          </div>
        </div>
      </div>

      <!-- Lyrics -->
      <div v-else-if="activeTab === 'lyric' && report" class="flex flex-col gap-3">
        <div
          v-if="!report.lyrics"
          class="rounded-2xl bg-[var(--md-sys-color-surface-container)] py-10 text-center text-xs text-[var(--md-sys-color-on-surface-variant)]"
        >
          {{ t('onlineSources.debug.emptyLyric') }}
        </div>
        <div
          v-else
          class="flex flex-col gap-3 rounded-2xl bg-[var(--md-sys-color-surface-container)] p-4"
        >
          <div class="flex items-center gap-2">
            <span class="rounded-full bg-[var(--md-sys-color-secondary-container)] px-2.5 py-0.5 text-[11px] font-medium text-[var(--md-sys-color-on-secondary-container)]">
              {{
                report.lyrics.synced
                  ? t('onlineSources.debug.syncedLyricsBadge')
                  : t('onlineSources.debug.plainLyricsBadge')
              }}
            </span>
            <span v-if="report.lyrics.display_title" class="text-xs font-medium">
              {{ report.lyrics.display_title }}
            </span>
          </div>
          <div v-if="report.lyrics.lines.length > 0" class="max-h-72 space-y-1 overflow-y-auto font-mono text-xs">
            <div v-for="(line, lIdx) in report.lyrics.lines" :key="lIdx" class="flex gap-2">
              <span class="shrink-0 text-[var(--md-sys-color-on-surface-variant)]">
                [{{ Math.floor(line.start_ms / 1000) }}s]
              </span>
              <span>{{ line.text }}</span>
            </div>
          </div>
          <pre v-else class="max-h-72 overflow-y-auto whitespace-pre-wrap text-xs">
            {{ report.lyrics.plain_text }}
          </pre>
        </div>
      </div>
    </template>
  </OnlineSourceDebugShell>
</template>
