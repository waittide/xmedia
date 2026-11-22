<script setup lang="ts">
import { nextTick, ref, watch } from 'vue'
import { LuArrowDown, LuArrowUp, LuDisc3, LuTrash2, LuVolume2 } from 'vue-icons-plus/lu'
import { MdIconButton } from '@/components/ui'
import MediaCoverImage from '@/components/ui/MediaCoverImage.vue'
import { buildCoverUrl } from '../api'
import { useMusicI18n as useI18n } from '../i18n'
import { resolveOnlineItemCoverUrl, usePlayerStore, type PlayerQueueItem } from '../player'

const props = withDefaults(
  defineProps<{
    active?: boolean
    compact?: boolean
  }>(),
  {
    active: false,
    compact: false,
  },
)

const { t } = useI18n()
const playerStore = usePlayerStore()
const rowRefs = ref<(HTMLDivElement | null)[]>([])

function setRowRef(el: unknown, idx: number): void {
  rowRefs.value[idx] = el instanceof HTMLDivElement ? el : null
}

async function scrollActiveIntoView(): Promise<void> {
  if (!props.active) return
  const idx = playerStore.currentIndex
  if (idx < 0) return
  await nextTick()
  const el = rowRefs.value[idx]
  if (el && typeof el.scrollIntoView === 'function') {
    el.scrollIntoView({ block: 'nearest', behavior: 'smooth' })
  }
}

watch(
  () => [props.active, playerStore.currentIndex] as const,
  ([isActive]) => {
    if (isActive) {
      void scrollActiveIntoView()
    }
  },
  { immediate: true },
)

watch(
  () => playerStore.queue.length,
  () => {
    rowRefs.value = []
  },
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

function itemSourceLabel(item: PlayerQueueItem): string | null {
  if (item.kind === 'online' || item.kind === 'remote') {
    return item.source_name
  }
  return null
}
</script>

<template>
  <div
    v-if="playerStore.queue.length === 0"
    class="text-body-small flex h-44 items-center justify-center text-on-surface-variant"
  >
    {{ t('player.noTrack') }}
  </div>

  <div v-else class="flex flex-col gap-1">
    <div
      v-for="(item, idx) in playerStore.queue"
      :key="`${item.key}-${idx}`"
      :ref="(el) => setRowRef(el, idx)"
      class="group flex items-center gap-3 rounded-corner-md transition-colors duration-150 ease-standard"
      :class="[
        compact ? 'px-2.5 py-1.5' : 'px-3 py-2',
        idx === playerStore.currentIndex
          ? 'bg-secondary-container text-on-secondary-container'
          : 'text-on-surface hover:bg-surface-container-high',
      ]"
    >
      <button
        type="button"
        class="flex min-w-0 flex-1 cursor-pointer items-center gap-3 text-left focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-secondary"
        @click="playerStore.playAtIndex(idx)"
      >
        <MediaCoverImage
          :src="itemCoverSrc(item)"
          :alt="itemTitle(item)"
          aspect-class="h-10 w-10 shrink-0"
          rounded="rounded-corner-sm"
          :fallback-icon="LuDisc3"
        />

        <span class="min-w-0 flex-1">
          <span class="flex items-center gap-1.5">
            <span class="text-body-medium truncate font-medium">
              {{ itemTitle(item) }}
            </span>
            <span
              v-if="itemSourceLabel(item)"
              class="text-label-small shrink-0 rounded-corner-full px-2 py-0.5"
              :class="
                idx === playerStore.currentIndex
                  ? 'bg-on-secondary-container/12 text-on-secondary-container'
                  : 'bg-surface-container-highest text-on-surface-variant'
              "
            >
              {{ itemSourceLabel(item) }}
            </span>
          </span>
          <span
            class="text-body-small block truncate"
            :class="
              idx === playerStore.currentIndex
                ? 'text-on-secondary-container/80'
                : 'text-on-surface-variant'
            "
          >
            {{ itemSubtitle(item) }}
          </span>
        </span>
      </button>

      <div class="flex shrink-0 items-center gap-0.5">
        <LuVolume2
          v-if="idx === playerStore.currentIndex"
          class="mr-1 h-4 w-4 shrink-0 text-on-secondary-container"
          aria-hidden="true"
        />

        <div
          v-if="playerStore.queue.length > 1"
          class="flex items-center opacity-100 transition-opacity sm:opacity-0 sm:group-focus-within:opacity-100 sm:group-hover:opacity-100"
        >
          <MdIconButton
            size="sm"
            :icon="LuArrowUp"
            :disabled="idx === 0"
            :aria-label="t('player.moveUp')"
            :title="t('player.moveUp')"
            @click="playerStore.moveQueueItem(idx, idx - 1)"
          />
          <MdIconButton
            size="sm"
            :icon="LuArrowDown"
            :disabled="idx === playerStore.queue.length - 1"
            :aria-label="t('player.moveDown')"
            :title="t('player.moveDown')"
            @click="playerStore.moveQueueItem(idx, idx + 1)"
          />
        </div>

        <MdIconButton
          size="sm"
          :icon="LuTrash2"
          :aria-label="t('common.delete')"
          :title="t('common.delete')"
          @click="playerStore.removeQueueAt(idx)"
        />
      </div>
    </div>
  </div>
</template>
