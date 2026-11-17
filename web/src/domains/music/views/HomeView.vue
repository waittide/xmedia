<script setup lang="ts">
import { computed, onMounted, ref, watch, type Component } from 'vue'
import { useRouter } from 'vue-router'
import { useI18n as useGlobalI18n } from 'vue-i18n'
import {
  LuArrowRight,
  LuBookmark,
  LuClock,
  LuDisc3,
  LuHeart,
  LuLibrary,
  LuListMusic,
  LuMic,
  LuMusic,
  LuPlay,
  LuRadio,
  LuRefreshCw,
  LuShuffle,
  LuSlidersHorizontal,
  LuTag,
} from 'vue-icons-plus/lu'
import { useThemeStore } from '@/stores/theme'
import { useUploadStore } from '@/stores/uploads'
import LayoutCustomizerModal from '@/components/LayoutCustomizerModal.vue'
import { api } from '../api'
import AlbumCard from '../components/AlbumCard.vue'
import CoverImage from '../components/CoverImage.vue'
import TrackTable from '../components/TrackTable.vue'
import { useMusicI18n as useI18n } from '../i18n'
import { usePlayerStore } from '../player'
import { useMusicStore } from '../store'
import type { HomeOverviewDto, TrackDto } from '../types'

const { t } = useI18n()
const { t: globalT } = useGlobalI18n()
const router = useRouter()
const musicStore = useMusicStore()
const playerStore = usePlayerStore()
const themeStore = useThemeStore()
const uploadStore = useUploadStore()

const loading = ref(true)
const overview = ref<HomeOverviewDto | null>(null)
const customizerOpen = ref(false)

async function loadHome(): Promise<void> {
  loading.value = true
  try {
    overview.value = await api.homeOverview()
  } finally {
    loading.value = false
  }
}

async function refreshRandomPicks(): Promise<void> {
  if (!overview.value) return
  overview.value.random_tracks = await api.randomTracks(12)
}

onMounted(loadHome)
watch(() => [musicStore.catalogVersion, uploadStore.versionCounter], loadHome)

interface StatCardItem {
  key: string
  label: string
  value: number
  icon: Component
  tone?: 'accent' | 'rose'
  onClick: () => void
}

const statCards = computed<StatCardItem[]>(() => {
  const st = overview.value?.stats
  const keys = themeStore.workspaceLayout.music.home_cards
  return keys.map((key) => {
    const label = globalT(`layoutCustomizer.cards.music.${key}`)
    switch (key) {
      case 'playlists':
        return {
          key,
          label,
          value: st?.playlists_count ?? musicStore.playlists.length,
          icon: LuListMusic,
          onClick: () => void router.push('/music/playlists'),
        }
      case 'listening':
        return {
          key,
          label,
          value: st?.listening_count ?? 0,
          icon: LuClock,
          onClick: () => void router.push('/music/continue'),
        }
      case 'favorites':
        return {
          key,
          label,
          value: st?.starred_tracks ?? 0,
          icon: LuHeart,
          tone: 'rose',
          onClick: () => void router.push('/music/favorites'),
        }
      case 'bookmarks':
        return {
          key,
          label,
          value: st?.bookmarks_count ?? 0,
          icon: LuBookmark,
          onClick: () => void router.push('/music/continue'),
        }
      case 'albums':
        return {
          key,
          label,
          value: st?.total_albums ?? 0,
          icon: LuDisc3,
          onClick: () => void router.push({ path: '/music/songs', query: { tab: 'albums' } }),
        }
      case 'artists':
        return {
          key,
          label,
          value: st?.total_artists ?? 0,
          icon: LuMic,
          onClick: () => void router.push({ path: '/music/songs', query: { tab: 'artists' } }),
        }
      case 'genres':
        return {
          key,
          label,
          value: st?.genres_count ?? musicStore.genres.length,
          icon: LuRadio,
          onClick: () => void router.push('/music/genres'),
        }
      case 'tags':
        return {
          key,
          label,
          value: st?.tags_count ?? musicStore.tags.length,
          icon: LuTag,
          onClick: () => void router.push('/music/tags'),
        }
      case 'libraries':
        return {
          key,
          label,
          value: st?.libraries_count ?? musicStore.libraries.length,
          icon: LuLibrary,
          onClick: () => void router.push('/music/libraries'),
        }
      case 'all':
      default:
        return {
          key: 'all',
          label,
          value: st?.total_tracks ?? 0,
          icon: LuMusic,
          onClick: () => void router.push('/music/songs'),
        }
    }
  })
})

