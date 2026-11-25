<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { LuCheck, LuKey, LuSearch, LuSparkles } from 'vue-icons-plus/lu'
import MdButton from '@/components/ui/MdButton.vue'
import MdChip from '@/components/ui/MdChip.vue'
import MdSegmentedButton from '@/components/ui/MdSegmentedButton.vue'
import MdTextField from '@/components/ui/MdTextField.vue'
import type { MediaFileDetail, ProviderStatusDto, ScrapeTargetKind } from '../../types'
import type { ApplyFillMode } from '../inspector/useMetadataForm'

defineProps<{
  detail: MediaFileDetail | null
  candidatesCount: number
  fillMode: ApplyFillMode
  targetKind: ScrapeTargetKind
  targetTabs: Array<{ value: ScrapeTargetKind; label: string }>
  keyword: string
  artistOrAuthor: string
  albumOrSeries: string
  isbn: string
  yearInput: string
  seasonInput: string
  episodeInput: string
  canSearch: boolean
  searching: boolean
  providers: ProviderStatusDto[]
  enabledProviderIds: string[]
  recentFlash: string | null
  searchError: string | null
}>()

const emit = defineEmits<{
  'update:fillMode': [value: ApplyFillMode]
  'update:targetKind': [value: ScrapeTargetKind]
  'update:keyword': [value: string]
  'update:artistOrAuthor': [value: string]
  'update:albumOrSeries': [value: string]
  'update:isbn': [value: string]
  'update:yearInput': [value: string]
  'update:seasonInput': [value: string]
  'update:episodeInput': [value: string]
  search: []
  'toggle-provider': [provider: ProviderStatusDto]
}>()

const { t } = useI18n()

const fillModeOptions = computed(() => [
  { value: 'overwrite' as const, label: t('tool.workbench.search.modeOverwrite') },
  { value: 'fill_missing' as const, label: t('tool.workbench.search.modeFillMissing') },
])
</script>

