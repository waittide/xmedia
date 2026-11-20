<script setup lang="ts">
import { computed, watch } from 'vue'
import { useRouter } from 'vue-router'
import { useI18n } from 'vue-i18n'
import {
  LuAlertCircle,
  LuBan,
  LuCheckCircle2,
  LuChevronDown,
  LuChevronUp,
  LuLoader2,
  LuRefreshCw,
  LuRotateCw,
  LuUpload,
  LuX,
} from 'vue-icons-plus/lu'
import { useEventsStore } from '@/stores/events'
import { useUploadStore } from '@/stores/uploads'

withDefaults(
  defineProps<{
    elevatedBottom?: boolean
  }>(),
  {
    elevatedBottom: false,
  },
)

interface ShelfTaskRow {
  id: string
  uploadTaskId?: string
  kind: 'scan' | 'upload'
  title: string
  subtitle: string
  status: 'queued' | 'running' | 'completed' | 'failed' | 'cancelled'
  progress: number | null
  targetRoute?: string
  canCancel: boolean
  canRetry: boolean
}

const { t, locale } = useI18n()
const router = useRouter()
const eventsStore = useEventsStore()
const uploadStore = useUploadStore()

const isZh = computed(() => locale.value.toLowerCase().startsWith('zh'))
const cancelledLabel = computed(() => (isZh.value ? '已取消' : 'Cancelled'))
const retryLabel = computed(() => (isZh.value ? '重试' : 'Retry'))

watch(
  () => eventsStore.anyScanning,
  (scanning, wasScanning) => {
    if (scanning && !wasScanning) {
      uploadStore.collapsed = false
    }
  },
)

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  return `${(bytes / (1024 * 1024)).toFixed(2)} MB`
}

const combinedTasks = computed<ShelfTaskRow[]>(() => {
  const scanItems: ShelfTaskRow[] = eventsStore.scanTaskList.map((sc) => {
    let progress: number | null = null
    if (sc.status === 'completed') {
      progress = 100
    } else if (sc.total !== null && sc.total > 0) {
      progress = Math.min(100, Math.round((sc.scanned / sc.total) * 100))
    }

    let subtitle = ''
    if (sc.status === 'running') {
      if (sc.total !== null && sc.total > 0) {
        subtitle = t('transfers.scanProgressDetail', {
          scanned: sc.scanned.toLocaleString(),
          total: sc.total.toLocaleString(),
        })
      } else if (sc.scanned > 0) {
        subtitle = t('admin.scanning', { scanned: sc.scanned.toLocaleString() })
      } else {
        subtitle = t('transfers.scanCounting')
      }
    } else if (sc.status === 'completed') {
      subtitle = t('admin.scanCompleted', {
        scanned: sc.scanned.toLocaleString(),
        added: sc.added ?? 0,
        updated: sc.updated ?? 0,
        missing: sc.missing ?? 0,
      })
    } else {
      subtitle = sc.error || t('transfers.statusFailed')
    }

    return {
      id: `scan:${sc.library_id}`,
      kind: 'scan',
      title: t('transfers.scanTaskTitle', { name: sc.library_name }),
      subtitle,
      status: sc.status,
      progress,
      targetRoute: `/${sc.media_kind}/libraries`,
      canCancel: false,
      canRetry: false,
    }
  })

  const uploadItems: ShelfTaskRow[] = uploadStore.tasks.map((task) => {
    let subtitle = ''
    if (task.status === 'uploading') {
      subtitle = `${formatSize(task.uploadedBytes)} / ${formatSize(task.totalBytes)}`
    } else if (task.status === 'failed') {
      subtitle = task.error || t('transfers.statusFailed')
    } else if (task.status === 'cancelled') {
      subtitle = `${cancelledLabel.value} · ${formatSize(task.totalBytes)}`
    } else if (task.status === 'completed') {
      subtitle = `${t('transfers.statusCompleted')} · ${formatSize(task.totalBytes)}`
    } else {
      subtitle = t('transfers.statusQueued')
    }

    return {
      id: `upload:${task.id}`,
      uploadTaskId: task.id,
      kind: 'upload',
      title: task.label,
      subtitle,
      status: task.status === 'uploading' ? 'running' : task.status,
      progress:
        task.status === 'completed'
          ? 100
          : task.status === 'uploading' || task.status === 'queued'
            ? task.progress
            : null,
      targetRoute: task.targetRoute,
      canCancel: task.status === 'queued' || task.status === 'uploading',
      canRetry: (task.status === 'failed' || task.status === 'cancelled') && task.canRetry,
    }
  })

  return [...scanItems, ...uploadItems]
})

