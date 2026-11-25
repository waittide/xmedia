<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { LuCheck, LuCombine, LuFileArchive, LuLayers } from 'vue-icons-plus/lu'
import AppSlider from '@/components/ui/AppSlider.vue'
import MdButton from '@/components/ui/MdButton.vue'
import MdDialog from '@/components/ui/MdDialog.vue'
import MdSelect from '@/components/ui/MdSelect.vue'
import MdTabs from '@/components/ui/MdTabs.vue'
import MdTextField from '@/components/ui/MdTextField.vue'
import { formatBytes } from '@/utils/format'
import { toolApi } from '../api'
import type { ComicImageReencodeFormat, MediaFileDetail, MediaFileEntry } from '../types'

export type ComicToolsTab = 'sync' | 'merge' | 'optimize'

const props = withDefaults(
  defineProps<{
    open: boolean
    rootId: string | null
    entries: MediaFileEntry[]
    initialTab?: ComicToolsTab
  }>(),
  {
    initialTab: 'sync',
  },
)

const emit = defineEmits<{
  close: []
  updated: [detail: MediaFileDetail]
  completed: []
}>()

const { t } = useI18n()

const activeTab = ref<ComicToolsTab>('sync')
const busy = ref(false)
const errorMsg = ref('')
const successMsg = ref('')

// Batch Sync state
const series = ref('')
const seriesGroup = ref('')
const writers = ref('')
const pencillers = ref('')
const publisher = ref('')
const genres = ref('')
const tags = ref('')
const readingDirection = ref('')
const numberingMode = ref<'none' | 'chapter' | 'volume'>('chapter')
const startIndex = ref('1')
const padWidth = ref('2')

// Merge state
const mergeOutputName = ref('')
const mergeSeries = ref('')
const mergeTitle = ref('')
const mergeVolume = ref('1')

// Optimize Archive state
const optReencode = ref<ComicImageReencodeFormat>('keep')
const optJpegQuality = ref(84)
const optMaxWidth = ref('')

const tabOptions = computed(() => [
  { value: 'sync' as const, label: t('tool.modals.comicSeries.tabSync'), icon: LuLayers },
  { value: 'merge' as const, label: t('tool.modals.comicSeries.tabMerge'), icon: LuCombine },
  {
    value: 'optimize' as const,
    label: t('tool.modals.comicSeries.tabOptimize'),
    icon: LuFileArchive,
  },
])

const directionOptions = computed(() => [
  { value: '', label: t('tool.modals.comicSeries.keepOriginal') },
  { value: 'rtl', label: t('tool.forms.comic.dirRtl') },
  { value: 'ltr', label: t('tool.forms.comic.dirLtr') },
  { value: 'webtoon', label: t('tool.forms.comic.dirWebtoon') },
])

const numberingOptions = computed(() => [
  { value: 'none', label: t('tool.modals.comicSeries.numNone') },
  { value: 'chapter', label: t('tool.modals.comicSeries.numChapter') },
  { value: 'volume', label: t('tool.modals.comicSeries.numVolume') },
])

const reencodeOptions = computed(() => [
  { value: 'keep', label: t('tool.modals.comicPages.encKeep') },
  { value: 'jpeg', label: t('tool.modals.comicPages.encJpeg') },
  { value: 'webp', label: t('tool.modals.comicPages.encWebp') },
])

const maxWidthOptions = computed(() => [
  { value: '', label: t('tool.modals.comicPages.widthNone') },
  { value: '2000', label: t('tool.modals.comicPages.width2000') },
  { value: '1600', label: t('tool.modals.comicPages.width1600') },
  { value: '1280', label: t('tool.modals.comicPages.width1280') },
])

const sortedEntries = computed(() =>
  [...props.entries].sort((a, b) =>
    a.file_name.localeCompare(b.file_name, undefined, { numeric: true }),
  ),
)

const optimizableEntries = computed(() =>
  sortedEntries.value.filter((e) => e.extension === 'cbz' || e.extension === 'zip'),
)

