<script setup lang="ts">
import { computed, reactive, ref, watch } from 'vue'
import {
  LuAlertCircle,
  LuArchive,
  LuBookOpen,
  LuCheckCircle2,
  LuLoader2,
  LuPackageCheck,
  LuX,
} from 'vue-icons-plus/lu'
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
}>({
  keep_source: true,
  compress_images: true,
  image_format: 'webp',
  max_image_dimension: 1600,
  image_quality: 82,
  generate_jacket: true,
  remove_orphans: true,
  pdf_scanned_fallback: true,
})

const running = ref(false)
const errorMsg = ref<string | null>(null)
const report = ref<NormalizeBooksBatchReportDto | null>(null)

watch(
  () => props.open,
  (isOpen) => {
    if (isOpen) {
      errorMsg.value = null
      report.value = null
    }
  },
)

const bookEntries = computed(() =>
  props.entries.filter(
    (e) =>
      e.media_kind === 'book' &&
      (e.extension === 'txt' || e.extension === 'epub' || e.extension === 'pdf'),
  ),
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
</script>

<template>
  <Teleport to="body">
    <div
      v-if="open"
      class="fixed inset-0 z-50 flex items-center justify-center bg-black/55 p-4 backdrop-blur-[2px]"
      @click.self="!running && emit('close')"
    >
      <div
        class="flex max-h-[88vh] w-full max-w-2xl flex-col overflow-hidden rounded-[28px] bg-[var(--md-sys-color-surface-container-low)] shadow-[var(--md-sys-elevation-3)]"
      >
        <!-- Header -->
        <div
          class="flex shrink-0 items-center justify-between gap-3 border-b border-[var(--md-sys-color-outline-variant)]/50 px-5 py-4"
        >
          <div class="flex items-center gap-2.5">
            <div
              class="flex h-10 w-10 items-center justify-center rounded-2xl bg-[var(--md-sys-color-primary-container)] text-[var(--md-sys-color-on-primary-container)]"
            >
              <LuPackageCheck class="h-5 w-5" />
            </div>
            <div>
              <h2 class="text-sm font-semibold text-[var(--md-sys-color-on-surface)]">
                转换为规范化压缩 EPUB 3.0
              </h2>
              <p class="mt-0.5 text-[11px] text-[var(--md-sys-color-on-surface-variant)]">
                已选 {{ bookEntries.length }} 本图书
                <span v-if="formatBreakdown.txt">· TXT {{ formatBreakdown.txt }}</span>
                <span v-if="formatBreakdown.epub">· EPUB {{ formatBreakdown.epub }}</span>
                <span v-if="formatBreakdown.pdf">· PDF {{ formatBreakdown.pdf }}</span>
              </p>
            </div>
          </div>

          <button
            type="button"
            :disabled="running"
            class="md-icon-btn md-icon-btn-sm md-state disabled:opacity-40"
            @click="emit('close')"
          >
            <LuX class="h-4 w-4" />
          </button>
        </div>

        <!-- Body -->
        <div class="flex-1 space-y-4 overflow-y-auto px-5 py-4">
          <!-- Source Retention Option (Highlighted, Default: Keep) -->
          <div class="rounded-2xl bg-[var(--md-sys-color-surface)] p-3.5">
            <label class="flex cursor-pointer items-start gap-2.5">
              <input
                v-model="form.keep_source"
                type="checkbox"
                :disabled="running"
                class="mt-0.5 h-4 w-4 accent-[var(--md-sys-color-primary)]"
              />
              <div class="min-w-0 flex-1">
                <div class="flex items-center gap-2 text-xs font-semibold text-[var(--md-sys-color-on-surface)]">
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
                    TXT / PDF 保留原文件并生成同名 .epub；EPUB 源会先将原文件备份为
                    <code class="rounded bg-[var(--md-sys-color-surface-container-high)] px-1 font-mono text-[10px]">原名.orig.epub</code>，规范化结果写回原文件名。
                  </template>
                  <template v-else>
                    TXT / PDF 转换成功后将删除源文件（含同名旁路）；EPUB 源直接原子覆盖原文件。
                  </template>
                </p>
              </div>
            </label>
          </div>

          <!-- Image Compression Settings (Default: WebP) -->
          <div class="space-y-3 rounded-2xl bg-[var(--md-sys-color-surface)] p-3.5">
            <label class="flex cursor-pointer items-center justify-between gap-2">
              <div>
                <div class="text-xs font-semibold text-[var(--md-sys-color-on-surface)]">
                  压缩封面与内嵌插图
                </div>
                <div class="mt-0.5 text-[11px] text-[var(--md-sys-color-on-surface-variant)]">
                  文本/XHTML/CSS 始终以最高级别 Deflate (9) 压缩；图片仅在确实缩小或超限时替换
                </div>
              </div>
              <input
                v-model="form.compress_images"
                type="checkbox"
                :disabled="running"
                class="h-4 w-4 accent-[var(--md-sys-color-primary)]"
              />
            </label>

            <div v-if="form.compress_images" class="grid grid-cols-1 gap-3 pt-1 sm:grid-cols-3">
              <div class="space-y-1">
                <label class="block text-[11px] font-medium text-[var(--md-sys-color-on-surface-variant)]">
                  目标图片格式
                </label>
                <select
                  v-model="form.image_format"
                  :disabled="running"
                  class="h-9 w-full rounded-xl border border-[var(--md-sys-color-outline-variant)] bg-[var(--md-sys-color-surface-container-low)] px-2.5 text-xs text-[var(--md-sys-color-on-surface)] outline-none focus:border-[var(--md-sys-color-primary)]"
                >
                  <option value="webp">WebP（推荐 · 默认）</option>
                  <option value="jpeg">JPEG（通用墨水屏兼容）</option>
                  <option value="keep">保持原格式</option>
                </select>
              </div>

              <div class="space-y-1">
                <label class="block text-[11px] font-medium text-[var(--md-sys-color-on-surface-variant)]">
                  最大边长限制
                </label>
                <select
                  v-model.number="form.max_image_dimension"
                  :disabled="running"
                  class="h-9 w-full rounded-xl border border-[var(--md-sys-color-outline-variant)] bg-[var(--md-sys-color-surface-container-low)] px-2.5 text-xs text-[var(--md-sys-color-on-surface)] outline-none focus:border-[var(--md-sys-color-primary)]"
                >
                  <option :value="1200">1200 px（极限瘦身）</option>
                  <option :value="1600">1600 px（均衡推荐）</option>
                  <option :value="2000">2000 px（高清）</option>
                  <option :value="0">不缩放原尺寸</option>
                </select>
              </div>

              <div class="space-y-1">
                <label class="flex items-center justify-between text-[11px] font-medium text-[var(--md-sys-color-on-surface-variant)]">
                  <span>JPEG 压缩质量</span>
                  <span class="font-mono text-[var(--md-sys-color-primary)]">{{ form.image_quality }}</span>
                </label>
                <input
                  v-model.number="form.image_quality"
                  type="range"
                  min="55"
                  max="95"
                  step="1"
                  :disabled="running"
                  class="mt-2 w-full accent-[var(--md-sys-color-primary)]"
                />
              </div>
            </div>
          </div>

          <!-- Normalization Options -->
          <div class="space-y-2.5 rounded-2xl bg-[var(--md-sys-color-surface)] p-3.5">
            <div class="text-xs font-semibold text-[var(--md-sys-color-on-surface)]">
              EPUB 3.0 结构规范化选项
            </div>

            <label class="flex cursor-pointer items-center gap-2 text-xs text-[var(--md-sys-color-on-surface)]">
              <input
                v-model="form.generate_jacket"
                type="checkbox"
                :disabled="running"
                class="h-3.5 w-3.5 accent-[var(--md-sys-color-primary)]"
              />
              <span>生成 / 更新标准化书籍简介扉页（xmedia_jacket.xhtml）</span>
            </label>

            <label class="flex cursor-pointer items-center gap-2 text-xs text-[var(--md-sys-color-on-surface)]">
              <input
                v-model="form.remove_orphans"
                type="checkbox"
                :disabled="running"
                class="h-3.5 w-3.5 accent-[var(--md-sys-color-primary)]"
              />
              <span>清理 EPUB 中未引用的孤立图片及系统垃圾（__MACOSX / .DS_Store）</span>
            </label>

            <label class="flex cursor-pointer items-center gap-2 text-xs text-[var(--md-sys-color-on-surface)]">
              <input
                v-model="form.pdf_scanned_fallback"
                type="checkbox"
                :disabled="running"
                class="h-3.5 w-3.5 accent-[var(--md-sys-color-primary)]"
              />
              <span>扫描版 PDF（无文字层）自动提取页面图片并压缩打包为图页 EPUB</span>
            </label>
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
                <LuCheckCircle2 class="h-4 w-4 text-emerald-500" />
                <span>
                  处理完成：成功 {{ report.succeeded }} 本
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
                    <span v-else-if="item.deleted_source" class="text-amber-500">已删源文件</span>
                  </div>
                </div>

                <div
                  v-if="!item.error"
                  class="shrink-0 text-right font-mono text-[11px] text-[var(--md-sys-color-on-surface-variant)]"
                >
                  <div>{{ formatBytes(item.before_bytes) }} → {{ formatBytes(item.after_bytes) }}</div>
                </div>
              </div>
            </div>
          </div>
        </div>

        <!-- Footer -->
        <div
          class="flex shrink-0 items-center justify-between gap-2 border-t border-[var(--md-sys-color-outline-variant)]/50 bg-[var(--md-sys-color-surface-container)]/70 px-5 py-3"
        >
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
              :disabled="running || bookEntries.length === 0"
              class="md-btn md-btn-sm md-btn-filled md-state"
              @click="handleRun"
            >
              <LuLoader2 v-if="running" class="h-3.5 w-3.5 animate-spin" />
              <LuPackageCheck v-else class="h-3.5 w-3.5" />
              <span>{{ running ? '正在转换与压缩…' : `开始转换 (${bookEntries.length} 本)` }}</span>
            </button>
          </div>
        </div>
      </div>
    </div>
  </Teleport>
</template>
