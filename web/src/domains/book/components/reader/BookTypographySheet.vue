<script setup lang="ts">
import { computed } from 'vue'
import { LuCheck, LuRotateCw } from 'vue-icons-plus/lu'
import FontPickerField from '@/components/FontPickerField.vue'
import {
  AppColorPicker,
  AppSlider,
  MdSegmentedButton,
  type SegmentedOption,
} from '@/components/ui'
import {
  READER_PAPER_PRESETS,
  useReaderAppearance,
  type ReaderPaperPreset,
} from '@/domains/book/composables/useReaderAppearance'
import { useBookI18n as useI18n } from '@/domains/book/i18n'
import { DEFAULT_BOOK_PREFS, usePreferencesStore } from '@/stores/preferences'
import type { ReaderPreferences } from '@/types/api'

const props = withDefaults(
  defineProps<{
    showFlowMode?: boolean
    compact?: boolean
  }>(),
  {
    showFlowMode: false,
    compact: true,
  },
)

const { t } = useI18n()
const preferencesStore = usePreferencesStore()
const appearance = useReaderAppearance()

const flowOptions = computed<SegmentedOption<'paginated' | 'scrolled'>[]>(() => [
  { value: 'paginated', label: t('reader.paginated') },
  { value: 'scrolled', label: t('reader.scrolled') },
])

function updatePref(patch: Partial<ReaderPreferences>, immediate = false): void {
  preferencesStore.updateReaderPreferences(
    patch,
    immediate ? { immediate: true, notifyOnError: true } : undefined,
  )
}

function flushSync(): void {
  void preferencesStore.flushPendingSync()
}

function isPresetActive(preset: ReaderPaperPreset): boolean {
  return (
    (appearance.customBgColor.value ?? null) === preset.bgColor &&
    (appearance.customTextColor.value ?? null) === preset.textColor
  )
}

function applyPaperPreset(preset: ReaderPaperPreset): void {
  updatePref(
    {
      bg_color: preset.bgColor,
      text_color: preset.textColor,
    },
    true,
  )
}

const hasCustomColors = computed(
  () => appearance.customBgColor.value !== null || appearance.customTextColor.value !== null,
)

const isModifiedFromDefaults = computed(() => {
  const r = preferencesStore.reader
  const d = DEFAULT_BOOK_PREFS
  return (
    r.font_family !== d.font_family ||
    r.font_size !== d.font_size ||
    r.line_height !== d.line_height ||
    r.margin_top_px !== d.margin_top_px ||
    r.margin_bottom_px !== d.margin_bottom_px ||
    r.margin_left_px !== d.margin_left_px ||
    r.margin_right_px !== d.margin_right_px ||
    r.paragraph_indent_em !== d.paragraph_indent_em ||
    r.letter_spacing_em !== d.letter_spacing_em ||
    r.paragraph_spacing_px !== d.paragraph_spacing_px ||
    Boolean(r.text_color) ||
    Boolean(r.bg_color) ||
    (props.showFlowMode && r.flow !== d.flow)
  )
})

function resetTypographyDefaults(): void {
  updatePref(
    {
      font_family: DEFAULT_BOOK_PREFS.font_family,
      font_size: DEFAULT_BOOK_PREFS.font_size,
      line_height: DEFAULT_BOOK_PREFS.line_height,
      margin_px: DEFAULT_BOOK_PREFS.margin_px,
      margin_top_px: DEFAULT_BOOK_PREFS.margin_top_px,
      margin_bottom_px: DEFAULT_BOOK_PREFS.margin_bottom_px,
      margin_left_px: DEFAULT_BOOK_PREFS.margin_left_px,
      margin_right_px: DEFAULT_BOOK_PREFS.margin_right_px,
      paragraph_indent_em: DEFAULT_BOOK_PREFS.paragraph_indent_em,
      letter_spacing_em: DEFAULT_BOOK_PREFS.letter_spacing_em,
      paragraph_spacing_px: DEFAULT_BOOK_PREFS.paragraph_spacing_px,
      text_color: null,
      bg_color: null,
      ...(props.showFlowMode ? { flow: DEFAULT_BOOK_PREFS.flow } : {}),
    },
    true,
  )
}

function formatSignedEm(val: number): string {
  const fixed = val.toFixed(2)
  return val > 0 ? `+${fixed} em` : `${fixed} em`
}
</script>

