<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { LuPalette, LuSun, LuMoon, LuMonitor, LuCheck, LuGlobe } from 'vue-icons-plus/lu'
import {
  CATPPUCCIN_ACCENTS,
  CATPPUCCIN_ACCENT_HEX,
  DARK_PALETTE_PRESETS,
  LIGHT_PALETTE_PRESETS,
  chooseContrastText,
  type CatppuccinAccent,
  type ThemeMode,
} from '@/theme/catppuccin'
import { useThemeStore } from '@/stores/theme'
import type { LocaleCode } from '@/types/api'

defineProps<{
  showLanguage?: boolean
}>()

const { t } = useI18n()
const themeStore = useThemeStore()
const open = ref(false)
const langOpen = ref(false)
const containerRef = ref<HTMLElement | null>(null)

function handleOutside(ev: MouseEvent): void {
  if (containerRef.value && !containerRef.value.contains(ev.target as Node)) {
    open.value = false
    langOpen.value = false
  }
}

watch(
  () => open.value || langOpen.value,
  (isAnyOpen) => {
    if (isAnyOpen) document.addEventListener('mousedown', handleOutside)
    else document.removeEventListener('mousedown', handleOutside)
  },
)
onBeforeUnmount(() => document.removeEventListener('mousedown', handleOutside))

const modes: { value: ThemeMode; labelKey: string }[] = [
  { value: 'light', labelKey: 'theme.light' },
  { value: 'dark', labelKey: 'theme.dark' },
  { value: 'system', labelKey: 'theme.system' },
]

const currentPresets = computed(() =>
  themeStore.isDark ? DARK_PALETTE_PRESETS : LIGHT_PALETTE_PRESETS,
)

const locales: { code: LocaleCode; nativeLabel: string }[] = [
  { code: 'zh-CN', nativeLabel: '简体中文' },
  { code: 'en-US', nativeLabel: 'English' },
]

function selectPreset(id: string): void {
  if (themeStore.isDark) {
    themeStore.setDarkPreset(id)
  } else {
    themeStore.setLightPreset(id)
  }
}

function accentColorHex(accent: CatppuccinAccent): string {
  return CATPPUCCIN_ACCENT_HEX[themeStore.isDark ? 'dark' : 'light'][accent]
}
</script>

