<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { useI18n } from 'vue-i18n'
import {
  LuBookOpen,
  LuClapperboard,
  LuDisc3,
  LuLayers,
  LuPlay,
} from 'vue-icons-plus/lu'
import { api } from '@/api/client'
import { useAuthStore } from '@/stores/auth'
import { useEventsStore } from '@/stores/events'
import { useMusicPlayerStore } from '@/stores/musicPlayer'
import type {
  BookSummaryDto,
  ContinueReadingItemDto,
  SystemOverviewDto,
  TrackDto,
  VideoSummaryDto,
} from '@/types/api'

const { t } = useI18n()
const router = useRouter()
const auth = useAuthStore()
const events = useEventsStore()
const musicPlayer = useMusicPlayerStore()

const overview = ref<SystemOverviewDto | null>(null)
const recentBooks = ref<BookSummaryDto[]>([])
const continueComics = ref<ContinueReadingItemDto[]>([])
const recentMusic = ref<TrackDto[]>([])
const continueVideos = ref<VideoSummaryDto[]>([])

const originUrl = computed(() => window.location.origin)

async function loadAll(): Promise<void> {
  try {
    overview.value = await api.systemOverview()
  } catch {
    // ignore
  }
  if (auth.hasModule('book')) {
    api.recentBooks(6).then((b) => (recentBooks.value = b)).catch(() => {})
  }
  if (auth.hasModule('comic')) {
    api
      .comicHome()
      .then((c) => (continueComics.value = c.continue_reading.slice(0, 6)))
      .catch(() => {})
  }
  if (auth.hasModule('music')) {
    api
      .musicHome()
      .then((m) => {
        recentMusic.value =
          m.recent_played_tracks.length > 0
            ? m.recent_played_tracks.slice(0, 6)
            : m.random_tracks.slice(0, 6)
      })
      .catch(() => {})
  }
  if (auth.hasModule('video')) {
    api
      .videoHome()
      .then((v) => {
        continueVideos.value =
          v.continue_watching.length > 0
            ? v.continue_watching.slice(0, 6)
            : v.recent_videos.slice(0, 6)
      })
      .catch(() => {})
  }
}

onMounted(loadAll)
watch(() => events.changeTick, loadAll, { deep: true })

function domainMeta(kind: string) {
  if (kind === 'book') return { label: t('nav.books'), path: '/books', icon: LuBookOpen }
  if (kind === 'comic') return { label: t('nav.comics'), path: '/comics', icon: LuLayers }
  if (kind === 'music') return { label: t('nav.music'), path: '/music', icon: LuDisc3 }
  return { label: t('nav.videos'), path: '/videos', icon: LuClapperboard }
}
</script>

