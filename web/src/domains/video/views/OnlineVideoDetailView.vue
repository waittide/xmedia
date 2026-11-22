<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { LuArrowUpDown, LuChevronLeft, LuFilm, LuPlay, LuSearch } from 'vue-icons-plus/lu'
import MdTabs, { type MdTabItem } from '@/components/ui/MdTabs.vue'
import MediaCoverImage from '@/components/ui/MediaCoverImage.vue'
import RefreshButton from '@/components/ui/RefreshButton.vue'
import { useVideoI18n as useI18n } from '../i18n'
import {
  ONLINE_VIDEO_CTX_KEY,
  ONLINE_VIDEO_PLAYER_CTX_KEY,
  onlineVideoApi,
  resolveOnlineVideoPosterUrl,
  type OnlineVideoContextStorage,
  type OnlineVideoDetailResponseDto,
  type OnlineVideoEpisode,
  type OnlineVideoPlayGroup,
  type OnlineVideoPlayerContextStorage,
} from '../onlineApi'

const { t } = useI18n()
const route = useRoute()
const router = useRouter()

const loading = ref(true)
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

function readQueryParam(keys: string[]): string {
  for (const k of keys) {
    const val = route.query[k]
    if (typeof val === 'string' && val.trim()) {
      return val.trim()
    }
  }
  return ''
}

function readStoredContext(): OnlineVideoContextStorage | null {
  try {
    const raw = sessionStorage.getItem(ONLINE_VIDEO_CTX_KEY)
    if (!raw) return null
    const parsed: unknown = JSON.parse(raw)
    if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) return null
    return parsed as OnlineVideoContextStorage
  } catch {
    return null
  }
}

function hydrateParams(): { sourceId: string; videoId: string; detailUrl: string } {
  const qSourceId = readQueryParam(['sourceId', 'source_id'])
  const qVideoId = readQueryParam(['videoId', 'video_id'])
  const qDetailUrl = readQueryParam(['detailUrl', 'detail_url'])
  const qSourceName = readQueryParam(['sourceName', 'source_name'])
  const qTitle = readQueryParam(['title', 'name'])
  const qCoverUrl = readQueryParam(['coverUrl', 'cover_url'])
  const qRemarks = readQueryParam(['remarks'])
  const qYear = readQueryParam(['year'])
  const qArea = readQueryParam(['area'])

  const stored = readStoredContext()
  const hasQueryIdentity = Boolean(qSourceId && (qVideoId || qDetailUrl))
  const matchesStored =
    stored &&
    ((hasQueryIdentity &&
      stored.sourceId === qSourceId &&
      ((qVideoId && stored.videoId === qVideoId) ||
        (qDetailUrl && stored.detailUrl === qDetailUrl))) ||
      (!qSourceId && !qVideoId && !qDetailUrl && Boolean(stored.sourceId && (stored.videoId || stored.detailUrl))))

  const sourceId = qSourceId || (matchesStored ? stored.sourceId : '')
  const videoId = qVideoId || (matchesStored ? (stored.videoId ?? '') : '')
  const detailUrl = qDetailUrl || (matchesStored ? (stored.detailUrl ?? '') : '')

  fallbackCtx.value = {
    sourceId,
    sourceName: qSourceName || (matchesStored ? stored.sourceName : undefined),
    videoId: videoId || undefined,
    detailUrl: detailUrl || undefined,
    title: qTitle || (matchesStored ? stored.title : undefined),
    coverUrl: qCoverUrl || (matchesStored ? stored.coverUrl : undefined),
    proxyCoverUrl: matchesStored ? stored.proxyCoverUrl : undefined,
    remarks: qRemarks || (matchesStored ? stored.remarks : undefined),
    year: qYear || (matchesStored ? stored.year : undefined),
    area: qArea || (matchesStored ? stored.area : undefined),
    typeName: matchesStored ? stored.typeName : undefined,
    returnKeyword: matchesStored ? stored.returnKeyword : undefined,
    returnSourceId: matchesStored ? stored.returnSourceId : undefined,
  }

  resolvedSourceId.value = sourceId
  resolvedSourceName.value = fallbackCtx.value.sourceName || ''
  resolvedVideoId.value = videoId
  resolvedDetailUrl.value = detailUrl

  return { sourceId, videoId, detailUrl }
}

