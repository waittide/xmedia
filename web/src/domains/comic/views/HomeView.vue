<script setup lang="ts">
import { computed, ref } from 'vue'
import { useRouter } from 'vue-router'
import { useI18n as useGlobalI18n } from 'vue-i18n'
import { useComicI18n as useI18n } from '@/domains/comic/i18n'
import {
  LuArrowRight,
  LuBookmark,
  LuBookMarked,
  LuBookOpen,
  LuClock,
  LuCompass,
  LuFolder,
  LuHeart,
  LuLayers,
  LuLibrary,
  LuPlay,
  LuTag,
  LuUpload,
} from 'vue-icons-plus/lu'
import { useHomeOverview } from '@/composables/useHomeOverview'
import { withAuthUrl } from '@/domains/comic/api'
import { useSidebarStore } from '@/domains/comic/store'
import { usePreferencesStore } from '@/stores/preferences'
import { useUploadStore } from '@/stores/uploads'
import SeriesCard from '@/domains/comic/components/SeriesCard.vue'
import HomeSkeleton from '@/components/HomeSkeleton.vue'
import HomeStatCards, { type StatCardItem } from '@/components/domain/HomeStatCards.vue'

defineOptions({ name: 'ComicHomeView' })

const { t } = useI18n()
const { t: globalT } = useGlobalI18n()
const router = useRouter()
const sidebarStore = useSidebarStore()
const prefsStore = usePreferencesStore()
const upload = useUploadStore()

const fileInputRef = ref<HTMLInputElement | null>(null)

const { overview, loading, refreshPage } = useHomeOverview('comic', {
  refreshSidebar: () => sidebarStore.refresh(),
  watchSources: () => upload.catalogVersion,
})

function onPickUpload(ev: Event): void {
  const input = ev.target as HTMLInputElement
  if (input.files && input.files.length > 0) {
    upload.enqueueFiles(input.files)
    input.value = ''
  }
}

const statCards = computed<StatCardItem[]>(() => {
  const st = overview.value?.stats
  return prefsStore.workspaceLayout.comic.home_cards.map((key) => {
    switch (key) {
      case 'reading':
        return {
          key,
          value: st?.in_progress_count ?? 0,
          icon: LuClock,
          onClick: () => void router.push('/comic/continue'),
        }
      case 'bookmarks':
        return {
          key,
          value: st?.bookmarks_count ?? 0,
          icon: LuBookmark,
          onClick: () => void router.push('/comic/bookmarks'),
        }
      case 'favorites':
        return {
          key,
          value: st?.favorites_count ?? 0,
          icon: LuHeart,
          tone: 'rose',
          onClick: () => void router.push('/comic/favorites'),
        }
      case 'chapters':
        return {
          key,
          value: st?.chapter_count ?? 0,
          icon: LuBookMarked,
          onClick: () => void router.push('/comic/series'),
        }
      case 'shelves':
        return {
          key,
          value: st?.shelves_count ?? sidebarStore.shelves.length,
          icon: LuFolder,
          onClick: () => void router.push('/comic/shelves'),
        }
      case 'tags':
        return {
          key,
          value: st?.tags_count ?? sidebarStore.tags.length,
          icon: LuTag,
          onClick: () => void router.push('/comic/tags'),
        }
      case 'libraries':
        return {
          key,
          value: sidebarStore.libraries.length,
          icon: LuLibrary,
          onClick: () => void router.push('/comic/libraries'),
        }
      case 'all':
      default:
        return {
          key: 'all',
          value: st?.series_count ?? 0,
          icon: LuLayers,
          onClick: () => void router.push('/comic/series'),
        }
    }
  })
})
</script>

