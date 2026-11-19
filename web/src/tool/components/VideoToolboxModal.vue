<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import {
  LuCheck,
  LuDownload,
  LuFilm,
  LuListOrdered,
  LuLoader2,
  LuSubtitles,
  LuX,
} from 'vue-icons-plus/lu'

import AppCheckbox from '@/components/ui/AppCheckbox.vue'
import AppSelect from '@/components/ui/AppSelect.vue'
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

function handleKeydown(ev: KeyboardEvent): void {
  if (props.open && ev.key === 'Escape') {
    ev.preventDefault()
    emit('close')
  }
}

watch(
  () => props.open,
  (isOpen) => {
    if (isOpen) window.addEventListener('keydown', handleKeydown)
    else window.removeEventListener('keydown', handleKeydown)
  },
  { immediate: true },
)

onBeforeUnmount(() => window.removeEventListener('keydown', handleKeydown))

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
        offset_ms: subOffsetMs.value,
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
        season_number: seasonNumber.value ?? undefined,
        mode: remapMode.value,
        start_episode: startEpisode.value,
        episode_offset: episodeOffset.value,
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
  <Teleport to="body">
    <div
      v-if="open"
      class="fixed inset-0 z-50 flex items-center justify-center bg-black/55 p-4 backdrop-blur-xs"
      @click.self="emit('close')"
    >
      <div
        class="flex max-h-[88vh] w-full max-w-2xl flex-col overflow-hidden rounded-2xl border border-[var(--ctp-surface0)] bg-[var(--ctp-panel)] text-[var(--ctp-text)] shadow-2xl"
      >
        <!-- Header -->
        <div class="flex items-center justify-between border-b border-[var(--ctp-surface0)] px-5 py-3.5">
          <div class="flex items-center gap-2.5">
            <div class="flex h-8 w-8 items-center justify-center rounded-xl bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)]">
              <LuFilm class="h-4 w-4" />
            </div>
            <div>
              <h3 class="text-sm font-semibold">
                影视字幕与剧集工具箱
                <span v-if="isSingleMode && primaryEntry"> · {{ primaryEntry.file_name }}</span>
                <span v-else>（已选 {{ sortedEntries.length }} 集视频）</span>
              </h3>
              <p class="text-[11px] text-[var(--ctp-subtext0)]">
                外挂字幕转 UTF-8 / 简繁 / 时间轴微调 / Jellyfin 语言重命名 · 内封字幕抽取 · 番剧绝对集数映射
              </p>
            </div>
          </div>
          <button
            type="button"
            class="rounded-xl p-1.5 text-[var(--ctp-subtext0)] hover:bg-[var(--ctp-surface0)]"
            @click="emit('close')"
          >
            <LuX class="h-4 w-4" />
          </button>
        </div>

        <!-- Tabs -->
        <div class="flex border-b border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)]/60 px-5 pt-2">
          <button
            v-if="isSingleMode"
            type="button"
            class="inline-flex items-center gap-1.5 border-b-2 px-3 py-2 text-xs font-medium"
            :class="activeTab === 'subtitles' ? 'border-[var(--ctp-accent)] text-[var(--ctp-accent)]' : 'border-transparent text-[var(--ctp-subtext0)]'"
            @click="activeTab = 'subtitles'"
          >
            <LuSubtitles class="h-3.5 w-3.5" />
            字幕精校 & 抽取
          </button>
          <button
            type="button"
            class="inline-flex items-center gap-1.5 border-b-2 px-3 py-2 text-xs font-medium"
            :class="activeTab === 'episodes' ? 'border-[var(--ctp-accent)] text-[var(--ctp-accent)]' : 'border-transparent text-[var(--ctp-subtext0)]'"
            @click="activeTab = 'episodes'"
          >
            <LuListOrdered class="h-3.5 w-3.5" />
            剧集/番剧整季映射
          </button>
        </div>

        <!-- Body -->
        <div class="min-h-0 flex-1 overflow-y-auto p-5 text-xs">
          <!-- TAB 1: Subtitles -->
          <div v-if="activeTab === 'subtitles' && isSingleMode" class="space-y-4">
            <div v-if="loading" class="flex items-center justify-center gap-2 py-8 text-[var(--ctp-subtext0)]">
              <LuLoader2 class="h-4 w-4 animate-spin" />
              正在扫描外挂与内封字幕…
            </div>

            <template v-else>
              <!-- Sidecar subtitles -->
              <div class="rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)]/70 p-4">
                <div class="mb-2 font-semibold">外挂字幕精校（共 {{ subtitles.length }} 条）</div>
                <div v-if="subtitles.length === 0" class="text-[11px] text-[var(--ctp-subtext0)]">
                  当前视频目录下未发现伴随外挂字幕（.srt / .ass / .ssa / .vtt）。
                </div>
                <div v-else class="space-y-3">
                  <div class="space-y-1.5">
                    <label
                      v-for="sub in subtitles"
                      :key="sub.filename"
                      class="flex cursor-pointer items-center justify-between rounded-xl border px-3 py-2 transition"
                      :class="
                        selectedSubName === sub.filename
                          ? 'border-[var(--ctp-accent)] bg-[var(--ctp-accent-soft)]/40'
                          : 'border-[var(--ctp-surface0)] bg-[var(--ctp-panel)]'
                      "
                    >
                      <div class="flex min-w-0 items-center gap-2">
                        <input v-model="selectedSubName" type="radio" :value="sub.filename" />
                        <span class="truncate font-medium">{{ sub.filename }}</span>
                      </div>
                      <div class="flex shrink-0 items-center gap-1.5 text-[10px]">
                        <span
                          class="rounded-md px-1.5 py-0.5 font-medium"
                          :class="sub.is_utf8 ? 'bg-emerald-500/15 text-emerald-500' : 'bg-amber-500/15 text-amber-500'"
                        >
                          {{ sub.detected_encoding }}
                        </span>
                        <span v-if="sub.language_tag" class="rounded-md bg-[var(--ctp-surface0)] px-1.5 py-0.5">
                          {{ sub.language_tag }}
                        </span>
                      </div>
                    </label>
                  </div>

                  <div class="grid grid-cols-1 gap-3 pt-1 sm:grid-cols-2">
                    <AppCheckbox v-model="convertUtf8" label="强制转为无 BOM UTF-8（防乱码）" />
                    <div class="inline-flex items-center gap-2">
                      <span class="text-[var(--ctp-subtext0)]">简繁转换</span>
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
                      <span class="text-[var(--ctp-subtext0)]">时间轴平移 (ms)：</span>
                      <button type="button" class="rounded-lg border border-[var(--ctp-surface0)] px-2 py-1 hover:border-[var(--ctp-accent)]" @click="subOffsetMs -= 500">-0.5s</button>
                      <input v-model.number="subOffsetMs" type="number" step="100" class="w-24 rounded-lg border border-[var(--ctp-surface0)] bg-[var(--ctp-panel)] px-2 py-1 text-center" />
                      <button type="button" class="rounded-lg border border-[var(--ctp-surface0)] px-2 py-1 hover:border-[var(--ctp-accent)]" @click="subOffsetMs += 500">+0.5s</button>
                    </div>
                    <div class="flex flex-wrap items-center gap-2 sm:col-span-2">
                      <span class="text-[var(--ctp-subtext0)]">规范重命名语言后缀：</span>
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
                      class="inline-flex items-center gap-1.5 rounded-xl bg-[var(--ctp-accent)] px-4 py-1.5 text-xs font-medium text-[var(--ctp-on-accent)] disabled:opacity-50"
                      @click="handleProcessSubtitle"
                    >
                      <LuLoader2 v-if="busy" class="h-3.5 w-3.5 animate-spin" />
                      应用字幕精校
                    </button>
                  </div>
                </div>
              </div>

              <!-- Embedded subtitle streams -->
              <div class="rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)]/70 p-4">
                <div class="mb-2 flex items-center justify-between">
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
                <div v-if="embeddedStreams.length === 0" class="text-[11px] text-[var(--ctp-subtext0)]">
                  该视频文件未检测到内封文本字幕轨。
                </div>
                <div v-else class="space-y-1.5">
                  <div
                    v-for="st in embeddedStreams"
                    :key="st.stream_index"
                    class="flex items-center justify-between rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-panel)] px-3 py-2"
                  >
                    <div class="min-w-0">
                      <span class="font-medium">流 #{{ st.stream_index }} · {{ st.codec }}</span>
                      <span class="ml-2 text-[11px] text-[var(--ctp-subtext0)]">
                        {{ st.language || 'und' }}{{ st.title ? ` (${st.title})` : '' }}
                      </span>
                    </div>
                    <button
                      type="button"
                      :disabled="busy"
                      class="inline-flex items-center gap-1 rounded-lg border border-[var(--ctp-surface0)] px-2.5 py-1 text-[11px] hover:border-[var(--ctp-accent)] disabled:opacity-50"
                      @click="handleExtractStream(st)"
                    >
                      <LuDownload class="h-3 w-3" />
                      抽取外挂
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
                <span class="mb-1 block text-[11px] text-[var(--ctp-subtext0)]">剧集名 (ShowTitle)</span>
                <input v-model="showTitle" type="text" class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 outline-none focus:border-[var(--ctp-accent)]" />
              </label>
              <label class="block">
                <span class="mb-1 block text-[11px] text-[var(--ctp-subtext0)]">季号 (Season · 0 为特别篇)</span>
                <input v-model.number="seasonNumber" type="number" min="0" class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 outline-none focus:border-[var(--ctp-accent)]" />
              </label>
            </div>

            <div class="rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)]/70 p-3.5 space-y-2.5">
              <div class="font-medium text-[11px] text-[var(--ctp-subtext0)]">集数映射策略</div>
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
                  <input v-model.number="startEpisode" type="number" min="1" class="w-16 rounded-lg border border-[var(--ctp-surface0)] bg-[var(--ctp-panel)] px-2 py-1" />
                </label>
                <label v-else class="inline-flex items-center gap-1.5">
                  <span>集数偏移量</span>
                  <input v-model.number="episodeOffset" type="number" class="w-20 rounded-lg border border-[var(--ctp-surface0)] bg-[var(--ctp-panel)] px-2 py-1" />
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
                class="inline-flex items-center gap-1.5 rounded-xl bg-[var(--ctp-accent)] px-4 py-1.5 text-xs font-medium text-[var(--ctp-on-accent)] disabled:opacity-50"
                @click="handleBatchRemap"
              >
                <LuLoader2 v-if="busy" class="h-3.5 w-3.5 animate-spin" />
                应用整季映射（{{ sortedEntries.length }} 集）
              </button>
            </div>
          </div>

          <div v-if="errorMsg" class="mt-3 rounded-xl bg-rose-500/10 px-3 py-2 text-rose-500">
            {{ errorMsg }}
          </div>
          <div v-if="successMsg" class="mt-3 flex items-center gap-1.5 rounded-xl bg-emerald-500/10 px-3 py-2 text-emerald-500">
            <LuCheck class="h-4 w-4 shrink-0" />
            <span>{{ successMsg }}</span>
          </div>
        </div>
      </div>
    </div>
  </Teleport>
</template>