async function loadDetail(): Promise<void> {
  const { sourceId, videoId, detailUrl } = hydrateParams()
  const seq = ++loadSeq
  errorMessage.value = null

  if (!sourceId || (!videoId && !detailUrl)) {
    detail.value = null
    loading.value = false
    errorMessage.value = t('online.missingDetailContext')
    return
  }

  loading.value = true
  try {
    const res = await onlineVideoApi.detail(sourceId, {
      video_id: videoId || null,
      detail_url: detailUrl || null,
    })
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

    const nextStored: OnlineVideoContextStorage = {
      sourceId,
      sourceName: resolvedSourceName.value,
      videoId: res.id || videoId || undefined,
      detailUrl: detailUrl || undefined,
      title: res.title || fallbackCtx.value?.title,
      coverUrl: res.cover_url ?? fallbackCtx.value?.coverUrl ?? null,
      proxyCoverUrl: res.proxy_cover_url ?? fallbackCtx.value?.proxyCoverUrl ?? null,
      remarks: res.remarks ?? fallbackCtx.value?.remarks ?? null,
      year: res.year ?? fallbackCtx.value?.year ?? null,
      area: res.area ?? fallbackCtx.value?.area ?? null,
      typeName: fallbackCtx.value?.typeName ?? null,
      returnKeyword: fallbackCtx.value?.returnKeyword,
      returnSourceId: fallbackCtx.value?.returnSourceId,
    }
    try {
      sessionStorage.setItem(ONLINE_VIDEO_CTX_KEY, JSON.stringify(nextStored))
    } catch {
      // ignore storage errors
    }
  } catch (err) {
    if (unmounted || seq !== loadSeq) return
    errorMessage.value = err instanceof Error ? err.message : String(err)
  } finally {
    if (!unmounted && seq === loadSeq) {
      loading.value = false
    }
  }
}

onMounted(() => {
  unmounted = false
  void loadDetail()
})

onBeforeUnmount(() => {
  unmounted = true
  loadSeq += 1
})

watch(
  () => [
    route.query.sourceId,
    route.query.source_id,
    route.query.videoId,
    route.query.video_id,
    route.query.detailUrl,
    route.query.detail_url,
  ],
  () => {
    void loadDetail()
  },
)

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

const displaySummary = computed(() => detail.value?.intro?.trim() || t('online.summaryEmpty'))
const displayRemarks = computed(() => detail.value?.remarks || fallbackCtx.value?.remarks || '')
const displayYear = computed(() => detail.value?.year || fallbackCtx.value?.year || '')
const displayArea = computed(() => detail.value?.area || fallbackCtx.value?.area || '')

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

const displayedEpisodes = computed<OnlineVideoEpisode[]>(() => {
  const q = episodeQuery.value.trim().toLowerCase()
  const base = orderAsc.value ? [...activeEpisodes.value] : [...activeEpisodes.value].reverse()
  if (!q) return base
  return base.filter((ep) => ep.name.toLowerCase().includes(q))
})

const firstPlayableEntry = computed<{ groupName: string; episode: OnlineVideoEpisode; index: number } | null>(
  () => {
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
  },
)

function openEpisode(ep: OnlineVideoEpisode, groupNameOverride?: string): void {
  if (!ep.play_url?.trim() || !resolvedSourceId.value) return
  const groupName = groupNameOverride || activeGroup.value?.name || ''
  const targetGroup = playGroups.value.find((g) => g.name === groupName) ?? activeGroup.value
  const list = targetGroup?.episodes ?? []
  const idx = Math.max(
    0,
    list.findIndex((item) => item.play_url === ep.play_url && item.index === ep.index),
  )

  const playerCtx: OnlineVideoPlayerContextStorage = {
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
  }
  try {
    sessionStorage.setItem(ONLINE_VIDEO_PLAYER_CTX_KEY, JSON.stringify(playerCtx))
  } catch {
    // ignore storage errors
  }

  const query: Record<string, string> = {
    sourceId: resolvedSourceId.value,
    playUrl: ep.play_url,
    episodeIndex: String(idx),
  }
  if (groupName) query.groupName = groupName
  if (resolvedVideoId.value) query.videoId = resolvedVideoId.value
  if (resolvedDetailUrl.value) query.detailUrl = resolvedDetailUrl.value
  if (displayTitle.value) query.title = displayTitle.value
  if (resolvedSourceName.value) query.sourceName = resolvedSourceName.value

  void router.push({ path: '/video/online/watch', query })
}

function playFirstEpisode(): void {
  if (!firstPlayableEntry.value) return
  openEpisode(firstPlayableEntry.value.episode, firstPlayableEntry.value.groupName)
}

