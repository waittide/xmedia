<script setup lang="ts">
import { computed, ref } from 'vue'
import { useRouter } from 'vue-router'
import { useI18n as useGlobalI18n } from 'vue-i18n'
import {
  LuCheck,
  LuChevronRight,
  LuDisc3,
  LuPlay,
  LuRefreshCw,
  LuShuffle,
} from 'vue-icons-plus/lu'
import { DomainHomeTemplate, MediaItemCard } from '@/components/domain'
import { MdButton, MdIconButton } from '@/components/ui'
import { useActionFeedback } from '@/composables/useActionFeedback'
import { useHomeOverview } from '@/composables/useHomeOverview'
import {
  isAvailabilityBlocked,
  itemRoute,
  useMergedShelf,
  useSourceFilter,
  type MediaItemVM,
} from '@/sources'
import { useSnackbarStore } from '@/stores/snackbar'
import { getAcceptedUploadExtensions, useUploadStore } from '@/stores/uploads'
import { formatTime } from '@/utils/format'
import { api, buildCoverUrl } from '../api'
import AlbumCard from '../components/AlbumCard.vue'
import TrackTable from '../components/TrackTable.vue'
import { useMusicI18n as useI18n } from '../i18n'
import { usePlayerStore } from '../player'
import { useMusicStore } from '../store'
import type { AlbumDto, TrackDto } from '../types'

defineOptions({ name: 'MusicHomeView' })

const { t } = useI18n()
const { t: globalT } = useGlobalI18n()
const router = useRouter()
const musicStore = useMusicStore()
const playerStore = usePlayerStore()
const uploadStore = useUploadStore()
const snackbarStore = useSnackbarStore()
const randomFeedback = useActionFeedback()

const uploadInputRef = ref<HTMLInputElement | null>(null)
const refreshing = ref(false)
const musicAcceptExts = getAcceptedUploadExtensions(['music'])

const sourceFilter = useSourceFilter('music')

const { overview, loading, error: homeError, refreshPage } = useHomeOverview('music', {
  refreshSidebar: () => musicStore.refreshSidebar(),
  watchSources: () => [musicStore.catalogVersion, uploadStore.versionCounter],
})

const continueShelf = useMergedShelf('music', 'continue', { limit: 12 })
const favoritesShelf = useMergedShelf('music', 'favorites', { limit: 12 })

// Combine active/saved local play queue track (with real progress) + bound local continue shelf
const continueItems = computed<MediaItemVM[]>(() => {
  const ov = overview.value
  const list: MediaItemVM[] = []
  const seenLocal = new Set<string>()

  if (ov?.play_queue?.current_track_id) {
    const cur = ov.play_queue.tracks.find((tr) => tr.id === ov.play_queue!.current_track_id)
    if (cur) {
      const resumeSecs = Math.max(0, (ov.play_queue.position_ms || 0) / 1000)
      const pct =
        cur.duration_secs > 0 && resumeSecs > 0
          ? Math.min(100, Math.max(1, Math.round((resumeSecs / cur.duration_secs) * 100)))
          : undefined
      seenLocal.add(cur.id)
      list.push({
        ref: { scope: 'local', domain: 'music', id: cur.id },
        title: cur.title,
        subtitle:
          resumeSecs > 0 && cur.duration_secs > 0
            ? `${cur.artist_name} · ${formatTime(resumeSecs)} / ${formatTime(cur.duration_secs)}`
            : [cur.artist_name, cur.album_title].filter(Boolean).join(' · ') || undefined,
        cover: cur.cover_key ? (buildCoverUrl(cur.id, 200) ?? undefined) : undefined,
        badge: {
          sourceKind: 'local',
          progressPercent: pct,
        },
        to: cur.album_id ? itemRoute({ scope: 'local', domain: 'music', id: cur.album_id }) : '/music/library?view=tracks',
      })
    }
  }

  for (const item of continueShelf.items.value) {
    if (item.ref.scope === 'local' && seenLocal.has(item.ref.id)) continue
    list.push(item)
    if (list.length >= 12) break
  }

  return list
})

const recentAlbumsMap = computed(() => {
  const map = new Map<string, AlbumDto>()
  for (const al of overview.value?.recent_added_albums ?? []) {
    map.set(al.id, al)
  }
  return map
})

