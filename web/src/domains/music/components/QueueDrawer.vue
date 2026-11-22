<script setup lang="ts">
import { computed } from 'vue'
import { LuDisc3, LuTrash2, LuPlay, LuVolume2 } from 'vue-icons-plus/lu'
import MdSideSheet from '@/components/ui/MdSideSheet.vue'
import MediaCoverImage from '@/components/ui/MediaCoverImage.vue'
import RefreshButton from '@/components/ui/RefreshButton.vue'
import { useDialogStore } from '@/stores/dialog'
import { buildCoverUrl } from '../api'
import { useMusicI18n as useI18n } from '../i18n'
import { resolveOnlineItemCoverUrl, usePlayerStore, type PlayerQueueItem } from '../player'

const { t } = useI18n()
const dialogStore = useDialogStore()
const playerStore = usePlayerStore()

const queueSubtitle = computed(() =>
  [t('player.queueSubtitle'), t('common.tracksCount', { count: playerStore.queue.length })]
    .filter(Boolean)
    .join(' · '),
)

function itemTitle(item: PlayerQueueItem): string {
  if (item.kind === 'local') return item.track.title
  if (item.kind === 'remote') return item.song.title
  return item.resolved?.track.title || item.track.title
}

function itemSubtitle(item: PlayerQueueItem): string {
  if (item.kind === 'local') {
    return [item.track.artist_name, item.track.album_title].filter(Boolean).join(' · ')
  }
  if (item.kind === 'remote') {
    return [item.song.artist, item.song.album].filter(Boolean).join(' · ')
  }
  const artist = item.resolved?.track.artist || item.track.artist
  const album = item.resolved?.track.album ?? item.track.album
  return [artist, album].filter(Boolean).join(' · ')
}

function itemCoverSrc(item: PlayerQueueItem): string | null {
  if (item.kind === 'online') return resolveOnlineItemCoverUrl(item)
  if (item.kind === 'remote') return item.song.cover_url ?? null
  return item.track.cover_key ? buildCoverUrl(item.track.id, 80) : null
}

async function handleClearQueue(): Promise<void> {
  if (playerStore.queue.length === 0) return
  const wasOpen = playerStore.queueOpen
  playerStore.queueOpen = false
  const ok = await dialogStore.confirm({
    title: t('player.clearQueueConfirmTitle'),
    message: t('player.clearQueueConfirm', { count: playerStore.queue.length }),
    danger: true,
  })
  if (!ok) {
    playerStore.queueOpen = wasOpen
    return
  }
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
        v-for="(item, idx) in playerStore.queue"
        :key="`${item.key}-${idx}`"
        class="group flex items-center gap-3 rounded-2xl px-3 py-2 text-xs transition hover:bg-[var(--md-sys-color-surface-container-high)]"
        :class="
          idx === playerStore.currentIndex
            ? 'bg-[var(--md-sys-color-secondary-container)]/70'
            : ''
        "
      >
        <MediaCoverImage
          :src="itemCoverSrc(item)"
          :alt="itemTitle(item)"
          aspect-class="h-10 w-10 shrink-0"
          rounded="rounded-lg"
          :fallback-icon="LuDisc3"
          class="cursor-pointer"
          @click="playerStore.playAtIndex(idx)"
        />

        <button
          type="button"
          class="min-w-0 flex-1 cursor-pointer text-left"
          @click="playerStore.playAtIndex(idx)"
        >
          <div class="flex items-center gap-1.5">
            <span
              class="truncate font-semibold"
              :class="
                idx === playerStore.currentIndex
                  ? 'text-[var(--md-sys-color-primary)]'
                  : 'text-[var(--md-sys-color-on-surface)]'
              "
            >
              {{ itemTitle(item) }}
            </span>
            <span
              v-if="item.kind === 'online'"
              class="shrink-0 rounded-full bg-[var(--md-sys-color-primary-container)] px-1.5 py-0.2 text-[10px] font-medium text-[var(--md-sys-color-on-primary-container)]"
            >
              {{ item.source_name }}
            </span>
          </div>
          <p class="truncate text-[11px] text-[var(--md-sys-color-on-surface-variant)]">
            {{ itemSubtitle(item) }}
          </p>
        </button>

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
