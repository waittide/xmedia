<script setup lang="ts">
import { computed, nextTick, onMounted, ref, watch, type Component } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useI18n } from 'vue-i18n'
import {
  LuAlertCircle,
  LuBookOpen,
  LuClapperboard,
  LuDisc3,
  LuGlobe,
  LuKeyRound,
  LuLayers,
  LuLibrary,
  LuPalette,
  LuRadio,
  LuSlidersHorizontal,
  LuWrench,
} from 'vue-icons-plus/lu'
import { useAuthStore } from '@/stores/auth'
import { useDomainStore } from '@/stores/domain'
import type { MediaKind } from '@/types/api'
import AppearanceSettingsPanel from '@/components/AppearanceSettingsPanel.vue'
import SettingsGeneralTab from '@/components/settings/SettingsGeneralTab.vue'
import SettingsAccountTab from '@/components/settings/SettingsAccountTab.vue'
import SettingsUsersTab from '@/components/settings/SettingsUsersTab.vue'
import SettingsDomainGeneralTab from '@/components/settings/SettingsDomainGeneralTab.vue'
import SettingsDomainProtocolsTab from '@/components/settings/SettingsDomainProtocolsTab.vue'
import SettingsDomainDataSourcesTab from '@/components/settings/SettingsDomainDataSourcesTab.vue'
import SettingsLibrariesTab from '@/components/settings/SettingsLibrariesTab.vue'
import MdTabs, { type MdTabItem } from '@/components/ui/MdTabs.vue'

type GlobalSettingsTab = 'appearance' | 'general' | 'account'
type DomainSettingsTab =
  | 'domain-general'
  | 'domain-protocols'
  | 'domain-datasources'
  | 'domain-libraries'
type SettingsTab = GlobalSettingsTab | DomainSettingsTab

interface SettingsNavItem {
  id: SettingsTab
  labelKey: string
  icon: Component
  adminOnly?: boolean
}

const GLOBAL_NAV_ITEMS: readonly SettingsNavItem[] = [
  { id: 'appearance', labelKey: 'settings.navAppearance', icon: LuPalette },
  { id: 'general', labelKey: 'settings.navGeneral', icon: LuWrench },
  { id: 'account', labelKey: 'settings.navAccount', icon: LuKeyRound },
]

const DOMAIN_NAV_ITEMS: readonly SettingsNavItem[] = [
  { id: 'domain-general', labelKey: 'settings.navDomainGeneral', icon: LuSlidersHorizontal },
  { id: 'domain-protocols', labelKey: 'settings.navDomainProtocols', icon: LuRadio },
  { id: 'domain-datasources', labelKey: 'settings.navDomainDataSources', icon: LuGlobe },
  {
    id: 'domain-libraries',
    labelKey: 'settings.navDomainLibraries',
    icon: LuLibrary,
    adminOnly: true,
  },
]

const DOMAIN_ICONS: Record<MediaKind, Component> = {
  book: LuBookOpen,
  comic: LuLayers,
  music: LuDisc3,
  video: LuClapperboard,
}

const { t } = useI18n()
const route = useRoute()
const router = useRouter()
const authStore = useAuthStore()
const domainStore = useDomainStore()

const visibleGlobalItems = computed(() =>
  GLOBAL_NAV_ITEMS.filter((item) => !item.adminOnly || authStore.isAdmin),
)

const visibleDomainItems = computed(() =>
  DOMAIN_NAV_ITEMS.filter((item) => !item.adminOnly || authStore.isAdmin),
)

const enabledDomainOptions = computed<MdTabItem<MediaKind>[]>(() =>
  authStore.enabledModules.map((kind) => ({
    value: kind,
    label: t(`app.domains.${kind}`),
    icon: DOMAIN_ICONS[kind],
  })),
)

function isDomainTab(tab: SettingsTab): tab is DomainSettingsTab {
  return (
    tab === 'domain-general' ||
    tab === 'domain-protocols' ||
    tab === 'domain-datasources' ||
    tab === 'domain-libraries'
  )
}

