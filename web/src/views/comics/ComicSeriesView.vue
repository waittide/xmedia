<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { LuArrowLeft, LuHeart, LuPlay } from 'vue-icons-plus/lu'
import { api } from '@/api/client'
import type { SeriesDetailDto } from '@/types/api'

const route = useRoute()
const router = useRouter()
const seriesId = String(route.params.id)
const detail = ref<SeriesDetailDto | null>(null)

onMounted(async () => {
  detail.value = await api.getComicSeries(seriesId)
})

async function toggleFav(): Promise<void> {
  if (!detail.value) return
  const next = !detail.value.is_favorite
  await api.setComicFavorite(seriesId, next)
  detail.value.is_favorite = next
}
</script>

<template>
  <div v-if="detail" class="mx-auto max-w-5xl space-y-6 p-6">
    <button
      type="button"
      class="inline-flex items-center gap-1.5 text-xs text-[var(--ctp-subtext1)] hover:text-[var(--ctp-text)]"
      @click="router.push('/comics')"
    >
      <LuArrowLeft class="h-4 w-4" /> Back to Comics
    </button>

    <div class="flex flex-col gap-6 rounded-2xl border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)] p-6 sm:flex-row">
      <div class="w-40 shrink-0 aspect-[2/3] overflow-hidden rounded-xl bg-[var(--ctp-surface0)] media-shadow">
        <img
          v-if="detail.cover_url"
          :src="api.comicCoverUrl(detail.cover_url) ?? ''"
          alt=""
          class="h-full w-full object-cover"
        />
      </div>

      <div class="flex-1 space-y-3">
        <h1 class="text-2xl font-bold">{{ detail.series.title }}</h1>
        <p v-if="detail.series.summary" class="text-sm text-[var(--ctp-subtext1)]">
          {{ detail.series.summary }}
        </p>
        <div class="flex flex-wrap gap-2 pt-2">
          <button
            v-if="detail.continue_reading"
            type="button"
            class="inline-flex items-center gap-2 rounded-xl bg-[var(--ctp-accent)] px-4 py-2 text-xs font-semibold text-[var(--ctp-on-accent)]"
            @click="router.push(`/comics/read/${detail.continue_reading.chapter_id}`)"
          >
            <LuPlay class="h-4 w-4" />
            Continue Ch.{{ detail.continue_reading.chapter_number }} (P{{ detail.continue_reading.current_page }})
          </button>
          <button
            type="button"
            class="inline-flex items-center gap-1.5 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-2 text-xs"
            @click="toggleFav"
          >
            <LuHeart class="h-4 w-4" :class="detail.is_favorite ? 'fill-current text-[var(--ctp-red)]' : ''" />
            Favorite
          </button>
        </div>
      </div>
    </div>

    <!-- Chapters List -->
    <div class="divide-y divide-[var(--ctp-surface0)] rounded-2xl border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)]">
      <button
        v-for="ch in detail.chapters"
        :key="ch.id"
        type="button"
        class="flex w-full items-center justify-between p-4 text-left text-sm hover:bg-[var(--ctp-surface0)]/40"
        @click="router.push(`/comics/read/${ch.id}`)"
      >
        <div>
          <span class="font-semibold">Ch.{{ ch.number_label }}</span>
          <span v-if="ch.title" class="ml-2 text-[var(--ctp-subtext1)]">{{ ch.title }}</span>
        </div>
        <div class="text-xs text-[var(--ctp-subtext0)]">
          <span v-if="ch.is_completed" class="text-[var(--ctp-green)]">Completed</span>
          <span v-else-if="ch.current_page > 1">P{{ ch.current_page }}/{{ ch.page_count }}</span>
          <span v-else>{{ ch.page_count }} pages</span>
        </div>
      </button>
    </div>
  </div>
</template>