<template>
  <div class="shrink-0 space-y-2.5 border-b border-outline-variant/50 p-3">
    <div class="flex items-center justify-between gap-2">
      <div class="text-title-small flex items-center gap-1.5 text-on-surface">
        <LuSparkles class="h-4 w-4 text-primary" aria-hidden="true" />
        <span>{{ t('tool.workbench.search.heading') }}</span>
        <span
          v-if="candidatesCount > 0"
          class="text-label-small rounded-corner-full bg-surface-container-high px-1.5 py-0.5 text-on-surface-variant"
        >
          {{ candidatesCount }}
        </span>
      </div>

      <MdSegmentedButton
        :model-value="fillMode"
        :options="fillModeOptions"
        size="sm"
        @update:model-value="emit('update:fillMode', $event)"
      />
    </div>

    <MdSegmentedButton
      v-if="detail && targetTabs.length > 1"
      :model-value="targetKind"
      :options="targetTabs"
      size="sm"
      @update:model-value="emit('update:targetKind', $event)"
    />

    <!-- Search Inputs -->
    <div class="flex items-center gap-1.5">
      <div class="min-w-0 flex-1">
        <MdTextField
          :model-value="keyword"
          size="sm"
          :disabled="!detail"
          :placeholder="t('tool.workbench.search.keywordPlaceholder')"
          :aria-label="t('tool.workbench.search.keywordPlaceholder')"
          @update:model-value="emit('update:keyword', $event)"
          @enter="emit('search')"
        />
      </div>
      <MdButton
        variant="filled"
        size="sm"
        :icon="LuSearch"
        :loading="searching"
        :disabled="!canSearch"
        @click="emit('search')"
      >
        {{ t('tool.common.search') }}
      </MdButton>
    </div>

    <!-- Domain-Specific Secondary Query Inputs -->
    <div v-if="detail?.entry.media_kind === 'book'" class="grid grid-cols-2 gap-1.5">
      <MdTextField
        :model-value="artistOrAuthor"
        size="sm"
        :placeholder="t('tool.workbench.search.authorOptional')"
        :aria-label="t('tool.workbench.search.authorOptional')"
        @update:model-value="emit('update:artistOrAuthor', $event)"
        @enter="emit('search')"
      />
      <MdTextField
        :model-value="isbn"
        size="sm"
        :placeholder="t('tool.workbench.search.isbnOptional')"
        :aria-label="t('tool.workbench.search.isbnOptional')"
        @update:model-value="emit('update:isbn', $event)"
        @enter="emit('search')"
      />
    </div>

    <div v-else-if="detail?.entry.media_kind === 'music'" class="grid grid-cols-5 gap-1.5">
      <div class="col-span-2">
        <MdTextField
          :model-value="artistOrAuthor"
          size="sm"
          :placeholder="t('tool.workbench.search.artistOptional')"
          :aria-label="t('tool.workbench.search.artistOptional')"
          @update:model-value="emit('update:artistOrAuthor', $event)"
          @enter="emit('search')"
        />
      </div>
      <div class="col-span-2">
        <MdTextField
          :model-value="albumOrSeries"
          size="sm"
          :placeholder="t('tool.workbench.search.albumOptional')"
          :aria-label="t('tool.workbench.search.albumOptional')"
          @update:model-value="emit('update:albumOrSeries', $event)"
          @enter="emit('search')"
        />
      </div>
      <MdTextField
        :model-value="yearInput"
        size="sm"
        :placeholder="t('tool.workbench.search.yearOptional')"
        :aria-label="t('tool.workbench.search.yearOptional')"
        @update:model-value="emit('update:yearInput', $event)"
        @enter="emit('search')"
      />
    </div>

    <div
      v-else-if="detail?.entry.media_kind === 'video' && targetKind === 'tv_show'"
      class="grid grid-cols-3 gap-1.5"
    >
      <MdTextField
        :model-value="seasonInput"
        size="sm"
        :placeholder="t('tool.workbench.search.seasonShort')"
        :aria-label="t('tool.workbench.search.seasonShort')"
        @update:model-value="emit('update:seasonInput', $event)"
        @enter="emit('search')"
      />
      <MdTextField
        :model-value="episodeInput"
        size="sm"
        :placeholder="t('tool.workbench.search.episodeShort')"
        :aria-label="t('tool.workbench.search.episodeShort')"
        @update:model-value="emit('update:episodeInput', $event)"
        @enter="emit('search')"
      />
      <MdTextField
        :model-value="yearInput"
        size="sm"
        :placeholder="t('tool.workbench.search.yearOptional')"
        :aria-label="t('tool.workbench.search.yearOptional')"
        @update:model-value="emit('update:yearInput', $event)"
        @enter="emit('search')"
      />
    </div>

    <div v-else-if="detail?.entry.media_kind === 'video'" class="grid grid-cols-2 gap-1.5">
      <MdTextField
        :model-value="yearInput"
        size="sm"
        :placeholder="t('tool.workbench.search.releaseYearOptional')"
        :aria-label="t('tool.workbench.search.releaseYearOptional')"
        @update:model-value="emit('update:yearInput', $event)"
        @enter="emit('search')"
      />
    </div>

    <div v-else class="grid grid-cols-3 gap-1.5">
      <div class="col-span-2">
        <MdTextField
          :model-value="artistOrAuthor"
          size="sm"
          :disabled="!detail"
          :placeholder="t('tool.workbench.search.creatorOptional')"
          :aria-label="t('tool.workbench.search.creatorOptional')"
          @update:model-value="emit('update:artistOrAuthor', $event)"
          @enter="emit('search')"
        />
      </div>
      <MdTextField
        :model-value="yearInput"
        size="sm"
        :disabled="!detail"
        :placeholder="t('tool.workbench.search.yearOptional')"
        :aria-label="t('tool.workbench.search.yearOptional')"
        @update:model-value="emit('update:yearInput', $event)"
        @enter="emit('search')"
      />
    </div>

    <!-- Provider Toggle Chips -->
    <div v-if="providers.length > 0" class="flex flex-wrap gap-1">
      <MdChip
        v-for="p in providers"
        :key="p.id"
        variant="filter"
        hide-check
        :selected="p.configured && enabledProviderIds.includes(p.id)"
        :title="p.description"
        @click="emit('toggle-provider', p)"
      >
        <template #icon>
          <LuKey
            v-if="!p.configured && p.requires_api_key"
            class="h-3.5 w-3.5 shrink-0 text-warning"
            aria-hidden="true"
          />
        </template>
        <span
          v-if="p.configured && enabledProviderIds.includes(p.id)"
          class="mr-1 font-mono opacity-80"
        >
          #{{ enabledProviderIds.indexOf(p.id) + 1 }}
        </span>
        <span>{{ p.name }}</span>
      </MdChip>
    </div>

    <!-- Flash Feedback -->
    <div
      v-if="recentFlash"
      class="text-body-small flex items-center gap-1.5 rounded-corner-md bg-success-container px-2.5 py-1 text-on-success-container"
    >
      <LuCheck class="h-3.5 w-3.5 shrink-0" aria-hidden="true" />
      <span class="truncate">{{ recentFlash }}</span>
    </div>
    <div
      v-if="searchError"
      role="alert"
      class="text-body-small rounded-corner-md bg-error-container px-2.5 py-1.5 text-on-error-container"
    >
      {{ searchError }}
    </div>
  </div>
</template>
