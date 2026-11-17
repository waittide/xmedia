<script setup lang="ts">
import { useRouter } from 'vue-router'
import { LuHeart, LuTv } from 'vue-icons-plus/lu'
import { useVideoI18n as useI18n } from '../i18n'
import { useVideoStore } from '../store'
import type { ShowSummaryDto } from '../types'
import PosterImage from './PosterImage.vue'

defineProps<{
  show: ShowSummaryDto
}>()

const { t } = useI18n()
const router = useRouter()
const videoStore = useVideoStore()
</script>

<template>
  <div
    class="group flex cursor-pointer flex-col rounded-2xl p-2.5 transition hover:bg-[var(--ctp-mantle)]"
    @click="router.push(`/video/shows/${show.id}`)"
  >
    <div class="relative mb-2.5">
      <PosterImage
        :poster-key="show.poster_key"
        :size="480"
        :alt="show.title"
        aspect="poster"
        rounded="rounded-2xl"
        class="book-shadow"
      >
        <div class="pointer-events-none absolute top-2 left-2 flex items-center gap-1">
          <span
            class="inline-flex items-center gap-1 rounded-md bg-black/65 px-2 py-0.5 text-[10px] font-bold text-white backdrop-blur-xs"
          >
            <LuTv class="h-3 w-3" />
            {{ t('common.watchedProgress', { watched: show.watched_episode_count, total: show.episode_count }) }}
          </span>
        </div>

        <button
          type="button"
          class="absolute top-2 right-2 flex h-7 w-7 items-center justify-center rounded-full bg-black/50 text-white opacity-0 backdrop-blur-xs transition group-hover:opacity-100 hover:scale-105"
          @click.stop="videoStore.toggleShowFavorite(show)"
        >
          <LuHeart
            class="h-3.5 w-3.5"
            :class="show.is_favorite ? 'fill-[var(--ctp-red)] text-[var(--ctp-red)]' : ''"
          />
        </button>
      </PosterImage>
    </div>

    <h3 class="truncate text-xs font-bold text-[var(--ctp-text)]">
      {{ show.title }}
    </h3>
    <div class="mt-0.5 flex items-center justify-between gap-1 text-[11px] text-[var(--ctp-subtext0)]">
      <span>
        {{ t('common.seasonsCount', { count: show.season_count }) }} ·
        {{ t('common.episodesCount', { count: show.episode_count }) }}
      </span>
      <span v-if="show.year" class="shrink-0">{{ show.year }}</span>
    </div>
  </div>
</template>