<template>
  <div class="space-y-5">
    <!-- Flow Mode & Font Family -->
    <div :class="compact ? 'space-y-4' : 'grid grid-cols-1 gap-4 sm:grid-cols-2 lg:gap-x-8'">
      <div v-if="showFlowMode" class="space-y-1.5">
        <label class="text-label-medium block text-on-surface-variant">
          {{ t('reader.flowMode') }}
        </label>
        <MdSegmentedButton
          :model-value="preferencesStore.reader.flow"
          :options="flowOptions"
          size="sm"
          full-width
          @update:model-value="(v) => updatePref({ flow: v }, true)"
        />
      </div>

      <div class="space-y-1.5" :class="!compact && showFlowMode ? '' : 'sm:col-span-2'">
        <label class="text-label-medium block text-on-surface-variant">
          {{ t('reader.fontFamily') }}
        </label>
        <FontPickerField
          :model-value="preferencesStore.reader.font_family"
          allow-inherit
          :compact="compact"
          :aria-label="t('reader.fontFamily')"
          @update:model-value="(v) => updatePref({ font_family: v }, true)"
        />
      </div>
    </div>

    <!-- Colors: Paper Presets + Independent Text & Background Color Pickers -->
    <div class="space-y-2.5 rounded-corner-md bg-surface-container p-3.5">
      <div class="flex items-center justify-between gap-2">
        <span class="text-label-medium text-on-surface-variant">
          {{ t('reader.colorSchemeLabel') }}
        </span>
        <button
          v-if="hasCustomColors"
          type="button"
          class="md-state text-label-small inline-flex cursor-pointer items-center gap-1 rounded-corner-full px-2 py-0.5 text-primary"
          @click="updatePref({ text_color: null, bg_color: null }, true)"
        >
          <LuRotateCw class="h-3 w-3" aria-hidden="true" />
          <span>{{ t('reader.resetColors') }}</span>
        </button>
      </div>

      <div class="flex flex-wrap items-center gap-2">
        <button
          v-for="preset in READER_PAPER_PRESETS"
          :key="preset.id"
          type="button"
          :aria-pressed="isPresetActive(preset)"
          class="md-state text-label-small inline-flex h-8 cursor-pointer items-center gap-1.5 rounded-corner-full border px-3 transition-colors"
          :class="
            isPresetActive(preset)
              ? 'border-primary ring-1 ring-primary'
              : 'border-outline-variant hover:border-outline'
          "
          :style="{
            backgroundColor: preset.previewBg,
            color: preset.previewFg,
          }"
          @click="applyPaperPreset(preset)"
        >
          <LuCheck v-if="isPresetActive(preset)" class="h-3.5 w-3.5 shrink-0" aria-hidden="true" />
          <span>{{ t(`reader.presets.${preset.id}`) }}</span>
        </button>
      </div>

      <div class="grid grid-cols-2 gap-3 pt-1">
        <div class="flex items-center justify-between gap-2 rounded-corner-sm bg-surface-container-low px-3 py-2">
          <span class="text-label-medium truncate text-on-surface-variant">
            {{ t('reader.textColor') }}
          </span>
          <div class="flex shrink-0 items-center gap-1.5">
            <span class="text-label-small hidden font-mono uppercase text-on-surface-variant sm:inline">
              {{ appearance.resolvedTextColor.value }}
            </span>
            <AppColorPicker
              :model-value="appearance.resolvedTextColor.value"
              @update:model-value="(hex) => updatePref({ text_color: hex })"
            />
          </div>
        </div>

        <div class="flex items-center justify-between gap-2 rounded-corner-sm bg-surface-container-low px-3 py-2">
          <span class="text-label-medium truncate text-on-surface-variant">
            {{ t('reader.bgColor') }}
          </span>
          <div class="flex shrink-0 items-center gap-1.5">
            <span class="text-label-small hidden font-mono uppercase text-on-surface-variant sm:inline">
              {{ appearance.resolvedBgColor.value }}
            </span>
            <AppColorPicker
              :model-value="appearance.resolvedBgColor.value"
              @update:model-value="(hex) => updatePref({ bg_color: hex })"
            />
          </div>
        </div>
      </div>
    </div>

    <!-- Typography Spacing: Font Size, Indent, Letter Spacing, Line Height, Paragraph Spacing -->
    <div :class="compact ? 'space-y-3.5' : 'grid grid-cols-1 gap-4 sm:grid-cols-2 lg:gap-x-8'">
      <div class="space-y-1">
        <div class="flex items-center justify-between">
          <span class="text-label-medium text-on-surface-variant">{{ t('reader.fontSize') }}</span>
          <span class="text-label-medium font-mono tabular-nums text-on-surface">
            {{ appearance.fontSizePx.value }} px
          </span>
        </div>
        <AppSlider
          :model-value="appearance.fontSizePx.value"
          :min="12"
          :max="36"
          :step="1"
          :aria-label="t('reader.fontSize')"
          @update:model-value="(v) => updatePref({ font_size: v })"
          @commit="flushSync"
        />
      </div>

      <div class="space-y-1">
        <div class="flex items-center justify-between">
          <span class="text-label-medium text-on-surface-variant">{{ t('reader.paragraphIndent') }}</span>
          <span class="text-label-medium font-mono tabular-nums text-on-surface">
            {{ appearance.paragraphIndentEm.value.toFixed(1) }} em
          </span>
        </div>
        <AppSlider
          :model-value="appearance.paragraphIndentEm.value"
          :min="0"
          :max="4"
          :step="0.5"
          :aria-label="t('reader.paragraphIndent')"
          @update:model-value="(v) => updatePref({ paragraph_indent_em: v })"
          @commit="flushSync"
        />
      </div>

      <div class="space-y-1">
        <div class="flex items-center justify-between">
          <span class="text-label-medium text-on-surface-variant">{{ t('reader.letterSpacing') }}</span>
          <span class="text-label-medium font-mono tabular-nums text-on-surface">
            {{ formatSignedEm(appearance.letterSpacingEm.value) }}
          </span>
        </div>
        <AppSlider
          :model-value="appearance.letterSpacingEm.value"
          :min="-0.05"
          :max="0.3"
          :step="0.01"
          :aria-label="t('reader.letterSpacing')"
          @update:model-value="(v) => updatePref({ letter_spacing_em: Number(v.toFixed(2)) })"
          @commit="flushSync"
        />
      </div>

      <div class="space-y-1">
        <div class="flex items-center justify-between">
          <span class="text-label-medium text-on-surface-variant">{{ t('reader.lineHeight') }}</span>
          <span class="text-label-medium font-mono tabular-nums text-on-surface">
            {{ appearance.lineHeight.value.toFixed(2) }}
          </span>
        </div>
        <AppSlider
          :model-value="appearance.lineHeight.value"
          :min="1.2"
          :max="2.6"
          :step="0.05"
          :aria-label="t('reader.lineHeight')"
          @update:model-value="(v) => updatePref({ line_height: Number(v.toFixed(2)) })"
          @commit="flushSync"
        />
      </div>

      <div class="space-y-1">
        <div class="flex items-center justify-between">
          <span class="text-label-medium text-on-surface-variant">{{ t('reader.paragraphSpacing') }}</span>
          <span class="text-label-medium font-mono tabular-nums text-on-surface">
            {{ appearance.paragraphSpacingPx.value }} px
          </span>
        </div>
        <AppSlider
          :model-value="appearance.paragraphSpacingPx.value"
          :min="0"
          :max="48"
          :step="2"
          :aria-label="t('reader.paragraphSpacing')"
          @update:model-value="(v) => updatePref({ paragraph_spacing_px: v })"
          @commit="flushSync"
        />
      </div>
    </div>

    <!-- 4-Side Page Margins: Top, Bottom, Left, Right -->
    <div class="space-y-2.5">
      <span class="text-label-medium block text-on-surface-variant">
        {{ t('reader.marginsHeading') }}
      </span>
      <div class="grid grid-cols-2 gap-x-4 gap-y-3" :class="compact ? '' : 'lg:grid-cols-4'">
        <div class="space-y-1">
          <div class="flex items-center justify-between">
            <span class="text-label-small text-on-surface-variant">{{ t('reader.marginTop') }}</span>
            <span class="text-label-small font-mono tabular-nums text-on-surface">
              {{ appearance.marginTopPx.value }} px
            </span>
          </div>
          <AppSlider
            :model-value="appearance.marginTopPx.value"
            :min="0"
            :max="120"
            :step="4"
            :aria-label="t('reader.marginTop')"
            @update:model-value="(v) => updatePref({ margin_top_px: v })"
            @commit="flushSync"
          />
        </div>

        <div class="space-y-1">
          <div class="flex items-center justify-between">
            <span class="text-label-small text-on-surface-variant">{{ t('reader.marginBottom') }}</span>
            <span class="text-label-small font-mono tabular-nums text-on-surface">
              {{ appearance.marginBottomPx.value }} px
            </span>
          </div>
          <AppSlider
            :model-value="appearance.marginBottomPx.value"
            :min="0"
            :max="120"
            :step="4"
            :aria-label="t('reader.marginBottom')"
            @update:model-value="(v) => updatePref({ margin_bottom_px: v })"
            @commit="flushSync"
          />
        </div>

        <div class="space-y-1">
          <div class="flex items-center justify-between">
            <span class="text-label-small text-on-surface-variant">{{ t('reader.marginLeft') }}</span>
            <span class="text-label-small font-mono tabular-nums text-on-surface">
              {{ appearance.marginLeftPx.value }} px
            </span>
          </div>
          <AppSlider
            :model-value="appearance.marginLeftPx.value"
            :min="0"
            :max="120"
            :step="4"
            :aria-label="t('reader.marginLeft')"
            @update:model-value="(v) => updatePref({ margin_left_px: v, margin_px: v })"
            @commit="flushSync"
          />
        </div>

        <div class="space-y-1">
          <div class="flex items-center justify-between">
            <span class="text-label-small text-on-surface-variant">{{ t('reader.marginRight') }}</span>
            <span class="text-label-small font-mono tabular-nums text-on-surface">
              {{ appearance.marginRightPx.value }} px
            </span>
          </div>
          <AppSlider
            :model-value="appearance.marginRightPx.value"
            :min="0"
            :max="120"
            :step="4"
            :aria-label="t('reader.marginRight')"
            @update:model-value="(v) => updatePref({ margin_right_px: v })"
            @commit="flushSync"
          />
        </div>
      </div>
    </div>

    <div v-if="isModifiedFromDefaults" class="flex justify-end pt-1">
      <button
        type="button"
        class="md-btn md-btn-text md-state"
        @click="resetTypographyDefaults"
      >
        <LuRotateCw class="h-3.5 w-3.5" aria-hidden="true" />
        <span>{{ t('reader.resetTypography') }}</span>
      </button>
    </div>
  </div>
</template>
