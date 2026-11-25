<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  LuCheck,
  LuDownload,
  LuFilm,
  LuListOrdered,
  LuLoader2,
  LuSubtitles,
} from 'vue-icons-plus/lu'
import AppCheckbox from '@/components/ui/AppCheckbox.vue'
import MdButton from '@/components/ui/MdButton.vue'
import MdDialog from '@/components/ui/MdDialog.vue'
import MdRadio from '@/components/ui/MdRadio.vue'
import MdSelect from '@/components/ui/MdSelect.vue'
import MdTabs from '@/components/ui/MdTabs.vue'
import MdTextField from '@/components/ui/MdTextField.vue'
import { toolApi } from '../api'
import type {
  MediaFileDetail,
  MediaFileEntry,
  ProbedSubtitleStreamDto,
  VideoSubtitleInfoDto,
} from '../types'

const props = defineProps<{
  open: boolean
  rootId: string | null
  entries: MediaFileEntry[]
}>()

const emit = defineEmits<{
  close: []
  updated: [detail: MediaFileDetail]
  completed: []
}>()

const { t } = useI18n()

const isSingleMode = computed(() => props.entries.length === 1)
const primaryEntry = computed(() => props.entries[0] ?? null)
const sortedEntries = computed(() =>
  [...props.entries].sort((a, b) => a.file_name.localeCompare(b.file_name, undefined, { numeric: true })),
)

const activeTab = ref<'subtitles' | 'episodes'>('subtitles')
const loading = ref(false)
const busy = ref(false)
const errorMsg = ref('')
const successMsg = ref('')

// Subtitle state
const subtitles = ref<VideoSubtitleInfoDto[]>([])
const embeddedStreams = ref<ProbedSubtitleStreamDto[]>([])
const selectedSubName = ref('')
const convertUtf8 = ref(true)
const subOffsetMsStr = ref('0')
const subZhDir = ref<'t2s' | 's2t' | ''>('')
const renameLang = ref('')
const subForced = ref(false)
const extractExt = ref<'srt' | 'ass'>('srt')

// Episode batch remap state
const showTitle = ref('')
const seasonNumberStr = ref('1')
const remapMode = ref<'sequential' | 'absolute_offset'>('sequential')
const startEpisodeStr = ref('1')
const episodeOffsetStr = ref('-24')
const writeSeasonNfo = ref(true)

const tabOptions = computed(() => {
  const list: Array<{ value: 'subtitles' | 'episodes'; label: string; icon: typeof LuFilm }> = []
  if (isSingleMode.value) {
    list.push({ value: 'subtitles', label: t('tool.modals.videoTools.tabSubtitles'), icon: LuSubtitles })
  }
  list.push({ value: 'episodes', label: t('tool.modals.videoTools.tabEpisodes'), icon: LuListOrdered })
  return list
})

const dialogTitle = computed(() =>
  isSingleMode.value && primaryEntry.value
    ? t('tool.modals.videoTools.titleSingle', { name: primaryEntry.value.file_name })
    : t('tool.modals.videoTools.titleBatch', { count: sortedEntries.value.length }),
)

const zhSubOptions = computed(() => [
  { value: '', label: t('tool.modals.batchMeta.zhNone') },
  { value: 't2s', label: t('tool.modals.videoTools.zhSubT2s') },
  { value: 's2t', label: t('tool.modals.videoTools.zhSubS2t') },
])

const renameLangOptions = computed(() => [
  { value: '', label: t('tool.modals.videoTools.keepFilename') },
  { value: 'zh-CN', label: '.zh-CN' },
  { value: 'zh-TW', label: '.zh-TW' },
  { value: 'en', label: '.en' },
  { value: 'ja', label: '.ja' },
])

const extractExtOptions = computed(() => [
  { value: 'srt', label: t('tool.modals.videoTools.extractSrt') },
  { value: 'ass', label: t('tool.modals.videoTools.extractAss') },
])

const remapModeOptions = computed(() => [
  { value: 'sequential', label: t('tool.modals.videoTools.remapSequential') },
  { value: 'absolute_offset', label: t('tool.modals.videoTools.remapOffset') },
])

let videoLoadSeq = 0

