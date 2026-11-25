<script setup lang="ts">
import { LuHeart, LuTag } from 'vue-icons-plus/lu'
import { MdButton } from '@/components/ui'
import { api } from '@/domains/book/api'
import { useBookI18n } from '@/domains/book/i18n'
import { useCatalogStore } from '@/domains/book/store'
import { useAuthStore } from '@/stores/auth'
import { useDialogStore } from '@/stores/dialog'
import { useSnackbarStore } from '@/stores/snackbar'

const emit = defineEmits<{
  changed: [refreshSidebar: boolean]
}>()

const { t } = useBookI18n()
const authStore = useAuthStore()
const catalogStore = useCatalogStore()
const dialogStore = useDialogStore()
const snackbarStore = useSnackbarStore()

async function batchFavorite(fav: boolean): Promise<void> {
  const ids = Array.from(catalogStore.selectedBookIds)
  if (ids.length === 0) return
  try {
    await api.batchBooks({ book_ids: ids, set_favorite: fav })
    catalogStore.clearSelection()
    emit('changed', false)
  } catch (err) {
    snackbarStore.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  }
}

async function batchAddTag(): Promise<void> {
  const ids = Array.from(catalogStore.selectedBookIds)
  if (ids.length === 0) return
  const raw = await dialogStore.prompt({
    title: t('catalog.batchAddTag'),
    message: t('catalog.batchTagPrompt'),
  })
  if (!raw) return
  const addTags = raw
    .split(/[,，]/)
    .map((s) => s.trim())
    .filter(Boolean)
  if (addTags.length === 0) return
  try {
    await api.batchBooks({ book_ids: ids, add_tags: addTags })
    catalogStore.clearSelection()
    emit('changed', true)
  } catch (err) {
    snackbarStore.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  }
}
</script>

<template>
  <div
    class="flex flex-wrap items-center justify-between gap-2 rounded-corner-md bg-secondary-container px-4 py-2.5 text-on-secondary-container"
  >
    <span class="text-title-small">
      {{ t('catalog.selectedCount', { count: catalogStore.selectedBookIds.size }) }}
    </span>
    <div class="flex flex-wrap items-center gap-1.5">
      <MdButton variant="elevated" size="sm" :icon="LuHeart" @click="batchFavorite(true)">
        {{ t('catalog.batchStar') }}
      </MdButton>
      <MdButton variant="elevated" size="sm" @click="batchFavorite(false)">
        {{ t('catalog.batchUnstar') }}
      </MdButton>
      <MdButton
        v-if="authStore.isAdmin"
        variant="elevated"
        size="sm"
        :icon="LuTag"
        @click="batchAddTag"
      >
        {{ t('catalog.batchAddTag') }}
      </MdButton>
      <MdButton variant="text" size="sm" @click="catalogStore.clearSelection()">
        {{ t('catalog.clearSelection') }}
      </MdButton>
    </div>
  </div>
</template>
