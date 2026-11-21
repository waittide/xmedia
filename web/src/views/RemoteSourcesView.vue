<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { LuBookOpen, LuCloud, LuDisc3, LuLoader2, LuSettings2 } from 'vue-icons-plus/lu'
import {
  appHostApi,
  type RemoteProtocol,
  type RemoteSourceSummaryDto,
  type TestRemoteSourceResponse,
} from '@/api/appHost'
import RemoteOpdsBrowser from '@/components/remote/RemoteOpdsBrowser.vue'
import RemoteSourceFormDialog from '@/components/remote/RemoteSourceFormDialog.vue'
import RemoteSourcesManageList from '@/components/remote/RemoteSourcesManageList.vue'
import RemoteSubsonicBrowser from '@/components/remote/RemoteSubsonicBrowser.vue'
import type { SegmentedOption } from '@/components/ui/MdSegmentedButton.vue'
import MdTabs from '@/components/ui/MdTabs.vue'
import { useAppClientStore } from '@/stores/appServers'
import { useDialogStore } from '@/stores/dialog'
import { useSnackbarStore } from '@/stores/snackbar'

type ActiveTab = 'opds' | 'subsonic' | 'manage'

const props = defineProps<{
  embeddedProtocol: RemoteProtocol
}>()

const { t } = useI18n()
const appClientStore = useAppClientStore()
const dialogStore = useDialogStore()
const snackbarStore = useSnackbarStore()

const activeTab = ref<ActiveTab>(props.embeddedProtocol)
const initLoading = ref(false)

// Source form dialog state
const formDialogOpen = ref(false)
const editingSource = ref<RemoteSourceSummaryDto | null>(null)
const formDefaultProtocol = ref<RemoteProtocol>(props.embeddedProtocol)

// Source management list actions
const testingSourceId = ref<string | null>(null)
const deletingSourceId = ref<string | null>(null)
const cardTestResults = ref<Record<string, TestRemoteSourceResponse>>({})

// Selected source IDs + browser refs
const selectedOpdsId = ref('')
const selectedSubsonicId = ref('')
const opdsBrowserRef = ref<InstanceType<typeof RemoteOpdsBrowser> | null>(null)
const subsonicBrowserRef = ref<InstanceType<typeof RemoteSubsonicBrowser> | null>(null)

const tabOptions = computed<SegmentedOption<ActiveTab>[]>(() => {
  if (props.embeddedProtocol === 'opds') {
    return [
      { value: 'manage', label: t('remote.tabManage'), icon: LuSettings2 },
      { value: 'opds', label: t('remote.browseBtn'), icon: LuBookOpen },
    ]
  }
  return [
    { value: 'manage', label: t('remote.tabManage'), icon: LuSettings2 },
    { value: 'subsonic', label: t('remote.browseBtn'), icon: LuDisc3 },
  ]
})

const managedSources = computed<RemoteSourceSummaryDto[]>(() =>
  props.embeddedProtocol === 'opds' ? appClientStore.opdsSources : appClientStore.subsonicSources,
)

const embeddedHeaderTitle = computed(() =>
  props.embeddedProtocol === 'opds'
    ? t('settings.remoteOpdsTitle')
    : t('settings.remoteSubsonicTitle'),
)

function openAddSourceDialog(protocol: RemoteProtocol = 'opds'): void {
  editingSource.value = null
  formDefaultProtocol.value = protocol
  formDialogOpen.value = true
}

function openEditSourceDialog(source: RemoteSourceSummaryDto): void {
  editingSource.value = source
  formDefaultProtocol.value = source.protocol
  formDialogOpen.value = true
}

function handleSourceSaved(source: RemoteSourceSummaryDto): void {
  const opdsIds = new Set(appClientStore.opdsSources.map((s) => s.id))
  if (selectedOpdsId.value && !opdsIds.has(selectedOpdsId.value)) {
    selectedOpdsId.value = appClientStore.opdsSources[0]?.id ?? ''
    opdsBrowserRef.value?.resetState(false)
  }

  const subsonicIds = new Set(appClientStore.subsonicSources.map((s) => s.id))
  if (selectedSubsonicId.value && !subsonicIds.has(selectedSubsonicId.value)) {
    selectedSubsonicId.value = appClientStore.subsonicSources[0]?.id ?? ''
    subsonicBrowserRef.value?.resetState(false)
  }

  if (source.protocol === 'opds') {
    if (!selectedOpdsId.value || selectedOpdsId.value === source.id) {
      selectedOpdsId.value = source.id
      opdsBrowserRef.value?.resetState(activeTab.value === 'opds', source.id)
    }
  } else if (source.protocol === 'subsonic') {
    if (!selectedSubsonicId.value || selectedSubsonicId.value === source.id) {
      selectedSubsonicId.value = source.id
      subsonicBrowserRef.value?.resetState(activeTab.value === 'subsonic', source.id)
    }
  }
}

