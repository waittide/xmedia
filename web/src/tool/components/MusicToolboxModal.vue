<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import {
  LuCheck,
  LuDisc3,
  LuImage,
  LuMusic,
  LuScissors,
  LuSlidersHorizontal,
  LuVolume2,
} from 'vue-icons-plus/lu'

import AppCheckbox from '@/components/ui/AppCheckbox.vue'
import MdButton from '@/components/ui/MdButton.vue'
import MdDialog from '@/components/ui/MdDialog.vue'
import MdSelect from '@/components/ui/MdSelect.vue'
import MdTabs from '@/components/ui/MdTabs.vue'
import MdTextField from '@/components/ui/MdTextField.vue'
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

const tabOptions = computed(() => {
  const list: Array<{ value: 'lyrics' | 'audio' | 'album'; label: string; icon: typeof LuMusic }> = []
  if (isSingleMode.value) {
    list.push({ value: 'lyrics', label: '歌词精校 & 同步', icon: LuSlidersHorizontal })
  }
  list.push({ value: 'audio', label: '响度 / 封面 / CUE 分轨', icon: LuVolume2 })
  list.push({ value: 'album', label: '专辑聚类批量同步', icon: LuDisc3 })
  return list
})

const dialogTitle = computed(() =>
  isSingleMode.value && primaryEntry.value
    ? `音乐精校工具箱 · ${primaryEntry.value.file_name}`
    : `音乐精校工具箱（已选 ${sortedEntries.value.length} 首曲目）`,
)

const zhOptions = [
  { value: '', label: '不转换' },
  { value: 't2s', label: '繁 → 简' },
  { value: 's2t', label: '简 → 繁' },
]
const syncModeOptions = [
  { value: 'both', label: '同时同步内嵌 + 侧车 .lrc' },
  { value: 'sidecar_to_embedded', label: '仅写入音频内嵌标签' },
  { value: 'embedded_to_sidecar', label: '仅导出侧车 .lrc 文件' },
  { value: 'none', label: '仅更新当前缓冲不额外同步' },
]
const coverDimOptions = [
  { value: '1200', label: '1200 px' },
  { value: '1000', label: '1000 px（推荐）' },
  { value: '800', label: '800 px' },
]

// Album batch sync state
const albumName = ref('')
const albumArtist = ref('')
const trackArtist = ref('')
const albumYear = ref<number | null>(null)
const albumGenre = ref('')
const discNumber = ref<number | null>(1)
const autoNumberTracks = ref(true)
const startTrack = ref(1)

let musicLoadSeq = 0

