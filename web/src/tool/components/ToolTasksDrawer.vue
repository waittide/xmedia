<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  LuAlertCircle,
  LuBan,
  LuBookOpen,
  LuCheckCircle2,
  LuChevronDown,
  LuChevronUp,
  LuClock,
  LuColumns3,
  LuFileSignature,
  LuFolderSearch,
  LuLoader2,
  LuPencil,
  LuRotateCcw,
  LuSlidersHorizontal,
  LuSparkles,
  LuTrash2,
} from 'vue-icons-plus/lu'
import MdLinearProgress from '@/components/ui/MdLinearProgress.vue'
import MdSegmentedButton from '@/components/ui/MdSegmentedButton.vue'
import MdSheet from '@/components/ui/MdSheet.vue'
import type { MediaKind } from '@/types/api'
import { useToolTaskStore, type ScrapeSubItem, type ToolTaskEntry } from '../tasks'

type TaskStatusFilter = 'all' | 'active' | 'completed' | 'failed'
type SubItemFilter = 'all' | 'running' | 'matched' | 'unmatched' | 'failed'

export interface ReviewItemRef {
  rootId: string
  relPath: string
}

export interface ReviewItemsPayload {
  domain: MediaKind
  items: ReviewItemRef[]
  openCandidates?: boolean
}

export interface InspectTaskItemPayload {
  domain: MediaKind
  rootId: string
  relPath: string
  openCandidates?: boolean
}

const props = defineProps<{
  open: boolean
}>()

const emit = defineEmits<{
  close: []
  reviewItems: [payload: ReviewItemsPayload]
  inspectItem: [payload: InspectTaskItemPayload]
  openBookEditor: [payload: InspectTaskItemPayload]
}>()

const { t } = useI18n()
const taskStore = useToolTaskStore()
const statusFilter = ref<TaskStatusFilter>('all')
const expandedTaskIds = ref<Set<string>>(new Set())
const manuallyCollapsedIds = ref<Set<string>>(new Set())
const subItemFilters = ref<Record<string, SubItemFilter>>({})
const nowMs = ref(Date.now())

let tickTimer: ReturnType<typeof setInterval> | null = null

function stopTickTimer(): void {
  if (tickTimer) {
    clearInterval(tickTimer)
    tickTimer = null
  }
}

watch(
  () => props.open && taskStore.activeCount > 0,
  (shouldTick) => {
    if (shouldTick) {
      nowMs.value = Date.now()
      if (!tickTimer) {
        tickTimer = setInterval(() => {
          nowMs.value = Date.now()
        }, 1000)
      }
    } else {
      stopTickTimer()
    }
  },
  { immediate: true },
)

onBeforeUnmount(stopTickTimer)

// Auto-expand active or newly created scrape tasks unless explicitly collapsed by user
watch(
  () => taskStore.persistentTasks.map((t) => `${t.id}:${t.status}`),
  () => {
    const next = new Set(expandedTaskIds.value)
    let changed = false
    for (const task of taskStore.persistentTasks) {
      if (
        task.subItems &&
        task.subItems.length > 0 &&
        (task.status === 'running' || task.status === 'queued') &&
        !manuallyCollapsedIds.value.has(task.id) &&
        !next.has(task.id)
      ) {
        next.add(task.id)
        changed = true
      }
    }
    if (changed) expandedTaskIds.value = next
  },
  { immediate: true },
)

const filteredTasks = computed<ToolTaskEntry[]>(() => {
  const list = taskStore.persistentTasks
  if (statusFilter.value === 'active') {
    return list.filter((t) => t.status === 'running' || t.status === 'queued')
  }
  if (statusFilter.value === 'completed') {
    return list.filter((t) => t.status === 'completed')
  }
  if (statusFilter.value === 'failed') {
    return list.filter(
      (t) =>
        t.status === 'failed' ||
        (t.failedCount ?? 0) > 0 ||
        (t.unmatchedCount ?? 0) > 0,
    )
  }
  return list
})

