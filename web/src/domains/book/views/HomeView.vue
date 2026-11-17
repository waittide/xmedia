<script setup lang="ts">
import { computed, onActivated, onMounted, ref, watch, type Component } from 'vue'
import { useRouter } from 'vue-router'
import { useI18n as useGlobalI18n } from 'vue-i18n'
import { useBookI18n as useI18n } from '@/domains/book/i18n'
import {
  LuArrowRight,
  LuBookmark,
  LuBookOpen,
  LuClock,
  LuFolder,
  LuHeart,
  LuLibrary,
  LuPlay,
  LuSlidersHorizontal,
  LuTag,
  LuUser,
} from 'vue-icons-plus/lu'
import { fetchHomeOverview, peekHomeCache } from '@/config/homeCache'
import { api } from '@/domains/book/api'
import { useCatalogStore } from '@/domains/book/store'
import { useThemeStore } from '@/stores/theme'
import { useUploadStore } from '@/stores/uploads'
import type { BookHomeOverviewDto, BookSummaryDto } from '@/domains/book/types'
import BookCard from '@/domains/book/components/BookCard.vue'
import HomeSkeleton from '@/components/HomeSkeleton.vue'
import LayoutCustomizerModal from '@/components/LayoutCustomizerModal.vue'

defineOptions({ name: 'BookHomeView' })

const { t } = useI18n()
const { t: globalT } = useGlobalI18n()
const router = useRouter()
const catalogStore = useCatalogStore()
const themeStore = useThemeStore()
const uploadStore = useUploadStore()

const overview = ref<BookHomeOverviewDto | null>(peekHomeCache('book'))
const loading = ref<boolean>(overview.value === null)
const customizerOpen = ref<boolean>(false)

async function loadHome(force = false): Promise<void> {
  if (!overview.value) loading.value = true
  try {
    overview.value = await fetchHomeOverview('book', { force })
  } finally {
    loading.value = false
  }
}

onMounted(() => {
  void loadHome(false)
})

onActivated(() => {
  void loadHome(false)
})

watch(
  () => [uploadStore.versionCounter, catalogStore.scanRefreshVersion],
  () => {
    void loadHome(true)
  },
)

function goSection(sec: 'all' | 'recent' | 'favorites'): void {
  catalogStore.selectSection(sec)
  void router.push('/book/catalog')
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
  const keys = themeStore.workspaceLayout.book.home_cards
  return keys.map((key) => {
    const label = globalT(`layoutCustomizer.cards.book.${key}`)
    switch (key) {
      case 'reading':
        return {
          key,
          label,
          value: st?.reading_count ?? 0,
          icon: LuClock,
          onClick: () => goSection('recent'),
        }
      case 'bookmarks':
        return {
          key,
          label,
          value: st?.bookmarks_count ?? 0,
          icon: LuBookmark,
          onClick: () => void router.push('/book/bookmarks'),
        }
      case 'favorites':
        return {
          key,
          label,
          value: st?.favorites_count ?? 0,
          icon: LuHeart,
          tone: 'rose',
          onClick: () => goSection('favorites'),
        }
      case 'shelves':
        return {
          key,
          label,
          value: st?.shelves_count ?? catalogStore.shelves.length,
          icon: LuFolder,
          onClick: () => void router.push('/book/shelves'),
        }
      case 'tags':
        return {
          key,
          label,
          value: st?.tags_count ?? catalogStore.tags.length,
          icon: LuTag,
          onClick: () => void router.push('/book/tags'),
        }
      case 'authors':
        return {
          key,
          label,
          value: st?.authors_count ?? catalogStore.authors.length,
          icon: LuUser,
          onClick: () => void router.push('/book/authors'),
        }
      case 'libraries':
        return {
          key,
          label,
          value: st?.libraries_count ?? catalogStore.libraries.length,
          icon: LuLibrary,
          onClick: () => void router.push('/book/libraries'),
        }
      case 'all':
      default:
        return {
          key: 'all',
          label,
          value: st?.total_books ?? 0,
          icon: LuBookOpen,
          onClick: () => goSection('all'),
        }
    }
  })
})

const cardsGridClass = computed(() => {
  const n = statCards.value.length
  if (n === 1) return 'grid-cols-1 sm:grid-cols-2'
  if (n === 2) return 'grid-cols-1 sm:grid-cols-2'
  if (n === 3) return 'grid-cols-1 sm:grid-cols-3'
  return 'grid-cols-2 lg:grid-cols-4'
})

function formatResumeSubtitle(book: BookSummaryDto): string {
  const prog = book.progress
  const percent = Math.round(prog?.progress_percent ?? 0)
  if (!prog) return t('home.resumePercent', { percent })

  if (prog.page_number != null && prog.page_number > 0) {
    if (book.metadata.page_count && book.metadata.page_count > 0) {
      return t('home.resumePage', {
        page: prog.page_number,
        total: book.metadata.page_count,
        percent,
      })
    }
    return t('home.resumePageSimple', { page: prog.page_number, percent })
  }

  if (prog.chapter_index != null) {
    const ch = prog.chapter_index + 1
    if (book.metadata.chapter_count && book.metadata.chapter_count > 0) {
      return t('home.resumeChapter', {
        chapter: ch,
        total: book.metadata.chapter_count,
        percent,
      })
    }
    return t('home.resumeChapterSimple', { chapter: ch, percent })
  }

  return t('home.resumePercent', { percent })
}
</script>

