<script setup lang="ts">
import { computed, onBeforeUnmount, ref, toRef, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  LuArrowRight,
  LuDisc3,
  LuFileText,
  LuMusic,
  LuPause,
  LuPlay,
  LuSearch,
  LuTerminal,
} from 'vue-icons-plus/lu'
import OnlineSourceDebugShell, {
  type DebugResultTabItem,
} from '@/components/online-sources/OnlineSourceDebugShell.vue'
import { useOnlineSourceDebug } from '@/components/online-sources/useOnlineSourceDebug'
import { AppSlider, MdButton, MdIconButton, type SegmentedOption } from '@/components/ui'
import MediaCoverImage from '@/components/ui/MediaCoverImage.vue'
import { useSnackbarStore } from '@/stores/snackbar'
import { formatTime } from '@/utils/format'
import { ensureMusicLocale, useMusicI18n } from '../i18n'
import {
  onlineMusicApi,
  resolveOnlineMusicAudioUrl,
  resolveOnlineMusicCoverUrl,
  type MusicSourceSummaryDto,
  type MusicSourceTestInput,
  type MusicSourceTestMode,
  type MusicSourceTestReportDto,
  type OnlineMusicTrackDto,
} from '../onlineApi'
import { usePlayerStore } from '../player'

const props = defineProps<{
  open: boolean
  source: MusicSourceSummaryDto | null
}>()

const emit = defineEmits<{
  close: []
  tested: []
}>()

const { t } = useI18n()
const { t: tm } = useMusicI18n()
const snackbar = useSnackbarStore()
const playerStore = usePlayerStore()

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

// Custom MD3 audio preview state (mutually exclusive with global playerStore)
const previewAudioRef = ref<HTMLAudioElement | null>(null)
const previewPlaying = ref(false)
const previewCurrentSecs = ref(0)
const previewDurationSecs = ref(0)
const previewScrubSecs = ref<number | null>(null)
const previewError = ref<string | null>(null)

function stopPreviewAudio(): void {
  const el = previewAudioRef.value
  if (el) {
    el.pause()
    el.currentTime = 0
  }
  previewPlaying.value = false
  previewCurrentSecs.value = 0
  previewDurationSecs.value = 0
  previewScrubSecs.value = null
  previewError.value = null
}

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
  defaultKeyword: '',
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
    stopPreviewAudio()
    lastTrack.value = null
    lastExploreUrl.value = ''
    qualityInput.value = ''
    pendingPayload.value = null
    pendingPreserve.value = false
  },
  onReportReady: (res, preferredTab) => {
    stopPreviewAudio()
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

watch(
  () => props.open,
  (isOpen) => {
    if (isOpen) {
      void ensureMusicLocale()
    } else {
      stopPreviewAudio()
    }
  },
  { immediate: true },
)

watch(
  () => playerStore.isPlaying,
  (globalPlaying) => {
    if (globalPlaying && previewPlaying.value && previewAudioRef.value) {
      previewAudioRef.value.pause()
    }
  },
)

watch(activeTab, (nextTab) => {
  if (nextTab !== 'stream' && previewPlaying.value && previewAudioRef.value) {
    previewAudioRef.value.pause()
  }
})

const showQualityControl = computed(
  () => mode.value === 'full' || mode.value === 'search' || mode.value === 'stream',
)

const resolvedDetailCoverUrl = computed(() => {
  const tr = report.value?.resolved_track
  if (!tr) return ''
  return resolveOnlineMusicCoverUrl(tr.proxy_cover_url, tr.cover_url, props.source?.id)
})

const previewAudioUrl = computed(() => {
  const probe = report.value?.stream_probe
  if (!probe) return ''
  return resolveOnlineMusicAudioUrl(probe.proxy_stream_url)
})

watch(previewAudioUrl, () => {
  stopPreviewAudio()
})

function searchItemCoverUrl(item: OnlineMusicTrackDto): string {
  return resolveOnlineMusicCoverUrl(item.proxy_cover_url, item.cover_url, props.source?.id)
}

async function togglePreviewPlay(): Promise<void> {
  const el = previewAudioRef.value
  if (!el || !previewAudioUrl.value) return
  if (previewPlaying.value) {
    el.pause()
    return
  }
  previewError.value = null
  playerStore.pause()
  try {
    await el.play()
  } catch (err) {
    if (err instanceof DOMException && err.name === 'AbortError') return
    previewPlaying.value = false
    previewError.value = err instanceof Error && err.message ? err.message : tm('sourceEditor.debug.previewError')
  }
}

function seekPreview(secs: number): void {
  const el = previewAudioRef.value
  if (!el) return
  el.currentTime = Math.max(0, secs)
  previewCurrentSecs.value = el.currentTime
}

function onPreviewTimeUpdate(): void {
  const el = previewAudioRef.value
  if (!el) return
  previewCurrentSecs.value = el.currentTime || 0
}

function onPreviewLoadedMetadata(): void {
  const el = previewAudioRef.value
  if (!el) return
  previewDurationSecs.value = Number.isFinite(el.duration) ? el.duration : 0
}

function onPreviewError(): void {
  previewPlaying.value = false
  previewError.value = tm('sourceEditor.debug.previewError')
}

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
    queryInput.value = props.source.check_keyword?.trim() || ''
  }
}

