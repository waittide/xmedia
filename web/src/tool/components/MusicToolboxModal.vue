<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import {
  LuCheck,
  LuDisc3,
  LuImage,
  LuLoader2,
  LuMusic,
  LuScissors,
  LuSlidersHorizontal,
  LuVolume2,
  LuX,
} from 'vue-icons-plus/lu'

import AppCheckbox from '@/components/ui/AppCheckbox.vue'
import AppSelect from '@/components/ui/AppSelect.vue'
import { toolApi } from '../api'
import type { MediaFileDetail, MediaFileEntry } from '../types'

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

const activeTab = ref<'lyrics' | 'audio' | 'album'>('lyrics')
const busy = ref(false)
const errorMsg = ref('')
const successMsg = ref('')

// Lyrics state
const lyricsText = ref('')
const offsetMs = ref(0)
const stripCreditNoise = ref(true)
const zhDirection = ref<'t2s' | 's2t' | ''>('')
const syncMode = ref<'both' | 'sidecar_to_embedded' | 'embedded_to_sidecar' | 'none'>('both')

// Cover optimize state
const coverMaxDimStr = ref('1000')
const coverQuality = ref(86)

const zhOptions = [
  { value: '', label: '不转换' },
  { value: 't2s', label: '繁 → 简' },
  { value: 's2t', label: '简 → 繁' },
]
const syncModeOptions = [
  { value: 'both', label: '同时同步内嵌 + 侧车 .lrc' },
  { value: 'sidecar_to_embedded', label: '仅写入音频内嵌标签' },
  { value: 'embedded_to_sidecar', label: '仅导出侧车 .lrc 文件' },
]
const coverDimOptions = [
  { value: '1200', label: '1200 px' },
  { value: '1000', label: '1000 px（推荐）' },
  { value: '800', label: '800 px' },
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

// Album batch sync state
const albumName = ref('')
const albumArtist = ref('')
const trackArtist = ref('')
const albumYear = ref<number | null>(null)
const albumGenre = ref('')
const discNumber = ref<number | null>(1)
const autoNumberTracks = ref(true)
const startTrack = ref(1)

watch(
  () => [props.open, props.entries.length, primaryEntry.value?.rel_path],
  async ([isOpen]) => {
    if (!isOpen) return
    errorMsg.value = ''
    successMsg.value = ''
    offsetMs.value = 0
    activeTab.value = isSingleMode.value ? 'lyrics' : 'album'

    if (isSingleMode.value && primaryEntry.value) {
      try {
        const detail = await toolApi.inspectFile(
          primaryEntry.value.root_id,
          primaryEntry.value.rel_path,
          'music',
        )
        if (detail.metadata.kind === 'music') {
          lyricsText.value =
            detail.metadata.data.sidecar_lrc || detail.metadata.data.embedded_lyrics || ''
          albumName.value = detail.metadata.data.album || ''
          albumArtist.value = detail.metadata.data.album_artist || detail.metadata.data.artist || ''
          albumYear.value = detail.metadata.data.year ?? null
          albumGenre.value = detail.metadata.data.genre || ''
        }
      } catch {
        // ignore preview error
      }
    }
  },
  { immediate: true },
)

async function handleProcessLyrics(): Promise<void> {
  if (!primaryEntry.value) return
  busy.value = true
  errorMsg.value = ''
  successMsg.value = ''
  try {
    const res = await toolApi.runMusicLyricsTool(
      primaryEntry.value.root_id,
      primaryEntry.value.rel_path,
      {
        offset_ms: offsetMs.value,
        strip_credit_noise: stripCreditNoise.value,
        zh_direction: zhDirection.value || null,
        sync_mode: syncMode.value,
        custom_text: lyricsText.value.trim() ? lyricsText.value : undefined,
      },
    )
    lyricsText.value = res.report.final_lyrics
    offsetMs.value = 0
    const targets: string[] = []
    if (res.report.updated_embedded) targets.push('内嵌歌词')
    if (res.report.updated_sidecar) targets.push('侧车 .lrc')
    successMsg.value = `歌词已更新至 ${targets.join(' + ')}${res.report.removed_lines > 0 ? `，清理 ${res.report.removed_lines} 行制作/版权噪声` : ''}`
    emit('updated', res.detail)
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
  } finally {
    busy.value = false
  }
}

async function handleOptimizeCover(): Promise<void> {
  if (!primaryEntry.value) return
  busy.value = true
  errorMsg.value = ''
  successMsg.value = ''
  try {
    const res = await toolApi.optimizeMusicCover(
      primaryEntry.value.root_id,
      primaryEntry.value.rel_path,
      Number(coverMaxDimStr.value) || 1000,
      coverQuality.value,
    )
    if (!res.report.had_cover) {
      successMsg.value = '该音频未包含内嵌封面'
    } else if (!res.report.optimized) {
      successMsg.value = `封面已足够精简（${(res.report.before_bytes / 1024).toFixed(1)} KB），无需重压`
    } else {
      successMsg.value = `封面已优化：${(res.report.before_bytes / 1024).toFixed(1)} KB → ${(res.report.after_bytes / 1024).toFixed(1)} KB（${res.report.width}×${res.report.height}）`
      emit('updated', res.detail)
    }
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
  } finally {
    busy.value = false
  }
}

async function handleReplayGain(): Promise<void> {
  if (!props.rootId || sortedEntries.value.length === 0) return
  busy.value = true
  errorMsg.value = ''
  successMsg.value = ''
  try {
    const rep = await toolApi.applyMusicReplayGain(
      props.rootId,
      sortedEntries.value.map((e) => e.rel_path),
    )
    const albumPart =
      rep.album_gain_db !== null ? `，专辑增益 ${rep.album_gain_db > 0 ? '+' : ''}${rep.album_gain_db} dB` : ''
    successMsg.value = `已写入 ${rep.processed} 首曲目的 ReplayGain 响度标签${albumPart}`
    emit('completed')
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
  } finally {
    busy.value = false
  }
}

async function handleSplitCue(): Promise<void> {
  if (!primaryEntry.value) return
  busy.value = true
  errorMsg.value = ''
  successMsg.value = ''
  try {
    const rep = await toolApi.splitMusicCue(primaryEntry.value.root_id, primaryEntry.value.rel_path)
    successMsg.value = `CUE 分轨完成：生成 ${rep.created_files.length} 首无损 FLAC 并写入标签`
    emit('completed')
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
  } finally {
    busy.value = false
  }
}

async function handleBatchSyncAlbum(): Promise<void> {
  if (!props.rootId || sortedEntries.value.length === 0) return
  busy.value = true
  errorMsg.value = ''
  successMsg.value = ''
  try {
    const rep = await toolApi.batchSyncMusicAlbum(
      props.rootId,
      sortedEntries.value.map((e) => e.rel_path),
      {
        album: albumName.value.trim() || undefined,
        album_artist: albumArtist.value.trim() || undefined,
        artist: trackArtist.value.trim() || undefined,
        year: albumYear.value ?? undefined,
        genre: albumGenre.value.trim() || undefined,
        disc_number: discNumber.value ?? undefined,
        auto_number_tracks: autoNumberTracks.value,
        start_track: startTrack.value,
      },
    )
    successMsg.value = `已同步 ${rep.updated_count} 首曲目的专辑信息${rep.failed_paths.length > 0 ? `（失败 ${rep.failed_paths.length}）` : ''}`
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
              <LuMusic class="h-4 w-4" />
            </div>
            <div>
              <h3 class="text-sm font-semibold">
                音乐精校工具箱
                <span v-if="isSingleMode && primaryEntry"> · {{ primaryEntry.file_name }}</span>
                <span v-else>（已选 {{ sortedEntries.length }} 首曲目）</span>
              </h3>
              <p class="text-[11px] text-[var(--ctp-subtext0)]">
                LRC 歌词时间轴微调与双向同步 · 封面瘦身 · ReplayGain 响度 · CUE 整轨分轨 · 专辑聚类同步
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
            :class="activeTab === 'lyrics' ? 'border-[var(--ctp-accent)] text-[var(--ctp-accent)]' : 'border-transparent text-[var(--ctp-subtext0)]'"
            @click="activeTab = 'lyrics'"
          >
            <LuSlidersHorizontal class="h-3.5 w-3.5" />
            歌词精校 & 同步
          </button>
          <button
            type="button"
            class="inline-flex items-center gap-1.5 border-b-2 px-3 py-2 text-xs font-medium"
            :class="activeTab === 'audio' ? 'border-[var(--ctp-accent)] text-[var(--ctp-accent)]' : 'border-transparent text-[var(--ctp-subtext0)]'"
            @click="activeTab = 'audio'"
          >
            <LuVolume2 class="h-3.5 w-3.5" />
            响度 / 封面 / CUE 分轨
          </button>
          <button
            type="button"
            class="inline-flex items-center gap-1.5 border-b-2 px-3 py-2 text-xs font-medium"
            :class="activeTab === 'album' ? 'border-[var(--ctp-accent)] text-[var(--ctp-accent)]' : 'border-transparent text-[var(--ctp-subtext0)]'"
            @click="activeTab = 'album'"
          >
            <LuDisc3 class="h-3.5 w-3.5" />
            专辑聚类批量同步
          </button>
        </div>

        <!-- Body -->
        <div class="min-h-0 flex-1 overflow-y-auto p-5 text-xs">
          <!-- TAB 1: Lyrics -->
          <div v-if="activeTab === 'lyrics' && isSingleMode" class="space-y-3">
            <div class="flex flex-wrap items-center gap-2">
              <span class="text-[11px] text-[var(--ctp-subtext0)]">时间轴偏移 (ms)：</span>
              <button
                type="button"
                class="rounded-lg border border-[var(--ctp-surface0)] px-2 py-1 hover:bg-[var(--ctp-surface0)]"
                @click="offsetMs -= 500"
              >
                -0.5s
              </button>
              <button
                type="button"
                class="rounded-lg border border-[var(--ctp-surface0)] px-2 py-1 hover:bg-[var(--ctp-surface0)]"
                @click="offsetMs -= 100"
              >
                -0.1s
              </button>
              <input
                v-model.number="offsetMs"
                type="number"
                step="100"
                class="w-24 rounded-lg border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-2 py-1 text-center"
              />
              <button
                type="button"
                class="rounded-lg border border-[var(--ctp-surface0)] px-2 py-1 hover:bg-[var(--ctp-surface0)]"
                @click="offsetMs += 100"
              >
                +0.1s
              </button>
              <button
                type="button"
                class="rounded-lg border border-[var(--ctp-surface0)] px-2 py-1 hover:bg-[var(--ctp-surface0)]"
                @click="offsetMs += 500"
              >
                +0.5s
              </button>
            </div>

            <div class="flex flex-wrap items-center gap-4">
              <AppCheckbox v-model="stripCreditNoise" label="清理头部作词/作曲/版权噪声行" />
              <div class="inline-flex items-center gap-1.5">
                <span class="text-[var(--ctp-subtext0)]">简繁转换</span>
                <div class="w-28">
                  <AppSelect
                    :model-value="zhDirection"
                    :options="zhOptions"
                    variant="compact"
                    @update:model-value="(v) => (zhDirection = v as '' | 't2s' | 's2t')"
                  />
                </div>
              </div>
              <div class="inline-flex items-center gap-1.5">
                <span class="text-[var(--ctp-subtext0)]">写入目标</span>
                <div class="w-48">
                  <AppSelect
                    :model-value="syncMode"
                    :options="syncModeOptions"
                    variant="compact"
                    @update:model-value="(v) => (syncMode = v as 'both' | 'sidecar_to_embedded' | 'embedded_to_sidecar' | 'none')"
                  />
                </div>
              </div>
            </div>

            <textarea
              v-model="lyricsText"
              rows="11"
              placeholder="[00:00.00] 歌词内容（留空则自动读取现有内嵌或侧车歌词）"
              class="w-full resize-y rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] p-3 font-mono text-[11px] leading-relaxed outline-none focus:border-[var(--ctp-accent)]"
            />

            <div class="flex justify-end">
              <button
                type="button"
                :disabled="busy"
                class="inline-flex items-center gap-1.5 rounded-xl bg-[var(--ctp-accent)] px-4 py-1.5 text-xs font-medium text-[var(--ctp-on-accent)] disabled:opacity-50"
                @click="handleProcessLyrics"
              >
                <LuLoader2 v-if="busy" class="h-3.5 w-3.5 animate-spin" />
                应用歌词精校并保存
              </button>
            </div>
          </div>

          <!-- TAB 2: Audio / ReplayGain / Cover / CUE -->
          <div v-else-if="activeTab === 'audio'" class="space-y-4">
            <div class="rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)]/70 p-4">
              <div class="mb-1 flex items-center gap-2 font-semibold">
                <LuVolume2 class="h-4 w-4 text-[var(--ctp-accent)]" />
                ReplayGain 2.0 响度归一化（FFmpeg EBU R128）
              </div>
              <p class="mb-3 text-[11px] text-[var(--ctp-subtext0)]">
                无损分析音轨集成响度（基准 -18 LUFS）并写入标准 REPLAYGAIN_TRACK_GAIN / ALBUM_GAIN 标签，不重新编码音频。
              </p>
              <button
                type="button"
                :disabled="busy"
                class="inline-flex items-center gap-1.5 rounded-xl bg-[var(--ctp-accent)] px-3.5 py-1.5 text-xs font-medium text-[var(--ctp-on-accent)] disabled:opacity-50"
                @click="handleReplayGain"
              >
                <LuLoader2 v-if="busy" class="h-3.5 w-3.5 animate-spin" />
                计算并写入 ReplayGain（{{ sortedEntries.length }} 首）
              </button>
            </div>

            <div v-if="isSingleMode" class="rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)]/70 p-4">
              <div class="mb-1 flex items-center gap-2 font-semibold">
                <LuImage class="h-4 w-4 text-[var(--ctp-accent)]" />
                内嵌封面压缩瘦身
              </div>
              <p class="mb-3 text-[11px] text-[var(--ctp-subtext0)]">
                检测超大内嵌封面并等比缩放为高画质 JPEG，显著加速播放器加载与扫码。
              </p>
              <div class="flex flex-wrap items-center gap-3">
                <div class="inline-flex items-center gap-1.5">
                  <span>最大边长</span>
                  <div class="w-36">
                    <AppSelect v-model="coverMaxDimStr" :options="coverDimOptions" variant="compact" />
                  </div>
                </div>
                <button
                  type="button"
                  :disabled="busy"
                  class="inline-flex items-center gap-1.5 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-panel)] px-3.5 py-1.5 text-xs font-medium hover:border-[var(--ctp-accent)] disabled:opacity-50"
                  @click="handleOptimizeCover"
                >
                  <LuLoader2 v-if="busy" class="h-3.5 w-3.5 animate-spin" />
                  压缩当前内嵌封面
                </button>
              </div>
            </div>

            <div v-if="isSingleMode" class="rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)]/70 p-4">
              <div class="mb-1 flex items-center gap-2 font-semibold">
                <LuScissors class="h-4 w-4 text-[var(--ctp-accent)]" />
                整轨音频 + 同名 .cue 一键无损分轨
              </div>
              <p class="mb-3 text-[11px] text-[var(--ctp-subtext0)]">
                自动读取同目录下伴随 .cue 索引（兼容 GBK/UTF-8），使用 FFmpeg 切分为单曲 FLAC 并自动写入曲名、艺人、专辑与音轨号。
              </p>
              <button
                type="button"
                :disabled="busy"
                class="inline-flex items-center gap-1.5 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-panel)] px-3.5 py-1.5 text-xs font-medium hover:border-[var(--ctp-accent)] disabled:opacity-50"
                @click="handleSplitCue"
              >
                <LuLoader2 v-if="busy" class="h-3.5 w-3.5 animate-spin" />
                按伴随 .cue 切分为单曲 FLAC
              </button>
            </div>
          </div>

          <!-- TAB 3: Album Cluster Batch Sync -->
          <div v-else class="space-y-3">
            <div class="grid grid-cols-1 gap-3 sm:grid-cols-2">
              <label class="block">
                <span class="mb-1 block text-[11px] text-[var(--ctp-subtext0)]">专辑名 (Album) · 留空不覆盖</span>
                <input v-model="albumName" type="text" class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 outline-none focus:border-[var(--ctp-accent)]" />
              </label>
              <label class="block">
                <span class="mb-1 block text-[11px] text-[var(--ctp-subtext0)]">专辑艺人 (AlbumArtist)</span>
                <input v-model="albumArtist" type="text" class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 outline-none focus:border-[var(--ctp-accent)]" />
              </label>
              <label class="block">
                <span class="mb-1 block text-[11px] text-[var(--ctp-subtext0)]">曲目艺人 (Artist · 可留空保留原值)</span>
                <input v-model="trackArtist" type="text" class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 outline-none focus:border-[var(--ctp-accent)]" />
              </label>
              <label class="block">
                <span class="mb-1 block text-[11px] text-[var(--ctp-subtext0)]">流派 (Genre)</span>
                <input v-model="albumGenre" type="text" class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 outline-none focus:border-[var(--ctp-accent)]" />
              </label>
              <label class="block">
                <span class="mb-1 block text-[11px] text-[var(--ctp-subtext0)]">发行年份</span>
                <input v-model.number="albumYear" type="number" class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 outline-none focus:border-[var(--ctp-accent)]" />
              </label>
              <label class="block">
                <span class="mb-1 block text-[11px] text-[var(--ctp-subtext0)]">碟号 (DiscNumber)</span>
                <input v-model.number="discNumber" type="number" min="1" class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 outline-none focus:border-[var(--ctp-accent)]" />
              </label>
            </div>
            <div class="flex flex-wrap items-center gap-3 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)]/70 p-3">
              <AppCheckbox v-model="autoNumberTracks" label="按文件名自然序自动重编音轨号 (Track 1..N)" />
              <label v-if="autoNumberTracks" class="inline-flex items-center gap-1.5">
                <span>起始曲目号</span>
                <input v-model.number="startTrack" type="number" min="1" class="w-16 rounded-lg border border-[var(--ctp-surface0)] bg-[var(--ctp-panel)] px-2 py-1" />
              </label>
            </div>
            <div class="flex justify-end">
              <button
                type="button"
                :disabled="busy"
                class="inline-flex items-center gap-1.5 rounded-xl bg-[var(--ctp-accent)] px-4 py-1.5 text-xs font-medium text-[var(--ctp-on-accent)] disabled:opacity-50"
                @click="handleBatchSyncAlbum"
              >
                <LuLoader2 v-if="busy" class="h-3.5 w-3.5 animate-spin" />
                同步整张专辑（{{ sortedEntries.length }} 首）
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
