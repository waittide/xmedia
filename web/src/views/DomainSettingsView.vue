<script setup lang="ts">
import { computed, ref, watch, type Component } from 'vue'
import { useRoute, useRouter, type LocationQueryRaw } from 'vue-router'
import {
  LuAlertCircle,
  LuGlobe,
  LuLibrary,
  LuRadio,
  LuSlidersHorizontal,
} from 'vue-icons-plus/lu'
import SettingsDomainDataSourcesTab from '@/components/settings/SettingsDomainDataSourcesTab.vue'
import SettingsDomainGeneralTab from '@/components/settings/SettingsDomainGeneralTab.vue'
import SettingsDomainProtocolsTab from '@/components/settings/SettingsDomainProtocolsTab.vue'
import SettingsLibrariesTab from '@/components/settings/SettingsLibrariesTab.vue'
import { useSettingsI18n } from '@/components/settings/i18nHelper'
import {
  DOMAIN_ICONS,
  readSingleQuery,
  SOURCES_ADVANCED_STORAGE_KEY,
} from '@/components/settings/sections'
import { MdChip } from '@/components/ui'
import { useAuthStore } from '@/stores/auth'
import { useDomainStore } from '@/stores/domain'
import type { MediaKind } from '@/types/api'

defineOptions({ name: 'DomainSettingsView' })

type DomainSettingsTab = 'preferences' | 'libraries' | 'sources' | 'protocols'

interface TabItem {
  id: DomainSettingsTab
  label: string
  icon: Component
  adminOnly?: boolean
}

const { t, tr } = useSettingsI18n()
const route = useRoute()
const router = useRouter()
const authStore = useAuthStore()
const domainStore = useDomainStore()

const domain = computed<MediaKind>(() => {
  const metaDomain = route.meta.domain
  if (
    metaDomain === 'book' ||
    metaDomain === 'comic' ||
    metaDomain === 'music' ||
    metaDomain === 'video'
  ) {
    return metaDomain
  }
  return domainStore.activeDomain
})

function readStoredAdvanced(): boolean {
  try {
    return localStorage.getItem(SOURCES_ADVANCED_STORAGE_KEY) === '1'
  } catch {
    return false
  }
}

function writeStoredAdvanced(val: boolean): void {
  try {
    localStorage.setItem(SOURCES_ADVANCED_STORAGE_KEY, val ? '1' : '0')
  } catch {
    // ignore storage errors
  }
}

function resolveTabFromQuery(): DomainSettingsTab {
  const raw = readSingleQuery(route.query, 'tab').toLowerCase()
  if (raw === 'libraries' || raw === 'domain-libraries') return 'libraries'
  if (raw === 'sources' || raw === 'datasources' || raw === 'domain-datasources') return 'sources'
  if (raw === 'protocols' || raw === 'opds' || raw === 'domain-protocols') return 'protocols'
  return 'preferences'
}

function resolveAdvancedFromQuery(): boolean {
  const rawAdv = readSingleQuery(route.query, 'advanced')
  if (rawAdv === '1' || rawAdv === 'true') return true
  if (rawAdv === '0' || rawAdv === 'false') return false
  return readStoredAdvanced()
}

const activeTab = ref<DomainSettingsTab>(resolveTabFromQuery())
const sourcesAdvanced = ref<boolean>(resolveAdvancedFromQuery())
const focusKind = ref<string>(readSingleQuery(route.query, 'kind'))
const errorMsg = ref('')

const tabs = computed<TabItem[]>(() => {
  const all: TabItem[] = [
    {
      id: 'preferences',
      label: tr('settings.navDomainGeneral', '偏好设置', 'Preferences'),
      icon: LuSlidersHorizontal,
    },
    {
      id: 'libraries',
      label: tr('settings.navLibraries', '媒体库', 'Libraries'),
      icon: LuLibrary,
      adminOnly: true,
    },
    {
      id: 'sources',
      label: tr('settings.navSources', '来源', 'Sources'),
      icon: LuGlobe,
      adminOnly: true,
    },
    {
      id: 'protocols',
      label: tr('settings.navDomainProtocols', '开放协议', 'Protocols'),
      icon: LuRadio,
    },
  ]
  return all.filter((item) => !item.adminOnly || authStore.isAdmin)
})

const pageTitle = computed(() =>
  tr(
    `settings.domainSettingsTitle.${domain.value}`,
    `${t(`app.domains.${domain.value}`)}设置`,
    `${t(`app.domains.${domain.value}`)} Settings`,
  ),
)

