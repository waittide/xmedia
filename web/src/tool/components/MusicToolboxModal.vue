<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
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

const { t } = useI18n()

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
const offsetMsStr = ref('0')
const stripCreditNoise = ref(true)
const zhDirection = ref<'t2s' | 's2t' | ''>('')
const syncMode = ref<'both' | 'sidecar_to_embedded' | 'embedded_to_sidecar' | 'none'>('both')

// Cover optimize state
const coverMaxDimStr = ref('1000')
const coverQuality = ref(86)

const tabOptions = computed(() => {
  const list: Array<{ value: 'lyrics' | 'audio' | 'album'; label: string; icon: typeof LuMusic }> = []
  if (isSingleMode.value) {
    list.push({ value: 'lyrics', label: t('tool.modals.musicTools.tabLyrics'), icon: LuSlidersHorizontal })
  }
  list.push({ value: 'audio', label: t('tool.modals.musicTools.tabAudio'), icon: LuVolume2 })
  list.push({ value: 'album', label: t('tool.modals.musicTools.tabAlbum'), icon: LuDisc3 })
  return list
})

const dialogTitle = computed(() =>
  isSingleMode.value && primaryEntry.value
    ? t('tool.modals.musicTools.titleSingle', { name: primaryEntry.value.file_name })
    : t('tool.modals.musicTools.titleBatch', { count: sortedEntries.value.length }),
)

const zhOptions = computed(() => [
  { value: '', label: t('tool.modals.batchMeta.zhNone') },
  { value: 't2s', label: t('tool.modals.batchMeta.zhT2s') },
  { value: 's2t', label: t('tool.modals.batchMeta.zhS2t') },
])

const syncModeOptions = computed(() => [
  { value: 'both', label: t('tool.modals.musicTools.syncBoth') },
  { value: 'sidecar_to_embedded', label: t('tool.modals.musicTools.syncEmbeddedOnly') },
  { value: 'embedded_to_sidecar', label: t('tool.modals.musicTools.syncSidecarOnly') },
  { value: 'none', label: t('tool.modals.musicTools.syncBufferOnly') },
])

const coverDimOptions = computed(() => [
  { value: '1200', label: '1200 px' },
  { value: '1000', label: '1000 px' },
  { value: '800', label: '800 px' },
])

// Album batch sync state
const albumName = ref('')
const albumArtist = ref('')
const trackArtist = ref('')
const albumYearStr = ref('')
const albumGenre = ref('')
const discNumberStr = ref('1')
const autoNumberTracks = ref(true)
const startTrackStr = ref('1')

let musicLoadSeq = 0

watch(
  () => [props.open, props.entries.length, primaryEntry.value?.rel_path],
  async ([isOpen]) => {
    const seq = ++musicLoadSeq
    if (!isOpen) return
    errorMsg.value = ''
    successMsg.value = ''
    offsetMsStr.value = '0'
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
          albumYearStr.value =
            detail.metadata.data.year != null ? String(detail.metadata.data.year) : ''
          albumGenre.value = detail.metadata.data.genre || ''
        }
      } catch {
        // ignore preview error
      }
    }
  },
  { immediate: true },
)

function toOptInt(raw: string): number | undefined {
  if (!raw.trim()) return undefined
  const parsed = Number.parseInt(raw.trim(), 10)
  return Number.isFinite(parsed) ? parsed : undefined
}

