<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  LuAlertCircle,
  LuCheckCircle2,
  LuChevronDown,
  LuChevronUp,
  LuFolderSearch,
  LuFileSignature,
  LuLoader2,
  LuRefreshCw,
  LuSparkles,
  LuUpload,
  LuX,
} from 'vue-icons-plus/lu'

export interface FloatingTaskItem {
  id: string
  kind: 'scan' | 'upload' | 'browse' | 'rename' | 'scrape'
  title: string
  subtitle: string
  status: 'queued' | 'running' | 'completed' | 'failed'
  /** 0..100 when determinate, null when counting/indeterminate */
  progress: number | null
  clickable?: boolean
}

const props = withDefaults(
  defineProps<{
    tasks: FloatingTaskItem[]
    collapsed: boolean
    elevatedBottom?: boolean
    highZIndex?: boolean
  }>(),
  {
    elevatedBottom: false,
    highZIndex: false,
  },
)

const emit = defineEmits<{
  'update:collapsed': [value: boolean]
  clearCompleted: []
  dismissAll: []
  selectTask: [task: FloatingTaskItem]
}>()

const { t } = useI18n()

const activeTasks = computed(() =>
  props.tasks.filter((item) => item.status === 'queued' || item.status === 'running'),
)
const completedCount = computed(
  () => props.tasks.filter((item) => item.status === 'completed').length,
)
const failedCount = computed(() => props.tasks.filter((item) => item.status === 'failed').length)

const overallProgress = computed<number | null>(() => {
  if (activeTasks.value.length === 0) return null
  const determinate = activeTasks.value.filter(
    (item): item is FloatingTaskItem & { progress: number } =>
      typeof item.progress === 'number' && Number.isFinite(item.progress),
  )
  if (determinate.length === 0) return null
  const sum = determinate.reduce((acc, item) => acc + Math.max(0, Math.min(100, item.progress)), 0)
  return Math.round(sum / determinate.length)
})

const summaryText = computed(() => {
  const activeLen = activeTasks.value.length
  if (activeLen > 0) {
    if (activeLen === 1) {
      const only = activeTasks.value[0]!
      if (only.progress !== null) {
        return `${only.title} (${only.progress}%)`
      }
      return only.title
    }
    if (overallProgress.value !== null) {
      return t('transfers.runningSummaryWithPct', {
        count: activeLen,
        pct: overallProgress.value,
      })
    }
    return t('transfers.runningSummary', { count: activeLen })
  }
  if (failedCount.value > 0) {
    return t('transfers.failedSummary', { done: completedCount.value, failed: failedCount.value })
  }
  return t('transfers.completedTasksSummary', { done: completedCount.value })
})
</script>

