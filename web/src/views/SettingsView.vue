<script setup lang="ts">
import { ref, watch } from 'vue'
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

const { t } = useI18n()
const route = useRoute()
const router = useRouter()
const authStore = useAuthStore()
const domainStore = useDomainStore()

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
      <h1 class="text-2xl font-bold tracking-tight">{{ t('settings.title') }}</h1>
      <p class="mt-1 text-xs text-[var(--ctp-subtext0)]">{{ t('settings.subtitle') }}</p>
    </div>

    <div class="grid grid-cols-1 gap-6 lg:grid-cols-[210px_1fr]">
      <!-- Left Settings Category Rail -->
      <nav class="flex gap-1.5 overflow-x-auto lg:flex-col">
        <button
          type="button"
          class="flex shrink-0 items-center gap-2.5 rounded-xl px-3.5 py-2.5 text-left text-xs font-semibold transition"
          :class="
            activeTab === 'appearance'
              ? 'bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]'
              : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-shell)]'
          "
          @click="selectTab('appearance')"
        >
          <LuPalette class="h-4 w-4" />
          <span>{{ t('settings.tabAppearance') }}</span>
        </button>

        <button
          type="button"
          class="flex shrink-0 items-center gap-2.5 rounded-xl px-3.5 py-2.5 text-left text-xs font-semibold transition"
          :class="
            activeTab === 'preferences'
              ? 'bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]'
              : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-shell)]'
          "
          @click="selectTab('preferences')"
        >
          <LuRadio class="h-4 w-4" />
          <span>{{ t('settings.tabPreferences') }}</span>
        </button>

        <button
          type="button"
          class="flex shrink-0 items-center gap-2.5 rounded-xl px-3.5 py-2.5 text-left text-xs font-semibold transition"
          :class="
            activeTab === 'account'
              ? 'bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]'
              : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-shell)]'
          "
          @click="selectTab('account')"
        >
          <LuKeyRound class="h-4 w-4" />
          <span>{{ t('settings.tabAccount') }}</span>
        </button>

        <template v-if="authStore.isAdmin">
          <div class="my-2 hidden border-t border-[var(--ctp-surface0)]/60 lg:block" />

          <button
            type="button"
            class="flex shrink-0 items-center gap-2.5 rounded-xl px-3.5 py-2.5 text-left text-xs font-semibold transition"
            :class="
              activeTab === 'libraries'
                ? 'bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]'
                : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-shell)]'
            "
            @click="selectTab('libraries')"
          >
            <LuLibrary class="h-4 w-4" />
            <span>{{ t('settings.tabLibraries') }}</span>
          </button>

          <button
            type="button"
            class="flex shrink-0 items-center gap-2.5 rounded-xl px-3.5 py-2.5 text-left text-xs font-semibold transition"
            :class="
              activeTab === 'users'
                ? 'bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]'
                : 'text-[var(--ctp-subtext1)] hover:bg-[var(--ctp-shell)]'
            "
            @click="selectTab('users')"
          >
            <LuUsers class="h-4 w-4" />
            <span>{{ t('settings.tabUsers') }}</span>
          </button>
        </template>
      </nav>

      <!-- Right Content Area -->
      <div class="space-y-6">
        <div
          v-if="errorMsg"
          class="flex items-center gap-2 rounded-xl border border-[var(--ctp-red)]/40 bg-[var(--ctp-red)]/10 px-4 py-3 text-xs text-[var(--ctp-red)]"
        >
          <LuAlertCircle class="h-4 w-4 shrink-0" />
          <span>{{ errorMsg }}</span>
        </div>

        <template v-if="activeTab === 'appearance'">
          <AppearanceSettingsPanel />

          <section class="card-soft rounded-2xl bg-[var(--ctp-shell)] p-6">
            <h2 class="text-sm font-bold">{{ t('layout.settingsSectionTitle') }}</h2>
            <p class="mt-1 mb-4 text-xs text-[var(--ctp-subtext0)]">
              {{ t('layout.settingsSectionDesc') }}
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
