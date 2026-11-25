<script setup lang="ts">
import { computed } from 'vue'
import {
  AppCheckbox,
  AppMultiSelect,
  MdSegmentedButton,
  MdSelect,
  type SegmentedOption,
} from '@/components/ui'
import { useBookI18n } from '@/domains/book/i18n'
import { useCatalogStore } from '@/domains/book/store'
import { BOOK_FORMATS, isBookFormat, type TagMatchMode } from '@/domains/book/types'

defineProps<{
  activeCollectionId: string
}>()

const emit = defineEmits<{
  refetch: []
  selectShelf: [shelfId: string]
}>()

const { t } = useBookI18n()
const catalogStore = useCatalogStore()

const formatOptions = computed(() =>
  BOOK_FORMATS.map((fmt) => ({
    value: fmt,
    label: fmt.toUpperCase(),
  })),
)

const tagModeOptions = computed<SegmentedOption<TagMatchMode>[]>(() => [
  { value: 'any', label: t('search.tagModeAny') },
  { value: 'all', label: t('search.tagModeAll') },
])

const authorOptions = computed(() =>
  catalogStore.authors.map((a) => ({
    value: a.name,
    label: a.name,
    badge: a.count,
  })),
)

const tagOptions = computed(() =>
  catalogStore.tags.map((tg) => ({
    value: tg.name,
    label: tg.name,
    badge: tg.count,
  })),
)

const libraryOptions = computed(() =>
  catalogStore.libraries.map((lib) => ({
    value: lib.id,
    label: lib.name,
    badge: lib.book_count,
  })),
)

const shelfOptions = computed(() => [
  { value: '', label: t('search.allShelves') },
  ...catalogStore.shelves.map((sh) => ({
    value: sh.id,
    label: `${sh.name} (${sh.book_count})`,
  })),
])

function onUpdateFormats(vals: string[]): void {
  catalogStore.filterFormats = vals.filter(isBookFormat)
  emit('refetch')
}
</script>

<template>
  <div class="space-y-4">
    <AppMultiSelect
      :model-value="catalogStore.filterFormats"
      :options="formatOptions"
      :label="t('search.format')"
      :all-placeholder="t('search.allFormats')"
      @update:model-value="onUpdateFormats"
    />

    <AppMultiSelect
      :model-value="catalogStore.filterLibraryIds"
      :options="libraryOptions"
      :label="t('search.library')"
      :all-placeholder="t('search.allLibraries')"
      @update:model-value="
        (vals) => {
          catalogStore.filterLibraryIds = vals
          emit('refetch')
        }
      "
    />

    <div class="space-y-1.5">
      <label class="text-label-medium block text-on-surface-variant">
        {{ t('search.shelf') }}
      </label>
      <MdSelect
        :model-value="activeCollectionId"
        :options="shelfOptions"
        size="sm"
        @update:model-value="emit('selectShelf', String($event))"
      />
    </div>

    <AppMultiSelect
      :model-value="catalogStore.filterAuthors"
      :options="authorOptions"
      :label="t('search.author')"
      :all-placeholder="t('search.allAuthors')"
      @update:model-value="
        (vals) => {
          catalogStore.filterAuthors = vals
          emit('refetch')
        }
      "
    />

    <AppMultiSelect
      :model-value="catalogStore.filterTags"
      :options="tagOptions"
      :label="t('search.tag')"
      :all-placeholder="t('search.allTags')"
      @update:model-value="
        (vals) => {
          catalogStore.filterTags = vals
          emit('refetch')
        }
      "
    >
      <template #header>
        <MdSegmentedButton
          :model-value="catalogStore.filterTagMode"
          :options="tagModeOptions"
          size="sm"
          :show-check="false"
          full-width
          @update:model-value="
            (v) => {
              catalogStore.filterTagMode = v
              emit('refetch')
            }
          "
        />
      </template>
    </AppMultiSelect>

    <div class="rounded-corner-md bg-surface-container-low px-3.5 py-2.5">
      <AppCheckbox
        v-model="catalogStore.includeMissing"
        :label="t('search.includeMissing')"
        @change="emit('refetch')"
      />
    </div>
  </div>
</template>