<template>
  <div class="mx-auto max-w-7xl space-y-10 px-6 py-7 sm:px-8">
    <HomeStatCards
      v-if="overview && overview.stats.series_count > 0"
      domain="comic"
      :cards="statCards"
      :on-refresh="refreshPage"
    />

    <!-- Cold-Load Skeleton -->
    <HomeSkeleton v-if="loading && !overview" />

    <!-- Empty Welcome State -->
    <section
      v-else-if="!loading && overview && overview.stats.series_count === 0"
      class="md-card-filled flex flex-col items-center justify-center rounded-3xl px-6 py-16 text-center"
    >
      <div
        class="mb-4 flex h-14 w-14 items-center justify-center rounded-2xl bg-[var(--md-sys-color-primary-container)] text-[var(--md-sys-color-on-primary-container)]"
      >
        <LuBookOpen class="h-7 w-7" />
      </div>
      <h2 class="text-lg font-bold text-[var(--md-sys-color-on-surface)]">
        {{ t('home.emptyTitle') }}
      </h2>
      <p class="mt-1.5 max-w-md text-xs leading-relaxed text-[var(--md-sys-color-on-surface-variant)]">
        {{ t('home.emptyDesc') }}
      </p>
      <div class="mt-6 flex flex-wrap items-center justify-center gap-3">
        <input
          ref="fileInputRef"
          type="file"
          multiple
          accept=".cbz,.zip"
          class="hidden"
          @change="onPickUpload"
        />
        <button
          type="button"
          class="md-btn md-btn-filled md-state"
          @click="fileInputRef?.click()"
        >
          <LuUpload class="h-4 w-4" />
          <span>{{ t('home.uploadNow') }}</span>
        </button>
        <RouterLink
          :to="{ path: '/comic/series', query: { scope: 'online' } }"
          class="md-btn md-btn-tonal md-state"
        >
          <LuCompass class="h-4 w-4" />
          <span>{{ globalT('nav.browseOnlineSources') }}</span>
        </RouterLink>
        <button
          type="button"
          class="md-btn md-btn-outlined md-state"
          @click="router.push('/comic/libraries')"
        >
          <LuLibrary class="h-4 w-4" />
          <span>{{ t('home.manageLibraries') }}</span>
        </button>
      </div>
    </section>

    <!-- Continue Reading Section (Unified Horizontal Cards) -->
    <section v-if="overview && overview.continue_reading.length > 0">
      <div class="mb-5 flex items-end justify-between">
        <div>
          <h2 class="text-lg font-bold tracking-tight text-[var(--md-sys-color-on-surface)]">
            {{ t('home.continueReading') }}
          </h2>
          <p class="mt-1 text-xs text-[var(--md-sys-color-on-surface-variant)]">
            {{ t('home.continueSubtitle') }}
          </p>
        </div>
        <RouterLink
          to="/comic/continue"
          class="inline-flex items-center gap-1 text-xs font-semibold text-[var(--md-sys-color-primary)] hover:underline"
        >
          <span>{{ t('home.viewAll') }}</span>
          <LuArrowRight class="h-3.5 w-3.5" />
        </RouterLink>
      </div>

      <div class="grid grid-cols-1 gap-4 md:grid-cols-2 xl:grid-cols-3">
        <RouterLink
          v-for="item in overview.continue_reading.slice(0, 6)"
          :key="item.series.id"
          :to="`/comic/read/${encodeURIComponent(item.pointer.chapter_id)}?page=${item.pointer.current_page}`"
          class="group md-card-filled flex items-center gap-4 border border-transparent p-3.5 transition duration-200 hover:-translate-y-0.5 hover:border-[var(--md-sys-color-outline-variant)] hover:bg-[var(--md-sys-color-surface-container)] hover:shadow-[var(--md-sys-elevation-1)]"
        >
          <div
            class="relative h-28 w-20 shrink-0 overflow-hidden rounded-xl bg-[var(--md-sys-color-surface-container)] shadow-[var(--md-sys-elevation-1)]"
          >
            <img
              v-if="item.series.cover_url"
              :src="withAuthUrl(item.series.cover_url)"
              :alt="item.series.title"
              loading="lazy"
              decoding="async"
              class="h-full w-full object-cover transition duration-300 group-hover:scale-105"
            />
            <div
              v-else
              class="flex h-full w-full items-center justify-center p-2 text-center text-[11px] font-bold text-[var(--md-sys-color-on-surface-variant)]"
            >
              {{ item.series.title }}
            </div>
          </div>

          <div class="flex min-w-0 flex-1 flex-col justify-center">
            <h3
              class="truncate text-sm font-bold text-[var(--md-sys-color-on-surface)] transition-colors group-hover:text-[var(--md-sys-color-primary)]"
            >
              {{ item.series.title }}
            </h3>
            <p class="mt-1.5 truncate text-xs text-[var(--md-sys-color-on-surface-variant)]">
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
            </p>

            <div class="mt-4 flex items-center gap-3">
              <div
                class="h-1.5 flex-1 overflow-hidden rounded-full bg-[var(--md-sys-color-surface-container-highest)]"
              >
                <div
                  class="h-full rounded-full bg-[var(--md-sys-color-primary)] transition-all duration-300"
                  :style="{
                    width: `${Math.max(
                      3,
                      Math.min(
                        100,
                        Math.round(
                          (item.pointer.current_page / Math.max(1, item.pointer.total_pages)) * 100,
                        ),
                      ),
                    )}%`,
                  }"
                />
              </div>
              <span
                class="flex h-7 w-7 shrink-0 items-center justify-center rounded-full bg-[var(--md-sys-color-primary)] text-[var(--md-sys-color-on-primary)] shadow-[var(--md-sys-elevation-1)] transition group-hover:scale-105"
              >
                <LuPlay class="ml-0.5 h-3.5 w-3.5 fill-current" />
              </span>
            </div>
          </div>
        </RouterLink>
      </div>
    </section>

    <!-- Recently Updated Section -->
    <section v-if="overview && overview.recently_updated.length > 0">
      <div class="mb-5 flex items-end justify-between">
        <div>
          <h2 class="text-lg font-bold tracking-tight text-[var(--md-sys-color-on-surface)]">
            {{ t('home.recentlyUpdated') }}
          </h2>
          <p class="mt-1 text-xs text-[var(--md-sys-color-on-surface-variant)]">
            {{ t('home.recentlyUpdatedSubtitle') }}
          </p>
        </div>
        <RouterLink
          to="/comic/series"
          class="inline-flex items-center gap-1 text-xs font-semibold text-[var(--md-sys-color-primary)] hover:underline"
        >
          <span>{{ t('home.viewAll') }}</span>
          <LuArrowRight class="h-3.5 w-3.5" />
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
