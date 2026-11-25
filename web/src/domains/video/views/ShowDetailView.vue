<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useI18n as useGlobalI18n } from 'vue-i18n'
import { useRoute, useRouter } from 'vue-router'
import { libraryRoute } from '@/sources'
import { useAuthStore } from '@/stores/auth'
import { useDialogStore } from '@/stores/dialog'
import { useSnackbarStore } from '@/stores/snackbar'
import { api, buildVideoDownloadUrl } from '../api'
import MovieDetailSection from '../components/detail/MovieDetailSection.vue'
import TvShowDetailSection from '../components/detail/TvShowDetailSection.vue'
import ShowMetadataModal from '../components/ShowMetadataModal.vue'
import VideoMetadataModal from '../components/VideoMetadataModal.vue'
import VideoShelfPickerDialog from '../components/VideoShelfPickerDialog.vue'
import { useVideoI18n as useI18n } from '../i18n'
import { onlineVideoApi } from '../onlineApi'
import { useVideoStore } from '../store'
import type { ShowDetailDto, VideoDetailDto, VideoSummaryDto } from '../types'

const { t } = useI18n()
const { t: globalT } = useGlobalI18n()
const route = useRoute()
const router = useRouter()
const authStore = useAuthStore()
const videoStore = useVideoStore()
const dialogStore = useDialogStore()
const snackbar = useSnackbarStore()

const isMovieQuery = computed(() => route.query.kind === 'movie')
const mode = ref<'show' | 'movie'>(isMovieQuery.value ? 'movie' : 'show')
const showDetail = ref<ShowDetailDto | null>(null)
const movieDetail = ref<VideoDetailDto | null>(null)
const selectedSeason = ref<string>('')
const loading = ref(true)
const errorMessage = ref<string | null>(null)
const favoriteBusy = ref(false)

const editShowOpen = ref(false)
const editingVideo = ref<VideoSummaryDto | null>(null)
const shelfPickerTarget = ref<{ id: string; title: string } | null>(null)
const refreshingTrackedShow = ref(false)
const cachingShow = ref(false)
const cachingVideoIds = ref<Set<string>>(new Set())

let loadSeq = 0

const itemId = computed(() => String(route.params.id || '').trim())

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
      mode.value = 'movie'
      const res = await api.getVideo(id)
      if (seq !== loadSeq) return
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
      mode.value === 'show' ? libraryRoute('video', { view: 'shows' }) : libraryRoute('video'),
    )
  }
}

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

