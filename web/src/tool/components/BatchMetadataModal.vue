<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import {
  LuAlertCircle,
  LuCheckCircle2,
  LuChevronDown,
  LuChevronUp,
  LuLanguages,
  LuLoader2,
  LuPencil,
  LuReplace,
  LuRotateCcw,
  LuUndo2,
  LuWand2,
} from 'vue-icons-plus/lu'
import AppCheckbox from '@/components/ui/AppCheckbox.vue'
import MdDialog from '@/components/ui/MdDialog.vue'
import MdSegmentedButton from '@/components/ui/MdSegmentedButton.vue'

import type { MediaKind } from '@/types/api'
import { toolApi } from '../api'
import type {
  BatchMetadataEditInputDto,
  BatchMetadataEditReportDto,
  MediaFileEntry,
} from '../types'

const props = defineProps<{
  open: boolean
  rootId: string
  domain: MediaKind
  entries: MediaFileEntry[]
}>()

const emit = defineEmits<{
  close: []
  completed: [report: BatchMetadataEditReportDto]
  openHistory: []
}>()

const titleTemplate = ref('')
const creator = ref('')
const seriesOrAlbum = ref('')
const publisherOrStudio = ref('')
const yearText = ref('')
const language = ref('')
const genre = ref('')
const addTagsText = ref('')
const removeTagsText = ref('')

const replaceFind = ref('')
const replaceWith = ref('')
const replaceRegex = ref(false)
const replaceFields = ref<string[]>(['title', 'series'])

const zhDirection = ref<'none' | 't2s' | 's2t'>('none')

const showFilesList = ref(false)
const submitting = ref(false)
const undoing = ref(false)
const errorMsg = ref<string | null>(null)
const report = ref<BatchMetadataEditReportDto | null>(null)
const undoSuccessMsg = ref<string | null>(null)

watch(
  () => props.open,
  (isOpen) => {
    if (isOpen) {
      errorMsg.value = null
      report.value = null
      undoSuccessMsg.value = null
      showFilesList.value = false
    }
  },
)

const domainFieldLabels = computed(() => {
  switch (props.domain) {
    case 'book':
      return {
        domainName: '电子书',
        creator: '作者',
        creatorPlaceholder: '多个作者可用逗号分隔（留空保持原值）',
        series: '所属系列',
        seriesPlaceholder: '例如：三体（留空保持原值）',
        publisher: '出版社',
        publisherPlaceholder: '例如：重庆出版社',
        genre: '分类 / 主题',
        showGenre: false,
      }
    case 'comic':
      return {
        domainName: '漫画',
        creator: '作者 / 编剧',
        creatorPlaceholder: '多个作者可用逗号分隔（留空保持原值）',
        series: '系列名',
        seriesPlaceholder: '例如：海贼王（留空保持原值）',
        publisher: '出版社 / 汉化组',
        publisherPlaceholder: '例如：集英社',
        genre: '题材 / 流派',
        showGenre: true,
      }
    case 'music':
      return {
        domainName: '音乐',
        creator: '艺术家 / 歌手',
        creatorPlaceholder: '例如：周杰伦（留空保持原值）',
        series: '专辑名',
        seriesPlaceholder: '例如：范特西（留空保持原值）',
        publisher: '专辑艺术家（可选）',
        publisherPlaceholder: '通常留空或填写主艺人名',
        genre: '音乐流派',
        showGenre: true,
      }
    case 'video':
      return {
        domainName: '视频',
        creator: '导演',
        creatorPlaceholder: '多个导演可用逗号分隔（留空保持原值）',
        series: '所属剧集名',
        seriesPlaceholder: '剧集条目可统一归并剧集名',
        publisher: '制片厂 / 工作室',
        publisherPlaceholder: '例如：京都动画 / A24',
        genre: '类型 / 流派',
        showGenre: true,
      }
  }
})

const templateTokens = [
  { token: '${filename}', label: '文件名' },
  { token: '${title}', label: '原标题' },
  { token: '${creator}', label: '作者/艺人' },
  { token: '${series}', label: '系列/专辑' },
  { token: '${year}', label: '年份' },
  { token: '${counter:2}', label: '序号(01)' },
]

