<script setup lang="ts">
import { LuArrowLeft, LuFolder, LuMoreVertical, LuPencil, LuTrash2 } from 'vue-icons-plus/lu'
import { MdButton, MdCard, MdIconButton, MdMenu, MdMenuItem } from '@/components/ui'
import { useComicI18n } from '@/domains/comic/i18n'
import type { ShelfDetailDto } from '@/domains/comic/types'

withDefaults(
  defineProps<{
    mode: 'subheader' | 'grid'
    activeCollection?: ShelfDetailDto | null
    shelves?: readonly ShelfDetailDto[]
  }>(),
  {
    activeCollection: null,
    shelves: () => [],
  },
)

const emit = defineEmits<{
  back: []
  select: [shelfId: string]
  edit: [shelf: ShelfDetailDto]
  delete: [shelf: ShelfDetailDto]
}>()

const { t } = useComicI18n()
</script>

<template>
  <MdCard v-if="mode === 'subheader' && activeCollection" variant="filled" padding="md">
    <div class="flex flex-wrap items-center justify-between gap-3">
      <div class="min-w-0 flex-1 space-y-1">
        <div class="flex flex-wrap items-center gap-2">
          <MdButton variant="text" :icon="LuArrowLeft" @click="emit('back')">
            {{ t('library.backToCollections') }}
          </MdButton>
          <span class="text-label-medium text-on-surface-variant">
            {{ t('library.collectionCount', { count: activeCollection.series.length }) }}
          </span>
        </div>
        <p
          v-if="activeCollection.shelf.description"
          class="text-body-small text-on-surface-variant"
        >
          {{ activeCollection.shelf.description }}
        </p>
      </div>

      <div class="flex shrink-0 items-center gap-1.5">
        <MdButton
          variant="tonal"
          :icon="LuPencil"
          @click="emit('edit', activeCollection)"
        >
          {{ t('shelves.editShelfBtn') }}
        </MdButton>
        <MdIconButton
          variant="standard"
          size="sm"
          :icon="LuTrash2"
          :aria-label="t('shelves.deleteShelf')"
          :tooltip="t('shelves.deleteShelf')"
          @click="emit('delete', activeCollection)"
        />
      </div>
    </div>
  </MdCard>

  <div v-else class="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-3">
    <MdCard
      v-for="sh in shelves"
      :key="sh.shelf.id"
      variant="filled"
      padding="md"
      class="group relative flex items-start justify-between gap-3 transition-shadow duration-150 ease-standard hover:shadow-elevation-1"
    >
      <button
        type="button"
        class="flex min-w-0 flex-1 cursor-pointer items-start gap-3 text-left outline-none after:absolute after:inset-0 after:rounded-corner-md focus-visible:after:outline-2 focus-visible:after:outline-offset-2 focus-visible:after:outline-secondary"
        @click="emit('select', sh.shelf.id)"
      >
        <div
          class="flex h-10 w-10 shrink-0 items-center justify-center rounded-corner-md bg-secondary-container text-on-secondary-container"
        >
          <LuFolder class="h-5 w-5" aria-hidden="true" />
        </div>
        <div class="min-w-0 flex-1">
          <h3 class="text-title-medium truncate text-on-surface group-hover:text-primary">
            {{ sh.shelf.name }}
          </h3>
          <p class="text-body-small mt-0.5 text-on-surface-variant">
            {{ t('library.collectionCount', { count: sh.series.length }) }}
          </p>
          <p
            v-if="sh.shelf.description"
            class="text-body-small mt-1 line-clamp-2 text-on-surface-variant/85"
          >
            {{ sh.shelf.description }}
          </p>
        </div>
      </button>

      <div class="relative z-10 shrink-0">
        <MdMenu placement="bottom-end" :aria-label="t('series.moreActions')">
          <template #trigger="{ toggle, triggerAttrs }">
            <MdIconButton
              v-bind="triggerAttrs"
              variant="standard"
              size="sm"
              :icon="LuMoreVertical"
              :aria-label="t('series.moreActions')"
              @click.stop="toggle"
            />
          </template>
          <template #default="{ close }">
            <MdMenuItem
              :label="t('shelves.editShelf')"
              :icon="LuPencil"
              @select="
                close();
                emit('edit', sh)
              "
            />
            <MdMenuItem
              :label="t('shelves.deleteShelf')"
              :icon="LuTrash2"
              destructive
              @select="
                close();
                emit('delete', sh)
              "
            />
          </template>
        </MdMenu>
      </div>
    </MdCard>
  </div>
</template>
