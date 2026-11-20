<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  LuCheck,
  LuMonitor,
  LuMoon,
  LuRotateCw,
  LuSlidersHorizontal,
  LuSun,
} from 'vue-icons-plus/lu'
import AppColorPicker from '@/components/ui/AppColorPicker.vue'
import MdSegmentedButton, { type SegmentedOption } from '@/components/ui/MdSegmentedButton.vue'
import { useThemeStore } from '@/stores/theme'
import {
  CATPPUCCIN_ACCENT_HEX,
  CATPPUCCIN_ACCENTS,
  DARK_PALETTE_PRESETS,
  DEFAULT_ACCENT_MODE,
  LIGHT_PALETTE_PRESETS,
  chooseContrastText,
  normalizeHex,
  type AccentMode,
  type CatppuccinAccent,
  type CustomPaletteTokens,
  type PalettePreset,
  type ThemeMode,
} from '@/theme/catppuccin'
import type { LocaleCode } from '@/types/api'

const { t } = useI18n()
const themeStore = useThemeStore()

const localeOptions: { code: LocaleCode; nativeLabel: string; subLabel: string }[] = [
  { code: 'zh-CN', nativeLabel: '简体中文', subLabel: 'Simplified Chinese' },
  { code: 'en-US', nativeLabel: 'English', subLabel: 'English (US)' },
]

const themeModeOptions = computed<SegmentedOption<ThemeMode>[]>(() => [
  { value: 'system', label: t('theme.system'), icon: LuMonitor },
  { value: 'light', label: t('theme.light'), icon: LuSun },
  { value: 'dark', label: t('theme.dark'), icon: LuMoon },
])

const accentModeOptions = computed<SegmentedOption<AccentMode>[]>(() => [
  { value: 'preset', label: t('theme.accentModePreset') },
  { value: 'catppuccin', label: t('theme.accentModeCatppuccin') },
  { value: 'custom', label: t('theme.accentModeCustom') },
])

const customTokenFields: { key: keyof CustomPaletteTokens; labelKey: string }[] = [
  { key: 'canvas', labelKey: 'theme.tokenCanvas' },
  { key: 'shell', labelKey: 'theme.tokenShell' },
  { key: 'surface', labelKey: 'theme.tokenSurface' },
  { key: 'text', labelKey: 'theme.tokenText' },
  { key: 'subtext', labelKey: 'theme.tokenSubtext' },
]

interface PalettePoolConfig {
  kind: 'light' | 'dark'
  titleKey: string
  presets: readonly PalettePreset[]
  selectedId: string
  customTokens: CustomPaletteTokens
  swatchRingClass: string
}

const palettePools = computed<PalettePoolConfig[]>(() => [
  {
    kind: 'light',
    titleKey: 'theme.lightPaletteTitle',
    presets: LIGHT_PALETTE_PRESETS,
    selectedId: themeStore.lightPreset,
    customTokens: themeStore.customLight,
    swatchRingClass: 'border-black/15',
  },
  {
    kind: 'dark',
    titleKey: 'theme.darkPaletteTitle',
    presets: DARK_PALETTE_PRESETS,
    selectedId: themeStore.darkPreset,
    customTokens: themeStore.customDark,
    swatchRingClass: 'border-white/20',
  },
])

function selectPoolPreset(kind: 'light' | 'dark', presetId: string): void {
  if (kind === 'light') {
    themeStore.setLightPreset(presetId)
  } else {
    themeStore.setDarkPreset(presetId)
  }
}

function selectPoolCustom(pool: PalettePoolConfig): void {
  if (pool.selectedId === 'custom') return
  themeStore.seedCustomFromActivePreset(pool.kind)
}

function accentHex(acc: CatppuccinAccent): string {
  return CATPPUCCIN_ACCENT_HEX[themeStore.isDark ? 'dark' : 'light'][acc]
}

function onCustomTokenInput(
  kind: 'light' | 'dark',
  field: keyof CustomPaletteTokens,
  raw: string,
  fallback: string,
): void {
  const normalized = normalizeHex(raw, fallback)
  themeStore.updateCustomPalette(kind, { [field]: normalized })
}

function handleResetDefaults(): void {
  themeStore.resetPalettesToDefault()
  themeStore.setAccentMode(DEFAULT_ACCENT_MODE)
}
</script>