const cardsGridClass = computed(() => {
  const n = statCards.value.length
  if (n <= 2) return 'grid-cols-1 sm:grid-cols-2'
  if (n === 3) return 'grid-cols-1 sm:grid-cols-3'
  return 'grid-cols-2 lg:grid-cols-4'
})

function formatClock(secs: number): string {
  const s = Math.max(0, Math.floor(secs))
  const m = Math.floor(s / 60)
  const rem = s % 60
  return `${m}:${String(rem).padStart(2, '0')}`
}

interface ContinueListenItem {
  track: TrackDto
  resumeSecs: number
  percent: number
  subtitle: string
}

const continueItems = computed<ContinueListenItem[]>(() => {
  const ov = overview.value
  if (!ov) return []
  const seen = new Set<string>()
  const out: ContinueListenItem[] = []

  // 1. Saved cloud play queue current track
  if (ov.play_queue && ov.play_queue.current_track_id) {
    const cur = ov.play_queue.tracks.find((tr) => tr.id === ov.play_queue!.current_track_id)
    if (cur) {
      const resumeSecs = ov.play_queue.position_ms / 1000
      const pct =
        cur.duration_secs > 0
          ? Math.min(100, Math.round((resumeSecs / cur.duration_secs) * 100))
          : 0
      seen.add(cur.id)
      out.push({
        track: cur,
        resumeSecs,
        percent: Math.max(5, pct),
        subtitle: `${cur.artist_name} · ${formatClock(resumeSecs)} / ${formatClock(cur.duration_secs)}`,
      })
    }
  }

  // 2. Audio bookmarks
  for (const bm of ov.bookmarks) {
    if (seen.has(bm.track.id)) continue
    seen.add(bm.track.id)
    const resumeSecs = bm.position_ms / 1000
    const pct =
      bm.track.duration_secs > 0
        ? Math.min(100, Math.round((resumeSecs / bm.track.duration_secs) * 100))
        : 0
    out.push({
      track: bm.track,
      resumeSecs,
      percent: Math.max(5, pct),
      subtitle: `${bm.track.artist_name} · ${formatClock(resumeSecs)} / ${formatClock(bm.track.duration_secs)}`,
    })
    if (out.length >= 6) return out
  }

  // 3. Recently played tracks
  for (const rt of ov.recent_played_tracks) {
    if (seen.has(rt.id)) continue
    seen.add(rt.id)
    out.push({
      track: rt,
      resumeSecs: 0,
      percent: 100,
      subtitle: `${rt.artist_name} · ${rt.album_title}`,
    })
    if (out.length >= 6) return out
  }

  return out
})
</script>

