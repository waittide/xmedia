<script setup lang="ts">
import { computed, reactive, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  LuAlertCircle,
  LuArchive,
  LuBookOpen,
  LuCheckCircle2,
  LuPackageCheck,
} from 'vue-icons-plus/lu'
import AppCheckbox from '@/components/ui/AppCheckbox.vue'
import AppSlider from '@/components/ui/AppSlider.vue'
import MdButton from '@/components/ui/MdButton.vue'
import MdDialog from '@/components/ui/MdDialog.vue'
import MdSelect from '@/components/ui/MdSelect.vue'
import MdSwitch from '@/components/ui/MdSwitch.vue'
import { formatBytes } from '@/utils/format'
import { toolApi } from '../api'
import type {
  BookImageCompressFormat,
  MediaFileEntry,
  NormalizeBooksBatchReportDto,
  NormalizeBookToEpubOptionsDto,
} from '../types'

const props = defineProps<{
  open: boolean
  rootId: string
  entries: MediaFileEntry[]
}>()

const emit = defineEmits<{
  close: []
  completed: [report: NormalizeBooksBatchReportDto]
}>()

const { t } = useI18n()

const form = reactive<{
  keep_source: boolean
  compress_images: boolean
  image_format: BookImageCompressFormat
  max_image_dimension: number
  image_quality: number
  generate_jacket: boolean
  remove_orphans: boolean
  pdf_scanned_fallback: boolean
  skip_normalized: boolean
}>({
  keep_source: true,
  compress_images: true,
  image_format: 'webp',
  max_image_dimension: 1600,
  image_quality: 82,
  generate_jacket: true,
  remove_orphans: true,
  pdf_scanned_fallback: true,
  skip_normalized: true,
})

const running = ref(false)
const errorMsg = ref<string | null>(null)
const report = ref<NormalizeBooksBatchReportDto | null>(null)

function isOrigBackupFile(fileName: string): boolean {
  return /\.orig(?:\.[^./]+)?\.epub$/i.test(fileName)
}

const bookEntries = computed(() =>
  props.entries.filter(
    (e) =>
      e.media_kind === 'book' &&
      (e.extension === 'txt' || e.extension === 'epub' || e.extension === 'pdf'),
  ),
)

const origBackupCount = computed(
  () => bookEntries.value.filter((e) => e.extension === 'epub' && isOrigBackupFile(e.file_name)).length,
)

const alreadyNormalizedCount = computed(
  () =>
    bookEntries.value.filter(
      (e) =>
        e.extension === 'epub' &&
        !isOrigBackupFile(e.file_name) &&
        Boolean(e.completeness.is_normalized),
    ).length,
)

const willSkipCount = computed(() =>
  form.skip_normalized ? alreadyNormalizedCount.value + origBackupCount.value : 0,
)

const runnableCount = computed(() => Math.max(0, bookEntries.value.length - willSkipCount.value))

const isSingleAlreadyNormalized = computed(
  () => bookEntries.value.length === 1 && alreadyNormalizedCount.value === 1,
)

watch(
  () => props.open,
  (isOpen) => {
    if (isOpen) {
      errorMsg.value = null
      report.value = null
      form.keep_source = true
      form.compress_images = true
      form.image_format = 'webp'
      form.max_image_dimension = 1600
      form.image_quality = 82
      form.generate_jacket = true
      form.remove_orphans = true
      form.pdf_scanned_fallback = true
      form.skip_normalized = bookEntries.value.length > 1 || origBackupCount.value > 0
    }
  },
  { immediate: true },
)

const formatBreakdown = computed(() => {
  let txt = 0
  let epub = 0
  let pdf = 0
  for (const e of bookEntries.value) {
    if (e.extension === 'txt') txt += 1
    else if (e.extension === 'epub') epub += 1
    else if (e.extension === 'pdf') pdf += 1
  }
  return { txt, epub, pdf }
})

const savingsText = computed(() => {
  if (!report.value || report.value.total_before_bytes === 0) return ''
  const diff = report.value.total_before_bytes - report.value.total_after_bytes
  const pct = Math.round((Math.abs(diff) / report.value.total_before_bytes) * 100)
  if (diff >= 0) {
    return t('tool.modals.normalizeEpub.savedBytes', { bytes: formatBytes(diff), pct })
  }
  return t('tool.modals.normalizeEpub.increasedBytes', { bytes: formatBytes(-diff), pct })
})

