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
  LuChevronDown,
  LuChevronRight,
  LuDownload,
  LuInbox,
  LuLoader2,
  LuRefreshCw,
  LuRotateCw,
  LuTrash2,
  LuUpload,
  LuX,
} from 'vue-icons-plus/lu'
import {
  MdButton,
  MdCard,
  MdEmptyState,
  MdIconButton,
  MdLinearProgress,
  MdSheet,
} from '@/components/ui'
import { buildOnlineDownloadFileUrl } from '@/domains/book/onlineApi'
import { useAuthStore } from '@/stores/auth'
import { useEventsStore } from '@/stores/events'
import {
  formatDownloadSpeed,
  isOnlineDownloadJobActive,
  useOnlineDownloadsStore,
} from '@/stores/onlineDownloads'
import { useSnackbarStore } from '@/stores/snackbar'
import { useUploadStore } from '@/stores/uploads'
import { formatBytes } from '@/utils/format'

const props = defineProps<{
  open: boolean
}>()

const emit = defineEmits<{
  close: []
}>()

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

const showCompleted = ref(false)
const clearing = ref(false)
const refreshing = ref(false)

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
      targetRoute: `/${sc.media_kind}/settings?tab=libraries`,
      canCancel: false,
      canRetry: false,
      canDelete: false,
      busy: false,
    })
  }

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
          ? `/book/item/${job.imported_book_id}`
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

/** Active + failed tasks are always visible by default. */
const priorityTasks = computed(() =>
  allTasks.value.filter((item) => item.isActive || item.status === 'failed'),
)

/** Completed + cancelled tasks are collapsed behind a disclosure toggle by default. */
const finishedTasks = computed(() =>
  allTasks.value.filter((item) => !item.isActive && item.status !== 'failed'),
)

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
  } catch (err) {
    useSnackbarStore().show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  } finally {
    clearing.value = false
  }
}
</script>