<template>
  <div class="mx-auto max-w-7xl space-y-10 px-6 py-8 sm:px-8">
    <!-- Customizable Library Stat Cards (1..=4) -->
    <section class="space-y-2.5">
      <div class="flex items-center justify-end">
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
      v-else-if="!loading && overview && overview.stats.total_books === 0"
      class="card-soft flex flex-col items-center justify-center rounded-3xl bg-[var(--ctp-shell)] px-6 py-16 text-center"
    >
      <div
        class="mb-4 flex h-14 w-14 items-center justify-center rounded-2xl bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]"
      >
        <LuBookOpen class="h-7 w-7" />
      </div>
      <h2 class="text-lg font-bold">{{ t('home.welcomeEmptyTitle') }}</h2>
      <p class="mt-1.5 max-w-md text-xs leading-relaxed text-[var(--ctp-subtext0)]">
        {{ t('home.welcomeEmptyDesc') }}
      </p>
    </section>

    <!-- Continue Reading Section -->
    <section v-if="overview && overview.continue_reading.length > 0">
      <div class="mb-5 flex items-end justify-between">
        <div>
          <h2 class="text-lg font-bold tracking-tight text-[var(--ctp-text)]">
            {{ t('home.continueReading') }}
          </h2>
          <p class="mt-1 text-xs text-[var(--ctp-subtext0)]">
            {{ t('home.continueSubtitle') }}
          </p>
        </div>
        <button
          type="button"
          class="inline-flex items-center gap-1 text-xs font-semibold text-[var(--ctp-accent)] hover:underline"
          @click="goSection('recent')"
        >
          <span>{{ t('home.seeAll') }}</span>
          <LuArrowRight class="h-3.5 w-3.5" />
        </button>
      </div>

      <div class="grid grid-cols-1 gap-4 md:grid-cols-2 xl:grid-cols-3">
        <RouterLink
          v-for="book in overview.continue_reading"
          :key="book.id"
          :to="`/book/read/${book.id}`"
          class="group flex items-center gap-4 rounded-2xl border border-[var(--ctp-surface0)]/85 bg-[var(--ctp-shell)]/90 p-3.5 transition duration-200 hover:-translate-y-0.5 hover:border-[var(--ctp-accent)]/60 hover:bg-[var(--ctp-mantle)]"
        >
          <div
            class="relative h-28 w-20 shrink-0 overflow-hidden rounded-xl bg-[var(--ctp-mantle)] shadow-md"
          >
            <img
              v-if="book.has_cover"
              :src="api.coverUrl(book.id, book.updated_at)"
              :alt="book.metadata.title"
              loading="lazy"
              decoding="async"
              class="h-full w-full object-cover transition duration-300 group-hover:scale-105"
            />
            <div v-else class="flex h-full w-full flex-col justify-between p-2 text-left">
              <span
                class="self-end rounded bg-[var(--ctp-accent-soft)] px-1.5 py-0.5 text-[9px] font-bold uppercase text-[var(--ctp-accent)]"
              >
                {{ book.format }}
              </span>
              <p class="line-clamp-2 text-[11px] font-bold leading-tight text-[var(--ctp-text)]">
                {{ book.metadata.title }}
              </p>
            </div>
          </div>

          <div class="flex min-w-0 flex-1 flex-col justify-center">
            <h3
              class="truncate text-sm font-bold text-[var(--ctp-text)] transition-colors group-hover:text-[var(--ctp-accent)]"
            >
              {{ book.metadata.title }}
            </h3>
            <p class="mt-1.5 truncate text-xs text-[var(--ctp-subtext0)]">
              {{ formatResumeSubtitle(book) }}
            </p>

            <div class="mt-4 flex items-center gap-3">
              <div class="h-1.5 flex-1 overflow-hidden rounded-full bg-[var(--ctp-canvas)]/90">
                <div
                  class="h-full rounded-full bg-[var(--ctp-accent)] transition-all duration-300"
                  :style="{
                    width: `${Math.max(3, Math.min(100, book.progress?.progress_percent ?? 0))}%`,
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

    <!-- Recently Added Section -->
    <section v-if="overview && overview.recently_added.length > 0">
      <div class="mb-5 flex items-end justify-between">
        <div>
          <h2 class="text-lg font-bold tracking-tight text-[var(--ctp-text)]">
            {{ t('home.recentlyAdded') }}
          </h2>
          <p class="mt-1 text-xs text-[var(--ctp-subtext0)]">
            {{ t('home.recentlySubtitle') }}
          </p>
        </div>
        <button
          type="button"
          class="inline-flex items-center gap-1 text-xs font-semibold text-[var(--ctp-accent)] hover:underline"
          @click="goSection('all')"
        >
          <span>{{ t('home.seeAll') }}</span>
          <LuArrowRight class="h-3.5 w-3.5" />
        </button>
      </div>

      <div class="grid grid-cols-2 gap-6 sm:grid-cols-3 lg:grid-cols-4 xl:grid-cols-5">
        <BookCard v-for="book in overview.recently_added" :key="book.id" :book="book" />
      </div>
    </section>

    <LayoutCustomizerModal
      :open="customizerOpen"
      domain="book"
      initial-tab="cards"
      @close="customizerOpen = false"
    />
  </div>
</template>