const pageSubtitle = computed(() =>
  authStore.isAdmin
    ? tr(
        'settings.domainSettingsAdminSubtitle',
        '管理当前领域的偏好、媒体库、在线来源与开放协议',
        'Manage preferences, local libraries, online sources, and open protocols for this domain',
      )
    : tr(
        'settings.domainSettingsSubtitle',
        '管理当前领域的阅读播放偏好与开放协议接入',
        'Manage playback preferences and open protocol endpoints for this domain',
      ),
)

function syncRoute(tab: DomainSettingsTab, opts?: { advanced?: boolean }): void {
  const query: LocationQueryRaw = {}
  if (tab !== 'preferences') {
    query.tab = tab
  }
  if (tab === 'sources') {
    const adv = opts?.advanced ?? sourcesAdvanced.value
    if (adv) query.advanced = '1'
  }
  void router.replace({
    path: `/${domain.value}/settings`,
    query: Object.keys(query).length > 0 ? query : undefined,
  })
}

function selectTab(tab: DomainSettingsTab): void {
  activeTab.value = tab
  focusKind.value = ''
  errorMsg.value = ''
  if (tab === 'sources') {
    sourcesAdvanced.value = readStoredAdvanced()
  }
  syncRoute(tab)
}

function handleAdvancedChange(nextAdvanced: boolean): void {
  sourcesAdvanced.value = nextAdvanced
  writeStoredAdvanced(nextAdvanced)
  errorMsg.value = ''
  syncRoute('sources', { advanced: nextAdvanced })
}

watch(
  () => [route.path, route.query.tab, route.query.advanced, route.query.kind] as const,
  () => {
    activeTab.value = resolveTabFromQuery()
    sourcesAdvanced.value = resolveAdvancedFromQuery()
    focusKind.value = readSingleQuery(route.query, 'kind')
    errorMsg.value = ''
  },
)
</script>

<template>
  <div class="mx-auto flex max-w-5xl flex-col gap-6 px-4 py-5 sm:px-6 sm:py-6 lg:px-8">
    <!-- Domain Settings Header -->
    <div class="flex items-center gap-3.5">
      <div
        class="flex h-11 w-11 shrink-0 items-center justify-center rounded-corner-lg bg-secondary-container text-on-secondary-container"
      >
        <component :is="DOMAIN_ICONS[domain]" class="h-5 w-5" aria-hidden="true" />
      </div>
      <div class="min-w-0">
        <h1 class="text-headline-small truncate text-on-surface">
          {{ pageTitle }}
        </h1>
        <p class="text-body-small mt-0.5 text-on-surface-variant">
          {{ pageSubtitle }}
        </p>
      </div>
    </div>

    <!-- Single-row MD3 Filter Chips for Domain Setting Sections -->
    <div
      role="tablist"
      :aria-label="t('settings.navLabel')"
      class="no-scrollbar flex flex-nowrap items-center gap-2 overflow-x-auto py-0.5"
    >
      <MdChip
        v-for="item in tabs"
        :key="item.id"
        :data-domain-settings-tab="item.id"
        variant="filter"
        hide-check
        :icon="item.icon"
        :label="item.label"
        :selected="activeTab === item.id"
        @click="selectTab(item.id)"
      />
    </div>

    <div
      v-if="errorMsg"
      role="alert"
      class="text-body-small flex items-center gap-2 rounded-corner-md bg-error-container px-4 py-3 text-on-error-container"
    >
      <LuAlertCircle class="h-4 w-4 shrink-0" />
      <span>{{ errorMsg }}</span>
    </div>

    <!-- 1. Domain Preferences -->
    <SettingsDomainGeneralTab
      v-if="activeTab === 'preferences'"
      :domain="domain"
    />

    <!-- 2. Domain Local Libraries (Admin) -->
    <SettingsLibrariesTab
      v-else-if="activeTab === 'libraries'"
      :locked-domain="domain"
      @error="errorMsg = $event"
    />

    <!-- 3. Domain Online Sources & Remote Servers (Admin) -->
    <SettingsDomainDataSourcesTab
      v-else-if="activeTab === 'sources'"
      :domain="domain"
      :advanced="sourcesAdvanced"
      :focus-kind="focusKind"
      hide-domain-selector
      @update:advanced="handleAdvancedChange"
      @error="errorMsg = $event"
    />

    <!-- 4. Domain Open Protocols -->
    <SettingsDomainProtocolsTab
      v-else-if="activeTab === 'protocols'"
      :domain="domain"
    />
  </div>
</template>
