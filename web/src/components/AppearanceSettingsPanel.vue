<script setup lang="ts">
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
import { useThemeStore } from '@/stores/theme'
import {
  CATPPUCCIN_ACCENT_HEX,
  CATPPUCCIN_ACCENTS,
  DARK_PALETTE_PRESETS,
  LIGHT_PALETTE_PRESETS,
  normalizeHex,
  type CatppuccinAccent,
  type CustomPaletteTokens,
} from '@/theme/catppuccin'
import type { LocaleCode } from '@/types/api'

const { t } = useI18n()
const themeStore = useThemeStore()

const localeOptions: { code: LocaleCode; nativeLabel: string; subLabel: string }[] = [
  { code: 'zh-CN', nativeLabel: '简体中文', subLabel: 'Simplified Chinese' },
  { code: 'en-US', nativeLabel: 'English', subLabel: 'English (US)' },
]

const customTokenFields: { key: keyof CustomPaletteTokens; labelKey: string }[] = [
  { key: 'canvas', labelKey: 'theme.tokenCanvas' },
  { key: 'shell', labelKey: 'theme.tokenShell' },
  { key: 'surface', labelKey: 'theme.tokenSurface' },
  { key: 'text', labelKey: 'theme.tokenText' },
  { key: 'subtext', labelKey: 'theme.tokenSubtext' },
]

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
</script>

