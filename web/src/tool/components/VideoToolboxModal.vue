<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import {
  LuCheck,
  LuDownload,
  LuFilm,
  LuListOrdered,
  LuLoader2,
  LuSubtitles,
} from 'vue-icons-plus/lu'

import AppCheckbox from '@/components/ui/AppCheckbox.vue'
import AppSelect from '@/components/ui/AppSelect.vue'
import MdDialog from '@/components/ui/MdDialog.vue'
import MdSegmentedButton from '@/components/ui/MdSegmentedButton.vue'
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
const subOffsetMs = ref(0)
const subZhDir = ref<'t2s' | 's2t' | ''>('')
const renameLang = ref('')
const subForced = ref(false)
const extractExt = ref<'srt' | 'ass'>('srt')

// Episode batch remap state
const showTitle = ref('')
const seasonNumber = ref<number | null>(1)
const remapMode = ref<'sequential' | 'absolute_offset'>('sequential')
const startEpisode = ref(1)
const episodeOffset = ref(-24)
const writeSeasonNfo = ref(true)

const tabOptions = computed(() => {
  const list: Array<{ value: 'subtitles' | 'episodes'; label: string; icon: typeof LuFilm }> = []
  if (isSingleMode.value) {
    list.push({ value: 'subtitles', label: '字幕精校 & 抽取', icon: LuSubtitles })
  }
  list.push({ value: 'episodes', label: '剧集/番剧整季映射', icon: LuListOrdered })
  return list
})

const dialogTitle = computed(() =>
  isSingleMode.value && primaryEntry.value
    ? `影视字幕与剧集工具箱 · ${primaryEntry.value.file_name}`
    : `影视字幕与剧集工具箱（已选 ${sortedEntries.value.length} 集视频）`,
)

const zhSubOptions = [
  { value: '', label: '不转换' },
  { value: 't2s', label: '繁 → 简（保留 ASS 特效）' },
  { value: 's2t', label: '简 → 繁（保留 ASS 特效）' },
]
const renameLangOptions = [
  { value: '', label: '保持原文件名' },
  { value: 'zh-CN', label: '.zh-CN（简体中文）' },
  { value: 'zh-TW', label: '.zh-TW（繁体中文）' },
  { value: 'en', label: '.en（英语）' },
  { value: 'ja', label: '.ja（日语）' },
]
const extractExtOptions = [
  { value: 'srt', label: '抽取为 .srt' },
  { value: 'ass', label: '抽取为 .ass' },
]
const remapModeOptions = [
  { value: 'sequential', label: '按文件名自然序顺序重编 (E01, E02...)' },
  { value: 'absolute_offset', label: '番剧绝对集数偏移映射 (如 EP25 - 24 → S02E01)' },
]

async function loadVideoContext(): Promise<void> {
  if (!primaryEntry.value) return
  loading.value = true
  errorMsg.value = ''
  try {
    const [subs, detail] = await Promise.all([
      toolApi.listVideoSubtitles(primaryEntry.value.root_id, primaryEntry.value.rel_path),
      toolApi.inspectFile(primaryEntry.value.root_id, primaryEntry.value.rel_path, 'video'),
    ])
    subtitles.value = subs
    selectedSubName.value = subs[0]?.filename ?? ''
    if (detail.metadata.kind === 'video') {
      embeddedStreams.value = detail.metadata.data.subtitle_streams
      showTitle.value = detail.metadata.data.show_title || detail.metadata.data.title || ''
      seasonNumber.value = detail.metadata.data.season_number ?? 1
    }
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
  } finally {
    loading.value = false
  }
}

watch(
  () => [props.open, props.entries.length, primaryEntry.value?.rel_path],
  ([isOpen]) => {
    if (!isOpen) return
    errorMsg.value = ''
    successMsg.value = ''
    subOffsetMs.value = 0
    activeTab.value = isSingleMode.value ? 'subtitles' : 'episodes'
    if (isSingleMode.value) {
      void loadVideoContext()
    } else if (sortedEntries.value[0]) {
      showTitle.value = sortedEntries.value[0].display_title
    }
  },
  { immediate: true },
)

