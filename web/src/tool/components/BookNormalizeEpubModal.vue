<script setup lang="ts">
import { computed, reactive, ref, watch } from 'vue'
import {
  LuAlertCircle,
  LuArchive,
  LuBookOpen,
  LuCheckCircle2,
  LuLoader2,
  LuPackageCheck,
} from 'vue-icons-plus/lu'
import AppCheckbox from '@/components/ui/AppCheckbox.vue'
import MdSelect from '@/components/ui/MdSelect.vue'
import AppSlider from '@/components/ui/AppSlider.vue'
import MdSwitch from '@/components/ui/MdSwitch.vue'
import MdDialog from '@/components/ui/MdDialog.vue'
import { toolApi } from '../api'
import { formatBytes } from '../i18n'
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
      // Batch mode defaults to skipping already-normalized EPUBs & .orig.epub backups;
      // single-book intentional re-run defaults to force reprocessing unless it's an .orig.epub backup.
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
    return `共节省 ${formatBytes(diff)} (-${pct}%)`
  }
  return `变化 +${formatBytes(-diff)} (+${pct}%)`
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
const imageFormatOptions = [
  { value: 'webp', label: 'WebP（推荐 · 默认）' },
  { value: 'jpeg', label: 'JPEG（通用墨水屏兼容）' },
  { value: 'keep', label: '保持原格式' },
]

const maxDimOptions = [
  { value: '1200', label: '1200 px（极限瘦身）' },
  { value: '1600', label: '1600 px（均衡推荐）' },
  { value: '2000', label: '2000 px（高清）' },
  { value: '0', label: '不缩放原尺寸' },
]

const maxDimModel = computed<string>({
  get: () => String(form.max_image_dimension),
  set: (v) => {
    form.max_image_dimension = Number(v) || 0
  },
})

const dialogSubtitle = computed(() => {
  const parts: string[] = [`已选 ${bookEntries.value.length} 本图书`]
  if (formatBreakdown.value.txt) parts.push(`TXT ${formatBreakdown.value.txt}`)
  if (formatBreakdown.value.epub) parts.push(`EPUB ${formatBreakdown.value.epub}`)
  if (formatBreakdown.value.pdf) parts.push(`PDF ${formatBreakdown.value.pdf}`)
  return parts.join(' · ')
})
</script>