const replacePresets = [
  { label: '去【】水印', find: '【[^】]+】', replace: '', regex: true },
  { label: '去[]标签', find: '\\[[^\\]]+\\]', replace: '', regex: true },
  { label: '去()括号', find: '\\([^)]+\\)', replace: '', regex: true },
  { label: '下划线转空格', find: '_', replace: ' ', regex: false },
]

const replaceFieldChoices = [
  { id: 'title', label: '标题' },
  { id: 'series', label: '系列 / 专辑' },
  { id: 'creator', label: '作者 / 艺人' },
  { id: 'summary', label: '简介' },
]

function insertToken(tok: string): void {
  titleTemplate.value = titleTemplate.value ? `${titleTemplate.value}${tok}` : tok
}

function applyReplacePreset(p: { find: string; replace: string; regex: boolean }): void {
  replaceFind.value = p.find
  replaceWith.value = p.replace
  replaceRegex.value = p.regex
  if (replaceFields.value.length === 0) {
    replaceFields.value = ['title', 'series']
  }
}

function toggleReplaceField(fieldId: string): void {
  const next = new Set(replaceFields.value)
  if (next.has(fieldId)) {
    next.delete(fieldId)
  } else {
    next.add(fieldId)
  }
  replaceFields.value = [...next]
}

function splitTokens(raw: string): string[] {
  return raw
    .split(/[,，、;；\n]+/)
    .map((s) => s.trim())
    .filter(Boolean)
}

function previewFirstTitle(): string | null {
  const tpl = titleTemplate.value.trim()
  const first = props.entries[0]
  if (!tpl || !first) return null
  const stem = first.file_name.replace(/\.[^.]+$/, '')
  let out = tpl
    .replaceAll('${filename}', stem)
    .replaceAll('${title}', first.display_title || stem)
    .replaceAll('${creator}', creator.value.trim() || first.display_subtitle || '')
    .replaceAll('${series}', seriesOrAlbum.value.trim() || '')
    .replaceAll('${year}', yearText.value.trim() || (first.year ? String(first.year) : ''))
    .replaceAll('${counter}', '1')
    .replaceAll('${counter:2}', '01')
    .replaceAll('${counter:3}', '001')
  out = out.trim()
  return out || null
}

const firstTitlePreview = computed(() => previewFirstTitle())

const hasAnyRule = computed(() => {
  return Boolean(
    titleTemplate.value.trim() ||
      creator.value.trim() ||
      seriesOrAlbum.value.trim() ||
      publisherOrStudio.value.trim() ||
      yearText.value.trim() ||
      language.value.trim() ||
      genre.value.trim() ||
      addTagsText.value.trim() ||
      removeTagsText.value.trim() ||
      replaceFind.value.length > 0 ||
      zhDirection.value !== 'none',
  )
})

function resetForm(): void {
  titleTemplate.value = ''
  creator.value = ''
  seriesOrAlbum.value = ''
  publisherOrStudio.value = ''
  yearText.value = ''
  language.value = ''
  genre.value = ''
  addTagsText.value = ''
  removeTagsText.value = ''
  replaceFind.value = ''
  replaceWith.value = ''
  replaceRegex.value = false
  replaceFields.value = ['title', 'series']
  zhDirection.value = 'none'
  errorMsg.value = null
  report.value = null
  undoSuccessMsg.value = null
}

