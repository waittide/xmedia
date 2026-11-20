<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { useI18n } from 'vue-i18n'
import {
  LuAlertCircle,
  LuArrowUpRight,
  LuBan,
  LuBookOpen,
  LuCheckCircle2,
  LuDownload,
  LuInbox,
  LuLoader2,
  LuRefreshCw,
  LuRotateCw,
  LuTrash2,
  LuUpload,
  LuX,
} from 'vue-icons-plus/lu'
import MdSegmentedButton, { type SegmentedOption } from '@/components/ui/MdSegmentedButton.vue'
import MdSideSheet from '@/components/ui/MdSideSheet.vue'
import { buildOnlineDownloadFileUrl } from '@/domains/book/onlineApi'
import { useAuthStore } from '@/stores/auth'
import { useEventsStore } from '@/stores/events'
import {
  formatDownloadSpeed,
  isOnlineDownloadJobActive,
  useOnlineDownloadsStore,
} from '@/stores/onlineDownloads'
import { useUploadStore } from '@/stores/uploads'
import { formatBytes } from '@/utils/format'

const props = defineProps<{
  open: boolean
}>()

const emit = defineEmits<{
  close: []
}>()

type TaskFilter = 'all' | 'active' | 'completed' | 'failed'
type UnifiedTaskKind = 'scan' | 'upload' | 'download'
type UnifiedTaskStatus = 'queued' | 'running' | 'completed' | 'failed' | 'cancelled'

interface UnifiedTaskItem {
  id: string
  kind: UnifiedTaskKind
  status: UnifiedTaskStatus
  isActive: boolean
  sortTime: number
  title: string
  badge?: string
  metaText: string
  secondaryText?: string
  warningText?: string
  errorText?: string
  progress: number | null
  targetRoute?: string
  uploadTaskId?: string
  downloadJobId?: string
  downloadFileUrl?: string
  downloadFileName?: string
  canCancel: boolean
  canRetry: boolean
  canDelete: boolean
  busy: boolean
}

const { t } = useI18n()
const router = useRouter()
const authStore = useAuthStore()
const eventsStore = useEventsStore()
const uploadStore = useUploadStore()
const onlineDownloadsStore = useOnlineDownloadsStore()

const activeFilter = ref<TaskFilter>('all')
const clearing = ref(false)
const refreshing = ref(false)

// Track per-upload task timestamps locally so uploads sort accurately alongside scans & downloads
const uploadTimestamps = new Map<string, number>()

watch(
  () => uploadStore.tasks.map((task) => `${task.id}:${task.status}`),
  () => {
    const now = Date.now()
    const currentIds = new Set<string>()
    uploadStore.tasks.forEach((task, idx) => {
      currentIds.add(task.id)
      if (!uploadTimestamps.has(task.id)) {
        uploadTimestamps.set(task.id, now - idx * 10)
      }
    })
    for (const id of Array.from(uploadTimestamps.keys())) {
      if (!currentIds.has(id)) {
        uploadTimestamps.delete(id)
      }
    }
  },
  { immediate: true },
)

watch(
  () => props.open,
  (isOpen) => {
    if (isOpen && authStore.isAuthenticated && authStore.hasModule('book')) {
      void onlineDownloadsStore.refresh()
    }
  },
)

function parseIsoTimestamp(iso?: string): number {
  if (!iso) return 0
  const parsed = Date.parse(iso)
  return Number.isFinite(parsed) ? parsed : 0
}

