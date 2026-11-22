<script setup lang="ts">
import { computed, ref } from 'vue'
import {
  LuCheck,
  LuDownload,
  LuFilm,
  LuFolderMinus,
  LuFolderPlus,
  LuPencil,
  LuPlay,
  LuTrash2,
  LuTv,
} from 'vue-icons-plus/lu'
import { MediaItemCard } from '@/components/domain'
import { MdMenuItem } from '@/components/ui'
import type { MediaItemVM } from '@/sources'
import { useAuthStore } from '@/stores/auth'
import { useDialogStore } from '@/stores/dialog'
import { useSnackbarStore } from '@/stores/snackbar'
import { formatDuration } from '@/utils/format'
import { api, buildPosterUrl, buildVideoDownloadUrl } from '../api'
import { useVideoI18n as useI18n } from '../i18n'
import { useVideoStore } from '../store'
import type { VideoSummaryDto } from '../types'
import VideoShelfPickerDialog from './VideoShelfPickerDialog.vue'

const props = withDefaults(
  defineProps<{
    video: VideoSummaryDto
    removableFromShelf?: boolean
    /** When true (e.g. Continue Watching), primary click jumps straight to playback. */
    preferPlay?: boolean
  }>(),
  {
    removableFromShelf: false,
    preferPlay: false,
  },
)

const emit = defineEmits<{
  edit: [video: VideoSummaryDto]
  deleted: [videoId: string]
  removeFromShelf: [videoId: string]
  updated: [video: VideoSummaryDto]
}>()

const { t } = useI18n()
const authStore = useAuthStore()
const videoStore = useVideoStore()
const dialogStore = useDialogStore()
const snackbar = useSnackbarStore()

const favoriteBusy = ref(false)
const shelfPickerOpen = ref(false)

const progressPercent = computed<number | undefined>(() => {
  const p = props.video.progress
  if (!p) return undefined
  if (p.completed) return 100
  const dur = Math.max(1, props.video.duration_secs || p.duration_secs || 1)
  const pct = Math.min(100, Math.round((p.position_secs / dur) * 100))
  return pct > 0 ? pct : undefined
})

const episodeTag = computed(() => {
  if (props.video.season_number != null && props.video.episode_number != null) {
    return `S${String(props.video.season_number).padStart(2, '0')}E${String(props.video.episode_number).padStart(2, '0')}`
  }
  if (props.video.episode_number != null) {
    return `E${String(props.video.episode_number).padStart(2, '0')}`
  }
  return ''
})

const subtitleText = computed(() => {
  const parts: string[] = []
  if (props.video.show_title) {
    parts.push(episodeTag.value ? `${props.video.show_title} ${episodeTag.value}` : props.video.show_title)
  } else if (episodeTag.value) {
    parts.push(episodeTag.value)
  }
  if (props.video.year) {
    parts.push(String(props.video.year))
  }
  return parts.join(' · ') || undefined
})

const durationLabel = computed(() =>
  props.video.duration_secs > 0 ? formatDuration(props.video.duration_secs) : '',
)

const cardVM = computed<MediaItemVM>(() => {
  const encodedId = encodeURIComponent(props.video.id)
  const playTarget = { path: `/video/play/${encodedId}` }
  const detailTarget = !props.video.show_id
    ? { path: `/video/item/${encodedId}`, query: { kind: 'movie' } }
    : playTarget

  return {
    ref: { scope: 'local', domain: 'video', id: props.video.id },
    title: props.video.title,
    subtitle: subtitleText.value,
    cover: buildPosterUrl(props.video.poster_key, 560) ?? undefined,
    badge: {
      sourceKind: 'local',
      progressPercent: progressPercent.value,
      status: props.video.progress?.completed ? t('common.watched') : undefined,
    },
    to: detailTarget,
    playTo: playTarget,
  }
})

async function handleToggleFavorite(): Promise<void> {
  if (favoriteBusy.value) return
  favoriteBusy.value = true
  try {
    await videoStore.toggleVideoFavorite(props.video)
    emit('updated', { ...props.video })
  } catch (err) {
    snackbar.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  } finally {
    favoriteBusy.value = false
  }
}

async function handleToggleWatched(close: () => void): Promise<void> {
  close()
  try {
    const nextWatched = !(props.video.progress?.completed ?? false)
    const prog = await api.setWatched(props.video.id, nextWatched)
    const copy: VideoSummaryDto = { ...props.video, progress: prog }
    emit('updated', copy)
    videoStore.bumpCatalogVersion()
  } catch (err) {
    snackbar.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  }
}

function handleDownload(close: () => void): void {
  close()
  const a = document.createElement('a')
  a.href = buildVideoDownloadUrl(props.video.id)
  a.download = ''
  a.rel = 'noopener'
  document.body.appendChild(a)
  a.click()
  a.remove()
}

async function handleConfirmDelete(close: () => void): Promise<void> {
  close()
  const ok = await dialogStore.confirm({
    title: t('common.delete'),
    message: t('metadata.deleteVideoConfirm', { title: props.video.title }),
    danger: true,
  })
  if (!ok) return
  try {
    await api.deleteVideo(props.video.id)
    await videoStore.refreshSidebar(true)
    videoStore.bumpCatalogVersion()
    emit('deleted', props.video.id)
  } catch (err) {
    snackbar.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  }
}
</script>

<template>
  <MediaItemCard
    :item="cardVM"
    aspect="video"
    cover-style="plain"
    :fallback-icon="LuFilm"
    :duration-label="durationLabel"
    :prefer-play-route="preferPlay"
    :show-favorite="true"
    :favorite="video.is_favorite"
    :favorite-loading="favoriteBusy"
    @toggle-favorite="handleToggleFavorite"
  >
    <template #menu="{ close }">
      <MdMenuItem
        :icon="LuPlay"
        :label="t('common.play')"
        :to="`/video/play/${encodeURIComponent(video.id)}`"
        @select="close()"
      />

      <MdMenuItem
        v-if="video.show_id"
        :icon="LuTv"
        :label="t('common.viewSeries')"
        :to="`/video/item/${encodeURIComponent(video.show_id)}`"
        @select="close()"
      />

      <MdMenuItem
        :icon="LuCheck"
        :label="video.progress?.completed ? t('common.markUnwatched') : t('common.markWatched')"
        @select="handleToggleWatched(close)"
      />

      <MdMenuItem
        :icon="LuFolderPlus"
        :label="t('common.addToShelf')"
        @select="
          close();
          shelfPickerOpen = true
        "
      />

      <MdMenuItem
        v-if="authStore.isAdmin"
        :icon="LuPencil"
        :label="t('common.editMetadata')"
        @select="
          close();
          emit('edit', video)
        "
      />

      <MdMenuItem
        :icon="LuDownload"
        :label="t('common.download')"
        @select="handleDownload(close)"
      />

      <MdMenuItem
        v-if="removableFromShelf"
        :icon="LuFolderMinus"
        :label="t('shelf.removeVideo')"
        @select="
          close();
          emit('removeFromShelf', video.id)
        "
      />

      <div
        v-if="authStore.isAdmin"
        class="my-1 border-t border-outline-variant pt-1"
        role="separator"
      >
        <MdMenuItem
          :icon="LuTrash2"
          :label="t('common.delete')"
          destructive
          @select="handleConfirmDelete(close)"
        />
      </div>
    </template>
  </MediaItemCard>

  <VideoShelfPickerDialog
    v-if="shelfPickerOpen"
    :open="shelfPickerOpen"
    :video-id="video.id"
    :video-title="video.title"
    @close="shelfPickerOpen = false"
  />
</template>
