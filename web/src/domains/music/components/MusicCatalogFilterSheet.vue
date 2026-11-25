<script setup lang="ts">
import { useI18n as useGlobalI18n } from 'vue-i18n'
import { MdSelect, type SelectOption } from '@/components/ui'
import { useMusicI18n as useI18n } from '../i18n'
import type { AlbumListType, ArtistRoleFilter } from '../types'

defineProps<{
  activeView: 'items' | 'tracks' | 'artists' | 'genres' | 'tags'
  selectedLibrary: string
  selectedGenre: string
  selectedTag: string
  tracksSortKey: string
  albumListType: AlbumListType
  artistRoleFilter: ArtistRoleFilter
  libraryOptions: SelectOption[]
  genreOptions: SelectOption[]
  tagOptions: SelectOption[]
  trackSortOptions: SelectOption[]
  albumTypeOptions: SelectOption[]
  artistRoleOptions: SelectOption[]
}>()

const emit = defineEmits<{
  'update:library': [value: string]
  'update:genre': [value: string]
  'update:tag': [value: string]
  'update:trackSort': [value: string]
  'update:albumSort': [value: string]
  'update:artistRole': [value: string]
}>()

const { t } = useI18n()
const { t: globalT } = useGlobalI18n()
</script>

<template>
  <div class="space-y-4">
    <div v-if="activeView === 'items' || activeView === 'tracks'" class="space-y-1.5">
      <label class="text-label-medium text-on-surface-variant">
        {{ t('catalog.allLibraries') }}
      </label>
      <MdSelect
        :model-value="selectedLibrary"
        :options="libraryOptions"
        @update:model-value="emit('update:library', String($event))"
      />
    </div>

    <div v-if="activeView === 'items' || activeView === 'tracks'" class="space-y-1.5">
      <label class="text-label-medium text-on-surface-variant">
        {{ t('nav.genres') }}
      </label>
      <MdSelect
        :model-value="selectedGenre"
        :options="genreOptions"
        searchable
        @update:model-value="emit('update:genre', String($event))"
      />
    </div>

    <div v-if="activeView === 'tracks'" class="space-y-1.5">
      <label class="text-label-medium text-on-surface-variant">
        {{ t('nav.tags') }}
      </label>
      <MdSelect
        :model-value="selectedTag"
        :options="tagOptions"
        searchable
        @update:model-value="emit('update:tag', String($event))"
      />
    </div>

    <div v-if="activeView === 'tracks'" class="space-y-1.5">
      <label class="text-label-medium text-on-surface-variant">
        {{ globalT('common.sort') }}
      </label>
      <MdSelect
        :model-value="tracksSortKey"
        :options="trackSortOptions"
        @update:model-value="emit('update:trackSort', String($event))"
      />
    </div>

    <div v-else-if="activeView === 'items'" class="space-y-1.5">
      <label class="text-label-medium text-on-surface-variant">
        {{ globalT('common.sort') }}
      </label>
      <MdSelect
        :model-value="albumListType"
        :options="albumTypeOptions"
        @update:model-value="emit('update:albumSort', String($event))"
      />
    </div>

    <div v-else-if="activeView === 'artists'" class="space-y-1.5">
      <label class="text-label-medium text-on-surface-variant">
        {{ t('nav.artists') }}
      </label>
      <MdSelect
        :model-value="artistRoleFilter"
        :options="artistRoleOptions"
        @update:model-value="emit('update:artistRole', String($event))"
      />
    </div>
  </div>
</template>
