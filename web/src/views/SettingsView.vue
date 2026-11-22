<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useRoute, useRouter, type LocationQueryRaw } from 'vue-router'
import { LuAlertCircle, LuArrowLeft, LuChevronRight } from 'vue-icons-plus/lu'
import AppearanceSettingsPanel from '@/components/AppearanceSettingsPanel.vue'
import SettingsAccountTab from '@/components/settings/SettingsAccountTab.vue'
import SettingsDomainDataSourcesTab from '@/components/settings/SettingsDomainDataSourcesTab.vue'
import SettingsDomainGeneralTab from '@/components/settings/SettingsDomainGeneralTab.vue'
import SettingsGeneralTab from '@/components/settings/SettingsGeneralTab.vue'
import SettingsLibrariesTab from '@/components/settings/SettingsLibrariesTab.vue'
import SettingsUsersTab from '@/components/settings/SettingsUsersTab.vue'
import { useSettingsI18n } from '@/components/settings/i18nHelper'
import {
  hasExplicitSectionInRoute,
  isLegacyDatasourcesDeepLink,
  readSingleQuery,
  resolveDomainFromQuery,
  resolveSectionFromRoute,
  SETTINGS_SECTIONS,
  SOURCES_ADVANCED_STORAGE_KEY,
  type SettingsSection,
  type SettingsSectionDef,
} from '@/components/settings/sections'
import { MdCard, MdIconButton } from '@/components/ui'
import { isMediaKind } from '@/sources'
import { useAppClientStore } from '@/stores/appServers'
import { useAuthStore } from '@/stores/auth'
import { useDomainStore } from '@/stores/domain'
import type { MediaKind } from '@/types/api'

const { t, tr } = useSettingsI18n()
const route = useRoute()
const router = useRouter()
const authStore = useAuthStore()
const domainStore = useDomainStore()
const appClientStore = useAppClientStore()

const visibleSections = computed<SettingsSectionDef[]>(() =>
  SETTINGS_SECTIONS.filter((item) => !item.adminOnly || authStore.isAdmin),
)

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

function resolveAdvancedFromRoute(): boolean {
  const rawAdv = readSingleQuery(route.query, 'advanced')
  if (rawAdv === '1' || rawAdv === 'true') return true
  if (rawAdv === '0' || rawAdv === 'false') return false
  if (isLegacyDatasourcesDeepLink(route)) return true
  return readStoredAdvanced()
}

function resolveLibraryFilterFromRoute(): 'all' | MediaKind {
  const rawDomain = readSingleQuery(route.query, 'domain')
  if (isMediaKind(rawDomain) && authStore.hasModule(rawDomain)) {
    return rawDomain
  }
  return 'all'
}

function supportsSectionPath(): boolean {
  return router
    .getRoutes()
    .some((r) => r.path.includes('/settings/:section') || r.name === 'settings-appearance')
}

const initialResolved = resolveSectionFromRoute(route, appClientStore.isStandaloneApp)
const activeSection = ref<SettingsSection>(initialResolved.section)
const activeAnchorHash = ref<string>(initialResolved.derivedHash)
const activeFocusKind = ref<string>(initialResolved.derivedKind)
const activeSourcesDomain = ref<MediaKind>(
  resolveDomainFromQuery(route, authStore.enabledModules, domainStore.activeDomain),
)
const libraryFilterDomain = ref<'all' | MediaKind>(resolveLibraryFilterFromRoute())
const sourcesAdvanced = ref<boolean>(resolveAdvancedFromRoute())
const mobileShowDetail = ref<boolean>(hasExplicitSectionInRoute(route))
const errorMsg = ref('')

const activeSectionDef = computed<SettingsSectionDef>(
  () => SETTINGS_SECTIONS.find((s) => s.id === activeSection.value) ?? SETTINGS_SECTIONS[0]!,
)

function sectionLabel(item: SettingsSectionDef): string {
  return tr(item.labelKey, item.labelZh, item.labelEn)
}

function sectionSummary(item: SettingsSectionDef): string {
  return tr(item.summaryKey, item.summaryZh, item.summaryEn)
}

