<script setup lang="ts">
import { computed } from 'vue'
import { useRouter } from 'vue-router'
import { LuBookOpen, LuHeart } from 'vue-icons-plus/lu'
import { AppCheckbox, MdIconButton, MediaCoverImage } from '@/components/ui'
import { api } from '@/domains/book/api'
import { useBookI18n } from '@/domains/book/i18n'
import { useCatalogStore } from '@/domains/book/store'
import type { BookSummaryDto } from '@/domains/book/types'
import {
  formatProgressPercentValue,
  hasBookProgress,
  normalizeBookProgressPercent,
} from '@/domains/book/utils/bookVm'
import { itemRoute } from '@/sources'
import { formatBytes } from '@/utils/format'

const props = defineProps<{
  books: readonly BookSummaryDto[]
}>()

const emit = defineEmits<{
  toggleFavorite: [book: BookSummaryDto]
}>()

const { t } = useBookI18n()
const router = useRouter()
const catalogStore = useCatalogStore()

const allCurrentSelected = computed(
  () =>
    props.books.length > 0 &&
    props.books.every((b) => catalogStore.selectedBookIds.has(b.id)),
)

function toggleSelectAll(): void {
  if (allCurrentSelected.value) {
    catalogStore.clearSelection()
  } else {
    catalogStore.selectedBookIds = new Set(props.books.map((b) => b.id))
  }
}

function openBookDetail(bookId: string): void {
  void router.push(itemRoute({ scope: 'local', domain: 'book', id: bookId }))
}
</script>

<template>
  <div class="overflow-x-auto rounded-corner-md bg-surface-container-low">
    <table class="w-full border-collapse text-left">
      <thead>
        <tr class="text-label-medium border-b border-outline-variant text-on-surface-variant">
          <th class="w-10 py-3 pl-4">
            <AppCheckbox
              :model-value="allCurrentSelected"
              @update:model-value="toggleSelectAll"
            />
          </th>
          <th class="w-10 py-3" />
          <th class="py-3 pr-4">{{ t('catalog.colTitle') }}</th>
          <th class="hidden py-3 pr-4 md:table-cell">{{ t('catalog.colAuthor') }}</th>
          <th class="hidden py-3 pr-4 sm:table-cell">{{ t('catalog.colProgress') }}</th>
          <th class="py-3 pr-4">{{ t('catalog.colFormat') }}</th>
          <th class="hidden py-3 pr-4 text-right sm:table-cell">
            {{ t('catalog.colSize') }}
          </th>
        </tr>
      </thead>
      <tbody class="divide-y divide-outline-variant/50">
        <tr
          v-for="book in books"
          :key="book.id"
          tabindex="0"
          class="group cursor-pointer transition-colors duration-150 ease-standard hover:bg-surface-container focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-secondary"
          @click="openBookDetail(book.id)"
          @keydown.enter.self="openBookDetail(book.id)"
          @keydown.space.self.prevent="openBookDetail(book.id)"
        >
          <td class="py-2.5 pl-4" @click.stop>
            <AppCheckbox
              :model-value="catalogStore.selectedBookIds.has(book.id)"
              @update:model-value="catalogStore.toggleSelectBook(book.id)"
            />
          </td>
          <td class="py-2.5 pr-1" @click.stop>
            <MdIconButton
              size="sm"
              variant="standard"
              :selected="book.is_favorite"
              :aria-label="
                book.is_favorite ? t('detail.favoriteRemove') : t('detail.favoriteAdd')
              "
              @click="emit('toggleFavorite', book)"
            >
              <LuHeart
                class="h-4 w-4"
                :class="book.is_favorite ? 'fill-error text-error' : ''"
              />
            </MdIconButton>
          </td>
          <td class="py-2.5 pr-4">
            <div class="flex items-center gap-3">
              <div class="h-10 w-7 shrink-0 overflow-hidden rounded-corner-xs">
                <MediaCoverImage
                  :src="book.has_cover ? api.coverUrl(book.id, book.updated_at) : null"
                  :alt="book.metadata.title"
                  aspect-class="aspect-[2/3]"
                  rounded="rounded-corner-xs"
                  bg-class="bg-surface-container-high"
                  :fallback-icon="LuBookOpen"
                  fallback-icon-class="h-3.5 w-3.5 text-on-surface-variant/60"
                />
              </div>
              <div class="min-w-0">
                <p class="text-title-small truncate text-on-surface group-hover:text-primary">
                  {{ book.metadata.title }}
                </p>
                <div v-if="book.metadata.tags.length > 0" class="mt-0.5 flex flex-wrap gap-1">
                  <span
                    v-for="tg in book.metadata.tags.slice(0, 2)"
                    :key="tg"
                    class="text-label-small rounded-corner-xs bg-surface-container-high px-1.5 py-0.5 text-on-surface-variant"
                  >
                    {{ tg }}
                  </span>
                </div>
              </div>
            </div>
          </td>
          <td class="text-body-small hidden py-2.5 pr-4 text-on-surface-variant md:table-cell">
            {{ book.metadata.authors.join(', ') || t('catalog.unknownAuthor') }}
          </td>
          <td class="text-label-medium hidden py-2.5 pr-4 tabular-nums sm:table-cell">
            <span v-if="hasBookProgress(book.progress)" class="text-primary">
              {{ formatProgressPercentValue(normalizeBookProgressPercent(book.progress) ?? 0) }}%
            </span>
            <span v-else class="text-on-surface-variant">—</span>
          </td>
          <td class="py-2.5 pr-4">
            <span
              class="text-label-small rounded-corner-xs bg-surface-container-high px-2 py-0.5 font-mono uppercase text-on-surface-variant"
            >
              {{ book.format }}
            </span>
          </td>
          <td
            class="text-body-small hidden py-2.5 pr-4 text-right tabular-nums text-on-surface-variant sm:table-cell"
          >
            {{ formatBytes(book.file_size) }}
          </td>
        </tr>
      </tbody>
    </table>
  </div>
</template>
