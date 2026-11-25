<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import {
  LuArrowUpDown,
  LuFilm,
  LuLibrary,
  LuPlay,
  LuRefreshCw,
} from 'vue-icons-plus/lu'
import {
  MediaDetailTemplate,
  type MediaDetailSpec,
} from '@/components/domain'
import {
  MdButton,
  MdChip,
  MdEmptyState,
  MdMenuItem,
  MdTextField,
  type MdTabItem,
} from '@/components/ui'
import { libraryRoute } from '@/sources'
import { useSnackbarStore } from '@/stores/snackbar'
import {
  buildOnlineVideoPlayRoute,
  hydrateOnlineVideoDetailContext,
  writeStoredDetailContext,
  writeStoredPlayerContext,
} from '../composables/useOnlineVideoContext'
import { useVideoI18n as useI18n } from '../i18n'
import {
  onlineVideoApi,
  resolveOnlineVideoPosterUrl,
  type OnlineVideoContextStorage,
  type OnlineVideoDetailResponseDto,
  type OnlineVideoEpisode,
  type OnlineVideoPlayGroup,
} from '../onlineApi'
import { useVideoStore } from '../store'

const { t } = useI18n()
const route = useRoute()
const router = useRouter()
const videoStore = useVideoStore()
const snackbar = useSnackbarStore()

const loading = ref(true)
const trackingBusy = ref(false)
const errorMessage = ref<string | null>(null)
const detail = ref<OnlineVideoDetailResponseDto | null>(null)
const resolvedSourceId = ref('')
const resolvedSourceName = ref('')
const resolvedVideoId = ref('')
const resolvedDetailUrl = ref('')
const fallbackCtx = ref<OnlineVideoContextStorage | null>(null)

const selectedGroupName = ref('')
const orderAsc = ref(true)
const episodeQuery = ref('')

let loadSeq = 0
let unmounted = false
let activeAbort: AbortController | null = null

const displayTitle = computed(
  () =>
    detail.value?.title ||
    fallbackCtx.value?.title ||
    resolvedVideoId.value ||
    resolvedDetailUrl.value ||
    'Video',
)

const displayPosterUrl = computed(() =>
  resolveOnlineVideoPosterUrl(
    detail.value?.proxy_cover_url ?? fallbackCtx.value?.proxyCoverUrl,
    detail.value?.cover_url ?? fallbackCtx.value?.coverUrl,
    resolvedSourceId.value,
  ),
)

const displayRemarks = computed(() => detail.value?.remarks || fallbackCtx.value?.remarks || '')
const displayYear = computed(() => detail.value?.year || fallbackCtx.value?.year || '')
const displayArea = computed(() => detail.value?.area || fallbackCtx.value?.area || '')

const displaySubtitle = computed(() =>
  [displayYear.value, displayArea.value, displayRemarks.value, resolvedSourceName.value]
    .filter(Boolean)
    .join(' · '),
)

const displayDescription = computed(() => detail.value?.intro?.trim() || null)

const playGroups = computed<OnlineVideoPlayGroup[]>(() => detail.value?.play_groups ?? [])

const totalEpisodesCount = computed(() =>
  playGroups.value.reduce((acc, grp) => acc + (grp.episodes?.length ?? 0), 0),
)

const groupTabs = computed<MdTabItem<string>[]>(() =>
  playGroups.value.map((grp) => ({
    value: grp.name,
    label: grp.name,
    badge: grp.episodes?.length ?? 0,
  })),
)

const activeGroup = computed<OnlineVideoPlayGroup | null>(() => {
  if (playGroups.value.length === 0) return null
  return (
    playGroups.value.find((g) => g.name === selectedGroupName.value) ??
    playGroups.value[0] ??
    null
  )
})

const activeEpisodes = computed<OnlineVideoEpisode[]>(() => activeGroup.value?.episodes ?? [])

const EPISODE_CHUNK_SIZE = 50
const selectedEpisodeChunk = ref(0)

const displayedEpisodes = computed<OnlineVideoEpisode[]>(() => {
  const q = episodeQuery.value.trim().toLowerCase()
  const base = orderAsc.value ? [...activeEpisodes.value] : [...activeEpisodes.value].reverse()
  if (!q) return base
  return base.filter((ep) => ep.name.toLowerCase().includes(q))
})

