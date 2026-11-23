<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { LuCheck, LuGlobe, LuMonitor, LuMoon, LuPalette, LuSun } from 'vue-icons-plus/lu'
import MdIconButton from '@/components/ui/MdIconButton.vue'
import MdSegmentedButton, { type SegmentedOption } from '@/components/ui/MdSegmentedButton.vue'
import { useEscapeStack } from '@/composables/useEscapeStack'
import { useFocusTrap } from '@/composables/useFocusTrap'
import { useThemeStore } from '@/stores/theme'
import { chooseContrastText, SEED_PRESETS, type ThemeMode } from '@/theme/md3'
import type { LocaleCode } from '@/types/api'

defineProps<{
  showLanguage?: boolean
}>()

const { t } = useI18n()
const themeStore = useThemeStore()
const escapeStack = useEscapeStack()
const open = ref(false)
const langOpen = ref(false)
const containerRef = ref<HTMLElement | null>(null)
const popoverRef = ref<HTMLElement | null>(null)

useFocusTrap(popoverRef, () => open.value)

escapeStack.register(
  () => open.value,
  () => {
    open.value = false
  },
)
escapeStack.register(
  () => langOpen.value,
  () => {
    langOpen.value = false
  },
)

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

const modeOptions = computed<SegmentedOption<ThemeMode>[]>(() => [
  { value: 'system', label: t('theme.system'), icon: LuMonitor },
  { value: 'light', label: t('theme.light'), icon: LuSun },
  { value: 'dark', label: t('theme.dark'), icon: LuMoon },
])

const locales: { code: LocaleCode; nativeLabel: string }[] = [
  { code: 'zh-CN', nativeLabel: '简体中文' },
  { code: 'en-US', nativeLabel: 'English' },
]
</script>

<template>
  <div ref="containerRef" class="relative flex items-center gap-1">
    <div v-if="showLanguage" class="relative">
      <button
        type="button"
        aria-haspopup="menu"
        :aria-expanded="langOpen"
        class="md-state md-touch-target text-label-medium flex h-9 cursor-pointer items-center gap-1.5 rounded-corner-full px-3 text-on-surface-variant hover:text-on-surface"
        @click="langOpen = !langOpen; open = false"
      >
        <LuGlobe class="h-4 w-4" />
        <span>{{ themeStore.locale === 'zh-CN' ? '中文' : 'EN' }}</span>
      </button>
      <div
        v-if="langOpen"
        role="menu"
        :aria-label="t('theme.languageTitle')"
        class="absolute top-11 right-0 z-[var(--z-popover)] w-40 max-w-[calc(100vw-2rem)] rounded-corner-xs bg-surface-container py-1 text-on-surface shadow-elevation-2"
      >
        <button
          v-for="loc in locales"
          :key="loc.code"
          type="button"
          role="menuitemradio"
          :aria-checked="themeStore.locale === loc.code"
          class="md-state text-body-medium flex h-10 w-full cursor-pointer items-center justify-between px-3"
          :class="
            themeStore.locale === loc.code
              ? 'bg-secondary-container text-on-secondary-container'
              : 'text-on-surface'
          "
          @click="themeStore.setLocale(loc.code); langOpen = false"
        >
          <span>{{ loc.nativeLabel }}</span>
          <LuCheck v-if="themeStore.locale === loc.code" class="h-4 w-4" />
        </button>
      </div>
    </div>

    <MdIconButton
      :icon="LuPalette"
      :aria-label="t('theme.title')"
      :tooltip="t('theme.title')"
      :aria-expanded="open"
      aria-haspopup="dialog"
      class="text-primary"
      @click="open = !open; langOpen = false"
    />

    <div
      v-if="open"
      ref="popoverRef"
      role="dialog"
      :aria-label="t('theme.title')"
      tabindex="-1"
      class="absolute top-11 right-0 z-[var(--z-popover)] w-72 max-w-[calc(100vw-2rem)] rounded-corner-lg bg-surface-container p-4 text-on-surface shadow-elevation-3 outline-none"
    >
      <div>
        <div class="text-label-medium mb-2 text-on-surface-variant">
          {{ t('theme.modeTitle') }}
        </div>
        <MdSegmentedButton
          :model-value="themeStore.themeMode"
          :options="modeOptions"
          size="sm"
          :show-check="false"
          full-width
          @update:model-value="themeStore.setThemeMode($event)"
        />
      </div>

      <div class="mt-4">
        <div class="text-label-medium mb-2 text-on-surface-variant">
          {{ t('theme.seedColorTitle') }}
        </div>
        <div class="flex items-center justify-between gap-1.5">
          <button
            v-for="preset in SEED_PRESETS"
            :key="preset.id"
            type="button"
            :aria-label="t(`theme.seeds.${preset.id}`)"
            :aria-pressed="themeStore.effectiveSeedColor === preset.hex"
            :title="t(`theme.seeds.${preset.id}`)"
            class="flex h-8 w-8 cursor-pointer items-center justify-center rounded-corner-full transition-transform duration-150 ease-standard hover:scale-110 focus-visible:outline-3 focus-visible:outline-offset-2 focus-visible:outline-secondary"
            :style="{ backgroundColor: preset.hex }"
            @click="themeStore.setSeedColor(preset.hex)"
          >
            <LuCheck
              v-if="themeStore.effectiveSeedColor === preset.hex"
              class="h-4 w-4 stroke-[2.5]"
              :style="{ color: chooseContrastText(preset.hex) }"
            />
          </button>
        </div>
      </div>
    </div>
  </div>
</template>
