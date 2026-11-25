<script setup lang="ts">
import { computed } from 'vue'
import { useI18n as useGlobalI18n } from 'vue-i18n'
import type { RouteLocationRaw } from 'vue-router'
import {
  LuCheck,
  LuCloud,
  LuDownload,
  LuFilm,
  LuFolderPlus,
  LuPencil,
  LuPlay,
  LuRefreshCw,
  LuTrash2,
} from 'vue-icons-plus/lu'
import {
  MediaDetailTemplate,
  type MediaDetailSpec,
  type MediaDetailTag,
} from '@/components/domain'
import { MdMenuItem } from '@/components/ui'
import { buildPosterUrl } from '@/domains/video/api'
import { useVideoI18n as useI18n } from '@/domains/video/i18n'
import type { VideoDetailDto, VideoSummaryDto } from '@/domains/video/types'
import { libraryRoute } from '@/sources'
import { formatBytes, formatDuration, formatTime } from '@/utils/format'

const props = defineProps<{
  detail: VideoDetailDto | null
  loading: boolean
  error: string | null
  favoriteBusy: boolean
  isAdmin: boolean
  cachingVideoIds: Set<string>
}>()

const emit = defineEmits<{
  back: []
  retry: []
  'toggle-favorite': []
  'open-shelf-picker': [target: { id: string; title: string }]
  'cache-video': [video: VideoSummaryDto]
  'toggle-watched': [video: VideoSummaryDto]
  'download-video': [videoId: string]
  'edit-video': [video: VideoSummaryDto]
  refresh: []
  unbind: []
  'delete-video': [video: VideoSummaryDto]
}>()

const { t } = useI18n()
const { t: globalT } = useGlobalI18n()

const video = computed(() => props.detail?.video ?? null)
const movieTracking = computed(() => video.value?.online_tracking ?? null)

const displayCover = computed(() => buildPosterUrl(video.value?.poster_key, 720))

const displaySubtitle = computed(() => {
  const v = video.value
  if (!v) return ''
  return [
    v.year ? String(v.year) : '',
    v.duration_secs > 0 ? formatDuration(v.duration_secs) : '',
    v.resolution || '',
  ]
    .filter(Boolean)
    .join(' · ')
})

const displayProgress = computed<{ percent: number | null; label: string }>(() => {
  const v = video.value
  const p = v?.progress
  if (!v || !p || p.completed || p.position_secs <= 0) {
    return { percent: null, label: '' }
  }
  const dur = p.duration_secs || v.duration_secs || 0
  if (dur <= 0) return { percent: null, label: '' }
  const pct = Math.min(100, Math.round((p.position_secs / dur) * 100))
  return {
    percent: pct > 0 ? pct : null,
    label: `${formatTime(p.position_secs)} / ${formatTime(dur)}`,
  }
})

const primaryActionLabel = computed(() => {
  const p = video.value?.progress
  return p && !p.completed && p.position_secs > 3 ? t('common.resume') : t('common.play')
})

const primaryActionTo = computed<RouteLocationRaw | undefined>(() =>
  video.value ? { path: `/video/play/${encodeURIComponent(video.value.id)}` } : undefined,
)

const displayTags = computed<MediaDetailTag[]>(() => {
  const genres = video.value?.genres ?? []
  const tags = video.value?.tags ?? []
  return [
    ...genres.map((g) => ({
      label: g,
      value: `genre:${g}`,
      to: libraryRoute('video', { genre: g }),
    })),
    ...tags.map((tg) => ({
      label: `#${tg}`,
      value: `tag:${tg}`,
      to: libraryRoute('video', { tag: tg }),
    })),
  ]
})

