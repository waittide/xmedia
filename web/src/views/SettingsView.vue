<script setup lang="ts">
import { computed, ref, watch, type Component } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useI18n } from 'vue-i18n'
import {
  LuAlertCircle,
  LuKeyRound,
  LuLibrary,
  LuPalette,
  LuRadio,
  LuUsers,
} from 'vue-icons-plus/lu'
import { useAuthStore } from '@/stores/auth'
import { useDomainStore } from '@/stores/domain'
import AppearanceSettingsPanel from '@/components/AppearanceSettingsPanel.vue'
import LayoutCustomizerModal from '@/components/LayoutCustomizerModal.vue'
import SettingsPreferencesTab from '@/components/settings/SettingsPreferencesTab.vue'
import SettingsAccountTab from '@/components/settings/SettingsAccountTab.vue'
import SettingsLibrariesTab from '@/components/settings/SettingsLibrariesTab.vue'
import SettingsUsersTab from '@/components/settings/SettingsUsersTab.vue'

type SettingsTab = 'appearance' | 'preferences' | 'account' | 'libraries' | 'users'

interface SettingsNavItem {
  id: SettingsTab
  labelKey: string
  icon: Component
  adminOnly?: boolean
}

const ALL_NAV_ITEMS: readonly SettingsNavItem[] = [
  { id: 'appearance', labelKey: 'settings.navAppearance', icon: LuPalette },
  { id: 'preferences', labelKey: 'settings.navPreferences', icon: LuRadio },
  { id: 'account', labelKey: 'settings.navAccount', icon: LuKeyRound },
  { id: 'libraries', labelKey: 'settings.navLibraries', icon: LuLibrary, adminOnly: true },
  { id: 'users', labelKey: 'settings.navUsers', icon: LuUsers, adminOnly: true },
]

const { t } = useI18n()
const route = useRoute()
const router = useRouter()
const authStore = useAuthStore()
const domainStore = useDomainStore()

const visibleNavItems = computed(() =>
  ALL_NAV_ITEMS.filter((item) => !item.adminOnly || authStore.isAdmin),
)

function resolveTabFromQuery(): SettingsTab {
  const q = route.query.tab
  if (q === 'preferences' || q === 'reader' || q === 'playback' || q === 'opds') {
    return 'preferences'
  }
  if (q === 'account' || q === 'profile') return 'account'
  if (authStore.isAdmin && (q === 'libraries' || q === 'users')) return q
  return 'appearance'
}

const activeTab = ref<SettingsTab>(resolveTabFromQuery())
const errorMsg = ref('')

watch(
  () => route.query.tab,
  () => {
    activeTab.value = resolveTabFromQuery()
  },
)

function selectTab(tab: SettingsTab): void {
  activeTab.value = tab
  errorMsg.value = ''
  void router.replace({ path: '/settings', query: { tab } })
}
</script>

<template>
  <div class="mx-auto max-w-5xl px-6 py-8 sm:px-8">
    <div class="mb-8">
      <h1 class="md-headline-small text-[var(--md-sys-color-on-surface)]">
        {{ t('settings.title') }}
      </h1>
      <p
        v-if="t('settings.subtitle')"
        class="md-body-small mt-1 text-[var(--md-sys-color-on-surface-variant)]"
      >
        {{ t('settings.subtitle') }}
      </p>
    </div>

    <div class="grid grid-cols-1 gap-6 lg:grid-cols-[220px_minmax(0,1fr)]">
      <!-- Left Settings Category Rail (MD3 Full-Pill Active Indicator) -->
      <nav class="flex gap-1 overflow-x-auto lg:flex-col">
        <template v-for="(item, idx) in visibleNavItems" :key="item.id">
          <div
            v-if="item.adminOnly && idx > 0 && !visibleNavItems[idx - 1]?.adminOnly"
            class="my-2 hidden h-px bg-[var(--md-sys-color-outline-variant)]/60 lg:block"
          />
          <button
            type="button"
            class="md-state md-label-large flex h-10 shrink-0 cursor-pointer items-center gap-2.5 rounded-full px-4 text-left"
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
        </template>
      </nav>

      <!-- Right Content Area -->
      <div class="min-w-0 space-y-6">
        <div
          v-if="errorMsg"
          class="flex items-center gap-2 rounded-[var(--md-sys-shape-corner-md)] bg-[var(--md-sys-color-error-container)] px-4 py-3 text-xs font-medium text-[var(--md-sys-color-on-error-container)]"
        >
          <LuAlertCircle class="h-4 w-4 shrink-0" />
          <span>{{ errorMsg }}</span>
        </div>

        <template v-if="activeTab === 'appearance'">
          <AppearanceSettingsPanel />

          <section class="md-card-outlined p-6">
            <h2 class="md-title-small text-[var(--md-sys-color-on-surface)]">
              {{ t('layoutCustomizer.title') }}
            </h2>
            <p
              v-if="t('layoutCustomizer.subtitle')"
              class="md-body-small mt-1 mb-4 text-[var(--md-sys-color-on-surface-variant)]"
            >
              {{ t('layoutCustomizer.subtitle') }}
            </p>
            <LayoutCustomizerModal :domain="domainStore.activeDomain" inline />
          </section>
        </template>

        <SettingsPreferencesTab v-else-if="activeTab === 'preferences'" />

        <SettingsAccountTab
          v-else-if="activeTab === 'account'"
          @error="errorMsg = $event"
        />

        <SettingsLibrariesTab
          v-else-if="activeTab === 'libraries' && authStore.isAdmin"
          @error="errorMsg = $event"
        />

        <SettingsUsersTab
          v-else-if="activeTab === 'users' && authStore.isAdmin"
          @error="errorMsg = $event"
        />
      </div>
    </div>
  </div>
</template>
