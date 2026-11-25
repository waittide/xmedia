<script setup lang="ts">
import { computed } from 'vue'
import { useI18n as useGlobalI18n } from 'vue-i18n'
import { RouterLink, type RouteLocationRaw } from 'vue-router'
import {
  LuCheck,
  LuCloud,
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
import { MediaDetailTemplate, type MediaDetailTag } from '@/components/domain'
import {
  MdEmptyState,
  MdIconButton,
  MdLinearProgress,
  MdMenu,
  MdMenuItem,
  MediaCoverImage,
  type MdTabItem,
} from '@/components/ui'
import { buildPosterUrl } from '@/domains/video/api'
import { useVideoI18n as useI18n } from '@/domains/video/i18n'
import type { ShowDetailDto, VideoSummaryDto } from '@/domains/video/types'
import { libraryRoute } from '@/sources'
import { formatDuration, formatTime } from '@/utils/format'

const props = defineProps<{
  detail: ShowDetailDto | null
  selectedSeason: string
  loading: boolean
  error: string | null
  favoriteBusy: boolean
  isAdmin: boolean
  refreshingTrackedShow: boolean
  cachingShow: boolean
  cachingVideoIds: Set<string>
}>()

const emit = defineEmits<{
  back: []
  retry: []
  'update:selectedSeason': [season: string]
  'toggle-favorite': []
  'open-shelf-picker': [target: { id: string; title: string }]
  'refresh-tracked-show': []
  'cache-tracked-show': []
  'cache-video': [video: VideoSummaryDto]
  'toggle-watched': [video: VideoSummaryDto]
  'download-video': [videoId: string]
  'edit-show': []
  'edit-video': [video: VideoSummaryDto]
  refresh: []
  unbind: []
  'delete-video': [video: VideoSummaryDto]
}>()

const { t } = useI18n()
const { t: globalT } = useGlobalI18n()

const show = computed(() => props.detail?.show ?? null)

const seasonTabs = computed<MdTabItem<string>[]>(() => {
  if (!props.detail) return []
  return props.detail.seasons.map((sg) => ({
    value: String(sg.season_number),
    label:
      sg.season_number === 0
        ? t('catalog.specialsHeader')
        : t('catalog.seasonHeader', { season: sg.season_number }),
    badge: sg.episodes.length,
  }))
})

const activeSeasonGroup = computed(() => {
  if (!props.detail) return null
  return (
    props.detail.seasons.find((s) => String(s.season_number) === props.selectedSeason) ??
    props.detail.seasons[0] ??
    null
  )
})

const primaryPlayEpisode = computed<VideoSummaryDto | null>(() => {
  if (!props.detail) return null
  if (props.detail.continue_episode) return props.detail.continue_episode
  return props.detail.seasons[0]?.episodes[0] ?? null
})

const showTrackedStats = computed(() => {
  if (!props.detail) {
    return { isTracked: false, cached: 0, total: 0, uncached: 0 }
  }
  const allEps = props.detail.seasons.flatMap((sg) => sg.episodes)
  const trackedEps = allEps.filter((ep) => Boolean(ep.online_tracking))
  const isTracked = Boolean(props.detail.show.online_tracking || trackedEps.length > 0)
  const cached = trackedEps.filter((ep) => Boolean(ep.online_tracking?.cached)).length
  const total = trackedEps.length || allEps.length
  return {
    isTracked,
    cached,
    total,
    uncached: Math.max(0, trackedEps.length - cached),
  }
})

const displayCover = computed(() => buildPosterUrl(show.value?.poster_key, 540))

const displaySubtitle = computed(() => {
  const sh = show.value
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
  const sh = show.value
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
  const ep = props.detail?.continue_episode
  if (ep && ep.season_number != null && ep.episode_number != null) {
    return t('catalog.continueEpisodeBtn', {
      s: String(ep.season_number).padStart(2, '0'),
      e: String(ep.episode_number).padStart(2, '0'),
    })
  }
  return primaryPlayEpisode.value ? t('catalog.startFirstEpisodeBtn') : ''
})

const primaryActionTo = computed<RouteLocationRaw | undefined>(() =>
  primaryPlayEpisode.value
    ? { path: `/video/play/${encodeURIComponent(primaryPlayEpisode.value.id)}` }
    : undefined,
)

const displayTags = computed<MediaDetailTag[]>(() => {
  const genres = show.value?.genres ?? []
  const tags = show.value?.tags ?? []
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
</script>

<template>
  <MediaDetailTemplate
    :title="show?.title ?? ''"
    :subtitle="displaySubtitle"
    :cover="displayCover"
    aspect="poster"
    cover-style="plain"
    :fallback-icon="LuTv"
    :back-label="t('common.back')"
    :progress-percent="displayProgress.percent"
    :progress-label="displayProgress.label"
    :primary-action-label="primaryActionLabel"
    :primary-action-icon="LuPlay"
    :primary-action-to="primaryActionTo"
    :show-favorite="Boolean(show)"
    :favorite="Boolean(show?.is_favorite)"
    :favorite-loading="favoriteBusy"
    :description="show?.summary ?? null"
    :tags="displayTags"
    :specs="[]"
    :specs-title="t('detail.specsTitle')"
    :tabs="seasonTabs"
    :active-tab="selectedSeason"
    :loading="loading"
    :error="error"
    @back="emit('back')"
    @toggle-favorite="emit('toggle-favorite')"
    @update:active-tab="emit('update:selectedSeason', $event)"
    @retry="emit('retry')"
  >
    <template v-if="showTrackedStats.isTracked" #hero-meta>
      <div class="mt-1 flex flex-wrap items-center gap-2">
        <span
          class="text-label-medium inline-flex items-center gap-1.5 rounded-corner-full bg-secondary-container px-2.5 py-0.5 text-on-secondary-container"
        >
          <LuCloud class="h-3.5 w-3.5 shrink-0" aria-hidden="true" />
          <span>
            {{
              t('online.trackedShowBadgeStatus', {
                cached: showTrackedStats.cached,
                total: showTrackedStats.total,
              })
            }}
          </span>
        </span>
      </div>
    </template>

    <template v-if="detail" #overflow-menu="{ close }">
      <template v-if="showTrackedStats.isTracked">
        <MdMenuItem
          :icon="LuRefreshCw"
          :label="
            refreshingTrackedShow
              ? t('online.refreshingTrackedShow')
              : t('online.refreshTrackedShow')
          "
          :disabled="refreshingTrackedShow"
          @select="
            close();
            emit('refresh-tracked-show')
          "
        />
        <MdMenuItem
          v-if="showTrackedStats.uncached > 0"
          :icon="LuCloud"
          :label="cachingShow ? t('online.cachingAllEpisodes') : t('online.cacheAllEpisodes')"
          :disabled="cachingShow"
          @select="
            close();
            emit('cache-tracked-show')
          "
        />
      </template>
      <MdMenuItem
        v-if="isAdmin"
        :icon="LuPencil"
        :label="t('common.editMetadata')"
        @select="
          close();
          emit('edit-show')
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

    <template v-if="detail" #overflow-danger="{ close }">
      <MdMenuItem
        :icon="LuTrash2"
        :label="globalT('pool.unbindBtn')"
        destructive
        @select="
          close();
          emit('unbind')
        "
      />
    </template>

    <template v-if="detail">
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
                <span
                  v-if="ep.online_tracking"
                  class="text-label-small shrink-0 rounded-corner-xs px-1.5 py-0.5"
                  :class="
                    ep.online_tracking.cached
                      ? 'bg-secondary-container text-on-secondary-container'
                      : 'bg-tertiary-container text-on-tertiary-container'
                  "
                >
                  {{
                    ep.online_tracking.cached
                      ? t('online.badgeCached')
                      : t('online.badgeOnlineStream')
                  }}
                </span>
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
                  v-if="ep.online_tracking && !ep.online_tracking.cached"
                  :icon="LuCloud"
                  :label="t('online.cacheVideoBtn')"
                  :disabled="cachingVideoIds.has(ep.id)"
                  @select="
                    close();
                    emit('cache-video', ep)
                  "
                />
                <MdMenuItem
                  :icon="LuCheck"
                  :label="ep.progress?.completed ? t('common.markUnwatched') : t('common.markWatched')"
                  @select="
                    close();
                    emit('toggle-watched', ep)
                  "
                />
                <MdMenuItem
                  :icon="LuFolderPlus"
                  :label="t('common.addToShelf')"
                  @select="
                    close();
                    emit('open-shelf-picker', { id: ep.id, title: ep.title })
                  "
                />
                <MdMenuItem
                  v-if="isAdmin"
                  :icon="LuPencil"
                  :label="t('common.editMetadata')"
                  @select="
                    close();
                    emit('edit-video', ep)
                  "
                />
                <MdMenuItem
                  v-if="!ep.online_tracking || ep.online_tracking.cached"
                  :icon="LuDownload"
                  :label="t('common.download')"
                  @select="
                    close();
                    emit('download-video', ep.id)
                  "
                />
                <div
                  v-if="isAdmin"
                  class="my-1 border-t border-outline-variant pt-1"
                  role="separator"
                >
                  <MdMenuItem
                    :icon="LuTrash2"
                    :label="t('common.delete')"
                    destructive
                    @select="
                      close();
                      emit('delete-video', ep)
                    "
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
</template>