<template>
  <div ref="containerRef" class="relative flex items-center gap-1">
    <div v-if="showLanguage" class="relative">
      <button
        type="button"
        class="md-state md-label-medium flex h-9 cursor-pointer items-center gap-1.5 rounded-full px-3 text-[var(--md-sys-color-on-surface-variant)] hover:text-[var(--md-sys-color-on-surface)]"
        @click="langOpen = !langOpen; open = false"
      >
        <LuGlobe class="h-4 w-4" />
        <span>{{ themeStore.locale === 'zh-CN' ? '中文' : 'EN' }}</span>
      </button>
      <div
        v-if="langOpen"
        class="absolute top-11 right-0 z-[var(--z-popover)] w-40 rounded-[var(--md-sys-shape-corner-md)] bg-[var(--md-sys-color-surface-container)] p-1.5 text-[var(--md-sys-color-on-surface)] shadow-[var(--md-sys-elevation-2)] ring-1 ring-[var(--md-sys-color-outline-variant)]/40"
      >
        <button
          v-for="loc in locales"
          :key="loc.code"
          type="button"
          class="md-state flex w-full cursor-pointer items-center justify-between rounded-[var(--md-sys-shape-corner-sm)] px-3 py-2 text-xs transition"
          :class="
            themeStore.locale === loc.code
              ? 'bg-[var(--md-sys-color-secondary-container)] font-semibold text-[var(--md-sys-color-on-secondary-container)]'
              : 'text-[var(--md-sys-color-on-surface)]'
          "
          @click="themeStore.setLocale(loc.code); langOpen = false"
        >
          <span>{{ loc.nativeLabel }}</span>
          <LuCheck v-if="themeStore.locale === loc.code" class="h-3.5 w-3.5" />
        </button>
      </div>
    </div>

    <button
      type="button"
      class="md-icon-btn md-state h-9 w-9"
      :title="t('theme.title')"
      :aria-label="t('theme.title')"
      @click="open = !open; langOpen = false"
    >
      <LuPalette class="h-4 w-4 text-[var(--md-sys-color-primary)]" />
    </button>

    <div
      v-if="open"
      class="absolute top-11 right-0 z-[var(--z-popover)] w-80 rounded-[var(--md-sys-shape-corner-lg)] bg-[var(--md-sys-color-surface-container)] p-4 text-[var(--md-sys-color-on-surface)] shadow-[var(--md-sys-elevation-3)] ring-1 ring-[var(--md-sys-color-outline-variant)]/40"
    >
      <div class="mb-3.5">
        <div class="md-label-medium mb-2 text-[var(--md-sys-color-on-surface-variant)]">
          {{ t('theme.modeTitle') }}
        </div>
        <div
          class="grid grid-cols-3 overflow-hidden rounded-full border border-[var(--md-sys-color-outline)]"
        >
          <button
            v-for="(m, idx) in modes"
            :key="m.value"
            type="button"
            class="md-state flex h-9 cursor-pointer items-center justify-center gap-1.5 text-xs font-medium transition focus-visible:z-10 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-[var(--md-sys-color-primary)]"
            :class="[
              idx > 0 ? 'border-l border-[var(--md-sys-color-outline)]' : '',
              themeStore.themeMode === m.value
                ? 'bg-[var(--md-sys-color-secondary-container)] font-semibold text-[var(--md-sys-color-on-secondary-container)]'
                : 'text-[var(--md-sys-color-on-surface-variant)] hover:text-[var(--md-sys-color-on-surface)]',
            ]"
            @click="themeStore.setThemeMode(m.value)"
          >
            <LuSun v-if="m.value === 'light'" class="h-3.5 w-3.5" />
            <LuMoon v-else-if="m.value === 'dark'" class="h-3.5 w-3.5" />
            <LuMonitor v-else class="h-3.5 w-3.5" />
            <span class="truncate">{{ t(m.labelKey) }}</span>
          </button>
        </div>
      </div>

      <div class="mb-3.5">
        <div class="md-label-medium mb-2 text-[var(--md-sys-color-on-surface-variant)]">
          {{ t('theme.activePaletteTitle') }}
        </div>
        <div class="grid grid-cols-2 gap-1.5">
          <button
            v-for="preset in currentPresets"
            :key="preset.id"
            type="button"
            class="md-state flex cursor-pointer items-center gap-2 rounded-[var(--md-sys-shape-corner-sm)] border px-2.5 py-2 text-left text-xs font-medium transition"
            :class="
              themeStore.activePresetId === preset.id
                ? 'border-[var(--md-sys-color-primary)] bg-[var(--md-sys-color-secondary-container)] text-[var(--md-sys-color-on-secondary-container)]'
                : 'border-[var(--md-sys-color-outline-variant)] bg-[var(--md-sys-color-surface-container-low)] text-[var(--md-sys-color-on-surface)]'
            "
            @click="selectPreset(preset.id)"
          >
            <span
              class="h-3 w-3 shrink-0 rounded-full border border-black/15"
              :style="{ backgroundColor: preset.tokens.canvas }"
            />
            <span class="truncate">{{ t(`theme.presets.${preset.id}`) }}</span>
          </button>
        </div>
      </div>

      <div>
        <div class="md-label-medium mb-2 text-[var(--md-sys-color-on-surface-variant)]">
          {{ t('theme.accentTitle') }}
        </div>
        <div class="grid grid-cols-7 gap-1.5">
          <button
            v-for="acc in CATPPUCCIN_ACCENTS"
            :key="acc"
            type="button"
            class="flex h-7 w-7 cursor-pointer items-center justify-center rounded-full transition hover:scale-110 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--md-sys-color-primary)]"
            :style="{ backgroundColor: accentColorHex(acc) }"
            :title="t(`theme.accents.${acc}`)"
            @click="themeStore.setCatppuccinAccent(acc)"
          >
            <LuCheck
              v-if="themeStore.accentMode === 'catppuccin' && themeStore.accent === acc"
              class="h-3.5 w-3.5 stroke-[2.5]"
              :style="{ color: chooseContrastText(accentColorHex(acc)) }"
            />
          </button>
        </div>
      </div>
    </div>
  </div>
</template>