const allTasks = computed<UnifiedTaskItem[]>(() => {
  const items: UnifiedTaskItem[] = []

  // 1. Library scans
  for (const sc of eventsStore.scanTaskList) {
    const isActive = sc.status === 'running'
    let progress: number | null = null
    if (sc.status === 'completed') {
      progress = 100
    } else if (sc.total !== null && sc.total > 0) {
      progress = Math.min(100, Math.round((sc.scanned / sc.total) * 100))
    }

    let metaText = ''
    let errorText: string | undefined
    if (sc.status === 'running') {
      if (sc.total !== null && sc.total > 0) {
        metaText = t('transfers.scanProgressDetail', {
          scanned: sc.scanned.toLocaleString(),
          total: sc.total.toLocaleString(),
        })
      } else if (sc.scanned > 0) {
        metaText = t('admin.scanning', { scanned: sc.scanned.toLocaleString() })
      } else {
        metaText = t('transfers.scanCounting')
      }
    } else if (sc.status === 'completed') {
      metaText = t('admin.scanCompleted', {
        scanned: sc.scanned.toLocaleString(),
        added: sc.added ?? 0,
        updated: sc.updated ?? 0,
        missing: sc.missing ?? 0,
      })
    } else {
      metaText = t('transfers.statusFailed')
      errorText = sc.error || undefined
    }

    items.push({
      id: `scan:${sc.library_id}`,
      kind: 'scan',
      status: sc.status,
      isActive,
      sortTime: sc.updated_at,
      title: sc.library_name,
      badge: t(`app.domains.${sc.media_kind}`),
      metaText,
      errorText,
      progress,
      targetRoute: `/${sc.media_kind}/libraries`,
      canCancel: false,
      canRetry: false,
      canDelete: false,
      busy: false,
    })
  }

  // 2. File / folder uploads
  uploadStore.tasks.forEach((task, idx) => {
    const status: UnifiedTaskStatus = task.status === 'uploading' ? 'running' : task.status
    const isActive = status === 'queued' || status === 'running'
    let metaText = ''
    let errorText: string | undefined

    if (task.status === 'uploading') {
      metaText = `${formatBytes(task.uploadedBytes)} / ${formatBytes(task.totalBytes)}`
    } else if (task.status === 'queued') {
      metaText = `${t('transfers.statusQueued')} · ${formatBytes(task.totalBytes)}`
    } else if (task.status === 'completed') {
      metaText = `${t('transfers.statusCompleted')} · ${formatBytes(task.totalBytes)}`
    } else if (task.status === 'cancelled') {
      metaText = `${t('transfers.statusCancelled')} · ${formatBytes(task.totalBytes)}`
    } else {
      metaText = t('transfers.statusFailed')
      errorText = task.error || undefined
    }

    items.push({
      id: `upload:${task.id}`,
      kind: 'upload',
      status,
      isActive,
      sortTime: uploadTimestamps.get(task.id) ?? Date.now() - idx * 10,
      title: task.label,
      badge: t(`app.domains.${task.mediaKind}`),
      metaText,
      errorText,
      progress:
        task.status === 'completed'
          ? 100
          : isActive
            ? Math.max(0, Math.min(100, task.progress))
            : null,
      targetRoute: task.status === 'completed' ? task.targetRoute : undefined,
      uploadTaskId: task.id,
      canCancel: isActive,
      canRetry: (task.status === 'failed' || task.status === 'cancelled') && task.canRetry,
      canDelete: false,
      busy: false,
    })
  })

  // 3. Online book downloads & packaging
  for (const job of onlineDownloadsStore.jobs) {
    const isActive = isOnlineDownloadJobActive(job.status)
    const status: UnifiedTaskStatus = job.status === 'pending' ? 'queued' : job.status
    const progress =
      job.status === 'completed'
        ? 100
        : job.total_chapters > 0
          ? Math.min(100, Math.round((job.processed_chapters / job.total_chapters) * 100))
          : null

    const chaptersText = t('transfers.chaptersProgress', {
      processed: job.processed_chapters.toLocaleString(),
      total: job.total_chapters > 0 ? job.total_chapters.toLocaleString() : '?',
    })
    const speed = isActive ? formatDownloadSpeed(job.chapters_per_sec) : ''

    let metaText = ''
    if (isActive) {
      metaText = speed ? `${chaptersText} · ${speed}` : chaptersText
    } else if (job.status === 'completed') {
      const sizeText = job.file_size ? ` · ${formatBytes(job.file_size)}` : ''
      metaText = `${t('transfers.statusPacked')} · ${chaptersText}${sizeText}`
    } else if (job.status === 'cancelled') {
      metaText = `${t('transfers.statusCancelled')} · ${chaptersText}`
    } else {
      metaText = `${t('transfers.statusFailed')} · ${chaptersText}`
    }

    items.push({
      id: `download:${job.id}`,
      kind: 'download',
      status,
      isActive,
      sortTime: parseIsoTimestamp(job.updated_at || job.created_at),
      title: job.book_name,
      badge: job.format.toUpperCase(),
      metaText,
      secondaryText: isActive && job.current_chapter_title ? job.current_chapter_title : undefined,
      warningText:
        job.failed_chapters > 0
          ? t('transfers.failedChaptersNote', { count: job.failed_chapters })
          : undefined,
      errorText: job.status === 'failed' ? job.error || undefined : undefined,
      progress,
      targetRoute:
        job.status === 'completed' && job.imported_book_id
          ? `/book/items/${job.imported_book_id}`
          : undefined,
      downloadJobId: job.id,
      downloadFileUrl:
        job.status === 'completed' ? buildOnlineDownloadFileUrl(job.id) : undefined,
      downloadFileName: job.output_file_name || undefined,
      canCancel: isActive,
      canRetry: false,
      canDelete: !isActive,
      busy: onlineDownloadsStore.busyJobId === job.id,
    })
  }

  return items.sort((a, b) => {
    if (a.isActive !== b.isActive) return a.isActive ? -1 : 1
    return b.sortTime - a.sortTime
  })
})

