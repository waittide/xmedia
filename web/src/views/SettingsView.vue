<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useRoute, useRouter } from 'vue-router'
import { LuAlertCircle, LuArrowLeft, LuChevronRight } from 'vue-icons-plus/lu'
import AppearanceSettingsPanel from '@/components/AppearanceSettingsPanel.vue'
import SettingsAccountTab from '@/components/settings/SettingsAccountTab.vue'
import SettingsGeneralTab from '@/components/settings/SettingsGeneralTab.vue'
import SettingsUsersTab from '@/components/settings/SettingsUsersTab.vue'
import {
  hasExplicitSectionInRoute,
  resolveSectionFromRoute,
  SETTINGS_SECTIONS,
  type SettingsSection,
  type SettingsSectionDef,
} from '@/components/settings/sections'
import { MdCard, MdIconButton } from '@/components/ui'
import { useAuthStore } from '@/stores/auth'

const { t } = useI18n()
const route = useRoute()
const router = useRouter()
const authStore = useAuthStore()

const visibleSections = computed<SettingsSectionDef[]>(() =>
  SETTINGS_SECTIONS.filter((item) => !item.adminOnly || authStore.isAdmin),
)

const activeSection = ref<SettingsSection>(resolveSectionFromRoute(route))
const mobileShowDetail = ref<boolean>(hasExplicitSectionInRoute(route))
const errorMsg = ref('')

const activeSectionDef = computed<SettingsSectionDef>(
  () => SETTINGS_SECTIONS.find((s) => s.id === activeSection.value) ?? SETTINGS_SECTIONS[0]!,
)

function sectionLabel(item: SettingsSectionDef): string {
  return t(item.labelKey)
}

function sectionSummary(item: SettingsSectionDef): string {
  return t(item.summaryKey)
}

function syncRoute(section: SettingsSection): void {
  void router.replace({ path: `/settings/${section}` })
}

function applyRouteState(): void {
  activeSection.value = resolveSectionFromRoute(route)
  mobileShowDetail.value = hasExplicitSectionInRoute(route)
}

onMounted(() => {
  applyRouteState()
})

watch(
  () => [route.params.section, route.query.tab] as const,
  () => {
    applyRouteState()
  },
)

function selectSection(section: SettingsSection): void {
  activeSection.value = section
  mobileShowDetail.value = true
  errorMsg.value = ''
  syncRoute(section)
}

function handleMobileBack(): void {
  mobileShowDetail.value = false
  errorMsg.value = ''
  void router.replace({ path: '/settings' })
}
</script>

<template>
  <div class="px-4 py-5 sm:px-6 sm:py-6">
    <div class="mx-auto w-full max-w-7xl">
      <!-- Page Header (Desktop always visible; Mobile visible on List view) -->
      <div class="mb-6" :class="mobileShowDetail ? 'hidden md:block' : 'block'">
        <h1 class="text-headline-small text-on-surface">
          {{ t('settings.title') }}
        </h1>
      </div>

      <!-- Mobile Detail Top Back Bar (<840px only) -->
      <div
        v-if="mobileShowDetail"
        class="mb-3 flex items-center gap-2 md:hidden"
      >
        <MdIconButton
          size="sm"
          :icon="LuArrowLeft"
          :aria-label="t('settings.backToList')"
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
          <MdCard variant="tonal" padding="none" class="overflow-hidden md:hidden">
            <div class="divide-y divide-on-surface/6">
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

          <!-- 2. Account & Security -->
          <div v-else-if="activeSection === 'account'" class="flex flex-col gap-5">
            <SettingsAccountTab @error="errorMsg = $event" />
            <SettingsUsersTab v-if="authStore.isAdmin" @error="errorMsg = $event" />
          </div>

          <!-- 3. About & System Info -->
          <SettingsGeneralTab v-else-if="activeSection === 'about'" />
        </div>
      </div>
    </div>
  </div>
</template>