async function loadVideoContext(): Promise<void> {
  if (!primaryEntry.value) return
  const seq = ++videoLoadSeq
  const target = primaryEntry.value
  loading.value = true
  errorMsg.value = ''
  try {
    const [subs, detail] = await Promise.all([
      toolApi.listVideoSubtitles(target.root_id, target.rel_path),
      toolApi.inspectFile(target.root_id, target.rel_path, 'video'),
    ])
    if (seq !== videoLoadSeq || !props.open) return
    subtitles.value = subs
    selectedSubName.value = subs[0]?.filename ?? ''
    if (detail.metadata.kind === 'video') {
      embeddedStreams.value = detail.metadata.data.subtitle_streams
      showTitle.value = detail.metadata.data.show_title || detail.metadata.data.title || ''
      seasonNumberStr.value = String(detail.metadata.data.season_number ?? 1)
    }
  } catch (err) {
    if (seq === videoLoadSeq && props.open) {
      errorMsg.value = err instanceof Error ? err.message : String(err)
    }
  } finally {
    if (seq === videoLoadSeq) {
      loading.value = false
    }
  }
}

watch(
  () => [props.open, props.entries.length, primaryEntry.value?.rel_path],
  ([isOpen]) => {
    if (!isOpen) {
      videoLoadSeq += 1
      return
    }
    errorMsg.value = ''
    successMsg.value = ''
    subOffsetMsStr.value = '0'
    activeTab.value = isSingleMode.value ? 'subtitles' : 'episodes'
    if (isSingleMode.value) {
      void loadVideoContext()
    } else if (sortedEntries.value[0]) {
      videoLoadSeq += 1
      showTitle.value = sortedEntries.value[0].display_title
    }
  },
  { immediate: true },
)

function toOptInt(raw: string): number | undefined {
  if (!raw.trim()) return undefined
  const parsed = Number.parseInt(raw.trim(), 10)
  return Number.isFinite(parsed) ? parsed : undefined
}

function shiftSubOffset(delta: number): void {
  const cur = toOptInt(subOffsetMsStr.value) ?? 0
  subOffsetMsStr.value = String(cur + delta)
}

async function handleProcessSubtitle(): Promise<void> {
  if (!primaryEntry.value || !selectedSubName.value) return
  busy.value = true
  errorMsg.value = ''
  successMsg.value = ''
  try {
    const res = await toolApi.processVideoSubtitle(
      primaryEntry.value.root_id,
      primaryEntry.value.rel_path,
      {
        subtitle_filename: selectedSubName.value,
        convert_utf8: convertUtf8.value,
        offset_ms: toOptInt(subOffsetMsStr.value) ?? 0,
        zh_direction: subZhDir.value || null,
        rename_language: renameLang.value.trim() || undefined,
        forced: subForced.value,
      },
    )
    subOffsetMsStr.value = '0'
    successMsg.value = t('tool.modals.videoTools.subProcessed', {
      filename: res.report.final_filename,
      encoding: res.report.original_encoding,
    })
    emit('updated', res.detail)
    await loadVideoContext()
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
  } finally {
    busy.value = false
  }
}

async function handleExtractStream(stream: ProbedSubtitleStreamDto): Promise<void> {
  if (!primaryEntry.value) return
  busy.value = true
  errorMsg.value = ''
  successMsg.value = ''
  try {
    const lang = stream.language || 'zh-CN'
    const res = await toolApi.extractVideoSubtitle(
      primaryEntry.value.root_id,
      primaryEntry.value.rel_path,
      stream.stream_index,
      lang,
      extractExt.value,
    )
    successMsg.value = t('tool.modals.videoTools.subExtracted', {
      filename: res.report.created_filename,
      kb: (res.report.file_size / 1024).toFixed(1),
    })
    emit('updated', res.detail)
    await loadVideoContext()
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
  } finally {
    busy.value = false
  }
}

