<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import { LuDisc3, LuPlus, LuX } from 'vue-icons-plus/lu'
import { MdButton, MediaCoverImage } from '@/components/ui'
import type { PoolTrackItemVM } from '@/sources'

defineProps<{
  items: readonly PoolTrackItemVM[]
  isTargetingSelf: boolean
  busyIds: ReadonlySet<string>
}>()

const emit = defineEmits<{
  bind: [id: string]
  unbind: [id: string]
  bindAlbum: [albumId: string]
}>()

const { t } = useI18n()

function formatDuration(secs: number): string {
  if (!Number.isFinite(secs) || secs <= 0) return '0:00'
  const m = Math.floor(secs / 60)
  const s = Math.floor(secs % 60)
  return `${m}:${String(s).padStart(2, '0')}`
}
</script>

<template>
  <div class="space-y-1 rounded-corner-lg bg-surface-container-low p-1.5">
    <div
      v-for="track in items"
      :key="track.id"
      class="flex items-center gap-3 rounded-corner-md px-3 py-2.5 transition-colors hover:bg-surface-container"
    >
      <div class="h-10 w-10 shrink-0 overflow-hidden rounded-corner-sm bg-surface-container-high">
        <MediaCoverImage
          :src="track.coverUrl"
          :alt="track.title"
          aspect-class="aspect-square"
          rounded="rounded-corner-sm"
          :fallback-icon="LuDisc3"
          fallback-icon-class="h-4 w-4 text-on-surface-variant/60"
        />
      </div>

      <div class="min-w-0 flex-1">
        <div class="flex items-center gap-2">
          <p class="text-title-small truncate text-on-surface">{{ track.title }}</p>
          <span
            v-if="isTargetingSelf && track.bound"
            class="text-label-small shrink-0 rounded-corner-full bg-primary-container px-2 py-0.5 text-on-primary-container"
          >
            {{ t('pool.boundLabel') }}
          </span>
        </div>
        <p class="text-body-small truncate text-on-surface-variant">
          {{ track.subtitle }}
        </p>
      </div>

      <span class="text-label-small hidden shrink-0 tabular-nums text-on-surface-variant sm:inline">
        {{ track.formatLabel }} · {{ formatDuration(track.durationSecs) }}
      </span>

      <div class="flex shrink-0 items-center gap-1.5">
        <MdButton
          v-if="track.albumId"
          variant="text"
          size="sm"
          class="hidden md:inline-flex"
          :loading="busyIds.has(`album:${track.albumId}`)"
          @click="emit('bindAlbum', track.albumId)"
        >
          {{ t('pool.bindAlbumBtn') }}
        </MdButton>

        <MdButton
          v-if="isTargetingSelf && track.bound"
          variant="outlined"
          size="sm"
          :icon="LuX"
          :loading="busyIds.has(track.id)"
          @click="emit('unbind', track.id)"
        >
          {{ t('pool.unbindBtn') }}
        </MdButton>
        <MdButton
          v-else
          variant="filled"
          size="sm"
          :icon="LuPlus"
          :loading="busyIds.has(track.id)"
          @click="emit('bind', track.id)"
        >
          {{ t('pool.bindBtn') }}
        </MdButton>
      </div>
    </div>
  </div>
</template>
