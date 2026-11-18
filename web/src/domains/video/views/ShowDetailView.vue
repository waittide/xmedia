<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { LuPlay, LuHeart, LuPencil } from 'vue-icons-plus/lu'
import { useAuthStore } from '@/stores/auth'
import RefreshButton from '@/components/ui/RefreshButton.vue'
import { api } from '../api'
import PosterImage from '../components/PosterImage.vue'
import ShowMetadataModal from '../components/ShowMetadataModal.vue'
import VideoCard from '../components/VideoCard.vue'
import VideoMetadataModal from '../components/VideoMetadataModal.vue'
import { useVideoI18n as useI18n } from '../i18n'
import { useVideoStore } from '../store'
import type { ShowDetailDto, VideoSummaryDto } from '../types'

const { t } = useI18n()
const route = useRoute()
const router = useRouter()
const authStore = useAuthStore()
const videoStore = useVideoStore()

const detail = ref<ShowDetailDto | null>(null)
const selectedSeason = ref<number | null>(null)
const editShowOpen = ref(false)
const editingEpisode = ref<VideoSummaryDto | null>(null)

async function loadShow(): Promise<void> {
  const id = String(route.params.id || '')
  if (!id) return
  const res = await api.getShow(id)
  detail.value = res
  if (
    res.seasons.length > 0 &&
    (selectedSeason.value === null || !res.seasons.some((s) => s.season_number === selectedSeason.value))
  ) {
    selectedSeason.value = res.seasons[0]!.season_number
  }
}

const activeSeasonGroup = computed(() => {
  if (!detail.value) return null
  return (
    detail.value.seasons.find((s) => s.season_number === selectedSeason.value) ??
    detail.value.seasons[0] ??
    null
  )
})

const primaryPlayEpisode = computed<VideoSummaryDto | null>(() => {
  if (!detail.value) return null
  if (detail.value.continue_episode) return detail.value.continue_episode
  return detail.value.seasons[0]?.episodes[0] ?? null
})

const primaryPlayLabel = computed(() => {
  const ep = detail.value?.continue_episode
  if (ep && ep.season_number != null && ep.episode_number != null) {
    return t('catalog.continueEpisodeBtn', {
      s: String(ep.season_number).padStart(2, '0'),
      e: String(ep.episode_number).padStart(2, '0'),
    })
  }
  return t('catalog.startFirstEpisodeBtn')
})

onMounted(loadShow)
watch(() => route.params.id, loadShow)
</script>

<template>
  <div v-if="detail" class="mx-auto max-w-6xl space-y-8 px-6 py-8 sm:px-8">
    <!-- Show Header -->
    <div class="flex flex-col items-start gap-6 sm:flex-row sm:items-end">
      <PosterImage
        :poster-key="detail.show.poster_key"
        :size="540"
        :alt="detail.show.title"
        aspect="poster"
        rounded="rounded-3xl"
        class="book-shadow w-40 shrink-0 sm:w-48"
      />

      <div class="min-w-0 flex-1 space-y-3">
        <div class="flex flex-wrap gap-1.5">
          <span
            v-for="g in [...detail.show.genres, ...detail.show.tags]"
            :key="g"
            class="rounded-full bg-[var(--ctp-accent-soft)] px-2.5 py-0.5 text-[11px] font-bold text-[var(--ctp-accent)]"
          >
            {{ g }}
          </span>
        </div>

        <h1 class="text-2xl font-extrabold tracking-tight sm:text-3xl">
          {{ detail.show.title }}
        </h1>

        <p class="text-xs text-[var(--ctp-subtext1)]">
          <span v-if="detail.show.year">{{ detail.show.year }} · </span>
          <span>{{ t('common.seasonsCount', { count: detail.show.season_count }) }} · </span>
          <span>
            {{
              t('common.watchedProgress', {
                watched: detail.show.watched_episode_count,
                total: detail.show.episode_count,
              })
            }}
          </span>
        </p>

        <p
          v-if="detail.show.summary"
          class="max-w-3xl text-xs leading-relaxed text-[var(--ctp-subtext0)]"
        >
          {{ detail.show.summary }}
        </p>

        <div class="flex flex-wrap items-center gap-2.5 pt-2">
          <button
            v-if="primaryPlayEpisode"
            type="button"
            class="inline-flex items-center gap-2 rounded-full bg-[var(--ctp-accent)] px-5 py-2.5 text-xs font-bold text-[var(--ctp-on-accent)] shadow-md transition hover:opacity-95"
            @click="router.push(`/video/watch/${primaryPlayEpisode.id}`)"
          >
            <LuPlay class="h-4 w-4 fill-current" />
            <span>{{ primaryPlayLabel }}</span>
          </button>

          <button
            type="button"
            class="inline-flex h-9 w-9 items-center justify-center rounded-full border border-[var(--ctp-surface0)] bg-[var(--ctp-shell)] transition hover:bg-[var(--ctp-surface0)]"
            @click="videoStore.toggleShowFavorite(detail.show)"
          >
            <LuHeart
              class="h-4 w-4"
              :class="
                detail.show.is_favorite
                  ? 'fill-[var(--ctp-red)] text-[var(--ctp-red)]'
                  : 'text-[var(--ctp-subtext0)]'
              "
            />
          </button>

          <button
            v-if="authStore.isAdmin"
            type="button"
            class="inline-flex items-center gap-1.5 rounded-full border border-[var(--ctp-surface0)] bg-[var(--ctp-shell)] px-3.5 py-2 text-xs font-medium text-[var(--ctp-subtext1)] transition hover:text-[var(--ctp-text)]"
            @click="editShowOpen = true"
          >
            <LuPencil class="h-3.5 w-3.5" />
            <span>{{ t('common.editMetadata') }}</span>
          </button>

          <RefreshButton size="md" :action="loadShow" />
        </div>
      </div>
    </div>

    <!-- Season Selector & Episodes -->
    <div v-if="detail.seasons.length > 0" class="space-y-4">
      <div v-if="detail.seasons.length > 1" class="flex flex-wrap gap-1.5 rounded-2xl bg-[var(--ctp-shell)] p-1.5 w-fit">
        <button
          v-for="sg in detail.seasons"
          :key="sg.season_number"
          type="button"
          class="rounded-xl px-4 py-1.5 text-xs font-semibold transition"
          :class="
            selectedSeason === sg.season_number
              ? 'bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)]'
              : 'text-[var(--ctp-subtext1)] hover:text-[var(--ctp-text)]'
          "
          @click="selectedSeason = sg.season_number"
        >
          {{
            sg.season_number === 0
              ? t('catalog.specialsHeader')
              : t('catalog.seasonHeader', { season: sg.season_number })
          }}
          ({{ sg.episodes.length }})
        </button>
      </div>

      <div
        v-if="activeSeasonGroup"
        class="grid grid-cols-1 gap-3.5 sm:grid-cols-2 md:grid-cols-3 lg:grid-cols-4"
      >
        <VideoCard
          v-for="ep in activeSeasonGroup.episodes"
          :key="ep.id"
          :video="ep"
          @edit="editingEpisode = $event"
          @deleted="loadShow"
          @updated="loadShow"
        />
      </div>
    </div>

    <ShowMetadataModal
      :open="editShowOpen"
      :show="detail.show"
      @close="editShowOpen = false"
      @saved="loadShow"
    />

    <VideoMetadataModal
      :open="editingEpisode !== null"
      :video="editingEpisode"
      @close="editingEpisode = null"
      @saved="loadShow"
    />
  </div>
</template>
