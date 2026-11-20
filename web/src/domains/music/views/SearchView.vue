<script setup lang="ts">
import { onMounted, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import RefreshButton from '@/components/ui/RefreshButton.vue'
import { api } from '../api'
import AlbumCard from '../components/AlbumCard.vue'
import CoverImage from '../components/CoverImage.vue'
import TrackTable from '../components/TrackTable.vue'
import { useMusicI18n as useI18n } from '../i18n'
import type { SearchResultDto } from '../types'

const { t } = useI18n()
const route = useRoute()
const router = useRouter()

const queryText = ref('')
const result = ref<SearchResultDto>({ artists: [], albums: [], tracks: [] })
const loading = ref(false)

async function runSearch(): Promise<void> {
  const q = String(route.query.q || '').trim()
  queryText.value = q
  if (!q) {
    result.value = { artists: [], albums: [], tracks: [] }
    return
  }
  loading.value = true
  try {
    result.value = await api.search(q, 24)
  } finally {
    loading.value = false
  }
}

onMounted(runSearch)
watch(() => route.query.q, runSearch)
</script>

<template>
  <div class="mx-auto max-w-6xl space-y-8 px-6 py-7 sm:px-8">
    <div class="flex flex-wrap items-center justify-between gap-4">
      <h1 class="text-2xl font-extrabold tracking-tight text-[var(--md-sys-color-on-surface)]">
        {{ t('catalog.searchResultsFor', { q: queryText }) }}
      </h1>

      <RefreshButton size="md" :action="runSearch" :busy="loading" />
    </div>

    <div
      v-if="
        !loading &&
        result.artists.length === 0 &&
        result.albums.length === 0 &&
        result.tracks.length === 0
      "
      class="py-16 text-center text-xs text-[var(--md-sys-color-on-surface-variant)]"
    >
      {{ t('common.noResults') }}
    </div>

    <section v-if="result.artists.length > 0">
      <h2 class="mb-3 text-base font-bold text-[var(--md-sys-color-on-surface)]">
        {{ t('nav.artists') }}
      </h2>
      <div class="grid grid-cols-2 gap-3 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-6">
        <div
          v-for="ar in result.artists"
          :key="ar.id"
          class="flex cursor-pointer flex-col items-center rounded-2xl p-3 text-center transition hover:bg-[var(--md-sys-color-surface-container)]"
          @click="router.push(`/music/artists/${ar.id}`)"
        >
          <CoverImage
            :cover-id="ar.id"
            :has-cover="Boolean(ar.cover_key)"
            :size="240"
            rounded="rounded-full"
            class="mb-2.5 w-3/4"
          />
          <p class="w-full truncate text-xs font-bold text-[var(--md-sys-color-on-surface)]">
            {{ ar.name }}
          </p>
        </div>
      </div>
    </section>

    <section v-if="result.albums.length > 0">
      <h2 class="mb-3 text-base font-bold text-[var(--md-sys-color-on-surface)]">
        {{ t('nav.albums') }}
      </h2>
      <div class="grid grid-cols-2 gap-3 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-6">
        <AlbumCard v-for="al in result.albums" :key="al.id" :album="al" />
      </div>
    </section>

    <section v-if="result.tracks.length > 0">
      <h2 class="mb-3 text-base font-bold text-[var(--md-sys-color-on-surface)]">
        {{ t('nav.songs') }}
      </h2>
      <TrackTable :tracks="result.tracks" />
    </section>
  </div>
</template>