const hasCompleted = computed(() =>
  taskStore.persistentTasks.some((t) => t.status === 'completed'),
)

function toggleExpand(id: string): void {
  const next = new Set(expandedTaskIds.value)
  const manual = new Set(manuallyCollapsedIds.value)
  if (next.has(id)) {
    next.delete(id)
    manual.add(id)
  } else {
    next.add(id)
    manual.delete(id)
  }
  expandedTaskIds.value = next
  manuallyCollapsedIds.value = manual
}

function getSubFilter(taskId: string): SubItemFilter {
  return subItemFilters.value[taskId] ?? 'all'
}

function setSubFilter(taskId: string, filter: SubItemFilter): void {
  subItemFilters.value = {
    ...subItemFilters.value,
    [taskId]: filter,
  }
}

interface TaskSubCounts {
  total: number
  running: number
  queued: number
  matched: number
  unmatched: number
  failed: number
}

interface TaskSubView {
  counts: TaskSubCounts
  runningItems: ScrapeSubItem[]
  filteredItems: ScrapeSubItem[]
  filterPills: Array<{ id: SubItemFilter; label: string }>
}

const EMPTY_SUB_VIEW: TaskSubView = {
  counts: { total: 0, running: 0, queued: 0, matched: 0, unmatched: 0, failed: 0 },
  runningItems: [],
  filteredItems: [],
  filterPills: [],
}

const taskSubViewMap = computed<Map<string, TaskSubView>>(() => {
  const map = new Map<string, TaskSubView>()
  for (const task of filteredTasks.value) {
    const list = task.subItems
    if (!list || list.length === 0) continue
    let running = 0
    let queued = 0
    let matched = 0
    let unmatched = 0
    let failed = 0
    const runningItems: ScrapeSubItem[] = []
    for (const it of list) {
      if (it.status === 'running') {
        running += 1
        runningItems.push(it)
      } else if (it.status === 'queued') {
        queued += 1
      } else if (it.status === 'matched') {
        matched += 1
      } else if (it.status === 'unmatched') {
        unmatched += 1
      } else if (it.status === 'failed' || it.status === 'cancelled') {
        failed += 1
      }
    }
    const counts: TaskSubCounts = {
      total: list.length,
      running,
      queued,
      matched,
      unmatched,
      failed,
    }
    const f = subItemFilters.value[task.id] ?? 'all'
    let filteredItems: ScrapeSubItem[]
    if (f === 'all') {
      filteredItems = list
    } else if (f === 'running') {
      filteredItems = list.filter((it) => it.status === 'running' || it.status === 'queued')
    } else if (f === 'matched') {
      filteredItems = list.filter((it) => it.status === 'matched')
    } else if (f === 'unmatched') {
      filteredItems = list.filter((it) => it.status === 'unmatched')
    } else {
      filteredItems = list.filter((it) => it.status === 'failed' || it.status === 'cancelled')
    }
    const filterPills: Array<{ id: SubItemFilter; label: string }> = [
      { id: 'all', label: `${t('tool.tasks.filterAll')} (${counts.total})` },
    ]
    if (running + queued > 0) {
      filterPills.push({ id: 'running', label: `${t('tool.tasks.filterActive')} (${running + queued})` })
    }
    filterPills.push({ id: 'matched', label: `${t('tool.tasks.statusMatched')} (${matched})` })
    filterPills.push({ id: 'unmatched', label: `${t('tool.tasks.statusUnmatched')} (${unmatched})` })
    if (failed > 0) {
      filterPills.push({ id: 'failed', label: `${t('tool.tasks.statusFailed')} (${failed})` })
    }
    map.set(task.id, { counts, runningItems, filteredItems, filterPills })
  }
  return map
})

function getTaskSubView(taskId: string): TaskSubView {
  return taskSubViewMap.value.get(taskId) ?? EMPTY_SUB_VIEW
}

