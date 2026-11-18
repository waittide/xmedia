<script setup lang="ts">
import { computed, onActivated, onMounted, ref, watch, type Component } from 'vue'
import { useRouter } from 'vue-router'
import { useI18n as useGlobalI18n } from 'vue-i18n'
import { useComicI18n as useI18n } from '@/domains/comic/i18n'
import {
  LuArrowRight,
  LuBookmark,
  LuBookMarked,
  LuBookOpen,
  LuClock,
  LuFolder,
  LuHeart,
  LuLayers,
  LuLibrary,
  LuPlay,
  LuSlidersHorizontal,
  LuTag,
  LuUpload,
} from 'vue-icons-plus/lu'
import { fetchHomeOverview, peekHomeCache } from '@/config/homeCache'
import { withAuthUrl } from '@/domains/comic/api'
import { useSidebarStore } from '@/domains/comic/store'
import { useThemeStore } from '@/stores/theme'
import { useUploadStore } from '@/stores/uploads'
import SeriesCard from '@/domains/comic/components/SeriesCard.vue'
import HomeSkeleton from '@/components/HomeSkeleton.vue'
import LayoutCustomizerModal from '@/components/LayoutCustomizerModal.vue'
import RefreshButton from '@/components/ui/RefreshButton.vue'
import type { HomeOverviewDto } from '@/domains/comic/types'

defineOptions({ name: 'ComicHomeView' })

const { t } = useI18n()
const { t: globalT } = useGlobalI18n()
const router = useRouter()
const sidebarStore = useSidebarStore()
const themeStore = useThemeStore()
const upload = useUploadStore()

const overview = ref<HomeOverviewDto | null>(peekHomeCache('comic'))
const loading = ref(overview.value === null)
const customizerOpen = ref(false)
const fileInputRef = ref<HTMLInputElement | null>(null)

async function loadHome(force = false): Promise<void> {
  if (!overview.value) loading.value = true
  try {
    overview.value = await fetchHomeOverview('comic', { force })
  } finally {
    loading.value = false
  }
}

async function refreshPage(): Promise<void> {
  await Promise.all([loadHome(true), sidebarStore.refresh()])
}

onMounted(() => void loadHome(false))
onActivated(() => void loadHome(false))
watch(() => upload.catalogVersion, () => void loadHome(true))

function onPickUpload(ev: Event): void {
  const input = ev.target as HTMLInputElement
  if (input.files && input.files.length > 0) {
    upload.enqueueFiles(input.files)
    input.value = ''
  }
}

interface StatCardItem {
  key: string
  label: string
  value: number
  icon: Component
  tone?: 'accent' | 'rose'
  onClick: () => void
}

const statCards = computed<StatCardItem[]>(() => {
  const st = overview.value?.stats
  const keys = themeStore.workspaceLayout.comic.home_cards
  return keys.map((key) => {
    const label = globalT(`layoutCustomizer.cards.comic.${key}`)
    switch (key) {
      case 'reading':
        return {
          key,
          label,
          value: st?.in_progress_count ?? 0,
          icon: LuClock,
          onClick: () => void router.push('/comic/continue'),
        }
      case 'bookmarks':
        return {
          key,
          label,
          value: st?.bookmarks_count ?? 0,
          icon: LuBookmark,
          onClick: () => void router.push('/comic/bookmarks'),
        }
      case 'favorites':
        return {
          key,
          label,
          value: st?.favorites_count ?? 0,
          icon: LuHeart,
          tone: 'rose',
          onClick: () => void router.push('/comic/favorites'),
        }
      case 'chapters':
        return {
          key,
          label,
          value: st?.chapter_count ?? 0,
          icon: LuBookMarked,
          onClick: () => void router.push('/comic/series'),
        }
      case 'shelves':
        return {
          key,
          label,
          value: st?.shelves_count ?? sidebarStore.shelves.length,
          icon: LuFolder,
          onClick: () => void router.push('/comic/shelves'),
        }
      case 'tags':
        return {
          key,
          label,
          value: st?.tags_count ?? sidebarStore.tags.length,
          icon: LuTag,
          onClick: () => void router.push('/comic/tags'),
        }
      case 'libraries':
        return {
          key,
          label,
          value: sidebarStore.libraries.length,
          icon: LuLibrary,
          onClick: () => void router.push('/comic/libraries'),
        }
      case 'all':
      default:
        return {
          key: 'all',
          label,
          value: st?.series_count ?? 0,
          icon: LuLayers,
          onClick: () => void router.push('/comic/series'),
        }
    }
  })
})

const cardsGridClass = computed(() => {
  const n = statCards.value.length
  if (n <= 2) return 'grid-cols-1 sm:grid-cols-2'
  if (n === 3) return 'grid-cols-1 sm:grid-cols-3'
  return 'grid-cols-2 lg:grid-cols-4'
})
</script>