function handleBrowseSource(source: RemoteSourceSummaryDto): void {
  if (source.protocol === 'opds') {
    activeTab.value = 'opds'
    if (selectedOpdsId.value !== source.id || !opdsBrowserRef.value?.hasFeed) {
      selectedOpdsId.value = source.id
      opdsBrowserRef.value?.selectSource(source.id)
    }
  } else {
    activeTab.value = 'subsonic'
    if (selectedSubsonicId.value !== source.id || !subsonicBrowserRef.value?.hasAlbums) {
      selectedSubsonicId.value = source.id
      subsonicBrowserRef.value?.selectSource(source.id)
    }
  }
}

async function handleTestSavedSource(source: RemoteSourceSummaryDto): Promise<void> {
  if (testingSourceId.value) return
  testingSourceId.value = source.id
  const startedAt = performance.now()
  try {
    let result: TestRemoteSourceResponse
    if (source.has_password) {
      if (source.protocol === 'opds') {
        const feed = await appHostApi.browseOpds(source.id)
        result = {
          ok: true,
          latency_ms: Math.max(1, Math.round(performance.now() - startedAt)),
          protocol: feed.opds_version || 'OPDS',
          detail: feed.title || feed.opds_version,
        }
      } else {
        const res = await appHostApi.listSubsonicAlbums(source.id, {
          type: 'newest',
          size: 1,
          offset: 0,
        })
        result = {
          ok: true,
          latency_ms: Math.max(1, Math.round(performance.now() - startedAt)),
          protocol: 'OpenSubsonic',
          detail: res.albums.length > 0 ? res.albums[0]!.name : 'OpenSubsonic',
        }
      }
    } else {
      result = await appClientStore.testSource({
        protocol: source.protocol,
        url: source.url,
        username: source.username || undefined,
        allow_insecure_tls: source.allow_insecure_tls,
      })
    }
    cardTestResults.value = { ...cardTestResults.value, [source.id]: result }
  } catch (err) {
    cardTestResults.value = {
      ...cardTestResults.value,
      [source.id]: {
        ok: false,
        latency_ms: Math.max(1, Math.round(performance.now() - startedAt)),
        protocol: source.protocol,
        error: err instanceof Error ? err.message : String(err),
      },
    }
  } finally {
    testingSourceId.value = null
  }
}

async function handleDeleteSource(source: RemoteSourceSummaryDto): Promise<void> {
  const ok = await dialogStore.confirm({
    title: t('remote.deleteTitle'),
    message: t('remote.deleteConfirm', { name: source.name || t('remote.unnamed') }),
    confirmText: t('common.delete'),
    cancelText: t('common.cancel'),
    danger: true,
  })
  if (!ok) return

  deletingSourceId.value = source.id
  try {
    await appClientStore.deleteSource(source.id)
    snackbarStore.show(t('remote.deletedMsg'))

    if (selectedOpdsId.value === source.id) {
      const nextId = appClientStore.opdsSources[0]?.id ?? ''
      selectedOpdsId.value = nextId
      opdsBrowserRef.value?.resetState(Boolean(nextId && activeTab.value === 'opds'), nextId)
    }
    if (selectedSubsonicId.value === source.id) {
      const nextId = appClientStore.subsonicSources[0]?.id ?? ''
      selectedSubsonicId.value = nextId
      subsonicBrowserRef.value?.resetState(
        Boolean(nextId && activeTab.value === 'subsonic'),
        nextId,
      )
    }
  } catch (err) {
    snackbarStore.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  } finally {
    deletingSourceId.value = null
  }
}

