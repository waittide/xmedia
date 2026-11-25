<script setup lang="ts">
import { ref, watch } from 'vue'
import { LuBookmark, LuChevronDown, LuChevronUp, LuList, LuTrash2 } from 'vue-icons-plus/lu'
import { MdIconButton } from '@/components/ui'
import TocTree from '@/domains/book/components/TocTree.vue'
import { useBookI18n as useI18n } from '@/domains/book/i18n'
import type { Bookmark, TocItem } from '@/domains/book/types'

const props = defineProps<{
  bookId: string
  toc: TocItem[]
  activeLocator?: string | null
  bookmarks: Bookmark[]
  deletingBookmarkIds: Set<string>
}>()

const emit = defineEmits<{
  'select-toc': [href: string]
  'open-bookmark': [bookmark: Bookmark]
  'delete-bookmark': [bookmark: Bookmark]
}>()

const { t } = useI18n()

const tocExpanded = ref(false)
const bookmarksExpanded = ref(false)

watch(
  () => props.bookId,
  () => {
    tocExpanded.value = false
    bookmarksExpanded.value = false
  },
)
</script>

<template>
  <div v-if="toc.length > 0 || bookmarks.length > 0" class="space-y-3">
    <section v-if="toc.length > 0" class="rounded-corner-lg bg-surface-container-low p-3">
      <button
        type="button"
        :aria-expanded="tocExpanded"
        class="md-state text-title-small inline-flex cursor-pointer items-center gap-2 rounded-corner-full px-3 py-2 text-on-surface-variant hover:text-on-surface"
        @click="tocExpanded = !tocExpanded"
      >
        <LuList class="h-4 w-4" aria-hidden="true" />
        <span>{{ t('reader.toc') }} ({{ toc.length }})</span>
        <LuChevronUp v-if="tocExpanded" class="h-4 w-4" aria-hidden="true" />
        <LuChevronDown v-else class="h-4 w-4" aria-hidden="true" />
      </button>

      <div
        v-if="tocExpanded"
        class="mt-2 max-h-96 overflow-y-auto rounded-corner-md bg-surface-container p-3"
      >
        <TocTree
          :items="toc"
          :active-href="activeLocator ?? undefined"
          searchable
          @select="emit('select-toc', $event)"
        />
      </div>
    </section>

    <section v-if="bookmarks.length > 0" class="rounded-corner-lg bg-surface-container-low p-3">
      <button
        type="button"
        :aria-expanded="bookmarksExpanded"
        class="md-state text-title-small inline-flex cursor-pointer items-center gap-2 rounded-corner-full px-3 py-2 text-on-surface-variant hover:text-on-surface"
        @click="bookmarksExpanded = !bookmarksExpanded"
      >
        <LuBookmark class="h-4 w-4" aria-hidden="true" />
        <span>{{ t('detail.bookmarksSection') }} ({{ bookmarks.length }})</span>
        <LuChevronUp v-if="bookmarksExpanded" class="h-4 w-4" aria-hidden="true" />
        <LuChevronDown v-else class="h-4 w-4" aria-hidden="true" />
      </button>

      <div v-if="bookmarksExpanded" class="mt-2 space-y-1.5">
        <div
          v-for="bm in bookmarks"
          :key="bm.id"
          class="flex items-center justify-between gap-2 rounded-corner-md bg-surface-container px-3.5 py-2.5"
        >
          <button
            type="button"
            class="md-state flex min-w-0 flex-1 cursor-pointer items-center gap-3 rounded-corner-sm py-0.5 text-left"
            @click="emit('open-bookmark', bm)"
          >
            <LuBookmark class="h-4 w-4 shrink-0 text-primary" aria-hidden="true" />
            <div class="min-w-0">
              <p class="text-body-medium truncate text-on-surface">
                {{ bm.title }}
              </p>
              <p class="text-label-small tabular-nums text-on-surface-variant">
                {{ Math.round(bm.location.progress_percent) }}%
              </p>
            </div>
          </button>

          <MdIconButton
            size="sm"
            variant="standard"
            :icon="LuTrash2"
            :disabled="deletingBookmarkIds.has(bm.id)"
            :aria-label="t('common.delete')"
            @click="emit('delete-bookmark', bm)"
          />
        </div>
      </div>
    </section>
  </div>
</template>