watch(
  () => [props.open, props.initialTab] as const,
  ([isOpen, initTab]) => {
    if (!isOpen) return
    activeTab.value = initTab
    errorMsg.value = ''
    successMsg.value = ''
    const first = sortedEntries.value[0]
    if (first) {
      const parentDir = first.rel_path.includes('/')
        ? first.rel_path.slice(0, first.rel_path.lastIndexOf('/'))
        : ''
      const guessedSeries = first.display_title.replace(/\s*(?:Vol\.?|Ch\.?|#)\s*\d+.*$/i, '').trim()
      series.value = guessedSeries
      mergeSeries.value = guessedSeries
      mergeTitle.value = `${guessedSeries || 'Volume'} Vol.01`
      const baseName = `${guessedSeries || 'merged'}_Vol.01.cbz`
      mergeOutputName.value = parentDir ? `${parentDir}/${baseName}` : baseName
    }
  },
  { immediate: true },
)

function splitCsv(raw: string): string[] | undefined {
  const arr = raw
    .split(/[,，、;；]/)
    .map((s) => s.trim())
    .filter(Boolean)
  return arr.length > 0 ? arr : undefined
}

function toOptInt(val: string): number | undefined {
  if (!val.trim()) return undefined
  const n = Number.parseInt(val.trim(), 10)
  return Number.isFinite(n) ? n : undefined
}

async function handleBatchSync(): Promise<void> {
  if (!props.rootId || sortedEntries.value.length === 0) return
  busy.value = true
  errorMsg.value = ''
  successMsg.value = ''
  try {
    const rep = await toolApi.batchSyncComics(
      props.rootId,
      sortedEntries.value.map((e) => e.rel_path),
      {
        series: series.value.trim() || undefined,
        series_group: seriesGroup.value.trim() || undefined,
        writers: splitCsv(writers.value),
        pencillers: splitCsv(pencillers.value),
        publisher: publisher.value.trim() || undefined,
        genres: splitCsv(genres.value),
        tags: splitCsv(tags.value),
        reading_direction: readingDirection.value || undefined,
        numbering_mode: numberingMode.value,
        start_index: toOptInt(startIndex.value) ?? 1,
        pad_width: toOptInt(padWidth.value) ?? 2,
      },
    )
    successMsg.value = t('tool.modals.comicSeries.syncedReport', { count: rep.updated_count })
    emit('completed')
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
  } finally {
    busy.value = false
  }
}

async function handleMerge(): Promise<void> {
  if (!props.rootId || sortedEntries.value.length < 2) return
  if (!mergeOutputName.value.trim()) {
    errorMsg.value = t('tool.modals.comicSeries.outputRequired')
    return
  }
  busy.value = true
  errorMsg.value = ''
  successMsg.value = ''
  try {
    const res = await toolApi.mergeComics(
      props.rootId,
      sortedEntries.value.map((e) => e.rel_path),
      mergeOutputName.value.trim(),
      mergeSeries.value.trim() || undefined,
      mergeTitle.value.trim() || undefined,
      toOptInt(mergeVolume.value) ?? null,
    )
    successMsg.value = t('tool.modals.comicSeries.mergedReport', {
      count: res.report.source_count,
      output: res.report.output_rel_path,
      pages: res.report.total_pages,
    })
    emit('completed')
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
  } finally {
    busy.value = false
  }
}

async function handleOptimize(): Promise<void> {
  if (!props.rootId || optimizableEntries.value.length === 0) return
  busy.value = true
  errorMsg.value = ''
  successMsg.value = ''
  try {
    const maxWidth = toOptInt(optMaxWidth.value) ?? null
    let totalBefore = 0
    let totalAfter = 0
    let lastDetail: MediaFileDetail | null = null
    for (const entry of optimizableEntries.value) {
      const res = await toolApi.optimizeComic(
        props.rootId,
        entry.rel_path,
        optReencode.value,
        optJpegQuality.value,
        maxWidth,
      )
      totalBefore += res.report.before_bytes
      totalAfter += res.report.after_bytes
      lastDetail = res.detail
    }
    if (lastDetail && optimizableEntries.value.length === 1) {
      emit('updated', lastDetail)
    }
    successMsg.value = t('tool.modals.comicSeries.optimizedReport', {
      count: optimizableEntries.value.length,
      before: formatBytes(totalBefore),
      after: formatBytes(totalAfter),
    })
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
    :title="t('tool.modals.comicSeries.title', { count: sortedEntries.length })"
    :subtitle="t('tool.modals.comicSeries.subtitle')"
    max-width="max-w-2xl"
    :close-on-backdrop="!busy"
    @close="!busy && emit('close')"
  >
    <template #icon>
      <LuLayers class="h-5 w-5" aria-hidden="true" />
    </template>

    <div class="space-y-4 py-1">
      <MdTabs
        v-model="activeTab"
        :tabs="tabOptions"
        variant="primary"
        :aria-label="t('tool.modals.comicSeries.title', { count: sortedEntries.length })"
      />

      <div v-if="activeTab === 'sync'" class="grid grid-cols-1 gap-3 sm:grid-cols-2">
        <MdTextField
          v-model="series"
          size="sm"
          :label="t('tool.modals.comicSeries.seriesKeepEmpty')"
        />
        <MdTextField v-model="seriesGroup" size="sm" :label="t('tool.forms.comic.seriesGroup')" />
        <MdTextField v-model="writers" size="sm" :label="t('tool.forms.comic.writers')" />
        <MdTextField v-model="pencillers" size="sm" :label="t('tool.forms.comic.pencillers')" />
        <MdTextField v-model="publisher" size="sm" :label="t('tool.forms.common.publisher')" />
        <div class="flex flex-col gap-1">
          <span class="text-label-medium text-on-surface-variant">
            {{ t('tool.forms.comic.direction') }}
          </span>
          <MdSelect v-model="readingDirection" :options="directionOptions" size="sm" />
        </div>
        <MdTextField v-model="genres" size="sm" :label="t('tool.forms.comic.genres')" />
        <MdTextField v-model="tags" size="sm" :label="t('tool.forms.comic.tags')" />

        <div class="space-y-2.5 rounded-corner-lg bg-surface p-4 sm:col-span-2">
          <div class="text-title-small text-on-surface">
            {{ t('tool.modals.comicSeries.numberingHeading') }}
          </div>
          <div class="grid grid-cols-1 gap-3 sm:grid-cols-12">
            <div class="sm:col-span-6">
              <span class="text-label-medium mb-1 block text-on-surface-variant">
                {{ t('tool.modals.comicSeries.tabSync') }}
              </span>
              <MdSelect
                :model-value="numberingMode"
                :options="numberingOptions"
                size="sm"
                @update:model-value="(v) => (numberingMode = v as 'none' | 'chapter' | 'volume')"
              />
            </div>
            <div v-if="numberingMode !== 'none'" class="sm:col-span-3">
              <MdTextField
                v-model="startIndex"
                size="sm"
                type="number"
                min="0"
                :label="t('tool.modals.comicSeries.startIndex')"
              />
            </div>
            <div v-if="numberingMode === 'chapter'" class="sm:col-span-3">
              <MdTextField
                v-model="padWidth"
                size="sm"
                type="number"
                min="1"
                max="4"
                :label="t('tool.modals.comicSeries.padWidth')"
              />
            </div>
          </div>
        </div>
      </div>

      <div v-else-if="activeTab === 'merge'" class="space-y-3">
        <MdTextField
          v-model="mergeOutputName"
          size="sm"
          :label="t('tool.modals.comicSeries.outputRelPath')"
        />
        <div class="grid grid-cols-1 gap-3 sm:grid-cols-3">
          <MdTextField v-model="mergeSeries" size="sm" :label="t('tool.forms.comic.series')" />
          <MdTextField
            v-model="mergeTitle"
            size="sm"
            :label="t('tool.modals.comicSeries.volumeTitle')"
          />
          <MdTextField
            v-model="mergeVolume"
            size="sm"
            type="number"
            min="1"
            :label="t('tool.forms.comic.volume')"
          />
        </div>
        <div class="rounded-corner-lg bg-surface p-4">
          <div class="text-label-medium mb-1.5 text-on-surface-variant">
            {{ t('tool.modals.comicSeries.mergeOrderHeading') }}
          </div>
          <ol class="text-body-small max-h-36 list-decimal space-y-1 overflow-y-auto pl-5">
            <li v-for="e in sortedEntries" :key="e.rel_path" class="truncate">
              {{ e.file_name }}
            </li>
          </ol>
        </div>
      </div>

      <div v-else class="space-y-3.5">
        <div class="space-y-3 rounded-corner-lg bg-surface p-4">
          <p class="text-body-small text-on-surface-variant">
            {{ t('tool.modals.comicSeries.optimizeDesc') }}
          </p>
          <div class="grid grid-cols-1 gap-3 sm:grid-cols-2">
            <div class="flex flex-col gap-1">
              <span class="text-label-medium text-on-surface-variant">
                {{ t('tool.modals.comicPages.imageEncoding') }}
              </span>
              <MdSelect
                :model-value="optReencode"
                :options="reencodeOptions"
                @update:model-value="(v) => (optReencode = v as ComicImageReencodeFormat)"
              />
            </div>
            <div class="flex flex-col gap-1">
              <span class="text-label-medium text-on-surface-variant">
                {{ t('tool.modals.comicPages.maxWidth') }}
              </span>
              <MdSelect v-model="optMaxWidth" :options="maxWidthOptions" />
            </div>
          </div>

          <div v-if="optReencode === 'jpeg'" class="space-y-1 pt-1">
            <div class="text-label-medium flex items-center justify-between text-on-surface-variant">
              <span>{{ t('tool.modals.comicPages.quality', { q: optJpegQuality }) }}</span>
              <span class="font-mono">{{ optJpegQuality }}%</span>
            </div>
            <AppSlider v-model="optJpegQuality" :min="55" :max="98" :step="1" />
          </div>
        </div>

        <div class="rounded-corner-lg bg-surface p-4">
          <div class="text-label-medium mb-1.5 text-on-surface-variant">
            {{
              t('tool.modals.comicSeries.optimizeTargetsHeading', {
                count: optimizableEntries.length,
              })
            }}
          </div>
          <ul
            v-if="optimizableEntries.length > 0"
            class="text-body-small max-h-36 space-y-1 overflow-y-auto"
          >
            <li
              v-for="e in optimizableEntries"
              :key="e.rel_path"
              class="flex items-center justify-between gap-2"
            >
              <span class="truncate">{{ e.file_name }}</span>
              <span class="text-label-small shrink-0 font-mono text-on-surface-variant">
                {{ formatBytes(e.file_size) }}
              </span>
            </li>
          </ul>
          <p v-else class="text-body-small text-on-surface-variant">
            {{ t('tool.modals.comicSeries.noCbzTargets') }}
          </p>
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
      <div class="ml-auto flex items-center gap-2">
        <MdButton variant="text" :disabled="busy" @click="emit('close')">
          {{ t('tool.common.close') }}
        </MdButton>
        <MdButton
          v-if="activeTab === 'sync'"
          variant="filled"
          :loading="busy"
          :disabled="sortedEntries.length === 0"
          @click="handleBatchSync"
        >
          {{ t('tool.modals.comicSeries.executeSync') }}
        </MdButton>
        <MdButton
          v-else-if="activeTab === 'merge'"
          variant="filled"
          :loading="busy"
          :disabled="sortedEntries.length < 2"
          @click="handleMerge"
        >
          {{ t('tool.modals.comicSeries.executeMerge') }}
        </MdButton>
        <MdButton
          v-else
          variant="filled"
          :icon="LuFileArchive"
          :loading="busy"
          :disabled="optimizableEntries.length === 0"
          @click="handleOptimize"
        >
          {{ t('tool.modals.comicSeries.executeOptimize', { count: optimizableEntries.length }) }}
        </MdButton>
      </div>
    </template>
  </MdDialog>
</template>
