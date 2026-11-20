<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, watch } from 'vue'
import {
  LuAlertTriangle,
  LuArrowRight,
  LuCheck,
  LuFileSignature,
  LuRefreshCw,
  LuX,
} from 'vue-icons-plus/lu'
import AppCheckbox from '@/components/ui/AppCheckbox.vue'
import { useActionFeedback } from '@/composables/useActionFeedback'
import type { MediaKind } from '@/types/api'
import { toolApi } from '../api'
import { useToolTaskStore } from '../tasks'
import type {
  MediaFileEntry,
  RenameConflictPolicy,
  RenameHistoryBatchDto,
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
const undoHistory = ref<RenameHistoryBatchDto[]>([])
const showUndoHistory = ref(false)
const undoingBatchId = ref<string | null>(null)
const undoNotice = ref<string | null>(null)

async function loadUndoHistory(): Promise<void> {
  try {
    undoHistory.value = await toolApi.listRenameHistory(props.rootId)
  } catch {
    // ignore history load failure
  }
}

async function handleUndoBatch(batchId: string): Promise<void> {
  undoingBatchId.value = batchId
  errorMsg.value = null
  undoNotice.value = null
  try {
    const rep = await toolApi.undoRenameBatch(batchId)
    undoNotice.value = '已撤销并恢复 ' + rep.restored_count + ' 个文件' + (rep.skipped_conflicts.length > 0 ? '（跳过 ' + rep.skipped_conflicts.length + ' 个冲突项）' : '')
    await loadUndoHistory()
    emit('renamed', rep.restored_count, {})
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
  } finally {
    undoingBatchId.value = null
  }
}

const dominantKind = computed<MediaKind>(() => props.entries[0]?.media_kind ?? 'music')

const tokenChips = computed<string[]>(() => {
  const k = dominantKind.value
  if (k === 'music') {
    return [
      '{album_artist}',
      '{artist}',
      '{album}',
      '{disc}',
      '{track:2}',
      '{title}',
      '{year}',
      '{genre}',
    ]
  }
  if (k === 'video') {
    return [
      '{show}',
      '{season:2}',
      '{episode:2}',
      '{title}',
      '{original_title}',
      '{year}',
      '{jellyfin_id}',
      '{tmdbid}',
    ]
  }
  if (k === 'comic') {
    return [
      '{series}',
      '{series_group}',
      '<Vol.{volume:2}>',
      '<Ch.{number:2}>',
      '{title}',
      '{publisher}',
      '{year}',
    ]
  }
  return [
    '{author}',
    '{series}',
    '{subseries}',
    '<Vol.{volume:2} >',
    '{title}',
    '{publisher}',
    '{year}',
    '{isbn}',
  ]
})

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
      void loadUndoHistory()
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
    const selected = activePlans.value
    const movedMap: Record<string, string> = {}
    for (const p of selected) {
      if (!p.unchanged && !p.conflict) {
        movedMap[p.from_rel_path] = p.to_rel_path
      }
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
  <Teleport to="body">
    <div
      v-if="open"
      class="fixed inset-0 z-50 flex items-center justify-center bg-black/55 p-3 backdrop-blur-xs sm:p-6"
      @click.self="emit('close')"
    >
      <div
        class="flex max-h-[88vh] w-full max-w-4xl flex-col overflow-hidden rounded-3xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] shadow-2xl"
      >
        <!-- Header -->
        <div
          class="flex shrink-0 items-center justify-between gap-3 border-b border-[var(--ctp-surface0)]/80 px-5 py-4"
        >
          <div class="flex items-center gap-2">
            <LuFileSignature class="h-4 w-4 text-[var(--ctp-accent)]" />
            <h2 class="text-sm font-bold text-[var(--ctp-text)]">
              按元信息模板整理归档 ({{ entries.length }} 个文件)
            </h2>
          </div>
          <button
            type="button"
            class="flex h-8 w-8 items-center justify-center rounded-xl text-[var(--ctp-subtext0)] transition hover:bg-[var(--ctp-surface0)]/70 hover:text-[var(--ctp-text)]"
            @click="emit('close')"
          >
            <LuX class="h-4 w-4" />
          </button>
        </div>

        <div
          v-if="(loadingPreview || executing) && inlineProgressPct !== null"
          class="h-1 w-full shrink-0 overflow-hidden bg-[var(--ctp-surface0)]"
        >
          <div
            class="h-full bg-[var(--ctp-accent)] transition-all duration-150"
            :style="{ width: `${inlineProgressPct}%` }"
          />
        </div>

        <!-- Template Editor -->
        <div class="shrink-0 space-y-2.5 border-b border-[var(--ctp-surface0)]/75 bg-[var(--ctp-mantle)]/40 px-5 py-3.5">
          <template v-if="dominantKind === 'video'">
            <div class="grid grid-cols-1 gap-2 sm:grid-cols-12">
              <label class="block sm:col-span-5">
                <span class="mb-1 block text-[11px] font-medium text-[var(--ctp-subtext0)]">
                  电影归档模板（对非剧集生效）
                </span>
                <input
                  ref="movieInputRef"
                  v-model="templateInput"
                  type="text"
                  placeholder="{title} ({year})/{title} ({year})"
                  class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 font-mono text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
                  @focus="focusedVideoInput = 'movie'"
                />
              </label>
              <label class="block sm:col-span-5">
                <span class="mb-1 block text-[11px] font-medium text-[var(--ctp-subtext0)]">
                  剧集归档模板（对剧集 SxxExx 生效）
                </span>
                <input
                  ref="episodeInputRef"
                  v-model="episodeTemplateInput"
                  type="text"
                  placeholder="{show} ({year})/Season {season:2}/{show} - S{season:2}E{episode:2} - {title}"
                  class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 font-mono text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
                  @focus="focusedVideoInput = 'episode'"
                />
              </label>
              <div class="flex items-end sm:col-span-2">
                <button
                  type="button"
                  :disabled="previewSpinning"
                  class="inline-flex w-full items-center justify-center gap-1.5 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs font-medium text-[var(--ctp-text)] transition hover:border-[var(--ctp-accent)] disabled:opacity-70"
                  :class="previewFeedback.done.value ? 'border-emerald-500/45 text-emerald-500' : ''"
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
              class="flex-1 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3.5 py-2 font-mono text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
            />
            <button
              type="button"
              :disabled="previewSpinning"
              class="inline-flex items-center gap-1.5 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs font-medium text-[var(--ctp-text)] transition hover:border-[var(--ctp-accent)] disabled:opacity-70"
              :class="previewFeedback.done.value ? 'border-emerald-500/45 text-emerald-500' : ''"
              @click="previewFeedback.run(runPreview)"
            >
              <LuCheck v-if="previewFeedback.done.value && !previewSpinning" class="h-3.5 w-3.5" />
              <LuRefreshCw v-else class="h-3.5 w-3.5" :class="previewSpinning ? 'animate-spin' : ''" />
              <span>{{ previewFeedback.done.value && !previewSpinning ? '已刷新' : '刷新预览' }}</span>
            </button>
          </div>

          <div class="flex flex-wrap items-center gap-1.5">
            <span class="text-[11px] text-[var(--ctp-subtext0)]">可用占位符：</span>
            <button
              v-for="chip in tokenChips"
              :key="chip"
              type="button"
              class="rounded-lg border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-2 py-0.5 font-mono text-[11px] text-[var(--ctp-accent)] transition hover:border-[var(--ctp-accent)]"
              @click="appendChip(chip)"
            >
              {{ chip }}
            </button>
            <span class="ml-auto text-[11px] text-[var(--ctp-subtext0)]">
              同名外挂文件（.opf / .lrc / .nfo / .srt / .ass）将自动跟随归档
            </span>
          </div>

          <div class="flex flex-wrap items-center gap-2 border-t border-[var(--ctp-surface0)]/60 pt-2">
            <span class="text-[11px] font-medium text-[var(--ctp-subtext0)]">目标冲突处理：</span>
            <div class="inline-flex flex-wrap items-center gap-1">
              <button
                v-for="opt in [
                  { id: 'skip' as const, label: '跳过冲突', tip: '目标路径已存在时跳过，保持原文件不动' },
                  { id: 'auto_suffix' as const, label: '自动追加序号 (2)', tip: '目标已存在时自动追加 (2)/(3) 共存归档' },
                  { id: 'overwrite' as const, label: '覆盖已有文件', tip: '直接覆盖目标位置的已有文件（请谨慎）' },
                ]"
                :key="opt.id"
                type="button"
                :title="opt.tip"
                class="rounded-lg border px-2.5 py-1 text-[11px] font-medium transition"
                :class="
                  conflictPolicy === opt.id
                    ? opt.id === 'overwrite'
                      ? 'border-amber-500/70 bg-amber-500/15 text-amber-400'
                      : 'border-[var(--ctp-accent)] bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]'
                    : 'border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] text-[var(--ctp-subtext0)] hover:border-[var(--ctp-surface1)]'
                "
                @click="conflictPolicy = opt.id"
              >
                {{ opt.label }}
              </button>
            </div>
            <span v-if="conflictCount > 0 && conflictPolicy === 'skip'" class="text-[11px] text-amber-400">
              当前有 {{ conflictCount }} 项冲突，可切换为「自动追加序号」或「覆盖已有文件」
            </span>
            <span v-else-if="overwriteCount > 0" class="text-[11px] text-amber-400">
              注意：将覆盖磁盘上 {{ overwriteCount }} 个已有文件
            </span>
          </div>
        </div>

        <!-- Preview Diff List -->
        <div class="flex-1 space-y-2 overflow-y-auto p-4">
          <div
            v-if="errorMsg"
            class="rounded-xl border border-red-500/35 bg-red-500/10 px-3 py-2 text-xs text-red-400"
          >
            {{ errorMsg }}
          </div>

          <div
            v-if="(skippedOrganizedCount ?? 0) > 0 || unchangedCount > 0"
            class="flex flex-wrap items-center justify-between gap-2 rounded-xl border border-[var(--ctp-surface0)]/80 bg-[var(--ctp-mantle)]/55 px-3 py-2 text-[11px] text-[var(--ctp-subtext1)]"
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
              class="rounded-lg border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-2 py-0.5 text-[11px] font-medium text-[var(--ctp-subtext1)] transition hover:border-[var(--ctp-accent)] hover:text-[var(--ctp-accent)]"
              @click="showUnchanged = !showUnchanged"
            >
              {{ showUnchanged ? '隐藏无变化项' : `查看无变化项 (${unchangedCount})` }}
            </button>
          </div>

          <div
            v-if="!loadingPreview && visiblePlans.length === 0 && plans.length > 0"
            class="rounded-2xl border border-dashed border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)]/25 py-8 text-center text-xs text-[var(--ctp-subtext0)]"
          >
            当前所选条目的路径均已符合模板，无需整理归档。
          </div>

          <div
            v-for="item in visiblePlans"
            :key="item.from_rel_path"
            class="rounded-2xl border border-[var(--ctp-surface0)]/80 bg-[var(--ctp-mantle)]/35 p-3 transition"
            :class="item.unchanged || excludedPaths.has(item.from_rel_path) ? 'opacity-50' : ''"
          >
            <div class="flex flex-wrap items-center gap-2 text-xs">
              <AppCheckbox
                :disabled="item.unchanged"
                :model-value="!item.unchanged && !excludedPaths.has(item.from_rel_path)"
                @change="toggleRowExcluded(item.from_rel_path)"
              />
              <span class="font-mono text-[var(--ctp-subtext1)]">{{ item.from_rel_path }}</span>
              <LuArrowRight class="h-3.5 w-3.5 shrink-0 text-[var(--ctp-subtext0)]" />
              <span
                class="font-mono font-semibold"
                :class="item.unchanged ? 'text-[var(--ctp-subtext0)]' : 'text-[var(--ctp-accent)]'"
              >
                {{ item.to_rel_path }}
              </span>

              <span
                v-if="item.unchanged"
                class="ml-auto rounded-full bg-[var(--ctp-surface0)]/65 px-2 py-0.5 text-[10px] text-[var(--ctp-subtext0)]"
              >
                路径无变化
              </span>
              <span
                v-else-if="item.conflict"
                class="ml-auto inline-flex items-center gap-1 rounded-full bg-red-500/15 px-2.5 py-0.5 text-[10px] font-semibold text-red-400"
              >
                <LuAlertTriangle class="h-3 w-3" />
                {{ item.conflict }}
              </span>
              <span
                v-else-if="item.will_overwrite"
                class="ml-auto inline-flex items-center gap-1 rounded-full bg-amber-500/15 px-2.5 py-0.5 text-[10px] font-semibold text-amber-400"
              >
                <LuAlertTriangle class="h-3 w-3" />
                将覆盖已有文件
              </span>
              <span
                v-else
                class="ml-auto inline-flex items-center gap-1 rounded-full bg-emerald-500/15 px-2.5 py-0.5 text-[10px] font-semibold text-emerald-500"
              >
                <LuCheck class="h-3 w-3" />
                可安全归档
              </span>
            </div>

            <div
              v-if="item.companion_moves.length > 0"
              class="mt-1.5 flex flex-wrap items-center gap-1.5 text-[11px] text-[var(--ctp-subtext0)]"
            >
              <span>联动附属文件：</span>
              <span
                v-for="[cFrom, cTo] in item.companion_moves"
                :key="cFrom"
                class="rounded-md bg-[var(--ctp-canvas)] px-2 py-0.5 font-mono text-[10px]"
              >
                {{ cFrom }} → {{ cTo }}
              </span>
            </div>
          </div>
        </div>

        <!-- Undo History Drawer -->
        <div
          v-if="showUndoHistory"
          class="max-h-44 shrink-0 overflow-y-auto border-t border-[var(--ctp-surface0)]/80 bg-[var(--ctp-canvas)]/85 px-5 py-3 text-xs"
        >
          <div class="mb-2 flex items-center justify-between">
            <span class="font-semibold">最近批量重命名记录（一键回滚原路径）</span>
            <span v-if="undoNotice" class="text-emerald-500">{{ undoNotice }}</span>
          </div>
          <div v-if="undoHistory.length === 0" class="text-[11px] text-[var(--ctp-subtext0)]">
            暂无可撤销的重命名记录。
          </div>
          <div v-else class="space-y-1.5">
            <div
              v-for="batch in undoHistory"
              :key="batch.batch_id"
              class="flex items-center justify-between gap-3 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-panel)] px-3 py-2"
            >
              <div class="min-w-0">
                <div class="font-medium">
                  已移动 {{ batch.moved_count }} 项 · {{ new Date(batch.created_at_unix * 1000).toLocaleString() }}
                </div>
                <div class="truncate text-[10px] text-[var(--ctp-subtext0)]">
                  示例：{{ batch.moves[0]?.from_rel_path }} → {{ batch.moves[0]?.to_rel_path }}
                </div>
              </div>
              <button
                type="button"
                :disabled="undoingBatchId === batch.batch_id"
                class="shrink-0 rounded-lg border border-amber-500/45 bg-amber-500/10 px-2.5 py-1 text-[11px] font-medium text-amber-500 hover:bg-amber-500/20 disabled:opacity-50"
                @click="handleUndoBatch(batch.batch_id)"
              >
                {{ undoingBatchId === batch.batch_id ? '回滚中…' : '一键撤销' }}
              </button>
            </div>
          </div>
        </div>

        <!-- Footer -->
        <div
          class="flex shrink-0 items-center justify-between gap-3 border-t border-[var(--ctp-surface0)]/80 bg-[var(--ctp-mantle)]/35 px-5 py-3.5"
        >
          <div class="flex items-center gap-3">
            <span class="text-xs text-[var(--ctp-subtext0)]">
              共 {{ actionableCount }} 项待归档（冲突项与未变化项会自动跳过）
            </span>
            <button
              v-if="undoHistory.length > 0"
              type="button"
              class="rounded-lg border border-[var(--ctp-surface0)] px-2.5 py-1 text-[11px] font-medium text-[var(--ctp-subtext1)] hover:border-[var(--ctp-accent)]"
              @click="showUndoHistory = !showUndoHistory"
            >
              {{ showUndoHistory ? '收起撤销历史' : '撤销历史 (' + undoHistory.length + ')' }}
            </button>
          </div>
          <div class="flex items-center gap-2">
            <button
              type="button"
              class="rounded-xl px-4 py-2 text-xs font-medium text-[var(--ctp-subtext1)] transition hover:bg-[var(--ctp-surface0)]/65"
              @click="emit('close')"
            >
              取消
            </button>
            <button
              type="button"
              :disabled="actionableCount === 0 || executing"
              class="inline-flex items-center gap-1.5 rounded-xl bg-[var(--ctp-accent)] px-4 py-2 text-xs font-semibold text-[var(--ctp-on-accent)] shadow-sm transition hover:opacity-95 disabled:opacity-50"
              @click="handleExecute"
            >
              <LuRefreshCw v-if="executing" class="h-3.5 w-3.5 animate-spin" />
              <LuCheck v-else class="h-3.5 w-3.5" />
              <span>执行整理归档 ({{ actionableCount }})</span>
            </button>
          </div>
        </div>
      </div>
    </div>
  </Teleport>
</template>