watch(activeTab, (tab) => {
  if (tab === 'opds' && !selectedOpdsId.value && appClientStore.opdsSources.length > 0) {
    selectedOpdsId.value = appClientStore.opdsSources[0]!.id
  } else if (
    tab === 'subsonic' &&
    !selectedSubsonicId.value &&
    appClientStore.subsonicSources.length > 0
  ) {
    selectedSubsonicId.value = appClientStore.subsonicSources[0]!.id
  }
})

watch(
  () => props.embeddedProtocol,
  (proto) => {
    if (proto !== 'subsonic') {
      subsonicBrowserRef.value?.closeAllOverlays()
    } else {
      opdsBrowserRef.value?.closeDetail()
    }
    formDefaultProtocol.value = proto
    if (proto === 'opds') {
      activeTab.value = appClientStore.opdsSources.length > 0 ? 'opds' : 'manage'
      if (appClientStore.opdsSources.length > 0 && !selectedOpdsId.value) {
        selectedOpdsId.value = appClientStore.opdsSources[0]!.id
      }
    } else {
      activeTab.value = appClientStore.subsonicSources.length > 0 ? 'subsonic' : 'manage'
      if (appClientStore.subsonicSources.length > 0 && !selectedSubsonicId.value) {
        selectedSubsonicId.value = appClientStore.subsonicSources[0]!.id
      }
    }
  },
)

onMounted(async () => {
  initLoading.value = true
  try {
    await appClientStore.refreshSources()
  } catch (err) {
    snackbarStore.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  } finally {
    if (props.embeddedProtocol === 'opds') {
      activeTab.value = appClientStore.opdsSources.length > 0 ? 'opds' : 'manage'
    } else {
      activeTab.value = appClientStore.subsonicSources.length > 0 ? 'subsonic' : 'manage'
    }

    if (appClientStore.opdsSources.length > 0) {
      selectedOpdsId.value = appClientStore.opdsSources[0]!.id
    }
    if (appClientStore.subsonicSources.length > 0) {
      selectedSubsonicId.value = appClientStore.subsonicSources[0]!.id
    }
    initLoading.value = false
  }
})
</script>

<template>
  <div class="space-y-4">
    <!-- Header + MD3 Tabs -->
    <div class="flex flex-wrap items-center justify-between gap-3">
      <h2 class="md-title-small flex items-center gap-2 text-[var(--md-sys-color-on-surface)]">
        <LuCloud class="h-4 w-4 text-[var(--md-sys-color-primary)]" />
        <span>{{ embeddedHeaderTitle }}</span>
      </h2>

      <MdTabs
        v-model="activeTab"
        :tabs="tabOptions"
        variant="primary"
        :aria-label="embeddedHeaderTitle"
      />
    </div>

    <div
      v-if="initLoading"
      class="flex items-center justify-center py-20 text-xs text-[var(--md-sys-color-on-surface-variant)]"
    >
      <LuLoader2 class="mr-2 h-4 w-4 animate-spin" />
      <span>{{ t('common.loading') }}</span>
    </div>

    <template v-else>
      <RemoteOpdsBrowser
        v-if="embeddedProtocol === 'opds'"
        v-show="activeTab === 'opds'"
        ref="opdsBrowserRef"
        v-model:selected-source-id="selectedOpdsId"
        :active="activeTab === 'opds'"
        @add-source="openAddSourceDialog('opds')"
      />

      <RemoteSourcesManageList
        v-if="activeTab === 'manage'"
        :protocol="embeddedProtocol"
        :sources="managedSources"
        :testing-source-id="testingSourceId"
        :deleting-source-id="deletingSourceId"
        :card-test-results="cardTestResults"
        @add="openAddSourceDialog"
        @browse="handleBrowseSource"
        @test="handleTestSavedSource"
        @edit="openEditSourceDialog"
        @delete="handleDeleteSource"
      />

      <RemoteSubsonicBrowser
        v-if="embeddedProtocol === 'subsonic'"
        ref="subsonicBrowserRef"
        v-model:selected-source-id="selectedSubsonicId"
        :active="activeTab === 'subsonic'"
        @add-source="openAddSourceDialog('subsonic')"
      />
    </template>

    <RemoteSourceFormDialog
      :open="formDialogOpen"
      :editing-source="editingSource"
      :default-protocol="formDefaultProtocol"
      :locked-protocol="embeddedProtocol"
      @close="formDialogOpen = false"
      @saved="handleSourceSaved"
    />
  </div>
</template>
