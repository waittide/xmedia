<script setup lang="ts">
import { computed, watch } from 'vue'
import { useRouter } from 'vue-router'
import { useI18n } from 'vue-i18n'
import FloatingTaskShelf, {
  type FloatingTaskItem,
} from '@/components/ui/FloatingTaskShelf.vue'
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

const { t } = useI18n()
const router = useRouter()
const eventsStore = useEventsStore()
const uploadStore = useUploadStore()

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

const routeByTaskId = computed<Record<string, string>>(() => {
  const map: Record<string, string> = {}
  for (const sc of eventsStore.scanTaskList) {
    map[`scan:${sc.library_id}`] =
      sc.media_kind === 'comic'
        ? '/comic/libraries'
        : `/${sc.media_kind}/collections/libraries`
  }
  for (const up of uploadStore.tasks) {
    if (up.targetRoute) {
      map[`upload:${up.id}`] = up.targetRoute
    }
  }
  return map
})

const combinedTasks = computed<FloatingTaskItem[]>(() => {
  const scanItems: FloatingTaskItem[] = eventsStore.scanTaskList.map((sc) => {
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
      clickable: true,
    }
  })

  const uploadItems: FloatingTaskItem[] = uploadStore.tasks.map((task) => {
    let subtitle = ''
    if (task.status === 'uploading') {
      subtitle = `${formatSize(task.uploadedBytes)} / ${formatSize(task.totalBytes)}`
    } else if (task.status === 'failed') {
      subtitle = task.error || t('transfers.statusFailed')
    } else if (task.status === 'completed') {
      subtitle = `${t('transfers.statusCompleted')} · ${formatSize(task.totalBytes)}`
    } else {
      subtitle = t('transfers.statusQueued')
    }

    return {
      id: `upload:${task.id}`,
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
      clickable: Boolean(task.targetRoute),
    }
  })

  return [...scanItems, ...uploadItems]
})

function handleClearCompleted(): void {
  eventsStore.clearCompletedScans()
  uploadStore.clearCompleted()
}

function handleDismissAll(): void {
  eventsStore.dismissFinishedScans()
  uploadStore.dismissAll()
}

function handleSelectTask(item: FloatingTaskItem): void {
  const target = routeByTaskId.value[item.id]
  if (target) {
    void router.push(target)
  }
}
</script>

<template>
  <FloatingTaskShelf
    :tasks="combinedTasks"
    :collapsed="uploadStore.collapsed"
    :elevated-bottom="elevatedBottom"
    @update:collapsed="uploadStore.collapsed = $event"
    @clear-completed="handleClearCompleted"
    @dismiss-all="handleDismissAll"
    @select-task="handleSelectTask"
  />
</template>
