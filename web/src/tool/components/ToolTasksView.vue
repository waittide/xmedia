<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
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
import MdSegmentedButton from '@/components/ui/MdSegmentedButton.vue'
import type { MediaKind } from '@/types/api'
import { useToolText } from '../i18n'
import { useToolTaskStore, type ScrapeSubItem, type ToolTaskEntry } from '../tasks'

type TaskStatusFilter = 'all' | 'active' | 'completed' | 'failed'
type SubItemFilter = 'all' | 'running' | 'matched' | 'unmatched' | 'failed'

export interface ReviewItemsPayload {
  domain: MediaKind
  rootId: string
  relPaths: string[]
  openCandidates?: boolean
}

export interface InspectTaskItemPayload {
  domain: MediaKind
  rootId: string
  relPath: string
  openCandidates?: boolean
}

const emit = defineEmits<{
  reviewItems: [payload: ReviewItemsPayload]
  inspectItem: [payload: InspectTaskItemPayload]
  openBookEditor: [payload: InspectTaskItemPayload]
}>()

const txt = useToolText()
const taskStore = useToolTaskStore()
const statusFilter = ref<TaskStatusFilter>('all')
const expandedTaskIds = ref<Set<string>>(new Set())
const manuallyCollapsedIds = ref<Set<string>>(new Set())
const subItemFilters = ref<Record<string, SubItemFilter>>({})
const nowMs = ref(Date.now())

let tickTimer: ReturnType<typeof setInterval> | null = null

onMounted(() => {
  tickTimer = setInterval(() => {
    if (taskStore.activeCount > 0) {
      nowMs.value = Date.now()
    }
  }, 1000)
})

onBeforeUnmount(() => {
  if (tickTimer) clearInterval(tickTimer)
})

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

function subCounts(task: ToolTaskEntry) {
  const list = task.subItems ?? []
  let running = 0
  let queued = 0
  let matched = 0
  let unmatched = 0
  let failed = 0
  for (const it of list) {
    if (it.status === 'running') running += 1
    else if (it.status === 'queued') queued += 1
    else if (it.status === 'matched') matched += 1
    else if (it.status === 'unmatched') unmatched += 1
    else if (it.status === 'failed' || it.status === 'cancelled') failed += 1
  }
  return { total: list.length, running, queued, matched, unmatched, failed }
}

function runningSubItems(task: ToolTaskEntry): ScrapeSubItem[] {
  return (task.subItems ?? []).filter((it) => it.status === 'running')
}

function filteredSubItems(task: ToolTaskEntry): ScrapeSubItem[] {
  const list = task.subItems ?? []
  const f = getSubFilter(task.id)
  if (f === 'all') return list
  if (f === 'running') return list.filter((it) => it.status === 'running' || it.status === 'queued')
  if (f === 'matched') return list.filter((it) => it.status === 'matched')
  if (f === 'unmatched') return list.filter((it) => it.status === 'unmatched')
  return list.filter((it) => it.status === 'failed' || it.status === 'cancelled')
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
  emit('reviewItems', {
    domain: task.domain ?? first.mediaKind,
    rootId: first.rootId,
    relPaths: targets.filter((t) => t.rootId === first.rootId).map((t) => t.relPath),
    openCandidates: true,
  })
}

function reviewTaskAllResults(task: ToolTaskEntry): void {
  const targets = (task.subItems ?? []).filter((it) => it.status !== 'queued')
  if (targets.length === 0) return
  const first: ScrapeSubItem = targets[0]!
  emit('reviewItems', {
    domain: task.domain ?? first.mediaKind,
    rootId: first.rootId,
    relPaths: targets.filter((t) => t.rootId === first.rootId).map((t) => t.relPath),
    openCandidates: false,
  })
}

function inspectSubItem(sub: ScrapeSubItem): void {
  emit('inspectItem', {
    domain: sub.mediaKind,
    rootId: sub.rootId,
    relPath: sub.relPath,
    openCandidates: sub.status === 'unmatched' || sub.status === 'failed',
  })
}