<template>
  <div class="mx-auto max-w-7xl space-y-10 px-6 py-8 sm:px-8">
    <!-- Customizable Music Stat Cards (1..=4) -->
    <section class="space-y-2.5">
      <div class="flex items-center justify-end">
        <button
          type="button"
          class="inline-flex items-center gap-1.5 rounded-lg px-2 py-1 text-[11px] font-medium text-[var(--ctp-subtext0)] transition hover:bg-[var(--ctp-shell)] hover:text-[var(--ctp-text)]"
          @click="customizerOpen = true"
        >
          <LuSlidersHorizontal class="h-3.5 w-3.5" />
          <span>{{ globalT('layoutCustomizer.customizeCards') }}</span>
        </button>
      </div>

      <div class="grid gap-4" :class="cardsGridClass">
        <button
          v-for="card in statCards"
          :key="card.key"
          type="button"
          class="card-soft flex items-center gap-3.5 rounded-2xl bg-[var(--ctp-shell)] p-4 text-left transition hover:border-[var(--ctp-accent)]"
          @click="card.onClick()"
        >
          <div
            class="flex h-10 w-10 shrink-0 items-center justify-center rounded-xl"
            :class="
              card.tone === 'rose'
                ? 'bg-[var(--ctp-red)]/15 text-[var(--ctp-red)]'
                : 'bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]'
            "
          >
            <component :is="card.icon" class="h-5 w-5" />
          </div>
          <div class="min-w-0">
            <p class="text-xl font-bold leading-none text-[var(--ctp-text)]">{{ card.value }}</p>
            <p class="mt-1 truncate text-xs text-[var(--ctp-subtext0)]">{{ card.label }}</p>
          </div>
        </button>
      </div>
    </section>

    <!-- Empty Library Onboarding -->
    <section
      v-if="!loading && overview && overview.stats.total_tracks === 0"
      class="card-soft flex flex-col items-center justify-center rounded-3xl bg-[var(--ctp-shell)] px-6 py-16 text-center"
    >
      <div
        class="mb-4 flex h-14 w-14 items-center justify-center rounded-2xl bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]"
      >
        <LuDisc3 class="h-7 w-7" />
      </div>
      <h2 class="text-lg font-bold">{{ t('home.emptyLibraryTitle') }}</h2>
      <p class="mt-1.5 max-w-md text-xs leading-relaxed text-[var(--ctp-subtext0)]">
        {{ t('home.emptyLibraryDesc') }}
      </p>
    </section>

    <template v-else-if="overview">
      <!-- Continue Listening (Unified Horizontal Cards) -->
      <section v-if="continueItems.length > 0">
        <div class="mb-5 flex items-end justify-between">
          <div>
            <h2 class="text-lg font-bold tracking-tight text-[var(--ctp-text)]">
              {{ t('continueView.title') }}
            </h2>
            <p class="mt-1 text-xs text-[var(--ctp-subtext0)]">
              {{ t('continueView.subtitle') }}
            </p>
          </div>
          <RouterLink
            to="/music/continue"
            class="inline-flex items-center gap-1 text-xs font-semibold text-[var(--ctp-accent)] hover:underline"
          >
            <span>{{ t('collections.seeAll') }}</span>
            <LuArrowRight class="h-3.5 w-3.5" />
          </RouterLink>
        </div>

        <div class="grid grid-cols-1 gap-4 md:grid-cols-2 xl:grid-cols-3">
          <button
            v-for="item in continueItems"
            :key="item.track.id"
            type="button"
            class="group flex items-center gap-4 rounded-2xl border border-[var(--ctp-surface0)]/85 bg-[var(--ctp-shell)]/90 p-3.5 text-left transition duration-200 hover:-translate-y-0.5 hover:border-[var(--ctp-accent)]/60 hover:bg-[var(--ctp-mantle)]"
            @click="playerStore.playTrackAt(item.track, item.resumeSecs)"
          >
            <CoverImage
              :cover-id="item.track.id"
              :has-cover="Boolean(item.track.cover_key)"
              :size="200"
              rounded="rounded-xl"
              class="h-24 w-24 shrink-0 shadow-md"
            />

            <div class="flex min-w-0 flex-1 flex-col justify-center">
              <h3
                class="truncate text-sm font-bold text-[var(--ctp-text)] transition-colors group-hover:text-[var(--ctp-accent)]"
              >
                {{ item.track.title }}
              </h3>
              <p class="mt-1.5 truncate text-xs text-[var(--ctp-subtext0)]">
                {{ item.subtitle }}
              </p>

              <div class="mt-4 flex items-center gap-3">
                <div class="h-1.5 flex-1 overflow-hidden rounded-full bg-[var(--ctp-canvas)]/90">
                  <div
                    class="h-full rounded-full bg-[var(--ctp-accent)] transition-all duration-300"
                    :style="{ width: `${item.percent}%` }"
                  />
                </div>
                <span
                  class="flex h-7 w-7 shrink-0 items-center justify-center rounded-full bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)] shadow-sm transition group-hover:scale-105"
                >
                  <LuPlay class="ml-0.5 h-3.5 w-3.5 fill-current" />
                </span>
              </div>
            </div>
          </button>
        </div>
      </section>

      <!-- Recently Added Albums -->
      <section v-if="overview.recent_added_albums.length > 0">
        <div class="mb-5 flex items-end justify-between">
          <h2 class="text-lg font-bold tracking-tight text-[var(--ctp-text)]">
            {{ t('home.recentAddedAlbums') }}
          </h2>
          <RouterLink
            :to="{ path: '/music/songs', query: { tab: 'albums' } }"
            class="inline-flex items-center gap-1 text-xs font-semibold text-[var(--ctp-accent)] hover:underline"
          >
            <span>{{ t('collections.seeAll') }}</span>
            <LuArrowRight class="h-3.5 w-3.5" />
          </RouterLink>
        </div>

        <div class="grid grid-cols-2 gap-4 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-6">
          <AlbumCard v-for="al in overview.recent_added_albums" :key="al.id" :album="al" />
        </div>
      </section>

      <!-- Most Played Albums -->
      <section v-if="overview.frequent_albums.length > 0">
        <div class="mb-5 flex items-end justify-between">
          <h2 class="text-lg font-bold tracking-tight text-[var(--ctp-text)]">
            {{ t('home.frequentAlbums') }}
          </h2>
        </div>

        <div class="grid grid-cols-2 gap-4 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-6">
          <AlbumCard
            v-for="al in overview.frequent_albums"
            :key="`freq-${al.id}`"
            :album="al"
          />
        </div>
      </section>

      <!-- Random Discovery -->
      <section v-if="overview.random_tracks.length > 0">
        <div class="mb-4 flex flex-wrap items-center justify-between gap-3">
          <h2 class="text-lg font-bold tracking-tight text-[var(--ctp-text)]">
            {{ t('home.randomDiscovery') }}
          </h2>
          <div class="flex flex-wrap items-center gap-2">
            <button
              type="button"
              class="inline-flex items-center gap-1.5 rounded-xl bg-[var(--ctp-accent)] px-3.5 py-1.5 text-xs font-bold text-[var(--ctp-on-accent)] shadow-xs transition hover:opacity-95"
              @click="playerStore.playQueue(overview.random_tracks, 0)"
            >
              <LuPlay class="h-3.5 w-3.5 fill-current" />
              <span>{{ t('common.playAll') }}</span>
            </button>
            <button
              type="button"
              class="inline-flex items-center gap-1.5 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-shell)] px-3.5 py-1.5 text-xs font-semibold text-[var(--ctp-text)] transition hover:bg-[var(--ctp-surface0)]"
              @click="playerStore.shuffleAndPlay(overview.random_tracks)"
            >
              <LuShuffle class="h-3.5 w-3.5" />
              <span>{{ t('common.shufflePlay') }}</span>
            </button>
            <button
              type="button"
              class="inline-flex items-center gap-1.5 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-shell)] px-3 py-1.5 text-xs font-medium text-[var(--ctp-subtext1)] transition hover:text-[var(--ctp-text)]"
              @click="refreshRandomPicks"
            >
              <LuRefreshCw class="h-3.5 w-3.5" />
              <span>{{ t('home.refreshRandom') }}</span>
            </button>
          </div>
        </div>
        <TrackTable :tracks="overview.random_tracks" />
      </section>
    </template>

    <LayoutCustomizerModal
      :open="customizerOpen"
      domain="music"
      initial-tab="cards"
      @close="customizerOpen = false"
    />
  </div>
</template>
