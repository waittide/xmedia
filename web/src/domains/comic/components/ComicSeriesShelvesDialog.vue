<script setup lang="ts">
import { AppCheckbox, MdButton, MdDialog } from '@/components/ui'
import { useComicI18n as useI18n } from '@/domains/comic/i18n'
import type { ShelfDetailDto } from '@/domains/comic/types'

defineProps<{
  open: boolean
  shelves: ShelfDetailDto[]
  selectedIds: string[]
  saving: boolean
}>()

const emit = defineEmits<{
  close: []
  'toggle-shelf': [id: string, checked: boolean]
  save: []
}>()

const { t } = useI18n()
</script>

<template>
  <MdDialog
    :open="open"
    :title="t('shelves.selectShelvesTitle')"
    size="sm"
    @close="emit('close')"
  >
    <div class="space-y-2.5">
      <p v-if="shelves.length === 0" class="text-body-medium text-on-surface-variant">
        {{ t('shelves.emptyShelves') }}
      </p>
      <div
        v-for="sh in shelves"
        :key="sh.shelf.id"
        class="rounded-corner-md bg-surface-container-low p-3"
      >
        <AppCheckbox
          :model-value="selectedIds.includes(sh.shelf.id)"
          :label="sh.shelf.name"
          :description="sh.shelf.description || undefined"
          @update:model-value="emit('toggle-shelf', sh.shelf.id, $event)"
        />
      </div>
    </div>

    <template #footer>
      <MdButton variant="text" :disabled="saving" @click="emit('close')">
        {{ t('common.cancel') }}
      </MdButton>
      <MdButton variant="filled" :loading="saving" :disabled="saving" @click="emit('save')">
        {{ t('common.save') }}
      </MdButton>
    </template>
  </MdDialog>
</template>