const episodeChunks = computed<Array<{ index: number; label: string; items: OnlineVideoEpisode[] }>>(() => {
  const list = displayedEpisodes.value
  if (list.length <= EPISODE_CHUNK_SIZE) {
    return [{ index: 0, label: '', items: list }]
  }
  const chunks: Array<{ index: number; label: string; items: OnlineVideoEpisode[] }> = []
  for (let start = 0; start < list.length; start += EPISODE_CHUNK_SIZE) {
    const end = Math.min(list.length, start + EPISODE_CHUNK_SIZE)
    chunks.push({
      index: chunks.length,
      label: `${start + 1}–${end}`,
      items: list.slice(start, end),
    })
  }
  return chunks
})

const visibleEpisodes = computed<OnlineVideoEpisode[]>(() => {
  const chunk = episodeChunks.value[selectedEpisodeChunk.value] ?? episodeChunks.value[0]
  return chunk?.items ?? []
})

watch([selectedGroupName, orderAsc, episodeQuery], () => {
  selectedEpisodeChunk.value = 0
})

const primaryPlayTarget = computed<{
  groupName: string
  episode: OnlineVideoEpisode
  index: number
} | null>(() => {
  const grp = activeGroup.value
  if (grp && grp.episodes.length > 0) {
    return { groupName: grp.name, episode: grp.episodes[0]!, index: 0 }
  }
  for (const g of playGroups.value) {
    if (g.episodes.length > 0) {
      return { groupName: g.name, episode: g.episodes[0]!, index: 0 }
    }
  }
  return null
})

const detailSpecs = computed<MediaDetailSpec[]>(() => {
  const specs: MediaDetailSpec[] = []
  if (resolvedSourceName.value) {
    specs.push({ label: t('online.sourceLabel'), value: resolvedSourceName.value })
  }
  if (detail.value?.director) {
    specs.push({ label: t('online.directorLabel'), value: detail.value.director })
  }
  if (detail.value?.actors) {
    specs.push({ label: t('online.actorsLabel'), value: detail.value.actors })
  }
  if (displayYear.value) {
    specs.push({ label: t('online.yearLabel'), value: displayYear.value })
  }
  if (displayArea.value) {
    specs.push({ label: t('online.areaLabel'), value: displayArea.value })
  }
  if (displayRemarks.value) {
    specs.push({ label: t('online.playLinesHeader'), value: displayRemarks.value })
  }
  return specs
})

async function loadDetail(): Promise<void> {
  activeAbort?.abort()
  const controller = new AbortController()
  activeAbort = controller

  const hydrated = hydrateOnlineVideoDetailContext(route)
  fallbackCtx.value = hydrated.fallbackCtx
  resolvedSourceId.value = hydrated.sourceId
  resolvedSourceName.value = hydrated.sourceName
  resolvedVideoId.value = hydrated.videoId
  resolvedDetailUrl.value = hydrated.detailUrl

  const seq = ++loadSeq
  errorMessage.value = null

  if (!hydrated.sourceId || (!hydrated.videoId && !hydrated.detailUrl)) {
    detail.value = null
    loading.value = false
    errorMessage.value = t('online.missingDetailContext')
    return
  }

  loading.value = true
  try {
    const res = await onlineVideoApi.detail(
      hydrated.sourceId,
      {
        video_id: hydrated.videoId || null,
        detail_url: hydrated.detailUrl || null,
      },
      { signal: controller.signal },
    )
    if (unmounted || seq !== loadSeq) return

    detail.value = res
    resolvedSourceName.value = res.source_name || resolvedSourceName.value
    if (res.id) resolvedVideoId.value = res.id

    const groups = res.play_groups ?? []
    if (groups.length > 0 && !groups.some((g) => g.name === selectedGroupName.value)) {
      selectedGroupName.value = groups[0]!.name
    } else if (groups.length === 0) {
      selectedGroupName.value = ''
    }

    writeStoredDetailContext({
      sourceId: hydrated.sourceId,
      sourceName: resolvedSourceName.value,
      videoId: res.id || hydrated.videoId || undefined,
      detailUrl: hydrated.detailUrl || undefined,
      title: res.title || fallbackCtx.value?.title,
      coverUrl: res.cover_url ?? fallbackCtx.value?.coverUrl ?? null,
      proxyCoverUrl: res.proxy_cover_url ?? fallbackCtx.value?.proxyCoverUrl ?? null,
      remarks: res.remarks ?? fallbackCtx.value?.remarks ?? null,
      year: res.year ?? fallbackCtx.value?.year ?? null,
      area: res.area ?? fallbackCtx.value?.area ?? null,
      typeName: fallbackCtx.value?.typeName ?? null,
      returnKeyword: fallbackCtx.value?.returnKeyword,
      returnSourceId: fallbackCtx.value?.returnSourceId,
    })
  } catch (err) {
    if (unmounted || seq !== loadSeq) return
    if (err instanceof Error && err.name === 'AbortError') return
    errorMessage.value = err instanceof Error ? err.message : t('online.detailLoadFailed')
  } finally {
    if (!unmounted && seq === loadSeq) {
      loading.value = false
    }
  }
}