function syncRoute(
  section: SettingsSection,
  opts?: {
    domain?: MediaKind | 'all'
    advanced?: boolean
    kind?: string
    hash?: string
  },
): void {
  const usePathMode = supportsSectionPath()
  const query: LocationQueryRaw = {}
  if (!usePathMode) {
    query.tab = section === 'sources' && opts?.advanced ? 'domain-datasources' : section
  }

  if (section === 'sources') {
    const dom =
      opts?.domain && opts.domain !== 'all' ? opts.domain : activeSourcesDomain.value
    query.domain = dom
    const adv = opts?.advanced ?? sourcesAdvanced.value
    if (adv) query.advanced = '1'
    if (opts?.kind) query.kind = opts.kind
  } else if (section === 'libraries') {
    const dom = opts?.domain ?? libraryFilterDomain.value
    if (dom && dom !== 'all') {
      query.domain = dom
    }
  }

  const targetPath = usePathMode ? `/settings/${section}` : '/settings'
  const hash = opts?.hash ?? ''
  void router.replace({
    path: targetPath,
    query: Object.keys(query).length > 0 ? query : undefined,
    hash: hash || undefined,
  })
}

function applyRouteState(): void {
  const resolved = resolveSectionFromRoute(route, appClientStore.isStandaloneApp)
  activeSection.value = resolved.section
  activeAnchorHash.value = resolved.derivedHash
  activeFocusKind.value = resolved.derivedKind

  if (resolved.section === 'sources') {
    activeSourcesDomain.value = resolveDomainFromQuery(
      route,
      authStore.enabledModules,
      domainStore.activeDomain,
    )
    sourcesAdvanced.value = resolveAdvancedFromRoute()
  } else if (resolved.section === 'libraries') {
    libraryFilterDomain.value = resolveLibraryFilterFromRoute()
  }

  mobileShowDetail.value = hasExplicitSectionInRoute(route)
}

onMounted(() => {
  applyRouteState()
})

watch(
  () => [route.params.section, route.query.tab, route.query.domain, route.query.advanced, route.query.kind, route.hash] as const,
  () => {
    applyRouteState()
  },
)

function selectSection(section: SettingsSection): void {
  activeSection.value = section
  activeAnchorHash.value = ''
  activeFocusKind.value = ''
  mobileShowDetail.value = true
  errorMsg.value = ''
  if (section === 'sources') {
    sourcesAdvanced.value = readStoredAdvanced()
  }
  syncRoute(section)
}

function handleMobileBack(): void {
  mobileShowDetail.value = false
  errorMsg.value = ''
  void router.replace({ path: '/settings' })
}

function handleSourcesDomainChange(nextDomain: MediaKind): void {
  activeSourcesDomain.value = nextDomain
  errorMsg.value = ''
  syncRoute('sources', { domain: nextDomain, advanced: sourcesAdvanced.value })
}

function handleSourcesAdvancedChange(nextAdvanced: boolean): void {
  sourcesAdvanced.value = nextAdvanced
  writeStoredAdvanced(nextAdvanced)
  errorMsg.value = ''
  syncRoute('sources', { domain: activeSourcesDomain.value, advanced: nextAdvanced })
}

function handleLibrariesFilterChange(nextFilter: 'all' | MediaKind): void {
  libraryFilterDomain.value = nextFilter
  errorMsg.value = ''
  syncRoute('libraries', { domain: nextFilter })
}
</script>

