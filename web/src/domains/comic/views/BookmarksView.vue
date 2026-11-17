<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import { useI18n } from 'vue-i18n'
import { LuBookmark, LuPlay, LuSearch, LuTrash2, LuX } from 'vue-icons-plus/lu'
import { api, withAuthUrl } from '@/domains/comic/api'
import { useSidebarStore } from '@/domains/comic/store'
import type { ComicBookmarkItemDto } from '@/domains/comic/types'

const { t } = useI18n()
const router = useRouter()
const sidebarStore = useSidebarStore()

const keyword = ref('')
const loading = ref(false)

async function refresh(): Promise<void> {
  loading.value = true
  try {
    await sidebarStore.refresh()
  } finally {
    loading.value = false
  }
}

onMounted(() => {
  void refresh()
})

const filteredBookmarks = computed<ComicBookmarkItemDto[]>(() => {
  const q = keyword.value.trim().toLowerCase()
  if (!q) return sidebarStore.bookmarks
  return sidebarStore.bookmarks.filter(
    (b) =>
      b.bookmark.title.toLowerCase().includes(q) ||
      (b.bookmark.note ?? '').toLowerCase().includes(q) ||
      b.series_title.toLowerCase().includes(q) ||
      b.chapter_title.toLowerCase().includes(q),
  )
})

function openBookmark(item: ComicBookmarkItemDto): void {
  void router.push(
    `/comic/read/${encodeURIComponent(item.bookmark.chapter_id)}?page=${item.bookmark.page_number}`,
  )
}

async function removeBookmark(id: string): Promise<void> {
  await api.deleteBookmark(id)
  await sidebarStore.refresh()
}
</script>

<template>
  <div class="mx-auto max-w-7xl space-y-6 px-6 py-8 sm:px-8">
    <div class="flex flex-wrap items-end justify-between gap-4">
      <div>
        <div class="flex items-center gap-2.5">
          <h1 class="text-2xl font-bold tracking-tight text-[var(--ctp-text)]">
            {{ t('bookmarksPage.comicTitle') }}
          </h1>
          <span
            class="rounded-full bg-[var(--ctp-accent-soft)] px-2.5 py-0.5 text-xs font-bold text-[var(--ctp-accent)]"
          >
            {{ sidebarStore.bookmarks.length }}
          </span>
        </div>
        <p class="mt-1 text-xs text-[var(--ctp-subtext0)]">
          {{ t('bookmarksPage.comicSubtitle') }}
        </p>
      </div>
    </div>

    <div
      class="flex items-center justify-between gap-3 rounded-2xl border border-[var(--ctp-surface0)]/75 bg-[var(--ctp-shell)] p-3"
    >
      <div class="relative max-w-md flex-1">
        <LuSearch
          class="pointer-events-none absolute left-3.5 top-1/2 h-4 w-4 -translate-y-1/2 text-[var(--ctp-subtext0)]"
        />
        <input
          v-model="keyword"
          type="text"
          :placeholder="t('common.filterPlaceholder')"
          class="h-9 w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] pl-9 pr-8 text-xs text-[var(--ctp-text)] placeholder-[var(--ctp-subtext0)] outline-none focus:border-[var(--ctp-accent)]"
        />
        <button
          v-if="keyword"
          type="button"
          class="absolute right-2.5 top-1/2 -translate-y-1/2 text-[var(--ctp-subtext0)] hover:text-[var(--ctp-text)]"
          @click="keyword = ''"
        >
          <LuX class="h-3.5 w-3.5" />
        </button>
      </div>
    </div>

    <div
      v-if="!loading && filteredBookmarks.length === 0"
      class="card-soft flex flex-col items-center justify-center rounded-3xl bg-[var(--ctp-shell)] px-6 py-16 text-center"
    >
      <div
        class="mb-4 flex h-14 w-14 items-center justify-center rounded-2xl bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]"
      >
        <LuBookmark class="h-7 w-7" />
      </div>
      <p class="text-xs text-[var(--ctp-subtext0)]">{{ t('bookmarksPage.comicEmpty') }}</p>
    </div>

    <div v-else class="grid grid-cols-1 gap-4 md:grid-cols-2 xl:grid-cols-3">
      <div
        v-for="item in filteredBookmarks"
        :key="item.bookmark.id"
        class="group card-soft flex items-center gap-4 rounded-2xl bg-[var(--ctp-shell)] p-3.5 transition hover:-translate-y-0.5 hover:border-[var(--ctp-accent)]/60"
      >
        <button
          type="button"
          class="relative h-24 w-17 shrink-0 overflow-hidden rounded-xl bg-[var(--ctp-mantle)] shadow-sm"
          @click="openBookmark(item)"
        >
          <img
            v-if="item.cover_url"
            :src="withAuthUrl(item.cover_url)"
            :alt="item.series_title"
            class="h-full w-full object-cover transition group-hover:scale-105"
          />
          <div
            v-else
            class="flex h-full w-full items-center justify-center p-2 text-center text-[10px] font-bold text-[var(--ctp-subtext0)]"
          >
            {{ item.series_title }}
          </div>
        </button>

        <div class="min-w-0 flex-1">
          <button
            type="button"
            class="block w-full truncate text-left text-sm font-bold text-[var(--ctp-text)] hover:text-[var(--ctp-accent)]"
            @click="openBookmark(item)"
          >
            {{ item.series_title }}
          </button>
          <p class="mt-0.5 truncate text-xs text-[var(--ctp-subtext0)]">
            #{{ item.chapter_number }} {{ item.chapter_title }}
          </p>

          <div class="mt-2.5 flex items-center justify-between gap-2">
            <span class="rounded-md bg-[var(--ctp-canvas)] px-2 py-0.5 text-[11px] font-semibold text-[var(--ctp-accent)]">
              {{ t('bookmarksPage.pageLabel', { page: item.bookmark.page_number }) }} / {{ item.total_pages }}
            </span>

            <div class="flex items-center gap-1">
              <button
                type="button"
                class="inline-flex items-center gap-1 rounded-lg bg-[var(--ctp-accent)] px-2.5 py-1 text-[11px] font-semibold text-[var(--ctp-on-accent)] transition hover:opacity-90"
                @click="openBookmark(item)"
              >
                <LuPlay class="h-3 w-3 fill-current" />
                <span>{{ t('bookmarksPage.jumpBtn') }}</span>
              </button>
              <button
                type="button"
                class="rounded-lg p-1.5 text-[var(--ctp-subtext0)] transition hover:bg-[var(--ctp-red)]/15 hover:text-[var(--ctp-red)]"
                :title="t('common.delete')"
                @click="removeBookmark(item.bookmark.id)"
              >
                <LuTrash2 class="h-3.5 w-3.5" />
              </button>
            </div>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>
