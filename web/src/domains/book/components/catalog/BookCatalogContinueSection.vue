<script setup lang="ts">
import { ref } from 'vue'
import { useRouter } from 'vue-router'
import { LuBookmark, LuBookOpen, LuTrash2 } from 'vue-icons-plus/lu'
import { MediaItemCard } from '@/components/domain'
import { MdIconButton, MediaCoverImage } from '@/components/ui'
import { api } from '@/domains/book/api'
import { useBookI18n } from '@/domains/book/i18n'
import { useCatalogStore } from '@/domains/book/store'
import type { BookBookmarkItemDto } from '@/domains/book/types'
import type { MediaItemVM } from '@/sources'
import { useSnackbarStore } from '@/stores/snackbar'

defineProps<{
  items: readonly MediaItemVM[]
  bookmarks: readonly BookBookmarkItemDto[]
}>()

const { t } = useBookI18n()
const router = useRouter()
const catalogStore = useCatalogStore()
const snackbarStore = useSnackbarStore()

const deletingBookmarkIds = ref<Set<string>>(new Set())

function openBookmarkInReader(entry: BookBookmarkItemDto): void {
  const bm = entry.bookmark
  void router.push({
    path: `/book/play/${encodeURIComponent(bm.book_id)}`,
    query: {
      locator: bm.location.locator,
      chapter: bm.location.chapter_index !== null ? String(bm.location.chapter_index) : undefined,
      page: bm.location.page_number !== null ? String(bm.location.page_number) : undefined,
      ratio: bm.location.scroll_ratio !== null ? String(bm.location.scroll_ratio) : undefined,
    },
  })
}

async function handleDeleteBookmark(entry: BookBookmarkItemDto): Promise<void> {
  const bm = entry.bookmark
  if (deletingBookmarkIds.value.has(bm.id)) return
  const next = new Set(deletingBookmarkIds.value)
  next.add(bm.id)
  deletingBookmarkIds.value = next
  try {
    await api.deleteBookmark(bm.id)
    catalogStore.bookmarks = catalogStore.bookmarks.filter((item) => item.bookmark.id !== bm.id)
    snackbarStore.show({
      message: t('catalog.bookmarkDeleted'),
      actionLabel: t('catalog.undo'),
      onAction: () => {
        void api
          .createBookmark(bm.book_id, {
            title: bm.title,
            note: bm.note ?? undefined,
            progress_percent: bm.location.progress_percent,
            locator: bm.location.locator,
            chapter_index: bm.location.chapter_index,
            page_number: bm.location.page_number,
            scroll_ratio: bm.location.scroll_ratio,
          })
          .then(() => catalogStore.refreshSidebarData())
          .then(() => {
            snackbarStore.show(t('catalog.bookRestored'))
          })
          .catch(() => {})
      },
    })
  } catch (err) {
    snackbarStore.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  } finally {
    const done = new Set(deletingBookmarkIds.value)
    done.delete(bm.id)
    deletingBookmarkIds.value = done
  }
}
</script>

<template>
  <div class="space-y-8">
    <div
      v-if="items.length > 0"
      class="grid grid-cols-2 gap-5 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 xl:grid-cols-6"
    >
      <MediaItemCard
        v-for="(item, idx) in items"
        :key="`cont-${idx}-${item.title}`"
        :item="item"
        aspect="poster"
        cover-style="book"
        prefer-play-route
      />
    </div>

    <section v-if="bookmarks.length > 0" class="space-y-3">
      <h2 class="text-title-large flex items-center gap-2 text-on-surface">
        <LuBookmark class="h-5 w-5 text-primary" aria-hidden="true" />
        <span>{{ t('catalog.bookmarksSectionTitle') }}</span>
        <span class="text-body-small tabular-nums text-on-surface-variant">
          {{ bookmarks.length }}
        </span>
      </h2>

      <div class="grid grid-cols-1 gap-3 md:grid-cols-2">
        <div
          v-for="entry in bookmarks"
          :key="entry.bookmark.id"
          class="flex items-center gap-3 rounded-corner-md bg-surface-container-low p-3"
        >
          <button
            type="button"
            class="md-state flex min-w-0 flex-1 cursor-pointer items-center gap-3 rounded-corner-sm text-left"
            @click="openBookmarkInReader(entry)"
          >
            <div class="h-14 w-10 shrink-0 overflow-hidden rounded-corner-xs">
              <MediaCoverImage
                :src="
                  entry.has_cover
                    ? api.coverUrl(entry.bookmark.book_id, entry.book_updated_at)
                    : null
                "
                :alt="entry.book_title"
                aspect-class="aspect-[2/3]"
                rounded="rounded-corner-xs"
                bg-class="bg-surface-container-high"
                :fallback-icon="LuBookOpen"
                fallback-icon-class="h-4 w-4 text-on-surface-variant/60"
              />
            </div>
            <div class="min-w-0 flex-1">
              <p class="text-title-small truncate text-on-surface">
                {{ entry.bookmark.title }}
              </p>
              <p class="text-body-small mt-0.5 truncate text-on-surface-variant">
                {{ entry.book_title }}
              </p>
              <p class="text-label-small mt-0.5 tabular-nums text-primary">
                {{ Math.round(entry.bookmark.location.progress_percent) }}%
              </p>
            </div>
          </button>

          <MdIconButton
            size="sm"
            variant="standard"
            :icon="LuTrash2"
            :disabled="deletingBookmarkIds.has(entry.bookmark.id)"
            :aria-label="t('common.delete')"
            @click="handleDeleteBookmark(entry)"
          />
        </div>
      </div>
    </section>
  </div>
</template>