<template>
  <!-- Language -->
  <section class="md-card-outlined p-6">
    <h2 class="md-title-small text-[var(--md-sys-color-on-surface)]">
      {{ t('theme.languageTitle') }}
    </h2>

    <div class="mt-4 grid grid-cols-1 gap-3 sm:grid-cols-2">
      <button
        v-for="loc in localeOptions"
        :key="loc.code"
        type="button"
        class="md-state flex cursor-pointer items-center justify-between rounded-[var(--md-sys-shape-corner-md)] border px-4 py-3 text-left transition"
        :class="
          themeStore.locale === loc.code
            ? 'border-[var(--md-sys-color-primary)] bg-[var(--md-sys-color-secondary-container)] text-[var(--md-sys-color-on-secondary-container)]'
            : 'border-[var(--md-sys-color-outline-variant)] bg-[var(--md-sys-color-surface-container-low)] text-[var(--md-sys-color-on-surface)] hover:border-[var(--md-sys-color-outline)]'
        "
        @click="themeStore.setLocale(loc.code)"
      >
        <div>
          <p class="md-title-small">{{ loc.nativeLabel }}</p>
          <p
            class="md-body-small mt-0.5"
            :class="
              themeStore.locale === loc.code
                ? 'opacity-80'
                : 'text-[var(--md-sys-color-on-surface-variant)]'
            "
          >
            {{ loc.subLabel }}
          </p>
        </div>
        <LuCheck v-if="themeStore.locale === loc.code" class="h-4 w-4 shrink-0" />
      </button>
    </div>
  </section>

  <!-- Theme Mode -->
  <section class="md-card-outlined p-6">
    <div class="flex flex-wrap items-center justify-between gap-2">
      <h2 class="md-title-small text-[var(--md-sys-color-on-surface)]">
        {{ t('theme.modeTitle') }}
      </h2>
      <button
        type="button"
        class="md-btn md-btn-sm md-btn-outlined"
        @click="handleResetDefaults"
      >
        <LuRotateCw class="h-3.5 w-3.5" />
        <span>{{ t('theme.resetDefaults') }}</span>
      </button>
    </div>

    <div class="mt-4">
      <MdSegmentedButton
        :model-value="themeStore.themeMode"
        :options="themeModeOptions"
        @update:model-value="themeStore.setThemeMode($event)"
      />
    </div>
  </section>

  <!-- Light & Dark Palette Pools (Unified Template) -->
  <section v-for="pool in palettePools" :key="pool.kind" class="md-card-outlined p-6">
    <h2 class="md-title-small text-[var(--md-sys-color-on-surface)]">
      {{ t(pool.titleKey) }}
    </h2>

    <div class="mt-4 grid grid-cols-1 gap-3 sm:grid-cols-3">
      <button
        v-for="preset in pool.presets"
        :key="preset.id"
        type="button"
        class="md-state flex cursor-pointer items-center justify-between rounded-[var(--md-sys-shape-corner-md)] border px-3.5 py-3 text-left text-xs font-medium transition"
        :class="
          pool.selectedId === preset.id
            ? 'border-[var(--md-sys-color-primary)] bg-[var(--md-sys-color-secondary-container)] font-semibold text-[var(--md-sys-color-on-secondary-container)]'
            : 'border-[var(--md-sys-color-outline-variant)] bg-[var(--md-sys-color-surface-container-low)] text-[var(--md-sys-color-on-surface)] hover:border-[var(--md-sys-color-outline)]'
        "
        @click="selectPoolPreset(pool.kind, preset.id)"
      >
        <span class="flex min-w-0 items-center gap-2">
          <span class="flex -space-x-1">
            <span
              class="h-3.5 w-3.5 rounded-full border"
              :class="pool.swatchRingClass"
              :style="{ backgroundColor: preset.tokens.canvas }"
            />
            <span
              class="h-3.5 w-3.5 rounded-full border"
              :class="pool.swatchRingClass"
              :style="{ backgroundColor: preset.tokens.shell }"
            />
            <span
              class="h-3.5 w-3.5 rounded-full border"
              :class="pool.swatchRingClass"
              :style="{ backgroundColor: preset.defaultAccent }"
            />
          </span>
          <span class="truncate">{{ t(`theme.presets.${preset.id}`) }}</span>
        </span>
        <LuCheck v-if="pool.selectedId === preset.id" class="h-4 w-4 shrink-0" />
      </button>

      <button
        type="button"
        class="md-state flex cursor-pointer items-center justify-between rounded-[var(--md-sys-shape-corner-md)] border px-3.5 py-3 text-left text-xs font-medium transition"
        :class="
          pool.selectedId === 'custom'
            ? 'border-[var(--md-sys-color-primary)] bg-[var(--md-sys-color-secondary-container)] font-semibold text-[var(--md-sys-color-on-secondary-container)]'
            : 'border-[var(--md-sys-color-outline-variant)] bg-[var(--md-sys-color-surface-container-low)] text-[var(--md-sys-color-on-surface)] hover:border-[var(--md-sys-color-outline)]'
        "
        @click="selectPoolCustom(pool)"
      >
        <span class="flex items-center gap-2">
          <LuSlidersHorizontal class="h-3.5 w-3.5 shrink-0" />
          <span>{{ t('theme.customPalette') }}</span>
        </span>
        <LuCheck v-if="pool.selectedId === 'custom'" class="h-4 w-4 shrink-0" />
      </button>
    </div>

    <div
      v-if="pool.selectedId === 'custom'"
      class="md-card-filled mt-4 grid grid-cols-2 gap-3 p-4 sm:grid-cols-5"
    >
      <div v-for="f in customTokenFields" :key="`${pool.kind}-${f.key}`">
        <label
          class="md-label-small mb-1.5 block truncate text-[var(--md-sys-color-on-surface-variant)]"
        >
          {{ t(f.labelKey) }}
        </label>
        <div class="flex items-center gap-1.5">
          <AppColorPicker
            :model-value="pool.customTokens[f.key]"
            @update:model-value="themeStore.updateCustomPalette(pool.kind, { [f.key]: $event })"
          />
          <input
            type="text"
            :value="pool.customTokens[f.key]"
            maxlength="7"
            class="md-input h-8 w-full !px-2 font-mono text-xs"
            @change="
              onCustomTokenInput(
                pool.kind,
                f.key,
                ($event.target as HTMLInputElement).value,
                pool.customTokens[f.key],
              )
            "
          />
        </div>
      </div>
    </div>
  </section>

  <!-- 3-Mode Accent Color -->
  <section class="md-card-outlined p-6">
    <h2 class="md-title-small text-[var(--md-sys-color-on-surface)]">
      {{ t('theme.accentTitle') }}
    </h2>

    <div class="mt-4">
      <MdSegmentedButton
        :model-value="themeStore.accentMode"
        :options="accentModeOptions"
        @update:model-value="themeStore.setAccentMode($event)"
      />
    </div>

    <div
      v-if="themeStore.accentMode === 'catppuccin'"
      class="mt-4 flex flex-wrap gap-2.5"
    >
      <button
        v-for="acc in CATPPUCCIN_ACCENTS"
        :key="acc"
        type="button"
        class="flex h-8 w-8 cursor-pointer items-center justify-center rounded-full transition hover:scale-110 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--md-sys-color-primary)]"
        :style="{ backgroundColor: accentHex(acc) }"
        :title="t(`theme.accents.${acc}`)"
        @click="themeStore.setCatppuccinAccent(acc)"
      >
        <LuCheck
          v-if="themeStore.accent === acc"
          class="h-4 w-4 stroke-[2.5]"
          :style="{ color: chooseContrastText(accentHex(acc)) }"
        />
      </button>
    </div>

    <div
      v-else-if="themeStore.accentMode === 'custom'"
      class="mt-4 flex items-center gap-2.5"
    >
      <AppColorPicker
        :model-value="themeStore.customAccent"
        @update:model-value="themeStore.setCustomAccentHex($event)"
      />
      <input
        type="text"
        :value="themeStore.customAccent"
        maxlength="7"
        class="md-input h-9 w-32 font-mono text-xs"
        @change="
          themeStore.setCustomAccentHex(
            normalizeHex(($event.target as HTMLInputElement).value, themeStore.customAccent),
          )
        "
      />
    </div>
  </section>
</template>
