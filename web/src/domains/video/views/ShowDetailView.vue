<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useRoute, useRouter, type RouteLocationRaw } from 'vue-router'
import {
  LuCheck,
  LuDownload,
  LuFilm,
  LuFolderPlus,
  LuMoreVertical,
  LuPencil,
  LuPlay,
  LuRefreshCw,
  LuTrash2,
  LuTv,
} from 'vue-icons-plus/lu'
import {
  MediaDetailTemplate,
  type MediaDetailSpec,
  type MediaDetailTag,
} from '@/components/domain'
import {
  MdEmptyState,
  MdIconButton,
  MdLinearProgress,
  MdMenu,
  MdMenuItem,
  MediaCoverImage,
  type MdTabItem,
} from '@/components/ui'
import { libraryRoute } from '@/sources'
import { useAuthStore } from '@/stores/auth'
import { useDialogStore } from '@/stores/dialog'
import { useSnackbarStore } from '@/stores/snackbar'
import { formatBytes, formatDuration, formatTime } from '@/utils/format'
import { api, buildPosterUrl, buildVideoDownloadUrl } from '../api'
import ShowMetadataModal from '../components/ShowMetadataModal.vue'
import VideoMetadataModal from '../components/VideoMetadataModal.vue'
import VideoShelfPickerDialog from '../components/VideoShelfPickerDialog.vue'
import { useVideoI18n as useI18n } from '../i18n'
import { useVideoStore } from '../store'
import type { ShowDetailDto, VideoDetailDto, VideoSummaryDto } from '../types'

const { t } = useI18n()
const route = useRoute()
const router = useRouter()
const authStore = useAuthStore()
const videoStore = useVideoStore()
const dialogStore = useDialogStore()
const snackbar = useSnackbarStore()

const mode = ref<'show' | 'movie'>('show')
const showDetail = ref<ShowDetailDto | null>(null)
const movieDetail = ref<VideoDetailDto | null>(null)
const selectedSeason = ref<string>('')
const loading = ref(true)
const errorMessage = ref<string | null>(null)
const favoriteBusy = ref(false)

const editShowOpen = ref(false)
const editingVideo = ref<VideoSummaryDto | null>(null)
const shelfPickerTarget = ref<{ id: string; title: string } | null>(null)

let loadSeq = 0

const itemId = computed(() => String(route.params.id || '').trim())
const isMovieQuery = computed(() => route.query.kind === 'movie')

async function loadItem(): Promise<void> {
  const id = itemId.value
  const seq = ++loadSeq
  errorMessage.value = null

  if (!id) {
    showDetail.value = null
    movieDetail.value = null
    loading.value = false
    errorMessage.value = t('detail.notFound')
    return
  }

  loading.value = true
  try {
    if (isMovieQuery.value) {
      const res = await api.getVideo(id)
      if (seq !== loadSeq) return
      mode.value = 'movie'
      movieDetail.value = res
      showDetail.value = null
      return
    }

    try {
      const res = await api.getShow(id)
      if (seq !== loadSeq) return
      mode.value = 'show'
      showDetail.value = res
      movieDetail.value = null
      if (res.seasons.length > 0) {
        const exists = res.seasons.some((s) => String(s.season_number) === selectedSeason.value)
        if (!exists) {
          selectedSeason.value = String(res.seasons[0]!.season_number)
        }
      } else {
        selectedSeason.value = ''
      }
      return
    } catch (showErr) {
      // Fallback: if this ID belongs to a standalone video without `?kind=movie`
      try {
        const res = await api.getVideo(id)
        if (seq !== loadSeq) return
        mode.value = 'movie'
        movieDetail.value = res
        showDetail.value = null
        return
      } catch {
        throw showErr
      }
    }
  } catch (err) {
    if (seq !== loadSeq) return
    showDetail.value = null
    movieDetail.value = null
    errorMessage.value = err instanceof Error ? err.message : t('detail.loadFailed')
  } finally {
    if (seq === loadSeq) {
      loading.value = false
    }
  }
}

