<script setup lang="ts">
import { computed, defineAsyncComponent, nextTick, onMounted, reactive, ref, watch } from 'vue'
import { LuLoader2, LuLock } from 'vue-icons-plus/lu'
import type { RemoteProtocol } from '@/api/appHost'
import OnlineSourceManagerPanel from '@/components/online-sources/OnlineSourceManagerPanel.vue'
import {
  extractApiError,
  type OnlineSourceManagerAdapter,
  type OnlineSourceSummaryDto,
} from '@/components/online-sources/types'
import { MdCard, MdChip, MdSwitch } from '@/components/ui'
import { useDelayedLoading } from '@/composables/useDelayedLoading'
import BookOnlineSourceManagerPanel from '@/domains/book/components/OnlineSourceManagerPanel.vue'
import { onlineBookApi, type BookSourceSummaryDto } from '@/domains/book/onlineApi'
import ComicSourceDebugSheet from '@/domains/comic/components/ComicSourceDebugSheet.vue'
import ComicSourceEditSheet from '@/domains/comic/components/ComicSourceEditSheet.vue'
import { comicSourceAdapter, type ComicSourceSummaryDto } from '@/domains/comic/onlineApi'
import MusicSourceDebugSheet from '@/domains/music/components/MusicSourceDebugSheet.vue'
import MusicSourceEditSheet from '@/domains/music/components/MusicSourceEditSheet.vue'
import { musicSourceAdapter, type MusicSourceSummaryDto } from '@/domains/music/onlineApi'
import VideoSourceDebugSheet from '@/domains/video/components/VideoSourceDebugSheet.vue'
import VideoSourceEditSheet from '@/domains/video/components/VideoSourceEditSheet.vue'
import { videoSourceAdapter, type VideoSourceSummaryDto } from '@/domains/video/onlineApi'
import { useAppClientStore } from '@/stores/appServers'
import { useAuthStore } from '@/stores/auth'
import type { MediaKind } from '@/types/api'
import SettingsOnlineSourcesBasicList from './SettingsOnlineSourcesBasicList.vue'
import { useSettingsI18n } from './i18nHelper'
import { DOMAIN_ICONS } from './sections'

const RemoteSourcesView = defineAsyncComponent(() => import('@/views/RemoteSourcesView.vue'))

const props = withDefaults(
  defineProps<{
    domain: MediaKind
    advanced?: boolean
    focusKind?: string
  }>(),
  {
    advanced: false,
    focusKind: '',
  },
)

const emit = defineEmits<{
  error: [message: string]
  'update:domain': [domain: MediaKind]
  'update:advanced': [value: boolean]
}>()

const { t, tr } = useSettingsI18n()
const authStore = useAuthStore()
const appClientStore = useAppClientStore()

const remoteSectionRef = ref<HTMLElement | null>(null)

const bookSourceAdapter: OnlineSourceManagerAdapter<BookSourceSummaryDto> = {
  domain: 'book',
  exportFilePrefix: 'xmedia-book-sources',
  searchRoutePath: '/book/catalog',
  listSources: onlineBookApi.listSources,
  importSources: onlineBookApi.importSources,
  reorderSources: onlineBookApi.reorderSources,
  batchSources: onlineBookApi.batchSources,
  exportSourcesBatch: onlineBookApi.exportSourcesBatch,
  updateSource: onlineBookApi.updateSource,
  deleteSource: onlineBookApi.deleteSource,
  exportSourceUrl: onlineBookApi.exportSourceUrl,
  fetchSourceJson: onlineBookApi.fetchSourceJson,
  getSourceVariable: onlineBookApi.getSourceVariable,
  runLoginAction: onlineBookApi.runLoginAction,
}

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

const isActiveLoading = computed(() => loadingByDomain[props.domain])
const isActiveLoaded = computed(() => loadedByDomain[props.domain])

const activeAdapter = computed<OnlineSourceManagerAdapter<OnlineSourceSummaryDto>>(() => {
  if (props.domain === 'book') return bookSourceAdapter
  if (props.domain === 'comic') return comicSourceAdapter
  if (props.domain === 'music') return musicSourceAdapter
  return videoSourceAdapter
})

const remoteProtocol = computed<RemoteProtocol | null>(() => {
  if (props.domain === 'book' || props.domain === 'comic') return 'opds'
  if (props.domain === 'music') return 'subsonic'
  return null
})