function toOptInt(val: unknown): number | undefined {
  return typeof val === 'number' && Number.isFinite(val) ? Math.trunc(val) : undefined
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
        offset_ms: toOptInt(subOffsetMs.value) ?? 0,
        zh_direction: subZhDir.value || null,
        rename_language: renameLang.value.trim() || undefined,
        forced: subForced.value,
      },
    )
    subOffsetMs.value = 0
    successMsg.value = `字幕已处理：${res.report.final_filename}（原编码 ${res.report.original_encoding}${res.report.converted_utf8 ? ' → UTF-8' : ''}${res.report.shifted_cues > 0 ? `，平移 ${res.report.shifted_cues} 条时间轴` : ''}）`
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
    successMsg.value = `已抽取内封字幕为 ${res.report.created_filename}（${(res.report.file_size / 1024).toFixed(1)} KB）`
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
        season_number: toOptInt(seasonNumber.value),
        mode: remapMode.value,
        start_episode: toOptInt(startEpisode.value) ?? 1,
        episode_offset: toOptInt(episodeOffset.value) ?? 0,
        write_season_nfo: writeSeasonNfo.value,
      },
    )
    successMsg.value = `已重映射 ${rep.updated_count} 集剧集 NFO${rep.failed_paths.length > 0 ? `（失败 ${rep.failed_paths.length}）` : ''}`
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
    subtitle="外挂字幕转码 / 简繁 / 时间轴平移、内封字幕轨抽取与剧集整季编号映射"
    max-width="max-w-2xl"
    @close="emit('close')"
  >
    <template #icon>
      <LuFilm class="h-5 w-5" />
    </template>

    <div class="space-y-4 py-1 text-xs">
      <MdSegmentedButton v-model="activeTab" :options="tabOptions" size="sm" />

      <!-- TAB 1: Subtitles -->
      <div v-if="activeTab === 'subtitles' && isSingleMode" class="space-y-4">
        <div v-if="loading" class="flex items-center justify-center gap-2 py-8 text-[var(--md-sys-color-on-surface-variant)]">
          <LuLoader2 class="h-4 w-4 animate-spin text-[var(--md-sys-color-primary)]" />
          <span>正在扫描外挂与内封字幕…</span>
        </div>

        <template v-else>
          <!-- Sidecar subtitles -->
          <div class="rounded-2xl border border-[var(--md-sys-color-outline-variant)]/55 bg-[var(--md-sys-color-surface-container-low)] p-4">
            <div class="mb-2 font-semibold">外挂字幕精校（共 {{ subtitles.length }} 条）</div>
            <div v-if="subtitles.length === 0" class="text-[11px] text-[var(--md-sys-color-on-surface-variant)]">
              当前视频目录下未发现伴随外挂字幕（.srt / .ass / .ssa / .vtt）。
            </div>
            <div v-else class="space-y-3">
              <div role="radiogroup" class="space-y-1.5">
                <button
                  v-for="sub in subtitles"
                  :key="sub.filename"
                  type="button"
                  role="radio"
                  :aria-checked="selectedSubName === sub.filename"
                  class="md-state flex w-full items-center justify-between rounded-xl border px-3 py-2 text-left transition"
                  :class="
                    selectedSubName === sub.filename
                      ? 'border-[var(--md-sys-color-primary)] bg-[var(--md-sys-color-primary-container)]/45'
                      : 'border-[var(--md-sys-color-outline-variant)]/60 bg-[var(--md-sys-color-surface)]'
                  "
                  @click="selectedSubName = sub.filename"
                >
                  <div class="flex min-w-0 items-center gap-2.5">
                    <span
                      class="flex h-4 w-4 shrink-0 items-center justify-center rounded-full border-2 transition"
                      :class="
                        selectedSubName === sub.filename
                          ? 'border-[var(--md-sys-color-primary)]'
                          : 'border-[var(--md-sys-color-outline)]'
                      "
                    >
                      <span
                        v-if="selectedSubName === sub.filename"
                        class="h-2 w-2 rounded-full bg-[var(--md-sys-color-primary)]"
                      />
                    </span>
                    <span class="truncate font-medium">{{ sub.filename }}</span>
                  </div>
                  <div class="flex shrink-0 items-center gap-1.5 text-[10px]">
                    <span
                      class="rounded-full px-2 py-0.5 font-medium"
                      :class="
                        sub.is_utf8
                          ? 'bg-[var(--md-sys-color-success-container)] text-[var(--md-sys-color-on-success-container)]'
                          : 'bg-[var(--md-sys-color-warning-container)] text-[var(--md-sys-color-on-warning-container)]'
                      "
                    >
                      {{ sub.detected_encoding }}
                    </span>
                    <span
                      v-if="sub.language_tag"
                      class="rounded-full bg-[var(--md-sys-color-surface-container-high)] px-2 py-0.5 text-[var(--md-sys-color-on-surface-variant)]"
                    >
                      {{ sub.language_tag }}
                    </span>
                  </div>
                </button>
              </div>

              <div class="grid grid-cols-1 gap-3 pt-1 sm:grid-cols-2">
                <AppCheckbox v-model="convertUtf8" label="强制转为无 BOM UTF-8（防乱码）" />
                <div class="inline-flex items-center gap-2">
                  <span class="text-[var(--md-sys-color-on-surface-variant)]">简繁转换</span>
                  <div class="w-48">
                    <AppSelect
                      :model-value="subZhDir"
                      :options="zhSubOptions"
                      variant="compact"
                      @update:model-value="(v) => (subZhDir = v as '' | 't2s' | 's2t')"
                    />
                  </div>
                </div>
                <div class="flex items-center gap-1.5 sm:col-span-2">
                  <span class="text-[var(--md-sys-color-on-surface-variant)]">时间轴平移 (ms)：</span>
                  <button
                    type="button"
                    class="md-btn md-btn-sm md-btn-outlined md-state !h-7 !px-2.5 !text-[11px]"
                    @click="subOffsetMs -= 500"
                  >
                    -0.5s
                  </button>
                  <input
                    v-model.number="subOffsetMs"
                    type="number"
                    step="100"
                    class="md-input h-8 w-24 !px-2 text-center text-xs"
                  />
                  <button
                    type="button"
                    class="md-btn md-btn-sm md-btn-outlined md-state !h-7 !px-2.5 !text-[11px]"
                    @click="subOffsetMs += 500"
                  >
                    +0.5s
                  </button>
                </div>
                <div class="flex flex-wrap items-center gap-2 sm:col-span-2">
                  <span class="text-[var(--md-sys-color-on-surface-variant)]">规范重命名语言后缀：</span>
                  <div class="w-44">
                    <AppSelect v-model="renameLang" :options="renameLangOptions" variant="compact" />
                  </div>
                  <AppCheckbox v-if="renameLang" v-model="subForced" label=".forced 强制字幕" />
                </div>
              </div>

              <div class="flex justify-end">
                <button
                  type="button"
                  :disabled="busy || !selectedSubName"
                  class="md-btn md-btn-sm md-btn-filled md-state"
                  @click="handleProcessSubtitle"
                >
                  <LuLoader2 v-if="busy" class="h-3.5 w-3.5 animate-spin" />
                  <span>应用字幕精校</span>
                </button>
              </div>
            </div>
          </div>

          <!-- Embedded subtitle streams -->
          <div class="rounded-2xl border border-[var(--md-sys-color-outline-variant)]/55 bg-[var(--md-sys-color-surface-container-low)] p-4">
            <div class="mb-2 flex items-center justify-between gap-2">
              <span class="font-semibold">内封字幕流抽取（共 {{ embeddedStreams.length }} 轨）</span>
              <div class="w-32">
                <AppSelect
                  :model-value="extractExt"
                  :options="extractExtOptions"
                  variant="compact"
                  @update:model-value="(v) => (extractExt = v as 'srt' | 'ass')"
                />
              </div>
            </div>
            <div v-if="embeddedStreams.length === 0" class="text-[11px] text-[var(--md-sys-color-on-surface-variant)]">
              该视频文件未检测到内封文本字幕轨。
            </div>
            <div v-else class="space-y-1.5">
              <div
                v-for="st in embeddedStreams"
                :key="st.stream_index"
                class="flex items-center justify-between rounded-xl border border-[var(--md-sys-color-outline-variant)]/55 bg-[var(--md-sys-color-surface)] px-3 py-2"
              >
                <div class="min-w-0">
                  <span class="font-medium">流 #{{ st.stream_index }} · {{ st.codec }}</span>
                  <span class="ml-2 text-[11px] text-[var(--md-sys-color-on-surface-variant)]">
                    {{ st.language || 'und' }}{{ st.title ? ` (${st.title})` : '' }}
                  </span>
                </div>
                <button
                  type="button"
                  :disabled="busy"
                  class="md-btn md-btn-sm md-btn-tonal md-state !h-7 !px-2.5 !text-[11px]"
                  @click="handleExtractStream(st)"
                >
                  <LuDownload class="h-3 w-3" />
                  <span>抽取外挂</span>
                </button>
              </div>
            </div>
          </div>
        </template>
      </div>

      <!-- TAB 2: Batch Episode Remap -->
      <div v-else class="space-y-3">
        <div class="grid grid-cols-1 gap-3 sm:grid-cols-2">
          <label class="block">
            <span class="mb-1 block text-[11px] text-[var(--md-sys-color-on-surface-variant)]">剧集名 (ShowTitle)</span>
            <input v-model="showTitle" type="text" class="md-input h-9 w-full text-xs" />
          </label>
          <label class="block">
            <span class="mb-1 block text-[11px] text-[var(--md-sys-color-on-surface-variant)]">季号 (Season · 0 为特别篇)</span>
            <input v-model.number="seasonNumber" type="number" min="0" class="md-input h-9 w-full text-xs" />
          </label>
        </div>

        <div class="space-y-2.5 rounded-2xl border border-[var(--md-sys-color-outline-variant)]/55 bg-[var(--md-sys-color-surface-container-low)] p-3.5">
          <div class="text-[11px] font-medium text-[var(--md-sys-color-on-surface-variant)]">集数映射策略</div>
          <div class="flex flex-wrap items-center gap-3">
            <div class="w-72">
              <AppSelect
                :model-value="remapMode"
                :options="remapModeOptions"
                variant="compact"
                @update:model-value="(v) => (remapMode = v as 'sequential' | 'absolute_offset')"
              />
            </div>
            <label v-if="remapMode === 'sequential'" class="inline-flex items-center gap-1.5">
              <span>起始集号</span>
              <input v-model.number="startEpisode" type="number" min="1" class="md-input h-8 w-16 !px-2 text-xs" />
            </label>
            <label v-else class="inline-flex items-center gap-1.5">
              <span>集数偏移量</span>
              <input v-model.number="episodeOffset" type="number" class="md-input h-8 w-20 !px-2 text-xs" />
            </label>
          </div>
          <div class="pt-1">
            <AppCheckbox v-model="writeSeasonNfo" label="同时在当前目录生成/更新标准 season.nfo" />
          </div>
        </div>

        <div class="flex justify-end">
          <button
            type="button"
            :disabled="busy || sortedEntries.length === 0"
            class="md-btn md-btn-sm md-btn-filled md-state"
            @click="handleBatchRemap"
          >
            <LuLoader2 v-if="busy" class="h-3.5 w-3.5 animate-spin" />
            <span>应用整季映射（{{ sortedEntries.length }} 集）</span>
          </button>
        </div>
      </div>

      <div
        v-if="errorMsg"
        class="rounded-2xl bg-[var(--md-sys-color-error-container)] px-3.5 py-2.5 text-[var(--md-sys-color-on-error-container)]"
      >
        {{ errorMsg }}
      </div>
      <div
        v-if="successMsg"
        class="flex items-center gap-1.5 rounded-2xl bg-[var(--md-sys-color-success-container)] px-3.5 py-2.5 text-[var(--md-sys-color-on-success-container)]"
      >
        <LuCheck class="h-4 w-4 shrink-0" />
        <span>{{ successMsg }}</span>
      </div>
    </div>

    <template #footer>
      <div class="ml-auto">
        <button type="button" class="md-btn md-btn-sm md-btn-text md-state" @click="emit('close')">
          关闭
        </button>
      </div>
    </template>
  </MdDialog>
</template>
