<script setup lang="ts">
import { LuCheck, LuKey, LuLoader2, LuSearch, LuSparkles } from 'vue-icons-plus/lu'
import MdSegmentedButton from '@/components/ui/MdSegmentedButton.vue'
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

const fillModeOptions = [
  { value: 'overwrite' as const, label: '覆盖' },
  { value: 'fill_missing' as const, label: '仅补空' },
]
</script>

<template>
  <div class="shrink-0 space-y-2.5 border-b border-[var(--md-sys-color-outline-variant)]/50 p-3">
    <div class="flex items-center justify-between gap-2">
      <div class="flex items-center gap-1.5 text-xs font-bold text-[var(--md-sys-color-on-surface)]">
        <LuSparkles class="h-3.5 w-3.5 text-[var(--md-sys-color-primary)]" />
        <span>在线候选源</span>
        <span
          v-if="candidatesCount > 0"
          class="rounded-full bg-[var(--md-sys-color-surface-container-high)] px-1.5 py-0.5 text-[10px] font-medium text-[var(--md-sys-color-on-surface-variant)]"
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
    <div class="flex gap-1.5">
      <input
        :value="keyword"
        type="text"
        :disabled="!detail"
        placeholder="标题 / 关键词 / ID…"
        class="md-input h-8 min-w-0 flex-1 text-xs disabled:opacity-50"
        @input="emit('update:keyword', ($event.target as HTMLInputElement).value)"
        @keydown.enter.prevent="emit('search')"
      />
      <button
        type="button"
        :disabled="!canSearch || searching"
        class="md-btn md-btn-sm md-btn-filled md-state shrink-0"
        @click="emit('search')"
      >
        <LuLoader2 v-if="searching" class="h-3.5 w-3.5 animate-spin" />
        <LuSearch v-else class="h-3.5 w-3.5" />
        <span>搜索</span>
      </button>
    </div>

    <!-- Domain-Specific Secondary Query Inputs -->
    <div v-if="detail?.entry.media_kind === 'book'" class="grid grid-cols-2 gap-1.5">
      <input
        :value="artistOrAuthor"
        type="text"
        placeholder="作者 (可选)"
        class="md-input h-7 !rounded-lg !px-2 text-[11px]"
        @input="emit('update:artistOrAuthor', ($event.target as HTMLInputElement).value)"
        @keydown.enter.prevent="emit('search')"
      />
      <input
        :value="isbn"
        type="text"
        placeholder="ISBN (可选)"
        class="md-input h-7 !rounded-lg !px-2 text-[11px]"
        @input="emit('update:isbn', ($event.target as HTMLInputElement).value)"
        @keydown.enter.prevent="emit('search')"
      />
    </div>

    <div v-else-if="detail?.entry.media_kind === 'music'" class="grid grid-cols-5 gap-1.5">
      <input
        :value="artistOrAuthor"
        type="text"
        placeholder="歌手 (可选)"
        class="md-input col-span-2 h-7 !rounded-lg !px-2 text-[11px]"
        @input="emit('update:artistOrAuthor', ($event.target as HTMLInputElement).value)"
        @keydown.enter.prevent="emit('search')"
      />
      <input
        :value="albumOrSeries"
        type="text"
        placeholder="专辑 (可选)"
        class="md-input col-span-2 h-7 !rounded-lg !px-2 text-[11px]"
        @input="emit('update:albumOrSeries', ($event.target as HTMLInputElement).value)"
        @keydown.enter.prevent="emit('search')"
      />
      <input
        :value="yearInput"
        type="text"
        placeholder="年份"
        class="md-input h-7 !rounded-lg !px-2 text-[11px]"
        @input="emit('update:yearInput', ($event.target as HTMLInputElement).value)"
        @keydown.enter.prevent="emit('search')"
      />
    </div>

    <div
      v-else-if="detail?.entry.media_kind === 'video' && targetKind === 'tv_show'"
      class="grid grid-cols-3 gap-1.5"
    >
      <input
        :value="seasonInput"
        type="text"
        placeholder="季 (S)"
        class="md-input h-7 !rounded-lg !px-2 text-[11px]"
        @input="emit('update:seasonInput', ($event.target as HTMLInputElement).value)"
        @keydown.enter.prevent="emit('search')"
      />
      <input
        :value="episodeInput"
        type="text"
        placeholder="集 (E)"
        class="md-input h-7 !rounded-lg !px-2 text-[11px]"
        @input="emit('update:episodeInput', ($event.target as HTMLInputElement).value)"
        @keydown.enter.prevent="emit('search')"
      />
      <input
        :value="yearInput"
        type="text"
        placeholder="年份"
        class="md-input h-7 !rounded-lg !px-2 text-[11px]"
        @input="emit('update:yearInput', ($event.target as HTMLInputElement).value)"
        @keydown.enter.prevent="emit('search')"
      />
    </div>

    <div v-else-if="detail?.entry.media_kind === 'video'" class="grid grid-cols-2 gap-1.5">
      <input
        :value="yearInput"
        type="text"
        placeholder="上映年份 (可选)"
        class="md-input h-7 !rounded-lg !px-2 text-[11px]"
        @input="emit('update:yearInput', ($event.target as HTMLInputElement).value)"
        @keydown.enter.prevent="emit('search')"
      />
    </div>

    <div v-else class="grid grid-cols-3 gap-1.5">
      <input
        :value="artistOrAuthor"
        type="text"
        :disabled="!detail"
        placeholder="作者/画师（可选）"
        class="md-input col-span-2 h-7 !rounded-lg !px-2 text-[11px] disabled:opacity-50"
        @input="emit('update:artistOrAuthor', ($event.target as HTMLInputElement).value)"
        @keydown.enter.prevent="emit('search')"
      />
      <input
        :value="yearInput"
        type="text"
        :disabled="!detail"
        placeholder="年份"
        class="md-input h-7 !rounded-lg !px-2 text-[11px] disabled:opacity-50"
        @input="emit('update:yearInput', ($event.target as HTMLInputElement).value)"
        @keydown.enter.prevent="emit('search')"
      />
    </div>

    <!-- Provider Toggle Chips -->
    <div v-if="providers.length > 0" class="flex flex-wrap gap-1">
      <button
        v-for="p in providers"
        :key="p.id"
        type="button"
        :title="p.description"
        class="md-state inline-flex items-center gap-1 rounded-full border px-2 py-0.5 text-[10px] font-medium transition"
        :class="
          !p.configured
            ? 'border-dashed border-[var(--md-sys-color-warning)]/55 bg-[var(--md-sys-color-warning-container)]/50 text-[var(--md-sys-color-on-warning-container)]'
            : enabledProviderIds.includes(p.id)
              ? 'border-[var(--md-sys-color-primary)] bg-[var(--md-sys-color-primary-container)] text-[var(--md-sys-color-on-primary-container)]'
              : 'border-[var(--md-sys-color-outline-variant)]/60 bg-[var(--md-sys-color-surface)] text-[var(--md-sys-color-on-surface-variant)]'
        "
        @click="emit('toggle-provider', p)"
      >
        <span
          v-if="p.configured && enabledProviderIds.includes(p.id)"
          class="font-mono text-[9px] font-bold opacity-80"
        >
          #{{ enabledProviderIds.indexOf(p.id) + 1 }}
        </span>
        <LuKey v-else-if="!p.configured && p.requires_api_key" class="h-2.5 w-2.5" />
        <span>{{ p.name }}</span>
      </button>
    </div>

    <!-- Flash Feedback -->
    <div
      v-if="recentFlash"
      class="flex items-center gap-1.5 rounded-xl bg-[var(--md-sys-color-success-container)] px-2.5 py-1 text-[11px] font-medium text-[var(--md-sys-color-on-success-container)]"
    >
      <LuCheck class="h-3 w-3 shrink-0" />
      <span class="truncate">{{ recentFlash }}</span>
    </div>
    <div
      v-if="searchError"
      class="rounded-xl bg-[var(--md-sys-color-error-container)] px-2.5 py-1.5 text-[11px] text-[var(--md-sys-color-on-error-container)]"
    >
      {{ searchError }}
    </div>
  </div>
</template>