async function handleRun(): Promise<void> {
  if (running.value || !props.rootId || bookEntries.value.length === 0) return
  running.value = true
  errorMsg.value = null
  report.value = null
  try {
    const options: NormalizeBookToEpubOptionsDto = {
      keep_source: form.keep_source,
      compress_images: form.compress_images,
      image_format: form.image_format,
      max_image_dimension: form.max_image_dimension > 0 ? form.max_image_dimension : null,
      image_quality: Math.min(95, Math.max(50, form.image_quality)),
      generate_jacket: form.generate_jacket,
      remove_orphans: form.remove_orphans,
      pdf_scanned_fallback: form.pdf_scanned_fallback,
      skip_normalized: form.skip_normalized,
    }
    const res = await toolApi.normalizeBooksToEpub(
      props.rootId,
      bookEntries.value.map((e) => e.rel_path),
      options,
    )
    report.value = res
    if (res.succeeded > 0) {
      emit('completed', res)
    }
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
  } finally {
    running.value = false
  }
}

const imageFormatOptions = computed(() => [
  { value: 'webp', label: t('tool.modals.normalizeEpub.fmtWebp') },
  { value: 'jpeg', label: t('tool.modals.normalizeEpub.fmtJpeg') },
  { value: 'keep', label: t('tool.modals.normalizeEpub.fmtKeep') },
])

const maxDimOptions = computed(() => [
  { value: '1200', label: t('tool.modals.normalizeEpub.dim1200') },
  { value: '1600', label: t('tool.modals.normalizeEpub.dim1600') },
  { value: '2000', label: t('tool.modals.normalizeEpub.dim2000') },
  { value: '0', label: t('tool.modals.normalizeEpub.dimKeep') },
])

const maxDimModel = computed<string>({
  get: () => String(form.max_image_dimension),
  set: (v) => {
    form.max_image_dimension = Number(v) || 0
  },
})

const dialogSubtitle = computed(() => {
  const parts: string[] = [
    t('tool.modals.normalizeEpub.selectedBooks', { count: bookEntries.value.length }),
  ]
  if (formatBreakdown.value.txt) parts.push(`TXT ${formatBreakdown.value.txt}`)
  if (formatBreakdown.value.epub) parts.push(`EPUB ${formatBreakdown.value.epub}`)
  if (formatBreakdown.value.pdf) parts.push(`PDF ${formatBreakdown.value.pdf}`)
  return parts.join(' · ')
})
</script>

