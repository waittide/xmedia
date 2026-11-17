<script setup lang="ts">
import { onMounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { useBookI18n as useI18n } from '@/domains/book/i18n'
import { LuArrowRight, LuBookOpen, LuLibrary, LuFolder, LuPlay } from 'vue-icons-plus/lu'
import { api } from '@/domains/book/api'
import { useCatalogStore } from '@/domains/book/store'
import { useUploadStore } from '@/stores/uploads'
import type { BookSummaryDto } from '@/domains/book/types'
import BookCard from '@/domains/book/components/BookCard.vue'

const { t } = useI18n()
const router = useRouter()
const catalogStore = useCatalogStore()
const uploadStore = useUploadStore()

const recentBooks = ref<BookSummaryDto[]>([])
const latestBooks = ref<BookSummaryDto[]>([])
const totalCount = ref<number>(0)
const loading = ref<boolean>(true)

async function loadHome(): Promise<void> {
  loading.value = true
  try {
    const [recent, latest] = await Promise.all([
      api.listRecent(6),
      api.searchBooks({
        sort_by: 'created_at',
        sort_order: 'desc',
        page: 1,
        page_size: 10,
      }),
    ])
    recentBooks.value = recent
    latestBooks.value = latest.items
    totalCount.value = latest.total
  } finally {
    loading.value = false
  }
}

onMounted(() => {
  void loadHome()
})

watch(
  () => [uploadStore.versionCounter, catalogStore.scanRefreshVersion],
  () => {
    void loadHome()
  },
)

function goSection(sec: 'all' | 'recent'): void {
  catalogStore.selectSection(sec)
  void router.push('/book/catalog')
}

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
  <div class="mx-auto max-w-7xl space-y-11 px-6 py-8 sm:px-8">
    <!-- Subtle Library Stats Strip -->
    <section class="grid grid-cols-3 gap-4">
      <button
        type="button"
        class="card-soft flex items-center gap-3.5 rounded-2xl bg-[var(--ctp-shell)] p-4 text-left transition hover:border-[var(--ctp-accent)]"
        @click="goSection('all')"
      >
        <div class="flex h-10 w-10 shrink-0 items-center justify-center rounded-xl bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]">
          <LuBookOpen class="h-5 w-5" />
        </div>
        <div>
          <p class="text-xl font-bold leading-none">{{ totalCount }}</p>
          <p class="mt-1 text-xs text-[var(--ctp-subtext0)]">{{ t('home.statsTotalBooks') }}</p>
        </div>
      </button>

      <RouterLink
        to="/book/libraries"
        class="card-soft flex items-center gap-3.5 rounded-2xl bg-[var(--ctp-shell)] p-4 text-left transition hover:border-[var(--ctp-accent)]"
      >
        <div class="flex h-10 w-10 shrink-0 items-center justify-center rounded-xl bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]">
          <LuLibrary class="h-5 w-5" />
        </div>
        <div>
          <p class="text-xl font-bold leading-none">{{ catalogStore.libraries.length }}</p>
          <p class="mt-1 text-xs text-[var(--ctp-subtext0)]">{{ t('home.statsLibraries') }}</p>
        </div>
      </RouterLink>

      <RouterLink
        to="/book/shelves"
        class="card-soft flex items-center gap-3.5 rounded-2xl bg-[var(--ctp-shell)] p-4 text-left transition hover:border-[var(--ctp-accent)]"
      >
        <div class="flex h-10 w-10 shrink-0 items-center justify-center rounded-xl bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]">
          <LuFolder class="h-5 w-5" />
        </div>
        <div>
          <p class="text-xl font-bold leading-none">{{ catalogStore.shelves.length }}</p>
          <p class="mt-1 text-xs text-[var(--ctp-subtext0)]">{{ t('home.statsShelves') }}</p>
        </div>
      </RouterLink>
    </section>

    <!-- Empty Welcome State -->
    <section
      v-if="!loading && latestBooks.length === 0"
      class="card-soft flex flex-col items-center justify-center rounded-3xl bg-[var(--ctp-shell)] py-16 px-6 text-center"
    >
      <div class="mb-4 flex h-14 w-14 items-center justify-center rounded-2xl bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]">
        <LuBookOpen class="h-7 w-7" />
      </div>
      <h2 class="text-lg font-bold">{{ t('home.welcomeEmptyTitle') }}</h2>
      <p class="mt-1.5 max-w-md text-xs leading-relaxed text-[var(--ctp-subtext0)]">
        {{ t('home.welcomeEmptyDesc') }}
      </p>
    </section>

    <!-- Continue Reading Section (Horizontal Resume Cards with Progress Track + Play Button) -->
    <section v-if="recentBooks.length > 0">
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
          v-for="book in recentBooks"
          :key="book.id"
          :to="`/book/read/${book.id}`"
          class="group flex items-center gap-4 rounded-2xl border border-[var(--ctp-surface0)]/85 bg-[var(--ctp-shell)]/90 p-3.5 transition duration-200 hover:-translate-y-0.5 hover:border-[var(--ctp-accent)]/60 hover:bg-[var(--ctp-mantle)]"
        >
          <!-- Left Thumbnail Cover -->
          <div class="relative h-28 w-20 shrink-0 overflow-hidden rounded-xl bg-[var(--ctp-mantle)] shadow-md">
            <img
              v-if="book.has_cover"
              :src="api.coverUrl(book.id, book.updated_at)"
              :alt="book.metadata.title"
              loading="lazy"
              class="h-full w-full object-cover transition duration-300 group-hover:scale-105"
            />
            <div
              v-else
              class="flex h-full w-full flex-col justify-between p-2 text-left"
            >
              <span class="self-end rounded bg-[var(--ctp-accent-soft)] px-1.5 py-0.5 text-[9px] font-bold uppercase text-[var(--ctp-accent)]">
                {{ book.format }}
              </span>
              <p class="line-clamp-2 text-[11px] font-bold leading-tight text-[var(--ctp-text)]">
                {{ book.metadata.title }}
              </p>
            </div>
          </div>

          <!-- Right Title + Subtitle + Progress Track + Play Circle -->
          <div class="flex min-w-0 flex-1 flex-col justify-center">
            <h3 class="truncate text-sm font-bold text-[var(--ctp-text)] transition-colors group-hover:text-[var(--ctp-accent)]">
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

    <!-- Recently Added Section (Larger Cover Cards) -->
    <section v-if="latestBooks.length > 0">
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
        <BookCard
          v-for="book in latestBooks"
          :key="book.id"
          :book="book"
        />
      </div>
    </section>
  </div>
</template>