<template>
  <!-- Language -->
  <section class="card-soft rounded-2xl bg-[var(--ctp-shell)] p-6">
    <h2 class="text-sm font-bold">{{ t('theme.languageTitle') }}</h2>
    <p class="mt-1 text-xs text-[var(--ctp-subtext0)]">{{ t('theme.languageDesc') }}</p>

    <div class="mt-4 grid grid-cols-1 gap-3 sm:grid-cols-2">
      <button
        v-for="loc in localeOptions"
        :key="loc.code"
        type="button"
        class="flex items-center justify-between rounded-xl border px-4 py-3 text-left transition"
        :class="
          themeStore.locale === loc.code
            ? 'border-[var(--ctp-accent)] bg-[var(--ctp-accent-soft)]'
            : 'border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] hover:border-[var(--ctp-surface1)]'
        "
        @click="themeStore.setLocale(loc.code)"
      >
        <div>
          <p class="text-sm font-semibold text-[var(--ctp-text)]">{{ loc.nativeLabel }}</p>
          <p class="text-xs text-[var(--ctp-subtext0)]">{{ loc.subLabel }}</p>
        </div>
        <LuCheck
          v-if="themeStore.locale === loc.code"
          class="h-4 w-4 text-[var(--ctp-accent)]"
        />
      </button>
    </div>
  </section>

  <!-- Theme Mode -->
  <section class="card-soft rounded-2xl bg-[var(--ctp-shell)] p-6">
    <div class="flex flex-wrap items-start justify-between gap-2">
      <div>
        <h2 class="text-sm font-bold">{{ t('theme.modeTitle') }}</h2>
        <p class="mt-1 text-xs text-[var(--ctp-subtext0)]">{{ t('theme.modeDesc') }}</p>
      </div>
      <button
        type="button"
        class="inline-flex items-center gap-1.5 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 py-1.5 text-xs font-medium text-[var(--ctp-subtext1)] transition hover:text-[var(--ctp-text)]"
        @click="themeStore.resetPalettesToDefault()"
      >
        <LuRotateCw class="h-3.5 w-3.5" />
        <span>{{ t('theme.resetDefaults') }}</span>
      </button>
    </div>

    <div class="mt-4 grid grid-cols-3 gap-3">
      <button
        type="button"
        class="flex items-center justify-center gap-2 rounded-xl border py-2.5 text-xs font-semibold transition"
        :class="
          themeStore.themeMode === 'system'
            ? 'border-[var(--ctp-accent)] bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)]'
            : 'border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] text-[var(--ctp-text)]'
        "
        @click="themeStore.setThemeMode('system')"
      >
        <LuMonitor class="h-4 w-4" />
        <span>{{ t('theme.system') }}</span>
      </button>

      <button
        type="button"
        class="flex items-center justify-center gap-2 rounded-xl border py-2.5 text-xs font-semibold transition"
        :class="
          themeStore.themeMode === 'light'
            ? 'border-[var(--ctp-accent)] bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)]'
            : 'border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] text-[var(--ctp-text)]'
        "
        @click="themeStore.setThemeMode('light')"
      >
        <LuSun class="h-4 w-4" />
        <span>{{ t('theme.light') }}</span>
      </button>

      <button
        type="button"
        class="flex items-center justify-center gap-2 rounded-xl border py-2.5 text-xs font-semibold transition"
        :class="
          themeStore.themeMode === 'dark'
            ? 'border-[var(--ctp-accent)] bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)]'
            : 'border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] text-[var(--ctp-text)]'
        "
        @click="themeStore.setThemeMode('dark')"
      >
        <LuMoon class="h-4 w-4" />
        <span>{{ t('theme.dark') }}</span>
      </button>
    </div>
  </section>

  <!-- Light Palette Pool -->
  <section class="card-soft rounded-2xl bg-[var(--ctp-shell)] p-6">
    <h2 class="text-sm font-bold">{{ t('theme.lightPaletteTitle') }}</h2>
    <p class="mt-1 text-xs text-[var(--ctp-subtext0)]">{{ t('theme.lightPaletteDesc') }}</p>

    <div class="mt-4 grid grid-cols-1 gap-3 sm:grid-cols-3">
      <button
        v-for="preset in LIGHT_PALETTE_PRESETS"
        :key="preset.id"
        type="button"
        class="flex items-center justify-between rounded-xl border px-3.5 py-3 text-left text-xs font-semibold transition"
        :class="
          themeStore.lightPreset === preset.id
            ? 'border-[var(--ctp-accent)] bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]'
            : 'border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] text-[var(--ctp-text)] hover:border-[var(--ctp-surface1)]'
        "
        @click="themeStore.setLightPreset(preset.id)"
      >
        <div class="flex min-w-0 items-center gap-2">
          <span class="flex -space-x-1">
            <span
              class="h-3.5 w-3.5 rounded-full border border-black/15"
              :style="{ backgroundColor: preset.tokens.canvas }"
            />
            <span
              class="h-3.5 w-3.5 rounded-full border border-black/15"
              :style="{ backgroundColor: preset.tokens.shell }"
            />
            <span
              class="h-3.5 w-3.5 rounded-full border border-black/15"
              :style="{ backgroundColor: preset.defaultAccent }"
            />
          </span>
          <span class="truncate">{{ t(`theme.presets.${preset.id}`) }}</span>
        </div>
        <LuCheck v-if="themeStore.lightPreset === preset.id" class="h-4 w-4 shrink-0" />
      </button>

      <button
        type="button"
        class="flex items-center justify-between rounded-xl border px-3.5 py-3 text-left text-xs font-semibold transition"
        :class="
          themeStore.lightPreset === 'custom'
            ? 'border-[var(--ctp-accent)] bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]'
            : 'border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] text-[var(--ctp-text)] hover:border-[var(--ctp-surface1)]'
        "
        @click="
          themeStore.lightPreset === 'custom'
            ? undefined
            : themeStore.seedCustomFromActivePreset('light')
        "
      >
        <span class="flex items-center gap-2">
          <LuSlidersHorizontal class="h-3.5 w-3.5 shrink-0" />
          <span>{{ t('theme.customPalette') }}</span>
        </span>
        <LuCheck v-if="themeStore.lightPreset === 'custom'" class="h-4 w-4 shrink-0" />
      </button>
    </div>

    <div
      v-if="themeStore.lightPreset === 'custom'"
      class="mt-4 grid grid-cols-2 gap-3 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] p-4 sm:grid-cols-5"
    >
      <div v-for="f in customTokenFields" :key="`light-${f.key}`">
        <label class="mb-1.5 block truncate text-[11px] font-medium text-[var(--ctp-subtext0)]">
          {{ t(f.labelKey) }}
        </label>
        <div class="flex items-center gap-1.5">
          <AppColorPicker
            :model-value="themeStore.customLight[f.key]"
            @update:model-value="themeStore.updateCustomPalette('light', { [f.key]: $event })"
          />
          <input
            type="text"
            :value="themeStore.customLight[f.key]"
            maxlength="7"
            class="h-8 w-full rounded-lg border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)] px-2 font-mono text-xs outline-none focus:border-[var(--ctp-accent)]"
            @change="
              onCustomTokenInput(
                'light',
                f.key,
                ($event.target as HTMLInputElement).value,
                themeStore.customLight[f.key],
              )
            "
          />
        </div>
      </div>
    </div>
  </section>

  <!-- Dark Palette Pool -->
  <section class="card-soft rounded-2xl bg-[var(--ctp-shell)] p-6">
    <h2 class="text-sm font-bold">{{ t('theme.darkPaletteTitle') }}</h2>
    <p class="mt-1 text-xs text-[var(--ctp-subtext0)]">{{ t('theme.darkPaletteDesc') }}</p>

    <div class="mt-4 grid grid-cols-1 gap-3 sm:grid-cols-3">
      <button
        v-for="preset in DARK_PALETTE_PRESETS"
        :key="preset.id"
        type="button"
        class="flex items-center justify-between rounded-xl border px-3.5 py-3 text-left text-xs font-semibold transition"
        :class="
          themeStore.darkPreset === preset.id
            ? 'border-[var(--ctp-accent)] bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]'
            : 'border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] text-[var(--ctp-text)] hover:border-[var(--ctp-surface1)]'
        "
        @click="themeStore.setDarkPreset(preset.id)"
      >
        <span class="flex min-w-0 items-center gap-2">
          <span class="flex -space-x-1">
            <span
              class="h-3.5 w-3.5 rounded-full border border-white/20"
              :style="{ backgroundColor: preset.tokens.canvas }"
            />
            <span
              class="h-3.5 w-3.5 rounded-full border border-white/20"
              :style="{ backgroundColor: preset.tokens.shell }"
            />
            <span
              class="h-3.5 w-3.5 rounded-full border border-white/20"
              :style="{ backgroundColor: preset.defaultAccent }"
            />
          </span>
          <span class="truncate">{{ t(`theme.presets.${preset.id}`) }}</span>
        </span>
        <LuCheck v-if="themeStore.darkPreset === preset.id" class="h-4 w-4 shrink-0" />
      </button>

      <button
        type="button"
        class="flex items-center justify-between rounded-xl border px-3.5 py-3 text-left text-xs font-semibold transition"
        :class="
          themeStore.darkPreset === 'custom'
            ? 'border-[var(--ctp-accent)] bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]'
            : 'border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] text-[var(--ctp-text)] hover:border-[var(--ctp-surface1)]'
        "
        @click="
          themeStore.darkPreset === 'custom'
            ? undefined
            : themeStore.seedCustomFromActivePreset('dark')
        "
      >
        <span class="flex items-center gap-2">
          <LuSlidersHorizontal class="h-3.5 w-3.5 shrink-0" />
          <span>{{ t('theme.customPalette') }}</span>
        </span>
        <LuCheck v-if="themeStore.darkPreset === 'custom'" class="h-4 w-4 shrink-0" />
      </button>
    </div>

    <div
      v-if="themeStore.darkPreset === 'custom'"
      class="mt-4 grid grid-cols-2 gap-3 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] p-4 sm:grid-cols-5"
    >
      <div v-for="f in customTokenFields" :key="`dark-${f.key}`">
        <label class="mb-1.5 block truncate text-[11px] font-medium text-[var(--ctp-subtext0)]">
          {{ t(f.labelKey) }}
        </label>
        <div class="flex items-center gap-1.5">
          <AppColorPicker
            :model-value="themeStore.customDark[f.key]"
            @update:model-value="themeStore.updateCustomPalette('dark', { [f.key]: $event })"
          />
          <input
            type="text"
            :value="themeStore.customDark[f.key]"
            maxlength="7"
            class="h-8 w-full rounded-lg border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)] px-2 font-mono text-xs outline-none focus:border-[var(--ctp-accent)]"
            @change="
              onCustomTokenInput(
                'dark',
                f.key,
                ($event.target as HTMLInputElement).value,
                themeStore.customDark[f.key],
              )
            "
          />
        </div>
      </div>
    </div>
  </section>

  <!-- 3-Mode Accent Color -->
  <section class="card-soft rounded-2xl bg-[var(--ctp-shell)] p-6">
    <h2 class="text-sm font-bold">{{ t('theme.accentTitle') }}</h2>
    <p class="mt-1 text-xs text-[var(--ctp-subtext0)]">{{ t('theme.accentDesc') }}</p>

    <div class="mt-4 grid grid-cols-1 gap-2.5 sm:grid-cols-3">
      <button
        type="button"
        class="rounded-xl border px-3.5 py-2.5 text-xs font-semibold transition"
        :class="
          themeStore.accentMode === 'preset'
            ? 'border-[var(--ctp-accent)] bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]'
            : 'border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] text-[var(--ctp-text)]'
        "
        @click="themeStore.setAccentMode('preset')"
      >
        {{ t('theme.accentModePreset') }}
      </button>

      <button
        type="button"
        class="rounded-xl border px-3.5 py-2.5 text-xs font-semibold transition"
        :class="
          themeStore.accentMode === 'catppuccin'
            ? 'border-[var(--ctp-accent)] bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]'
            : 'border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] text-[var(--ctp-text)]'
        "
        @click="themeStore.setAccentMode('catppuccin')"
      >
        {{ t('theme.accentModeCatppuccin') }}
      </button>

      <button
        type="button"
        class="rounded-xl border px-3.5 py-2.5 text-xs font-semibold transition"
        :class="
          themeStore.accentMode === 'custom'
            ? 'border-[var(--ctp-accent)] bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]'
            : 'border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] text-[var(--ctp-text)]'
        "
        @click="themeStore.setAccentMode('custom')"
      >
        {{ t('theme.accentModeCustom') }}
      </button>
    </div>

    <div
      v-if="themeStore.accentMode === 'catppuccin'"
      class="mt-4 flex flex-wrap gap-2.5"
    >
      <button
        v-for="acc in CATPPUCCIN_ACCENTS"
        :key="acc"
        type="button"
        class="flex h-8 w-8 items-center justify-center rounded-full transition hover:scale-110"
        :style="{ backgroundColor: accentHex(acc) }"
        :title="t(`theme.accents.${acc}`)"
        @click="themeStore.setCatppuccinAccent(acc)"
      >
        <LuCheck
          v-if="themeStore.accent === acc"
          class="h-4 w-4"
          :style="{ color: themeStore.isDark ? '#11111b' : '#ffffff' }"
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
        class="h-9 w-32 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-3 font-mono text-xs outline-none focus:border-[var(--ctp-accent)]"
        @change="
          themeStore.setCustomAccentHex(
            normalizeHex(($event.target as HTMLInputElement).value, themeStore.customAccent),
          )
        "
      />
    </div>
  </section>
</template>
