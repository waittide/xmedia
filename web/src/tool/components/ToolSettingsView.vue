<script setup lang="ts">
import { computed, ref, watch, type Component } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  LuAlertCircle,
  LuArrowLeft,
  LuBookOpen,
  LuCheckCircle2,
  LuClapperboard,
  LuDisc3,
  LuFileSignature,
  LuLayers,
  LuLibrary,
  LuPalette,
  LuSparkles,
} from 'vue-icons-plus/lu'
import AppearanceSettingsPanel from '@/components/AppearanceSettingsPanel.vue'
import MdTabs from '@/components/ui/MdTabs.vue'
import type { FfmpegStatusDto, MediaKind } from '@/types/api'
import { useToolText } from '../i18n'
import type { PublicSettingsDto } from '../types'
import ToolScrapersSettingsTab from './settings/ToolScrapersSettingsTab.vue'
import ToolRootsSettingsTab from './settings/ToolRootsSettingsTab.vue'
import ToolTemplatesSettingsTab from './settings/ToolTemplatesSettingsTab.vue'

export type ToolSettingsTab = 'appearance' | 'scrapers' | 'roots' | 'templates'

const props = defineProps<{
  settings: PublicSettingsDto | null
  activeDomain: MediaKind
  initialTab?: ToolSettingsTab
  ffmpegStatus?: FfmpegStatusDto | null
}>()

const emit = defineEmits<{
  back: []
  updated: [settings: PublicSettingsDto]
  selectDomain: [domain: MediaKind]
}>()

const { t } = useI18n()
const txt = useToolText()

const activeTab = ref<ToolSettingsTab>(props.initialTab ?? 'appearance')
const domainSubTab = ref<MediaKind>(props.activeDomain)
const savedBanner = ref(false)
const errorMsg = ref<string | null>(null)

watch(
  () => props.initialTab,
  (next) => {
    if (next) activeTab.value = next
  },
)

watch(
  () => props.activeDomain,
  (dom) => {
    domainSubTab.value = dom
  },
)

function selectDomainSubTab(dom: MediaKind): void {
  domainSubTab.value = dom
  emit('selectDomain', dom)
}

function flashSavedBanner(): void {
  savedBanner.value = true
  setTimeout(() => {
    savedBanner.value = false
  }, 2200)
}

const domainTabItems = computed<{ value: MediaKind; label: string; icon: Component }[]>(() => [
  { value: 'book', label: txt.value.domains.book, icon: LuBookOpen },
  { value: 'comic', label: txt.value.domains.comic, icon: LuLayers },
  { value: 'music', label: txt.value.domains.music, icon: LuDisc3 },
  { value: 'video', label: txt.value.domains.video, icon: LuClapperboard },
])
</script>

