<script setup lang="ts">
import { computed } from 'vue'
import { useRouter } from 'vue-router'
import { useI18n } from 'vue-i18n'
import {
  LuUpload,
  LuChevronDown,
  LuChevronUp,
  LuX,
  LuCheckCircle2,
  LuAlertCircle,
  LuLoader2,
} from 'vue-icons-plus/lu'
import { useUploadStore } from '@/stores/uploads'

const { t } = useI18n()
const router = useRouter()
const uploadStore = useUploadStore()

const summaryText = computed(() => {
  const total = uploadStore.tasks.length
  const done = uploadStore.completedCount
  const failed = uploadStore.failedCount
  if (uploadStore.activeCount > 0) {
    return t('transfers.uploadingSummary', { done, total })
  }
  if (failed > 0) {
    return t('transfers.failedSummary', { done, failed })
  }
  return t('transfers.completedSummary', { done })
})

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  return `${(bytes / (1024 * 1024)).toFixed(2)} MB`
}
</script>

<template>
  <section
    v-if="uploadStore.tasks.length > 0"
    class="fixed bottom-24 right-4 z-40 w-[min(24rem,calc(100vw-2rem))] overflow-hidden rounded-2xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] shadow-2xl transition-all"
  >
    <div
      class="flex cursor-pointer items-center justify-between gap-2 bg-[var(--ctp-mantle)] px-4 py-3"
      @click="uploadStore.collapsed = !uploadStore.collapsed"
    >
      <div class="flex min-w-0 items-center gap-2.5">
        <div
          class="flex h-7 w-7 shrink-0 items-center justify-center rounded-lg bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]"
        >
          <LuLoader2 v-if="uploadStore.activeCount > 0" class="h-4 w-4 animate-spin" />
          <LuUpload v-else class="h-4 w-4" />
        </div>
        <p class="truncate text-xs font-semibold text-[var(--ctp-text)]">{{ summaryText }}</p>
      </div>

      <div class="flex shrink-0 items-center gap-1" @click.stop>
        <button
          v-if="uploadStore.completedCount > 0 && uploadStore.activeCount === 0"
          type="button"
          class="rounded-lg px-2 py-1 text-[11px] font-medium text-[var(--ctp-subtext0)] hover:bg-[var(--ctp-surface0)] hover:text-[var(--ctp-text)]"
          @click="uploadStore.clearCompleted()"
        >
          {{ t('transfers.clearCompleted') }}
        </button>

        <button
          type="button"
          class="rounded-lg p-1 text-[var(--ctp-subtext0)] hover:bg-[var(--ctp-surface0)] hover:text-[var(--ctp-text)]"
          @click="uploadStore.collapsed = !uploadStore.collapsed"
        >
          <LuChevronUp v-if="uploadStore.collapsed" class="h-4 w-4" />
          <LuChevronDown v-else class="h-4 w-4" />
        </button>

        <button
          v-if="uploadStore.activeCount === 0"
          type="button"
          class="rounded-lg p-1 text-[var(--ctp-subtext0)] hover:bg-[var(--ctp-surface0)] hover:text-[var(--ctp-text)]"
          @click="uploadStore.dismissAll()"
        >
          <LuX class="h-4 w-4" />
        </button>
      </div>
    </div>

    <div v-if="uploadStore.activeCount > 0" class="h-1 w-full bg-[var(--ctp-surface0)]">
      <div
        class="h-full bg-[var(--ctp-accent)] transition-all duration-200"
        :style="{ width: `${uploadStore.overallProgress}%` }"
      />
    </div>

    <div
      v-if="!uploadStore.collapsed"
      class="max-h-56 divide-y divide-[var(--ctp-surface0)]/60 overflow-y-auto"
    >
      <div
        v-for="task in uploadStore.tasks"
        :key="task.id"
        class="flex items-center gap-3 px-4 py-2.5 text-xs transition hover:bg-[var(--ctp-row-hover)]"
        :class="task.targetRoute ? 'cursor-pointer' : ''"
        @click="task.targetRoute && router.push(task.targetRoute)"
      >
        <div class="min-w-0 flex-1">
          <p class="truncate font-medium text-[var(--ctp-text)]">{{ task.label }}</p>
          <div class="mt-0.5 flex items-center gap-2 text-[11px] text-[var(--ctp-subtext0)]">
            <span v-if="task.status === 'uploading'">
              {{ formatSize(task.uploadedBytes) }} / {{ formatSize(task.totalBytes) }} ({{ task.progress }}%)
            </span>
            <span v-else-if="task.status === 'failed'" class="truncate text-[var(--ctp-red)]">
              {{ task.error || t('transfers.statusFailed') }}
            </span>
            <span v-else-if="task.status === 'completed'" class="text-[var(--ctp-green)]">
              {{ t('transfers.statusCompleted') }} · {{ formatSize(task.totalBytes) }}
            </span>
            <span v-else>{{ t('transfers.statusQueued') }}</span>
          </div>
        </div>

        <LuCheckCircle2
          v-if="task.status === 'completed'"
          class="h-4 w-4 shrink-0 text-[var(--ctp-green)]"
        />
        <LuAlertCircle
          v-else-if="task.status === 'failed'"
          class="h-4 w-4 shrink-0 text-[var(--ctp-red)]"
        />
        <LuLoader2
          v-else-if="task.status === 'uploading'"
          class="h-4 w-4 shrink-0 animate-spin text-[var(--ctp-accent)]"
        />
      </div>
    </div>
  </section>
</template>
