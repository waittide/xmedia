<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import { useComicI18n as useI18n } from '@/domains/comic/i18n'
import RefreshButton from '@/components/ui/RefreshButton.vue'
import { api } from '@/domains/comic/api'
import { useSidebarStore } from '@/domains/comic/store'
import type { TaxonomyOverviewDto } from '@/domains/comic/types'

const { t } = useI18n()
const router = useRouter()
const sidebarStore = useSidebarStore()
const data = ref<TaxonomyOverviewDto | null>(null)

async function loadTaxonomy(): Promise<void> {
  data.value = await api.taxonomyOverview()
}

async function refreshPage(): Promise<void> {
  await Promise.all([loadTaxonomy(), sidebarStore.refresh()])
}

onMounted(loadTaxonomy)
</script>

<template>
  <div class="mx-auto max-w-6xl space-y-6 p-6 sm:p-8">
    <div class="flex items-center justify-end">
      <RefreshButton :action="refreshPage" />
    </div>

    <div
      v-if="
        data &&
        data.genres.length === 0 &&
        data.tags.length === 0 &&
        data.writers.length === 0 &&
        data.pencillers.length === 0
      "
      class="rounded-3xl border border-dashed border-[var(--border-subtle)] bg-[var(--bg-surface)]/50 py-20 text-center text-sm text-[var(--text-secondary)]"
    >
      {{ t('tags.empty') }}
    </div>

    <template v-if="data">
      <section v-if="data.genres.length > 0" class="space-y-3">
        <h2 class="text-base font-bold text-[var(--text-primary)]">{{ t('tags.genresSection') }}</h2>
        <div class="flex flex-wrap gap-2.5">
          <button
            v-for="item in data.genres"
            :key="item.name"
            type="button"
            class="inline-flex items-center gap-2 rounded-xl border border-[var(--border-subtle)] bg-[var(--bg-surface)] px-3.5 py-2 text-sm font-medium text-[var(--text-primary)] transition hover:border-[var(--accent)] hover:text-[var(--accent)]"
            @click="router.push({ path: '/comic/series', query: { genre: item.name } })"
          >
            <span>{{ item.name }}</span>
            <span class="rounded-md bg-[var(--bg-mantle)] px-1.5 py-0.5 text-xs text-[var(--text-secondary)]">
              {{ item.count }}
            </span>
          </button>
        </div>
      </section>

      <section v-if="data.tags.length > 0" class="space-y-3">
        <h2 class="text-base font-bold text-[var(--text-primary)]">{{ t('tags.tagsSection') }}</h2>
        <div class="flex flex-wrap gap-2.5">
          <button
            v-for="item in data.tags"
            :key="item.name"
            type="button"
            class="inline-flex items-center gap-2 rounded-xl border border-[var(--border-subtle)] bg-[var(--bg-surface)] px-3.5 py-2 text-sm font-medium text-[var(--text-primary)] transition hover:border-[var(--accent)] hover:text-[var(--accent)]"
            @click="router.push({ path: '/comic/series', query: { tag: item.name } })"
          >
            <span>#{{ item.name }}</span>
            <span class="rounded-md bg-[var(--bg-mantle)] px-1.5 py-0.5 text-xs text-[var(--text-secondary)]">
              {{ item.count }}
            </span>
          </button>
        </div>
      </section>

      <section v-if="data.writers.length > 0" class="space-y-3">
        <h2 class="text-base font-bold text-[var(--text-primary)]">{{ t('tags.writersSection') }}</h2>
        <div class="flex flex-wrap gap-2.5">
          <button
            v-for="item in data.writers"
            :key="item.name"
            type="button"
            class="inline-flex items-center gap-2 rounded-xl border border-[var(--border-subtle)] bg-[var(--bg-surface)] px-3.5 py-2 text-sm font-medium text-[var(--text-primary)] transition hover:border-[var(--accent)] hover:text-[var(--accent)]"
            @click="router.push({ path: '/comic/series', query: { writer: item.name } })"
          >
            <span>{{ item.name }}</span>
            <span class="rounded-md bg-[var(--bg-mantle)] px-1.5 py-0.5 text-xs text-[var(--text-secondary)]">
              {{ item.count }}
            </span>
          </button>
        </div>
      </section>

      <section v-if="data.pencillers.length > 0" class="space-y-3">
        <h2 class="text-base font-bold text-[var(--text-primary)]">{{ t('tags.pencillersSection') }}</h2>
        <div class="flex flex-wrap gap-2.5">
          <button
            v-for="item in data.pencillers"
            :key="item.name"
            type="button"
            class="inline-flex items-center gap-2 rounded-xl border border-[var(--border-subtle)] bg-[var(--bg-surface)] px-3.5 py-2 text-sm font-medium text-[var(--text-primary)] transition hover:border-[var(--accent)] hover:text-[var(--accent)]"
            @click="router.push({ path: '/comic/series', query: { penciller: item.name } })"
          >
            <span>{{ item.name }}</span>
            <span class="rounded-md bg-[var(--bg-mantle)] px-1.5 py-0.5 text-xs text-[var(--text-secondary)]">
              {{ item.count }}
            </span>
          </button>
        </div>
      </section>
    </template>
  </div>
</template>