async function handleSubmit(): Promise<void> {
  if (props.entries.length === 0 || !hasAnyRule.value || submitting.value) return
  errorMsg.value = null
  undoSuccessMsg.value = null

  const parsedYear = yearText.value.trim() ? Number.parseInt(yearText.value.trim(), 10) : null
  if (yearText.value.trim() && (!Number.isFinite(parsedYear) || (parsedYear ?? 0) <= 0)) {
    errorMsg.value = '年份必须为有效的正整数（例如 2024）'
    return
  }

  const payload: BatchMetadataEditInputDto = {
    paths: props.entries.map((e) => e.rel_path),
    media_kind: props.domain,
  }
  if (titleTemplate.value.trim()) payload.title_template = titleTemplate.value.trim()
  if (creator.value.trim()) payload.creator = creator.value.trim()
  if (seriesOrAlbum.value.trim()) payload.series_or_album = seriesOrAlbum.value.trim()
  if (publisherOrStudio.value.trim()) payload.publisher_or_studio = publisherOrStudio.value.trim()
  if (parsedYear && parsedYear > 0) payload.year = parsedYear
  if (language.value.trim()) payload.language = language.value.trim()
  if (genre.value.trim()) payload.genre = genre.value.trim()

  const addTags = splitTokens(addTagsText.value)
  if (addTags.length > 0) payload.add_tags = addTags
  const removeTags = splitTokens(removeTagsText.value)
  if (removeTags.length > 0) payload.remove_tags = removeTags

  if (replaceFind.value.length > 0) {
    payload.replace_rule = {
      find: replaceFind.value,
      replace: replaceWith.value,
      use_regex: replaceRegex.value,
      fields: replaceFields.value.length > 0 ? replaceFields.value : ['title', 'series'],
    }
  }
  if (zhDirection.value !== 'none') {
    payload.zh_direction = zhDirection.value
  }

  submitting.value = true
  try {
    const res = await toolApi.batchEditMetadata(props.rootId, payload)
    report.value = res
    emit('completed', res)
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
  } finally {
    submitting.value = false
  }
}

async function handleUndoLastBatch(): Promise<void> {
  if (!report.value?.batch_id || undoing.value) return
  undoing.value = true
  errorMsg.value = null
  try {
    const res = await toolApi.undoMetadataBatch(report.value.batch_id)
    undoSuccessMsg.value = `已回滚 ${res.restored_count} 个文件至修改前的元数据状态${
      res.failed_paths.length > 0 ? `（${res.failed_paths.length} 项失败）` : ''
    }`
    report.value = { ...report.value, batch_id: null }
    emit('completed', {
      batch_id: null,
      updated_count: res.restored_count,
      skipped_count: 0,
      failed_count: res.failed_paths.length,
      errors: res.failed_paths,
    })
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
  } finally {
    undoing.value = false
  }
}
</script>