<template>
  <MdDialog
    :open="open"
    title="转换为规范化压缩 EPUB 3.0"
    :subtitle="dialogSubtitle"
    max-width="max-w-2xl"
    :close-on-backdrop="!running"
    :hide-close="running"
    @close="!running && emit('close')"
  >
    <template #icon>
      <LuPackageCheck class="h-5 w-5" />
    </template>

    <div class="space-y-4 py-1">
      <!-- Single Already-Normalized Notice -->
      <div
        v-if="isSingleAlreadyNormalized"
        class="flex items-start gap-2 rounded-2xl bg-[var(--md-sys-color-primary-container)]/55 px-3.5 py-2.5 text-xs text-[var(--md-sys-color-on-primary-container)]"
      >
        <LuCheckCircle2 class="mt-0.5 h-4 w-4 shrink-0" />
        <span>该书已规范化，继续将按新参数重新处理。</span>
      </div>

      <!-- Source Retention Option (Highlighted, Default: Keep) -->
      <div class="rounded-2xl bg-[var(--md-sys-color-surface)] p-3.5">
        <div class="flex items-start justify-between gap-3">
          <div class="min-w-0 flex-1">
            <div class="flex flex-wrap items-center gap-2 text-xs font-semibold text-[var(--md-sys-color-on-surface)]">
              <span>保留源文件</span>
              <span
                class="rounded-full px-2 py-0.5 text-[10px] font-semibold"
                :class="
                  form.keep_source
                    ? 'bg-[var(--md-sys-color-success-container)] text-[var(--md-sys-color-on-success-container)]'
                    : 'bg-[var(--md-sys-color-warning-container)] text-[var(--md-sys-color-on-warning-container)]'
                "
              >
                {{ form.keep_source ? '默认开启 · 安全保留' : '转换后将删除或覆盖源文件' }}
              </span>
            </div>
            <p class="mt-1 text-[11px] leading-relaxed text-[var(--md-sys-color-on-surface-variant)]">
              <template v-if="form.keep_source">
                TXT / PDF 保留原文件并生成 .epub；EPUB 备份为
                <code class="rounded bg-[var(--md-sys-color-surface-container-high)] px-1 font-mono text-[10px]">.orig.epub</code>。
              </template>
              <template v-else>
                转换后删除 TXT / PDF 源文件，EPUB 直接覆盖原文件。
              </template>
            </p>
          </div>
          <MdSwitch v-model="form.keep_source" :disabled="running" />
        </div>
      </div>

      <!-- Image Compression Settings (Default: WebP) -->
      <div class="space-y-3 rounded-2xl bg-[var(--md-sys-color-surface)] p-3.5">
        <div class="flex items-center justify-between gap-2">
          <div>
            <div class="text-xs font-semibold text-[var(--md-sys-color-on-surface)]">
              压缩封面与内嵌插图
            </div>
            <div class="mt-0.5 text-[11px] text-[var(--md-sys-color-on-surface-variant)]">
              仅在图片超限或体积缩小时替换
            </div>
          </div>
          <MdSwitch v-model="form.compress_images" :disabled="running" />
        </div>

        <div v-if="form.compress_images" class="grid grid-cols-1 gap-3 pt-1 sm:grid-cols-3">
          <div class="space-y-1">
            <label class="block text-[11px] font-medium text-[var(--md-sys-color-on-surface-variant)]">
              目标图片格式
            </label>
            <MdSelect
              :model-value="form.image_format"
              :options="imageFormatOptions"
              size="sm"
              :disabled="running"
              @update:model-value="form.image_format = $event as BookImageCompressFormat"
            />
          </div>

          <div class="space-y-1">
            <label class="block text-[11px] font-medium text-[var(--md-sys-color-on-surface-variant)]">
              最大边长限制
            </label>
            <MdSelect
              v-model="maxDimModel"
              :options="maxDimOptions"
              size="sm"
              :disabled="running"
            />
          </div>

          <div class="space-y-1">
            <label class="flex items-center justify-between text-[11px] font-medium text-[var(--md-sys-color-on-surface-variant)]">
              <span>{{ form.image_format.toUpperCase() }} 压缩质量</span>
              <span class="font-mono text-[var(--md-sys-color-primary)]">{{ form.image_quality }}</span>
            </label>
            <div class="pt-1.5">
              <AppSlider
                v-model="form.image_quality"
                :min="55"
                :max="95"
                :step="1"
                :disabled="running"
                :aria-label="`${form.image_format.toUpperCase()} 压缩质量`"
              />
            </div>
          </div>
        </div>
      </div>

      <!-- Normalization Options -->
      <div class="space-y-2.5 rounded-2xl bg-[var(--md-sys-color-surface)] p-3.5">
        <div class="text-xs font-semibold text-[var(--md-sys-color-on-surface)]">
          EPUB 3.0 结构规范化选项
        </div>

        <div class="flex flex-col gap-2">
          <AppCheckbox
            v-model="form.generate_jacket"
            :disabled="running"
            label="生成 / 更新书籍简介扉页"
          />
          <AppCheckbox
            v-model="form.remove_orphans"
            :disabled="running"
            label="清理未引用图片与系统垃圾文件"
          />
          <AppCheckbox
            v-model="form.pdf_scanned_fallback"
            :disabled="running"
            label="扫描版 PDF 自动转为图页 EPUB"
          />
          <div class="flex flex-wrap items-center justify-between gap-2">
            <AppCheckbox
              v-model="form.skip_normalized"
              :disabled="running"
              label="跳过已规范化 EPUB 与 .orig.epub 原件备份"
            />
            <span
              v-if="alreadyNormalizedCount + origBackupCount > 0"
              class="shrink-0 rounded-full px-2 py-0.5 text-[10px] font-semibold"
              :class="
                form.skip_normalized
                  ? 'bg-[var(--md-sys-color-surface-container-high)] text-[var(--md-sys-color-on-surface-variant)]'
                  : 'bg-[var(--md-sys-color-warning-container)] text-[var(--md-sys-color-on-warning-container)]'
              "
            >
              {{
                form.skip_normalized
                  ? `将跳过 ${willSkipCount} 项`
                  : `强制重跑 ${alreadyNormalizedCount + origBackupCount} 项`
              }}
            </span>
          </div>
        </div>
      </div>

      <!-- Error Alert -->
      <div
        v-if="errorMsg"
        class="flex items-start gap-2 rounded-2xl bg-[var(--md-sys-color-error-container)] px-3.5 py-2.5 text-xs text-[var(--md-sys-color-on-error-container)]"
      >
        <LuAlertCircle class="mt-0.5 h-4 w-4 shrink-0" />
        <span>{{ errorMsg }}</span>
      </div>

      <!-- Execution Report -->
      <div v-if="report" class="space-y-2.5 rounded-2xl bg-[var(--md-sys-color-surface)] p-3.5">
        <div class="flex flex-wrap items-center justify-between gap-2">
          <div class="flex items-center gap-2 text-xs font-semibold text-[var(--md-sys-color-on-surface)]">
            <LuCheckCircle2 class="h-4 w-4 text-[var(--md-sys-color-success)]" />
            <span>
              处理完成：成功 {{ report.succeeded }} 本
              <template v-if="report.skipped > 0">，跳过 {{ report.skipped }} 本</template>
              <template v-if="report.failed > 0">，失败 {{ report.failed }} 本</template>
            </span>
          </div>
          <span
            v-if="savingsText"
            class="rounded-full bg-[var(--md-sys-color-primary-container)] px-2.5 py-0.5 font-mono text-[11px] font-semibold text-[var(--md-sys-color-on-primary-container)]"
          >
            {{ formatBytes(report.total_before_bytes) }} → {{ formatBytes(report.total_after_bytes) }} · {{ savingsText }}
          </span>
        </div>

        <div class="max-h-56 divide-y divide-[var(--md-sys-color-outline-variant)]/40 overflow-y-auto rounded-xl border border-[var(--md-sys-color-outline-variant)]/45">
          <div
            v-for="item in report.items"
            :key="item.source_rel_path"
            class="flex items-start justify-between gap-3 px-3 py-2 text-xs"
          >
            <div class="min-w-0 flex-1">
              <div class="flex items-center gap-1.5">
                <LuBookOpen class="h-3.5 w-3.5 shrink-0 text-[var(--md-sys-color-primary)]" />
                <span class="truncate font-semibold text-[var(--md-sys-color-on-surface)]">
                  {{ item.output_rel_path || item.source_rel_path }}
                </span>
                <span class="shrink-0 rounded bg-[var(--md-sys-color-surface-container-high)] px-1.5 py-0.5 font-mono text-[9px] uppercase text-[var(--md-sys-color-on-surface-variant)]">
                  {{ item.source_format }}
                </span>
              </div>

              <div
                v-if="item.error"
                class="mt-1 text-[11px] text-[var(--md-sys-color-error)]"
              >
                {{ item.error }}
              </div>
              <div
                v-else-if="item.skipped"
                class="mt-0.5 flex items-center gap-1.5 text-[11px] text-[var(--md-sys-color-on-surface-variant)]"
              >
                <span class="rounded-full bg-[var(--md-sys-color-surface-container-high)] px-2 py-0.5 text-[10px] font-medium">
                  已跳过
                </span>
                <span>{{ item.skip_reason || '已是规范化 EPUB' }}</span>
              </div>
              <div
                v-else
                class="mt-0.5 flex flex-wrap items-center gap-x-2.5 gap-y-0.5 text-[11px] text-[var(--md-sys-color-on-surface-variant)]"
              >
                <span>{{ item.chapter_count }} 章/页</span>
                <span v-if="item.compressed_images > 0">压缩 {{ item.compressed_images }} 张图</span>
                <span v-if="item.removed_orphans > 0">清理 {{ item.removed_orphans }} 孤立项</span>
                <span v-if="item.backup_rel_path" class="inline-flex items-center gap-1 text-[var(--md-sys-color-primary)]">
                  <LuArchive class="h-3 w-3" />
                  原件备份：{{ item.backup_rel_path.split('/').pop() }}
                </span>
                <span v-else-if="item.deleted_source" class="text-[var(--md-sys-color-warning)]">已删源文件</span>
              </div>
            </div>

            <div
              v-if="!item.error && !item.skipped"
              class="shrink-0 text-right font-mono text-[11px] text-[var(--md-sys-color-on-surface-variant)]"
            >
              <div>{{ formatBytes(item.before_bytes) }} → {{ formatBytes(item.after_bytes) }}</div>
            </div>
          </div>
        </div>
      </div>
    </div>

    <template #footer>
      <span class="text-[11px] text-[var(--md-sys-color-on-surface-variant)]">
        支持 TXT / EPUB / PDF 统一规范化打包
      </span>
      <div class="flex items-center gap-2">
        <button
          type="button"
          :disabled="running"
          class="md-btn md-btn-sm md-btn-text md-state"
          @click="emit('close')"
        >
          {{ report ? '完成' : '取消' }}
        </button>
        <button
          type="button"
          :disabled="running || runnableCount === 0"
          class="md-btn md-btn-sm md-btn-filled md-state"
          @click="handleRun"
        >
          <LuLoader2 v-if="running" class="h-3.5 w-3.5 animate-spin" />
          <LuPackageCheck v-else class="h-3.5 w-3.5" />
          <span>
            {{
              running
                ? '正在转换与压缩…'
                : runnableCount === 0
                  ? '所选均已跳过（可取消勾选以重跑）'
                  : `开始转换 (${runnableCount} 本)`
            }}
          </span>
        </button>
      </div>
    </template>
  </MdDialog>
</template>