function handleBack(): void {
  if (window.history.state?.back) {
    router.back()
  } else {
    void router.push(
      mode.value === 'show'
        ? libraryRoute('video', { view: 'shows' })
        : libraryRoute('video'),
    )
  }
}

// Show computeds
const seasonTabs = computed<MdTabItem<string>[]>(() => {
  if (mode.value !== 'show' || !showDetail.value) return []
  return showDetail.value.seasons.map((sg) => ({
    value: String(sg.season_number),
    label:
      sg.season_number === 0
        ? t('catalog.specialsHeader')
        : t('catalog.seasonHeader', { season: sg.season_number }),
    badge: sg.episodes.length,
  }))
})

const activeSeasonGroup = computed(() => {
  if (mode.value !== 'show' || !showDetail.value) return null
  return (
    showDetail.value.seasons.find((s) => String(s.season_number) === selectedSeason.value) ??
    showDetail.value.seasons[0] ??
    null
  )
})

const primaryPlayEpisode = computed<VideoSummaryDto | null>(() => {
  if (mode.value !== 'show' || !showDetail.value) return null
  if (showDetail.value.continue_episode) return showDetail.value.continue_episode
  return showDetail.value.seasons[0]?.episodes[0] ?? null
})

// Unified template bindings
const displayTitle = computed(() => {
  if (mode.value === 'movie') return movieDetail.value?.video.title ?? ''
  return showDetail.value?.show.title ?? ''
})

const displayCover = computed(() => {
  if (mode.value === 'movie') {
    return buildPosterUrl(movieDetail.value?.video.poster_key, 720)
  }
  return buildPosterUrl(showDetail.value?.show.poster_key, 540)
})

const displaySubtitle = computed(() => {
  if (mode.value === 'movie') {
    const v = movieDetail.value?.video
    if (!v) return ''
    return [
      v.year ? String(v.year) : '',
      v.duration_secs > 0 ? formatDuration(v.duration_secs) : '',
      v.resolution || '',
    ]
      .filter(Boolean)
      .join(' · ')
  }
  const sh = showDetail.value?.show
  if (!sh) return ''
  return [
    sh.year ? String(sh.year) : '',
    t('common.seasonsCount', { count: sh.season_count }),
    t('common.watchedProgress', {
      watched: sh.watched_episode_count,
      total: sh.episode_count,
    }),
  ]
    .filter(Boolean)
    .join(' · ')
})

const displayProgress = computed<{ percent: number | null; label: string }>(() => {
  if (mode.value === 'movie') {
    const v = movieDetail.value?.video
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
  }

  const sh = showDetail.value?.show
  if (!sh || sh.episode_count <= 0 || sh.watched_episode_count <= 0) {
    return { percent: null, label: '' }
  }
  return {
    percent: Math.min(100, Math.round((sh.watched_episode_count / sh.episode_count) * 100)),
    label: t('common.watchedProgress', {
      watched: sh.watched_episode_count,
      total: sh.episode_count,
    }),
  }
})

const primaryActionLabel = computed(() => {
  if (mode.value === 'movie') {
    const p = movieDetail.value?.video.progress
    return p && !p.completed && p.position_secs > 3 ? t('common.resume') : t('common.play')
  }
  const ep = showDetail.value?.continue_episode
  if (ep && ep.season_number != null && ep.episode_number != null) {
    return t('catalog.continueEpisodeBtn', {
      s: String(ep.season_number).padStart(2, '0'),
      e: String(ep.episode_number).padStart(2, '0'),
    })
  }
  return primaryPlayEpisode.value ? t('catalog.startFirstEpisodeBtn') : ''
})

const primaryActionTo = computed<RouteLocationRaw | undefined>(() => {
  if (mode.value === 'movie') {
    const v = movieDetail.value?.video
    return v ? { path: `/video/play/${encodeURIComponent(v.id)}` } : undefined
  }
  return primaryPlayEpisode.value
    ? { path: `/video/play/${encodeURIComponent(primaryPlayEpisode.value.id)}` }
    : undefined
})

