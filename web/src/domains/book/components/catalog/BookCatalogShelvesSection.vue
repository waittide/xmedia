<script setup lang="ts">
import { LuArrowLeft, LuFolder, LuMoreVertical, LuPencil, LuTrash2 } from 'vue-icons-plus/lu'
import { MdButton, MdIconButton, MdMenu, MdMenuItem } from '@/components/ui'
import { useBookI18n } from '@/domains/book/i18n'
import type { ShelfDto } from '@/domains/book/types'

withDefaults(
  defineProps<{
    mode: 'subheader' | 'grid'
    activeShelf?: ShelfDto | null
    shelves?: readonly ShelfDto[]
  }>(),
  {
    activeShelf: null,
    shelves: () => [],
  },
)

const emit = defineEmits<{
  back: []
  select: [shelfId: string]
  rename: [shelf: ShelfDto]
  delete: [shelf: ShelfDto]
}>()

const { t } = useBookI18n()
</script>

<template>
  <div
    v-if="mode === 'subheader' && activeShelf"
    class="flex flex-wrap items-center justify-between gap-3 rounded-corner-md bg-surface-container-low px-4 py-3"
  >
    <div class="flex min-w-0 items-center gap-2.5">
      <MdButton variant="text" :icon="LuArrowLeft" @click="emit('back')">
        {{ t('collections.backToShelves') }}
      </MdButton>
      <span class="text-title-medium truncate text-on-surface">
        {{ activeShelf.name }}
      </span>
      <span class="text-body-small shrink-0 tabular-nums text-on-surface-variant">
        {{ t('collections.bookCountUnit', { count: activeShelf.book_count }) }}
      </span>
    </div>

    <div v-if="!activeShelf.is_favorite" class="flex items-center gap-1">
      <MdButton variant="text" :icon="LuPencil" @click="emit('rename', activeShelf)">
        {{ t('collections.renameShelf') }}
      </MdButton>
      <MdButton
        variant="text"
        tone="error"
        :icon="LuTrash2"
        @click="emit('delete', activeShelf)"
      >
        {{ t('collections.deleteShelf') }}
      </MdButton>
    </div>
  </div>

  <div v-else class="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-3">
    <div
      v-for="shelf in shelves"
      :key="shelf.id"
      class="flex items-center justify-between gap-2 rounded-corner-md bg-surface-container-low px-4 py-3.5 transition-colors duration-150 ease-standard hover:bg-surface-container"
    >
      <button
        type="button"
        class="md-state flex min-w-0 flex-1 cursor-pointer items-center gap-3 rounded-corner-sm text-left"
        @click="emit('select', shelf.id)"
      >
        <div
          class="flex h-10 w-10 shrink-0 items-center justify-center rounded-corner-md bg-secondary-container text-on-secondary-container"
        >
          <LuFolder class="h-5 w-5" aria-hidden="true" />
        </div>
        <div class="min-w-0 flex-1">
          <div class="flex items-center gap-2">
            <span class="text-title-small truncate text-on-surface">
              {{ shelf.name }}
            </span>
            <span
              v-if="shelf.is_favorite"
              class="text-label-small shrink-0 rounded-corner-full bg-primary-container px-2 py-0.5 text-on-primary-container"
            >
              {{ t('collections.defaultFavoriteBadge') }}
            </span>
          </div>
          <p class="text-body-small mt-0.5 tabular-nums text-on-surface-variant">
            {{ t('collections.bookCountUnit', { count: shelf.book_count }) }}
          </p>
        </div>
      </button>

      <MdMenu
        v-if="!shelf.is_favorite"
        placement="bottom-end"
        :aria-label="t('detail.moreActions')"
      >
        <template #trigger="{ toggle, triggerAttrs }">
          <MdIconButton
            v-bind="triggerAttrs"
            size="sm"
            variant="standard"
            :icon="LuMoreVertical"
            :aria-label="t('detail.moreActions')"
            @click="toggle"
          />
        </template>
        <MdMenuItem
          :icon="LuPencil"
          :label="t('collections.renameShelf')"
          @select="emit('rename', shelf)"
        />
        <MdMenuItem
          :icon="LuTrash2"
          :label="t('collections.deleteShelf')"
          destructive
          @select="emit('delete', shelf)"
        />
      </MdMenu>
    </div>
  </div>
</template>