<template>
  <MdDialog
    :open="open"
    :title="t('tool.modals.normalizeEpub.title')"
    :subtitle="dialogSubtitle"
    max-width="max-w-2xl"
    footer-align="between"
    :close-on-backdrop="!running"
    :hide-close="running"
    @close="!running && emit('close')"
  >
    <template #icon>
      <LuPackageCheck class="h-5 w-5" aria-hidden="true" />
    </template>

    <div class="space-y-4 py-1">
      <div
        v-if="isSingleAlreadyNormalized"
        class="text-body-small flex items-start gap-2 rounded-corner-lg bg-primary-container/55 px-3.5 py-2.5 text-on-primary-container"
      >
        <LuCheckCircle2 class="mt-0.5 h-4 w-4 shrink-0" aria-hidden="true" />
        <span>{{ t('tool.modals.normalizeEpub.alreadyNormalizedNotice') }}</span>
      </div>

      <!-- Source Retention Option -->
      <div class="rounded-corner-lg bg-surface p-4">
        <div class="flex items-start justify-between gap-3">
          <div class="min-w-0 flex-1">
            <div class="text-title-small flex flex-wrap items-center gap-2 text-on-surface">
              <span>{{ t('tool.modals.normalizeEpub.keepSource') }}</span>
              <span
                class="text-label-small rounded-corner-full px-2 py-0.5"
                :class="
                  form.keep_source
                    ? 'bg-success-container text-on-success-container'
                    : 'bg-warning-container text-on-warning-container'
                "
              >
                {{
                  form.keep_source
                    ? t('tool.modals.normalizeEpub.keepSourceOn')
                    : t('tool.modals.normalizeEpub.keepSourceOff')
                }}
              </span>
            </div>
            <p class="text-body-small mt-1 leading-relaxed text-on-surface-variant">
              {{
                form.keep_source
                  ? t('tool.modals.normalizeEpub.keepSourceDescOn')
                  : t('tool.modals.normalizeEpub.keepSourceDescOff')
              }}
            </p>
          </div>
          <MdSwitch
            v-model="form.keep_source"
            :disabled="running"
            :aria-label="t('tool.modals.normalizeEpub.keepSource')"
          />
        </div>
      </div>

      <!-- Image Compression Settings -->
      <div class="space-y-3 rounded-corner-lg bg-surface p-4">
        <div class="flex items-center justify-between gap-2">
          <div>
            <div class="text-title-small text-on-surface">
              {{ t('tool.modals.normalizeEpub.compressImages') }}
            </div>
            <div class="text-body-small mt-0.5 text-on-surface-variant">
              {{ t('tool.modals.normalizeEpub.compressImagesDesc') }}
            </div>
          </div>
          <MdSwitch
            v-model="form.compress_images"
            :disabled="running"
            :aria-label="t('tool.modals.normalizeEpub.compressImages')"
          />
        </div>

        <div v-if="form.compress_images" class="grid grid-cols-1 gap-3 pt-1 sm:grid-cols-3">
          <div class="space-y-1">
            <span class="text-label-medium block text-on-surface-variant">
              {{ t('tool.modals.normalizeEpub.imageFormat') }}
            </span>
            <MdSelect
              :model-value="form.image_format"
              :options="imageFormatOptions"
              size="sm"
              :disabled="running"
              @update:model-value="form.image_format = $event as BookImageCompressFormat"
            />
          </div>

          <div class="space-y-1">
            <span class="text-label-medium block text-on-surface-variant">
              {{ t('tool.modals.normalizeEpub.maxDim') }}
            </span>
            <MdSelect
              v-model="maxDimModel"
              :options="maxDimOptions"
              size="sm"
              :disabled="running"
            />
          </div>

          <div class="space-y-1">
            <div class="text-label-medium flex items-center justify-between text-on-surface-variant">
              <span>
                {{
                  t('tool.modals.normalizeEpub.qualityLabel', {
                    fmt: form.image_format.toUpperCase(),
                  })
                }}
              </span>
              <span class="font-mono text-primary">{{ form.image_quality }}</span>
            </div>
            <div class="pt-1.5">
              <AppSlider
                v-model="form.image_quality"
                :min="55"
                :max="95"
                :step="1"
                :disabled="running"
                :aria-label="
                  t('tool.modals.normalizeEpub.qualityLabel', {
                    fmt: form.image_format.toUpperCase(),
                  })
                "
              />
            </div>
          </div>
        </div>
      </div>

      <!-- Normalization Options -->
      <div class="space-y-2.5 rounded-corner-lg bg-surface p-4">
        <div class="text-title-small text-on-surface">
          {{ t('tool.modals.normalizeEpub.optionsHeading') }}
        </div>

        <div class="flex flex-col gap-2">
          <AppCheckbox
            v-model="form.generate_jacket"
            :disabled="running"
            :label="t('tool.modals.normalizeEpub.optJacket')"
          />
          <AppCheckbox
            v-model="form.remove_orphans"
            :disabled="running"
            :label="t('tool.modals.normalizeEpub.optOrphans')"
          />
          <AppCheckbox
            v-model="form.pdf_scanned_fallback"
            :disabled="running"
            :label="t('tool.modals.normalizeEpub.optPdfFallback')"
          />
          <div class="flex flex-wrap items-center justify-between gap-2">
            <AppCheckbox
              v-model="form.skip_normalized"
              :disabled="running"
              :label="t('tool.modals.normalizeEpub.optSkipNormalized')"
            />
            <span
              v-if="alreadyNormalizedCount + origBackupCount > 0"
              class="text-label-small shrink-0 rounded-corner-full px-2 py-0.5"
              :class="
                form.skip_normalized
                  ? 'bg-surface-container-high text-on-surface-variant'
                  : 'bg-warning-container text-on-warning-container'
              "
            >
              {{
                form.skip_normalized
                  ? t('tool.modals.normalizeEpub.willSkip', { count: willSkipCount })
                  : t('tool.modals.normalizeEpub.forceRerun', {
                      count: alreadyNormalizedCount + origBackupCount,
                    })
              }}
            </span>
          </div>
        </div>
      </div>

      <div
        v-if="errorMsg"
        role="alert"
        class="text-body-small flex items-start gap-2 rounded-corner-lg bg-error-container px-3.5 py-2.5 text-on-error-container"
      >
        <LuAlertCircle class="mt-0.5 h-4 w-4 shrink-0" aria-hidden="true" />
        <span>{{ errorMsg }}</span>
      </div>

      <!-- Execution Report -->
      <div v-if="report" class="space-y-2.5 rounded-corner-lg bg-surface p-4">
        <div class="flex flex-wrap items-center justify-between gap-2">
          <div class="text-title-small flex items-center gap-2 text-on-surface">
            <LuCheckCircle2 class="h-4 w-4 text-success" aria-hidden="true" />
            <span>
              {{ t('tool.modals.normalizeEpub.doneSummary', { succeeded: report.succeeded }) }}
            </span>
          </div>
          <span
            v-if="savingsText"
            class="text-label-medium rounded-corner-full bg-primary-container px-2.5 py-0.5 font-mono text-on-primary-container"
          >
            {{ formatBytes(report.total_before_bytes) }} → {{ formatBytes(report.total_after_bytes) }} · {{ savingsText }}
          </span>
        </div>

        <div class="max-h-56 divide-y divide-outline-variant/35 overflow-y-auto rounded-corner-md bg-surface-container-low">
          <div
            v-for="item in report.items"
            :key="item.source_rel_path"
            class="text-body-small flex items-start justify-between gap-3 px-3 py-2"
          >
            <div class="min-w-0 flex-1">
              <div class="flex items-center gap-1.5">
                <LuBookOpen class="h-3.5 w-3.5 shrink-0 text-primary" aria-hidden="true" />
                <span class="truncate font-medium text-on-surface">
                  {{ item.output_rel_path || item.source_rel_path }}
                </span>
                <span class="text-label-small shrink-0 rounded-corner-xs bg-surface-container-high px-1.5 py-0.5 font-mono uppercase text-on-surface-variant">
                  {{ item.source_format }}
                </span>
              </div>

              <div v-if="item.error" class="mt-1 text-error">
                {{ item.error }}
              </div>
              <div
                v-else-if="item.skipped"
                class="mt-0.5 flex items-center gap-1.5 text-on-surface-variant"
              >
                <span class="text-label-small rounded-corner-full bg-surface-container-high px-2 py-0.5">
                  {{ t('tool.modals.normalizeEpub.skippedBadge') }}
                </span>
                <span>
                  {{ item.skip_reason || t('tool.modals.normalizeEpub.alreadyNormalizedReason') }}
                </span>
              </div>
              <div
                v-else
                class="mt-0.5 flex flex-wrap items-center gap-x-2.5 gap-y-0.5 text-on-surface-variant"
              >
                <span>
                  {{ t('tool.modals.normalizeEpub.chaptersOrPages', { count: item.chapter_count }) }}
                </span>
                <span v-if="item.compressed_images > 0">
                  {{
                    t('tool.modals.normalizeEpub.compressedImgs', {
                      count: item.compressed_images,
                    })
                  }}
                </span>
                <span v-if="item.removed_orphans > 0">
                  {{
                    t('tool.modals.normalizeEpub.removedOrphans', {
                      count: item.removed_orphans,
                    })
                  }}
                </span>
                <span v-if="item.backup_rel_path" class="inline-flex items-center gap-1 text-primary">
                  <LuArchive class="h-3 w-3" aria-hidden="true" />
                  {{
                    t('tool.modals.normalizeEpub.backupLabel', {
                      name: item.backup_rel_path.split('/').pop(),
                    })
                  }}
                </span>
                <span v-else-if="item.deleted_source" class="text-warning">
                  {{ t('tool.modals.normalizeEpub.deletedSource') }}
                </span>
              </div>
            </div>

            <div
              v-if="!item.error && !item.skipped"
              class="text-label-small shrink-0 text-right font-mono text-on-surface-variant"
            >
              <div>{{ formatBytes(item.before_bytes) }} → {{ formatBytes(item.after_bytes) }}</div>
            </div>
          </div>
        </div>
      </div>
    </div>

    <template #footer>
      <span class="text-body-small text-on-surface-variant">
        {{ t('tool.modals.normalizeEpub.footerHint') }}
      </span>
      <div class="flex items-center gap-2">
        <MdButton variant="text" :disabled="running" @click="emit('close')">
          {{ report ? t('tool.common.done') : t('tool.common.cancel') }}
        </MdButton>
        <MdButton
          variant="filled"
          :icon="LuPackageCheck"
          :loading="running"
          :disabled="runnableCount === 0"
          @click="handleRun"
        >
          {{
            running
              ? t('tool.modals.normalizeEpub.converting')
              : runnableCount === 0
                ? t('tool.modals.normalizeEpub.allSkippedBtn')
                : t('tool.modals.normalizeEpub.startConvert', { count: runnableCount })
          }}
        </MdButton>
      </div>
    </template>
  </MdDialog>
</template>