const activeCount = computed(() => allTasks.value.filter((item) => item.isActive).length)
const completedCount = computed(
  () => allTasks.value.filter((item) => item.status === 'completed').length,
)
const failedCount = computed(() => allTasks.value.filter((item) => item.status === 'failed').length)

const filterOptions = computed<SegmentedOption<TaskFilter>[]>(() => [
  {
    value: 'all',
    label:
      allTasks.value.length > 0
        ? `${t('transfers.filterAll')} ${allTasks.value.length}`
        : t('transfers.filterAll'),
  },
  {
    value: 'active',
    label:
      activeCount.value > 0
        ? `${t('transfers.filterActive')} ${activeCount.value}`
        : t('transfers.filterActive'),
  },
  {
    value: 'completed',
    label:
      completedCount.value > 0
        ? `${t('transfers.filterCompleted')} ${completedCount.value}`
        : t('transfers.filterCompleted'),
  },
  {
    value: 'failed',
    label:
      failedCount.value > 0
        ? `${t('transfers.filterFailed')} ${failedCount.value}`
        : t('transfers.filterFailed'),
  },
])

const filteredTasks = computed(() => {
  if (activeFilter.value === 'active') {
    return allTasks.value.filter((item) => item.isActive)
  }
  if (activeFilter.value === 'completed') {
    return allTasks.value.filter((item) => item.status === 'completed')
  }
  if (activeFilter.value === 'failed') {
    return allTasks.value.filter((item) => item.status === 'failed')
  }
  return allTasks.value
})

const canClearFinished = computed(() => {
  const hasFinishedScan = eventsStore.scanTaskList.some((sc) => sc.status !== 'running')
  const hasClearableUpload =
    uploadStore.activeCount === 0
      ? uploadStore.tasks.length > 0
      : uploadStore.completedCount > 0
  const hasFinishedDownload = onlineDownloadsStore.finishedCount > 0
  return hasFinishedScan || hasClearableUpload || hasFinishedDownload
})

function kindLabel(kind: UnifiedTaskKind): string {
  if (kind === 'scan') return t('transfers.typeScan')
  if (kind === 'upload') return t('transfers.typeUpload')
  return t('transfers.typeDownload')
}