<template>
  <div class="mx-auto max-w-5xl px-6 py-8 sm:px-8">
    <!-- Top Header (Aligned with Server SettingsView) -->
    <div class="mb-8 flex flex-wrap items-center justify-between gap-4">
      <div>
        <h1 class="text-2xl font-bold tracking-tight text-[var(--md-sys-color-on-surface)]">
          {{ t('settings.title') }}
        </h1>
        <p class="mt-1 text-xs text-[var(--md-sys-color-on-surface-variant)]">
          {{ txt.settings.subtitle }}
        </p>
      </div>
      <button
        type="button"
        class="md-btn md-btn-sm md-btn-tonal md-state"
        @click="emit('back')"
      >
        <LuArrowLeft class="h-3.5 w-3.5" />
        <span>{{ txt.settings.backToWorkbench }}</span>
      </button>
    </div>

    <div class="grid grid-cols-1 gap-6 lg:grid-cols-[210px_minmax(0,1fr)]">
      <!-- Left Settings Category Rail (MD3 Full-Pill Active Indicator) -->
      <nav class="no-scrollbar -m-1 flex gap-1 overflow-x-auto p-1 lg:flex-col lg:overflow-visible">
        <button
          type="button"
          class="md-state flex h-10 shrink-0 items-center gap-2.5 rounded-full px-4 text-left text-[13px] font-medium"
          :class="
            activeTab === 'appearance'
              ? 'bg-[var(--md-sys-color-secondary-container)] font-semibold text-[var(--md-sys-color-on-secondary-container)]'
              : 'text-[var(--md-sys-color-on-surface-variant)] hover:text-[var(--md-sys-color-on-surface)]'
          "
          @click="activeTab = 'appearance'"
        >
          <LuPalette class="h-4 w-4" />
          <span>{{ t('settings.navAppearance') }}</span>
        </button>

        <button
          type="button"
          class="md-state flex h-10 shrink-0 items-center gap-2.5 rounded-full px-4 text-left text-[13px] font-medium"
          :class="
            activeTab === 'scrapers'
              ? 'bg-[var(--md-sys-color-secondary-container)] font-semibold text-[var(--md-sys-color-on-secondary-container)]'
              : 'text-[var(--md-sys-color-on-surface-variant)] hover:text-[var(--md-sys-color-on-surface)]'
          "
          @click="activeTab = 'scrapers'"
        >
          <LuSparkles class="h-4 w-4" />
          <span>{{ txt.settings.tabScrapers }}</span>
        </button>

        <div class="my-2 hidden h-px bg-[var(--md-sys-color-outline-variant)]/50 lg:block" />

        <button
          type="button"
          class="md-state flex h-10 shrink-0 items-center gap-2.5 rounded-full px-4 text-left text-[13px] font-medium"
          :class="
            activeTab === 'roots'
              ? 'bg-[var(--md-sys-color-secondary-container)] font-semibold text-[var(--md-sys-color-on-secondary-container)]'
              : 'text-[var(--md-sys-color-on-surface-variant)] hover:text-[var(--md-sys-color-on-surface)]'
          "
          @click="activeTab = 'roots'"
        >
          <LuLibrary class="h-4 w-4" />
          <span>{{ txt.settings.tabRoots }}</span>
        </button>

        <button
          type="button"
          class="md-state flex h-10 shrink-0 items-center gap-2.5 rounded-full px-4 text-left text-[13px] font-medium"
          :class="
            activeTab === 'templates'
              ? 'bg-[var(--md-sys-color-secondary-container)] font-semibold text-[var(--md-sys-color-on-secondary-container)]'
              : 'text-[var(--md-sys-color-on-surface-variant)] hover:text-[var(--md-sys-color-on-surface)]'
          "
          @click="activeTab = 'templates'"
        >
          <LuFileSignature class="h-4 w-4" />
          <span>{{ txt.settings.tabTemplates }}</span>
        </button>

      </nav>

      <!-- Right Content Area -->
      <div class="min-w-0 space-y-6">
        <div
          v-if="errorMsg"
          class="flex items-center gap-2 rounded-2xl bg-[var(--md-sys-color-error-container)] px-4 py-3 text-xs text-[var(--md-sys-color-on-error-container)]"
        >
          <LuAlertCircle class="h-4 w-4 shrink-0" />
          <span>{{ errorMsg }}</span>
        </div>

        <div
          v-if="savedBanner"
          class="flex items-center gap-2 rounded-2xl bg-[var(--md-sys-color-success-container)] px-4 py-3 text-xs font-medium text-[var(--md-sys-color-on-success-container)]"
        >
          <LuCheckCircle2 class="h-4 w-4 shrink-0" />
          <span>{{ txt.settings.savedBanner }}</span>
        </div>

        <!-- Domain Isolation Sub-Tab Bar (for scrapers / roots / templates) -->
        <MdTabs
          v-if="activeTab !== 'appearance'"
          :model-value="domainSubTab"
          :tabs="domainTabItems"
          variant="primary"
          aria-label="媒体领域"
          @update:model-value="selectDomainSubTab($event)"
        />

        <template v-if="activeTab === 'appearance'">
          <AppearanceSettingsPanel />
        </template>

        <ToolScrapersSettingsTab
          v-else-if="activeTab === 'scrapers'"
          :settings="settings"
          :domain-sub-tab="domainSubTab"
          :ffmpeg-status="ffmpegStatus"
          @updated="emit('updated', $event)"
          @error="errorMsg = $event"
          @saved="flashSavedBanner"
        />

        <ToolRootsSettingsTab
          v-else-if="activeTab === 'roots'"
          :settings="settings"
          :domain-sub-tab="domainSubTab"
          @updated="emit('updated', $event)"
          @error="errorMsg = $event"
          @saved="flashSavedBanner"
        />

        <ToolTemplatesSettingsTab
          v-else-if="activeTab === 'templates'"
          :settings="settings"
          :domain-sub-tab="domainSubTab"
          @updated="emit('updated', $event)"
          @error="errorMsg = $event"
          @saved="flashSavedBanner"
        />
      </div>
    </div>
  </div>
</template>
