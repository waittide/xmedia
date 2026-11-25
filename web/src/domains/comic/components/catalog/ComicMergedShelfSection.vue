<script setup lang="ts">
import { RouterLink } from 'vue-router'
import { LuBookmark, LuBookOpen, LuTrash2 } from 'vue-icons-plus/lu'
import { MediaItemCard } from '@/components/domain'
import { MdIconButton, MediaCoverImage } from '@/components/ui'
import { api, withAuthUrl } from '@/domains/comic/api'
import { useComicI18n } from '@/domains/comic/i18n'
import { useSidebarStore } from '@/domains/comic/store'
import type { ComicBookmarkItemDto } from '@/domains/comic/types'
import { playRoute, type MediaItemVM } from '@/sources'
import { useDialogStore } from '@/stores/dialog'
import { useSnackbarStore } from '@/stores/snackbar'

defineProps<{
  items: readonly MediaItemVM[]
  bookmarks: readonly ComicBookmarkItemDto[]
  preferPlayRoute: boolean
}>()

const { t } = useComicI18n()
const sidebarStore = useSidebarStore()
const dialogStore = useDialogStore()
const snackbarStore = useSnackbarStore()

function mergedItemKey(item: MediaItemVM, idx: number): string {
  if (item.ref.scope === 'local') return `local:${item.ref.id}:${idx}`
  return `${item.ref.scope}:${item.ref.sourceId}:${item.ref.ref}:${idx}`
}

async function handleDeleteBookmark(bm: ComicBookmarkItemDto): Promise<void> {
  const ok = await dialogStore.confirm({
    title: t('library.bookmarkDeleteConfirmTitle'),
    description: t('library.bookmarkDeleteConfirmDesc', { title: bm.series_title }),
    danger: true,
    confirmText: t('common.delete'),
  })
  if (!ok) return
  try {
    await api.deleteBookmark(bm.bookmark.id)
    snackbarStore.show({ message: t('library.bookmarkDeleted') })
    await sidebarStore.refresh()
  } catch (err) {
    snackbarStore.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
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
        :key="mergedItemKey(item, idx)"
        :item="item"
        aspect="poster"
        cover-style="book"
        :fallback-icon="LuBookOpen"
        :prefer-play-route="preferPlayRoute"
      />
    </div>

    <section v-if="bookmarks.length > 0" class="space-y-3">
      <div class="flex items-center gap-2">
        <LuBookmark class="h-4 w-4 text-primary" aria-hidden="true" />
        <h2 class="text-title-large text-on-surface">
          {{ t('library.bookmarksTitle') }}
        </h2>
        <span class="text-body-small tabular-nums text-on-surface-variant">
          {{ bookmarks.length }}
        </span>
      </div>

      <div class="grid grid-cols-1 gap-3 md:grid-cols-2 xl:grid-cols-3">
        <article
          v-for="bm in bookmarks"
          :key="bm.bookmark.id"
          class="group relative flex items-center gap-3 rounded-corner-md bg-surface-container-low p-3 transition-colors duration-150 ease-standard hover:bg-surface-container"
        >
          <div class="h-20 w-14 shrink-0 overflow-hidden rounded-corner-sm bg-surface-container-high">
            <MediaCoverImage
              :src="bm.cover_url ? withAuthUrl(bm.cover_url) : null"
              :alt="bm.series_title"
              aspect-class="aspect-[2/3]"
              rounded="rounded-corner-sm"
              :fallback-icon="LuBookOpen"
              fallback-icon-class="h-5 w-5 text-on-surface-variant/60"
            />
          </div>

          <RouterLink
            :to="
              playRoute(
                { scope: 'local', domain: 'comic', id: bm.bookmark.chapter_id },
                { position: bm.bookmark.page_number, duration: bm.total_pages },
              )
            "
            class="min-w-0 flex-1 outline-none after:absolute after:inset-0 after:rounded-corner-md focus-visible:after:outline-2 focus-visible:after:outline-offset-2 focus-visible:after:outline-secondary"
          >
            <h3 class="text-title-small truncate text-on-surface group-hover:text-primary">
              {{ bm.series_title }}
            </h3>
            <p class="text-body-small mt-0.5 truncate text-on-surface-variant">
              {{ bm.chapter_title || `#${bm.chapter_number}` }} ·
              {{
                t('library.bookmarkPage', {
                  page: bm.bookmark.page_number,
                  total: bm.total_pages,
                })
              }}
            </p>
            <p
              v-if="bm.bookmark.title || bm.bookmark.note"
              class="text-body-small mt-1 line-clamp-1 text-on-surface-variant/85"
            >
              {{ bm.bookmark.note || bm.bookmark.title }}
            </p>
          </RouterLink>

          <div class="relative z-10 shrink-0">
            <MdIconButton
              variant="standard"
              size="sm"
              :icon="LuTrash2"
              :aria-label="t('common.delete')"
              :tooltip="t('common.delete')"
              @click.stop="handleDeleteBookmark(bm)"
            />
          </div>
        </article>
      </div>
    </section>
  </div>
</template>