function resolveTabFromQuery(): SettingsTab {
  const q = route.query.tab
  if (q === 'general') return 'general'
  if (q === 'account' || q === 'profile' || q === 'users') return 'account'
  if (q === 'domain-general' || q === 'preferences' || q === 'reader' || q === 'playback') {
    return 'domain-general'
  }
  if (q === 'domain-protocols' || q === 'protocols' || q === 'opds') {
    return 'domain-protocols'
  }
  if (q === 'domain-datasources' || q === 'datasources' || q === 'sources') {
    return 'domain-datasources'
  }
  if (authStore.isAdmin && (q === 'domain-libraries' || q === 'libraries')) {
    return 'domain-libraries'
  }
  return 'appearance'
}

function resolveDomainFromQuery(): MediaKind {
  const q = route.query.domain
  if (
    (q === 'book' || q === 'comic' || q === 'music' || q === 'video') &&
    authStore.enabledModules.includes(q)
  ) {
    return q
  }
  if (authStore.enabledModules.includes(domainStore.activeDomain)) {
    return domainStore.activeDomain
  }
  return authStore.enabledModules[0] ?? 'book'
}

const activeTab = ref<SettingsTab>(resolveTabFromQuery())
const activeDomain = ref<MediaKind>(resolveDomainFromQuery())
const errorMsg = ref('')
const navRef = ref<HTMLElement | null>(null)

const showDomainTabs = computed(
  () => isDomainTab(activeTab.value) && enabledDomainOptions.value.length > 0,
)

function scrollActiveNavIntoView(smooth = false): void {
  void nextTick(() => {
    const navEl = navRef.value
    if (!navEl || navEl.scrollWidth <= navEl.clientWidth) return
    const activeEl = navEl.querySelector<HTMLElement>(`[data-settings-tab="${activeTab.value}"]`)
    activeEl?.scrollIntoView({
      behavior: smooth ? 'smooth' : 'auto',
      inline: 'center',
      block: 'nearest',
    })
  })
}

function normalizeRouteQuery(): void {
  const resolvedTab = resolveTabFromQuery()
  const resolvedDomain = resolveDomainFromQuery()
  activeTab.value = resolvedTab
  activeDomain.value = resolvedDomain

  const rawTab = typeof route.query.tab === 'string' ? route.query.tab : ''
  const rawDomain = typeof route.query.domain === 'string' ? route.query.domain : ''
  const needsTabSync = Boolean(rawTab) && rawTab !== resolvedTab
  const needsDomainSync = isDomainTab(resolvedTab)
    ? rawDomain !== resolvedDomain
    : Boolean(rawDomain)

  if (needsTabSync || needsDomainSync) {
    syncRoute(resolvedTab, resolvedDomain)
  }
}

onMounted(() => {
  normalizeRouteQuery()
  scrollActiveNavIntoView(false)
})

watch(
  () => [route.query.tab, route.query.domain] as const,
  () => {
    normalizeRouteQuery()
    scrollActiveNavIntoView(true)
  },
)

function syncRoute(tab: SettingsTab, domain: MediaKind): void {
  const query: Record<string, string> = { tab }
  if (isDomainTab(tab)) {
    query.domain = domain
  }
  void router.replace({ path: '/settings', query })
}

function selectTab(tab: SettingsTab): void {
  activeTab.value = tab
  errorMsg.value = ''
  syncRoute(tab, activeDomain.value)
}

function selectDomain(domain: MediaKind): void {
  activeDomain.value = domain
  errorMsg.value = ''
  syncRoute(activeTab.value, domain)
}
</script>