<template>
  <div class="mx-auto max-w-6xl px-4 py-5 sm:px-6 sm:py-6 lg:px-8">
    <!-- Page Header (Desktop always visible; Mobile visible on List view) -->
    <div class="mb-6" :class="mobileShowDetail ? 'hidden md:block' : 'block'">
      <h1 class="text-headline-small text-on-surface">
        {{ t('settings.title') }}
      </h1>
    </div>

    <!-- Mobile Detail Top Back Bar (<840px only) -->
    <div
      v-if="mobileShowDetail"
      class="mb-4 flex items-center gap-2 border-b border-outline-variant/60 pb-3 md:hidden"
    >
      <MdIconButton
        size="sm"
        :icon="LuArrowLeft"
        :aria-label="tr('settings.backToList', '返回设置列表', 'Back to settings list')"
        @click="handleMobileBack"
      />
      <div class="min-w-0">
        <h1 class="text-title-large truncate text-on-surface">
          {{ sectionLabel(activeSectionDef) }}
        </h1>
      </div>
    </div>

    <!-- MD3 Single-Level 6-Section List-Detail Grid -->
    <div class="grid grid-cols-1 gap-6 md:grid-cols-[240px_minmax(0,1fr)] md:gap-8">
      <!-- Left List Pane: Desktop sticky nav / Mobile full-width list card -->
      <nav
        :aria-label="t('settings.navLabel')"
        class="md:sticky md:top-6 md:self-start"
        :class="mobileShowDetail ? 'hidden md:block' : 'block'"
      >
        <!-- Mobile List Card (<840px) -->
        <MdCard variant="outlined" padding="none" class="overflow-hidden md:hidden">
          <div class="divide-y divide-outline-variant/50">
            <button
              v-for="item in visibleSections"
              :key="`mobile-${item.id}`"
              type="button"
              class="md-state flex min-h-[72px] w-full cursor-pointer items-center gap-4 px-4 py-3 text-left"
              :class="
                activeSection === item.id && mobileShowDetail
                  ? 'bg-secondary-container text-on-secondary-container'
                  : 'text-on-surface'
              "
              @click="selectSection(item.id)"
            >
              <div
                class="flex h-10 w-10 shrink-0 items-center justify-center rounded-corner-md bg-surface-container-high text-on-surface-variant"
              >
                <component :is="item.icon" class="h-5 w-5" />
              </div>
              <div class="min-w-0 flex-1">
                <p class="text-body-large truncate text-on-surface">
                  {{ sectionLabel(item) }}
                </p>
                <p class="text-body-small truncate text-on-surface-variant">
                  {{ sectionSummary(item) }}
                </p>
              </div>
              <LuChevronRight class="h-4 w-4 shrink-0 text-on-surface-variant" />
            </button>
          </div>
        </MdCard>

        <!-- Desktop Single-Level 6-Item Rail (>=840px) -->
        <div class="hidden flex-col gap-1 md:flex">
          <button
            v-for="item in visibleSections"
            :key="item.id"
            :data-settings-tab="item.testId"
            type="button"
            :aria-current="activeSection === item.id ? 'page' : undefined"
            class="md-state text-label-large flex h-12 w-full cursor-pointer items-center gap-3 rounded-corner-full px-4 text-left transition-colors"
            :class="
              activeSection === item.id
                ? 'bg-secondary-container text-on-secondary-container'
                : 'text-on-surface-variant hover:text-on-surface'
            "
            @click="selectSection(item.id)"
          >
            <component :is="item.icon" class="h-[18px] w-[18px] shrink-0" />
            <span class="truncate">{{ sectionLabel(item) }}</span>
          </button>
        </div>
      </nav>

      <!-- Right Detail Pane (Avoid `.space-y-4` to prevent E2E selector collision with source import form) -->
      <div
        class="min-w-0 flex-col gap-6"
        :class="mobileShowDetail ? 'flex' : 'hidden md:flex'"
      >
        <!-- Desktop Active Section Header -->
        <div class="hidden md:block">
          <h2 class="text-title-large text-on-surface">
            {{ sectionLabel(activeSectionDef) }}
          </h2>
          <p class="text-body-small mt-0.5 text-on-surface-variant">
            {{ sectionSummary(activeSectionDef) }}
          </p>
        </div>

        <div
          v-if="errorMsg"
          role="alert"
          class="text-body-small flex items-center gap-2 rounded-corner-md bg-error-container px-4 py-3 text-on-error-container"
        >
          <LuAlertCircle class="h-4 w-4 shrink-0" />
          <span>{{ errorMsg }}</span>
        </div>

        <!-- 1. Appearance -->
        <div v-if="activeSection === 'appearance'" class="flex flex-col gap-5">
          <AppearanceSettingsPanel />
        </div>

        <!-- 2. Reading & Playback (4 anchored domain sections) -->
        <SettingsDomainGeneralTab
          v-else-if="activeSection === 'playback'"
          :anchor-hash="activeAnchorHash"
        />

        <!-- 3. Media Libraries (All enabled domains on one page) -->
        <SettingsLibrariesTab
          v-else-if="activeSection === 'libraries'"
          :filter-domain="libraryFilterDomain"
          @update:filter-domain="handleLibrariesFilterChange"
          @error="errorMsg = $event"
        />

        <!-- 4. Sources (Online Sources + Standalone Remote Servers + Advanced toggle) -->
        <SettingsDomainDataSourcesTab
          v-else-if="activeSection === 'sources'"
          :domain="activeSourcesDomain"
          :advanced="sourcesAdvanced"
          :focus-kind="activeFocusKind"
          @update:domain="handleSourcesDomainChange"
          @update:advanced="handleSourcesAdvancedChange"
          @error="errorMsg = $event"
        />

        <!-- 5. Account & Security -->
        <div v-else-if="activeSection === 'account'" class="flex flex-col gap-5">
          <SettingsAccountTab @error="errorMsg = $event" />
          <SettingsUsersTab v-if="authStore.isAdmin" @error="errorMsg = $event" />
        </div>

        <!-- 6. About & Exposed Protocols -->
        <SettingsGeneralTab
          v-else-if="activeSection === 'about'"
          :anchor-hash="activeAnchorHash"
        />
      </div>
    </div>
  </div>
</template>
