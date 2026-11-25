<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import { LuDatabase, LuFolderPlus, LuLibrary, LuUserCheck } from 'vue-icons-plus/lu'
import { MdButton, MdChip, MdSelect, type SelectOption } from '@/components/ui'
import type { CoreLibraryDto } from '@/types/api'

defineProps<{
  libraries: readonly CoreLibraryDto[]
  selectedLibraryId: string | null
  isAdmin: boolean
  userSelectOptions: readonly SelectOption[]
  targetUserId: string
  visibleUnboundCount: number
  batchBusy: boolean
}>()

const emit = defineEmits<{
  'select-library': [id: string | null]
  'update:targetUserId': [userId: string]
  'bind-library': []
  'bind-visible': []
}>()

const { t } = useI18n()
</script>

<template>
  <div class="flex flex-wrap items-center justify-between gap-2.5">
    <div
      role="group"
      :aria-label="t('pool.allLibraries')"
      class="no-scrollbar flex min-w-0 flex-1 items-center gap-1.5 overflow-x-auto py-0.5"
    >
      <MdChip
        variant="filter"
        :icon="LuDatabase"
        :label="t('pool.allLibraries')"
        :selected="selectedLibraryId === null"
        hide-check
        @click="emit('select-library', null)"
      />
      <MdChip
        v-for="lib in libraries"
        :key="lib.id"
        variant="filter"
        :icon="LuLibrary"
        :label="`${lib.name} (${lib.item_count})`"
        :selected="selectedLibraryId === lib.id"
        hide-check
        @click="emit('select-library', lib.id)"
      />
    </div>

    <div class="flex shrink-0 flex-wrap items-center gap-2">
      <div v-if="isAdmin && userSelectOptions.length > 1" class="flex items-center gap-1.5">
        <LuUserCheck class="h-4 w-4 text-on-surface-variant" aria-hidden="true" />
        <MdSelect
          :model-value="targetUserId"
          :options="userSelectOptions as SelectOption[]"
          variant="compact"
          :placeholder="t('pool.assignTargetLabel')"
          :aria-label="t('pool.assignTargetLabel')"
          @update:model-value="emit('update:targetUserId', $event)"
        />
      </div>

      <MdButton
        v-if="selectedLibraryId"
        variant="tonal"
        :icon="LuFolderPlus"
        :loading="batchBusy"
        @click="emit('bind-library')"
      >
        {{ t('pool.bindLibraryBtn') }}
      </MdButton>
      <MdButton
        v-else-if="visibleUnboundCount > 0"
        variant="tonal"
        :icon="LuFolderPlus"
        :loading="batchBusy"
        @click="emit('bind-visible')"
      >
        {{ t('pool.bindAllVisibleBtn') }}
      </MdButton>
    </div>
  </div>
</template>