function handleNavigate(route?: string): void {
  if (!route) return
  emit('close')
  void router.push(route)
}

function handleCancel(item: UnifiedTaskItem): void {
  if (item.uploadTaskId) {
    uploadStore.cancelTask(item.uploadTaskId)
  } else if (item.downloadJobId) {
    void onlineDownloadsStore.cancelJob(item.downloadJobId)
  }
}

function handleRetry(item: UnifiedTaskItem): void {
  if (item.uploadTaskId) {
    uploadTimestamps.set(item.uploadTaskId, Date.now())
    uploadStore.retryTask(item.uploadTaskId)
  }
}

function handleDelete(item: UnifiedTaskItem): void {
  if (item.downloadJobId) {
    void onlineDownloadsStore.deleteJob(item.downloadJobId)
  }
}

async function handleRefresh(): Promise<void> {
  if (refreshing.value) return
  refreshing.value = true
  try {
    if (authStore.isAuthenticated && authStore.hasModule('book')) {
      await onlineDownloadsStore.refresh()
    }
  } finally {
    refreshing.value = false
  }
}

async function handleClearFinished(): Promise<void> {
  if (clearing.value || !canClearFinished.value) return
  clearing.value = true
  try {
    eventsStore.dismissFinishedScans()
    if (uploadStore.activeCount === 0) {
      uploadStore.dismissAll()
    } else {
      uploadStore.clearCompleted()
    }
    if (onlineDownloadsStore.finishedCount > 0) {
      await onlineDownloadsStore.clearFinished()
    }
  } finally {
    clearing.value = false
  }
}
</script>

