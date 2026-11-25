<script setup lang="ts">
import { computed } from 'vue'
import { useI18n as useGlobalI18n } from 'vue-i18n'
import { LuBookOpen, LuTrash2, LuX } from 'vue-icons-plus/lu'
import { MediaItemCard } from '@/components/domain'
import { MdMenuItem } from '@/components/ui'
import { api } from '@/domains/book/api'
import { useBookI18n } from '@/domains/book/i18n'
import type { BookSummaryDto } from '@/domains/book/types'
import { bookSummaryToVM, hasBookProgress } from '@/domains/book/utils/bookVm'
import { itemRoute, playRoute, type MediaItemVM } from '@/sources'
import { useAuthStore } from '@/stores/auth'
import { useDialogStore } from '@/stores/dialog'
import { useSnackbarStore } from '@/stores/snackbar'

const props = defineProps<{
  books: readonly BookSummaryDto[]
}>()

const emit = defineEmits<{
  toggleFavorite: [book: BookSummaryDto]
  mutated: []
}>()

const { t } = useBookI18n()
const { t: globalT } = useGlobalI18n()
const authStore = useAuthStore()
const dialogStore = useDialogStore()
const snackbarStore = useSnackbarStore()

const localBookVMs = computed<Array<{ book: BookSummaryDto; vm: MediaItemVM }>>(() =>
  props.books.map((book) => ({
    book,
    vm: bookSummaryToVM(book, {
      unknownAuthor: t('catalog.unknownAuthor'),
      missingLabel: t('catalog.missingBadge'),
    }),
  })),
)

async function handleDeleteBookFromCard(book: BookSummaryDto): Promise<void> {
  const confirmed = await dialogStore.confirm({
    title: t('detail.deleteBook'),
    message: t('detail.confirmDeleteBook', { title: book.metadata.title }),
    danger: true,
  })
  if (!confirmed) return
  try {
    await api.deleteBook(book.id)
    emit('mutated')
  } catch (err) {
    snackbarStore.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  }
}

async function handleUnbindBookFromCard(book: BookSummaryDto): Promise<void> {
  const confirmed = await dialogStore.confirm({
    title: globalT('pool.unbindConfirmTitle'),
    message: book.online_tracking
      ? globalT('pool.unbindOnlineConfirmMessage', { title: book.metadata.title })
      : globalT('pool.unbindConfirmMessage', { title: book.metadata.title }),
    danger: true,
    confirmText: globalT('pool.unbindBtn'),
  })
  if (!confirmed) return
  try {
    await api.unbindItem(book.id)
    snackbarStore.show(globalT('pool.unboundToast'))
    emit('mutated')
  } catch (err) {
    snackbarStore.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  }
}
</script>

<template>
  <div class="grid grid-cols-2 gap-5 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 xl:grid-cols-6">
    <MediaItemCard
      v-for="entry in localBookVMs"
      :key="entry.book.id"
      :item="entry.vm"
      aspect="poster"
      cover-style="book"
      show-favorite
      :favorite="entry.book.is_favorite"
      @toggle-favorite="emit('toggleFavorite', entry.book)"
    >
      <template #menu>
        <MdMenuItem
          v-if="!entry.book.is_missing"
          :icon="LuBookOpen"
          :label="
            hasBookProgress(entry.book.progress)
              ? t('catalog.continueReading')
              : t('catalog.startReading')
          "
          :to="playRoute({ scope: 'local', domain: 'book', id: entry.book.id })"
        />
        <MdMenuItem
          :icon="LuBookOpen"
          :label="t('catalog.openDetail')"
          :to="itemRoute({ scope: 'local', domain: 'book', id: entry.book.id })"
        />
        <MdMenuItem
          :icon="LuX"
          :label="globalT('pool.unbindBtn')"
          destructive
          @select="handleUnbindBookFromCard(entry.book)"
        />
        <MdMenuItem
          v-if="authStore.isAdmin"
          :icon="LuTrash2"
          :label="t('detail.deleteBook')"
          destructive
          @select="handleDeleteBookFromCard(entry.book)"
        />
      </template>
    </MediaItemCard>
  </div>
</template>