<template>
  <MdSheet
    :open="open"
    :title="t('transfers.drawerTitle')"
    size="md"
    @close="emit('close')"
  >
    <template #headerActions>
      <MdIconButton
        v-if="authStore.hasModule('book')"
        size="sm"
        :icon="LuRefreshCw"
        :loading="refreshing"
        :tooltip="t('common.refresh')"
        @click="handleRefresh"
      />
    </template>

    <!-- Overall Empty State -->
    <MdEmptyState
      v-if="allTasks.length === 0"
      :icon="LuInbox"
      :title="t('transfers.emptyAll')"
      compact
    />

    <div v-else class="space-y-4">
      <!-- Active & Failed Tasks (always shown) -->
      <section v-if="priorityTasks.length > 0" class="space-y-2.5">
        <MdCard
          v-for="item in priorityTasks"
          :key="item.id"
          variant="tonal"
          padding="md"
          class="space-y-2.5"
        >
          <div class="flex items-start justify-between gap-3">
            <div class="flex min-w-0 items-start gap-3">
              <div
                class="mt-0.5 flex h-8 w-8 shrink-0 items-center justify-center rounded-corner-sm"
                :class="
                  item.status === 'failed'
                    ? 'bg-error-container text-on-error-container'
                    : 'bg-primary-container text-on-primary-container'
                "
              >
                <LuAlertCircle v-if="item.status === 'failed'" class="h-4 w-4" aria-hidden="true" />
                <LuLoader2 v-else-if="item.kind === 'scan'" class="h-4 w-4 animate-spin" aria-hidden="true" />
                <LuUpload v-else-if="item.kind === 'upload'" class="h-4 w-4" aria-hidden="true" />
                <LuDownload v-else class="h-4 w-4" aria-hidden="true" />
              </div>

              <div class="min-w-0">
                <div class="flex flex-wrap items-center gap-1.5">
                  <p class="text-title-small truncate text-on-surface">{{ item.title }}</p>
                  <span
                    class="text-label-small rounded-corner-xs bg-surface-container-highest px-1.5 py-0.5 text-on-surface-variant"
                  >
                    {{ kindLabel(item.kind) }}
                  </span>
                  <span
                    v-if="item.badge"
                    class="text-label-small rounded-corner-xs bg-secondary-container px-1.5 py-0.5 text-on-secondary-container"
                  >
                    {{ item.badge }}
                  </span>
                </div>
                <p class="text-body-small mt-0.5 text-on-surface-variant tabular-nums">
                  {{ item.metaText }}
                </p>
                <p v-if="item.secondaryText" class="text-body-small truncate text-on-surface-variant">
                  {{ item.secondaryText }}
                </p>
                <p v-if="item.warningText" class="text-body-small text-warning">
                  {{ item.warningText }}
                </p>
                <p v-if="item.errorText" class="text-body-small text-error break-words">
                  {{ item.errorText }}
                </p>
              </div>
            </div>

            <div class="flex shrink-0 items-center gap-1">
              <MdIconButton
                v-if="item.canRetry"
                size="sm"
                :icon="LuRotateCw"
                :disabled="item.busy"
                :tooltip="t('transfers.retry')"
                @click="handleRetry(item)"
              />
              <MdIconButton
                v-if="item.canCancel"
                size="sm"
                :icon="LuX"
                :disabled="item.busy"
                :tooltip="t('common.cancel')"
                @click="handleCancel(item)"
              />
              <MdIconButton
                v-if="item.canDelete"
                size="sm"
                :icon="LuTrash2"
                :disabled="item.busy"
                :tooltip="t('common.delete')"
                @click="handleDelete(item)"
              />
            </div>
          </div>

          <MdLinearProgress
            v-if="item.isActive"
            :value="item.progress"
            :indeterminate="item.progress === null"
            tone="primary"
          />
        </MdCard>
      </section>

      <MdEmptyState
        v-else
        :icon="LuCheckCircle2"
        :title="t('transfers.emptyFiltered')"
        compact
      />

      <!-- Collapsible Completed / Cancelled Tasks -->
      <section v-if="finishedTasks.length > 0" class="border-t border-outline-variant pt-3">
        <button
          type="button"
          class="md-state text-label-large flex h-10 w-full cursor-pointer items-center justify-between rounded-corner-sm px-2 text-on-surface-variant hover:text-on-surface"
          :aria-expanded="showCompleted"
          @click="showCompleted = !showCompleted"
        >
          <span class="flex items-center gap-2">
            <LuChevronDown v-if="showCompleted" class="h-4 w-4" aria-hidden="true" />
            <LuChevronRight v-else class="h-4 w-4" aria-hidden="true" />
            <span>{{ t('transfers.filterCompleted') }}</span>
          </span>
          <span class="text-label-small rounded-corner-full bg-surface-container-highest px-2 py-0.5 tabular-nums">
            {{ finishedTasks.length }}
          </span>
        </button>

        <div v-if="showCompleted" class="mt-2 space-y-2">
          <MdCard
            v-for="item in finishedTasks"
            :key="item.id"
            variant="filled"
            padding="sm"
            class="space-y-2"
          >
            <div class="flex items-start justify-between gap-2">
              <div class="flex min-w-0 items-start gap-2.5">
                <div
                  class="mt-0.5 flex h-7 w-7 shrink-0 items-center justify-center rounded-corner-sm"
                  :class="
                    item.status === 'cancelled'
                      ? 'bg-surface-container text-on-surface-variant'
                      : 'bg-success-container text-on-success-container'
                  "
                >
                  <LuBan v-if="item.status === 'cancelled'" class="h-4 w-4" aria-hidden="true" />
                  <LuCheckCircle2 v-else class="h-4 w-4" aria-hidden="true" />
                </div>
                <div class="min-w-0">
                  <p class="text-body-medium truncate text-on-surface">{{ item.title }}</p>
                  <p class="text-body-small text-on-surface-variant tabular-nums">
                    {{ item.metaText }}
                  </p>
                </div>
              </div>

              <div class="flex shrink-0 items-center gap-1">
                <MdButton
                  v-if="item.downloadFileUrl"
                  variant="text"
                  size="sm"
                  :href="item.downloadFileUrl"
                  :icon="LuDownload"
                  :download="item.downloadFileName"
                >
                  {{ t('transfers.downloadFile') }}
                </MdButton>
                <MdButton
                  v-if="item.targetRoute"
                  variant="text"
                  size="sm"
                  :icon="item.kind === 'download' ? LuBookOpen : LuArrowUpRight"
                  @click="handleNavigate(item.targetRoute)"
                >
                  {{ item.kind === 'download' ? t('transfers.openImportedBook') : t('transfers.openTarget') }}
                </MdButton>
                <MdIconButton
                  v-if="item.canRetry"
                  size="sm"
                  :icon="LuRotateCw"
                  :disabled="item.busy"
                  :tooltip="t('transfers.retry')"
                  @click="handleRetry(item)"
                />
                <MdIconButton
                  v-if="item.canDelete"
                  size="sm"
                  :icon="LuTrash2"
                  :disabled="item.busy"
                  :tooltip="t('common.delete')"
                  @click="handleDelete(item)"
                />
              </div>
            </div>
          </MdCard>
        </div>
      </section>
    </div>

    <template v-if="canClearFinished" #footer>
      <MdButton
        variant="text"
        size="sm"
        :icon="LuTrash2"
        :loading="clearing"
        @click="handleClearFinished"
      >
        {{ t('transfers.clearCompleted') }}
      </MdButton>
    </template>
  </MdSheet>
</template>