function shiftOffset(delta: number): void {
  const cur = toOptInt(offsetMsStr.value) ?? 0
  offsetMsStr.value = String(cur + delta)
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
        offset_ms: toOptInt(offsetMsStr.value) ?? 0,
        strip_credit_noise: stripCreditNoise.value,
        zh_direction: zhDirection.value || null,
        sync_mode: syncMode.value,
        custom_text: lyricsText.value.trim() ? lyricsText.value : undefined,
      },
    )
    lyricsText.value = res.report.final_lyrics
    offsetMsStr.value = '0'
    successMsg.value = t('tool.common.saved')
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
      successMsg.value = t('tool.modals.musicTools.noEmbeddedCover')
    } else if (!res.report.optimized) {
      successMsg.value = t('tool.modals.musicTools.coverAlreadyOptimal', {
        kb: (res.report.before_bytes / 1024).toFixed(1),
      })
    } else {
      successMsg.value = t('tool.modals.musicTools.coverOptimized', {
        before: (res.report.before_bytes / 1024).toFixed(1),
        after: (res.report.after_bytes / 1024).toFixed(1),
        w: res.report.width,
        h: res.report.height,
      })
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
    successMsg.value = t('tool.modals.musicTools.replayGainDone', { count: rep.processed })
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
    successMsg.value = t('tool.modals.musicTools.cueDone', { count: rep.created_files.length })
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
        year: toOptInt(albumYearStr.value),
        genre: albumGenre.value.trim() || undefined,
        disc_number: toOptInt(discNumberStr.value),
        auto_number_tracks: autoNumberTracks.value,
        start_track: toOptInt(startTrackStr.value) ?? 1,
      },
    )
    successMsg.value = t('tool.modals.musicTools.albumSynced', { count: rep.updated_count })
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
    :subtitle="t('tool.modals.musicTools.subtitle')"
    max-width="max-w-2xl"
    :close-on-backdrop="!busy"
    @close="!busy && emit('close')"
  >
    <template #icon>
      <LuMusic class="h-5 w-5" aria-hidden="true" />
    </template>

    <div class="space-y-4 py-1">
      <MdTabs v-model="activeTab" :tabs="tabOptions" variant="primary" :aria-label="dialogTitle" />

      <!-- TAB 1: Lyrics -->
      <div v-if="activeTab === 'lyrics' && isSingleMode" class="space-y-3">
        <div class="flex flex-wrap items-end gap-2">
          <div class="w-36">
            <MdTextField
              v-model="offsetMsStr"
              size="sm"
              type="number"
              step="100"
              :label="t('tool.modals.musicTools.offsetLabel')"
            />
          </div>
          <MdButton variant="outlined" size="sm" @click="shiftOffset(-500)">-0.5s</MdButton>
          <MdButton variant="outlined" size="sm" @click="shiftOffset(-100)">-0.1s</MdButton>
          <MdButton variant="outlined" size="sm" @click="shiftOffset(100)">+0.1s</MdButton>
          <MdButton variant="outlined" size="sm" @click="shiftOffset(500)">+0.5s</MdButton>
        </div>

        <div class="flex flex-wrap items-center gap-4">
          <AppCheckbox v-model="stripCreditNoise" :label="t('tool.modals.musicTools.stripNoise')" />
          <div class="inline-flex items-center gap-1.5">
            <span class="text-label-medium text-on-surface-variant">
              {{ t('tool.modals.musicTools.zhConvert') }}
            </span>
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
            <span class="text-label-medium text-on-surface-variant">
              {{ t('tool.modals.musicTools.syncTarget') }}
            </span>
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
          :placeholder="t('tool.modals.musicTools.lyricsPlaceholder')"
          :aria-label="t('tool.modals.musicTools.tabLyrics')"
        />

        <div class="flex justify-end">
          <MdButton variant="filled" size="sm" :loading="busy" @click="handleProcessLyrics">
            {{ t('tool.modals.musicTools.applyLyrics') }}
          </MdButton>
        </div>
      </div>

      <!-- TAB 2: Audio / ReplayGain / Cover / CUE -->
      <div v-else-if="activeTab === 'audio'" class="space-y-3.5">
        <div class="rounded-corner-lg bg-surface p-4">
          <div class="text-title-small mb-1 flex items-center gap-2 text-on-surface">
            <LuVolume2 class="h-4 w-4 text-primary" aria-hidden="true" />
            <span>{{ t('tool.modals.musicTools.replayGainHeading') }}</span>
          </div>
          <p class="text-body-small mb-3 text-on-surface-variant">
            {{ t('tool.modals.musicTools.replayGainDesc') }}
          </p>
          <MdButton variant="filled" size="sm" :loading="busy" @click="handleReplayGain">
            {{ t('tool.modals.musicTools.runReplayGain', { count: sortedEntries.length }) }}
          </MdButton>
        </div>

        <div v-if="isSingleMode" class="rounded-corner-lg bg-surface p-4">
          <div class="text-title-small mb-1 flex items-center gap-2 text-on-surface">
            <LuImage class="h-4 w-4 text-primary" aria-hidden="true" />
            <span>{{ t('tool.modals.musicTools.coverOptHeading') }}</span>
          </div>
          <p class="text-body-small mb-3 text-on-surface-variant">
            {{ t('tool.modals.musicTools.coverOptDesc') }}
          </p>
          <div class="flex flex-wrap items-center gap-3">
            <div class="inline-flex items-center gap-1.5">
              <span class="text-label-medium text-on-surface-variant">
                {{ t('tool.modals.musicTools.maxEdge') }}
              </span>
              <div class="w-36">
                <MdSelect v-model="coverMaxDimStr" :options="coverDimOptions" variant="compact" />
              </div>
            </div>
            <MdButton variant="tonal" size="sm" :loading="busy" @click="handleOptimizeCover">
              {{ t('tool.modals.musicTools.optCoverBtn') }}
            </MdButton>
          </div>
        </div>

        <div v-if="isSingleMode" class="rounded-corner-lg bg-surface p-4">
          <div class="text-title-small mb-1 flex items-center gap-2 text-on-surface">
            <LuScissors class="h-4 w-4 text-primary" aria-hidden="true" />
            <span>{{ t('tool.modals.musicTools.cueHeading') }}</span>
          </div>
          <p class="text-body-small mb-3 text-on-surface-variant">
            {{ t('tool.modals.musicTools.cueDesc') }}
          </p>
          <MdButton variant="tonal" size="sm" :loading="busy" @click="handleSplitCue">
            {{ t('tool.modals.musicTools.splitCueBtn') }}
          </MdButton>
        </div>
      </div>

      <!-- TAB 3: Album Cluster Batch Sync -->
      <div v-else class="space-y-3">
        <div class="grid grid-cols-1 gap-3 sm:grid-cols-2">
          <MdTextField
            v-model="albumName"
            size="sm"
            :label="t('tool.modals.musicTools.albumKeepEmpty')"
          />
          <MdTextField
            v-model="albumArtist"
            size="sm"
            :label="t('tool.forms.music.albumArtist')"
          />
          <MdTextField
            v-model="trackArtist"
            size="sm"
            :label="t('tool.modals.musicTools.trackArtistKeepEmpty')"
          />
          <MdTextField
            v-model="albumGenre"
            size="sm"
            :label="t('tool.forms.music.genre')"
          />
          <MdTextField
            v-model="albumYearStr"
            size="sm"
            type="number"
            :label="t('tool.forms.music.year')"
          />
          <MdTextField
            v-model="discNumberStr"
            size="sm"
            type="number"
            min="1"
            :label="t('tool.forms.music.disc')"
          />
        </div>
        <div class="flex flex-wrap items-center justify-between gap-3 rounded-corner-lg bg-surface p-4">
          <AppCheckbox v-model="autoNumberTracks" :label="t('tool.modals.musicTools.autoTrackNumbers')" />
          <div v-if="autoNumberTracks" class="w-32">
            <MdTextField
              v-model="startTrackStr"
              size="sm"
              type="number"
              min="1"
              :label="t('tool.modals.musicTools.startTrack')"
            />
          </div>
        </div>
        <div class="flex justify-end">
          <MdButton variant="filled" size="sm" :loading="busy" @click="handleBatchSyncAlbum">
            {{ t('tool.modals.musicTools.syncAlbumBtn', { count: sortedEntries.length }) }}
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