<template>
  <section
    v-if="tasks.length > 0"
    class="fixed right-4 w-[min(24rem,calc(100vw-2rem))] overflow-hidden rounded-[24px] bg-[var(--md-sys-color-surface-container-high)] text-[var(--md-sys-color-on-surface)] shadow-[var(--md-sys-elevation-3)] transition-all"
    :class="[elevatedBottom ? 'bottom-24' : 'bottom-5', highZIndex ? 'z-[60]' : 'z-40']"
  >
    <!-- Header -->
    <div
      class="md-state flex cursor-pointer items-center justify-between gap-2 px-4 py-3 select-none"
      @click="emit('update:collapsed', !collapsed)"
    >
      <div class="flex min-w-0 items-center gap-2.5">
        <div
          class="flex h-8 w-8 shrink-0 items-center justify-center rounded-full"
          :class="
            activeTasks.length > 0
              ? 'bg-[var(--md-sys-color-primary-container)] text-[var(--md-sys-color-on-primary-container)]'
              : failedCount > 0
                ? 'bg-[var(--md-sys-color-error-container)] text-[var(--md-sys-color-on-error-container)]'
                : 'bg-[var(--md-sys-color-success-container)] text-[var(--md-sys-color-on-success-container)]'
          "
        >
          <LuLoader2 v-if="activeTasks.length > 0" class="h-4 w-4 animate-spin" />
          <LuAlertCircle v-else-if="failedCount > 0" class="h-4 w-4" />
          <LuCheckCircle2 v-else class="h-4 w-4" />
        </div>
        <p class="truncate text-xs font-semibold text-[var(--md-sys-color-on-surface)]">{{ summaryText }}</p>
      </div>

      <div class="flex shrink-0 items-center gap-1" @click.stop>
        <button
          v-if="completedCount > 0"
          type="button"
          class="md-state rounded-full px-2.5 py-1 text-[11px] font-medium text-[var(--md-sys-color-on-surface-variant)] hover:text-[var(--md-sys-color-on-surface)]"
          @click="emit('clearCompleted')"
        >
          {{ t('transfers.clearCompleted') }}
        </button>

        <button
          type="button"
          class="md-icon-btn md-icon-btn-sm md-state"
          @click="emit('update:collapsed', !collapsed)"
        >
          <LuChevronUp v-if="collapsed" class="h-4 w-4" />
          <LuChevronDown v-else class="h-4 w-4" />
        </button>

        <button
          v-if="activeTasks.length === 0"
          type="button"
          class="md-icon-btn md-icon-btn-sm md-state"
          @click="emit('dismissAll')"
        >
          <LuX class="h-4 w-4" />
        </button>
      </div>
    </div>

    <!-- Overall Top Progress Bar -->
    <div
      v-if="activeTasks.length > 0"
      class="relative h-1 w-full overflow-hidden bg-[var(--md-sys-color-surface-container-highest)]"
    >
      <div
        v-if="overallProgress !== null"
        class="h-full bg-[var(--md-sys-color-primary)] transition-all duration-200"
        :style="{ width: `${overallProgress}%` }"
      />
      <div
        v-else
        class="task-indeterminate-bar h-full w-2/5 rounded-full bg-[var(--md-sys-color-primary)]"
      />
    </div>

    <!-- Task Rows -->
    <div
      v-if="!collapsed"
      class="max-h-64 divide-y divide-[var(--md-sys-color-outline-variant)]/50 overflow-y-auto"
    >
      <div
        v-for="task in tasks"
        :key="task.id"
        class="space-y-1.5 px-4 py-2.5 text-xs transition hover:bg-[color-mix(in_srgb,var(--md-sys-color-on-surface)_6%,transparent)]"
        :class="task.clickable ? 'cursor-pointer' : ''"
        @click="task.clickable && emit('selectTask', task)"
      >
        <div class="flex items-center justify-between gap-2.5">
          <div class="flex min-w-0 items-center gap-2">
            <LuRefreshCw
              v-if="task.kind === 'scan'"
              class="h-3.5 w-3.5 shrink-0 text-[var(--md-sys-color-on-surface-variant)]"
              :class="task.status === 'running' ? 'animate-spin text-[var(--md-sys-color-primary)]' : ''"
            />
            <LuUpload
              v-else-if="task.kind === 'upload'"
              class="h-3.5 w-3.5 shrink-0 text-[var(--md-sys-color-on-surface-variant)]"
            />
            <LuFolderSearch
              v-else-if="task.kind === 'browse'"
              class="h-3.5 w-3.5 shrink-0 text-[var(--md-sys-color-on-surface-variant)]"
            />
            <LuFileSignature
              v-else-if="task.kind === 'rename'"
              class="h-3.5 w-3.5 shrink-0 text-[var(--md-sys-color-on-surface-variant)]"
            />
            <LuSparkles
              v-else
              class="h-3.5 w-3.5 shrink-0 text-[var(--md-sys-color-on-surface-variant)]"
            />
            <p class="truncate font-semibold text-[var(--md-sys-color-on-surface)]">{{ task.title }}</p>
          </div>

          <div class="flex shrink-0 items-center gap-1.5">
            <span
              v-if="(task.status === 'running' || task.status === 'queued') && task.progress !== null"
              class="font-mono text-[11px] font-bold text-[var(--md-sys-color-primary)]"
            >
              {{ task.progress }}%
            </span>
            <LuCheckCircle2
              v-if="task.status === 'completed'"
              class="h-4 w-4 shrink-0 text-[var(--md-sys-color-success)]"
            />
            <LuAlertCircle
              v-else-if="task.status === 'failed'"
              class="h-4 w-4 shrink-0 text-[var(--md-sys-color-error)]"
            />
            <LuLoader2
              v-else-if="task.status === 'running' && task.progress === null"
              class="h-3.5 w-3.5 shrink-0 animate-spin text-[var(--md-sys-color-primary)]"
            />
          </div>
        </div>

        <p
          class="truncate text-[11px]"
          :class="
            task.status === 'failed'
              ? 'text-[var(--md-sys-color-error)]'
              : task.status === 'completed'
                ? 'text-[var(--md-sys-color-success)]'
                : 'text-[var(--md-sys-color-on-surface-variant)]'
          "
        >
          {{ task.subtitle }}
        </p>

        <!-- Per-Task Progress Bar -->
        <div
          v-if="task.status === 'running' || task.status === 'queued'"
          class="relative h-1.5 w-full overflow-hidden rounded-full bg-[var(--md-sys-color-surface-container-highest)]"
        >
          <div
            v-if="task.progress !== null"
            class="h-full rounded-full bg-[var(--md-sys-color-primary)] transition-all duration-150"
            :style="{ width: `${task.progress}%` }"
          />
          <div
            v-else
            class="task-indeterminate-bar h-full w-2/5 rounded-full bg-[var(--md-sys-color-primary)]"
          />
        </div>
      </div>
    </div>
  </section>
</template>

<style scoped>
@keyframes task-slide {
  0% {
    transform: translateX(-110%);
  }
  100% {
    transform: translateX(260%);
  }
}

.task-indeterminate-bar {
  animation: task-slide 1.25s ease-in-out infinite;
}
</style>