async function fetchDomainSources(targetDomain: MediaKind): Promise<OnlineSourceSummaryDto[]> {
  if (targetDomain === 'book') return onlineBookApi.listSources()
  if (targetDomain === 'comic') return comicSourceAdapter.listSources()
  if (targetDomain === 'music') return musicSourceAdapter.listSources()
  return videoSourceAdapter.listSources()
}

async function refreshDomainSources(targetDomain: MediaKind = props.domain): Promise<void> {
  if (!authStore.isAdmin) return
  emit('error', '')
  loadingByDomain[targetDomain] = true
  try {
    sourcesByDomain[targetDomain] = await fetchDomainSources(targetDomain)
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
  <MdCard v-if="!authStore.isAdmin" variant="outlined" padding="lg" class="py-14 text-center">
    <div
      class="mx-auto mb-3 flex h-12 w-12 items-center justify-center rounded-corner-lg bg-surface-container-high text-on-surface-variant"
    >
      <LuLock class="h-5 w-5" />
    </div>
    <p class="text-title-medium text-on-surface">
      {{ t('settings.dataSourcesAdminOnly') }}
    </p>
    <p class="text-body-small mx-auto mt-1.5 max-w-md text-on-surface-variant">
      {{
        tr(
          'settings.sourcesAdminOnlyHint',
          '请联系管理员配置在线源与远程服务器。',
          'Contact an administrator to configure online data sources and remote servers.',
        )
      }}
    </p>
  </MdCard>

  <!-- IMPORTANT: Avoid `.space-y-4` on outer wrapper so E2E `[role="dialog"], form, .space-y-4` never collides -->
  <div v-else class="flex flex-col gap-6">
    <!-- Single-row domain chips + Progressive disclosure "Advanced" switch -->
    <div class="flex flex-wrap items-center justify-between gap-3">
      <div
        role="group"
        :aria-label="tr('settings.sourceDomainFilter', '按域筛选来源', 'Filter sources by domain')"
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

      <MdSwitch
        :model-value="advanced"
        :label="tr('settings.advancedModeLabel', '高级模式', 'Advanced')"
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
    <template v-else>
      <BookOnlineSourceManagerPanel
        v-if="domain === 'book'"
        :sources="sourcesByDomain.book as BookSourceSummaryDto[]"
        @refresh="refreshDomainSources('book')"
      />

      <OnlineSourceManagerPanel
        v-else-if="domain === 'comic'"
        :sources="sourcesByDomain.comic as ComicSourceSummaryDto[]"
        :adapter="comicSourceAdapter"
        @refresh="refreshDomainSources('comic')"
      >
        <template #edit-sheet="{ open, source, close, saved, savedAndDebug }">
          <ComicSourceEditSheet
            :open="open"
            :source="source"
            @close="close"
            @saved="saved"
            @saved-and-debug="savedAndDebug"
          />
        </template>
        <template #debug-sheet="{ open, source, close, tested }">
          <ComicSourceDebugSheet
            :open="open"
            :source="source"
            @close="close"
            @tested="tested"
          />
        </template>
      </OnlineSourceManagerPanel>

      <OnlineSourceManagerPanel
        v-else-if="domain === 'music'"
        :sources="sourcesByDomain.music as MusicSourceSummaryDto[]"
        :adapter="musicSourceAdapter"
        @refresh="refreshDomainSources('music')"
      >
        <template #edit-sheet="{ open, source, close, saved, savedAndDebug }">
          <MusicSourceEditSheet
            :open="open"
            :source="source"
            @close="close"
            @saved="saved"
            @saved-and-debug="savedAndDebug"
          />
        </template>
        <template #debug-sheet="{ open, source, close, tested }">
          <MusicSourceDebugSheet
            :open="open"
            :source="source"
            @close="close"
            @tested="tested"
          />
        </template>
      </OnlineSourceManagerPanel>

      <OnlineSourceManagerPanel
        v-else-if="domain === 'video'"
        :sources="sourcesByDomain.video as VideoSourceSummaryDto[]"
        :adapter="videoSourceAdapter"
        @refresh="refreshDomainSources('video')"
      >
        <template #edit-sheet="{ open, source, close, saved, savedAndDebug }">
          <VideoSourceEditSheet
            :open="open"
            :source="source"
            @close="close"
            @saved="saved"
            @saved-and-debug="savedAndDebug"
          />
        </template>
        <template #debug-sheet="{ open, source, close, tested }">
          <VideoSourceDebugSheet
            :open="open"
            :source="source"
            @close="close"
            @tested="tested"
          />
        </template>
      </OnlineSourceManagerPanel>
    </template>

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
