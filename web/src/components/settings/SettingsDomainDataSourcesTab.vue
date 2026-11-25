<script setup lang="ts">
import {
  computed,
  defineAsyncComponent,
  nextTick,
  onMounted,
  reactive,
  ref,
  watch,
  type Component,
} from 'vue'
import { useI18n } from 'vue-i18n'
import { LuLoader2, LuLock } from 'vue-icons-plus/lu'
import OnlineSourceManagerPanel from '@/components/online-sources/OnlineSourceManagerPanel.vue'
import {
  extractApiError,
  type OnlineSourceSummaryDto,
} from '@/components/online-sources/types'
import { MdCard, MdChip, MdSwitch } from '@/components/ui'
import { useDelayedLoading } from '@/composables/useDelayedLoading'
import { getDomainPlugin } from '@/sources'
import { useAppClientStore } from '@/stores/appServers'
import { useAuthStore } from '@/stores/auth'
import type { MediaKind } from '@/types/api'
import SettingsOnlineSourcesBasicList from './SettingsOnlineSourcesBasicList.vue'
import { DOMAIN_ICONS } from './sections'

const RemoteSourcesView = defineAsyncComponent(() => import('@/views/RemoteSourcesView.vue'))

const props = withDefaults(
  defineProps<{
    domain: MediaKind
    advanced?: boolean
    focusKind?: string
    hideDomainSelector?: boolean
  }>(),
  {
    advanced: false,
    focusKind: '',
    hideDomainSelector: false,
  },
)

const emit = defineEmits<{
  error: [message: string]
  'update:domain': [domain: MediaKind]
  'update:advanced': [value: boolean]
}>()

const { t } = useI18n()
const authStore = useAuthStore()
const appClientStore = useAppClientStore()

const remoteSectionRef = ref<HTMLElement | null>(null)
const asyncEditSheetCache = new Map<MediaKind, Component>()
const asyncDebugSheetCache = new Map<MediaKind, Component>()

const sourcesByDomain = reactive<Record<MediaKind, OnlineSourceSummaryDto[]>>({
  book: [],
  comic: [],
  music: [],
  video: [],
})

const loadingByDomain = reactive<Record<MediaKind, boolean>>({
  book: false,
  comic: false,
  music: false,
  video: false,
})

const loadedByDomain = reactive<Record<MediaKind, boolean>>({
  book: false,
  comic: false,
  music: false,
  video: false,
})

const activePlugin = computed(() => getDomainPlugin(props.domain))
const isActiveLoading = computed(() => loadingByDomain[props.domain])
const isActiveLoaded = computed(() => loadedByDomain[props.domain])
const activeAdapter = computed(() => activePlugin.value.onlineSources.getAdapter())
const remoteProtocol = computed(() => activePlugin.value.onlineSources.remoteProtocol)

const activeEditSheet = computed<Component>(() => {
  const kind = props.domain
  let cached = asyncEditSheetCache.get(kind)
  if (!cached) {
    cached = defineAsyncComponent(getDomainPlugin(kind).onlineSources.loadEditSheet)
    asyncEditSheetCache.set(kind, cached)
  }
  return cached
})

const activeDebugSheet = computed<Component>(() => {
  const kind = props.domain
  let cached = asyncDebugSheetCache.get(kind)
  if (!cached) {
    cached = defineAsyncComponent(getDomainPlugin(kind).onlineSources.loadDebugSheet)
    asyncDebugSheetCache.set(kind, cached)
  }
  return cached
})

async function refreshDomainSources(targetDomain: MediaKind = props.domain): Promise<void> {
  if (!authStore.isAdmin) return
  emit('error', '')
  loadingByDomain[targetDomain] = true
  try {
    sourcesByDomain[targetDomain] = await getDomainPlugin(targetDomain)
      .onlineSources.getAdapter()
      .listSources()
    loadedByDomain[targetDomain] = true
  } catch (err) {
    emit('error', extractApiError(err))
  } finally {
    loadingByDomain[targetDomain] = false
  }
}

function scrollToRemoteIfNeeded(): void {
  if (props.focusKind !== 'remote' || !appClientStore.isStandaloneApp) return
  void nextTick(() => {
    remoteSectionRef.value?.scrollIntoView({ behavior: 'smooth', block: 'start' })
  })
}

onMounted(() => {
  if (authStore.isAdmin) {
    void refreshDomainSources(props.domain)
  }
  scrollToRemoteIfNeeded()
})