function progressPct(task: ToolTaskEntry): number | null {
  if (task.status === 'completed') return 100
  if (task.total !== null && task.total > 0) {
    return Math.min(100, Math.round((task.processed / task.total) * 100))
  }
  return null
}

function hasRetryableSubItems(task: ToolTaskEntry): boolean {
  if (task.status === 'running' || task.status === 'queued') return false
  return Boolean(
    task.subItems?.some(
      (it) =>
        it.status === 'failed' || it.status === 'unmatched' || it.status === 'cancelled',
    ),
  )
}

function reviewTaskUnmatched(task: ToolTaskEntry): void {
  const targets = (task.subItems ?? []).filter(
    (it) => it.status === 'unmatched' || it.status === 'failed' || it.status === 'cancelled',
  )
  if (targets.length === 0) return
  const first: ScrapeSubItem = targets[0]!
  emit('close')
  emit('reviewItems', {
    domain: task.domain ?? first.mediaKind,
    items: targets.map((t) => ({ rootId: t.rootId, relPath: t.relPath })),
    openCandidates: true,
  })
}

function reviewTaskAllResults(task: ToolTaskEntry): void {
  const targets = (task.subItems ?? []).filter((it) => it.status !== 'queued')
  if (targets.length === 0) return
  const first: ScrapeSubItem = targets[0]!
  emit('close')
  emit('reviewItems', {
    domain: task.domain ?? first.mediaKind,
    items: targets.map((t) => ({ rootId: t.rootId, relPath: t.relPath })),
    openCandidates: false,
  })
}

function inspectSubItem(sub: ScrapeSubItem): void {
  emit('close')
  emit('inspectItem', {
    domain: sub.mediaKind,
    rootId: sub.rootId,
    relPath: sub.relPath,
    openCandidates: sub.status === 'unmatched' || sub.status === 'failed',
  })
}

function openSubItemBookEditor(sub: ScrapeSubItem): void {
  emit('close')
  emit('openBookEditor', {
    domain: sub.mediaKind,
    rootId: sub.rootId,
    relPath: sub.relPath,
  })
}

function formatTime(ms: number): string {
  return new Date(ms).toLocaleTimeString([], {
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
  })
}

function formatElapsed(task: ToolTaskEntry): string {
  const end = task.finishedAt ?? nowMs.value
  const secs = Math.max(0, Math.floor((end - task.startedAt) / 1000))
  if (secs < 60) return `${secs}s`
  const m = Math.floor(secs / 60)
  const s = secs % 60
  return `${m}m ${s}s`
}
</script>

