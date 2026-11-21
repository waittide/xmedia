<script setup lang="ts">
import { computed } from 'vue'
import { LuTrash2, LuPlay, LuVolume2 } from 'vue-icons-plus/lu'
import MdSideSheet from '@/components/ui/MdSideSheet.vue'
import RefreshButton from '@/components/ui/RefreshButton.vue'
import { useDialogStore } from '@/stores/dialog'
import { useMusicI18n as useI18n } from '../i18n'
import { usePlayerStore } from '../player'
import CoverImage from './CoverImage.vue'

const { t } = useI18n()
const dialogStore = useDialogStore()
const playerStore = usePlayerStore()

const queueSubtitle = computed(() =>
  [t('player.queueSubtitle'), t('common.tracksCount', { count: playerStore.queue.length })]
    .filter(Boolean)
    .join(' · '),
)

async function handleClearQueue(): Promise<void> {
  if (playerStore.queue.length === 0) return
  const ok = await dialogStore.confirm({
    title: t('player.clearQueueConfirmTitle'),
    message: t('player.clearQueueConfirm', { count: playerStore.queue.length }),
    danger: true,
  })
  if (!ok) return
  playerStore.clearQueue()
}
</script>

<template>
  <MdSideSheet
    :open="playerStore.queueOpen"
    :title="t('player.queueTitle')"
    :subtitle="queueSubtitle"
    width-class="w-full max-w-md"
    @close="playerStore.queueOpen = false"
  >
    <template #subheader>
      <div
        class="flex items-center justify-between gap-2 border-b border-[var(--md-sys-color-outline-variant)]/50 pb-3"
      >
        <RefreshButton
          variant="ghost"
          size="md"
          :action="() => playerStore.restoreRemoteQueue(true)"
          :title="t('player.syncFromCloud')"
        />

        <button
          v-if="playerStore.queue.length > 0"
          type="button"
          class="md-btn md-btn-sm md-state bg-[var(--md-sys-color-error-container)] text-[var(--md-sys-color-on-error-container)]"
          :title="t('player.clearQueue')"
          @click="handleClearQueue"
        >
          <LuTrash2 class="h-3.5 w-3.5" />
          <span>{{ t('player.clearQueue') }}</span>
        </button>
      </div>
    </template>

    <div
      v-if="playerStore.queue.length === 0"
      class="flex h-48 items-center justify-center text-xs text-[var(--md-sys-color-on-surface-variant)]"
    >
      {{ t('player.noTrack') }}
    </div>

    <div v-else class="space-y-1">
      <div
        v-for="(tr, idx) in playerStore.queue"
        :key="`${tr.id}-${idx}`"
        class="group flex items-center gap-3 rounded-2xl px-3 py-2 text-xs transition hover:bg-[var(--md-sys-color-surface-container-high)]"
        :class="
          idx === playerStore.currentIndex
            ? 'bg-[var(--md-sys-color-secondary-container)]/70'
            : ''
        "
      >
        <CoverImage
          :cover-id="tr.id"
          :has-cover="Boolean(tr.cover_key)"
          :size="80"
          rounded="rounded-lg"
          class="h-10 w-10 shrink-0 cursor-pointer"
          @click="playerStore.playAtIndex(idx)"
        />

        <div
          class="min-w-0 flex-1 cursor-pointer"
          @click="playerStore.playAtIndex(idx)"
        >
          <p
            class="truncate font-semibold"
            :class="
              idx === playerStore.currentIndex
                ? 'text-[var(--md-sys-color-primary)]'
                : 'text-[var(--md-sys-color-on-surface)]'
            "
          >
            {{ tr.title }}
          </p>
          <p class="truncate text-[11px] text-[var(--md-sys-color-on-surface-variant)]">
            {{ tr.artist_name }} · {{ tr.album_title }}
          </p>
        </div>

        <LuVolume2
          v-if="idx === playerStore.currentIndex"
          class="h-4 w-4 shrink-0 text-[var(--md-sys-color-primary)]"
        />
        <button
          v-else
          type="button"
          class="md-icon-btn md-icon-btn-sm md-state hidden group-hover:inline-flex"
          :title="t('player.play')"
          @click="playerStore.playAtIndex(idx)"
        >
          <LuPlay class="h-3.5 w-3.5 fill-current" />
        </button>

        <button
          type="button"
          class="md-icon-btn md-icon-btn-sm md-state hover:text-[var(--md-sys-color-error)]"
          :title="t('common.delete')"
          @click="playerStore.removeQueueAt(idx)"
        >
          <LuTrash2 class="h-3.5 w-3.5" />
        </button>
      </div>
    </div>
  </MdSideSheet>
</template>