<template>
  <div class="mx-auto max-w-7xl space-y-10 px-6 py-8 sm:px-8">
    <!-- Customizable Comic Stat Cards (1..=4) -->
    <section class="space-y-2.5">
      <div class="flex items-center justify-end gap-1.5">
        <RefreshButton variant="ghost" :action="refreshPage" />
        <button
          type="button"
          class="inline-flex items-center gap-1.5 rounded-lg px-2 py-1 text-[11px] font-medium text-[var(--ctp-subtext0)] transition hover:bg-[var(--ctp-shell)] hover:text-[var(--ctp-text)]"
          @click="customizerOpen = true"
        >
          <LuSlidersHorizontal class="h-3.5 w-3.5" />
          <span>{{ globalT('layoutCustomizer.customizeCards') }}</span>
        </button>
      </div>

      <div class="grid gap-4" :class="cardsGridClass">
        <button
          v-for="card in statCards"
          :key="card.key"
          type="button"
          class="card-soft flex items-center gap-3.5 rounded-2xl bg-[var(--ctp-shell)] p-4 text-left transition hover:border-[var(--ctp-accent)]"
          @click="card.onClick()"
        >
          <div
            class="flex h-10 w-10 shrink-0 items-center justify-center rounded-xl"
            :class="
              card.tone === 'rose'
                ? 'bg-[var(--ctp-red)]/15 text-[var(--ctp-red)]'
                : 'bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]'
            "
          >
            <component :is="card.icon" class="h-5 w-5" />
          </div>
          <div class="min-w-0">
            <p class="text-xl font-bold leading-none text-[var(--ctp-text)]">{{ card.value }}</p>
            <p class="mt-1 truncate text-xs text-[var(--ctp-subtext0)]">{{ card.label }}</p>
          </div>
        </button>
      </div>
    </section>

    <!-- Cold-Load Skeleton -->
    <HomeSkeleton v-if="loading && !overview" />

    <!-- Empty Welcome State -->
    <section
      v-else-if="!loading && overview && overview.stats.series_count === 0"
      class="card-soft flex flex-col items-center justify-center rounded-3xl bg-[var(--ctp-shell)] px-6 py-16 text-center"
    >
      <div
        class="mb-4 flex h-14 w-14 items-center justify-center rounded-2xl bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]"
      >
        <LuBookOpen class="h-7 w-7" />
      </div>
      <h2 class="text-lg font-bold text-[var(--ctp-text)]">{{ t('home.emptyTitle') }}</h2>
      <p class="mt-1.5 max-w-md text-xs leading-relaxed text-[var(--ctp-subtext0)]">
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
          class="inline-flex items-center gap-2 rounded-xl bg-[var(--ctp-accent)] px-4 py-2 text-xs font-semibold text-[var(--ctp-on-accent)] shadow-sm transition hover:opacity-90"
          @click="fileInputRef?.click()"
        >
          <LuUpload class="h-4 w-4" />
          <span>{{ t('home.uploadNow') }}</span>
        </button>
        <button
          type="button"
          class="inline-flex items-center gap-2 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-4 py-2 text-xs font-medium text-[var(--ctp-text)] transition hover:border-[var(--ctp-accent)]"
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
          <h2 class="text-lg font-bold tracking-tight text-[var(--ctp-text)]">
            {{ t('home.continueReading') }}
          </h2>
          <p class="mt-1 text-xs text-[var(--ctp-subtext0)]">{{ t('home.continueSubtitle') }}</p>
        </div>
        <RouterLink
          to="/comic/continue"
          class="inline-flex items-center gap-1 text-xs font-semibold text-[var(--ctp-accent)] hover:underline"
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
          class="group flex items-center gap-4 rounded-2xl border border-[var(--ctp-surface0)]/85 bg-[var(--ctp-shell)]/90 p-3.5 transition duration-200 hover:-translate-y-0.5 hover:border-[var(--ctp-accent)]/60 hover:bg-[var(--ctp-mantle)]"
        >
          <div
            class="relative h-28 w-20 shrink-0 overflow-hidden rounded-xl bg-[var(--ctp-mantle)] shadow-md"
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
              class="flex h-full w-full items-center justify-center p-2 text-center text-[11px] font-bold text-[var(--ctp-text)]"
            >
              {{ item.series.title }}
            </div>
          </div>

          <div class="flex min-w-0 flex-1 flex-col justify-center">
            <h3
              class="truncate text-sm font-bold text-[var(--ctp-text)] transition-colors group-hover:text-[var(--ctp-accent)]"
            >
              {{ item.series.title }}
            </h3>
            <p class="mt-1.5 truncate text-xs text-[var(--ctp-subtext0)]">
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
              <div class="h-1.5 flex-1 overflow-hidden rounded-full bg-[var(--ctp-canvas)]/90">
                <div
                  class="h-full rounded-full bg-[var(--ctp-accent)] transition-all duration-300"
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
                class="flex h-7 w-7 shrink-0 items-center justify-center rounded-full bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)] shadow-sm transition group-hover:scale-105"
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
          <h2 class="text-lg font-bold tracking-tight text-[var(--ctp-text)]">
            {{ t('home.recentlyUpdated') }}
          </h2>
          <p class="mt-1 text-xs text-[var(--ctp-subtext0)]">
            {{ t('home.recentlyUpdatedSubtitle') }}
          </p>
        </div>
        <RouterLink
          to="/comic/series"
          class="inline-flex items-center gap-1 text-xs font-semibold text-[var(--ctp-accent)] hover:underline"
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

    <LayoutCustomizerModal
      :open="customizerOpen"
      domain="comic"
      initial-tab="cards"
      @close="customizerOpen = false"
    />
  </div>
</template>
