<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { LuArrowLeft, LuCheck, LuPlay } from 'vue-icons-plus/lu'
import { api } from '@/api/client'
import type { ShowDetailDto } from '@/types/api'

const route = useRoute()
const router = useRouter()
const showId = String(route.params.id)
const detail = ref<ShowDetailDto | null>(null)

onMounted(async () => {
  detail.value = await api.getShow(showId)
})
</script>

<template>
  <div v-if="detail" class="mx-auto max-w-5xl space-y-6 p-6">
    <button
      type="button"
      class="inline-flex items-center gap-1.5 text-xs text-[var(--ctp-subtext1)] hover:text-[var(--ctp-text)]"
      @click="router.push('/videos')"
    >
      <LuArrowLeft class="h-4 w-4" /> Back to Videos
    </button>

    <div class="flex flex-col gap-6 rounded-2xl border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)] p-6 sm:flex-row">
      <div class="w-56 shrink-0 aspect-video overflow-hidden rounded-xl bg-[var(--ctp-surface0)]">
        <img
          v-if="detail.show.poster_key"
          :src="api.videoPosterUrl(detail.show.poster_key) ?? ''"
          alt=""
          class="h-full w-full object-cover"
        />
      </div>
      <div class="space-y-3">
        <h1 class="text-2xl font-bold">{{ detail.show.title }}</h1>
        <p class="text-xs text-[var(--ctp-subtext0)]">
          {{ detail.show.season_count }} season(s) · {{ detail.show.watched_episode_count }}/{{ detail.show.episode_count }} watched
        </p>
        <button
          v-if="detail.continue_episode"
          type="button"
          class="inline-flex items-center gap-2 rounded-xl bg-[var(--ctp-accent)] px-4 py-2 text-xs font-semibold text-[var(--ctp-on-accent)]"
          @click="router.push(`/videos/watch/${detail.continue_episode.id}`)"
        >
          <LuPlay class="h-4 w-4" />
          Play S{{ detail.continue_episode.season_number ?? 1 }}E{{ detail.continue_episode.episode_number ?? 1 }} · {{ detail.continue_episode.title }}
        </button>
      </div>
    </div>

    <div v-for="season in detail.seasons" :key="season.season_number" class="space-y-2">
      <h2 class="text-sm font-semibold text-[var(--ctp-subtext1)]">Season {{ season.season_number }}</h2>
      <div class="divide-y divide-[var(--ctp-surface0)] rounded-2xl border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)]">
        <button
          v-for="ep in season.episodes"
          :key="ep.id"
          type="button"
          class="flex w-full items-center justify-between p-4 text-left text-sm hover:bg-[var(--ctp-surface0)]/40"
          @click="router.push(`/videos/watch/${ep.id}`)"
        >
          <div>
            <span class="font-semibold">E{{ ep.episode_number ?? 1 }}</span>
            <span class="ml-2">{{ ep.title }}</span>
          </div>
          <div class="flex items-center gap-2 text-xs text-[var(--ctp-subtext0)]">
            <LuCheck v-if="ep.progress?.completed" class="h-4 w-4 text-[var(--ctp-green)]" />
            <span v-if="ep.resolution">{{ ep.resolution }}</span>
          </div>
        </button>
      </div>
    </div>
  </div>
</template>
