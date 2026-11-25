<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, watch } from 'vue'
import {
  LuAlertTriangle,
  LuArrowRight,
  LuCheck,
  LuFileSignature,
  LuRefreshCw,
  LuUndo2,
} from 'vue-icons-plus/lu'
import AppCheckbox from '@/components/ui/AppCheckbox.vue'
import MdDialog from '@/components/ui/MdDialog.vue'
import MdLinearProgress from '@/components/ui/MdLinearProgress.vue'
import MdSegmentedButton from '@/components/ui/MdSegmentedButton.vue'
import { useActionFeedback } from '@/composables/useActionFeedback'
import type { MediaKind } from '@/types/api'
import { toolApi } from '../api'
import { useToolTaskStore } from '../tasks'
import { DOMAIN_RENAME_TEMPLATE_SPECS } from '../templates'
import type {
  MediaFileEntry,
  RenameConflictPolicy,
  RenamePlanItem,
  RenameTemplatesDto,
} from '../types'

const props = defineProps<{
  open: boolean
  rootId: string
  entries: MediaFileEntry[]
  skippedOrganizedCount?: number
  templates: RenameTemplatesDto
}>()

const emit = defineEmits<{
  close: []
  renamed: [movedCount: number, movedMap: Record<string, string>]
  openHistory: []
}>()

const templateInput = ref('')
const episodeTemplateInput = ref('')
const conflictPolicy = ref<RenameConflictPolicy>('skip')
const focusedVideoInput = ref<'movie' | 'episode'>('movie')
const movieInputRef = ref<HTMLInputElement | null>(null)
const episodeInputRef = ref<HTMLInputElement | null>(null)
const genericInputRef = ref<HTMLInputElement | null>(null)
const plans = ref<RenamePlanItem[]>([])
const excludedPaths = ref<Set<string>>(new Set())
const showUnchanged = ref(false)
const taskStore = useToolTaskStore()
const loadingPreview = ref(false)
const previewFeedback = useActionFeedback()
const previewSpinning = computed(() => loadingPreview.value || previewFeedback.running.value)
const executing = ref(false)
const inlineProgressPct = ref<number | null>(null)
const errorMsg = ref<string | null>(null)

const dominantKind = computed<MediaKind>(() => props.entries[0]?.media_kind ?? 'music')

const tokenChips = computed<string[]>(() => DOMAIN_RENAME_TEMPLATE_SPECS[dominantKind.value].quickChips)

function seedTemplatesForEntries(): void {
  const k = dominantKind.value
  if (k === 'music') {
    templateInput.value = props.templates.music
    episodeTemplateInput.value = ''
  } else if (k === 'comic') {
    templateInput.value = props.templates.comic
    episodeTemplateInput.value = ''
  } else if (k === 'book') {
    templateInput.value = props.templates.book
    episodeTemplateInput.value = ''
  } else {
    templateInput.value = props.templates.video_movie
    episodeTemplateInput.value = props.templates.video_episode
    const looksLikeEpisode = props.entries.some((e) => /s\d{1,2}e\d{1,3}/i.test(e.file_name))
    focusedVideoInput.value = looksLikeEpisode ? 'episode' : 'movie'
  }
}

let debounceTimer: ReturnType<typeof setTimeout> | null = null
let requestSeq = 0
let suppressPreviewWatch = false

function clearDebounceTimer(): void {
  if (debounceTimer) {
    clearTimeout(debounceTimer)
    debounceTimer = null
  }
}

async function runPreview(): Promise<void> {
  if (!props.open || props.entries.length === 0) return
  const seq = ++requestSeq
  loadingPreview.value = true
  inlineProgressPct.value = 0
  errorMsg.value = null
  const taskId = 'rename:preview'
  taskStore.startTask({
    id: taskId,
    kind: 'rename',
    title: `生成整理归档预览 (${props.entries.length} 项)`,
    total: props.entries.length,
    ephemeral: true,
  })
  try {
    const nextPlans = await toolApi.previewRename(
      props.rootId,
      props.entries.map((e) => e.rel_path),
      templateInput.value.trim() || null,
      dominantKind.value,
      dominantKind.value === 'video' ? episodeTemplateInput.value.trim() || null : undefined,
      (prog) => {
        if (seq !== requestSeq) return
        taskStore.updateTask(taskId, prog)
        if (prog.total && prog.total > 0) {
          inlineProgressPct.value = Math.min(100, Math.round((prog.processed / prog.total) * 100))
        }
      },
      conflictPolicy.value,
    )
    if (seq !== requestSeq || !props.open) return
    plans.value = nextPlans
    taskStore.completeTask(taskId, `已生成 ${nextPlans.length.toLocaleString()} 条预览方案`)
  } catch (err) {
    if (seq !== requestSeq) return
    plans.value = []
    const msg = err instanceof Error ? err.message : String(err)
    errorMsg.value = msg
    taskStore.failTask(taskId, msg)
  } finally {
    if (seq === requestSeq) {
      loadingPreview.value = false
      inlineProgressPct.value = null
    }
  }
}