function openEpisode(ep: OnlineVideoEpisode, groupNameOverride?: string): void {
  if (!ep.play_url?.trim() || !resolvedSourceId.value) return
  const groupName = groupNameOverride || activeGroup.value?.name || ''
  const targetGroup = playGroups.value.find((g) => g.name === groupName) ?? activeGroup.value
  const list = targetGroup?.episodes ?? []
  const idx = Math.max(
    0,
    list.findIndex((item) => item.play_url === ep.play_url && item.index === ep.index),
  )

  writeStoredPlayerContext({
    sourceId: resolvedSourceId.value,
    sourceName: resolvedSourceName.value || undefined,
    videoId: resolvedVideoId.value || undefined,
    detailUrl: resolvedDetailUrl.value || undefined,
    title: displayTitle.value,
    coverUrl: detail.value?.cover_url ?? fallbackCtx.value?.coverUrl ?? null,
    proxyCoverUrl: detail.value?.proxy_cover_url ?? fallbackCtx.value?.proxyCoverUrl ?? null,
    groupName,
    episodeIndex: idx,
    episodeName: ep.name,
    playUrl: ep.play_url,
    playGroups: playGroups.value,
  })

  void router.push(
    buildOnlineVideoPlayRoute({
      sourceId: resolvedSourceId.value,
      detailUrl: resolvedDetailUrl.value,
      videoId: resolvedVideoId.value,
      playUrl: ep.play_url,
      groupName,
      episodeIndex: idx,
      title: displayTitle.value,
      cover: detail.value?.cover_url ?? fallbackCtx.value?.coverUrl ?? null,
      sourceName: resolvedSourceName.value,
    }),
  )
}

function handlePrimaryPlay(): void {
  if (!primaryPlayTarget.value) return
  openEpisode(primaryPlayTarget.value.episode, primaryPlayTarget.value.groupName)
}

function handleBack(): void {
  if (window.history.state?.back) {
    router.back()
    return
  }
  void router.push(
    libraryRoute('video', {
      scope: 'online',
      q: fallbackCtx.value?.returnKeyword || undefined,
      sourceId: fallbackCtx.value?.returnSourceId || undefined,
    }),
  )
}

async function handleTrackToLibrary(): Promise<void> {
  if (
    trackingBusy.value ||
    !resolvedSourceId.value ||
    (!resolvedVideoId.value && !resolvedDetailUrl.value)
  ) {
    return
  }
  trackingBusy.value = true
  try {
    const res = await onlineVideoApi.trackOnlineVideo({
      source_id: resolvedSourceId.value,
      video_id: resolvedVideoId.value || null,
      detail_url: resolvedDetailUrl.value || null,
      group_name: activeGroup.value?.name || null,
    })
    videoStore.bumpCatalogVersion()
    void videoStore.refreshSidebar(true)
    snackbar.show({ message: t('online.trackedToLibraryToast') })
    if (res.kind === 'show' && res.show) {
      await router.push(`/video/item/${encodeURIComponent(res.show.show.id)}`)
    } else if (res.video) {
      await router.push({
        path: `/video/item/${encodeURIComponent(res.video.id)}`,
        query: { kind: 'movie' },
      })
    }
  } catch (err) {
    snackbar.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  } finally {
    trackingBusy.value = false
  }
}

onMounted(() => {
  unmounted = false
  void loadDetail()
})

onBeforeUnmount(() => {
  unmounted = true
  loadSeq += 1
  activeAbort?.abort()
})

