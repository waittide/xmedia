<script setup lang="ts">
import { onMounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { useI18n } from 'vue-i18n'
import { LuHeart, LuLayers, LuSearch } from 'vue-icons-plus/lu'
import { api } from '@/api/client'
import { useEventsStore } from '@/stores/events'
import type { SeriesSummaryDto } from '@/types/api'

const { t } = useI18n()
const router = useRouter()
const events = useEventsStore()

const seriesList = ref<SeriesSummaryDto[]>([])
const search = ref('')
const favOnly = ref(false)

async function loadSeries(): Promise<void> {
  const params: Record<string, string> = {}
  if (search.value.trim()) params.search = search.value.trim()
  if (favOnly.value) params.favorite_only = 'true'
  seriesList.value = await api.listComicSeries(params)
}

onMounted(loadSeries)
watch(() => events.changeTick.comic, loadSeries)
</script>

<template>
  <div class="mx-auto max-w-7xl space-y-6 p-6">
    <div class="flex flex-wrap items-center justify-between gap-3">
      <div class="relative flex-1 min-w-60 max-w-md">
        <LuSearch class="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-[var(--ctp-subtext0)]" />
        <input
          v-model="search"
          :placeholder="t('common.search')"
          class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)] pl-9 pr-4 py-2 text-sm outline-none focus:border-[var(--ctp-accent)]"
          @keyup.enter="loadSeries"
        />
      </div>

      <button
        type="button"
        class="inline-flex items-center gap-1.5 rounded-xl border px-3 py-2 text-xs transition"
        :class="
          favOnly
            ? 'border-[var(--ctp-accent)] bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)] font-semibold'
            : 'border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)] text-[var(--ctp-subtext1)]'
        "
        @click="favOnly = !favOnly; loadSeries()"
      >
        <LuHeart class="h-3.5 w-3.5" /> {{ t('common.favorites') }}
      </button>
    </div>

    <div v-if="seriesList.length === 0" class="py-20 text-center text-sm text-[var(--ctp-subtext0)]">
      {{ t('common.noItems') }}
    </div>

    <div v-else class="grid grid-cols-2 gap-5 sm:grid-cols-3 md:grid-cols-5 lg:grid-cols-6">
      <div
        v-for="s in seriesList"
        :key="s.id"
        class="group cursor-pointer space-y-2"
        @click="router.push(`/comics/series/${s.id}`)"
      >
        <div class="relative aspect-[2/3] overflow-hidden rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)] media-shadow">
          <img
            v-if="s.cover_url"
            :src="api.comicCoverUrl(s.cover_url) ?? ''"
            alt=""
            class="h-full w-full object-cover transition group-hover:scale-105"
          />
          <div v-else class="flex h-full w-full items-center justify-center">
            <LuLayers class="h-10 w-10 text-[var(--ctp-subtext0)]" />
          </div>
          <span class="absolute right-2 top-2 rounded bg-black/70 px-1.5 py-0.5 text-[10px] font-semibold text-white">
            {{ s.completed_chapters }}/{{ s.chapter_count }}
          </span>
        </div>
        <div class="truncate text-xs font-semibold">{{ s.title }}</div>
      </div>
    </div>
  </div>
</template>