watch(
  () => props.open,
  (isOpen) => {
    clearDebounceTimer()
    if (isOpen) {
      suppressPreviewWatch = true
      excludedPaths.value = new Set()
      showUnchanged.value = false
      conflictPolicy.value = 'skip'
      seedTemplatesForEntries()
      void nextTick(() => {
        suppressPreviewWatch = false
      })
      void runPreview()
    } else {
      requestSeq += 1
    }
  },
  { immediate: true },
)

watch([templateInput, episodeTemplateInput], () => {
  if (!props.open || suppressPreviewWatch) return
  clearDebounceTimer()
  debounceTimer = setTimeout(() => {
    debounceTimer = null
    void runPreview()
  }, 260)
})

watch(conflictPolicy, () => {
  if (!props.open || suppressPreviewWatch) return
  clearDebounceTimer()
  void runPreview()
})

onBeforeUnmount(() => {
  clearDebounceTimer()
  requestSeq += 1
})

async function insertTokenAtCursor(
  model: { value: string },
  inputEl: HTMLInputElement | null,
  tok: string,
): Promise<void> {
  if (!inputEl) {
    model.value = `${model.value}${tok}`
    return
  }
  const start = inputEl.selectionStart ?? model.value.length
  const end = inputEl.selectionEnd ?? start
  const nextVal = `${model.value.slice(0, start)}${tok}${model.value.slice(end)}`
  model.value = nextVal
  await nextTick()
  inputEl.focus()
  const caret = start + tok.length
  inputEl.setSelectionRange(caret, caret)
}

function appendChip(tok: string): void {
  if (dominantKind.value === 'video' && focusedVideoInput.value === 'episode') {
    void insertTokenAtCursor(episodeTemplateInput, episodeInputRef.value, tok)
  } else if (dominantKind.value === 'video') {
    void insertTokenAtCursor(templateInput, movieInputRef.value, tok)
  } else {
    void insertTokenAtCursor(templateInput, genericInputRef.value, tok)
  }
}

function toggleRowExcluded(fromRelPath: string): void {
  const next = new Set(excludedPaths.value)
  if (next.has(fromRelPath)) {
    next.delete(fromRelPath)
  } else {
    next.add(fromRelPath)
  }
  excludedPaths.value = next
}

const activePlans = computed(() =>
  plans.value.filter((p) => !excludedPaths.value.has(p.from_rel_path)),
)

const unchangedCount = computed(() => plans.value.filter((p) => p.unchanged).length)
const visiblePlans = computed(() =>
  showUnchanged.value ? plans.value : plans.value.filter((p) => !p.unchanged),
)

const actionableCount = computed(
  () => activePlans.value.filter((p) => !p.unchanged && !p.conflict).length,
)

const conflictCount = computed(() => activePlans.value.filter((p) => Boolean(p.conflict)).length)
const overwriteCount = computed(
  () => activePlans.value.filter((p) => !p.unchanged && !p.conflict && p.will_overwrite).length,
)

