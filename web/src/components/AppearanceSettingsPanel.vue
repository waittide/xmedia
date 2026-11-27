<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { LuCheck, LuMonitor, LuMoon, LuRotateCw, LuSun } from 'vue-icons-plus/lu'
import FontPickerField from '@/components/FontPickerField.vue'
import AppColorPicker from '@/components/ui/AppColorPicker.vue'
import MdSegmentedButton, { type SegmentedOption } from '@/components/ui/MdSegmentedButton.vue'
import MdSwitch from '@/components/ui/MdSwitch.vue'
import { useThemeStore } from '@/stores/theme'
import { APP_ICON_PRESETS } from '@/theme/appIcons'
import {
  chooseContrastText,
  normalizeHex,
  SEED_PRESETS,
  type ThemeMode,
} from '@/theme/md3'
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

function isSeedSelected(hex: string): boolean {
  return themeStore.effectiveSeedColor === hex
}

function onHexInputChange(raw: string): void {
  const normalized = normalizeHex(raw, themeStore.effectiveSeedColor)
  themeStore.setSeedColor(normalized)
}
</script>

<template>
  <!-- Language -->
  <section class="rounded-corner-lg bg-surface-container-low p-6">
    <h2 class="text-title-small text-on-surface">
      {{ t('theme.languageTitle') }}
    </h2>

    <div class="mt-4 grid grid-cols-1 gap-3 sm:grid-cols-2">
      <button
        v-for="loc in localeOptions"
        :key="loc.code"
        type="button"
        :aria-pressed="themeStore.locale === loc.code"
        class="md-state flex cursor-pointer items-center justify-between rounded-corner-md px-4 py-3 text-left transition-colors duration-150 ease-standard"
        :class="
          themeStore.locale === loc.code
            ? 'bg-secondary-container text-on-secondary-container'
            : 'bg-surface-container text-on-surface hover:bg-surface-container-high'
        "
        @click="themeStore.setLocale(loc.code)"
      >
        <div>
          <p class="text-title-small">{{ loc.nativeLabel }}</p>
          <p
            class="text-body-small mt-0.5"
            :class="
              themeStore.locale === loc.code ? 'opacity-80' : 'text-on-surface-variant'
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
  <section class="rounded-corner-lg bg-surface-container-low p-6">
    <h2 class="text-title-small text-on-surface">
      {{ t('theme.modeTitle') }}
    </h2>

    <div class="mt-4">
      <div class="no-scrollbar -m-1 overflow-x-auto p-1">
        <MdSegmentedButton
          :model-value="themeStore.themeMode"
          :options="themeModeOptions"
          @update:model-value="themeStore.setThemeMode($event)"
        />
      </div>
    </div>
  </section>

  <!-- Sidebar & Navigation Bar Labels -->
  <section
    data-appearance-nav-labels
    class="rounded-corner-lg bg-surface-container-low p-6"
  >
    <div>
      <h2 class="text-title-small text-on-surface">
        {{ t('theme.navDisplayTitle') }}
      </h2>
      <p class="text-body-small mt-0.5 text-on-surface-variant">
        {{ t('theme.navDisplayHint') }}
      </p>
    </div>

    <div class="mt-4 grid grid-cols-1 gap-3 sm:grid-cols-2">
      <div class="flex items-center justify-between gap-3 rounded-corner-md bg-surface-container px-4 py-3">
        <MdSwitch
          :model-value="themeStore.showSidebarLabels"
          :label="t('theme.showSidebarLabels')"
          @update:model-value="themeStore.setShowSidebarLabels($event)"
        />
      </div>

      <div class="flex items-center justify-between gap-3 rounded-corner-md bg-surface-container px-4 py-3">
        <MdSwitch
          :model-value="themeStore.showNavLabels"
          :label="t('theme.showNavLabels')"
          @update:model-value="themeStore.setShowNavLabels($event)"
        />
      </div>
    </div>
  </section>

  <!-- UI Font -->
  <section
    data-appearance-ui-font
    class="rounded-corner-lg bg-surface-container-low p-6"
  >
    <div class="flex flex-wrap items-center justify-between gap-2">
      <div>
        <h2 class="text-title-small text-on-surface">
          {{ t('theme.uiFontTitle') }}
        </h2>
        <p class="text-body-small mt-0.5 text-on-surface-variant">
          {{ t('theme.uiFontHint') }}
        </p>
      </div>
      <button
        v-if="themeStore.uiFont !== null"
        type="button"
        data-reset-ui-font
        class="md-btn md-btn-text md-state"
        @click="themeStore.resetUiFont()"
      >
        <LuRotateCw class="h-3.5 w-3.5" />
        <span>{{ t('theme.resetDefaults') }}</span>
      </button>
    </div>

    <div class="mt-4">
      <FontPickerField
        :model-value="themeStore.uiFont ?? 'system-ui'"
        :aria-label="t('theme.uiFontTitle')"
        @update:model-value="themeStore.setUiFont($event === 'system-ui' ? null : $event)"
      />
    </div>
  </section>

  <!-- Dynamic Seed Color -->
  <section class="rounded-corner-lg bg-surface-container-low p-6">
    <div class="flex flex-wrap items-center justify-between gap-2">
      <h2 class="text-title-small text-on-surface">
        {{ t('theme.seedColorTitle') }}
      </h2>
      <button
        v-if="themeStore.seedColor !== null"
        type="button"
        class="md-btn md-btn-text md-state"
        @click="themeStore.resetSeedColor()"
      >
        <LuRotateCw class="h-3.5 w-3.5" />
        <span>{{ t('theme.resetDefaults') }}</span>
      </button>
    </div>

    <div class="mt-4 flex flex-wrap items-center gap-3">
      <button
        v-for="preset in SEED_PRESETS"
        :key="preset.id"
        type="button"
        :aria-label="t(`theme.seeds.${preset.id}`)"
        :aria-pressed="isSeedSelected(preset.hex)"
        :title="t(`theme.seeds.${preset.id}`)"
        class="md-touch-target flex h-10 w-10 cursor-pointer items-center justify-center rounded-corner-full transition-transform duration-150 ease-standard hover:scale-105 focus-visible:outline-3 focus-visible:outline-offset-2 focus-visible:outline-secondary"
        :class="
          isSeedSelected(preset.hex)
            ? 'ring-2 ring-primary ring-offset-2 ring-offset-surface-container-low'
            : ''
        "
        :style="{ backgroundColor: preset.hex }"
        @click="themeStore.setSeedColor(preset.hex)"
      >
        <LuCheck
          v-if="isSeedSelected(preset.hex)"
          class="h-4 w-4 stroke-[2.5]"
          :style="{ color: chooseContrastText(preset.hex) }"
        />
      </button>
    </div>

    <div class="mt-5 flex flex-wrap items-center gap-3 rounded-corner-md bg-surface-container px-4 py-3">
      <span class="text-label-medium text-on-surface-variant">
        {{ t('theme.customSeedLabel') }}
      </span>
      <AppColorPicker
        :model-value="themeStore.effectiveSeedColor"
        @update:model-value="themeStore.setSeedColor($event)"
      />
      <input
        type="text"
        :value="themeStore.effectiveSeedColor"
        :aria-label="t('theme.hexInputLabel')"
        maxlength="7"
        class="md-input sm:text-body-small h-9 w-28 font-mono"
        @change="onHexInputChange(($event.target as HTMLInputElement).value)"
      />
    </div>
  </section>

  <!-- App Icon -->
  <section class="rounded-corner-lg bg-surface-container-low p-6">
    <div class="flex flex-wrap items-center justify-between gap-2">
      <div>
        <h2 class="text-title-small text-on-surface">
          {{ t('theme.appIconTitle') }}
        </h2>
        <p class="text-body-small mt-0.5 text-on-surface-variant">
          {{ t('theme.appIconHint') }}
        </p>
      </div>
      <button
        v-if="themeStore.appIcon !== null"
        type="button"
        class="md-btn md-btn-text md-state"
        @click="themeStore.resetAppIcon()"
      >
        <LuRotateCw class="h-3.5 w-3.5" />
        <span>{{ t('theme.resetDefaults') }}</span>
      </button>
    </div>

    <div class="mt-4 grid grid-cols-2 gap-3 sm:grid-cols-4">
      <button
        v-for="preset in APP_ICON_PRESETS"
        :key="preset.id"
        type="button"
        :aria-pressed="themeStore.effectiveAppIcon === preset.id"
        class="md-state relative flex cursor-pointer flex-col items-center gap-2.5 rounded-corner-md px-3 py-4 text-center transition-colors duration-150 ease-standard"
        :class="
          themeStore.effectiveAppIcon === preset.id
            ? 'bg-secondary-container text-on-secondary-container'
            : 'bg-surface-container text-on-surface hover:bg-surface-container-high'
        "
        @click="themeStore.setAppIcon(preset.id)"
      >
        <span
          v-if="themeStore.effectiveAppIcon === preset.id"
          class="absolute top-2 right-2 flex h-5 w-5 items-center justify-center rounded-corner-full bg-primary text-on-primary"
        >
          <LuCheck class="h-3 w-3 stroke-[2.5]" />
        </span>
        <img
          :src="preset.src"
          alt=""
          aria-hidden="true"
          class="h-14 w-14 shrink-0 select-none"
        />
        <span class="text-label-medium">{{ t(`theme.icons.${preset.id}`) }}</span>
      </button>
    </div>
  </section>
</template>