<template>
  <MdSideSheet
    :open="open"
    :title="t('transfers.drawerTitle')"
    width-class="w-[min(28rem,100vw)]"
    @close="emit('close')"
  >
    <template #subheader>
      <div class="overflow-x-auto pb-0.5">
        <MdSegmentedButton
          v-model="activeFilter"
          :options="filterOptions"
          size="sm"
          :show-check="false"
        />
      </div>
    </template>

    <!-- Empty State -->
    <div
      v-if="filteredTasks.length === 0"
      class="flex flex-col items-center justify-center py-16 text-center"
    >
      <div
        class="mb-3 flex h-12 w-12 items-center justify-center rounded-full bg-[var(--md-sys-color-surface-container-high)] text-[var(--md-sys-color-on-surface-variant)]"
      >
        <LuInbox class="h-5 w-5" />
      </div>
      <p class="md-body-medium text-[var(--md-sys-color-on-surface-variant)]">
        {{ activeFilter === 'all' ? t('transfers.emptyAll') : t('transfers.emptyFiltered') }}
      </p>
    </div>

    <!-- Unified Task List -->
    <div v-else class="space-y-2.5">
      <article
        v-for="item in filteredTasks"
        :key="item.id"
        class="md-card-outlined p-3.5 transition-colors"
        :class="
          item.targetRoute
            ? 'cursor-pointer hover:bg-[var(--md-sys-color-surface-container)]'
            : ''
        "
        @click="item.targetRoute && handleNavigate(item.targetRoute)"
      >
        <!-- Top Row: Icon + Title + Badges + Status / Quick Controls -->
        <div class="flex items-start justify-between gap-2.5">
          <div class="flex min-w-0 flex-1 items-start gap-2.5">
            <div
              class="mt-0.5 flex h-8 w-8 shrink-0 items-center justify-center rounded-full"
              :class="
                item.isActive
                  ? 'bg-[var(--md-sys-color-primary-container)] text-[var(--md-sys-color-on-primary-container)]'
                  : item.status === 'completed'
                    ? 'bg-[var(--md-sys-color-success-container)] text-[var(--md-sys-color-on-success-container)]'
                    : item.status === 'failed'
                      ? 'bg-[var(--md-sys-color-error-container)] text-[var(--md-sys-color-on-error-container)]'
                      : 'bg-[var(--md-sys-color-surface-container-highest)] text-[var(--md-sys-color-on-surface-variant)]'
              "
            >
              <LuRefreshCw
                v-if="item.kind === 'scan'"
                class="h-4 w-4"
                :class="item.isActive ? 'animate-spin' : ''"
              />
              <LuUpload v-else-if="item.kind === 'upload'" class="h-4 w-4" />
              <LuDownload v-else class="h-4 w-4" />
            </div>

            <div class="min-w-0 flex-1">
              <div class="flex items-center gap-1.5">
                <h4 class="truncate text-xs font-semibold text-[var(--md-sys-color-on-surface)]">
                  {{ item.title }}
                </h4>
                <span
                  v-if="item.badge"
                  class="shrink-0 rounded-full px-1.5 py-0.5 text-[10px] font-semibold leading-none"
                  :class="
                    item.kind === 'download'
                      ? 'bg-[var(--md-sys-color-secondary-container)] text-[var(--md-sys-color-on-secondary-container)]'
                      : 'bg-[var(--md-sys-color-surface-container-high)] text-[var(--md-sys-color-on-surface-variant)]'
                  "
                >
                  {{ item.badge }}
                </span>
              </div>

              <p class="mt-0.5 truncate text-[11px] text-[var(--md-sys-color-on-surface-variant)]">
                {{ kindLabel(item.kind) }} · {{ item.metaText }}
              </p>
            </div>
          </div>

          <!-- Right Status / Icon Actions -->
          <div class="flex shrink-0 items-center gap-1" @click.stop>
            <span
              v-if="item.isActive && item.progress !== null"
              class="font-mono text-[11px] font-semibold text-[var(--md-sys-color-primary)]"
            >
              {{ item.progress }}%
            </span>
            <LuLoader2
              v-else-if="item.isActive"
              class="h-4 w-4 animate-spin text-[var(--md-sys-color-primary)]"
            />
            <LuCheckCircle2
              v-else-if="item.status === 'completed'"
              class="h-4 w-4 text-[var(--md-sys-color-success)]"
            />
            <LuAlertCircle
              v-else-if="item.status === 'failed'"
              class="h-4 w-4 text-[var(--md-sys-color-error)]"
            />
            <LuBan
              v-else-if="item.status === 'cancelled'"
              class="h-4 w-4 text-[var(--md-sys-color-on-surface-variant)]"
            />

            <button
              v-if="item.canCancel"
              type="button"
              :disabled="item.busy"
              class="md-icon-btn md-icon-btn-sm md-state !h-7 !w-7 text-[var(--md-sys-color-on-surface-variant)] hover:text-[var(--md-sys-color-error)]"
              :title="t('common.cancel')"
              :aria-label="t('common.cancel')"
              @click="handleCancel(item)"
            >
              <LuLoader2 v-if="item.busy" class="h-3.5 w-3.5 animate-spin" />
              <LuX v-else class="h-3.5 w-3.5" />
            </button>

            <button
              v-if="item.canRetry"
              type="button"
              class="md-icon-btn md-icon-btn-sm md-state !h-7 !w-7 text-[var(--md-sys-color-primary)]"
              :title="t('transfers.retry')"
              :aria-label="t('transfers.retry')"
              @click="handleRetry(item)"
            >
              <LuRotateCw class="h-3.5 w-3.5" />
            </button>

            <button
              v-if="item.canDelete"
              type="button"
              :disabled="item.busy"
              class="md-icon-btn md-icon-btn-sm md-state !h-7 !w-7 text-[var(--md-sys-color-on-surface-variant)] hover:text-[var(--md-sys-color-error)]"
              :title="t('common.delete')"
              :aria-label="t('common.delete')"
              @click="handleDelete(item)"
            >
              <LuLoader2 v-if="item.busy" class="h-3.5 w-3.5 animate-spin" />
              <LuTrash2 v-else class="h-3.5 w-3.5" />
            </button>
          </div>
        </div>

        <!-- Secondary / Warning / Error Notes -->
        <p
          v-if="item.secondaryText"
          class="mt-1.5 truncate text-[11px] text-[var(--md-sys-color-on-surface-variant)]"
        >
          {{ item.secondaryText }}
        </p>
        <p v-if="item.warningText" class="mt-1 text-[11px] text-[var(--md-sys-color-warning)]">
          {{ item.warningText }}
        </p>
        <p v-if="item.errorText" class="mt-1 break-words text-[11px] text-[var(--md-sys-color-error)]">
          {{ item.errorText }}
        </p>

        <!-- Linear Progress Bar -->
        <div
          v-if="item.isActive"
          class="relative mt-2.5 h-1.5 w-full overflow-hidden rounded-full bg-[var(--md-sys-color-surface-container-highest)]"
        >
          <div
            v-if="item.progress !== null"
            class="h-full rounded-full bg-[var(--md-sys-color-primary)] transition-all duration-200"
            :style="{ width: `${item.progress}%` }"
          />
          <div
            v-else
            class="task-indeterminate-bar h-full w-2/5 rounded-full bg-[var(--md-sys-color-primary)]"
          />
        </div>

        <!-- Bottom Action Bar (Completed Download / Target Jump) -->
        <div
          v-if="item.downloadFileUrl || (item.status === 'completed' && item.targetRoute)"
          class="mt-2.5 flex flex-wrap items-center gap-1.5"
          @click.stop
        >
          <a
            v-if="item.downloadFileUrl"
            :href="item.downloadFileUrl"
            :download="item.downloadFileName"
            class="md-btn md-btn-sm md-btn-filled md-state !h-7 !px-3 text-[11px]"
          >
            <LuDownload class="h-3.5 w-3.5" />
            <span>{{ t('transfers.downloadFile') }}</span>
          </a>

          <button
            v-if="item.kind === 'download' && item.targetRoute"
            type="button"
            class="md-btn md-btn-sm md-btn-tonal md-state !h-7 !px-3 text-[11px]"
            @click="handleNavigate(item.targetRoute)"
          >
            <LuBookOpen class="h-3.5 w-3.5" />
            <span>{{ t('transfers.openImportedBook') }}</span>
          </button>

          <button
            v-else-if="item.kind !== 'download' && item.status === 'completed' && item.targetRoute"
            type="button"
            class="md-btn md-btn-sm md-btn-tonal md-state !h-7 !px-3 text-[11px]"
            @click="handleNavigate(item.targetRoute)"
          >
            <LuArrowUpRight class="h-3.5 w-3.5" />
            <span>{{ t('transfers.openTarget') }}</span>
          </button>
        </div>
      </article>
    </div>

    <template #footer>
      <div class="flex w-full items-center justify-between gap-2">
        <button
          type="button"
          class="md-btn md-btn-sm md-btn-text md-state"
          :disabled="refreshing || onlineDownloadsStore.loading"
          @click="handleRefresh"
        >
          <LuRefreshCw
            class="h-3.5 w-3.5"
            :class="refreshing || onlineDownloadsStore.loading ? 'animate-spin' : ''"
          />
          <span>{{ t('common.refresh') }}</span>
        </button>

        <button
          type="button"
          class="md-btn md-btn-sm md-btn-tonal md-state"
          :disabled="!canClearFinished || clearing"
          @click="handleClearFinished"
        >
          <LuLoader2 v-if="clearing" class="h-3.5 w-3.5 animate-spin" />
          <span>{{ t('transfers.clearCompleted') }}</span>
        </button>
      </div>
    </template>
  </MdSideSheet>
</template>

<style scoped>
@keyframes task-drawer-indeterminate {
  0% {
    transform: translateX(-110%);
  }
  100% {
    transform: translateX(260%);
  }
}

.task-indeterminate-bar {
  animation: task-drawer-indeterminate 1.25s ease-in-out infinite;
}
</style>
