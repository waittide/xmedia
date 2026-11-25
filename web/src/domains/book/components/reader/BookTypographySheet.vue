<script setup lang="ts">
import { computed } from 'vue'
import { AppSlider, MdSegmentedButton, type SegmentedOption } from '@/components/ui'
import { useBookI18n as useI18n } from '@/domains/book/i18n'
import { usePreferencesStore } from '@/stores/preferences'

withDefaults(
  defineProps<{
    showFlowMode?: boolean
  }>(),
  {
    showFlowMode: false,
  },
)

const { t } = useI18n()
const preferencesStore = usePreferencesStore()

const flowOptions = computed<SegmentedOption<'paginated' | 'scrolled'>[]>(() => [
  { value: 'paginated', label: t('reader.paginated') },
  { value: 'scrolled', label: t('reader.scrolled') },
])

const fontOptions = computed<SegmentedOption<string>[]>(() => [
  { value: 'system-ui', label: t('reader.fontSystem') },
  { value: 'serif', label: t('reader.fontSerif') },
  { value: 'mono', label: t('reader.fontMono') },
])
</script>

<template>
  <div class="space-y-5">
    <div v-if="showFlowMode" class="space-y-2">
      <label class="text-label-medium block text-on-surface-variant">
        {{ t('reader.flowMode') }}
      </label>
      <MdSegmentedButton
        :model-value="preferencesStore.reader.flow"
        :options="flowOptions"
        size="sm"
        full-width
        @update:model-value="(v) => preferencesStore.updateReaderPreferences({ flow: v })"
      />
    </div>

    <div class="space-y-2">
      <label class="text-label-medium block text-on-surface-variant">
        {{ t('reader.fontFamily') }}
      </label>
      <MdSegmentedButton
        :model-value="preferencesStore.reader.font_family"
        :options="fontOptions"
        size="sm"
        full-width
        @update:model-value="(v) => preferencesStore.updateReaderPreferences({ font_family: v })"
      />
    </div>

    <div class="space-y-1.5">
      <div class="flex items-center justify-between">
        <span class="text-label-medium text-on-surface-variant">{{ t('reader.fontSize') }}</span>
        <span class="text-label-medium font-mono tabular-nums text-on-surface">
          {{ preferencesStore.reader.font_size }} px
        </span>
      </div>
      <AppSlider
        :model-value="preferencesStore.reader.font_size"
        :min="14"
        :max="30"
        :step="1"
        :aria-label="t('reader.fontSize')"
        @update:model-value="(v) => preferencesStore.updateReaderPreferences({ font_size: v })"
      />
    </div>

    <div class="space-y-1.5">
      <div class="flex items-center justify-between">
        <span class="text-label-medium text-on-surface-variant">{{ t('reader.lineHeight') }}</span>
        <span class="text-label-medium font-mono tabular-nums text-on-surface">
          {{ preferencesStore.reader.line_height.toFixed(2) }}
        </span>
      </div>
      <AppSlider
        :model-value="preferencesStore.reader.line_height"
        :min="1.3"
        :max="2.4"
        :step="0.05"
        :aria-label="t('reader.lineHeight')"
        @update:model-value="(v) => preferencesStore.updateReaderPreferences({ line_height: v })"
      />
    </div>

    <div class="space-y-1.5">
      <div class="flex items-center justify-between">
        <span class="text-label-medium text-on-surface-variant">{{ t('reader.margin') }}</span>
        <span class="text-label-medium font-mono tabular-nums text-on-surface">
          {{ preferencesStore.reader.margin_px }} px
        </span>
      </div>
      <AppSlider
        :model-value="preferencesStore.reader.margin_px"
        :min="16"
        :max="120"
        :step="8"
        :aria-label="t('reader.margin')"
        @update:model-value="(v) => preferencesStore.updateReaderPreferences({ margin_px: v })"
      />
    </div>
  </div>
</template>