function openSubItemBookEditor(sub: ScrapeSubItem): void {
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
  <div class="mx-auto max-w-5xl space-y-5 p-4 sm:p-6">
    <!-- Header -->
    <div class="flex flex-wrap items-center justify-between gap-3">
      <div>
        <h1 class="text-xl font-bold tracking-tight text-[var(--md-sys-color-on-surface)]">
          {{ txt.tasks.title }}
        </h1>
        <p class="mt-0.5 text-xs text-[var(--md-sys-color-on-surface-variant)]">
          {{ txt.tasks.subtitle }}
        </p>
      </div>

      <div class="flex items-center gap-2">
        <button
          v-if="hasCompleted"
          type="button"
          class="md-btn md-btn-sm md-btn-outlined md-state"
          @click="taskStore.clearCompleted()"
        >
          <LuTrash2 class="h-3.5 w-3.5" />
          <span>{{ txt.tasks.clearCompleted }}</span>
        </button>
      </div>
    </div>

    <!-- MD3 Segmented Filter Tabs -->
    <MdSegmentedButton
      v-model="statusFilter"
      :options="[
        { value: 'all', label: `${txt.tasks.filterAll} (${taskStore.persistentTasks.length})` },
        { value: 'active', label: `${txt.tasks.filterActive} (${taskStore.activeCount})` },
        { value: 'completed', label: txt.tasks.filterCompleted },
        { value: 'failed', label: txt.tasks.filterFailed },
      ]"
      size="sm"
      show-check
    />

    <!-- Empty State -->
    <div
      v-if="filteredTasks.length === 0"
      class="rounded-[24px] bg-[var(--md-sys-color-surface-container-low)] py-16 text-center"
    >
      <LuClock class="mx-auto mb-3 h-7 w-7 text-[var(--md-sys-color-on-surface-variant)]" />
      <p class="text-sm font-semibold">{{ txt.tasks.emptyTitle }}</p>
      <p class="mt-1 text-xs text-[var(--md-sys-color-on-surface-variant)]">
        {{ txt.tasks.emptyDesc }}
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
                        ? txt.tasks.statusRunning
                        : task.status === 'queued'
                          ? txt.tasks.statusQueued
                          : task.status === 'failed'
                            ? txt.tasks.statusFailed
                            : txt.tasks.filterCompleted
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
                    {{ txt.tasks.concurrency.replace('{n}', String(task.concurrency)) }}
                  </span>
                  <span class="font-mono text-[11px] text-[var(--md-sys-color-on-surface-variant)]">
                    {{ formatTime(task.startedAt) }} · {{ txt.tasks.elapsed }} {{ formatElapsed(task) }}
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
                    v-if="subCounts(task).matched > 0"
                    class="inline-flex items-center gap-1 rounded-full bg-[var(--md-sys-color-success-container)] px-2.5 py-0.5 font-medium text-[var(--md-sys-color-on-success-container)]"
                  >
                    <LuCheckCircle2 class="h-3 w-3" />
                    <span>{{ txt.tasks.statusMatched }} {{ subCounts(task).matched }}</span>
                  </span>
                  <span
                    v-if="subCounts(task).running > 0"
                    class="inline-flex items-center gap-1 rounded-full bg-[var(--md-sys-color-primary-container)] px-2.5 py-0.5 font-medium text-[var(--md-sys-color-on-primary-container)]"
                  >
                    <LuLoader2 class="h-3 w-3 animate-spin" />
                    <span>{{ txt.tasks.filterActive }} {{ subCounts(task).running }}</span>
                  </span>
                  <span
                    v-if="subCounts(task).queued > 0"
                    class="rounded-full bg-[var(--md-sys-color-surface-container-highest)] px-2.5 py-0.5 text-[var(--md-sys-color-on-surface-variant)]"
                  >
                    {{ txt.tasks.statusQueued }} {{ subCounts(task).queued }}
                  </span>
                  <span
                    v-if="subCounts(task).unmatched > 0"
                    class="inline-flex items-center gap-1 rounded-full bg-[var(--md-sys-color-warning-container)] px-2.5 py-0.5 font-medium text-[var(--md-sys-color-on-warning-container)]"
                  >
                    <LuAlertCircle class="h-3 w-3" />
                    <span>{{ txt.tasks.statusUnmatched }} {{ subCounts(task).unmatched }}</span>
                  </span>
                  <span
                    v-if="subCounts(task).failed > 0"
                    class="inline-flex items-center gap-1 rounded-full bg-[var(--md-sys-color-error-container)] px-2.5 py-0.5 font-medium text-[var(--md-sys-color-on-error-container)]"
                  >
                    <LuAlertCircle class="h-3 w-3" />
                    <span>{{ txt.tasks.statusFailed }} {{ subCounts(task).failed }}</span>
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
                  v-if="task.status === 'running' && runningSubItems(task).length > 0"
                  class="mt-2 flex flex-wrap items-center gap-1.5 text-[11px] text-[var(--md-sys-color-primary)]"
                >
                  <span class="font-semibold">{{ txt.tasks.runningNow }}:</span>
                  <span
                    v-for="rItem in runningSubItems(task)"
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
                <span>{{ txt.tasks.cancel }}</span>
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
                  {{ txt.tasks.reviewAllResults.replace('{count}', String(task.processed)) }}
                </span>
              </button>

              <button
                v-if="hasRetryableSubItems(task)"
                type="button"
                class="md-btn md-btn-sm md-btn-filled md-state"
                @click="reviewTaskUnmatched(task)"
              >
                <LuColumns3 class="h-3.5 w-3.5" />
                <span>{{ txt.tasks.reviewUnmatched }}</span>
              </button>

              <button
                v-if="hasRetryableSubItems(task)"
                type="button"
                class="md-btn md-btn-sm md-btn-outlined md-state"
                @click="taskStore.retryTaskFailedOrUnmatched(task.id)"
              >
                <LuRotateCcw class="h-3.5 w-3.5" />
                <span>{{ txt.tasks.retryUnfinished }}</span>
              </button>

              <button
                v-if="task.subItems && task.subItems.length > 0"
                type="button"
                class="md-btn md-btn-sm md-btn-outlined md-state"
                @click="toggleExpand(task.id)"
              >
                <span>{{ txt.tasks.details }} ({{ task.subItems.length }})</span>
                <LuChevronUp v-if="expandedTaskIds.has(task.id)" class="h-3.5 w-3.5" />
                <LuChevronDown v-else class="h-3.5 w-3.5" />
              </button>

              <button
                v-if="task.status !== 'running' && task.status !== 'queued'"
                type="button"
                class="md-icon-btn md-icon-btn-sm md-state text-[var(--md-sys-color-on-surface-variant)] hover:text-[var(--md-sys-color-error)]"
                :title="txt.tasks.removeRecord"
                @click="taskStore.removeTask(task.id)"
              >
                <LuTrash2 class="h-3.5 w-3.5" />
              </button>
            </div>
          </div>

          <!-- Progress Bar -->
          <div
            class="h-2 w-full overflow-hidden rounded-full bg-[var(--md-sys-color-surface-container-highest)]"
          >
            <div
              v-if="progressPct(task) !== null"
              class="h-full rounded-full transition-all duration-200"
              :class="
                task.status === 'failed'
                  ? 'bg-[var(--md-sys-color-error)]'
                  : task.status === 'completed'
                    ? 'bg-[var(--md-sys-color-success)]'
                    : 'bg-[var(--md-sys-color-primary)]'
              "
              :style="{ width: `${progressPct(task)}%` }"
            />
          </div>
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
              v-for="pill in [
                { id: 'all' as const, label: `${txt.tasks.filterAll} (${subCounts(task).total})` },
                ...(subCounts(task).running + subCounts(task).queued > 0
                  ? [
                      {
                        id: 'running' as const,
                        label: `${txt.tasks.filterActive} (${subCounts(task).running + subCounts(task).queued})`,
                      },
                    ]
                  : []),
                {
                  id: 'matched' as const,
                  label: `${txt.tasks.statusMatched} (${subCounts(task).matched})`,
                },
                {
                  id: 'unmatched' as const,
                  label: `${txt.tasks.statusUnmatched} (${subCounts(task).unmatched})`,
                },
                ...(subCounts(task).failed > 0
                  ? [
                      {
                        id: 'failed' as const,
                        label: `${txt.tasks.statusFailed} (${subCounts(task).failed})`,
                      },
                    ]
                  : []),
              ]"
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
            v-if="filteredSubItems(task).length === 0"
            class="py-6 text-center text-xs text-[var(--md-sys-color-on-surface-variant)]"
          >
            {{ txt.tasks.noSubItemsInFilter }}
          </div>

          <div
            v-else
            class="max-h-96 divide-y divide-[var(--md-sys-color-outline-variant)]/30 overflow-y-auto"
          >
            <div
              v-for="sub in filteredSubItems(task)"
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
                    {{ sub.hasCover ? txt.tasks.badgeHasCover : txt.tasks.badgeNoCover }}
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
                    {{ sub.hasSummary ? txt.tasks.badgeHasSummary : txt.tasks.badgeNoSummary }}
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
                    {{ sub.hasTags ? txt.tasks.badgeHasTags : txt.tasks.badgeNoTags }}
                  </span>
                </div>
              </div>

              <div class="flex shrink-0 flex-wrap items-center gap-2">
                <span
                  v-if="typeof sub.confidence === 'number'"
                  class="rounded-full bg-[var(--md-sys-color-surface-container-highest)] px-2 py-0.5 font-mono text-[10px] font-semibold text-[var(--md-sys-color-on-surface-variant)]"
                  title="自动匹配置信度"
                >
                  {{ sub.confidence }}%
                </span>

                <span
                  v-if="sub.status === 'matched'"
                  class="inline-flex items-center gap-1 text-[11px] font-semibold text-[var(--md-sys-color-success)]"
                >
                  <LuCheckCircle2 class="h-3.5 w-3.5" />
                  <span>{{ txt.tasks.statusMatched }}</span>
                </span>
                <span
                  v-else-if="sub.status === 'unmatched'"
                  class="inline-flex items-center gap-1 text-[11px] font-medium text-[var(--md-sys-color-warning)]"
                >
                  <LuAlertCircle class="h-3.5 w-3.5" />
                  <span>{{ txt.tasks.statusUnmatched }}</span>
                </span>
                <span
                  v-else-if="sub.status === 'running'"
                  class="inline-flex items-center gap-1 text-[11px] font-semibold text-[var(--md-sys-color-primary)]"
                >
                  <LuLoader2 class="h-3.5 w-3.5 animate-spin" />
                  <span>{{ txt.tasks.statusRunning }}</span>
                </span>
                <span
                  v-else-if="sub.status === 'failed'"
                  class="inline-flex max-w-[14rem] items-center gap-1 truncate text-[11px] font-medium text-[var(--md-sys-color-error)]"
                  :title="sub.error"
                >
                  <LuAlertCircle class="h-3.5 w-3.5 shrink-0" />
                  <span class="truncate">{{ sub.error || txt.tasks.statusFailed }}</span>
                </span>
                <span
                  v-else-if="sub.status === 'cancelled'"
                  class="text-[11px] text-[var(--md-sys-color-on-surface-variant)]"
                >
                  {{ txt.tasks.statusCancelled }}
                </span>
                <span v-else class="text-[11px] text-[var(--md-sys-color-on-surface-variant)]">
                  {{ txt.tasks.statusQueued }}
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
                    <span>{{ txt.tasks.editMetadata }}</span>
                  </button>

                  <button
                    v-if="sub.mediaKind === 'book'"
                    type="button"
                    class="md-btn md-btn-sm md-btn-outlined md-state !h-7 !px-2.5 !text-[11px]"
                    @click="openSubItemBookEditor(sub)"
                  >
                    <LuBookOpen class="h-3 w-3" />
                    <span>{{ txt.tasks.editBookContent }}</span>
                  </button>
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>
