<script setup lang="ts">
import { computed, ref } from 'vue'
import {
  LuAlertCircle,
  LuBan,
  LuCheckCircle2,
  LuChevronDown,
  LuChevronUp,
  LuClock,
  LuColumns3,
  LuFileSignature,
  LuFolderSearch,
  LuLoader2,
  LuRotateCcw,
  LuSparkles,
  LuTrash2,
} from 'vue-icons-plus/lu'
import type { MediaKind } from '@/types/api'
import { useToolText } from '../i18n'
import { useToolTaskStore, type ScrapeSubItem, type ToolTaskEntry } from '../tasks'

type TaskStatusFilter = 'all' | 'active' | 'completed' | 'failed'

export interface ReviewItemsPayload {
  domain: MediaKind
  rootId: string
  relPaths: string[]
}

const emit = defineEmits<{
  reviewItems: [payload: ReviewItemsPayload]
}>()

const txt = useToolText()
const taskStore = useToolTaskStore()
const statusFilter = ref<TaskStatusFilter>('all')
const expandedTaskIds = ref<Set<string>>(new Set())

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
  if (next.has(id)) {
    next.delete(id)
  } else {
    next.add(id)
  }
  expandedTaskIds.value = next
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
    (it) => it.status === 'unmatched' || it.status === 'failed',
  )
  if (targets.length === 0) return
  const first = targets[0]!
  emit('reviewItems', {
    domain: first.mediaKind,
    rootId: first.rootId,
    relPaths: targets.filter((t) => t.rootId === first.rootId).map((t) => t.relPath),
  })
}

function reviewSingleSubItem(sub: ScrapeSubItem): void {
  emit('reviewItems', {
    domain: sub.mediaKind,
    rootId: sub.rootId,
    relPaths: [sub.relPath],
  })
}

function formatTime(ms: number): string {
  return new Date(ms).toLocaleTimeString([], {
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
  })
}
</script>