watch(
  () => [props.open, props.entries.length, primaryEntry.value?.rel_path],
  async ([isOpen]) => {
    const seq = ++musicLoadSeq
    if (!isOpen) return
    errorMsg.value = ''
    successMsg.value = ''
    offsetMs.value = 0
    activeTab.value = isSingleMode.value ? 'lyrics' : 'album'

    if (isSingleMode.value && primaryEntry.value) {
      const target = primaryEntry.value
      try {
        const detail = await toolApi.inspectFile(target.root_id, target.rel_path, 'music')
        if (seq !== musicLoadSeq || !props.open) return
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

function toOptInt(val: unknown): number | undefined {
  return typeof val === 'number' && Number.isFinite(val) ? Math.trunc(val) : undefined
}

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
        offset_ms: toOptInt(offsetMs.value) ?? 0,
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
        year: toOptInt(albumYear.value),
        genre: albumGenre.value.trim() || undefined,
        disc_number: toOptInt(discNumber.value),
        auto_number_tracks: autoNumberTracks.value,
        start_track: toOptInt(startTrack.value) ?? 1,
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
  <MdDialog
    :open="open"
    :title="dialogTitle"
    subtitle="歌词偏移精校、ReplayGain 响度归一化、封面瘦身、CUE 无损分轨与专辑批量同步"
    max-width="max-w-2xl"
    :close-on-backdrop="!busy"
    @close="!busy && emit('close')"
  >
    <template #icon>
      <LuMusic class="h-5 w-5" />
    </template>

    <div class="space-y-4 py-1 text-xs">
      <MdTabs v-model="activeTab" :tabs="tabOptions" variant="primary" aria-label="音乐工具箱功能" />

      <!-- TAB 1: Lyrics -->
      <div v-if="activeTab === 'lyrics' && isSingleMode" class="space-y-3">
        <div class="flex flex-wrap items-center gap-2">
          <span class="text-[11px] text-[var(--md-sys-color-on-surface-variant)]">时间轴偏移 (ms)：</span>
          <button
            type="button"
            class="md-btn md-btn-sm md-btn-outlined md-state !h-7 !px-2.5 !text-[11px]"
            @click="offsetMs -= 500"
          >
            -0.5s
          </button>
          <button
            type="button"
            class="md-btn md-btn-sm md-btn-outlined md-state !h-7 !px-2.5 !text-[11px]"
            @click="offsetMs -= 100"
          >
            -0.1s
          </button>
          <input
            v-model.number="offsetMs"
            type="number"
            step="100"
            class="md-input h-8 w-24 !px-2 text-center text-xs"
          />
          <button
            type="button"
            class="md-btn md-btn-sm md-btn-outlined md-state !h-7 !px-2.5 !text-[11px]"
            @click="offsetMs += 100"
          >
            +0.1s
          </button>
          <button
            type="button"
            class="md-btn md-btn-sm md-btn-outlined md-state !h-7 !px-2.5 !text-[11px]"
            @click="offsetMs += 500"
          >
            +0.5s
          </button>
        </div>

        <div class="flex flex-wrap items-center gap-4">
          <AppCheckbox v-model="stripCreditNoise" label="清理头部作词/作曲/版权噪声行" />
          <div class="inline-flex items-center gap-1.5">
            <span class="text-[var(--md-sys-color-on-surface-variant)]">简繁转换</span>
            <div class="w-28">
              <MdSelect
                :model-value="zhDirection"
                :options="zhOptions"
                variant="compact"
                @update:model-value="(v) => (zhDirection = v as '' | 't2s' | 's2t')"
              />
            </div>
          </div>
          <div class="inline-flex items-center gap-1.5">
            <span class="text-[var(--md-sys-color-on-surface-variant)]">写入目标</span>
            <div class="w-48">
              <MdSelect
                :model-value="syncMode"
                :options="syncModeOptions"
                variant="compact"
                @update:model-value="(v) => (syncMode = v as 'both' | 'sidecar_to_embedded' | 'embedded_to_sidecar' | 'none')"
              />
            </div>
          </div>
        </div>

        <MdTextField
          v-model="lyricsText"
          multiline
          :rows="11"
          placeholder="[00:00.00] 歌词内容（留空则自动读取现有内嵌或侧车歌词）"
          aria-label="歌词内容"
        />

        <div class="flex justify-end">
          <MdButton variant="filled" size="sm" :loading="busy" @click="handleProcessLyrics">
            应用歌词精校并保存
          </MdButton>
        </div>
      </div>

      <!-- TAB 2: Audio / ReplayGain / Cover / CUE -->
      <div v-else-if="activeTab === 'audio'" class="space-y-3.5">
        <div class="rounded-2xl bg-surface-container-low p-4">
          <div class="mb-1 flex items-center gap-2 font-semibold">
            <LuVolume2 class="h-4 w-4 text-primary" />
            ReplayGain 2.0 响度归一化（FFmpeg EBU R128）
          </div>
          <p class="mb-3 text-[11px] text-on-surface-variant">
            无损分析音轨集成响度（基准 -18 LUFS）并写入标准 REPLAYGAIN_TRACK_GAIN / ALBUM_GAIN 标签，不重新编码音频。
          </p>
          <MdButton variant="filled" size="sm" :loading="busy" @click="handleReplayGain">
            计算并写入 ReplayGain（{{ sortedEntries.length }} 首）
          </MdButton>
        </div>

        <div
          v-if="isSingleMode"
          class="rounded-2xl bg-surface-container-low p-4"
        >
          <div class="mb-1 flex items-center gap-2 font-semibold">
            <LuImage class="h-4 w-4 text-primary" />
            内嵌封面压缩瘦身
          </div>
          <p class="mb-3 text-[11px] text-on-surface-variant">
            检测超大内嵌封面并等比缩放为高画质 JPEG，显著加速播放器加载与扫码。
          </p>
          <div class="flex flex-wrap items-center gap-3">
            <div class="inline-flex items-center gap-1.5">
              <span>最大边长</span>
              <div class="w-36">
                <MdSelect v-model="coverMaxDimStr" :options="coverDimOptions" variant="compact" />
              </div>
            </div>
            <MdButton variant="tonal" size="sm" :loading="busy" @click="handleOptimizeCover">
              压缩当前内嵌封面
            </MdButton>
          </div>
        </div>

        <div
          v-if="isSingleMode"
          class="rounded-2xl bg-surface-container-low p-4"
        >
          <div class="mb-1 flex items-center gap-2 font-semibold">
            <LuScissors class="h-4 w-4 text-primary" />
            整轨音频 + 同名 .cue 一键无损分轨
          </div>
          <p class="mb-3 text-[11px] text-on-surface-variant">
            自动读取同目录下伴随 .cue 索引（兼容 GBK/UTF-8），使用 FFmpeg 切分为单曲 FLAC 并自动写入曲名、艺人、专辑与音轨号。
          </p>
          <MdButton variant="tonal" size="sm" :loading="busy" @click="handleSplitCue">
            按伴随 .cue 切分为单曲 FLAC
          </MdButton>
        </div>
      </div>

      <!-- TAB 3: Album Cluster Batch Sync -->
      <div v-else class="space-y-3">
        <div class="grid grid-cols-1 gap-3 sm:grid-cols-2">
          <label class="block">
            <span class="mb-1 block text-[11px] text-[var(--md-sys-color-on-surface-variant)]">专辑名 (Album) · 留空不覆盖</span>
            <input v-model="albumName" type="text" class="md-input h-9 w-full text-xs" />
          </label>
          <label class="block">
            <span class="mb-1 block text-[11px] text-[var(--md-sys-color-on-surface-variant)]">专辑艺人 (AlbumArtist)</span>
            <input v-model="albumArtist" type="text" class="md-input h-9 w-full text-xs" />
          </label>
          <label class="block">
            <span class="mb-1 block text-[11px] text-[var(--md-sys-color-on-surface-variant)]">曲目艺人 (Artist · 可留空保留原值)</span>
            <input v-model="trackArtist" type="text" class="md-input h-9 w-full text-xs" />
          </label>
          <label class="block">
            <span class="mb-1 block text-[11px] text-[var(--md-sys-color-on-surface-variant)]">流派 (Genre)</span>
            <input v-model="albumGenre" type="text" class="md-input h-9 w-full text-xs" />
          </label>
          <label class="block">
            <span class="mb-1 block text-[11px] text-[var(--md-sys-color-on-surface-variant)]">发行年份</span>
            <input v-model.number="albumYear" type="number" class="md-input h-9 w-full text-xs" />
          </label>
          <label class="block">
            <span class="mb-1 block text-[11px] text-[var(--md-sys-color-on-surface-variant)]">碟号 (DiscNumber)</span>
            <input v-model.number="discNumber" type="number" min="1" class="md-input h-9 w-full text-xs" />
          </label>
        </div>
        <div class="flex flex-wrap items-center gap-3 rounded-2xl border border-[var(--md-sys-color-outline-variant)]/55 bg-[var(--md-sys-color-surface-container-low)] p-3.5">
          <AppCheckbox v-model="autoNumberTracks" label="按文件名自然序自动重编音轨号 (Track 1..N)" />
          <label v-if="autoNumberTracks" class="inline-flex items-center gap-1.5">
            <span>起始曲目号</span>
            <input v-model.number="startTrack" type="number" min="1" class="md-input h-8 w-16 !px-2 text-xs" />
          </label>
        </div>
        <div class="flex justify-end">
          <MdButton variant="filled" size="sm" :loading="busy" @click="handleBatchSyncAlbum">
            同步整张专辑（{{ sortedEntries.length }} 首）
          </MdButton>
        </div>
      </div>

      <div
        v-if="errorMsg"
        class="rounded-2xl bg-error-container px-3.5 py-2.5 text-on-error-container"
      >
        {{ errorMsg }}
      </div>
      <div
        v-if="successMsg"
        class="flex items-center gap-1.5 rounded-2xl bg-success-container px-3.5 py-2.5 text-on-success-container"
      >
        <LuCheck class="h-4 w-4 shrink-0" />
        <span>{{ successMsg }}</span>
      </div>
    </div>

    <template #footer>
      <div class="ml-auto">
        <MdButton variant="text" size="sm" @click="emit('close')">关闭</MdButton>
      </div>
    </template>
  </MdDialog>
</template>
