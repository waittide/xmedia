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
  <div class="mx-auto max-w-7xl space-y-6 px-6 py-7 sm:px-8">
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
      class="md-card-outlined rounded-3xl border-dashed py-20 text-center text-sm text-[var(--md-sys-color-on-surface-variant)]"
    >
      {{ t('tags.empty') }}
    </div>

    <template v-if="data">
      <section v-if="data.genres.length > 0" class="space-y-3">
        <h2 class="text-base font-bold text-[var(--md-sys-color-on-surface)]">
          {{ t('tags.genresSection') }}
        </h2>
        <div class="flex flex-wrap gap-2.5">
          <button
            v-for="item in data.genres"
            :key="item.name"
            type="button"
            class="md-chip md-state !h-9 !rounded-xl !bg-[var(--md-sys-color-surface-container-low)] !px-3.5 text-sm !text-[var(--md-sys-color-on-surface)] hover:!border-[var(--md-sys-color-primary)] hover:!text-[var(--md-sys-color-primary)]"
            @click="router.push({ path: '/comic/series', query: { genre: item.name } })"
          >
            <span>{{ item.name }}</span>
            <span
              class="rounded-md bg-[var(--md-sys-color-surface-container-high)] px-1.5 py-0.5 text-xs text-[var(--md-sys-color-on-surface-variant)]"
            >
              {{ item.count }}
            </span>
          </button>
        </div>
      </section>

      <section v-if="data.tags.length > 0" class="space-y-3">
        <h2 class="text-base font-bold text-[var(--md-sys-color-on-surface)]">
          {{ t('tags.tagsSection') }}
        </h2>
        <div class="flex flex-wrap gap-2.5">
          <button
            v-for="item in data.tags"
            :key="item.name"
            type="button"
            class="md-chip md-state !h-9 !rounded-xl !bg-[var(--md-sys-color-surface-container-low)] !px-3.5 text-sm !text-[var(--md-sys-color-on-surface)] hover:!border-[var(--md-sys-color-primary)] hover:!text-[var(--md-sys-color-primary)]"
            @click="router.push({ path: '/comic/series', query: { tag: item.name } })"
          >
            <span>#{{ item.name }}</span>
            <span
              class="rounded-md bg-[var(--md-sys-color-surface-container-high)] px-1.5 py-0.5 text-xs text-[var(--md-sys-color-on-surface-variant)]"
            >
              {{ item.count }}
            </span>
          </button>
        </div>
      </section>

      <section v-if="data.writers.length > 0" class="space-y-3">
        <h2 class="text-base font-bold text-[var(--md-sys-color-on-surface)]">
          {{ t('tags.writersSection') }}
        </h2>
        <div class="flex flex-wrap gap-2.5">
          <button
            v-for="item in data.writers"
            :key="item.name"
            type="button"
            class="md-chip md-state !h-9 !rounded-xl !bg-[var(--md-sys-color-surface-container-low)] !px-3.5 text-sm !text-[var(--md-sys-color-on-surface)] hover:!border-[var(--md-sys-color-primary)] hover:!text-[var(--md-sys-color-primary)]"
            @click="router.push({ path: '/comic/series', query: { writer: item.name } })"
          >
            <span>{{ item.name }}</span>
            <span
              class="rounded-md bg-[var(--md-sys-color-surface-container-high)] px-1.5 py-0.5 text-xs text-[var(--md-sys-color-on-surface-variant)]"
            >
              {{ item.count }}
            </span>
          </button>
        </div>
      </section>

      <section v-if="data.pencillers.length > 0" class="space-y-3">
        <h2 class="text-base font-bold text-[var(--md-sys-color-on-surface)]">
          {{ t('tags.pencillersSection') }}
        </h2>
        <div class="flex flex-wrap gap-2.5">
          <button
            v-for="item in data.pencillers"
            :key="item.name"
            type="button"
            class="md-chip md-state !h-9 !rounded-xl !bg-[var(--md-sys-color-surface-container-low)] !px-3.5 text-sm !text-[var(--md-sys-color-on-surface)] hover:!border-[var(--md-sys-color-primary)] hover:!text-[var(--md-sys-color-primary)]"
            @click="router.push({ path: '/comic/series', query: { penciller: item.name } })"
          >
            <span>{{ item.name }}</span>
            <span
              class="rounded-md bg-[var(--md-sys-color-surface-container-high)] px-1.5 py-0.5 text-xs text-[var(--md-sys-color-on-surface-variant)]"
            >
              {{ item.count }}
            </span>
          </button>
        </div>
      </section>
    </template>
  </div>
</template>
