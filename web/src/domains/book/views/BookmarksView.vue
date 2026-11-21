<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import { useI18n } from 'vue-i18n'
import DomainBookmarksView from '@/components/domain/DomainBookmarksView.vue'
import { api } from '@/domains/book/api'
import { useCatalogStore } from '@/domains/book/store'
import type { BookBookmarkItemDto } from '@/domains/book/types'
import { useSnackbarStore } from '@/stores/snackbar'

const { t } = useI18n()
const router = useRouter()
const catalogStore = useCatalogStore()
const snackbarStore = useSnackbarStore()

const keyword = ref('')
const loading = ref(false)

async function refresh(): Promise<void> {
  loading.value = true
  try {
    await catalogStore.refreshSidebarData()
  } finally {
    loading.value = false
  }
}

onMounted(() => {
  void refresh()
})

const filteredBookmarks = computed<BookBookmarkItemDto[]>(() => {
  const q = keyword.value.trim().toLowerCase()
  if (!q) return catalogStore.bookmarks
  return catalogStore.bookmarks.filter(
    (b) =>
      b.bookmark.title.toLowerCase().includes(q) ||
      (b.bookmark.note ?? '').toLowerCase().includes(q) ||
      b.book_title.toLowerCase().includes(q) ||
      b.book_authors.some((a) => a.toLowerCase().includes(q)),
  )
})

function openBookmark(item: BookBookmarkItemDto): void {
  const loc = item.bookmark.location
  const query: Record<string, string> = { locator: loc.locator }
  if (loc.chapter_index !== null) query.chapter = String(loc.chapter_index)
  if (loc.page_number !== null) query.page = String(loc.page_number)
  if (loc.scroll_ratio !== null) query.ratio = String(loc.scroll_ratio)
  void router.push({ path: `/book/read/${item.bookmark.book_id}`, query })
}

async function removeBookmark(item: BookBookmarkItemDto): Promise<void> {
  try {
    await api.deleteBookmark(item.bookmark.id)
    await catalogStore.refreshSidebarData()
  } catch (err) {
    snackbarStore.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  }
}
</script>

<template>
  <DomainBookmarksView
    v-model:keyword="keyword"
    :title="t('bookmarksPage.bookTitle')"
    :subtitle="t('bookmarksPage.bookSubtitle')"
    :empty-text="t('bookmarksPage.bookEmpty')"
    :total-count="catalogStore.bookmarks.length"
    :items="filteredBookmarks"
    :item-key="(item) => item.bookmark.id"
    :loading="loading"
    :on-refresh="refresh"
    @open="openBookmark"
    @delete="removeBookmark"
  >
    <template #cover="{ item }">
      <img
        v-if="item.has_cover"
        :src="api.coverUrl(item.bookmark.book_id, item.book_updated_at)"
        :alt="item.book_title"
        loading="lazy"
        decoding="async"
        class="h-full w-full object-cover transition group-hover:scale-105"
      />
      <div v-else class="flex h-full w-full flex-col justify-between p-2 text-left">
        <span
          class="self-end rounded bg-[var(--md-sys-color-primary-container)] px-1 py-0.5 text-[8px] font-bold uppercase text-[var(--md-sys-color-on-primary-container)]"
        >
          {{ item.book_format }}
        </span>
        <p class="line-clamp-2 text-[10px] font-bold leading-tight text-[var(--md-sys-color-on-surface)]">
          {{ item.book_title }}
        </p>
      </div>
    </template>

    <template #body="{ item, open }">
      <button
        type="button"
        class="block w-full truncate text-left text-sm font-bold text-[var(--md-sys-color-on-surface)] transition hover:text-[var(--md-sys-color-primary)]"
        @click="open()"
      >
        {{ item.bookmark.title }}
      </button>
      <p class="mt-0.5 truncate text-xs text-[var(--md-sys-color-on-surface-variant)]">
        {{ item.book_title }}
      </p>
      <p
        v-if="item.bookmark.note"
        class="mt-1 line-clamp-1 text-[11px] text-[var(--md-sys-color-on-surface-variant)]"
      >
        {{ item.bookmark.note }}
      </p>
    </template>

    <template #badge="{ item }">
      <span
        class="rounded-full bg-[var(--md-sys-color-primary-container)] px-2 py-0.5 text-[11px] font-semibold text-[var(--md-sys-color-on-primary-container)]"
      >
        {{ Math.round(item.bookmark.location.progress_percent) }}%
      </span>
    </template>
  </DomainBookmarksView>
</template>
