<script setup lang="ts">
import { onMounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { useComicI18n as useI18n } from '@/domains/comic/i18n'
import {
  LuBookMarked,
  LuBookOpen,
  LuCheckCheck,
  LuHeart,
  LuLibrary,
  LuPlay,
  LuUpload,
} from 'vue-icons-plus/lu'
import { api, withAuthUrl } from '@/domains/comic/api'
import { useUploadStore } from '@/stores/uploads'
import SeriesCard from '@/domains/comic/components/SeriesCard.vue'
import type { HomeOverviewDto } from '@/domains/comic/types'

const { t } = useI18n()
const router = useRouter()
const upload = useUploadStore()

const overview = ref<HomeOverviewDto | null>(null)
const loading = ref(true)
const fileInputRef = ref<HTMLInputElement | null>(null)

async function loadHome(): Promise<void> {
  loading.value = true
  try {
    overview.value = await api.homeOverview()
  } finally {
    loading.value = false
  }
}

onMounted(loadHome)
watch(() => upload.catalogVersion, loadHome)

function onPickUpload(ev: Event): void {
  const input = ev.target as HTMLInputElement
  if (input.files && input.files.length > 0) {
    upload.enqueueFiles(input.files)
    input.value = ''
  }
}
</script>

<template>
  <div class="mx-auto max-w-7xl space-y-10 p-6 sm:p-8">
    <div
      v-if="overview && overview.stats.series_count > 0"
      class="grid grid-cols-2 gap-3.5 sm:grid-cols-4"
    >
      <div class="flex items-center gap-3.5 rounded-2xl border border-[var(--border-subtle)] bg-[var(--bg-surface)] p-4">
        <div class="flex h-10 w-10 items-center justify-center rounded-xl bg-[var(--accent)]/15 text-[var(--accent)]">
          <LuBookOpen class="h-5 w-5" />
        </div>
        <div>
          <div class="text-xl font-bold text-[var(--text-primary)]">{{ overview.stats.series_count }}</div>
          <div class="text-xs text-[var(--text-secondary)]">{{ t('home.statsSeries') }}</div>
        </div>
      </div>

      <div class="flex items-center gap-3.5 rounded-2xl border border-[var(--border-subtle)] bg-[var(--bg-surface)] p-4">
        <div class="flex h-10 w-10 items-center justify-center rounded-xl bg-[var(--accent)]/15 text-[var(--accent)]">
          <LuBookMarked class="h-5 w-5" />
        </div>
        <div>
          <div class="text-xl font-bold text-[var(--text-primary)]">{{ overview.stats.chapter_count }}</div>
          <div class="text-xs text-[var(--text-secondary)]">{{ t('home.statsChapters') }}</div>
        </div>
      </div>

      <div class="flex items-center gap-3.5 rounded-2xl border border-[var(--border-subtle)] bg-[var(--bg-surface)] p-4">
        <div class="flex h-10 w-10 items-center justify-center rounded-xl bg-emerald-500/15 text-emerald-500">
          <LuCheckCheck class="h-5 w-5" />
        </div>
        <div>
          <div class="text-xl font-bold text-[var(--text-primary)]">{{ overview.stats.completed_chapters }}</div>
          <div class="text-xs text-[var(--text-secondary)]">{{ t('home.statsCompleted') }}</div>
        </div>
      </div>

      <div class="flex items-center gap-3.5 rounded-2xl border border-[var(--border-subtle)] bg-[var(--bg-surface)] p-4">
        <div class="flex h-10 w-10 items-center justify-center rounded-xl bg-rose-500/15 text-rose-500">
          <LuHeart class="h-5 w-5" />
        </div>
        <div>
          <div class="text-xl font-bold text-[var(--text-primary)]">{{ overview.stats.favorites_count }}</div>
          <div class="text-xs text-[var(--text-secondary)]">{{ t('home.statsFavorites') }}</div>
        </div>
      </div>
    </div>

    <div
      v-if="!loading && overview && overview.stats.series_count === 0"
      class="flex flex-col items-center justify-center rounded-3xl border border-dashed border-[var(--border-subtle)] bg-[var(--bg-surface)]/70 px-6 py-20 text-center"
    >
      <div
        class="mb-4 flex h-16 w-16 items-center justify-center rounded-2xl bg-[var(--accent)]/15 text-[var(--accent)]"
      >
        <LuBookOpen class="h-8 w-8" />
      </div>
      <h2 class="text-lg font-bold text-[var(--text-primary)]">{{ t('home.emptyTitle') }}</h2>
      <p class="mt-2 max-w-md text-sm leading-relaxed text-[var(--text-secondary)]">
        {{ t('home.emptyDesc') }}
      </p>
      <div class="mt-6 flex flex-wrap items-center justify-center gap-3">
        <input
          ref="fileInputRef"
          type="file"
          multiple
          accept=".cbz,.zip,.epub"
          class="hidden"
          @change="onPickUpload"
        />
        <button
          type="button"
          class="inline-flex items-center gap-2 rounded-xl bg-[var(--accent)] px-5 py-2.5 text-sm font-semibold text-[var(--accent-contrast)] shadow-sm hover:opacity-90"
          @click="fileInputRef?.click()"
        >
          <LuUpload class="h-4 w-4" />
          <span>{{ t('home.uploadNow') }}</span>
        </button>
        <button
          type="button"
          class="inline-flex items-center gap-2 rounded-xl border border-[var(--border-subtle)] bg-[var(--bg-surface)] px-5 py-2.5 text-sm font-medium text-[var(--text-primary)] hover:bg-[var(--surface-hover)]"
          @click="router.push('/comic/libraries')"
        >
          <LuLibrary class="h-4 w-4" />
          <span>{{ t('home.manageLibraries') }}</span>
        </button>
      </div>
    </div>

    <section v-if="overview && overview.continue_reading.length > 0" class="space-y-4">
      <div class="flex items-end justify-between">
        <div>
          <h2 class="text-base font-bold text-[var(--text-primary)]">
            {{ t('home.continueReading') }}
          </h2>
          <p class="text-xs text-[var(--text-secondary)]">{{ t('home.continueSubtitle') }}</p>
        </div>
        <RouterLink
          to="/comic/continue"
          class="text-xs font-semibold text-[var(--accent)] hover:underline"
        >
          {{ t('home.viewAll') }} →
        </RouterLink>
      </div>

      <div class="flex gap-4 overflow-x-auto pb-2">
        <div
          v-for="item in overview.continue_reading"
          :key="item.series.id"
          class="group flex w-72 shrink-0 cursor-pointer items-center gap-3.5 rounded-2xl border border-[var(--border-subtle)] bg-[var(--bg-surface)] p-3 transition hover:border-[var(--accent)]/60 sm:w-80"
          @click="
            router.push(
              '/comic/read/' + item.pointer.chapter_id + '?page=' + item.pointer.current_page
            )
          "
        >
          <div class="book-shadow h-24 w-16 shrink-0 overflow-hidden rounded-lg bg-[var(--bg-mantle)]">
            <img
              v-if="item.series.cover_url"
              :src="withAuthUrl(item.series.cover_url)"
              :alt="item.series.title"
              class="h-full w-full object-cover"
            />
          </div>
          <div class="min-w-0 flex-1 space-y-1.5">
            <div class="truncate text-sm font-bold text-[var(--text-primary)] group-hover:text-[var(--accent)]">
              {{ item.series.title }}
            </div>
            <div class="truncate text-xs text-[var(--text-secondary)]">
              {{
                item.pointer.is_resuming_mid_chapter
                  ? t('home.resumeAt', {
                      chapter: item.pointer.chapter_number,
                      page: item.pointer.current_page,
                      total: item.pointer.total_pages,
                    })
                  : t('home.nextChapterAt', {
                      chapter: item.pointer.chapter_number,
                      total: item.pointer.total_pages,
                    })
              }}
            </div>
            <div class="flex items-center gap-2 pt-1">
              <div class="h-1.5 flex-1 overflow-hidden rounded-full bg-[var(--bg-mantle)]">
                <div
                  class="h-full rounded-full bg-[var(--accent)]"
                  :style="{
                    width:
                      Math.min(
                        100,
                        Math.round(
                          (item.pointer.current_page / Math.max(1, item.pointer.total_pages)) * 100
                        )
                      ) + '%',
                  }"
                />
              </div>
              <span
                class="flex h-6 w-6 shrink-0 items-center justify-center rounded-full bg-[var(--accent)] text-[var(--accent-contrast)]"
              >
                <LuPlay class="h-3 w-3 fill-current" />
              </span>
            </div>
          </div>
        </div>
      </div>
    </section>

    <section v-if="overview && overview.recently_updated.length > 0" class="space-y-4">
      <div class="flex items-end justify-between">
        <div>
          <h2 class="text-base font-bold text-[var(--text-primary)]">
            {{ t('home.recentlyUpdated') }}
          </h2>
          <p class="text-xs text-[var(--text-secondary)]">
            {{ t('home.recentlyUpdatedSubtitle') }}
          </p>
        </div>
        <RouterLink
          to="/comic/series"
          class="text-xs font-semibold text-[var(--accent)] hover:underline"
        >
          {{ t('home.viewAll') }} →
        </RouterLink>
      </div>

      <div
        class="grid grid-cols-2 gap-5 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 xl:grid-cols-6"
      >
        <SeriesCard v-for="s in overview.recently_updated" :key="s.id" :series="s" />
      </div>
    </section>
  </div>
</template>