watch(
  () => [
    route.query.source,
    route.query.sourceId,
    route.query.source_id,
    route.query.ref,
    route.query.vid,
    route.query.videoId,
    route.query.video_id,
    route.query.detailUrl,
    route.query.detail_url,
  ],
  () => {
    void loadDetail()
  },
)
</script>

<template>
  <div class="px-4 py-6 sm:px-6 lg:px-8">
    <MediaDetailTemplate
      :title="displayTitle"
      :subtitle="displaySubtitle"
      :cover="displayPosterUrl || null"
      aspect="poster"
      cover-style="plain"
      :fallback-icon="LuFilm"
      :back-label="t('online.backToSearch')"
      :primary-action-label="t('online.playNow')"
      :primary-action-icon="LuPlay"
      :primary-action-disabled="!primaryPlayTarget"
      :secondary-action-label="
        detail ? (trackingBusy ? t('online.trackingBtn') : t('online.trackToLibraryBtn')) : ''
      "
      :secondary-action-icon="LuLibrary"
      :secondary-action-disabled="trackingBusy"
      :show-favorite="false"
      :description="displayDescription"
      :specs="detailSpecs"
      :specs-title="t('detail.castAndCrew')"
      :tabs="groupTabs"
      :active-tab="selectedGroupName"
      :loading="loading && !detail"
      :error="errorMessage"
      @back="handleBack"
      @primary-action="handlePrimaryPlay"
      @secondary-action="handleTrackToLibrary"
      @update:active-tab="selectedGroupName = $event"
      @retry="loadDetail"
    >
      <template #overflow-menu="{ close }">
        <MdMenuItem
          :icon="LuRefreshCw"
          :label="t('common.refresh')"
          @select="
            close();
            loadDetail()
          "
        />
      </template>

      <!-- Episodes Section -->
      <section class="space-y-4">
        <div class="flex flex-wrap items-center justify-between gap-3">
          <div class="flex items-baseline gap-2.5">
            <h2 class="text-title-large text-on-surface">
              {{ t('online.episodesHeader') }}
            </h2>
            <span class="text-body-small tabular-nums text-on-surface-variant">
              {{ t('online.episodesCount', { count: activeEpisodes.length }) }}
            </span>
          </div>

          <div v-if="activeEpisodes.length > 0" class="flex flex-wrap items-center gap-2">
            <div v-if="activeEpisodes.length > 12" class="w-48">
              <MdTextField
                v-model="episodeQuery"
                type="search"
                size="sm"
                clearable
                :placeholder="t('online.filterEpisodesPlaceholder')"
              />
            </div>

            <MdButton
              v-if="activeEpisodes.length > 1"
              variant="outlined"
              size="sm"
              :icon="LuArrowUpDown"
              @click="orderAsc = !orderAsc"
            >
              {{ orderAsc ? t('online.orderAsc') : t('online.orderDesc') }}
            </MdButton>
          </div>
        </div>

        <MdEmptyState
          v-if="!loading && totalEpisodesCount === 0"
          compact
          :icon="LuFilm"
          :title="t('online.emptyPlayGroups')"
        />

        <template v-else>
          <div v-if="episodeChunks.length > 1" class="flex flex-wrap items-center gap-2">
            <MdChip
              v-for="chunk in episodeChunks"
              :key="chunk.index"
              variant="filter"
              :label="chunk.label"
              :selected="selectedEpisodeChunk === chunk.index"
              hide-check
              @click="selectedEpisodeChunk = chunk.index"
            />
          </div>

          <div
            class="grid grid-cols-2 gap-2 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-6"
          >
            <button
              v-for="ep in visibleEpisodes"
              :key="`${selectedGroupName}-${ep.index}-${ep.play_url}`"
              type="button"
              :disabled="!ep.play_url"
              class="md-state text-label-large flex min-h-12 cursor-pointer items-center justify-center truncate rounded-corner-md bg-surface-container-low px-3 text-on-surface transition-colors duration-150 ease-standard hover:bg-surface-container-high disabled:cursor-not-allowed disabled:opacity-38"
              @click="openEpisode(ep)"
            >
              <span class="truncate">{{ ep.name }}</span>
            </button>
          </div>
        </template>
      </section>
    </MediaDetailTemplate>
  </div>
</template>