const recentAlbumVMs = computed<MediaItemVM[]>(() =>
  (overview.value?.recent_added_albums ?? []).map((al) => ({
    ref: { scope: 'local' as const, domain: 'music' as const, id: al.id },
    title: al.title,
    subtitle: al.album_artist_name || undefined,
    cover: al.cover_key ? (buildCoverUrl(al.id, 320) ?? undefined) : undefined,
    badge: { sourceKind: 'local' as const },
    to: itemRoute({ scope: 'local', domain: 'music', id: al.id }),
  })),
)

const randomPicks = computed<TrackDto[]>(() => (overview.value?.random_tracks ?? []).slice(0, 6))

const summaryText = computed(() => {
  const st = overview.value?.stats
  if (!st || st.total_tracks === 0) return ''
  const parts = [t('common.tracksCount', { count: st.total_tracks })]
  if (st.total_albums > 0) {
    parts.push(t('common.albumsCount', { count: st.total_albums }))
  }
  return parts.join(' · ')
})

const combinedLoading = computed(
  () =>
    (loading.value && !overview.value) ||
    (continueShelf.loading.value &&
      continueItems.value.length === 0 &&
      recentAlbumVMs.value.length === 0),
)

const combinedError = computed(() => {
  if (continueItems.value.length > 0 || recentAlbumVMs.value.length > 0) return null
  return homeError.value || continueShelf.error.value
})

async function handleRefresh(): Promise<void> {
  refreshing.value = true
  try {
    await Promise.allSettled([
      refreshPage(),
      continueShelf.refresh(),
      favoritesShelf.refresh(),
    ])
  } finally {
    refreshing.value = false
  }
}

async function refreshRandomPicks(): Promise<void> {
  if (!overview.value) return
  overview.value.random_tracks = await api.randomTracks(12)
}

function findLocalResumeTrack(trackId: string): { track: TrackDto; resumeSecs: number } | null {
  const ov = overview.value
  if (!ov) return null
  if (ov.play_queue?.current_track_id === trackId) {
    const found = ov.play_queue.tracks.find((tr) => tr.id === trackId)
    if (found) {
      return { track: found, resumeSecs: Math.max(0, (ov.play_queue.position_ms || 0) / 1000) }
    }
  }
  const bm = ov.bookmarks.find((b) => b.track.id === trackId)
  if (bm) {
    return { track: bm.track, resumeSecs: Math.max(0, bm.position_ms / 1000) }
  }
  const recent = ov.recent_played_tracks.find((tr) => tr.id === trackId)
  if (recent) {
    return { track: recent, resumeSecs: 0 }
  }
  return null
}

async function handlePlayShelfItem(item: MediaItemVM): Promise<void> {
  if (item.badge?.blocked || isAvailabilityBlocked(item.badge?.availability)) {
    snackbarStore.show({ message: t('common.sourceUnavailable'), tone: 'error' })
    return
  }

  if (item.ref.scope === 'local') {
    const cached = findLocalResumeTrack(item.ref.id)
    if (cached) {
      playerStore.playTrackAt(cached.track, cached.resumeSecs)
      return
    }
    try {
      const tr = await api.getTrack(item.ref.id)
      playerStore.playTrackAt(tr, 0)
    } catch {
      void router.push(item.to)
    }
    return
  }

  void router.push(item.to)
}

function handleFavoriteCardSelect(item: MediaItemVM): void {
  if (item.ref.scope === 'local') {
    const isAlbum =
      typeof item.to === 'object' &&
      item.to !== null &&
      'path' in item.to &&
      item.to.path === `/music/item/${encodeURIComponent(item.ref.id)}`
    if (isAlbum) {
      void router.push(item.to)
      return
    }
  }
  void handlePlayShelfItem(item)
}

function triggerUpload(): void {
  uploadInputRef.value?.click()
}

function handleFileInputChange(ev: Event): void {
  const input = ev.target as HTMLInputElement
  if (input.files && input.files.length > 0) {
    uploadStore.enqueueFiles(input.files)
    input.value = ''
  }
}
</script>

