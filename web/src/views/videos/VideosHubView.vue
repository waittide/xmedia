<script setup lang="ts">
import { onMounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { useI18n } from 'vue-i18n'
import { LuClapperboard, LuSearch } from 'vue-icons-plus/lu'
import { api } from '@/api/client'
import { useEventsStore } from '@/stores/events'
import type { ShowSummaryDto, VideoSummaryDto } from '@/types/api'

const { t } = useI18n()
const router = useRouter()
const events = useEventsStore()

type VideoTab = 'videos' | 'shows'
const tab = ref<VideoTab>('videos')
const search = ref('')
const videos = ref<VideoSummaryDto[]>([])
const shows = ref<ShowSummaryDto[]>([])

async function loadData(): Promise<void> {
  const q = search.value.trim()
  if (tab.value === 'videos') {
    videos.value = await api.listVideos(q ? { search: q } : {})
  } else {
    shows.value = await api.listShows(q ? { search: q } : {})
  }
}

onMounted(loadData)
watch(tab, loadData)
watch(() => events.changeTick.video, loadData)
</script>

<template>
  <div class="mx-auto max-w-7xl space-y-6 p-6">
    <div class="flex flex-wrap items-center justify-between gap-3">
      <div class="flex gap-1.5 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)] p-1 text-xs">
        <button
          type="button"
          class="rounded-lg px-3 py-1.5 transition"
          :class="tab === 'videos' ? 'bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)] font-semibold' : 'text-[var(--ctp-subtext1)]'"
          @click="tab = 'videos'"
        >
          Movies & All Videos
        </button>
        <button
          type="button"
          class="rounded-lg px-3 py-1.5 transition"
          :class="tab === 'shows' ? 'bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)] font-semibold' : 'text-[var(--ctp-subtext1)]'"
          @click="tab = 'shows'"
        >
          TV & Anime Shows
        </button>
      </div>

      <div class="relative min-w-60 max-w-sm flex-1">
        <LuSearch class="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-[var(--ctp-subtext0)]" />
        <input
          v-model="search"
          :placeholder="t('common.search')"
          class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)] pl-9 pr-4 py-2 text-sm outline-none focus:border-[var(--ctp-accent)]"
          @keyup.enter="loadData"
        />
      </div>
    </div>

    <!-- Videos Grid -->
    <div v-if="tab === 'videos'" class="grid grid-cols-1 gap-5 sm:grid-cols-2 md:grid-cols-3 lg:grid-cols-4">
      <div
        v-for="v in videos"
        :key="v.id"
        class="group cursor-pointer overflow-hidden rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)] transition hover:border-[var(--ctp-accent)]"
        @click="router.push(`/videos/watch/${v.id}`)"
      >
        <div class="relative aspect-video bg-[var(--ctp-surface0)] flex items-center justify-center overflow-hidden">
          <img
            v-if="v.poster_key"
            :src="api.videoPosterUrl(v.poster_key) ?? ''"
            alt=""
            class="h-full w-full object-cover transition group-hover:scale-105"
          />
          <LuClapperboard v-else class="h-10 w-10 text-[var(--ctp-subtext0)]" />

          <span
            v-if="v.resolution"
            class="absolute right-2 top-2 rounded bg-black/75 px-1.5 py-0.5 text-[10px] font-semibold text-white"
          >
            {{ v.resolution }}
          </span>

          <div
            v-if="v.progress && v.duration_secs > 0"
            class="absolute inset-x-0 bottom-0 h-1 bg-black/50"
          >
            <div
              class="h-full bg-[var(--ctp-accent)]"
              :style="{ width: `${Math.min(100, (v.progress.position_secs / v.duration_secs) * 100)}%` }"
            />
          </div>
        </div>

        <div class="p-3">
          <div class="truncate text-sm font-semibold">{{ v.title }}</div>
          <div class="truncate text-xs text-[var(--ctp-subtext0)]">
            {{ v.show_title ? `${v.show_title} · S${v.season_number ?? 1}E${v.episode_number ?? 1}` : v.container.toUpperCase() }}
            <span v-if="v.year"> · {{ v.year }}</span>
          </div>
        </div>
      </div>
    </div>

    <!-- Shows Grid -->
    <div v-else class="grid grid-cols-2 gap-5 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5">
      <div
        v-for="s in shows"
        :key="s.id"
        class="group cursor-pointer overflow-hidden rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)] transition hover:border-[var(--ctp-accent)]"
        @click="router.push(`/videos/shows/${s.id}`)"
      >
        <div class="aspect-video bg-[var(--ctp-surface0)] overflow-hidden">
          <img
            v-if="s.poster_key"
            :src="api.videoPosterUrl(s.poster_key) ?? ''"
            alt=""
            class="h-full w-full object-cover transition group-hover:scale-105"
          />
        </div>
        <div class="p-3">
          <div class="truncate text-sm font-semibold">{{ s.title }}</div>
          <div class="text-xs text-[var(--ctp-subtext0)]">
            {{ s.watched_episode_count }}/{{ s.episode_count }} episodes
          </div>
        </div>
      </div>
    </div>
  </div>
</template>