<template>
  <div class="mx-auto max-w-7xl space-y-8 p-6">
    <!-- Hero Banner -->
    <div class="rounded-2xl border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)] p-6">
      <h1 class="text-2xl font-bold tracking-tight">
        {{ t('dashboard.welcome', { name: auth.user?.display_name || auth.user?.username || '' }) }}
      </h1>
      <p class="mt-1 text-sm text-[var(--ctp-subtext0)]">{{ t('dashboard.subtitle') }}</p>

      <!-- Domain Stat Cards -->
      <div v-if="overview" class="mt-5 grid grid-cols-2 gap-3 md:grid-cols-4">
        <button
          v-for="d in overview.domains"
          :key="d.media_kind"
          type="button"
          class="flex items-center gap-3 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] p-4 text-left transition hover:border-[var(--ctp-accent)]"
          @click="router.push(domainMeta(d.media_kind).path)"
        >
          <div class="flex h-10 w-10 items-center justify-center rounded-xl bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]">
            <component :is="domainMeta(d.media_kind).icon" class="h-5 w-5" />
          </div>
          <div>
            <div class="text-xs text-[var(--ctp-subtext0)]">{{ domainMeta(d.media_kind).label }}</div>
            <div class="text-lg font-bold tabular-nums">{{ d.item_count }}</div>
          </div>
        </button>
      </div>
    </div>

    <!-- Continue Watching Videos -->
    <section v-if="continueVideos.length > 0" class="space-y-3">
      <div class="flex items-center justify-between">
        <h2 class="text-base font-semibold">{{ t('dashboard.continueVideos') }}</h2>
        <RouterLink to="/videos" class="text-xs text-[var(--ctp-accent)] hover:underline">
          {{ t('common.all') }} →
        </RouterLink>
      </div>
      <div class="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-3">
        <div
          v-for="v in continueVideos"
          :key="v.id"
          class="group cursor-pointer overflow-hidden rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)] transition hover:border-[var(--ctp-accent)]"
          @click="router.push(`/videos/watch/${v.id}`)"
        >
          <div class="relative aspect-video bg-[var(--ctp-surface0)] overflow-hidden">
            <img
              v-if="v.poster_key"
              :src="api.videoPosterUrl(v.poster_key) ?? ''"
              alt=""
              class="h-full w-full object-cover transition group-hover:scale-105"
            />
            <span
              v-if="v.resolution"
              class="absolute right-2 top-2 rounded bg-black/70 px-1.5 py-0.5 text-[10px] font-semibold text-white"
            >
              {{ v.resolution }}
            </span>
          </div>
          <div class="p-3">
            <div class="truncate text-sm font-semibold">{{ v.title }}</div>
            <div class="truncate text-xs text-[var(--ctp-subtext0)]">
              {{ v.show_title ? `${v.show_title} · S${v.season_number ?? 1}E${v.episode_number ?? 1}` : v.container.toUpperCase() }}
            </div>
          </div>
        </div>
      </div>
    </section>

    <!-- Continue Reading Books -->
    <section v-if="recentBooks.length > 0" class="space-y-3">
      <div class="flex items-center justify-between">
        <h2 class="text-base font-semibold">{{ t('dashboard.continueBooks') }}</h2>
        <RouterLink to="/books" class="text-xs text-[var(--ctp-accent)] hover:underline">
          {{ t('common.all') }} →
        </RouterLink>
      </div>
      <div class="grid grid-cols-2 gap-4 sm:grid-cols-3 md:grid-cols-6">
        <div
          v-for="b in recentBooks"
          :key="b.id"
          class="cursor-pointer space-y-2"
          @click="router.push(`/books/read/${b.id}`)"
        >
          <div class="aspect-[2/3] overflow-hidden rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)] media-shadow">
            <img
              v-if="b.has_cover"
              :src="api.bookCoverUrl(b.id)"
              alt=""
              class="h-full w-full object-cover"
            />
          </div>
          <div class="truncate text-xs font-semibold">{{ b.metadata.title }}</div>
          <div class="truncate text-[11px] text-[var(--ctp-subtext0)]">
            {{ b.progress ? `${Math.round(b.progress.progress_percent)}%` : b.format.toUpperCase() }}
          </div>
        </div>
      </div>
    </section>

    <!-- Continue Reading Comics -->
    <section v-if="continueComics.length > 0" class="space-y-3">
      <div class="flex items-center justify-between">
        <h2 class="text-base font-semibold">{{ t('dashboard.continueComics') }}</h2>
        <RouterLink to="/comics" class="text-xs text-[var(--ctp-accent)] hover:underline">
          {{ t('common.all') }} →
        </RouterLink>
      </div>
      <div class="grid grid-cols-2 gap-4 sm:grid-cols-3 md:grid-cols-6">
        <div
          v-for="item in continueComics"
          :key="item.chapter_id"
          class="cursor-pointer space-y-2"
          @click="router.push(`/comics/read/${item.chapter_id}`)"
        >
          <div class="aspect-[2/3] overflow-hidden rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)] media-shadow">
            <img
              v-if="item.series.cover_url"
              :src="api.comicCoverUrl(item.series.cover_url) ?? ''"
              alt=""
              class="h-full w-full object-cover"
            />
          </div>
          <div class="truncate text-xs font-semibold">{{ item.series.title }}</div>
          <div class="truncate text-[11px] text-[var(--ctp-subtext0)]">
            Ch.{{ item.chapter_number }} · P{{ item.current_page }}/{{ item.page_count }}
          </div>
        </div>
      </div>
    </section>

    <!-- Music Quick Play -->
    <section v-if="recentMusic.length > 0" class="space-y-3">
      <div class="flex items-center justify-between">
        <h2 class="text-base font-semibold">{{ t('dashboard.recentMusic') }}</h2>
        <RouterLink to="/music" class="text-xs text-[var(--ctp-accent)] hover:underline">
          {{ t('common.all') }} →
        </RouterLink>
      </div>
      <div class="grid grid-cols-1 gap-2 sm:grid-cols-2 lg:grid-cols-3">
        <button
          v-for="(tr, idx) in recentMusic"
          :key="tr.id"
          type="button"
          class="flex items-center gap-3 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)] p-2.5 text-left hover:border-[var(--ctp-accent)]"
          @click="musicPlayer.playTracks(recentMusic, idx)"
        >
          <div class="flex h-11 w-11 shrink-0 items-center justify-center overflow-hidden rounded-lg bg-[var(--ctp-surface0)]">
            <img
              v-if="tr.cover_key"
              :src="api.musicCoverUrl(tr.id, 128) ?? ''"
              alt=""
              class="h-full w-full object-cover"
            />
            <LuPlay v-else class="h-4 w-4 text-[var(--ctp-accent)]" />
          </div>
          <div class="min-w-0 flex-1">
            <div class="truncate text-xs font-semibold">{{ tr.title }}</div>
            <div class="truncate text-[11px] text-[var(--ctp-subtext0)]">
              {{ tr.artist_name }} · {{ tr.album_title }}
            </div>
          </div>
        </button>
      </div>
    </section>

    <!-- Open Protocols Card -->
    <section class="rounded-2xl border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)] p-5 space-y-3">
      <h2 class="text-sm font-semibold">{{ t('dashboard.protocolsTitle') }}</h2>
      <div class="grid grid-cols-1 gap-2 text-xs md:grid-cols-3">
        <div v-if="auth.hasModule('book')" class="rounded-xl bg-[var(--ctp-canvas)] p-3 border border-[var(--ctp-surface0)]">
          <div class="font-semibold text-[var(--ctp-accent)]">Book OPDS 1.2 / 2.0</div>
          <code class="mt-1 block truncate text-[11px] text-[var(--ctp-subtext0)]">{{ originUrl }}/opds/book/v1.2</code>
          <code class="block truncate text-[11px] text-[var(--ctp-subtext0)]">{{ originUrl }}/opds/book/v2</code>
        </div>
        <div v-if="auth.hasModule('comic')" class="rounded-xl bg-[var(--ctp-canvas)] p-3 border border-[var(--ctp-surface0)]">
          <div class="font-semibold text-[var(--ctp-accent)]">Comic OPDS 1.2 + PSE 1.1</div>
          <code class="mt-1 block truncate text-[11px] text-[var(--ctp-subtext0)]">{{ originUrl }}/opds/comic/v1.2</code>
        </div>
        <div v-if="auth.hasModule('music')" class="rounded-xl bg-[var(--ctp-canvas)] p-3 border border-[var(--ctp-surface0)]">
          <div class="font-semibold text-[var(--ctp-accent)]">OpenSubsonic 1.16.1</div>
          <code class="mt-1 block truncate text-[11px] text-[var(--ctp-subtext0)]">{{ originUrl }}</code>
        </div>
      </div>
    </section>
  </div>
</template>