function backToOnlineSearch(): void {
  const query: Record<string, string> = { scope: 'online' }
  if (fallbackCtx.value?.returnKeyword?.trim()) {
    query.q = fallbackCtx.value.returnKeyword.trim()
  }
  if (fallbackCtx.value?.returnSourceId?.trim()) {
    query.source = fallbackCtx.value.returnSourceId.trim()
  }
  void router.push({ path: '/video/catalog', query })
}
</script>

<template>
  <div class="mx-auto max-w-6xl space-y-8 p-6 sm:p-8">
    <!-- Top Back Bar -->
    <div class="flex items-center justify-between gap-3">
      <button
        type="button"
        class="md-btn md-btn-sm md-btn-tonal md-state"
        @click="backToOnlineSearch"
      >
        <LuChevronLeft class="h-4 w-4" />
        <span>{{ t('online.backToSearch') }}</span>
      </button>

      <RefreshButton
        v-if="resolvedSourceId && (resolvedVideoId || resolvedDetailUrl)"
        size="md"
        :action="loadDetail"
        :busy="loading"
      />
    </div>

    <!-- Error State -->
    <div
      v-if="errorMessage && !loading"
      class="md-card-filled flex flex-col items-center justify-center space-y-4 rounded-3xl p-10 text-center"
    >
      <p class="text-sm font-semibold text-[var(--md-sys-color-error)]">
        {{ t('online.detailLoadFailed') }}
      </p>
      <p class="max-w-md text-xs text-[var(--md-sys-color-on-surface-variant)]">
        {{ errorMessage }}
      </p>
      <div class="flex flex-wrap items-center justify-center gap-2.5">
        <button
          v-if="resolvedSourceId && (resolvedVideoId || resolvedDetailUrl)"
          type="button"
          class="md-btn md-btn-sm md-btn-filled md-state"
          @click="loadDetail"
        >
          {{ t('online.retry') }}
        </button>
        <button
          type="button"
          class="md-btn md-btn-sm md-btn-tonal md-state"
          @click="backToOnlineSearch"
        >
          {{ t('online.backToOnlineList') }}
        </button>
      </div>
    </div>

    <!-- Loading Skeleton -->
    <div
      v-else-if="loading && !detail"
      class="md-card-filled flex flex-col items-center justify-center rounded-3xl py-20 text-xs text-[var(--md-sys-color-on-surface-variant)]"
    >
      <div
        class="mb-3 h-7 w-7 animate-spin rounded-full border-2 border-[var(--md-sys-color-primary)] border-t-transparent"
      />
      <span>{{ t('online.loadingDetail') }}</span>
    </div>

    <!-- Hero & Play Groups Content -->
    <template v-else>
      <section
        class="md-card-filled flex flex-col gap-8 rounded-3xl border border-[var(--md-sys-color-outline-variant)]/50 p-6 shadow-[var(--md-sys-elevation-1)] md:flex-row md:p-8"
      >
        <div class="mx-auto w-48 shrink-0 md:mx-0 md:w-56">
          <div class="book-shadow overflow-hidden rounded-2xl">
            <MediaCoverImage
              :src="displayPosterUrl"
              :alt="displayTitle"
              aspect-class="aspect-[2/3]"
              rounded="rounded-2xl"
              :fallback-icon="LuFilm"
              fallback-icon-class="h-12 w-12 text-[var(--md-sys-color-on-surface-variant)]/40"
            />
          </div>
        </div>

        <div class="flex min-w-0 flex-1 flex-col justify-between space-y-5">
          <div class="space-y-3.5">
            <div class="flex flex-wrap items-center gap-2 text-xs">
              <span
                v-if="resolvedSourceName"
                class="rounded-full bg-[var(--md-sys-color-primary-container)] px-3 py-1 font-semibold text-[var(--md-sys-color-on-primary-container)]"
              >
                {{ t('online.sourceLabel') }}: {{ resolvedSourceName }}
              </span>
              <span
                v-if="displayRemarks"
                class="rounded-full bg-[var(--md-sys-color-secondary-container)] px-3 py-1 font-semibold text-[var(--md-sys-color-on-secondary-container)]"
              >
                {{ displayRemarks }}
              </span>
              <span
                v-if="displayYear"
                class="rounded-full bg-[var(--md-sys-color-surface-container-high)] px-3 py-1 font-medium text-[var(--md-sys-color-on-surface-variant)]"
              >
                {{ t('online.yearLabel') }}: {{ displayYear }}
              </span>
              <span
                v-if="displayArea"
                class="rounded-full bg-[var(--md-sys-color-surface-container-high)] px-3 py-1 font-medium text-[var(--md-sys-color-on-surface-variant)]"
              >
                {{ t('online.areaLabel') }}: {{ displayArea }}
              </span>
            </div>

            <h1
              class="text-2xl font-extrabold tracking-tight text-[var(--md-sys-color-on-surface)] sm:text-3xl"
            >
              {{ displayTitle }}
            </h1>

            <div
              v-if="detail?.director || detail?.actors"
              class="space-y-1 text-xs text-[var(--md-sys-color-on-surface-variant)]"
            >
              <p v-if="detail?.director">
                <span class="font-semibold">{{ t('online.directorLabel') }}:</span>
                {{ detail.director }}
              </p>
              <p v-if="detail?.actors">
                <span class="font-semibold">{{ t('online.actorsLabel') }}:</span>
                {{ detail.actors }}
              </p>
            </div>

            <p
              class="text-sm leading-relaxed whitespace-pre-line text-[var(--md-sys-color-on-surface-variant)]"
            >
              {{ displaySummary }}
            </p>
          </div>

          <div class="flex flex-wrap items-center gap-2.5 pt-2">
            <button
              type="button"
              :disabled="!firstPlayableEntry"
              class="md-btn md-btn-filled md-state !h-11 !px-6 text-sm shadow-[var(--md-sys-elevation-1)]"
              @click="playFirstEpisode"
            >
              <LuPlay class="h-4 w-4 fill-current" />
              <span>{{ t('online.playNow') }}</span>
            </button>
          </div>
        </div>
      </section>

      <!-- Play Lines & Episodes Section -->
      <section class="space-y-4">
        <div class="flex flex-wrap items-center justify-between gap-3">
          <div class="flex items-center gap-3">
            <h2 class="text-lg font-bold text-[var(--md-sys-color-on-surface)]">
              {{ t('online.episodesHeader') }}
            </h2>
            <span
              class="rounded-full bg-[var(--md-sys-color-surface-container-high)] px-2.5 py-0.5 text-xs font-semibold text-[var(--md-sys-color-on-surface-variant)]"
            >
              {{ t('online.episodesCount', { count: activeEpisodes.length }) }}
            </span>
          </div>

          <div v-if="activeEpisodes.length > 0" class="flex flex-wrap items-center gap-2.5">
            <div v-if="activeEpisodes.length > 12" class="relative w-48">
              <LuSearch
                class="pointer-events-none absolute top-1/2 left-3 h-3.5 w-3.5 -translate-y-1/2 text-[var(--md-sys-color-on-surface-variant)]"
              />
              <input
                v-model="episodeQuery"
                type="text"
                :placeholder="t('online.filterEpisodesPlaceholder')"
                class="md-input h-8 w-full !pl-8 text-xs"
              />
            </div>

            <button
              v-if="activeEpisodes.length > 1"
              type="button"
              class="md-btn md-btn-sm md-btn-outlined md-state"
              @click="orderAsc = !orderAsc"
            >
              <LuArrowUpDown class="h-3.5 w-3.5" />
              <span>{{ orderAsc ? t('online.orderAsc') : t('online.orderDesc') }}</span>
            </button>
          </div>
        </div>

        <!-- Empty Play Groups Notice -->
        <div
          v-if="!loading && totalEpisodesCount === 0"
          class="md-card-outlined rounded-3xl border-dashed py-16 text-center text-sm text-[var(--md-sys-color-on-surface-variant)]"
        >
          {{ t('online.emptyPlayGroups') }}
        </div>

        <template v-else>
          <MdTabs
            v-if="playGroups.length >= 1"
            v-model="selectedGroupName"
            :tabs="groupTabs"
            variant="secondary"
            :aria-label="t('online.playLinesHeader')"
          />

          <div
            class="grid grid-cols-2 gap-2.5 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-6"
          >
            <button
              v-for="ep in displayedEpisodes"
              :key="`${selectedGroupName}-${ep.index}-${ep.play_url}`"
              type="button"
              :disabled="!ep.play_url"
              class="md-btn md-btn-tonal md-state !h-10 justify-center truncate !rounded-xl px-3 text-xs font-semibold"
              @click="openEpisode(ep)"
            >
              {{ ep.name }}
            </button>
          </div>
        </template>
      </section>
    </template>
  </div>
</template>