async function handleExecute(): Promise<void> {
  if (actionableCount.value === 0 || executing.value) return
  executing.value = true
  inlineProgressPct.value = 0
  errorMsg.value = null
  const totalToMove = actionableCount.value
  const taskId = `rename:execute:${Date.now()}`
  taskStore.startTask({
    id: taskId,
    kind: 'rename',
    title: `批量重命名归档 (${totalToMove} 项)`,
    total: totalToMove,
    ephemeral: false,
  })
  try {
    const selected = activePlans.value.filter((p) => !p.unchanged && !p.conflict)
    if (selected.length === 0) return
    const movedMap: Record<string, string> = {}
    for (const p of selected) {
      movedMap[p.from_rel_path] = p.to_rel_path
    }
    const res = await toolApi.executeRename(props.rootId, selected, (prog) => {
      taskStore.updateTask(taskId, prog)
      if (prog.total && prog.total > 0) {
        inlineProgressPct.value = Math.min(100, Math.round((prog.processed / prog.total) * 100))
      }
    })
    taskStore.completeTask(taskId, `已完成 ${res.moved_count.toLocaleString()} 个文件的重命名与归档`)
    emit('renamed', res.moved_count, movedMap)
    emit('close')
  } catch (err) {
    const msg = err instanceof Error ? err.message : String(err)
    errorMsg.value = msg
    taskStore.failTask(taskId, msg)
  } finally {
    executing.value = false
    inlineProgressPct.value = null
  }
}
</script>

