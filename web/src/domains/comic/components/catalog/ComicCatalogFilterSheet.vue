<script setup lang="ts">
import { computed } from 'vue'
import { MdSelect, type SelectOption } from '@/components/ui'
import { useComicI18n } from '@/domains/comic/i18n'
import { resolveLibrarySeriesCount, useSidebarStore } from '@/domains/comic/store'

defineProps<{
  libraryId: string
  statusFilter: string
  readStatusFilter: string
  sortBy: string
  sortOrder: string
}>()

const emit = defineEmits<{
  patchQuery: [patch: Record<string, string | null | undefined>]
}>()

const { t } = useComicI18n()
const sidebarStore = useSidebarStore()

const statusOptions = computed<SelectOption[]>(() => [
  { value: '', label: t('series.all') },
  { value: 'ongoing', label: t('series.statusOngoing') },
  { value: 'completed', label: t('series.statusCompleted') },
  { value: 'hiatus', label: t('series.statusHiatus') },
])

const readOptions = computed<SelectOption[]>(() => [
  { value: '', label: t('series.all') },
  { value: 'unread', label: t('series.readUnread') },
  { value: 'in_progress', label: t('series.readInProgress') },
  { value: 'completed', label: t('series.readCompleted') },
])

const libraryOptions = computed<SelectOption[]>(() => [
  { value: '', label: t('series.all') },
  ...sidebarStore.libraries.map((lib) => ({
    value: lib.id,
    label: lib.name,
    badge: resolveLibrarySeriesCount(lib),
  })),
])

const sortOptions = computed<SelectOption[]>(() => [
  { value: 'updated_at', label: t('series.sortUpdated') },
  { value: 'created_at', label: t('series.sortCreated') },
  { value: 'title', label: t('series.sortTitle') },
  { value: 'chapter_count', label: t('series.sortChapters') },
])

const orderOptions = computed<SelectOption[]>(() => [
  { value: 'desc', label: t('series.sortDesc') },
  { value: 'asc', label: t('series.sortAsc') },
])
</script>

<template>
  <div class="space-y-4">
    <div class="space-y-1.5">
      <span class="text-label-medium block text-on-surface-variant">
        {{ t('series.filterLibrary') }}
      </span>
      <MdSelect
        :model-value="libraryId"
        :options="libraryOptions"
        @update:model-value="emit('patchQuery', { library: String($event) || null, page: null })"
      />
    </div>

    <div class="space-y-1.5">
      <span class="text-label-medium block text-on-surface-variant">
        {{ t('series.filterStatus') }}
      </span>
      <MdSelect
        :model-value="statusFilter"
        :options="statusOptions"
        @update:model-value="emit('patchQuery', { status: String($event) || null, page: null })"
      />
    </div>

    <div class="space-y-1.5">
      <span class="text-label-medium block text-on-surface-variant">
        {{ t('series.filterReadStatus') }}
      </span>
      <MdSelect
        :model-value="readStatusFilter"
        :options="readOptions"
        @update:model-value="
          emit('patchQuery', { read_status: String($event) || null, page: null })
        "
      />
    </div>

    <div class="space-y-4 rounded-corner-md bg-surface-container-low p-3.5">
      <div class="space-y-1.5">
        <span class="text-label-medium block text-on-surface-variant">
          {{ t('library.sortLabel') }}
        </span>
        <MdSelect
          :model-value="sortBy"
          :options="sortOptions"
          @update:model-value="
            emit('patchQuery', { sort: String($event) === 'updated_at' ? null : String($event) })
          "
        />
      </div>

      <div class="space-y-1.5">
        <span class="text-label-medium block text-on-surface-variant">
          {{ t('library.sortOrderLabel') }}
        </span>
        <MdSelect
          :model-value="sortOrder"
          :options="orderOptions"
          @update:model-value="
            emit('patchQuery', { order: String($event) === 'desc' ? null : String($event) })
          "
        />
      </div>
    </div>
  </div>
</template>