<template>
  <div class="mx-auto max-w-5xl space-y-5 p-4 sm:p-6">
    <!-- Header -->
    <div class="flex flex-wrap items-center justify-between gap-3">
      <div>
        <h1 class="text-xl font-bold tracking-tight text-[var(--ctp-text)]">{{ txt.tasks.title }}</h1>
        <p class="mt-0.5 text-xs text-[var(--ctp-subtext0)]">
          {{ txt.tasks.subtitle }}
        </p>
      </div>

      <div class="flex items-center gap-2">
        <button
          v-if="hasCompleted"
          type="button"
          class="inline-flex h-9 items-center gap-1.5 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-shell)] px-3.5 text-xs font-semibold text-[var(--ctp-subtext1)] transition hover:border-[var(--ctp-accent)] hover:text-[var(--ctp-text)]"
          @click="taskStore.clearCompleted()"
        >
          <LuTrash2 class="h-3.5 w-3.5" />
          <span>{{ txt.tasks.clearCompleted }}</span>
        </button>
      </div>
    </div>

    <!-- Filter Tabs -->
    <div class="inline-flex flex-wrap gap-1 rounded-2xl bg-[var(--ctp-shell)] p-1">
      <button
        v-for="tab in [
          { id: 'all' as const, label: `全部 (${taskStore.persistentTasks.length})` },
          { id: 'active' as const, label: `进行中 (${taskStore.activeCount})` },
          { id: 'completed' as const, label: '已完成' },
          { id: 'failed' as const, label: '未命中 / 异常' },
        ]"
        :key="tab.id"
        type="button"
        class="rounded-xl px-3.5 py-1.5 text-xs font-semibold transition"
        :class="
          statusFilter === tab.id
            ? 'bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)]'
            : 'text-[var(--ctp-subtext1)] hover:text-[var(--ctp-text)]'
        "
        @click="statusFilter = tab.id"
      >
        {{ tab.label }}
      </button>
    </div>

    <!-- Empty State -->
    <div
      v-if="filteredTasks.length === 0"
      class="rounded-3xl border border-dashed border-[var(--ctp-surface1)] bg-[var(--ctp-shell)]/55 py-16 text-center"
    >
      <LuClock class="mx-auto mb-3 h-7 w-7 text-[var(--ctp-subtext0)]" />
      <p class="text-sm font-semibold text-[var(--ctp-text)]">{{ txt.tasks.emptyTitle }}</p>
      <p class="mt-1 text-xs text-[var(--ctp-subtext0)]">
        {{ txt.tasks.emptyDesc }}
      </p>
    </div>

    <!-- Task Cards -->
    <div v-else class="space-y-3">
      <div
        v-for="task in filteredTasks"
        :key="task.id"
        class="card-soft overflow-hidden rounded-2xl bg-[var(--ctp-shell)]"
      >
        <div class="space-y-3 p-4 sm:p-5">
          <div class="flex flex-wrap items-start justify-between gap-3">
            <div class="flex min-w-0 items-start gap-3">
              <div
                class="mt-0.5 flex h-9 w-9 shrink-0 items-center justify-center rounded-xl"
                :class="
                  task.status === 'running' || task.status === 'queued'
                    ? 'bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]'
                    : task.status === 'failed'
                      ? 'bg-red-500/15 text-red-500'
                      : 'bg-emerald-500/15 text-emerald-500'
                "
              >
                <LuLoader2
                  v-if="task.status === 'running'"
                  class="h-4 w-4 animate-spin"
                />
                <LuClock v-else-if="task.status === 'queued'" class="h-4 w-4" />
                <LuSparkles v-else-if="task.kind === 'scrape'" class="h-4 w-4" />
                <LuFileSignature v-else-if="task.kind === 'rename'" class="h-4 w-4" />
                <LuFolderSearch v-else class="h-4 w-4" />
              </div>

              <div class="min-w-0">
                <div class="flex flex-wrap items-center gap-2">
                  <h3 class="text-sm font-bold text-[var(--ctp-text)]">{{ task.title }}</h3>
                  <span
                    class="rounded-full px-2 py-0.5 text-[10px] font-semibold"
                    :class="
                      task.status === 'running'
                        ? 'bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]'
                        : task.status === 'queued'
                          ? 'bg-[var(--ctp-surface0)] text-[var(--ctp-subtext1)]'
                          : task.status === 'failed'
                            ? 'bg-red-500/15 text-red-400'
                            : 'bg-emerald-500/15 text-emerald-500'
                    "
                  >
                    {{
                      task.status === 'running'
                        ? '执行中'
                        : task.status === 'queued'
                          ? '排队中'
                          : task.status === 'failed'
                            ? '失败'
                            : '已完成'
                    }}
                  </span>
                  <span
                    v-if="task.strategyLabel"
                    class="rounded-full border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-2 py-0.5 text-[10px] font-medium text-[var(--ctp-subtext1)]"
                  >
                    {{ task.strategyLabel }}
                  </span>
                  <span class="font-mono text-[11px] text-[var(--ctp-subtext0)]">
                    {{ formatTime(task.startedAt) }}
                  </span>
                </div>

                <p class="mt-1 text-xs text-[var(--ctp-subtext0)]">
                  <template v-if="task.status === 'running' && task.total">
                    进度 {{ task.processed.toLocaleString() }} / {{ task.total.toLocaleString() }} ·
                    {{ task.detail }}
                  </template>
                  <template v-else>
                    {{ task.detail }}
                  </template>
                </p>
              </div>
            </div>

            <!-- Task Actions -->
            <div class="flex shrink-0 items-center gap-1.5">
              <span
                v-if="progressPct(task) !== null && (task.status === 'running' || task.status === 'queued')"
                class="mr-1 font-mono text-xs font-bold text-[var(--ctp-accent)]"
              >
                {{ progressPct(task) }}%
              </span>

              <button
                v-if="task.cancellable && (task.status === 'running' || task.status === 'queued')"
                type="button"
                class="inline-flex h-8 items-center gap-1 rounded-xl border border-amber-500/40 bg-amber-500/10 px-2.5 text-xs font-semibold text-amber-500 transition hover:bg-amber-500/20"
                @click="taskStore.cancelTask(task.id)"
              >
                <LuBan class="h-3.5 w-3.5" />
                <span>{{ txt.tasks.cancel }}</span>
              </button>

              <button
                v-if="hasRetryableSubItems(task)"
                type="button"
                class="inline-flex h-8 items-center gap-1 rounded-xl bg-[var(--ctp-accent-soft)] px-2.5 text-xs font-semibold text-[var(--ctp-accent)] transition hover:bg-[var(--ctp-accent)] hover:text-[var(--ctp-on-accent)]"
                title="一键聚焦未命中/失败项并在右侧展开在线候选对照精修"
                @click="reviewTaskUnmatched(task)"
              >
                <LuColumns3 class="h-3.5 w-3.5" />
                <span>聚焦精修未命中项</span>
              </button>

              <button
                v-if="hasRetryableSubItems(task)"
                type="button"
                class="inline-flex h-8 items-center gap-1 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-2.5 text-xs font-semibold text-[var(--ctp-text)] transition hover:border-[var(--ctp-accent)]"
                @click="taskStore.retryTaskFailedOrUnmatched(task.id)"
              >
                <LuRotateCcw class="h-3.5 w-3.5 text-[var(--ctp-accent)]" />
                <span>{{ txt.tasks.retryUnfinished }}</span>
              </button>

              <button
                v-if="task.subItems && task.subItems.length > 0"
                type="button"
                class="inline-flex h-8 items-center gap-1 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-2.5 text-xs font-medium text-[var(--ctp-subtext1)] transition hover:text-[var(--ctp-text)]"
                @click="toggleExpand(task.id)"
              >
                <span>{{ txt.tasks.details }} ({{ task.subItems.length }})</span>
                <LuChevronUp v-if="expandedTaskIds.has(task.id)" class="h-3.5 w-3.5" />
                <LuChevronDown v-else class="h-3.5 w-3.5" />
              </button>

              <button
                v-if="task.status !== 'running' && task.status !== 'queued'"
                type="button"
                class="flex h-8 w-8 items-center justify-center rounded-xl text-[var(--ctp-subtext0)] transition hover:bg-red-500/10 hover:text-red-500"
                :title="txt.tasks.removeRecord"
                @click="taskStore.removeTask(task.id)"
              >
                <LuTrash2 class="h-3.5 w-3.5" />
              </button>
            </div>
          </div>

          <!-- Progress Bar -->
          <div class="h-1.5 w-full overflow-hidden rounded-full bg-[var(--ctp-surface0)]">
            <div
              v-if="progressPct(task) !== null"
              class="h-full rounded-full transition-all duration-200"
              :class="
                task.status === 'failed'
                  ? 'bg-red-500'
                  : task.status === 'completed'
                    ? 'bg-emerald-500'
                    : 'bg-[var(--ctp-accent)]'
              "
              :style="{ width: `${progressPct(task)}%` }"
            />
          </div>
        </div>

        <!-- Expandable Sub-Items Table -->
        <div
          v-if="task.subItems && expandedTaskIds.has(task.id)"
          class="max-h-72 divide-y divide-[var(--ctp-surface0)]/60 overflow-y-auto border-t border-[var(--ctp-surface0)]/80 bg-[var(--ctp-canvas)]/70"
        >
          <div
            v-for="sub in task.subItems"
            :key="`${sub.rootId}:${sub.relPath}`"
            class="flex items-center justify-between gap-3 px-5 py-2 text-xs"
          >
            <div class="min-w-0 flex-1">
              <p class="truncate font-medium text-[var(--ctp-text)]">{{ sub.fileName }}</p>
              <p class="truncate font-mono text-[10px] text-[var(--ctp-subtext0)]">
                {{ sub.relPath }}
              </p>
            </div>

            <div class="flex shrink-0 items-center gap-2">
              <span
                v-if="typeof sub.confidence === 'number'"
                class="rounded-md bg-[var(--ctp-surface0)]/70 px-1.5 py-0.5 font-mono text-[10px] text-[var(--ctp-subtext1)]"
                title="自动匹配置信度"
              >
                {{ sub.confidence }}%
              </span>
              <span
                v-if="sub.status === 'matched'"
                class="inline-flex items-center gap-1 text-[11px] font-semibold text-emerald-500"
              >
                <LuCheckCircle2 class="h-3.5 w-3.5" />
                <span>{{ txt.tasks.statusMatched }}</span>
              </span>
              <span
                v-else-if="sub.status === 'unmatched'"
                class="inline-flex items-center gap-1 text-[11px] font-medium text-amber-500"
              >
                <LuAlertCircle class="h-3.5 w-3.5" />
                <span>{{ txt.tasks.statusUnmatched }}</span>
              </span>
              <span
                v-else-if="sub.status === 'running'"
                class="inline-flex items-center gap-1 text-[11px] font-semibold text-[var(--ctp-accent)]"
              >
                <LuLoader2 class="h-3.5 w-3.5 animate-spin" />
                <span>{{ txt.tasks.statusRunning }}</span>
              </span>
              <span
                v-else-if="sub.status === 'failed'"
                class="inline-flex max-w-[14rem] items-center gap-1 truncate text-[11px] font-medium text-red-400"
                :title="sub.error"
              >
                <LuAlertCircle class="h-3.5 w-3.5 shrink-0" />
                <span class="truncate">{{ sub.error || txt.tasks.statusFailed }}</span>
              </span>
              <span
                v-else-if="sub.status === 'cancelled'"
                class="text-[11px] text-[var(--ctp-subtext0)]"
              >
                {{ txt.tasks.statusCancelled }}
              </span>
              <span v-else class="text-[11px] text-[var(--ctp-subtext0)]">{{ txt.tasks.statusQueued }}</span>
              <button
                v-if="sub.status === 'unmatched' || sub.status === 'failed' || sub.status === 'matched'"
                type="button"
                class="rounded-lg border border-[var(--ctp-surface0)] bg-[var(--ctp-shell)] px-2 py-0.5 text-[10px] font-medium text-[var(--ctp-subtext1)] transition hover:border-[var(--ctp-accent)] hover:text-[var(--ctp-accent)]"
                @click="reviewSingleSubItem(sub)"
              >
                精修
              </button>
            </div>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>
