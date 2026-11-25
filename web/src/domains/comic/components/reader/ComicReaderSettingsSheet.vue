<script setup lang="ts">
import { computed } from 'vue'
import {
  LuColumns2,
  LuEye,
  LuFileText,
  LuScrollText,
  LuZoomIn,
  LuZoomOut,
} from 'vue-icons-plus/lu'
import {
  AppCheckbox,
  AppSlider,
  MdButton,
  MdIconButton,
  MdSegmentedButton,
  MdSwitch,
  type SegmentedOption,
} from '@/components/ui'
import { useComicI18n } from '@/domains/comic/i18n'
import type { ReaderFitMode, ReaderMode } from '@/domains/comic/types'

defineProps<{
  readerMode: ReaderMode
  fitMode: ReaderFitMode
  zoomPercent: number
  offsetCoverPage: boolean
  webtoonWidth: number
  dimImages: boolean
}>()

const emit = defineEmits<{
  'update:readerMode': [mode: ReaderMode]
  'update:fitMode': [fit: ReaderFitMode]
  'adjust-zoom': [delta: number]
  'reset-zoom': []
  'update:offsetCoverPage': [val: boolean]
  'update:webtoonWidth': [val: number]
  'commit-webtoon-width': [val: number]
  'update:dimImages': [val: boolean]
}>()

const { t } = useComicI18n()

const modeOptions = computed<SegmentedOption<ReaderMode>[]>(() => [
  { value: 'rtl_double', label: t('reader.modeRtlDouble'), icon: LuColumns2 },
  { value: 'ltr_single', label: t('reader.modeLtrSingle'), icon: LuFileText },
  { value: 'ltr_double', label: t('reader.modeLtrDouble'), icon: LuColumns2 },
  { value: 'webtoon', label: t('reader.modeWebtoon'), icon: LuScrollText },
])

const fitOptions = computed<SegmentedOption<ReaderFitMode>[]>(() => [
  { value: 'fit_height', label: t('reader.fitHeight') },
  { value: 'fit_width', label: t('reader.fitWidth') },
  { value: 'original', label: t('reader.fitOriginal') },
])
</script>

<template>
  <div class="space-y-5">
    <div class="space-y-2">
      <span class="text-label-medium block text-on-surface-variant">
        {{ t('settings.defaultReaderMode') }}
      </span>
      <div class="grid grid-cols-2 gap-2">
        <MdButton
          v-for="opt in modeOptions"
          :key="opt.value"
          :variant="readerMode === opt.value ? 'tonal' : 'outlined'"
          :icon="opt.icon"
          @click="emit('update:readerMode', opt.value)"
        >
          {{ opt.label }}
        </MdButton>
      </div>
    </div>

    <div v-if="readerMode !== 'webtoon'" class="space-y-2">
      <span class="text-label-medium block text-on-surface-variant">
        {{ t('settings.defaultFitMode') }}
      </span>
      <MdSegmentedButton
        :model-value="fitMode"
        :options="fitOptions"
        size="sm"
        full-width
        :show-check="false"
        @update:model-value="emit('update:fitMode', $event)"
      />
    </div>

    <div class="space-y-2">
      <div class="flex items-center justify-between gap-2">
        <span class="text-label-medium text-on-surface-variant">
          {{ t('reader.zoomLabel') }}
        </span>
        <MdButton
          variant="text"
          :disabled="zoomPercent === 100"
          @click="emit('reset-zoom')"
        >
          {{ t('reader.zoomReset') }}
        </MdButton>
      </div>
      <div class="flex items-center justify-between gap-2 rounded-corner-md bg-surface-container-low px-3 py-1.5">
        <MdIconButton
          size="sm"
          variant="standard"
          :icon="LuZoomOut"
          :disabled="zoomPercent <= 50"
          :aria-label="t('reader.zoomOut')"
          @click="emit('adjust-zoom', -10)"
        />
        <button
          type="button"
          class="text-label-large cursor-pointer rounded-corner-full px-3 py-1 font-mono tabular-nums text-on-surface hover:bg-surface-container-high"
          :title="t('reader.zoomReset')"
          @click="emit('reset-zoom')"
        >
          {{ zoomPercent }}%
        </button>
        <MdIconButton
          size="sm"
          variant="standard"
          :icon="LuZoomIn"
          :disabled="zoomPercent >= 200"
          :aria-label="t('reader.zoomIn')"
          @click="emit('adjust-zoom', 10)"
        />
      </div>
    </div>

    <div v-if="readerMode === 'rtl_double' || readerMode === 'ltr_double'">
      <AppCheckbox
        :model-value="offsetCoverPage"
        :label="t('reader.offsetCover')"
        @update:model-value="emit('update:offsetCoverPage', $event)"
      />
    </div>

    <div v-if="readerMode === 'webtoon'" class="space-y-2">
      <div class="flex items-center justify-between gap-2">
        <span class="text-label-medium text-on-surface-variant">
          {{ t('reader.webtoonWidth') }}
        </span>
        <span class="text-label-medium font-mono tabular-nums text-on-surface">
          {{ webtoonWidth }} px
        </span>
      </div>
      <AppSlider
        :model-value="webtoonWidth"
        :min="520"
        :max="1400"
        :step="20"
        :aria-label="t('reader.webtoonWidth')"
        @update:model-value="emit('update:webtoonWidth', $event)"
        @change="emit('commit-webtoon-width', $event)"
      />
    </div>

    <div class="flex items-center justify-between gap-3 rounded-corner-md bg-surface-container-low px-3.5 py-2.5">
      <div class="text-body-medium flex items-center gap-2 text-on-surface">
        <LuEye class="h-4 w-4 text-primary" aria-hidden="true" />
        <span>{{ t('reader.eyeCareDim') }}</span>
      </div>
      <MdSwitch
        :model-value="dimImages"
        :aria-label="t('reader.dimToggleAria')"
        @update:model-value="emit('update:dimImages', $event)"
      />
    </div>

    <p class="text-body-small rounded-corner-md bg-surface-container-low px-3.5 py-2.5 text-on-surface-variant">
      {{ t('reader.shortcutsHint') }}
    </p>
  </div>
</template>