const isFavorite = computed(() =>
  mode.value === 'movie'
    ? Boolean(movieDetail.value?.video.is_favorite)
    : Boolean(showDetail.value?.show.is_favorite),
)

const displayDescription = computed(() =>
  mode.value === 'movie'
    ? movieDetail.value?.video.summary ?? null
    : showDetail.value?.show.summary ?? null,
)

const displayTags = computed<MediaDetailTag[]>(() => {
  const genres =
    mode.value === 'movie'
      ? (movieDetail.value?.video.genres ?? [])
      : (showDetail.value?.show.genres ?? [])
  const tags =
    mode.value === 'movie'
      ? (movieDetail.value?.video.tags ?? [])
      : (showDetail.value?.show.tags ?? [])

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
  if (mode.value !== 'movie' || !movieDetail.value) return []
  const d = movieDetail.value
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
          [a.title || a.language?.toUpperCase(), a.codec.toUpperCase(), a.channels ? `${a.channels}ch` : '']
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
  if (authStore.isAdmin && d.path) {
    specs.push({ label: t('detail.filePath'), value: d.path, mono: true })
  }
  return specs
})

// Actions
async function handleToggleFavorite(): Promise<void> {
  if (favoriteBusy.value) return
  favoriteBusy.value = true
  try {
    if (mode.value === 'movie' && movieDetail.value) {
      await videoStore.toggleVideoFavorite(movieDetail.value.video)
    } else if (mode.value === 'show' && showDetail.value) {
      await videoStore.toggleShowFavorite(showDetail.value.show)
    }
  } catch (err) {
    snackbar.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  } finally {
    favoriteBusy.value = false
  }
}

async function handleToggleVideoWatched(video: VideoSummaryDto, close?: () => void): Promise<void> {
  close?.()
  try {
    const nextWatched = !(video.progress?.completed ?? false)
    await api.setWatched(video.id, nextWatched)
    videoStore.bumpCatalogVersion()
    await loadItem()
  } catch (err) {
    snackbar.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  }
}

function triggerVideoDownload(videoId: string, close?: () => void): void {
  close?.()
  const a = document.createElement('a')
  a.href = buildVideoDownloadUrl(videoId)
  a.download = ''
  a.rel = 'noopener'
  document.body.appendChild(a)
  a.click()
  a.remove()
}

async function handleDeleteVideo(video: VideoSummaryDto, close?: () => void): Promise<void> {
  close?.()
  const ok = await dialogStore.confirm({
    title: t('common.delete'),
    message: t('metadata.deleteVideoConfirm', { title: video.title }),
    danger: true,
  })
  if (!ok) return
  try {
    await api.deleteVideo(video.id)
    await videoStore.refreshSidebar(true)
    videoStore.bumpCatalogVersion()
    if (mode.value === 'movie') {
      await router.replace(libraryRoute('video'))
    } else {
      await loadItem()
    }
  } catch (err) {
    snackbar.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  }
}

function formatEpisodeCode(ep: VideoSummaryDto): string {
  if (ep.season_number != null && ep.episode_number != null) {
    return `S${String(ep.season_number).padStart(2, '0')}E${String(ep.episode_number).padStart(2, '0')}`
  }
  if (ep.episode_number != null) {
    return `E${String(ep.episode_number).padStart(2, '0')}`
  }
  return ''
}

function episodeProgressPercent(ep: VideoSummaryDto): number {
  const p = ep.progress
  if (!p) return 0
  if (p.completed) return 100
  const dur = Math.max(1, ep.duration_secs || p.duration_secs || 1)
  return Math.min(100, Math.round((p.position_secs / dur) * 100))
}

onMounted(loadItem)
watch([itemId, isMovieQuery], () => {
  void loadItem()
})
</script>

