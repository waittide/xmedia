<script setup lang="ts">
import { computed } from 'vue'
import { MdSelect, type SelectOption } from '@/components/ui'
import { useVideoI18n } from '@/domains/video/i18n'
import { useVideoStore } from '@/domains/video/store'
import type { VideoKindFilter } from '@/domains/video/types'

defineProps<{
  selectedLibrary: string
  selectedGenre: string
  selectedTag: string
  kindFilter: VideoKindFilter
  sortKey: string
  sortOptions: SelectOption[]
  activeView: string
  showVideoSort: boolean
}>()

const emit = defineEmits<{
  patchQuery: [patch: Record<string, string | null | undefined>]
  sortChange: [val: string]
}>()

const { t } = useVideoI18n()
const videoStore = useVideoStore()

const libraryOptions = computed<SelectOption[]>(() => [
  { value: '', label: t('catalog.allLibraries') },
  ...videoStore.libraries.map((l) => ({ value: l.id, label: l.name, badge: l.video_count })),
])

const genreOptions = computed<SelectOption[]>(() => [
  { value: '', label: t('catalog.allGenres') },
  ...videoStore.genres.map((g) => ({ value: g.name, label: g.name, badge: g.count })),
])

const tagOptions = computed<SelectOption[]>(() => [
  { value: '', label: t('catalog.allTags') },
  ...videoStore.tags.map((tg) => ({ value: tg.name, label: `#${tg.name}`, badge: tg.count })),
])

const kindOptions = computed<SelectOption[]>(() => [
  { value: 'all', label: t('catalog.kindAll') },
  { value: 'standalone', label: t('catalog.kindStandalone') },
  { value: 'episodes', label: t('catalog.kindEpisodes') },
])
</script>

<template>
  <div class="space-y-4">
    <div class="space-y-1.5">
      <span class="text-label-medium block text-on-surface-variant">
        {{ t('catalog.libraryLabel') }}
      </span>
      <MdSelect
        :model-value="selectedLibrary"
        :options="libraryOptions"
        size="sm"
        @change="emit('patchQuery', { library: $event || null })"
      />
    </div>

    <div class="space-y-1.5">
      <span class="text-label-medium block text-on-surface-variant">
        {{ t('catalog.genreLabel') }}
      </span>
      <MdSelect
        :model-value="selectedGenre"
        :options="genreOptions"
        size="sm"
        searchable
        @change="emit('patchQuery', { genre: $event || null })"
      />
    </div>

    <div v-if="videoStore.tags.length > 0 || selectedTag" class="space-y-1.5">
      <span class="text-label-medium block text-on-surface-variant">
        {{ t('catalog.tagLabel') }}
      </span>
      <MdSelect
        :model-value="selectedTag"
        :options="tagOptions"
        size="sm"
        searchable
        @change="emit('patchQuery', { tag: $event || null })"
      />
    </div>

    <div v-if="activeView === 'items'" class="space-y-1.5">
      <span class="text-label-medium block text-on-surface-variant">
        {{ t('catalog.kindLabel') }}
      </span>
      <MdSelect
        :model-value="kindFilter"
        :options="kindOptions"
        size="sm"
        @change="emit('patchQuery', { kind: $event === 'all' ? null : $event })"
      />
    </div>

    <div v-if="showVideoSort" class="space-y-1.5">
      <span class="text-label-medium block text-on-surface-variant">
        {{ t('catalog.sortLabel') }}
      </span>
      <MdSelect
        :model-value="sortKey"
        :options="sortOptions"
        size="sm"
        @change="emit('sortChange', $event)"
      />
    </div>
  </div>
</template>
