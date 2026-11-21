<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import { useI18n } from 'vue-i18n'
import DomainBookmarksView from '@/components/domain/DomainBookmarksView.vue'
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

async function removeBookmark(item: ComicBookmarkItemDto): Promise<void> {
  await api.deleteBookmark(item.bookmark.id)
  await sidebarStore.refresh()
}
</script>

<template>
  <DomainBookmarksView
    v-model:keyword="keyword"
    :title="t('bookmarksPage.comicTitle')"
    :subtitle="t('bookmarksPage.comicSubtitle')"
    :empty-text="t('bookmarksPage.comicEmpty')"
    :total-count="sidebarStore.bookmarks.length"
    :items="filteredBookmarks"
    :item-key="(item) => item.bookmark.id"
    :loading="loading"
    :on-refresh="refresh"
    @open="openBookmark"
    @delete="removeBookmark"
  >
    <template #cover="{ item }">
      <img
        v-if="item.cover_url"
        :src="withAuthUrl(item.cover_url)"
        :alt="item.series_title"
        loading="lazy"
        decoding="async"
        class="h-full w-full object-cover transition group-hover:scale-105"
      />
      <div
        v-else
        class="flex h-full w-full items-center justify-center p-2 text-center text-[10px] font-bold text-[var(--md-sys-color-on-surface-variant)]"
      >
        {{ item.series_title }}
      </div>
    </template>

    <template #body="{ item, open }">
      <button
        type="button"
        class="block w-full truncate text-left text-sm font-bold text-[var(--md-sys-color-on-surface)] transition hover:text-[var(--md-sys-color-primary)]"
        @click="open()"
      >
        {{ item.series_title }}
      </button>
      <p class="mt-0.5 truncate text-xs text-[var(--md-sys-color-on-surface-variant)]">
        #{{ item.chapter_number }} {{ item.chapter_title }}
      </p>
    </template>

    <template #badge="{ item }">
      <span
        class="rounded-full bg-[var(--md-sys-color-secondary-container)] px-2.5 py-0.5 text-[11px] font-semibold text-[var(--md-sys-color-on-secondary-container)]"
      >
        {{ t('bookmarksPage.pageLabel', { page: item.bookmark.page_number }) }} / {{ item.total_pages }}
      </span>
    </template>
  </DomainBookmarksView>
</template>
