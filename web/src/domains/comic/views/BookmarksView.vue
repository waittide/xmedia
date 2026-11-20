<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import { useI18n } from 'vue-i18n'
import { LuBookmark, LuPlay, LuSearch, LuTrash2, LuX } from 'vue-icons-plus/lu'
import RefreshButton from '@/components/ui/RefreshButton.vue'
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
          <h1 class="text-2xl font-bold tracking-tight text-[var(--md-sys-color-on-surface)]">
            {{ t('bookmarksPage.comicTitle') }}
          </h1>
          <span
            class="rounded-full bg-[var(--md-sys-color-primary-container)] px-2.5 py-0.5 text-xs font-bold text-[var(--md-sys-color-on-primary-container)]"
          >
            {{ sidebarStore.bookmarks.length }}
          </span>
        </div>
        <p class="mt-1 text-xs text-[var(--md-sys-color-on-surface-variant)]">
          {{ t('bookmarksPage.comicSubtitle') }}
        </p>
      </div>

      <RefreshButton size="md" :action="refresh" :busy="loading" />
    </div>

    <div class="md-card-filled flex items-center justify-between gap-3 p-3">
      <div class="relative max-w-md flex-1">
        <LuSearch
          class="pointer-events-none absolute left-3.5 top-1/2 h-4 w-4 -translate-y-1/2 text-[var(--md-sys-color-on-surface-variant)]"
        />
        <input
          v-model="keyword"
          type="text"
          :placeholder="t('common.filterPlaceholder')"
          class="md-input h-9 w-full pl-9 pr-8 text-xs placeholder:text-[var(--md-sys-color-on-surface-variant)]"
        />
        <button
          v-if="keyword"
          type="button"
          class="md-icon-btn md-icon-btn-sm md-state absolute right-1 top-1/2 !h-7 !w-7 -translate-y-1/2"
          @click="keyword = ''"
        >
          <LuX class="h-3.5 w-3.5" />
        </button>
      </div>
    </div>

    <div
      v-if="!loading && filteredBookmarks.length === 0"
      class="md-card-filled flex flex-col items-center justify-center rounded-3xl px-6 py-16 text-center"
    >
      <div
        class="mb-4 flex h-14 w-14 items-center justify-center rounded-2xl bg-[var(--md-sys-color-primary-container)] text-[var(--md-sys-color-on-primary-container)]"
      >
        <LuBookmark class="h-7 w-7" />
      </div>
      <p class="text-xs text-[var(--md-sys-color-on-surface-variant)]">
        {{ t('bookmarksPage.comicEmpty') }}
      </p>
    </div>

    <div v-else class="grid grid-cols-1 gap-4 md:grid-cols-2 xl:grid-cols-3">
      <div
        v-for="item in filteredBookmarks"
        :key="item.bookmark.id"
        class="group md-card-filled flex items-center gap-4 border border-transparent p-3.5 transition hover:-translate-y-0.5 hover:border-[var(--md-sys-color-outline-variant)] hover:shadow-[var(--md-sys-elevation-1)]"
      >
        <button
          type="button"
          class="relative h-24 w-17 shrink-0 overflow-hidden rounded-xl bg-[var(--md-sys-color-surface-container)] shadow-[var(--md-sys-elevation-1)]"
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
            class="flex h-full w-full items-center justify-center p-2 text-center text-[10px] font-bold text-[var(--md-sys-color-on-surface-variant)]"
          >
            {{ item.series_title }}
          </div>
        </button>

        <div class="min-w-0 flex-1">
          <button
            type="button"
            class="block w-full truncate text-left text-sm font-bold text-[var(--md-sys-color-on-surface)] transition hover:text-[var(--md-sys-color-primary)]"
            @click="openBookmark(item)"
          >
            {{ item.series_title }}
          </button>
          <p class="mt-0.5 truncate text-xs text-[var(--md-sys-color-on-surface-variant)]">
            #{{ item.chapter_number }} {{ item.chapter_title }}
          </p>

          <div class="mt-2.5 flex items-center justify-between gap-2">
            <span
              class="rounded-full bg-[var(--md-sys-color-secondary-container)] px-2.5 py-0.5 text-[11px] font-semibold text-[var(--md-sys-color-on-secondary-container)]"
            >
              {{ t('bookmarksPage.pageLabel', { page: item.bookmark.page_number }) }} / {{ item.total_pages }}
            </span>

            <div class="flex items-center gap-1">
              <button
                type="button"
                class="md-btn md-btn-sm md-btn-filled md-state !h-7 !px-2.5 !text-[11px]"
                @click="openBookmark(item)"
              >
                <LuPlay class="h-3 w-3 fill-current" />
                <span>{{ t('bookmarksPage.jumpBtn') }}</span>
              </button>
              <button
                type="button"
                class="md-icon-btn md-icon-btn-sm md-state !h-7 !w-7 hover:!text-[var(--md-sys-color-error)]"
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