<template>
  <div class="mx-auto max-w-6xl px-6 py-6 sm:px-8 sm:py-7">
    <div class="mb-6">
      <h1 class="md-headline-small font-semibold tracking-tight text-[var(--md-sys-color-on-surface)]">
        {{ t('settings.title') }}
      </h1>
      <p
        v-if="t('settings.subtitle')"
        class="md-body-small mt-1 text-[var(--md-sys-color-on-surface-variant)]"
      >
        {{ t('settings.subtitle') }}
      </p>
    </div>

    <div class="grid grid-cols-1 gap-6 md:grid-cols-[204px_minmax(0,1fr)] md:gap-8">
      <!-- Left Settings Category Rail (Upper: Global / Lower: Domain-Scoped) -->
      <nav
        ref="navRef"
        :aria-label="t('settings.navLabel')"
        class="-m-1 flex flex-wrap items-center gap-1 p-1 md:flex-col md:flex-nowrap md:items-stretch"
      >
        <p
          class="md-label-small hidden px-3.5 pt-1 pb-1.5 tracking-wide text-[var(--md-sys-color-on-surface-variant)] md:block"
        >
          {{ t('settings.groupGlobal') }}
        </p>

        <button
          v-for="item in visibleGlobalItems"
          :key="item.id"
          :data-settings-tab="item.id"
          type="button"
          class="md-state md-label-large flex h-9 shrink-0 cursor-pointer items-center gap-2 rounded-full px-3.5 text-left md:h-10 md:gap-2.5 md:px-4"
          :class="
            activeTab === item.id
              ? 'bg-[var(--md-sys-color-secondary-container)] font-semibold text-[var(--md-sys-color-on-secondary-container)]'
              : 'text-[var(--md-sys-color-on-surface-variant)] hover:text-[var(--md-sys-color-on-surface)]'
          "
          @click="selectTab(item.id)"
        >
          <component :is="item.icon" class="h-4 w-4 shrink-0" />
          <span>{{ t(item.labelKey) }}</span>
        </button>

        <!-- Divider between Global and Domain sections -->
        <div
          class="mx-1 hidden h-5 w-px shrink-0 bg-[var(--md-sys-color-outline-variant)]/60 sm:block md:mx-2 md:my-2.5 md:h-px md:w-auto"
          aria-hidden="true"
        />

        <p
          class="md-label-small hidden px-3.5 pt-0.5 pb-1.5 tracking-wide text-[var(--md-sys-color-on-surface-variant)] md:block"
        >
          {{ t('settings.groupDomain') }}
        </p>

        <button
          v-for="item in visibleDomainItems"
          :key="item.id"
          :data-settings-tab="item.id"
          type="button"
          class="md-state md-label-large flex h-9 shrink-0 cursor-pointer items-center gap-2 rounded-full px-3.5 text-left md:h-10 md:gap-2.5 md:px-4"
          :class="
            activeTab === item.id
              ? 'bg-[var(--md-sys-color-secondary-container)] font-semibold text-[var(--md-sys-color-on-secondary-container)]'
              : 'text-[var(--md-sys-color-on-surface-variant)] hover:text-[var(--md-sys-color-on-surface)]'
          "
          @click="selectTab(item.id)"
        >
          <component :is="item.icon" class="h-4 w-4 shrink-0" />
          <span>{{ t(item.labelKey) }}</span>
        </button>
      </nav>

      <!-- Right Content Area -->
      <div class="min-w-0 space-y-6">
        <!-- Domain Switcher Tabs (shown for all lower domain-scoped categories) -->
        <MdTabs
          v-if="showDomainTabs"
          :model-value="activeDomain"
          :tabs="enabledDomainOptions"
          variant="primary"
          :aria-label="t('settings.groupDomain')"
          @update:model-value="selectDomain($event)"
        />

        <div
          v-if="errorMsg"
          class="flex items-center gap-2 rounded-[var(--md-sys-shape-corner-md)] bg-[var(--md-sys-color-error-container)] px-4 py-3 text-xs font-medium text-[var(--md-sys-color-on-error-container)]"
        >
          <LuAlertCircle class="h-4 w-4 shrink-0" />
          <span>{{ errorMsg }}</span>
        </div>

        <!-- Global Sections -->
        <AppearanceSettingsPanel v-if="activeTab === 'appearance'" />

        <SettingsGeneralTab v-else-if="activeTab === 'general'" />

        <template v-else-if="activeTab === 'account'">
          <SettingsAccountTab @error="errorMsg = $event" />
          <SettingsUsersTab v-if="authStore.isAdmin" @error="errorMsg = $event" />
        </template>

        <!-- Domain-Scoped Sections -->
        <SettingsDomainGeneralTab
          v-else-if="activeTab === 'domain-general'"
          :domain="activeDomain"
        />

        <SettingsDomainProtocolsTab
          v-else-if="activeTab === 'domain-protocols'"
          :domain="activeDomain"
        />

        <SettingsDomainDataSourcesTab
          v-else-if="activeTab === 'domain-datasources'"
          :domain="activeDomain"
          @error="errorMsg = $event"
        />

        <SettingsLibrariesTab
          v-else-if="activeTab === 'domain-libraries' && authStore.isAdmin"
          :domain="activeDomain"
          @error="errorMsg = $event"
        />
      </div>
    </div>
  </div>
</template>