<template>
  <MdSheet
    :open="open"
    :title="t('tool.tasks.title')"
    :subtitle="t('tool.tasks.subtitle')"
    size="lg"
    @close="emit('close')"
  >
    <template #subheader>
      <div class="flex flex-wrap items-center justify-between gap-2">
        <div class="no-scrollbar -m-1 max-w-full overflow-x-auto p-1">
          <MdSegmentedButton
            v-model="statusFilter"
            :options="[
              { value: 'all', label: `${t('tool.tasks.filterAll')} (${taskStore.persistentTasks.length})` },
              { value: 'active', label: `${t('tool.tasks.filterActive')} (${taskStore.activeCount})` },
              { value: 'completed', label: t('tool.tasks.filterCompleted') },
              { value: 'failed', label: t('tool.tasks.filterFailed') },
            ]"
            size="sm"
            show-check
          />
        </div>

        <button
          v-if="hasCompleted"
          type="button"
          class="md-btn md-btn-sm md-btn-outlined md-state"
          @click="taskStore.clearCompleted()"
        >
          <LuTrash2 class="h-3.5 w-3.5" />
          <span>{{ t('tool.tasks.clearCompleted') }}</span>
        </button>
      </div>
    </template>

    <div class="space-y-3.5">
      <!-- Empty State -->
      <div
        v-if="filteredTasks.length === 0"
        class="rounded-[24px] bg-[var(--md-sys-color-surface-container)] py-16 text-center"
      >
        <LuClock class="mx-auto mb-3 h-7 w-7 text-[var(--md-sys-color-on-surface-variant)]" />
        <p class="text-sm font-semibold">{{ t('tool.tasks.emptyTitle') }}</p>
        <p class="mt-1 text-xs text-[var(--md-sys-color-on-surface-variant)]">
          {{ t('tool.tasks.emptyDesc') }}
        </p>
      </div>

      <!-- MD3 Tonal Task Cards -->
      <div v-else class="space-y-3.5">
      <div
        v-for="task in filteredTasks"
        :key="task.id"
        class="md-card-filled overflow-hidden"
      >
        <div class="space-y-3.5 p-4 sm:p-5">
          <!-- Top Row: Icon + Title + Badges + Actions -->
          <div class="flex flex-wrap items-start justify-between gap-3">
            <div class="flex min-w-0 items-start gap-3">
              <div
                class="mt-0.5 flex h-10 w-10 shrink-0 items-center justify-center rounded-2xl"
                :class="
                  task.status === 'running' || task.status === 'queued'
                    ? 'bg-[var(--md-sys-color-primary-container)] text-[var(--md-sys-color-on-primary-container)]'
                    : task.status === 'failed'
                      ? 'bg-[var(--md-sys-color-error-container)] text-[var(--md-sys-color-on-error-container)]'
                      : 'bg-[var(--md-sys-color-success-container)] text-[var(--md-sys-color-on-success-container)]'
                "
              >
                <LuLoader2 v-if="task.status === 'running'" class="h-4 w-4 animate-spin" />
                <LuClock v-else-if="task.status === 'queued'" class="h-4 w-4" />
                <LuSparkles v-else-if="task.kind === 'scrape'" class="h-4 w-4" />
                <LuFileSignature v-else-if="task.kind === 'rename'" class="h-4 w-4" />
                <LuFolderSearch v-else class="h-4 w-4" />
              </div>

              <div class="min-w-0">
                <div class="flex flex-wrap items-center gap-2">
                  <h3 class="text-sm font-bold text-[var(--md-sys-color-on-surface)]">
                    {{ task.title }}
                  </h3>
                  <span
                    class="rounded-full px-2.5 py-0.5 text-[10px] font-semibold"
                    :class="
                      task.status === 'running'
                        ? 'bg-[var(--md-sys-color-primary-container)] text-[var(--md-sys-color-on-primary-container)]'
                        : task.status === 'queued'
                          ? 'bg-[var(--md-sys-color-surface-container-highest)] text-[var(--md-sys-color-on-surface-variant)]'
                          : task.status === 'failed'
                            ? 'bg-[var(--md-sys-color-error-container)] text-[var(--md-sys-color-on-error-container)]'
                            : 'bg-[var(--md-sys-color-success-container)] text-[var(--md-sys-color-on-success-container)]'
                    "
                  >
                    {{
                      task.status === 'running'
                        ? t('tool.tasks.statusRunning')
                        : task.status === 'queued'
                          ? t('tool.tasks.statusQueued')
                          : task.status === 'failed'
                            ? t('tool.tasks.statusFailed')
                            : t('tool.tasks.filterCompleted')
                    }}
                  </span>
                  <span
                    v-if="task.strategyLabel"
                    class="rounded-full bg-[var(--md-sys-color-surface-container-highest)] px-2 py-0.5 text-[10px] font-medium text-[var(--md-sys-color-on-surface-variant)]"
                  >
                    {{ task.strategyLabel }}
                  </span>
                  <span
                    v-if="task.concurrency && task.concurrency > 1"
                    class="rounded-full bg-[var(--md-sys-color-surface-container-highest)] px-2 py-0.5 font-mono text-[10px] text-[var(--md-sys-color-on-surface-variant)]"
                  >
                    {{ t('tool.tasks.concurrency', { n: task.concurrency }) }}
                  </span>
                  <span class="font-mono text-[11px] text-[var(--md-sys-color-on-surface-variant)]">
                    {{ formatTime(task.startedAt) }} · {{ t('tool.tasks.elapsed') }} {{ formatElapsed(task) }}
                  </span>
                </div>

                <!-- Structured Live Progress & Quota Counters -->
                <div
                  v-if="task.subItems && task.subItems.length > 0"
                  class="mt-2 flex flex-wrap items-center gap-1.5 text-[11px]"
                >
                  <span
                    class="rounded-full bg-[var(--md-sys-color-surface-container-highest)] px-2.5 py-0.5 font-mono font-semibold text-[var(--md-sys-color-on-surface)]"
                  >
                    {{ task.processed }} / {{ task.total ?? task.subItems.length }}
                    <template v-if="progressPct(task) !== null">({{ progressPct(task) }}%)</template>
                  </span>
                  <span
                    v-if="getTaskSubView(task.id).counts.matched > 0"
                    class="inline-flex items-center gap-1 rounded-full bg-[var(--md-sys-color-success-container)] px-2.5 py-0.5 font-medium text-[var(--md-sys-color-on-success-container)]"
                  >
                    <LuCheckCircle2 class="h-3 w-3" />
                    <span>{{ t('tool.tasks.statusMatched') }} {{ getTaskSubView(task.id).counts.matched }}</span>
                  </span>
                  <span
                    v-if="getTaskSubView(task.id).counts.running > 0"
                    class="inline-flex items-center gap-1 rounded-full bg-[var(--md-sys-color-primary-container)] px-2.5 py-0.5 font-medium text-[var(--md-sys-color-on-primary-container)]"
                  >
                    <LuLoader2 class="h-3 w-3 animate-spin" />
                    <span>{{ t('tool.tasks.filterActive') }} {{ getTaskSubView(task.id).counts.running }}</span>
                  </span>
                  <span
                    v-if="getTaskSubView(task.id).counts.queued > 0"
                    class="rounded-full bg-[var(--md-sys-color-surface-container-highest)] px-2.5 py-0.5 text-[var(--md-sys-color-on-surface-variant)]"
                  >
                    {{ t('tool.tasks.statusQueued') }} {{ getTaskSubView(task.id).counts.queued }}
                  </span>
                  <span
                    v-if="getTaskSubView(task.id).counts.unmatched > 0"
                    class="inline-flex items-center gap-1 rounded-full bg-[var(--md-sys-color-warning-container)] px-2.5 py-0.5 font-medium text-[var(--md-sys-color-on-warning-container)]"
                  >
                    <LuAlertCircle class="h-3 w-3" />
                    <span>{{ t('tool.tasks.statusUnmatched') }} {{ getTaskSubView(task.id).counts.unmatched }}</span>
                  </span>
                  <span
                    v-if="getTaskSubView(task.id).counts.failed > 0"
                    class="inline-flex items-center gap-1 rounded-full bg-[var(--md-sys-color-error-container)] px-2.5 py-0.5 font-medium text-[var(--md-sys-color-on-error-container)]"
                  >
                    <LuAlertCircle class="h-3 w-3" />
                    <span>{{ t('tool.tasks.statusFailed') }} {{ getTaskSubView(task.id).counts.failed }}</span>
                  </span>
                </div>

                <p
                  v-else
                  class="mt-1 text-xs text-[var(--md-sys-color-on-surface-variant)]"
                >
                  <template v-if="task.status === 'running' && task.total">
                    {{ task.processed.toLocaleString() }} / {{ task.total.toLocaleString() }} ·
                    {{ task.detail }}
                  </template>
                  <template v-else>
                    {{ task.detail }}
                  </template>
                </p>

                <!-- Currently Processing Active Lanes Banner -->
                <div
                  v-if="task.status === 'running' && getTaskSubView(task.id).runningItems.length > 0"
                  class="mt-2 flex flex-wrap items-center gap-1.5 text-[11px] text-[var(--md-sys-color-primary)]"
                >
                  <span class="font-semibold">{{ t('tool.tasks.runningNow') }}:</span>
                  <span
                    v-for="rItem in getTaskSubView(task.id).runningItems"
                    :key="rItem.relPath"
                    class="inline-flex max-w-[18rem] items-center gap-1 rounded-full bg-[var(--md-sys-color-primary-container)]/70 px-2.5 py-0.5 text-[var(--md-sys-color-on-primary-container)]"
                  >
                    <LuLoader2 class="h-3 w-3 shrink-0 animate-spin" />
                    <span class="truncate">{{ rItem.fileName }}</span>
                  </span>
                </div>
              </div>
            </div>

            <!-- Task-Level Actions -->
            <div class="flex flex-wrap shrink-0 items-center gap-1.5">
              <button
                v-if="task.cancellable && (task.status === 'running' || task.status === 'queued')"
                type="button"
                class="md-btn md-btn-sm md-state bg-[var(--md-sys-color-warning-container)] text-[var(--md-sys-color-on-warning-container)]"
                @click="taskStore.cancelTask(task.id)"
              >
                <LuBan class="h-3.5 w-3.5" />
                <span>{{ t('tool.tasks.cancel') }}</span>
              </button>

              <!-- Review & Edit ALL completed results (Works even when 100% matched!) -->
              <button
                v-if="task.subItems && task.processed > 0"
                type="button"
                class="md-btn md-btn-sm md-btn-tonal md-state"
                @click="reviewTaskAllResults(task)"
              >
                <LuSlidersHorizontal class="h-3.5 w-3.5" />
                <span>
                  {{ t('tool.tasks.reviewAllResults', { count: task.processed }) }}
                </span>
              </button>

              <button
                v-if="hasRetryableSubItems(task)"
                type="button"
                class="md-btn md-btn-sm md-btn-filled md-state"
                @click="reviewTaskUnmatched(task)"
              >
                <LuColumns3 class="h-3.5 w-3.5" />
                <span>{{ t('tool.tasks.reviewUnmatched') }}</span>
              </button>

              <button
                v-if="hasRetryableSubItems(task)"
                type="button"
                class="md-btn md-btn-sm md-btn-outlined md-state"
                @click="taskStore.retryTaskFailedOrUnmatched(task.id)"
              >
                <LuRotateCcw class="h-3.5 w-3.5" />
                <span>{{ t('tool.tasks.retryUnfinished') }}</span>
              </button>

              <button
                v-if="task.subItems && task.subItems.length > 0"
                type="button"
                class="md-btn md-btn-sm md-btn-outlined md-state"
                @click="toggleExpand(task.id)"
              >
                <span>{{ t('tool.tasks.details') }} ({{ task.subItems.length }})</span>
                <LuChevronUp v-if="expandedTaskIds.has(task.id)" class="h-3.5 w-3.5" />
                <LuChevronDown v-else class="h-3.5 w-3.5" />
              </button>

              <button
                v-if="task.status !== 'running' && task.status !== 'queued'"
                type="button"
                class="md-icon-btn md-icon-btn-sm md-state text-[var(--md-sys-color-on-surface-variant)] hover:text-[var(--md-sys-color-error)]"
                :title="t('tool.tasks.removeRecord')"
                @click="taskStore.removeTask(task.id)"
              >
                <LuTrash2 class="h-3.5 w-3.5" />
              </button>
            </div>
          </div>

          <!-- Progress Bar -->
          <MdLinearProgress
            :value="progressPct(task)"
            :indeterminate="(task.status === 'running' || task.status === 'queued') && progressPct(task) === null"
            :tone="
              task.status === 'failed'
                ? 'error'
                : task.status === 'completed'
                  ? 'success'
                  : 'primary'
            "
            :aria-label="task.title"
          />
        </div>

        <!-- Expandable Sub-Items Panel with Status Filter & Per-Item Edit Controls -->
        <div
          v-if="task.subItems && expandedTaskIds.has(task.id)"
          class="border-t border-[var(--md-sys-color-outline-variant)]/40 bg-[var(--md-sys-color-surface-container-low)]"
        >
          <!-- Sub-item Filter Strip -->
          <div
            class="flex flex-wrap items-center gap-1.5 border-b border-[var(--md-sys-color-outline-variant)]/30 px-4 py-2.5 sm:px-5"
          >
            <button
              v-for="pill in getTaskSubView(task.id).filterPills"
              :key="pill.id"
              type="button"
              class="md-state rounded-full px-2.5 py-1 text-[11px] font-medium transition"
              :class="
                getSubFilter(task.id) === pill.id
                  ? 'bg-[var(--md-sys-color-secondary-container)] font-semibold text-[var(--md-sys-color-on-secondary-container)]'
                  : 'text-[var(--md-sys-color-on-surface-variant)] hover:text-[var(--md-sys-color-on-surface)]'
              "
              @click="setSubFilter(task.id, pill.id)"
            >
              {{ pill.label }}
            </button>
          </div>

          <div
            v-if="getTaskSubView(task.id).filteredItems.length === 0"
            class="py-6 text-center text-xs text-[var(--md-sys-color-on-surface-variant)]"
          >
            {{ t('tool.tasks.noSubItemsInFilter') }}
          </div>

          <div
            v-else
            class="max-h-96 divide-y divide-[var(--md-sys-color-outline-variant)]/30 overflow-y-auto"
          >
            <div
              v-for="sub in getTaskSubView(task.id).filteredItems"
              :key="`${sub.rootId}:${sub.relPath}`"
              class="flex flex-wrap items-center justify-between gap-3 px-4 py-2.5 text-xs transition hover:bg-[var(--md-sys-color-surface-container)] sm:px-5"
            >
              <div class="min-w-0 flex-1">
                <div class="flex flex-wrap items-center gap-2">
                  <p class="truncate font-semibold text-[var(--md-sys-color-on-surface)]">
                    {{ sub.matchedTitle || sub.fileName }}
                  </p>
                  <span
                    v-if="sub.matchedSubtitle"
                    class="truncate text-[11px] text-[var(--md-sys-color-on-surface-variant)]"
                  >
                    · {{ sub.matchedSubtitle }}
                  </span>
                </div>
                <p class="mt-0.5 truncate font-mono text-[10px] text-[var(--md-sys-color-on-surface-variant)]">
                  {{ sub.relPath }}
                </p>
                <!-- Scraped Completeness Mini Badges -->
                <div
                  v-if="sub.status === 'matched' || sub.status === 'unmatched'"
                  class="mt-1 flex flex-wrap items-center gap-1 text-[10px]"
                >
                  <span
                    v-if="sub.hasCover !== undefined"
                    class="rounded-full px-1.5 py-0.25"
                    :class="
                      sub.hasCover
                        ? 'bg-[var(--md-sys-color-success-container)] text-[var(--md-sys-color-on-success-container)]'
                        : 'bg-[var(--md-sys-color-surface-container-highest)] text-[var(--md-sys-color-on-surface-variant)]'
                    "
                  >
                    {{ sub.hasCover ? t('tool.tasks.badgeHasCover') : t('tool.tasks.badgeNoCover') }}
                  </span>
                  <span
                    v-if="sub.hasSummary !== undefined"
                    class="rounded-full px-1.5 py-0.25"
                    :class="
                      sub.hasSummary
                        ? 'bg-[var(--md-sys-color-success-container)] text-[var(--md-sys-color-on-success-container)]'
                        : 'bg-[var(--md-sys-color-surface-container-highest)] text-[var(--md-sys-color-on-surface-variant)]'
                    "
                  >
                    {{ sub.hasSummary ? t('tool.tasks.badgeHasSummary') : t('tool.tasks.badgeNoSummary') }}
                  </span>
                  <span
                    v-if="sub.hasTags !== undefined"
                    class="rounded-full px-1.5 py-0.25"
                    :class="
                      sub.hasTags
                        ? 'bg-[var(--md-sys-color-success-container)] text-[var(--md-sys-color-on-success-container)]'
                        : 'bg-[var(--md-sys-color-surface-container-highest)] text-[var(--md-sys-color-on-surface-variant)]'
                    "
                  >
                    {{ sub.hasTags ? t('tool.tasks.badgeHasTags') : t('tool.tasks.badgeNoTags') }}
                  </span>
                </div>
              </div>

              <div class="flex shrink-0 flex-wrap items-center gap-2">
                <span
                  v-if="typeof sub.confidence === 'number'"
                  class="rounded-full bg-[var(--md-sys-color-surface-container-highest)] px-2 py-0.5 font-mono text-[10px] font-semibold text-[var(--md-sys-color-on-surface-variant)]"
                  :title="t('tool.tasks.confidenceTooltip')"
                >
                  {{ sub.confidence }}%
                </span>

                <span
                  v-if="sub.status === 'matched'"
                  class="inline-flex items-center gap-1 text-[11px] font-semibold text-[var(--md-sys-color-success)]"
                >
                  <LuCheckCircle2 class="h-3.5 w-3.5" />
                  <span>{{ t('tool.tasks.statusMatched') }}</span>
                </span>
                <span
                  v-else-if="sub.status === 'unmatched'"
                  class="inline-flex items-center gap-1 text-[11px] font-medium text-[var(--md-sys-color-warning)]"
                >
                  <LuAlertCircle class="h-3.5 w-3.5" />
                  <span>{{ t('tool.tasks.statusUnmatched') }}</span>
                </span>
                <span
                  v-else-if="sub.status === 'running'"
                  class="inline-flex items-center gap-1 text-[11px] font-semibold text-[var(--md-sys-color-primary)]"
                >
                  <LuLoader2 class="h-3.5 w-3.5 animate-spin" />
                  <span>{{ t('tool.tasks.statusRunning') }}</span>
                </span>
                <span
                  v-else-if="sub.status === 'failed'"
                  class="inline-flex max-w-[14rem] items-center gap-1 truncate text-[11px] font-medium text-[var(--md-sys-color-error)]"
                  :title="sub.error"
                >
                  <LuAlertCircle class="h-3.5 w-3.5 shrink-0" />
                  <span class="truncate">{{ sub.error || t('tool.tasks.statusFailed') }}</span>
                </span>
                <span
                  v-else-if="sub.status === 'cancelled'"
                  class="text-[11px] text-[var(--md-sys-color-on-surface-variant)]"
                >
                  {{ t('tool.tasks.statusCancelled') }}
                </span>
                <span v-else class="text-[11px] text-[var(--md-sys-color-on-surface-variant)]">
                  {{ t('tool.tasks.statusQueued') }}
                </span>

                <!-- Direct Post-Task Edit Controls for Each Item -->
                <div
                  v-if="sub.status !== 'running' && sub.status !== 'queued'"
                  class="ml-1 flex items-center gap-1"
                >
                  <button
                    type="button"
                    class="md-btn md-btn-sm md-btn-tonal md-state !h-7 !px-2.5 !text-[11px]"
                    @click="inspectSubItem(sub)"
                  >
                    <LuPencil class="h-3 w-3" />
                    <span>{{ t('tool.tasks.editMetadata') }}</span>
                  </button>

                  <button
                    v-if="sub.mediaKind === 'book'"
                    type="button"
                    class="md-btn md-btn-sm md-btn-outlined md-state !h-7 !px-2.5 !text-[11px]"
                    @click="openSubItemBookEditor(sub)"
                  >
                    <LuBookOpen class="h-3 w-3" />
                    <span>{{ t('tool.tasks.editBookContent') }}</span>
                  </button>
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>
      </div>
    </div>
  </MdSheet>
</template>