async function handleBatchRemap(): Promise<void> {
  if (!props.rootId || sortedEntries.value.length === 0) return
  busy.value = true
  errorMsg.value = ''
  successMsg.value = ''
  try {
    const rep = await toolApi.batchRemapVideoEpisodes(
      props.rootId,
      sortedEntries.value.map((e) => e.rel_path),
      {
        show_title: showTitle.value.trim() || undefined,
        season_number: toOptInt(seasonNumberStr.value),
        mode: remapMode.value,
        start_episode: toOptInt(startEpisodeStr.value) ?? 1,
        episode_offset: toOptInt(episodeOffsetStr.value) ?? 0,
        write_season_nfo: writeSeasonNfo.value,
      },
    )
    successMsg.value = t('tool.modals.videoTools.episodesRemapped', { count: rep.updated_count })
    emit('completed')
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <MdDialog
    :open="open"
    :title="dialogTitle"
    :subtitle="t('tool.modals.videoTools.subtitle')"
    max-width="max-w-2xl"
    :close-on-backdrop="!busy"
    @close="!busy && emit('close')"
  >
    <template #icon>
      <LuFilm class="h-5 w-5" aria-hidden="true" />
    </template>

    <div class="space-y-4 py-1">
      <MdTabs v-model="activeTab" :tabs="tabOptions" variant="primary" :aria-label="dialogTitle" />

      <!-- TAB 1: Subtitles -->
      <div v-if="activeTab === 'subtitles' && isSingleMode" class="space-y-4">
        <div
          v-if="loading"
          class="text-body-small flex items-center justify-center gap-2 py-8 text-on-surface-variant"
        >
          <LuLoader2 class="h-4 w-4 animate-spin text-primary" aria-hidden="true" />
          <span>{{ t('tool.modals.videoTools.scanningSubs') }}</span>
        </div>

        <template v-else>
          <!-- Sidecar subtitles -->
          <div class="space-y-3 rounded-corner-lg bg-surface p-4">
            <div class="text-title-small text-on-surface">
              {{ t('tool.modals.videoTools.sidecarHeading', { count: subtitles.length }) }}
            </div>
            <div v-if="subtitles.length === 0" class="text-body-small text-on-surface-variant">
              {{ t('tool.modals.videoTools.noSidecarSubs') }}
            </div>
            <div v-else class="space-y-3">
              <div
                role="radiogroup"
                :aria-label="t('tool.modals.videoTools.sidecarHeading', { count: subtitles.length })"
                class="space-y-1.5"
              >
                <div
                  v-for="sub in subtitles"
                  :key="sub.filename"
                  class="flex w-full items-center justify-between gap-2 rounded-corner-md px-3 py-2 transition"
                  :class="
                    selectedSubName === sub.filename
                      ? 'bg-secondary-container/65'
                      : 'bg-surface-container-low'
                  "
                >
                  <MdRadio
                    :checked="selectedSubName === sub.filename"
                    :label="sub.filename"
                    class="min-w-0 flex-1"
                    @select="selectedSubName = sub.filename"
                  />
                  <div class="flex shrink-0 items-center gap-1.5">
                    <span
                      class="text-label-small rounded-corner-full px-2 py-0.5"
                      :class="
                        sub.is_utf8
                          ? 'bg-success-container text-on-success-container'
                          : 'bg-warning-container text-on-warning-container'
                      "
                    >
                      {{ sub.detected_encoding }}
                    </span>
                    <span
                      v-if="sub.language_tag"
                      class="text-label-small rounded-corner-full bg-surface-container-high px-2 py-0.5 text-on-surface-variant"
                    >
                      {{ sub.language_tag }}
                    </span>
                  </div>
                </div>
              </div>

              <div class="grid grid-cols-1 gap-3 pt-1 sm:grid-cols-2">
                <AppCheckbox v-model="convertUtf8" :label="t('tool.modals.videoTools.convertUtf8')" />
                <div class="inline-flex items-center gap-2">
                  <span class="text-label-medium text-on-surface-variant">
                    {{ t('tool.modals.musicTools.zhConvert') }}
                  </span>
                  <div class="w-48">
                    <MdSelect
                      :model-value="subZhDir"
                      :options="zhSubOptions"
                      variant="compact"
                      @update:model-value="(v) => (subZhDir = v as '' | 't2s' | 's2t')"
                    />
                  </div>
                </div>
                <div class="flex flex-wrap items-end gap-2 sm:col-span-2">
                  <div class="w-36">
                    <MdTextField
                      v-model="subOffsetMsStr"
                      size="sm"
                      type="number"
                      step="100"
                      :label="t('tool.modals.videoTools.offsetLabel')"
                    />
                  </div>
                  <MdButton variant="outlined" size="sm" @click="shiftSubOffset(-500)">-0.5s</MdButton>
                  <MdButton variant="outlined" size="sm" @click="shiftSubOffset(500)">+0.5s</MdButton>
                </div>
                <div class="flex flex-wrap items-center gap-2 sm:col-span-2">
                  <span class="text-label-medium text-on-surface-variant">
                    {{ t('tool.modals.videoTools.renameSuffix') }}
                  </span>
                  <div class="w-44">
                    <MdSelect v-model="renameLang" :options="renameLangOptions" variant="compact" />
                  </div>
                  <AppCheckbox
                    v-if="renameLang"
                    v-model="subForced"
                    :label="t('tool.modals.videoTools.forcedSub')"
                  />
                </div>
              </div>

              <div class="flex justify-end">
                <MdButton
                  variant="filled"
                  size="sm"
                  :loading="busy"
                  :disabled="!selectedSubName"
                  @click="handleProcessSubtitle"
                >
                  {{ t('tool.modals.videoTools.applySubBtn') }}
                </MdButton>
              </div>
            </div>
          </div>

          <!-- Embedded subtitle streams -->
          <div class="space-y-2.5 rounded-corner-lg bg-surface p-4">
            <div class="flex items-center justify-between gap-2">
              <span class="text-title-small text-on-surface">
                {{ t('tool.modals.videoTools.embeddedHeading', { count: embeddedStreams.length }) }}
              </span>
              <div class="w-36">
                <MdSelect
                  :model-value="extractExt"
                  :options="extractExtOptions"
                  variant="compact"
                  @update:model-value="(v) => (extractExt = v as 'srt' | 'ass')"
                />
              </div>
            </div>
            <div v-if="embeddedStreams.length === 0" class="text-body-small text-on-surface-variant">
              {{ t('tool.modals.videoTools.noEmbeddedSubs') }}
            </div>
            <div v-else class="space-y-1.5">
              <div
                v-for="st in embeddedStreams"
                :key="st.stream_index"
                class="flex items-center justify-between rounded-corner-md bg-surface-container-low px-3 py-2"
              >
                <div class="min-w-0">
                  <span class="text-body-small font-medium text-on-surface">
                    {{
                      t('tool.modals.videoTools.streamLabel', {
                        index: st.stream_index,
                        codec: st.codec,
                      })
                    }}
                  </span>
                  <span class="text-body-small ml-2 text-on-surface-variant">
                    {{ st.language || 'und' }}{{ st.title ? ` (${st.title})` : '' }}
                  </span>
                </div>
                <MdButton
                  variant="tonal"
                  size="sm"
                  :icon="LuDownload"
                  :disabled="busy"
                  @click="handleExtractStream(st)"
                >
                  {{ t('tool.modals.videoTools.extractBtn') }}
                </MdButton>
              </div>
            </div>
          </div>
        </template>
      </div>

      <!-- TAB 2: Batch Episode Remap -->
      <div v-else class="space-y-3">
        <div class="grid grid-cols-1 gap-3 sm:grid-cols-2">
          <MdTextField
            v-model="showTitle"
            size="sm"
            :label="t('tool.forms.video.showTitle')"
          />
          <MdTextField
            v-model="seasonNumberStr"
            size="sm"
            type="number"
            min="0"
            :label="t('tool.modals.videoTools.seasonLabel')"
          />
        </div>

        <div class="space-y-3 rounded-corner-lg bg-surface p-4">
          <div class="text-title-small text-on-surface">
            {{ t('tool.modals.videoTools.remapStrategy') }}
          </div>
          <div class="grid grid-cols-1 gap-3 sm:grid-cols-12">
            <div class="flex flex-col gap-1 sm:col-span-8">
              <span class="text-label-medium text-on-surface-variant">
                {{ t('tool.modals.videoTools.tabEpisodes') }}
              </span>
              <MdSelect
                :model-value="remapMode"
                :options="remapModeOptions"
                size="sm"
                @update:model-value="(v) => (remapMode = v as 'sequential' | 'absolute_offset')"
              />
            </div>
            <div v-if="remapMode === 'sequential'" class="sm:col-span-4">
              <MdTextField
                v-model="startEpisodeStr"
                size="sm"
                type="number"
                min="1"
                :label="t('tool.modals.videoTools.startEpisode')"
              />
            </div>
            <div v-else class="sm:col-span-4">
              <MdTextField
                v-model="episodeOffsetStr"
                size="sm"
                type="number"
                :label="t('tool.modals.videoTools.episodeOffset')"
              />
            </div>
          </div>
          <AppCheckbox v-model="writeSeasonNfo" :label="t('tool.modals.videoTools.writeSeasonNfo')" />
        </div>

        <div class="flex justify-end">
          <MdButton
            variant="filled"
            size="sm"
            :loading="busy"
            :disabled="sortedEntries.length === 0"
            @click="handleBatchRemap"
          >
            {{ t('tool.modals.videoTools.applyRemapBtn', { count: sortedEntries.length }) }}
          </MdButton>
        </div>
      </div>

      <div
        v-if="errorMsg"
        role="alert"
        class="text-body-small rounded-corner-lg bg-error-container px-3.5 py-2.5 text-on-error-container"
      >
        {{ errorMsg }}
      </div>
      <div
        v-if="successMsg"
        class="text-body-small flex items-center gap-1.5 rounded-corner-lg bg-success-container px-3.5 py-2.5 text-on-success-container"
      >
        <LuCheck class="h-4 w-4 shrink-0" aria-hidden="true" />
        <span>{{ successMsg }}</span>
      </div>
    </div>

    <template #footer>
      <div class="ml-auto">
        <MdButton variant="text" size="sm" @click="emit('close')">
          {{ t('tool.common.close') }}
        </MdButton>
      </div>
    </template>
  </MdDialog>
</template>
