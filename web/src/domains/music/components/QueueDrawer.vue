<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { LuDownload, LuTrash2 } from 'vue-icons-plus/lu'
import { MdButton, MdSheet } from '@/components/ui'
import { useMusicI18n as useI18n } from '../i18n'
import { usePlayerStore } from '../player'
import QueueListContent from './QueueListContent.vue'

const { t } = useI18n()
const playerStore = usePlayerStore()

const restoring = ref(false)
const confirmingClear = ref(false)
const confirmingRestore = ref(false)

const queueSubtitle = computed(() =>
  [t('player.queueSubtitle'), t('common.tracksCount', { count: playerStore.queue.length })]
    .filter(Boolean)
    .join(' · '),
)

watch(
  () => [playerStore.queueOpen, playerStore.queue.length] as const,
  ([isOpen, len]) => {
    if (!isOpen || len === 0) {
      confirmingClear.value = false
      confirmingRestore.value = false
    }
  },
)

function requestClearQueue(): void {
  if (playerStore.queue.length === 0) return
  confirmingRestore.value = false
  confirmingClear.value = true
}

function confirmClearQueue(): void {
  confirmingClear.value = false
  playerStore.clearQueue()
}

async function runRestoreCloudQueue(): Promise<void> {
  if (restoring.value) return
  restoring.value = true
  confirmingRestore.value = false
  try {
    await playerStore.restoreRemoteQueue(true)
  } finally {
    restoring.value = false
  }
}

function requestRestoreCloudQueue(): void {
  confirmingClear.value = false
  if (playerStore.queue.length > 0) {
    confirmingRestore.value = true
    return
  }
  void runRestoreCloudQueue()
}
</script>

<template>
  <MdSheet
    :open="playerStore.queueOpen"
    :title="t('player.queueTitle')"
    :subtitle="queueSubtitle"
    size="md"
    @close="playerStore.queueOpen = false"
  >
    <template #subheader>
      <!-- Inline Confirm Clear (avoids closing/flickering sheet and preserves [role="dialog"] E2E flow) -->
      <div
        v-if="confirmingClear"
        class="flex flex-wrap items-center justify-between gap-2 rounded-corner-md bg-error-container px-3 py-2 text-on-error-container"
      >
        <span class="text-body-small">
          {{ t('player.clearQueueConfirm', { count: playerStore.queue.length }) }}
        </span>
        <div class="flex shrink-0 items-center gap-1">
          <MdButton variant="filled" tone="error" @click="confirmClearQueue">
            {{ t('player.clearQueueConfirmAction') }}
          </MdButton>
          <MdButton variant="text" @click="confirmingClear = false">
            {{ t('player.cancel') }}
          </MdButton>
        </div>
      </div>

      <!-- Inline Confirm Restore Cloud Queue -->
      <div
        v-else-if="confirmingRestore"
        class="flex flex-wrap items-center justify-between gap-2 rounded-corner-md bg-secondary-container px-3 py-2 text-on-secondary-container"
      >
        <span class="text-body-small">
          {{ t('player.syncFromCloudConfirm', { count: playerStore.queue.length }) }}
        </span>
        <div class="flex shrink-0 items-center gap-1">
          <MdButton
            variant="filled"
            :loading="restoring"
            @click="runRestoreCloudQueue"
          >
            {{ t('player.syncFromCloudConfirmAction') }}
          </MdButton>
          <MdButton variant="text" @click="confirmingRestore = false">
            {{ t('player.cancel') }}
          </MdButton>
        </div>
      </div>

      <!-- Default Action Bar -->
      <div
        v-else
        class="flex items-center justify-between gap-2 rounded-corner-md bg-surface-container-low px-3 py-2"
      >
        <MdButton
          variant="text"
          :icon="LuDownload"
          :loading="restoring"
          @click="requestRestoreCloudQueue"
        >
          {{ t('player.syncFromCloud') }}
        </MdButton>

        <MdButton
          v-if="playerStore.queue.length > 0"
          variant="tonal"
          tone="error"
          :icon="LuTrash2"
          @click="requestClearQueue"
        >
          {{ t('player.clearQueue') }}
        </MdButton>
      </div>
    </template>

    <QueueListContent :active="playerStore.queueOpen" />
  </MdSheet>
</template>