<template>
  <MdDialog
    :open="open"
    :title="'批量元数据编辑 · ' + domainFieldLabels.domainName"
    :subtitle="'已选 ' + entries.length + ' 个文件 · 仅修改非空字段'"
    max-width="max-w-2xl"
    @close="emit('close')"
  >
    <template #icon>
      <LuPencil class="h-5 w-5" />
    </template>

    <template #headerActions>
      <button
        type="button"
        class="md-btn md-btn-sm md-btn-tonal md-state"
        @click="emit('openHistory')"
      >
        <LuUndo2 class="h-3.5 w-3.5" />
        <span>变更历史</span>
      </button>
    </template>

    <div class="space-y-5 py-2">
      <!-- Selected Files Collapsible Preview -->
      <div class="rounded-2xl border border-[var(--md-sys-color-outline-variant)]/60 bg-[var(--md-sys-color-surface-container-low)]">
        <button
          type="button"
          class="flex w-full items-center justify-between px-4 py-2.5 text-left text-xs"
          @click="showFilesList = !showFilesList"
        >
          <span class="font-medium text-[var(--md-sys-color-on-surface)]">
            待处理目标：{{ entries.length }} 项
            <span v-if="entries[0]" class="ml-1.5 font-normal text-[var(--md-sys-color-on-surface-variant)]">
              （首项：{{ entries[0].file_name }}）
            </span>
          </span>
          <LuChevronUp v-if="showFilesList" class="h-4 w-4 text-[var(--md-sys-color-on-surface-variant)]" />
          <LuChevronDown v-else class="h-4 w-4 text-[var(--md-sys-color-on-surface-variant)]" />
        </button>
        <div
          v-if="showFilesList"
          class="max-h-32 divide-y divide-[var(--md-sys-color-outline-variant)]/40 overflow-y-auto border-t border-[var(--md-sys-color-outline-variant)]/50 px-4 py-1.5"
        >
          <div
            v-for="(item, idx) in entries"
            :key="item.rel_path"
            class="flex items-center justify-between gap-2 py-1 text-[11px]"
          >
            <span class="truncate text-[var(--md-sys-color-on-surface)]">
              <span class="mr-1.5 font-mono text-[10px] text-[var(--md-sys-color-on-surface-variant)]">
                {{ String(idx + 1).padStart(2, '0') }}
              </span>
              {{ item.file_name }}
            </span>
            <span class="shrink-0 truncate text-[10px] text-[var(--md-sys-color-on-surface-variant)]">
              {{ item.display_title }}
            </span>
          </div>
        </div>
      </div>

      <!-- Feedback Banners -->
      <div
        v-if="errorMsg"
        class="flex items-start gap-2 rounded-2xl bg-[var(--md-sys-color-error-container)] px-4 py-3 text-xs text-[var(--md-sys-color-on-error-container)]"
      >
        <LuAlertCircle class="mt-0.5 h-4 w-4 shrink-0" />
        <span>{{ errorMsg }}</span>
      </div>

      <div
        v-if="undoSuccessMsg"
        class="flex items-center gap-2 rounded-2xl bg-[var(--md-sys-color-success-container)] px-4 py-3 text-xs font-medium text-[var(--md-sys-color-on-success-container)]"
      >
        <LuCheckCircle2 class="h-4 w-4 shrink-0" />
        <span>{{ undoSuccessMsg }}</span>
      </div>

      <div
        v-if="report"
        class="space-y-2 rounded-2xl bg-[var(--md-sys-color-success-container)] px-4 py-3 text-xs text-[var(--md-sys-color-on-success-container)]"
      >
        <div class="flex flex-wrap items-center justify-between gap-2">
          <div class="flex items-center gap-2 font-semibold">
            <LuCheckCircle2 class="h-4 w-4 shrink-0" />
            <span>
              批量更新完成：成功 {{ report.updated_count }} 项，无变化跳过 {{ report.skipped_count }} 项
              <template v-if="report.failed_count > 0">，失败 {{ report.failed_count }} 项</template>
            </span>
          </div>

          <button
            v-if="report.batch_id"
            type="button"
            :disabled="undoing"
            class="md-btn md-btn-sm md-state bg-[var(--md-sys-color-warning-container)] text-[var(--md-sys-color-on-warning-container)]"
            @click="handleUndoLastBatch"
          >
            <LuLoader2 v-if="undoing" class="h-3 w-3 animate-spin" />
            <LuRotateCcw v-else class="h-3 w-3" />
            <span>立即撤销本次修改</span>
          </button>
        </div>
        <ul
          v-if="report.errors.length > 0"
          class="max-h-24 space-y-0.5 overflow-y-auto pl-6 font-mono text-[10px] text-[var(--md-sys-color-error)]"
        >
          <li v-for="e in report.errors" :key="e">{{ e }}</li>
        </ul>
      </div>

      <!-- Section 1: Title Template -->
      <section class="space-y-2">
        <div class="flex flex-wrap items-center justify-between gap-2">
          <label class="text-xs font-bold text-[var(--md-sys-color-on-surface)]">
            标题模板（可选）
          </label>
          <div class="flex flex-wrap gap-1">
            <button
              v-for="t in templateTokens"
              :key="t.token"
              type="button"
              class="md-state rounded-lg border border-[var(--md-sys-color-outline-variant)]/70 bg-[var(--md-sys-color-surface-container-low)] px-2 py-0.5 font-mono text-[10px] text-[var(--md-sys-color-on-surface-variant)] transition hover:border-[var(--md-sys-color-primary)] hover:text-[var(--md-sys-color-primary)]"
              :title="`插入 ${t.label} 变量`"
              @click="insertToken(t.token)"
            >
              {{ t.token }}
            </button>
          </div>
        </div>
        <input
          v-model="titleTemplate"
          type="text"
          placeholder="例如：${counter:2}. ${filename}（留空保持原标题）"
          class="md-input h-10 w-full text-xs"
        />
        <p
          v-if="firstTitlePreview"
          class="text-[11px] text-[var(--md-sys-color-on-surface-variant)]"
        >
          首项预览：<span class="font-semibold text-[var(--md-sys-color-primary)]">{{ firstTitlePreview }}</span>
        </p>
      </section>

      <!-- Section 2: Common Field Overrides -->
      <section class="space-y-3">
        <div class="text-xs font-bold text-[var(--md-sys-color-on-surface)]">
          统一覆盖字段（留空即跳过）
        </div>
        <div class="grid grid-cols-1 gap-3 sm:grid-cols-2">
          <div>
            <label class="mb-1 block text-[11px] font-medium text-[var(--md-sys-color-on-surface-variant)]">
              {{ domainFieldLabels.creator }}
            </label>
            <input
              v-model="creator"
              type="text"
              :placeholder="domainFieldLabels.creatorPlaceholder"
              class="md-input h-9 w-full text-xs"
            />
          </div>

          <div>
            <label class="mb-1 block text-[11px] font-medium text-[var(--md-sys-color-on-surface-variant)]">
              {{ domainFieldLabels.series }}
            </label>
            <input
              v-model="seriesOrAlbum"
              type="text"
              :placeholder="domainFieldLabels.seriesPlaceholder"
              class="md-input h-9 w-full text-xs"
            />
          </div>

          <div>
            <label class="mb-1 block text-[11px] font-medium text-[var(--md-sys-color-on-surface-variant)]">
              {{ domainFieldLabels.publisher }}
            </label>
            <input
              v-model="publisherOrStudio"
              type="text"
              :placeholder="domainFieldLabels.publisherPlaceholder"
              class="md-input h-9 w-full text-xs"
            />
          </div>

          <div class="grid grid-cols-2 gap-2.5">
            <div>
              <label class="mb-1 block text-[11px] font-medium text-[var(--md-sys-color-on-surface-variant)]">
                年份
              </label>
              <input
                v-model="yearText"
                type="text"
                inputmode="numeric"
                placeholder="如 2024"
                class="md-input h-9 w-full text-xs"
              />
            </div>
            <div>
              <label class="mb-1 block text-[11px] font-medium text-[var(--md-sys-color-on-surface-variant)]">
                语言代码
              </label>
              <input
                v-model="language"
                type="text"
                placeholder="zh / ja / en"
                class="md-input h-9 w-full text-xs"
              />
            </div>
          </div>

          <div v-if="domainFieldLabels.showGenre" class="sm:col-span-2">
            <label class="mb-1 block text-[11px] font-medium text-[var(--md-sys-color-on-surface-variant)]">
              {{ domainFieldLabels.genre }}
            </label>
            <input
              v-model="genre"
              type="text"
              placeholder="多个流派可用逗号分隔（音乐取首个）"
              class="md-input h-9 w-full text-xs"
            />
          </div>
        </div>
      </section>

      <!-- Section 3: Tag Add / Remove -->
      <section v-if="domain !== 'music'" class="space-y-2.5">
        <div class="text-xs font-bold text-[var(--md-sys-color-on-surface)]">
          标签增量增删（保留文件原有其它标签）
        </div>
        <div class="grid grid-cols-1 gap-3 sm:grid-cols-2">
          <div>
            <label class="mb-1 block text-[11px] font-medium text-[var(--md-sys-color-success)]">
              + 追加标签
            </label>
            <input
              v-model="addTagsText"
              type="text"
              placeholder="逗号分隔，如：科幻, 雨果奖"
              class="md-input h-9 w-full text-xs"
            />
          </div>
          <div>
            <label class="mb-1 block text-[11px] font-medium text-[var(--md-sys-color-warning)]">
              − 移除标签
            </label>
            <input
              v-model="removeTagsText"
              type="text"
              placeholder="逗号分隔，如：待整理, 广告"
              class="md-input h-9 w-full text-xs"
            />
          </div>
        </div>
      </section>

      <!-- Section 4: Regex / Text Replace -->
      <section
        class="space-y-3 rounded-2xl border border-[var(--md-sys-color-outline-variant)]/60 bg-[var(--md-sys-color-surface-container-low)] p-4"
      >
        <div class="flex flex-wrap items-center justify-between gap-2">
          <div class="flex items-center gap-1.5 text-xs font-bold text-[var(--md-sys-color-on-surface)]">
            <LuReplace class="h-3.5 w-3.5 text-[var(--md-sys-color-primary)]" />
            <span>查找替换 / 水印清洗</span>
          </div>
          <div class="flex flex-wrap gap-1">
            <button
              v-for="p in replacePresets"
              :key="p.label"
              type="button"
              class="md-state rounded-lg border border-[var(--md-sys-color-outline-variant)]/70 bg-[var(--md-sys-color-surface)] px-2 py-0.5 text-[10px] font-medium text-[var(--md-sys-color-on-surface-variant)] transition hover:border-[var(--md-sys-color-primary)] hover:text-[var(--md-sys-color-primary)]"
              @click="applyReplacePreset(p)"
            >
              {{ p.label }}
            </button>
          </div>
        </div>

        <div class="grid grid-cols-1 gap-2.5 sm:grid-cols-2">
          <input
            v-model="replaceFind"
            type="text"
            placeholder="查找文本或正则表达式"
            class="md-input h-9 w-full font-mono text-xs"
          />
          <input
            v-model="replaceWith"
            type="text"
            placeholder="替换为（留空即删除匹配内容）"
            class="md-input h-9 w-full font-mono text-xs"
          />
        </div>

        <div class="flex flex-wrap items-center justify-between gap-2">
          <div class="flex flex-wrap items-center gap-1.5">
            <span class="text-[11px] text-[var(--md-sys-color-on-surface-variant)]">作用字段：</span>
            <button
              v-for="f in replaceFieldChoices"
              :key="f.id"
              type="button"
              class="md-chip md-state !h-7 !rounded-full !px-2.5 !text-[11px]"
              :class="replaceFields.includes(f.id) ? 'md-chip-selected' : ''"
              @click="toggleReplaceField(f.id)"
            >
              {{ f.label }}
            </button>
          </div>

          <AppCheckbox v-model="replaceRegex" label="启用正则 (Regex)" />
        </div>
      </section>

      <!-- Section 5: Traditional <-> Simplified Conversion -->
      <section class="flex flex-wrap items-center justify-between gap-3 rounded-2xl border border-[var(--md-sys-color-outline-variant)]/60 bg-[var(--md-sys-color-surface-container-low)] px-4 py-3">
        <div class="flex items-center gap-2">
          <LuLanguages class="h-4 w-4 text-[var(--md-sys-color-primary)]" />
          <div>
            <div class="text-xs font-bold text-[var(--md-sys-color-on-surface)]">元数据繁简统一转换</div>
            <div class="text-[11px] text-[var(--md-sys-color-on-surface-variant)]">
              自动转换标题、作者、系列、简介与标签文本
            </div>
          </div>
        </div>

        <MdSegmentedButton
          v-model="zhDirection"
          :options="[
            { value: 'none', label: '不转换' },
            { value: 't2s', label: '繁 → 简' },
            { value: 's2t', label: '简 → 繁' },
          ]"
          size="sm"
        />
      </section>
    </div>

    <template #footer>
      <button
        type="button"
        class="md-btn md-btn-sm md-btn-text md-state"
        @click="resetForm"
      >
        <LuRotateCcw class="h-3.5 w-3.5" />
        <span>重置表单</span>
      </button>

      <div class="flex items-center gap-2">
        <button
          type="button"
          class="md-btn md-btn-sm md-btn-text md-state"
          @click="emit('close')"
        >
          关闭
        </button>
        <button
          type="button"
          :disabled="submitting || !hasAnyRule || entries.length === 0"
          class="md-btn md-btn-sm md-btn-filled md-state"
          @click="handleSubmit"
        >
          <LuLoader2 v-if="submitting" class="h-3.5 w-3.5 animate-spin" />
          <LuWand2 v-else class="h-3.5 w-3.5" />
          <span>{{ submitting ? '写入中...' : '应用到 ' + entries.length + ' 个文件' }}</span>
        </button>
      </div>
    </template>
  </MdDialog>
</template>
