<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { LuCheck, LuDownload } from 'vue-icons-plus/lu'
import {
  MdButton,
  MdChip,
  MdSelect,
  MdTextField,
  type SelectOption,
} from '@/components/ui'
import { VIDEO_SPEED_OPTIONS } from '@/domains/video/composables/useVideoPlayerCore'
import { useVideoI18n as useI18n } from '@/domains/video/i18n'

export interface VideoPlayerEpisodeItem {
  id: string
  label: string
  subtitle?: string
  active: boolean
  watched?: boolean
  disabled?: boolean
}

const props = defineProps<{
  sheetKey: string | null
  teleportTo: HTMLElement | null
  playbackRate: number
  qualityValue: string
  qualityOptions: SelectOption[]
  lineValue: string
  lineOptions: SelectOption[]
  audioValue: string
  audioOptions: SelectOption[]
  subtitleValue: string
  subtitleOptions: SelectOption[]
  downloadUrl: string | null
  episodes: VideoPlayerEpisodeItem[]
  episodeGroupValue: string
  episodeGroupOptions: SelectOption[]
}>()

const emit = defineEmits<{
  'select-speed': [rateValue: string]
  'update:quality': [value: string]
  'update:line': [value: string]
  'update:audio': [value: string]
  'update:subtitle': [value: string]
  'select-episode': [id: string]
}>()

const { t } = useI18n()

const episodeFilter = ref('')

watch(
  () => props.sheetKey,
  (next) => {
    if (next !== 'episodes') {
      episodeFilter.value = ''
    }
  },
)

const filteredEpisodes = computed(() => {
  const q = episodeFilter.value.trim().toLowerCase()
  if (!q) return props.episodes
  return props.episodes.filter(
    (ep) =>
      ep.label.toLowerCase().includes(q) ||
      (ep.subtitle?.toLowerCase().includes(q) ?? false),
  )
})
</script>

<template>
  <div v-if="sheetKey === 'episodes'" class="space-y-4">
    <div v-if="episodeGroupOptions.length > 1" class="space-y-1.5">
      <span class="text-label-medium block text-on-surface-variant">
        {{ t('player.lineLabel') }}
      </span>
      <MdSelect
        :model-value="episodeGroupValue"
        :options="episodeGroupOptions"
        size="sm"
        :teleport-to="teleportTo"
        @change="emit('update:line', $event)"
      />
    </div>

    <MdTextField
      v-if="episodes.length > 12"
      v-model="episodeFilter"
      type="search"
      size="sm"
      clearable
      :placeholder="t('online.filterEpisodesPlaceholder')"
    />

    <div class="space-y-1.5">
      <button
        v-for="ep in filteredEpisodes"
        :key="ep.id"
        type="button"
        :disabled="ep.disabled"
        class="md-state flex w-full cursor-pointer items-center justify-between gap-3 rounded-corner-md px-3.5 py-2.5 text-left transition-colors duration-150 ease-standard disabled:cursor-not-allowed disabled:opacity-38"
        :class="
          ep.active
            ? 'bg-primary-container text-on-primary-container'
            : 'bg-surface-container-low text-on-surface hover:bg-surface-container-high'
        "
        @click="emit('select-episode', ep.id)"
      >
        <div class="min-w-0 flex-1">
          <div class="text-title-small truncate">{{ ep.label }}</div>
          <div v-if="ep.subtitle" class="text-body-small mt-0.5 truncate opacity-80">
            {{ ep.subtitle }}
          </div>
        </div>
        <LuCheck v-if="ep.active || ep.watched" class="h-4 w-4 shrink-0" aria-hidden="true" />
      </button>
    </div>
  </div>

  <div v-else-if="sheetKey === 'settings'" class="space-y-5">
    <div class="space-y-2">
      <span class="text-label-medium block text-on-surface-variant">
        {{ t('player.speed') }}
      </span>
      <div class="flex flex-wrap gap-1.5">
        <MdChip
          v-for="opt in VIDEO_SPEED_OPTIONS"
          :key="opt.value"
          variant="filter"
          :label="opt.label"
          :selected="String(playbackRate) === opt.value"
          hide-check
          @click="emit('select-speed', opt.value)"
        />
      </div>
    </div>

    <div v-if="qualityOptions.length > 0" class="space-y-1.5">
      <span class="text-label-medium block text-on-surface-variant">
        {{ t('player.quality') }}
      </span>
      <MdSelect
        :model-value="qualityValue"
        :options="qualityOptions"
        size="sm"
        :teleport-to="teleportTo"
        @change="emit('update:quality', $event)"
      />
    </div>

    <div v-if="lineOptions.length > 1" class="space-y-1.5">
      <span class="text-label-medium block text-on-surface-variant">
        {{ t('player.lineLabel') }}
      </span>
      <MdSelect
        :model-value="lineValue"
        :options="lineOptions"
        size="sm"
        :teleport-to="teleportTo"
        @change="emit('update:line', $event)"
      />
    </div>

    <div v-if="audioOptions.length > 1" class="space-y-1.5">
      <span class="text-label-medium block text-on-surface-variant">
        {{ t('player.audioTrack') }}
      </span>
      <MdSelect
        :model-value="audioValue"
        :options="audioOptions"
        size="sm"
        :teleport-to="teleportTo"
        @change="emit('update:audio', $event)"
      />
    </div>

    <div v-if="subtitleOptions.length > 1" class="space-y-1.5">
      <span class="text-label-medium block text-on-surface-variant">
        {{ t('player.subtitles') }}
      </span>
      <MdSelect
        :model-value="subtitleValue"
        :options="subtitleOptions"
        size="sm"
        :teleport-to="teleportTo"
        @change="emit('update:subtitle', $event)"
      />
    </div>

    <div v-if="downloadUrl" class="pt-2">
      <MdButton variant="tonal" :icon="LuDownload" :href="downloadUrl" block>
        {{ t('common.download') }}
      </MdButton>
    </div>
  </div>
</template>