async function handleToggleVideoWatched(video: VideoSummaryDto): Promise<void> {
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

function triggerVideoDownload(videoId: string): void {
  const a = document.createElement('a')
  a.href = buildVideoDownloadUrl(videoId)
  a.download = ''
  a.rel = 'noopener'
  document.body.appendChild(a)
  a.click()
  a.remove()
}

async function handleUnbindCurrent(): Promise<void> {
  const targetId =
    mode.value === 'movie' ? movieDetail.value?.video.id : showDetail.value?.show.id
  const title =
    mode.value === 'movie'
      ? (movieDetail.value?.video.title ?? '')
      : (showDetail.value?.show.title ?? '')
  if (!targetId) return
  const isOnline =
    mode.value === 'movie'
      ? Boolean(movieDetail.value?.video.online_tracking)
      : Boolean(showDetail.value?.show.online_tracking)
  const ok = await dialogStore.confirm({
    title: globalT('pool.unbindConfirmTitle'),
    message: isOnline
      ? globalT('pool.unbindOnlineConfirmMessage', { title })
      : globalT('pool.unbindConfirmMessage', { title }),
    confirmText: globalT('pool.unbindBtn'),
    danger: true,
  })
  if (!ok) return
  try {
    await api.unbindItem(targetId)
    await videoStore.refreshSidebar(true)
    videoStore.bumpCatalogVersion()
    snackbar.show({ message: globalT('pool.unboundToast') })
    await router.replace(libraryRoute('video'))
  } catch (err) {
    snackbar.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  }
}

async function handleDeleteVideo(video: VideoSummaryDto): Promise<void> {
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

async function handleRefreshTrackedShow(): Promise<void> {
  if (!showDetail.value || refreshingTrackedShow.value) return
  refreshingTrackedShow.value = true
  try {
    const updated = await onlineVideoApi.refreshTrackedShow(showDetail.value.show.id)
    showDetail.value = updated
    if (
      updated.seasons.length > 0 &&
      !updated.seasons.some((s) => String(s.season_number) === selectedSeason.value)
    ) {
      selectedSeason.value = String(updated.seasons[0]!.season_number)
    }
    videoStore.bumpCatalogVersion()
    void videoStore.refreshSidebar(true)
    snackbar.show({
      message: t('online.trackedShowRefreshed', { count: updated.show.episode_count }),
    })
  } catch (err) {
    snackbar.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  } finally {
    refreshingTrackedShow.value = false
  }
}

async function handleCacheTrackedShow(): Promise<void> {
  if (!showDetail.value || cachingShow.value) return
  cachingShow.value = true
  try {
    const report = await onlineVideoApi.cacheTrackedShow(showDetail.value.show.id)
    await loadItem()
    videoStore.bumpCatalogVersion()
    if (report.failed_count > 0) {
      snackbar.show({
        message: t('online.cacheShowPartialToast', {
          cached: report.cached_count,
          failed: report.failed_count,
        }),
        tone: 'error',
      })
    } else {
      snackbar.show({
        message: t('online.cacheShowReportToast', {
          cached: report.cached_count,
          skipped: report.skipped_count,
        }),
      })
    }
  } catch (err) {
    snackbar.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  } finally {
    cachingShow.value = false
  }
}

async function handleCacheTrackedVideo(video: VideoSummaryDto): Promise<void> {
  if (cachingVideoIds.value.has(video.id)) return
  const next = new Set(cachingVideoIds.value)
  next.add(video.id)
  cachingVideoIds.value = next
  snackbar.show({ message: t('online.cachingVideoToast', { title: video.title }) })
  try {
    await onlineVideoApi.cacheTrackedVideo(video.id)
    await loadItem()
    videoStore.bumpCatalogVersion()
    snackbar.show({ message: t('online.videoCachedToast', { title: video.title }) })
  } catch (err) {
    snackbar.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  } finally {
    const done = new Set(cachingVideoIds.value)
    done.delete(video.id)
    cachingVideoIds.value = done
  }
}

onMounted(loadItem)
watch([itemId, isMovieQuery], () => {
  void loadItem()
})
</script>

<template>
  <div class="px-4 py-6 sm:px-6 lg:px-8">
    <MovieDetailSection
      v-if="mode === 'movie'"
      :detail="movieDetail"
      :loading="loading"
      :error="errorMessage"
      :favorite-busy="favoriteBusy"
      :is-admin="authStore.isAdmin"
      :caching-video-ids="cachingVideoIds"
      @back="handleBack"
      @retry="loadItem"
      @toggle-favorite="handleToggleFavorite"
      @open-shelf-picker="shelfPickerTarget = $event"
      @cache-video="handleCacheTrackedVideo"
      @toggle-watched="handleToggleVideoWatched"
      @download-video="triggerVideoDownload"
      @edit-video="editingVideo = $event"
      @refresh="loadItem"
      @unbind="handleUnbindCurrent"
      @delete-video="handleDeleteVideo"
    />

    <TvShowDetailSection
      v-else
      :detail="showDetail"
      :selected-season="selectedSeason"
      :loading="loading"
      :error="errorMessage"
      :favorite-busy="favoriteBusy"
      :is-admin="authStore.isAdmin"
      :refreshing-tracked-show="refreshingTrackedShow"
      :caching-show="cachingShow"
      :caching-video-ids="cachingVideoIds"
      @back="handleBack"
      @retry="loadItem"
      @update:selected-season="selectedSeason = $event"
      @toggle-favorite="handleToggleFavorite"
      @open-shelf-picker="shelfPickerTarget = $event"
      @refresh-tracked-show="handleRefreshTrackedShow"
      @cache-tracked-show="handleCacheTrackedShow"
      @cache-video="handleCacheTrackedVideo"
      @toggle-watched="handleToggleVideoWatched"
      @download-video="triggerVideoDownload"
      @edit-show="editShowOpen = true"
      @edit-video="editingVideo = $event"
      @refresh="loadItem"
      @unbind="handleUnbindCurrent"
      @delete-video="handleDeleteVideo"
    />

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