<template>
  <div class="px-4 py-6 sm:px-6 lg:px-8">
    <MediaDetailTemplate
      :title="displayTitle"
      :subtitle="displaySubtitle"
      :cover="displayCover"
      :aspect="mode === 'movie' ? 'video' : 'poster'"
      cover-style="plain"
      :fallback-icon="mode === 'movie' ? LuFilm : LuTv"
      :back-label="t('common.back')"
      :progress-percent="displayProgress.percent"
      :progress-label="displayProgress.label"
      :primary-action-label="primaryActionLabel"
      :primary-action-icon="LuPlay"
      :primary-action-to="primaryActionTo"
      :secondary-action-label="mode === 'movie' && movieDetail ? t('common.addToShelf') : ''"
      :secondary-action-icon="LuFolderPlus"
      :show-favorite="Boolean(showDetail || movieDetail)"
      :favorite="isFavorite"
      :favorite-loading="favoriteBusy"
      :description="displayDescription"
      :tags="displayTags"
      :specs="movieSpecs"
      :specs-title="t('detail.specsTitle')"
      :tabs="seasonTabs"
      :active-tab="selectedSeason"
      :loading="loading"
      :error="errorMessage"
      @back="handleBack"
      @secondary-action="
        movieDetail &&
          (shelfPickerTarget = { id: movieDetail.video.id, title: movieDetail.video.title })
      "
      @toggle-favorite="handleToggleFavorite"
      @update:active-tab="selectedSeason = $event"
      @retry="loadItem"
    >
      <template #overflow-menu="{ close }">
        <template v-if="mode === 'movie' && movieDetail">
          <MdMenuItem
            :icon="LuCheck"
            :label="
              movieDetail.video.progress?.completed
                ? t('common.markUnwatched')
                : t('common.markWatched')
            "
            @select="handleToggleVideoWatched(movieDetail.video, close)"
          />
          <MdMenuItem
            :icon="LuDownload"
            :label="t('common.download')"
            @select="triggerVideoDownload(movieDetail.video.id, close)"
          />
          <MdMenuItem
            v-if="authStore.isAdmin"
            :icon="LuPencil"
            :label="t('common.editMetadata')"
            @select="
              close();
              editingVideo = movieDetail.video
            "
          />
        </template>

        <template v-else-if="mode === 'show' && showDetail">
          <MdMenuItem
            v-if="authStore.isAdmin"
            :icon="LuPencil"
            :label="t('common.editMetadata')"
            @select="
              close();
              editShowOpen = true
            "
          />
        </template>

        <MdMenuItem
          :icon="LuRefreshCw"
          :label="t('common.refresh')"
          @select="
            close();
            loadItem()
          "
        />
      </template>

      <template v-if="mode === 'movie' && movieDetail && authStore.isAdmin" #overflow-danger="{ close }">
        <MdMenuItem
          :icon="LuTrash2"
          :label="t('common.delete')"
          destructive
          @select="handleDeleteVideo(movieDetail.video, close)"
        />
      </template>

      <!-- Show Mode: Lightweight Episode List (replaces redundant 4-col VideoCard wall) -->
      <template v-if="mode === 'show' && showDetail">
        <div v-if="activeSeasonGroup && activeSeasonGroup.episodes.length > 0" class="space-y-2">
          <div
            v-for="ep in activeSeasonGroup.episodes"
            :key="ep.id"
            class="group relative flex items-center gap-3.5 rounded-corner-md bg-surface-container-low p-2.5 transition-colors duration-150 ease-standard hover:bg-surface-container"
          >
            <RouterLink
              :to="`/video/play/${encodeURIComponent(ep.id)}`"
              class="flex min-w-0 flex-1 items-center gap-3.5 outline-none focus-visible:outline-3 focus-visible:outline-offset-2 focus-visible:outline-secondary"
            >
              <div class="relative w-32 shrink-0 overflow-hidden rounded-corner-sm bg-surface-container-high sm:w-40">
                <MediaCoverImage
                  :src="buildPosterUrl(ep.poster_key, 360)"
                  :alt="ep.title"
                  aspect-class="aspect-video"
                  rounded="rounded-corner-sm"
                  :fallback-icon="LuFilm"
                  fallback-icon-class="h-6 w-6 text-on-surface-variant/50"
                >
                  <span
                    v-if="ep.duration_secs > 0"
                    class="text-label-small pointer-events-none absolute right-1.5 bottom-1.5 rounded-corner-xs bg-scrim/75 px-1.5 py-0.5 tabular-nums text-white"
                  >
                    {{ formatDuration(ep.duration_secs) }}
                  </span>
                  <div
                    v-if="episodeProgressPercent(ep) > 0"
                    class="pointer-events-none absolute right-0 bottom-0 left-0"
                  >
                    <MdLinearProgress
                      :value="episodeProgressPercent(ep)"
                      :aria-label="`${ep.title} ${episodeProgressPercent(ep)}%`"
                    />
                  </div>
                </MediaCoverImage>
              </div>

              <div class="min-w-0 flex-1 space-y-1">
                <div class="flex items-center gap-2">
                  <span
                    v-if="formatEpisodeCode(ep)"
                    class="text-label-small shrink-0 rounded-corner-xs bg-surface-container-highest px-1.5 py-0.5 font-mono text-on-surface-variant"
                  >
                    {{ formatEpisodeCode(ep) }}
                  </span>
                  <h3 class="text-title-small truncate text-on-surface">
                    {{ ep.title }}
                  </h3>
                  <LuCheck
                    v-if="ep.progress?.completed"
                    class="h-4 w-4 shrink-0 text-success"
                    :aria-label="t('common.watched')"
                  />
                </div>
                <p
                  v-if="ep.summary"
                  class="text-body-small line-clamp-2 text-on-surface-variant"
                >
                  {{ ep.summary }}
                </p>
                <p
                  v-else-if="ep.progress && !ep.progress.completed && ep.progress.position_secs > 0"
                  class="text-body-small tabular-nums text-on-surface-variant"
                >
                  {{ formatTime(ep.progress.position_secs) }} /
                  {{ formatTime(ep.progress.duration_secs || ep.duration_secs) }}
                </p>
              </div>
            </RouterLink>

            <div class="shrink-0">
              <MdMenu placement="bottom-end" :aria-label="t('common.moreActions')">
                <template #trigger="{ toggle, triggerAttrs }">
                  <MdIconButton
                    v-bind="triggerAttrs"
                    variant="standard"
                    size="sm"
                    :icon="LuMoreVertical"
                    :aria-label="t('common.moreActions')"
                    @click="toggle"
                  />
                </template>
                <template #default="{ close }">
                  <MdMenuItem
                    :icon="LuCheck"
                    :label="ep.progress?.completed ? t('common.markUnwatched') : t('common.markWatched')"
                    @select="handleToggleVideoWatched(ep, close)"
                  />
                  <MdMenuItem
                    :icon="LuFolderPlus"
                    :label="t('common.addToShelf')"
                    @select="
                      close();
                      shelfPickerTarget = { id: ep.id, title: ep.title }
                    "
                  />
                  <MdMenuItem
                    v-if="authStore.isAdmin"
                    :icon="LuPencil"
                    :label="t('common.editMetadata')"
                    @select="
                      close();
                      editingVideo = ep
                    "
                  />
                  <MdMenuItem
                    :icon="LuDownload"
                    :label="t('common.download')"
                    @select="triggerVideoDownload(ep.id, close)"
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
                      @select="handleDeleteVideo(ep, close)"
                    />
                  </div>
                </template>
              </MdMenu>
            </div>
          </div>
        </div>

        <MdEmptyState
          v-else
          compact
          :icon="LuFilm"
          :title="t('common.noResults')"
        />
      </template>
    </MediaDetailTemplate>

    <ShowMetadataModal
      v-if="showDetail"
      :open="editShowOpen"
      :show="showDetail.show"
      @close="editShowOpen = false"
      @saved="loadItem"
    />

    <VideoMetadataModal
      :open="editingVideo !== null"
      :video="editingVideo"
      @close="editingVideo = null"
      @saved="loadItem"
    />

    <VideoShelfPickerDialog
      v-if="shelfPickerTarget"
      :open="shelfPickerTarget !== null"
      :video-id="shelfPickerTarget.id"
      :video-title="shelfPickerTarget.title"
      @close="shelfPickerTarget = null"
    />
  </div>
</template>
