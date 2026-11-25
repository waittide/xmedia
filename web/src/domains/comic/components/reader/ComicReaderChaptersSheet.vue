<script setup lang="ts">
import { computed, nextTick, onMounted, ref } from 'vue'
import { LuBookmark, LuTrash2 } from 'vue-icons-plus/lu'
import { MdEmptyState, MdIconButton } from '@/components/ui'
import { useComicI18n } from '@/domains/comic/i18n'
import type {
  ChaptersSheetTab,
  ComicReaderBookmarkItem,
  ComicReaderChapterItem,
} from './types'

const props = defineProps<{
  tab: ChaptersSheetTab
  chapterKey: string
  chapters: readonly ComicReaderChapterItem[]
  bookmarks: readonly ComicReaderBookmarkItem[]
  filterQuery: string
}>()

const emit = defineEmits<{
  selectChapter: [key: string]
  jumpToPage: [page: number]
  deleteBookmark: [id: string]
}>()

const { t } = useComicI18n()
const listRef = ref<HTMLDivElement | null>(null)

const filteredChapters = computed(() => {
  const q = props.filterQuery.trim().toLowerCase()
  if (!q) return props.chapters
  return props.chapters.filter((ch) => ch.label.toLowerCase().includes(q))
})

const sortedBookmarks = computed(() =>
  [...props.bookmarks].sort((a, b) => a.page - b.page),
)

onMounted(async () => {
  await nextTick()
  const activeBtn = listRef.value?.querySelector<HTMLElement>('[data-active-chapter="true"]')
  activeBtn?.scrollIntoView({ block: 'center' })
})
</script>

<template>
  <div ref="listRef" class="space-y-1">
    <template v-if="tab === 'bookmarks'">
      <MdEmptyState
        v-if="sortedBookmarks.length === 0"
        compact
        :icon="LuBookmark"
        :title="t('reader.noChapterBookmarks')"
      />
      <div
        v-for="bm in sortedBookmarks"
        :key="bm.id"
        class="flex items-center gap-2 rounded-corner-md px-2 py-1 hover:bg-surface-container-high"
      >
        <button
          type="button"
          class="min-w-0 flex-1 cursor-pointer py-1.5 text-left outline-none"
          @click="emit('jumpToPage', bm.page)"
        >
          <div class="text-title-small truncate text-on-surface">
            {{ t('reader.bookmarkPageLabel', { page: bm.page }) }}
          </div>
          <p v-if="bm.note || bm.title" class="text-body-small truncate text-on-surface-variant">
            {{ bm.note || bm.title }}
          </p>
        </button>
        <MdIconButton
          size="sm"
          variant="standard"
          :icon="LuTrash2"
          :aria-label="t('common.delete')"
          @click="emit('deleteBookmark', bm.id)"
        />
      </div>
    </template>

    <template v-else>
      <button
        v-for="ch in filteredChapters"
        :key="ch.key"
        type="button"
        :disabled="ch.disabled"
        :data-active-chapter="ch.key === chapterKey ? 'true' : undefined"
        class="md-state text-body-medium flex w-full cursor-pointer items-center justify-between gap-2 rounded-corner-md px-3 py-2.5 text-left transition-colors duration-150 disabled:cursor-default disabled:opacity-45"
        :class="
          ch.key === chapterKey
            ? 'bg-secondary-container text-on-secondary-container'
            : 'text-on-surface hover:bg-surface-container-high'
        "
        @click="emit('selectChapter', ch.key)"
      >
        <span class="truncate">{{ ch.label }}</span>
        <span
          v-if="ch.metaLabel"
          class="text-label-small shrink-0 text-on-surface-variant"
        >
          {{ ch.metaLabel }}
        </span>
      </button>
    </template>
  </div>
</template>