const activeTasks = computed(() =>
  combinedTasks.value.filter((item) => item.status === 'queued' || item.status === 'running'),
)
const completedCount = computed(
  () => combinedTasks.value.filter((item) => item.status === 'completed').length,
)
const failedCount = computed(
  () => combinedTasks.value.filter((item) => item.status === 'failed').length,
)
const cancelledCount = computed(
  () => combinedTasks.value.filter((item) => item.status === 'cancelled').length,
)

const overallProgress = computed<number | null>(() => {
  if (activeTasks.value.length === 0) return null
  const determinate = activeTasks.value.filter(
    (item): item is ShelfTaskRow & { progress: number } =>
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
  if (completedCount.value === 0 && cancelledCount.value > 0) {
    return `${cancelledLabel.value} (${cancelledCount.value})`
  }
  return t('transfers.completedTasksSummary', { done: completedCount.value })
})

function handleClearCompleted(): void {
  eventsStore.clearCompletedScans()
  uploadStore.clearCompleted()
}

function handleDismissAll(): void {
  eventsStore.dismissFinishedScans()
  uploadStore.dismissAll()
}

function handleSelectTask(item: ShelfTaskRow): void {
  if (item.targetRoute) {
    void router.push(item.targetRoute)
  }
}

function handleCancelUpload(item: ShelfTaskRow): void {
  if (item.uploadTaskId) {
    uploadStore.cancelTask(item.uploadTaskId)
  }
}

function handleRetryUpload(item: ShelfTaskRow): void {
  if (item.uploadTaskId) {
    uploadStore.retryTask(item.uploadTaskId)
  }
}
</script>

<template>
  <section
    v-if="combinedTasks.length > 0"
    class="fixed right-4 z-40 w-[min(24rem,calc(100vw-2rem))] overflow-hidden rounded-[24px] bg-[var(--md-sys-color-surface-container-high)] text-[var(--md-sys-color-on-surface)] shadow-[var(--md-sys-elevation-3)] transition-all"
    :class="elevatedBottom ? 'bottom-24' : 'bottom-5'"
  >
    <!-- Header -->
    <div
      class="md-state flex cursor-pointer items-center justify-between gap-2 px-4 py-3 select-none"
      @click="uploadStore.collapsed = !uploadStore.collapsed"
    >
      <div class="flex min-w-0 items-center gap-2.5">
        <div
          class="flex h-8 w-8 shrink-0 items-center justify-center rounded-full"
          :class="
            activeTasks.length > 0
              ? 'bg-[var(--md-sys-color-primary-container)] text-[var(--md-sys-color-on-primary-container)]'
              : failedCount > 0
                ? 'bg-[var(--md-sys-color-error-container)] text-[var(--md-sys-color-on-error-container)]'
                : completedCount === 0 && cancelledCount > 0
                  ? 'bg-[var(--md-sys-color-surface-container-highest)] text-[var(--md-sys-color-on-surface-variant)]'
                  : 'bg-[var(--md-sys-color-success-container)] text-[var(--md-sys-color-on-success-container)]'
          "
        >
          <LuLoader2 v-if="activeTasks.length > 0" class="h-4 w-4 animate-spin" />
          <LuAlertCircle v-else-if="failedCount > 0" class="h-4 w-4" />
          <LuBan v-else-if="completedCount === 0 && cancelledCount > 0" class="h-4 w-4" />
          <LuCheckCircle2 v-else class="h-4 w-4" />
        </div>
        <p class="truncate text-xs font-semibold text-[var(--md-sys-color-on-surface)]">
          {{ summaryText }}
        </p>
      </div>

      <div class="flex shrink-0 items-center gap-1" @click.stop>
        <button
          v-if="completedCount > 0"
          type="button"
          class="md-state rounded-full px-2.5 py-1 text-[11px] font-medium text-[var(--md-sys-color-on-surface-variant)] hover:text-[var(--md-sys-color-on-surface)]"
          @click="handleClearCompleted"
        >
          {{ t('transfers.clearCompleted') }}
        </button>

        <button
          type="button"
          class="md-icon-btn md-icon-btn-sm md-state !h-7 !w-7"
          @click="uploadStore.collapsed = !uploadStore.collapsed"
        >
          <LuChevronUp v-if="uploadStore.collapsed" class="h-4 w-4" />
          <LuChevronDown v-else class="h-4 w-4" />
        </button>

        <button
          v-if="activeTasks.length === 0"
          type="button"
          class="md-icon-btn md-icon-btn-sm md-state !h-7 !w-7"
          @click="handleDismissAll"
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
        class="upload-indeterminate-bar h-full w-2/5 rounded-full bg-[var(--md-sys-color-primary)]"
      />
    </div>

    <!-- Task Rows -->
    <div
      v-if="!uploadStore.collapsed"
      class="max-h-64 divide-y divide-[var(--md-sys-color-outline-variant)]/40 overflow-y-auto"
    >
      <div
        v-for="task in combinedTasks"
        :key="task.id"
        class="space-y-1.5 px-4 py-2.5 text-xs transition hover:bg-[var(--md-sys-color-surface-container-highest)]/60"
        :class="task.targetRoute ? 'cursor-pointer' : ''"
        @click="task.targetRoute && handleSelectTask(task)"
      >
        <div class="flex items-center justify-between gap-2">
          <div class="flex min-w-0 items-center gap-2">
            <LuRefreshCw
              v-if="task.kind === 'scan'"
              class="h-3.5 w-3.5 shrink-0"
              :class="
                task.status === 'running'
                  ? 'animate-spin text-[var(--md-sys-color-primary)]'
                  : 'text-[var(--md-sys-color-on-surface-variant)]'
              "
            />
            <LuUpload
              v-else
              class="h-3.5 w-3.5 shrink-0 text-[var(--md-sys-color-on-surface-variant)]"
            />
            <p class="truncate font-semibold text-[var(--md-sys-color-on-surface)]">
              {{ task.title }}
            </p>
          </div>

          <div class="flex shrink-0 items-center gap-1" @click.stop>
            <span
              v-if="(task.status === 'running' || task.status === 'queued') && task.progress !== null"
              class="font-mono text-[11px] font-semibold text-[var(--md-sys-color-primary)]"
            >
              {{ task.progress }}%
            </span>

            <LuLoader2
              v-else-if="task.status === 'running' && task.progress === null"
              class="h-3.5 w-3.5 shrink-0 animate-spin text-[var(--md-sys-color-primary)]"
            />

            <button
              v-if="task.canCancel"
              type="button"
              class="md-icon-btn md-icon-btn-sm md-state !h-6 !w-6 text-[var(--md-sys-color-on-surface-variant)] hover:text-[var(--md-sys-color-error)]"
              :title="t('common.cancel')"
              :aria-label="t('common.cancel')"
              @click="handleCancelUpload(task)"
            >
              <LuX class="h-3.5 w-3.5" />
            </button>

            <button
              v-if="task.canRetry"
              type="button"
              class="md-icon-btn md-icon-btn-sm md-state !h-6 !w-6 text-[var(--md-sys-color-primary)]"
              :title="retryLabel"
              :aria-label="retryLabel"
              @click="handleRetryUpload(task)"
            >
              <LuRotateCw class="h-3.5 w-3.5" />
            </button>

            <LuCheckCircle2
              v-if="task.status === 'completed'"
              class="h-4 w-4 shrink-0 text-[var(--md-sys-color-success)]"
            />
            <LuAlertCircle
              v-else-if="task.status === 'failed'"
              class="h-4 w-4 shrink-0 text-[var(--md-sys-color-error)]"
            />
            <LuBan
              v-else-if="task.status === 'cancelled'"
              class="h-3.5 w-3.5 shrink-0 text-[var(--md-sys-color-on-surface-variant)]"
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
            class="upload-indeterminate-bar h-full w-2/5 rounded-full bg-[var(--md-sys-color-primary)]"
          />
        </div>
      </div>
    </div>
  </section>
</template>

<style scoped>
@keyframes upload-task-slide {
  0% {
    transform: translateX(-110%);
  }
  100% {
    transform: translateX(260%);
  }
}

.upload-indeterminate-bar {
  animation: upload-task-slide 1.25s ease-in-out infinite;
}
</style>