<template>
  <div class="px-4 py-6 sm:px-6 lg:px-8">
    <input
      ref="uploadInputRef"
      type="file"
      multiple
      :accept="musicAcceptExts"
      class="hidden"
      @change="handleFileInputChange"
    />

    <DomainHomeTemplate
      domain="music"
      :summary="summaryText"
      :continue-items="continueItems"
      :recent-items="recentAlbumVMs"
      :continue-title="t('continueView.title')"
      :recent-title="t('home.recentAddedAlbums')"
      :continue-see-all-to="{ path: '/music/library', query: { tab: 'continue' } }"
      :recent-see-all-to="{ path: '/music/library', query: { view: 'items' } }"
      aspect="square"
      cover-style="album"
      :loading="combinedLoading"
      :refreshing="refreshing"
      :error="combinedError"
      :has-online-sources="sourceFilter.hasOnlineSources.value || sourceFilter.loading.value"
      :empty-title="t('home.emptyLibraryTitle')"
      :empty-description="t('home.emptyLibraryDesc')"
      @refresh="handleRefresh"
      @retry="handleRefresh"
      @upload="triggerUpload"
    >
      <template #continue-card="{ item }">
        <MediaItemCard
          :item="item"
          aspect="square"
          layout="horizontal"
          as-button
          :fallback-icon="LuDisc3"
          @select="handlePlayShelfItem"
        />
      </template>

      <template #recent-card="{ item }">
        <AlbumCard
          v-if="item.ref.scope === 'local' && recentAlbumsMap.get(item.ref.id)"
          :album="recentAlbumsMap.get(item.ref.id)!"
        />
        <MediaItemCard
          v-else
          :item="item"
          aspect="square"
          cover-style="album"
          :fallback-icon="LuDisc3"
          @select="handlePlayShelfItem"
        />
      </template>

      <template #extra-sections>
        <!-- Cross-Source Favorites Rail (local + online/remote) -->
        <section v-if="favoritesShelf.items.value.length > 0" class="space-y-3">
          <div class="flex items-center justify-between gap-2">
            <h2 class="text-title-large text-on-surface">
              {{ t('favorites.title') }}
            </h2>
            <MdButton
              variant="text"
              :to="{ path: '/music/library', query: { tab: 'favorites' } }"
            >
              <span>{{ globalT('common.viewAll') }}</span>
              <template #trailing>
                <LuChevronRight class="h-4 w-4" aria-hidden="true" />
              </template>
            </MdButton>
          </div>

          <div class="no-scrollbar -mx-1 flex snap-x snap-mandatory gap-4 overflow-x-auto px-1 pb-2">
            <div
              v-for="(item, idx) in favoritesShelf.items.value"
              :key="`fav-${idx}-${item.ref.scope === 'local' ? item.ref.id : item.ref.ref}`"
              class="w-36 shrink-0 snap-start sm:w-40"
            >
              <MediaItemCard
                :item="item"
                aspect="square"
                cover-style="album"
                :as-button="item.ref.scope !== 'local'"
                :fallback-icon="LuDisc3"
                @select="handleFavoriteCardSelect"
              />
            </div>
          </div>
        </section>

        <!-- Compact Random Discovery (6 tracks, single tonal Play + icon actions) -->
        <section v-if="randomPicks.length > 0" class="space-y-3">
          <div class="flex flex-wrap items-center justify-between gap-2">
            <h2 class="text-title-large text-on-surface">
              {{ t('home.randomDiscovery') }}
            </h2>
            <div class="flex items-center gap-1.5">
              <MdButton
                variant="tonal"
                :icon="LuPlay"
                @click="playerStore.playQueue(overview!.random_tracks, 0)"
              >
                {{ t('common.playAll') }}
              </MdButton>
              <MdIconButton
                size="sm"
                variant="standard"
                :icon="LuShuffle"
                :aria-label="t('common.shufflePlay')"
                :tooltip="t('common.shufflePlay')"
                @click="playerStore.shuffleAndPlay(overview!.random_tracks)"
              />
              <MdIconButton
                size="sm"
                variant="standard"
                :icon="randomFeedback.done.value ? LuCheck : LuRefreshCw"
                :loading="randomFeedback.running.value"
                :aria-label="t('home.refreshRandom')"
                :tooltip="t('home.refreshRandom')"
                @click="randomFeedback.run(refreshRandomPicks)"
              />
            </div>
          </div>

          <TrackTable :tracks="randomPicks" />
        </section>
      </template>
    </DomainHomeTemplate>
  </div>
</template>