const hasQuickHint = computed(() => {
  if ((mode.value === 'full' || mode.value === 'search') && props.source?.check_keyword) return true
  if (mode.value === 'explore' && Boolean(lastExploreUrl.value)) return true
  return false
})

const inputPlaceholder = computed(() => {
  const kw = props.source?.check_keyword?.trim()
  if (mode.value === 'full' || mode.value === 'search') {
    return kw
      ? t('onlineSources.debug.keywordPlaceholder', { keyword: kw })
      : tm('sourceEditor.debug.keywordEmptyPlaceholder')
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

onBeforeUnmount(() => {
  stopPreviewAudio()
})
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
    <template v-if="showQualityControl" #extra-controls>
      <div class="text-body-small flex items-center gap-2">
        <span class="text-on-surface-variant">
          {{ t('onlineSources.debug.qualityLabel') }}
        </span>
        <input
          v-model="qualityInput"
          type="text"
          placeholder="320 / 999 / flac"
          class="md-input text-body-small w-40 py-1 font-mono"
        />
      </div>
    </template>

    <template #quick-hints>
      <button
        v-if="source?.check_keyword && (mode === 'full' || mode === 'search')"
        type="button"
        class="md-state text-label-small cursor-pointer rounded-corner-full bg-surface-container-high px-2.5 py-1 text-on-surface"
        @click="queryInput = source.check_keyword!"
      >
        {{ t('onlineSources.debug.checkKeywordHint', { keyword: source.check_keyword }) }}
      </button>
      <button
        v-if="lastExploreUrl && mode === 'explore'"
        type="button"
        class="md-state text-label-small max-w-xs cursor-pointer truncate rounded-corner-full bg-surface-container-high px-2.5 py-1 font-mono text-on-surface"
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
            :key="`${item.track_id}-${idx}`"
            class="flex items-start gap-3 rounded-corner-lg bg-surface-container p-3.5"
          >
            <MediaCoverImage
              :src="searchItemCoverUrl(item) || null"
              :alt="item.title"
              aspect-class="h-14 w-14 shrink-0"
              rounded="rounded-corner-md"
              :fallback-icon="LuDisc3"
            />

            <div class="min-w-0 flex-1 space-y-1">
              <div class="flex flex-wrap items-start justify-between gap-2">
                <div class="min-w-0">
                  <div class="flex flex-wrap items-center gap-1.5">
                    <span class="text-label-small font-mono text-on-surface-variant">
                      #{{ idx + 1 }}
                    </span>
                    <span class="text-title-small text-on-surface">
                      {{ item.title }}
                    </span>
                    <span v-if="item.artist" class="text-body-small text-on-surface-variant">
                      · {{ item.artist }}
                    </span>
                  </div>
                  <p v-if="item.album" class="text-body-small text-on-surface-variant">
                    {{ item.album }}
                  </p>
                </div>
                <MdButton
                  variant="tonal"
                  size="sm"
                  :disabled="running"
                  class="shrink-0"
                  @click="handleDebugTrack(item)"
                >
                  <span>{{ t('onlineSources.debug.debugTrackBtn') }}</span>
                  <template #trailing>
                    <LuArrowRight class="h-3.5 w-3.5" aria-hidden="true" />
                  </template>
                </MdButton>
              </div>
            </div>
          </div>
        </template>
      </div>

      <!-- Resolved Track Detail -->
      <div v-else-if="activeTab === 'detail' && report" class="flex flex-col gap-3">
        <div
          v-if="!report.resolved_track"
          class="text-body-small rounded-corner-lg bg-surface-container py-10 text-center text-on-surface-variant"
        >
          {{ t('onlineSources.debug.emptyDetail') }}
        </div>
        <div v-else class="flex flex-col gap-3 rounded-corner-lg bg-surface-container p-4">
          <div class="flex items-center gap-3.5">
            <MediaCoverImage
              :src="resolvedDetailCoverUrl || null"
              :alt="report.resolved_track.track.title"
              aspect-class="h-16 w-16 shrink-0"
              rounded="rounded-corner-md"
              :fallback-icon="LuDisc3"
            />
            <div class="min-w-0">
              <div class="text-title-medium truncate text-on-surface">
                {{ report.resolved_track.track.title }}
              </div>
              <div class="text-body-small text-on-surface-variant">
                {{ report.resolved_track.track.artist }}
                <template v-if="report.resolved_track.track.album">
                  · {{ report.resolved_track.track.album }}
                </template>
              </div>
              <div class="text-label-small mt-1 flex flex-wrap gap-2 font-mono text-on-surface-variant">
                <span v-if="report.resolved_track.format">{{ report.resolved_track.format }}</span>
                <span v-if="report.resolved_track.bitrate">
                  {{ report.resolved_track.bitrate }} kbps
                </span>
              </div>
            </div>
          </div>
        </div>
      </div>

      <!-- Stream Probe & Custom MD3 Audio Preview -->
      <div v-else-if="activeTab === 'stream' && report" class="flex flex-col gap-3">
        <div
          v-if="!report.stream_probe"
          class="text-body-small rounded-corner-lg bg-surface-container py-10 text-center text-on-surface-variant"
        >
          {{ t('onlineSources.debug.emptyStream') }}
        </div>
        <div v-else class="flex flex-col gap-4 rounded-corner-lg bg-surface-container p-4">
          <!-- Custom MD3 Preview Player (auto-pauses global playerStore to avoid overlap) -->
          <div
            v-if="previewAudioUrl"
            class="flex flex-col gap-2 rounded-corner-md bg-surface-container-high p-3"
          >
            <audio
              ref="previewAudioRef"
              :src="previewAudioUrl"
              preload="none"
              class="hidden"
              @play="previewPlaying = true"
              @pause="previewPlaying = false"
              @ended="previewPlaying = false"
              @timeupdate="onPreviewTimeUpdate"
              @loadedmetadata="onPreviewLoadedMetadata"
              @error="onPreviewError"
            />

            <div class="flex items-center gap-3">
              <MdIconButton
                variant="filled"
                size="sm"
                :aria-label="
                  previewPlaying
                    ? tm('sourceEditor.debug.previewPause')
                    : tm('sourceEditor.debug.previewPlay')
                "
                :title="
                  previewPlaying
                    ? tm('sourceEditor.debug.previewPause')
                    : tm('sourceEditor.debug.previewPlay')
                "
                @click="togglePreviewPlay"
              >
                <LuPause v-if="previewPlaying" class="h-4 w-4 fill-current" />
                <LuPlay v-else class="ml-0.5 h-4 w-4 fill-current" />
              </MdIconButton>

              <span class="text-label-small w-10 text-right font-mono tabular-nums text-on-surface-variant">
                {{ formatTime(previewScrubSecs ?? previewCurrentSecs) }}
              </span>

              <div class="min-w-0 flex-1">
                <AppSlider
                  :model-value="previewCurrentSecs"
                  :min="0"
                  :max="Math.max(1, previewDurationSecs)"
                  :step="0.2"
                  :disabled="previewDurationSecs <= 0"
                  commit-on-release
                  :always-show-thumb="false"
                  :aria-label="tm('sourceEditor.debug.previewTitle')"
                  @preview="previewScrubSecs = $event"
                  @commit="seekPreview"
                />
              </div>

              <span class="text-label-small w-10 font-mono tabular-nums text-on-surface-variant">
                {{ formatTime(previewDurationSecs) }}
              </span>
            </div>

            <p v-if="previewError" role="alert" class="text-body-small text-error">
              {{ previewError }}
            </p>
          </div>

          <div class="text-body-small grid grid-cols-1 gap-2 sm:grid-cols-2">
            <div>
              <span class="text-on-surface-variant">
                {{ t('onlineSources.debug.formatLabel') }}:
              </span>
              <span class="ml-1 font-mono text-on-surface">
                {{ report.stream_probe.format || '-' }}
              </span>
            </div>
            <div>
              <span class="text-on-surface-variant">
                {{ t('onlineSources.debug.bitrateLabel') }}:
              </span>
              <span class="ml-1 font-mono text-on-surface">
                {{ report.stream_probe.bitrate ? `${report.stream_probe.bitrate} kbps` : '-' }}
              </span>
            </div>
            <div>
              <span class="text-on-surface-variant">
                {{ t('onlineSources.debug.expiresLabel') }}:
              </span>
              <span class="ml-1 font-mono text-on-surface">
                {{
                  report.stream_probe.expires_in_secs != null
                    ? `${report.stream_probe.expires_in_secs}s`
                    : '-'
                }}
              </span>
            </div>
            <div>
              <span class="text-on-surface-variant">
                {{ t('onlineSources.debug.headersAppliedLabel') }}:
              </span>
              <span class="ml-1 text-on-surface">
                {{
                  report.stream_probe.headers_applied
                    ? t('onlineSources.debug.yes')
                    : t('onlineSources.debug.no')
                }}
              </span>
            </div>
          </div>

          <div class="text-label-small space-y-1 break-all font-mono text-on-surface-variant">
            <div>
              {{ t('onlineSources.debug.streamUrlLabel') }}: {{ report.stream_probe.stream_url }}
            </div>
            <div>
              {{ t('onlineSources.debug.proxyStreamUrlLabel') }}:
              {{ report.stream_probe.proxy_stream_url }}
            </div>
          </div>
        </div>
      </div>

      <!-- Lyrics -->
      <div v-else-if="activeTab === 'lyric' && report" class="flex flex-col gap-3">
        <div
          v-if="!report.lyrics"
          class="text-body-small rounded-corner-lg bg-surface-container py-10 text-center text-on-surface-variant"
        >
          {{ t('onlineSources.debug.emptyLyric') }}
        </div>
        <div v-else class="flex flex-col gap-3 rounded-corner-lg bg-surface-container p-4">
          <div class="flex items-center gap-2">
            <span
              class="text-label-small rounded-corner-full bg-secondary-container px-2.5 py-0.5 text-on-secondary-container"
            >
              {{
                report.lyrics.synced
                  ? t('onlineSources.debug.syncedLyricsBadge')
                  : t('onlineSources.debug.plainLyricsBadge')
              }}
            </span>
            <span v-if="report.lyrics.display_title" class="text-body-small text-on-surface">
              {{ report.lyrics.display_title }}
            </span>
          </div>
          <div
            v-if="report.lyrics.lines.length > 0"
            class="text-body-small max-h-72 space-y-1 overflow-y-auto font-mono"
          >
            <div v-for="(line, lIdx) in report.lyrics.lines" :key="lIdx" class="flex gap-2">
              <span class="shrink-0 tabular-nums text-on-surface-variant">
                [{{ Math.floor(line.start_ms / 1000) }}s]
              </span>
              <span class="text-on-surface">{{ line.text }}</span>
            </div>
          </div>
          <pre v-else class="text-body-small max-h-72 overflow-y-auto whitespace-pre-wrap text-on-surface">
            {{ report.lyrics.plain_text }}
          </pre>
        </div>
      </div>
    </template>
  </OnlineSourceDebugShell>
</template>
