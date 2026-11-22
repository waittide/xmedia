<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { LuGlobe, LuMusic } from 'vue-icons-plus/lu'
import MdSegmentedButton from '@/components/ui/MdSegmentedButton.vue'
import RefreshButton from '@/components/ui/RefreshButton.vue'
import { api } from '../api'
import AlbumCard from '../components/AlbumCard.vue'
import CoverImage from '../components/CoverImage.vue'
import OnlineMusicSearchSection from '../components/OnlineMusicSearchSection.vue'
import TrackTable from '../components/TrackTable.vue'
import { useMusicI18n as useI18n } from '../i18n'
import { useMusicStore } from '../store'
import type { SearchResultDto } from '../types'

type SearchScope = 'local' | 'online'

const { t } = useI18n()
const route = useRoute()
const router = useRouter()
const musicStore = useMusicStore()

const searchScope = ref<SearchScope>(route.query.scope === 'online' ? 'online' : 'local')
const onlineSourceId = computed(() =>
  typeof route.query.source === 'string' && route.query.source.trim()
    ? route.query.source.trim()
    : null,
)
let syncingRouteQuery = false

const searchScopeOptions = computed(
  () =>
    [
      { value: 'local', label: t('online.scopeLocal'), icon: LuMusic },
      { value: 'online', label: t('online.scopeOnline'), icon: LuGlobe },
    ] as const,
)

const queryText = ref('')
const result = ref<SearchResultDto>({ artists: [], albums: [], tracks: [] })
const loading = ref(false)

async function runSearch(): Promise<void> {
  if (searchScope.value !== 'local') return
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

function syncSearchRoute(nextScope: SearchScope, kw?: string, sourceId?: string | null): void {
  const nextQuery: Record<string, string> = {}
  const qVal = (kw !== undefined ? kw : musicStore.globalSearchQuery || queryText.value).trim()
  if (qVal) nextQuery.q = qVal
  if (nextScope === 'online') {
    nextQuery.scope = 'online'
    const src = sourceId !== undefined ? sourceId : onlineSourceId.value
    if (src) nextQuery.source = src
  }
  syncingRouteQuery = true
  void router.replace({ path: '/music/search', query: nextQuery }).finally(() => {
    syncingRouteQuery = false
  })
}

function switchSearchScope(next: SearchScope): void {
  if (next === searchScope.value) return
  searchScope.value = next
  syncSearchRoute(next)
  if (next === 'local') {
    void runSearch()
  }
}

function handleOnlineKeywordUpdate(kw: string): void {
  if (musicStore.globalSearchQuery !== kw) {
    musicStore.globalSearchQuery = kw
  }
  syncSearchRoute('online', kw)
}

function handleOnlineSourceUpdate(sourceId: string | null): void {
  syncSearchRoute('online', undefined, sourceId)
}

onMounted(() => {
  if (route.query.scope === 'online') {
    searchScope.value = 'online'
    const q = typeof route.query.q === 'string' ? route.query.q : ''
    if (musicStore.globalSearchQuery !== q) {
      musicStore.globalSearchQuery = q
    }
  } else {
    searchScope.value = 'local'
    void runSearch()
  }
})

watch(
  () => [route.query.scope, route.query.q, route.query.source] as const,
  ([qScope, qVal], prev) => {
    if (syncingRouteQuery) return
    const nextScope: SearchScope = qScope === 'online' ? 'online' : 'local'
    const scopeChanged = nextScope !== searchScope.value
    searchScope.value = nextScope
    if (nextScope === 'online') {
      if (qVal !== prev?.[1] || scopeChanged) {
        const nextQ = typeof qVal === 'string' ? qVal : ''
        if (musicStore.globalSearchQuery !== nextQ) {
          musicStore.globalSearchQuery = nextQ
        }
      }
      return
    }
    if (scopeChanged || qVal !== prev?.[1]) {
      void runSearch()
    }
  },
)
</script>

<template>
  <div class="mx-auto max-w-7xl space-y-8 px-6 py-7 sm:px-8">
    <div class="flex flex-wrap items-center justify-between gap-4">
      <h1 class="text-2xl font-extrabold tracking-tight text-[var(--md-sys-color-on-surface)]">
        {{
          searchScope === 'online'
            ? t('online.scopeOnline')
            : t('catalog.searchResultsFor', { q: queryText })
        }}
      </h1>

      <div class="flex flex-wrap items-center gap-2">
        <MdSegmentedButton
          :model-value="searchScope"
          :options="searchScopeOptions"
          size="sm"
          :show-check="false"
          @update:model-value="switchSearchScope($event)"
        />
        <RefreshButton v-if="searchScope === 'local'" size="md" :action="runSearch" :busy="loading" />
      </div>
    </div>

    <OnlineMusicSearchSection
      v-if="searchScope === 'online'"
      :keyword="musicStore.globalSearchQuery"
      :initial-source-id="onlineSourceId"
      @update:keyword="handleOnlineKeywordUpdate"
      @update:source-id="handleOnlineSourceUpdate"
    />

    <template v-else>
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
    </template>
  </div>
</template>
