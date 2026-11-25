<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch, type Component } from 'vue'
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
import MdButton from '@/components/ui/MdButton.vue'
import MdTabs from '@/components/ui/MdTabs.vue'
import type { MediaKind } from '@/types/api'
import type { ToolSettingsTab } from '../router'
import { useToolWorkspaceStore } from '../stores/workspace'
import ToolRootsSettingsTab from './settings/ToolRootsSettingsTab.vue'
import ToolScrapersSettingsTab from './settings/ToolScrapersSettingsTab.vue'
import ToolTemplatesSettingsTab from './settings/ToolTemplatesSettingsTab.vue'

export type { ToolSettingsTab }

const { t } = useI18n()
const workspaceStore = useToolWorkspaceStore()

const activeTab = computed<ToolSettingsTab>(() => workspaceStore.settingsInitialTab)
const settings = computed(() => workspaceStore.settings)
const ffmpegStatus = computed(() => workspaceStore.toolStatus?.ffmpeg ?? null)
const domainSubTab = ref<MediaKind>(workspaceStore.activeDomain)
const savedBanner = ref(false)
const errorMsg = ref<string | null>(null)

watch(
  () => workspaceStore.activeDomain,
  (dom) => {
    domainSubTab.value = dom
  },
)

function selectSettingsTab(tab: ToolSettingsTab): void {
  void workspaceStore.openSettingsTab(tab)
}

function selectDomainSubTab(dom: MediaKind): void {
  domainSubTab.value = dom
  void workspaceStore.switchDomain(dom)
}

let savedBannerTimer: ReturnType<typeof setTimeout> | null = null

function flashSavedBanner(): void {
  if (savedBannerTimer) clearTimeout(savedBannerTimer)
  savedBanner.value = true
  savedBannerTimer = setTimeout(() => {
    savedBannerTimer = null
    savedBanner.value = false
  }, 2200)
}

onBeforeUnmount(() => {
  if (savedBannerTimer) {
    clearTimeout(savedBannerTimer)
    savedBannerTimer = null
  }
})

const domainTabItems = computed<{ value: MediaKind; label: string; icon: Component }[]>(() => [
  { value: 'book', label: t('tool.domains.book'), icon: LuBookOpen },
  { value: 'comic', label: t('tool.domains.comic'), icon: LuLayers },
  { value: 'music', label: t('tool.domains.music'), icon: LuDisc3 },
  { value: 'video', label: t('tool.domains.video'), icon: LuClapperboard },
])
</script>

<template>
  <div class="mx-auto max-w-5xl px-6 py-8 sm:px-8">
    <!-- Top Header -->
    <div class="mb-8 flex flex-wrap items-center justify-between gap-4">
      <div>
        <h1 class="text-headline-medium text-on-surface">
          {{ t('settings.title') }}
        </h1>
        <p v-if="t('tool.settings.subtitle')" class="text-body-small mt-1 text-on-surface-variant">
          {{ t('tool.settings.subtitle') }}
        </p>
      </div>
      <MdButton variant="tonal" :icon="LuArrowLeft" @click="workspaceStore.exitSettings()">
        {{ t('tool.settings.backToWorkbench') }}
      </MdButton>
    </div>

    <div class="grid grid-cols-1 gap-6 lg:grid-cols-[210px_minmax(0,1fr)]">
      <!-- Left Settings Category Rail -->
      <nav class="no-scrollbar -m-1 flex gap-1 overflow-x-auto p-1 lg:flex-col lg:overflow-visible">
        <button
          type="button"
          class="md-state text-label-large flex h-10 shrink-0 cursor-pointer items-center gap-2.5 rounded-corner-full px-4 text-left"
          :class="
            activeTab === 'appearance'
              ? 'bg-secondary-container text-on-secondary-container'
              : 'text-on-surface-variant hover:text-on-surface'
          "
          @click="selectSettingsTab('appearance')"
        >
          <LuPalette class="h-4 w-4" aria-hidden="true" />
          <span>{{ t('settings.navAppearance') }}</span>
        </button>

        <button
          type="button"
          class="md-state text-label-large flex h-10 shrink-0 cursor-pointer items-center gap-2.5 rounded-corner-full px-4 text-left"
          :class="
            activeTab === 'scrapers'
              ? 'bg-secondary-container text-on-secondary-container'
              : 'text-on-surface-variant hover:text-on-surface'
          "
          @click="selectSettingsTab('scrapers')"
        >
          <LuSparkles class="h-4 w-4" aria-hidden="true" />
          <span>{{ t('tool.settings.tabScrapers') }}</span>
        </button>

        <div class="my-2 hidden h-px bg-outline-variant/50 lg:block" />

        <button
          type="button"
          class="md-state text-label-large flex h-10 shrink-0 cursor-pointer items-center gap-2.5 rounded-corner-full px-4 text-left"
          :class="
            activeTab === 'roots'
              ? 'bg-secondary-container text-on-secondary-container'
              : 'text-on-surface-variant hover:text-on-surface'
          "
          @click="selectSettingsTab('roots')"
        >
          <LuLibrary class="h-4 w-4" aria-hidden="true" />
          <span>{{ t('tool.settings.tabRoots') }}</span>
        </button>

        <button
          type="button"
          class="md-state text-label-large flex h-10 shrink-0 cursor-pointer items-center gap-2.5 rounded-corner-full px-4 text-left"
          :class="
            activeTab === 'templates'
              ? 'bg-secondary-container text-on-secondary-container'
              : 'text-on-surface-variant hover:text-on-surface'
          "
          @click="selectSettingsTab('templates')"
        >
          <LuFileSignature class="h-4 w-4" aria-hidden="true" />
          <span>{{ t('tool.settings.tabTemplates') }}</span>
        </button>
      </nav>

      <!-- Right Content Area -->
      <div class="min-w-0 space-y-6">
        <div
          v-if="errorMsg"
          role="alert"
          class="text-body-small flex items-center gap-2 rounded-corner-lg bg-error-container px-4 py-3 text-on-error-container"
        >
          <LuAlertCircle class="h-4 w-4 shrink-0" aria-hidden="true" />
          <span>{{ errorMsg }}</span>
        </div>

        <div
          v-if="savedBanner"
          class="text-body-small flex items-center gap-2 rounded-corner-lg bg-success-container px-4 py-3 font-medium text-on-success-container"
        >
          <LuCheckCircle2 class="h-4 w-4 shrink-0" aria-hidden="true" />
          <span>{{ t('tool.settings.savedBanner') }}</span>
        </div>

        <!-- Domain Isolation Sub-Tab Bar -->
        <MdTabs
          v-if="activeTab !== 'appearance'"
          :model-value="domainSubTab"
          :tabs="domainTabItems"
          variant="primary"
          :aria-label="t('tool.settings.domainsAriaLabel')"
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
          @updated="workspaceStore.handleSettingsUpdated($event)"
          @error="errorMsg = $event"
          @saved="flashSavedBanner"
        />

        <ToolRootsSettingsTab
          v-else-if="activeTab === 'roots'"
          :settings="settings"
          :domain-sub-tab="domainSubTab"
          @updated="workspaceStore.handleSettingsUpdated($event)"
          @error="errorMsg = $event"
          @saved="flashSavedBanner"
        />

        <ToolTemplatesSettingsTab
          v-else-if="activeTab === 'templates'"
          :settings="settings"
          :domain-sub-tab="domainSubTab"
          @updated="workspaceStore.handleSettingsUpdated($event)"
          @error="errorMsg = $event"
          @saved="flashSavedBanner"
        />
      </div>
    </div>
  </div>
</template>