watch(
  () => props.domain,
  (next) => {
    if (authStore.isAdmin && !loadedByDomain[next] && !loadingByDomain[next]) {
      void refreshDomainSources(next)
    }
  },
)

const showLoadingSpinner = useDelayedLoading(() => isActiveLoading.value && !isActiveLoaded.value)

watch(
  () => props.focusKind,
  () => {
    scrollToRemoteIfNeeded()
  },
)
</script>

<template>
  <!-- Admin-only lock card across ALL domains (T2-F12-01: must include 管理员/admin and list 0 sources) -->
  <MdCard v-if="!authStore.isAdmin" variant="tonal" padding="lg" class="py-14 text-center">
    <div
      class="mx-auto mb-3 flex h-12 w-12 items-center justify-center rounded-corner-lg bg-surface-container-high text-on-surface-variant"
    >
      <LuLock class="h-5 w-5" />
    </div>
    <p class="text-title-medium text-on-surface">
      {{ t('settings.dataSourcesAdminOnly') }}
    </p>
    <p class="text-body-small mx-auto mt-1.5 max-w-md text-on-surface-variant">
      {{ t('settings.sourcesAdminOnlyHint') }}
    </p>
  </MdCard>

  <!-- IMPORTANT: Avoid `.space-y-4` on outer wrapper so E2E `[role="dialog"], form, .space-y-4` never collides -->
  <div v-else class="flex flex-col gap-6">
    <!-- Single-row domain chips + Progressive disclosure "Advanced" switch -->
    <div class="flex flex-wrap items-center justify-between gap-3">
      <div
        v-if="!hideDomainSelector"
        role="group"
        :aria-label="t('settings.sourceDomainFilter')"
        class="no-scrollbar flex flex-nowrap items-center gap-2 overflow-x-auto py-0.5"
      >
        <MdChip
          v-for="kind in authStore.enabledModules"
          :key="kind"
          variant="filter"
          hide-check
          :icon="DOMAIN_ICONS[kind]"
          :label="t(`app.domains.${kind}`)"
          :selected="domain === kind"
          @click="emit('update:domain', kind)"
        />
      </div>
      <div v-else class="min-w-0">
        <h2 class="text-title-medium text-on-surface">
          {{ t('settings.navDomainDataSources') }}
        </h2>
        <p class="text-body-small text-on-surface-variant">
          {{ t('settings.summarySources') }}
        </p>
      </div>

      <MdSwitch
        :model-value="advanced"
        :label="t('settings.advancedModeLabel')"
        @update:model-value="emit('update:advanced', $event)"
      />
    </div>

    <!-- Group 1: Online Sources -->
    <div
      v-if="isActiveLoading && !isActiveLoaded"
      class="text-body-small flex items-center justify-center gap-2 py-16 text-on-surface-variant"
    >
      <template v-if="showLoadingSpinner">
        <LuLoader2 class="h-4 w-4 animate-spin" />
        <span>{{ t('common.loading') }}</span>
      </template>
    </div>

    <!-- Basic Mode (Progressive Disclosure Default) -->
    <SettingsOnlineSourcesBasicList
      v-else-if="!advanced"
      :sources="sourcesByDomain[domain]"
      :adapter="activeAdapter"
      @refresh="refreshDomainSources(domain)"
      @enable-advanced="emit('update:advanced', true)"
    />

    <!-- Advanced Mode: Full Rule Authoring / Debug / Paste JSON / Batch / Login Variables -->
    <OnlineSourceManagerPanel
      v-else
      :key="domain"
      :sources="sourcesByDomain[domain]"
      :adapter="activeAdapter"
      @refresh="refreshDomainSources(domain)"
    >
      <template #edit-sheet="{ open, source, close, saved, savedAndDebug }">
        <component
          :is="activeEditSheet"
          :open="open"
          :source="source"
          @close="close"
          @saved="saved"
          @saved-and-debug="savedAndDebug"
        />
      </template>
      <template #debug-sheet="{ open, source, close, tested }">
        <component
          :is="activeDebugSheet"
          :open="open"
          :source="source"
          @close="close"
          @tested="tested"
        />
      </template>
    </OnlineSourceManagerPanel>

    <!-- Group 2: Remote Servers (Standalone mode only; no embedded browser) -->
    <div
      v-if="appClientStore.isStandaloneApp && remoteProtocol"
      id="remote-servers"
      ref="remoteSectionRef"
      class="scroll-mt-20"
    >
      <RemoteSourcesView :embedded-protocol="remoteProtocol" :domain="domain" />
    </div>
  </div>
</template>