const movieSpecs = computed<MediaDetailSpec[]>(() => {
  if (!props.detail) return []
  const d = props.detail
  const v = d.video
  const specs: MediaDetailSpec[] = []
  if (v.resolution) {
    const dims = v.width && v.height ? ` (${v.width}×${v.height})` : ''
    specs.push({ label: t('detail.resolution'), value: `${v.resolution}${dims}` })
  }
  if (v.video_codec) {
    specs.push({ label: t('detail.codec'), value: v.video_codec.toUpperCase(), mono: true })
  }
  if (d.bit_rate_kbps) {
    specs.push({ label: t('detail.bitrate'), value: `${d.bit_rate_kbps} kbps`, mono: true })
  }
  if (v.container) {
    specs.push({ label: t('detail.container'), value: v.container.toUpperCase(), mono: true })
  }
  if (v.file_size > 0) {
    specs.push({ label: t('detail.fileSize'), value: formatBytes(v.file_size) })
  }
  if (v.duration_secs > 0) {
    specs.push({ label: t('detail.duration'), value: formatDuration(v.duration_secs) })
  }
  if (d.audio_streams.length > 0) {
    specs.push({
      label: t('detail.audioTracks'),
      value: d.audio_streams
        .map((a) =>
          [
            a.title || a.language?.toUpperCase(),
            a.codec.toUpperCase(),
            a.channels ? `${a.channels}ch` : '',
          ]
            .filter(Boolean)
            .join(' '),
        )
        .join(' / '),
    })
  }
  specs.push({
    label: t('detail.subtitles'),
    value:
      d.subtitles.length > 0
        ? d.subtitles.map((s) => s.label).join(', ')
        : t('detail.noSubtitles'),
  })
  if (props.isAdmin && d.path) {
    specs.push({ label: t('detail.filePath'), value: d.path, mono: true })
  }
  return specs
})
</script>

<template>
  <MediaDetailTemplate
    :title="video?.title ?? ''"
    :subtitle="displaySubtitle"
    :cover="displayCover"
    aspect="video"
    cover-style="plain"
    :fallback-icon="LuFilm"
    :back-label="t('common.back')"
    :progress-percent="displayProgress.percent"
    :progress-label="displayProgress.label"
    :primary-action-label="primaryActionLabel"
    :primary-action-icon="LuPlay"
    :primary-action-to="primaryActionTo"
    :secondary-action-label="video ? t('common.addToShelf') : ''"
    :secondary-action-icon="LuFolderPlus"
    :show-favorite="Boolean(video)"
    :favorite="Boolean(video?.is_favorite)"
    :favorite-loading="favoriteBusy"
    :description="video?.summary ?? null"
    :tags="displayTags"
    :specs="movieSpecs"
    :specs-title="t('detail.specsTitle')"
    :loading="loading"
    :error="error"
    @back="emit('back')"
    @secondary-action="video && emit('open-shelf-picker', { id: video.id, title: video.title })"
    @toggle-favorite="emit('toggle-favorite')"
    @retry="emit('retry')"
  >
    <template v-if="movieTracking" #hero-meta>
      <div class="mt-1 flex flex-wrap items-center gap-2">
        <span
          class="text-label-medium inline-flex items-center gap-1.5 rounded-corner-full px-2.5 py-0.5"
          :class="
            movieTracking.cached
              ? 'bg-secondary-container text-on-secondary-container'
              : 'bg-tertiary-container text-on-tertiary-container'
          "
        >
          <LuCloud class="h-3.5 w-3.5 shrink-0" aria-hidden="true" />
          <span>
            {{
              movieTracking.cached
                ? t('online.trackedVideoCachedBadge')
                : t('online.trackedVideoOnlineBadge')
            }}
          </span>
        </span>
      </div>
    </template>

    <template v-if="video" #overflow-menu="{ close }">
      <MdMenuItem
        v-if="movieTracking && !movieTracking.cached"
        :icon="LuCloud"
        :label="t('online.cacheVideoBtn')"
        :disabled="cachingVideoIds.has(video.id)"
        @select="
          close();
          emit('cache-video', video)
        "
      />
      <MdMenuItem
        :icon="LuCheck"
        :label="video.progress?.completed ? t('common.markUnwatched') : t('common.markWatched')"
        @select="
          close();
          emit('toggle-watched', video)
        "
      />
      <MdMenuItem
        v-if="!movieTracking || movieTracking.cached"
        :icon="LuDownload"
        :label="t('common.download')"
        @select="
          close();
          emit('download-video', video.id)
        "
      />
      <MdMenuItem
        v-if="isAdmin"
        :icon="LuPencil"
        :label="t('common.editMetadata')"
        @select="
          close();
          emit('edit-video', video)
        "
      />
      <MdMenuItem
        :icon="LuRefreshCw"
        :label="t('common.refresh')"
        @select="
          close();
          emit('refresh')
        "
      />
    </template>

    <template v-if="video" #overflow-danger="{ close }">
      <MdMenuItem
        :icon="LuTrash2"
        :label="globalT('pool.unbindBtn')"
        destructive
        @select="
          close();
          emit('unbind')
        "
      />
      <MdMenuItem
        v-if="isAdmin"
        :icon="LuTrash2"
        :label="t('common.delete')"
        destructive
        @select="
          close();
          emit('delete-video', video)
        "
      />
    </template>
  </MediaDetailTemplate>
</template>
