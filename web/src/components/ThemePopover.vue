<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { LuPalette, LuSun, LuMoon, LuMonitor, LuCheck, LuGlobe } from 'vue-icons-plus/lu'
import {
  CATPPUCCIN_ACCENTS,
  CATPPUCCIN_ACCENT_HEX,
  DARK_PALETTE_PRESETS,
  LIGHT_PALETTE_PRESETS,
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

onMounted(() => document.addEventListener('mousedown', handleOutside))
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
        class="flex h-9 items-center gap-1.5 rounded-xl px-2.5 text-xs font-medium text-[var(--ctp-subtext1)] transition hover:bg-[var(--ctp-surface0)] hover:text-[var(--ctp-text)]"
        @click="langOpen = !langOpen; open = false"
      >
        <LuGlobe class="h-4 w-4" />
        <span>{{ themeStore.locale === 'zh-CN' ? '中文' : 'EN' }}</span>
      </button>
      <div
        v-if="langOpen"
        class="absolute right-0 top-11 z-50 w-36 rounded-2xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] p-1.5 shadow-xl"
      >
        <button
          v-for="loc in locales"
          :key="loc.code"
          type="button"
          class="flex w-full items-center justify-between rounded-xl px-3 py-2 text-xs transition"
          :class="
            themeStore.locale === loc.code
              ? 'bg-[var(--ctp-accent-soft)] font-semibold text-[var(--ctp-accent)]'
              : 'text-[var(--ctp-text)] hover:bg-[var(--ctp-surface0)]'
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
      class="flex h-9 w-9 items-center justify-center rounded-xl text-[var(--ctp-subtext1)] transition hover:bg-[var(--ctp-surface0)] hover:text-[var(--ctp-text)]"
      :title="t('theme.title')"
      @click="open = !open; langOpen = false"
    >
      <LuPalette class="h-4 w-4 text-[var(--ctp-accent)]" />
    </button>

    <div
      v-if="open"
      class="absolute right-0 top-11 z-50 w-80 rounded-2xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] p-4 shadow-2xl"
    >
      <div class="mb-3">
        <div class="mb-1.5 text-xs font-medium text-[var(--ctp-subtext0)]">
          {{ t('theme.modeTitle') }}
        </div>
        <div class="grid grid-cols-3 gap-1 rounded-xl bg-[var(--ctp-mantle)] p-1">
          <button
            v-for="m in modes"
            :key="m.value"
            type="button"
            class="flex items-center justify-center gap-1 rounded-lg py-1.5 text-xs font-medium transition"
            :class="
              themeStore.themeMode === m.value
                ? 'bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)] shadow-sm'
                : 'text-[var(--ctp-subtext1)] hover:text-[var(--ctp-text)]'
            "
            @click="themeStore.setThemeMode(m.value)"
          >
            <LuSun v-if="m.value === 'light'" class="h-3.5 w-3.5" />
            <LuMoon v-else-if="m.value === 'dark'" class="h-3.5 w-3.5" />
            <LuMonitor v-else class="h-3.5 w-3.5" />
            <span class="truncate">{{ t(m.labelKey) }}</span>
          </button>
        </div>
      </div>

      <div class="mb-3">
        <div class="mb-1.5 text-xs font-medium text-[var(--ctp-subtext0)]">
          {{ t('theme.activePaletteTitle') }}
        </div>
        <div class="grid grid-cols-2 gap-1.5">
          <button
            v-for="preset in currentPresets"
            :key="preset.id"
            type="button"
            class="flex items-center gap-2 rounded-xl border px-2.5 py-1.5 text-left text-xs font-medium transition"
            :class="
              themeStore.activePresetId === preset.id
                ? 'border-[var(--ctp-accent)] bg-[var(--ctp-accent-soft)] text-[var(--ctp-text)]'
                : 'border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)] text-[var(--ctp-subtext1)]'
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
        <div class="mb-1.5 text-xs font-medium text-[var(--ctp-subtext0)]">
          {{ t('theme.accentTitle') }}
        </div>
        <div class="grid grid-cols-7 gap-1.5">
          <button
            v-for="acc in CATPPUCCIN_ACCENTS"
            :key="acc"
            type="button"
            class="flex h-7 w-7 items-center justify-center rounded-full transition hover:scale-110"
            :style="{ backgroundColor: accentColorHex(acc) }"
            :title="t(`theme.accents.${acc}`)"
            @click="themeStore.setCatppuccinAccent(acc)"
          >
            <LuCheck
              v-if="themeStore.accentMode === 'catppuccin' && themeStore.accent === acc"
              class="h-3.5 w-3.5"
              :style="{ color: themeStore.isDark ? '#11111b' : '#ffffff' }"
            />
          </button>
        </div>
      </div>
    </div>
  </div>
</template>