<template>
  <MdDialog
    :open="open"
    :title="`按元信息模板整理归档 (${entries.length} 个文件)`"
    subtitle="同名外挂文件（.opf / .lrc / .nfo / .srt / .ass）将自动跟随归档"
    max-width="max-w-4xl"
    :close-on-backdrop="!executing"
    @close="!executing && emit('close')"
  >
    <template #icon>
      <LuFileSignature class="h-5 w-5" />
    </template>

    <div class="space-y-4 py-1">
      <MdLinearProgress
        v-if="(loadingPreview || executing) && inlineProgressPct !== null"
        :value="inlineProgressPct"
        aria-label="整理归档进度"
      />

      <!-- Template Editor -->
      <div class="space-y-2.5 rounded-2xl border border-[var(--md-sys-color-outline-variant)]/55 bg-[var(--md-sys-color-surface-container-low)] p-3.5">
        <template v-if="dominantKind === 'video'">
          <div class="grid grid-cols-1 gap-2 sm:grid-cols-12">
            <label class="block sm:col-span-5">
              <span class="mb-1 block text-[11px] font-medium text-[var(--md-sys-color-on-surface-variant)]">
                电影归档模板（对非剧集生效）
              </span>
              <input
                ref="movieInputRef"
                v-model="templateInput"
                type="text"
                placeholder="{title} ({year})/{title} ({year})"
                class="md-input h-9 w-full font-mono text-xs"
                @focus="focusedVideoInput = 'movie'"
              />
            </label>
            <label class="block sm:col-span-5">
              <span class="mb-1 block text-[11px] font-medium text-[var(--md-sys-color-on-surface-variant)]">
                剧集归档模板（对剧集 SxxExx 生效）
              </span>
              <input
                ref="episodeInputRef"
                v-model="episodeTemplateInput"
                type="text"
                placeholder="{show} ({year})/Season {season:2}/{show} - S{season:2}E{episode:2} - {title}"
                class="md-input h-9 w-full font-mono text-xs"
                @focus="focusedVideoInput = 'episode'"
              />
            </label>
            <div class="flex items-end sm:col-span-2">
              <button
                type="button"
                :disabled="previewSpinning"
                class="md-btn md-btn-sm md-btn-outlined md-state w-full"
                :class="
                  previewFeedback.done.value
                    ? '!border-[var(--md-sys-color-success)] !text-[var(--md-sys-color-success)]'
                    : ''
                "
                @click="previewFeedback.run(runPreview)"
              >
                <LuCheck v-if="previewFeedback.done.value && !previewSpinning" class="h-3.5 w-3.5" />
                <LuRefreshCw v-else class="h-3.5 w-3.5" :class="previewSpinning ? 'animate-spin' : ''" />
                <span>{{ previewFeedback.done.value && !previewSpinning ? '已刷新' : '刷新' }}</span>
              </button>
            </div>
          </div>
        </template>
        <div v-else class="flex items-center gap-2">
          <input
            ref="genericInputRef"
            v-model="templateInput"
            type="text"
            placeholder="输入重命名模板，例如 {track:2} - {artist} - {title}"
            class="md-input h-9 flex-1 font-mono text-xs"
          />
          <button
            type="button"
            :disabled="previewSpinning"
            class="md-btn md-btn-sm md-btn-outlined md-state"
            :class="
              previewFeedback.done.value
                ? '!border-[var(--md-sys-color-success)] !text-[var(--md-sys-color-success)]'
                : ''
            "
            @click="previewFeedback.run(runPreview)"
          >
            <LuCheck v-if="previewFeedback.done.value && !previewSpinning" class="h-3.5 w-3.5" />
            <LuRefreshCw v-else class="h-3.5 w-3.5" :class="previewSpinning ? 'animate-spin' : ''" />
            <span>{{ previewFeedback.done.value && !previewSpinning ? '已刷新' : '刷新预览' }}</span>
          </button>
        </div>

        <div class="flex flex-wrap items-center gap-1.5">
          <span class="text-[11px] text-[var(--md-sys-color-on-surface-variant)]">可用占位符：</span>
          <button
            v-for="chip in tokenChips"
            :key="chip"
            type="button"
            class="md-state rounded-lg border border-[var(--md-sys-color-outline-variant)]/70 bg-[var(--md-sys-color-surface)] px-2 py-0.5 font-mono text-[11px] text-[var(--md-sys-color-primary)] transition hover:border-[var(--md-sys-color-primary)]"
            @click="appendChip(chip)"
          >
            {{ chip }}
          </button>
        </div>

        <div class="flex flex-wrap items-center gap-2 border-t border-[var(--md-sys-color-outline-variant)]/45 pt-2.5">
          <span class="text-[11px] font-medium text-[var(--md-sys-color-on-surface-variant)]">目标冲突处理：</span>
          <MdSegmentedButton
            v-model="conflictPolicy"
            :options="[
              { value: 'skip', label: '跳过冲突', title: '目标路径已存在时跳过，保持原文件不动' },
              { value: 'auto_suffix', label: '自动追加序号 (2)', title: '目标已存在时自动追加 (2)/(3) 共存归档' },
              { value: 'overwrite', label: '覆盖已有文件', title: '直接覆盖目标位置的已有文件（请谨慎）' },
            ]"
            size="sm"
          />
          <span
            v-if="conflictCount > 0 && conflictPolicy === 'skip'"
            class="text-[11px] text-[var(--md-sys-color-warning)]"
          >
            当前有 {{ conflictCount }} 项冲突，可切换为「自动追加序号」或「覆盖已有文件」
          </span>
          <span v-else-if="overwriteCount > 0" class="text-[11px] text-[var(--md-sys-color-warning)]">
            注意：将覆盖磁盘上 {{ overwriteCount }} 个已有文件
          </span>
        </div>
      </div>

      <!-- Preview Diff List -->
      <div class="space-y-2">
        <div
          v-if="errorMsg"
          class="rounded-2xl bg-[var(--md-sys-color-error-container)] px-3.5 py-2.5 text-xs text-[var(--md-sys-color-on-error-container)]"
        >
          {{ errorMsg }}
        </div>

        <div
          v-if="(skippedOrganizedCount ?? 0) > 0 || unchangedCount > 0"
          class="flex flex-wrap items-center justify-between gap-2 rounded-2xl bg-[var(--md-sys-color-surface-container-low)] px-3.5 py-2 text-[11px] text-[var(--md-sys-color-on-surface-variant)]"
        >
          <span>
            已自动跳过
            <template v-if="(skippedOrganizedCount ?? 0) > 0">
              {{ skippedOrganizedCount }} 项已按默认模板归档的条目
            </template>
            <template v-if="(skippedOrganizedCount ?? 0) > 0 && unchangedCount > 0">
              及
            </template>
            <template v-if="unchangedCount > 0">
              {{ unchangedCount }} 项路径无变化的条目
            </template>
          </span>
          <button
            v-if="unchangedCount > 0"
            type="button"
            class="md-btn md-btn-sm md-btn-text md-state !h-7 !px-2.5 !text-[11px]"
            @click="showUnchanged = !showUnchanged"
          >
            {{ showUnchanged ? '隐藏无变化项' : `查看无变化项 (${unchangedCount})` }}
          </button>
        </div>

        <div
          v-if="!loadingPreview && visiblePlans.length === 0 && plans.length > 0"
          class="rounded-2xl border border-dashed border-[var(--md-sys-color-outline-variant)] bg-[var(--md-sys-color-surface-container-low)] py-8 text-center text-xs text-[var(--md-sys-color-on-surface-variant)]"
        >
          当前所选条目的路径均已符合模板，无需整理归档。
        </div>

        <div
          v-for="item in visiblePlans"
          :key="item.from_rel_path"
          class="rounded-2xl border border-[var(--md-sys-color-outline-variant)]/55 bg-[var(--md-sys-color-surface-container-low)] p-3 transition"
          :class="item.unchanged || excludedPaths.has(item.from_rel_path) ? 'opacity-50' : ''"
        >
          <div class="flex flex-wrap items-center gap-2 text-xs">
            <AppCheckbox
              :disabled="item.unchanged"
              :model-value="!item.unchanged && !excludedPaths.has(item.from_rel_path)"
              @change="toggleRowExcluded(item.from_rel_path)"
            />
            <span class="font-mono text-[var(--md-sys-color-on-surface-variant)]">{{ item.from_rel_path }}</span>
            <LuArrowRight class="h-3.5 w-3.5 shrink-0 text-[var(--md-sys-color-on-surface-variant)]" />
            <span
              class="font-mono font-semibold"
              :class="item.unchanged ? 'text-[var(--md-sys-color-on-surface-variant)]' : 'text-[var(--md-sys-color-primary)]'"
            >
              {{ item.to_rel_path }}
            </span>

            <span
              v-if="item.unchanged"
              class="ml-auto rounded-full bg-[var(--md-sys-color-surface-container-high)] px-2 py-0.5 text-[10px] text-[var(--md-sys-color-on-surface-variant)]"
            >
              路径无变化
            </span>
            <span
              v-else-if="item.conflict"
              class="ml-auto inline-flex items-center gap-1 rounded-full bg-[var(--md-sys-color-error-container)] px-2.5 py-0.5 text-[10px] font-semibold text-[var(--md-sys-color-on-error-container)]"
            >
              <LuAlertTriangle class="h-3 w-3" />
              {{ item.conflict }}
            </span>
            <span
              v-else-if="item.will_overwrite"
              class="ml-auto inline-flex items-center gap-1 rounded-full bg-[var(--md-sys-color-warning-container)] px-2.5 py-0.5 text-[10px] font-semibold text-[var(--md-sys-color-on-warning-container)]"
            >
              <LuAlertTriangle class="h-3 w-3" />
              将覆盖已有文件
            </span>
            <span
              v-else
              class="ml-auto inline-flex items-center gap-1 rounded-full bg-[var(--md-sys-color-success-container)] px-2.5 py-0.5 text-[10px] font-semibold text-[var(--md-sys-color-on-success-container)]"
            >
              <LuCheck class="h-3 w-3" />
              可安全归档
            </span>
          </div>

          <div
            v-if="item.companion_moves.length > 0"
            class="mt-1.5 flex flex-wrap items-center gap-1.5 text-[11px] text-[var(--md-sys-color-on-surface-variant)]"
          >
            <span>联动附属文件：</span>
            <span
              v-for="[cFrom, cTo] in item.companion_moves"
              :key="cFrom"
              class="rounded-md bg-[var(--md-sys-color-surface)] px-2 py-0.5 font-mono text-[10px]"
            >
              {{ cFrom }} → {{ cTo }}
            </span>
          </div>
        </div>
      </div>
    </div>

    <template #footer>
      <div class="flex flex-wrap items-center gap-2.5">
        <span class="text-xs text-[var(--md-sys-color-on-surface-variant)]">
          共 {{ actionableCount }} 项待归档（冲突项与未变化项会自动跳过）
        </span>
        <button
          type="button"
          class="md-btn md-btn-sm md-btn-text md-state"
          @click="emit('openHistory')"
        >
          <LuUndo2 class="h-3.5 w-3.5" />
          <span>撤销历史</span>
        </button>
      </div>
      <div class="flex items-center gap-2">
        <button
          type="button"
          class="md-btn md-btn-sm md-btn-text md-state"
          @click="emit('close')"
        >
          取消
        </button>
        <button
          type="button"
          :disabled="actionableCount === 0 || executing"
          class="md-btn md-btn-sm md-btn-filled md-state"
          @click="handleExecute"
        >
          <LuRefreshCw v-if="executing" class="h-3.5 w-3.5 animate-spin" />
          <LuCheck v-else class="h-3.5 w-3.5" />
          <span>执行整理归档 ({{ actionableCount }})</span>
        </button>
      </div>
    </template>
  </MdDialog>
</template>
