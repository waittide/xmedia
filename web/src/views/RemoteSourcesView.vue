<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import {
  LuAlertCircle,
  LuBookOpen,
  LuCheckCircle2,
  LuCloud,
  LuDisc3,
  LuExternalLink,
  LuLoader2,
  LuPencil,
  LuPlus,
  LuShieldAlert,
  LuTrash2,
  LuWifi,
} from 'vue-icons-plus/lu'
import {
  appHostApi,
  type RemoteProtocol,
  type RemoteSourceSummaryDto,
  type TestRemoteSourceResponse,
} from '@/api/appHost'
import RemoteSourceFormDialog from '@/components/remote/RemoteSourceFormDialog.vue'
import { MdButton, MdCard, MdEmptyState, MdIconButton } from '@/components/ui'
import { useDelayedLoading } from '@/composables/useDelayedLoading'
import { resolveLibraryRoute, useSettingsI18n } from '@/components/settings/i18nHelper'
import { useAppClientStore } from '@/stores/appServers'
import { useDialogStore } from '@/stores/dialog'
import { useSnackbarStore } from '@/stores/snackbar'
import type { MediaKind } from '@/types/api'

const props = withDefaults(
  defineProps<{
    embeddedProtocol?: RemoteProtocol
    domain?: MediaKind
  }>(),
  {
    embeddedProtocol: undefined,
    domain: undefined,
  },
)

const { t, tr } = useSettingsI18n()
const router = useRouter()
const appClientStore = useAppClientStore()
const dialogStore = useDialogStore()
const snackbarStore = useSnackbarStore()

const initLoading = ref(false)
const showInitSpinner = useDelayedLoading(initLoading)
const formDialogOpen = ref(false)
const editingSource = ref<RemoteSourceSummaryDto | null>(null)
const formDefaultProtocol = ref<RemoteProtocol>(props.embeddedProtocol ?? 'opds')

const testingSourceId = ref<string | null>(null)
const deletingSourceId = ref<string | null>(null)
const cardTestResults = ref<Record<string, TestRemoteSourceResponse>>({})

const managedSources = computed<RemoteSourceSummaryDto[]>(() => {
  if (props.embeddedProtocol === 'opds') return appClientStore.opdsSources
  if (props.embeddedProtocol === 'subsonic') return appClientStore.subsonicSources
  return appClientStore.remoteSources
})

const sectionTitle = computed(() => {
  if (props.embeddedProtocol === 'opds') return t('settings.remoteOpdsTitle')
  if (props.embeddedProtocol === 'subsonic') return t('settings.remoteSubsonicTitle')
  return tr('settings.remoteServersTitle', '远程服务器', 'Remote Servers')
})

function resolveTargetDomain(source: RemoteSourceSummaryDto): MediaKind {
  if (source.protocol === 'subsonic') return 'music'
  if (props.domain === 'comic') return 'comic'
  return 'book'
}

function openInLibrary(source: RemoteSourceSummaryDto): void {
  const targetDomain = resolveTargetDomain(source)
  void router.push(
    resolveLibraryRoute(router, targetDomain, {
      scope: 'remote',
      sourceId: source.id,
    }),
  )
}

function openAddSourceDialog(protocol: RemoteProtocol = props.embeddedProtocol ?? 'opds'): void {
  editingSource.value = null
  formDefaultProtocol.value = protocol
  formDialogOpen.value = true
}

function openEditSourceDialog(source: RemoteSourceSummaryDto): void {
  editingSource.value = source
  formDefaultProtocol.value = source.protocol
  formDialogOpen.value = true
}

async function handleTestSavedSource(source: RemoteSourceSummaryDto): Promise<TestRemoteSourceResponse> {
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
        source_id: source.id,
        protocol: source.protocol,
        url: source.url,
        username: source.username || undefined,
        allow_insecure_tls: source.allow_insecure_tls,
      })
    }
    cardTestResults.value = { ...cardTestResults.value, [source.id]: result }
    return result
  } catch (err) {
    const failed: TestRemoteSourceResponse = {
      ok: false,
      latency_ms: Math.max(1, Math.round(performance.now() - startedAt)),
      protocol: source.protocol,
      error: err instanceof Error ? err.message : String(err),
    }
    cardTestResults.value = { ...cardTestResults.value, [source.id]: failed }
    return failed
  } finally {
    if (testingSourceId.value === source.id) {
      testingSourceId.value = null
    }
  }
}

async function handleSourceSaved(source: RemoteSourceSummaryDto): Promise<void> {
  const result = await handleTestSavedSource(source)
  if (result.ok) {
    snackbarStore.show({
      message: tr(
        'settings.remoteSavedAvailableInLibrary',
        '连接正常，已出现在资源来源中',
        'Connected — now available in Resource sources',
      ),
      actionLabel: tr('settings.openInLibraryAction', '前往查看', 'Open Resources'),
      onAction: () => openInLibrary(source),
    })
  } else {
    snackbarStore.show({
      message: tr(
        'settings.remoteSavedTestWarning',
        '已保存服务器，但连接测试未通过：{error}',
        'Server saved, but connection test failed: {error}',
        { error: result.error || t('remote.testFailed') },
      ),
      tone: 'error',
    })
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
    const nextResults = { ...cardTestResults.value }
    delete nextResults[source.id]
    cardTestResults.value = nextResults
    snackbarStore.show(t('remote.deletedMsg'))
  } catch (err) {
    snackbarStore.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  } finally {
    deletingSourceId.value = null
  }
}

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
    initLoading.value = false
  }
})
</script>

<template>
  <MdCard variant="outlined" padding="none" class="overflow-hidden">
    <div class="flex flex-wrap items-center justify-between gap-3 border-b border-outline-variant/60 px-5 py-4">
      <div class="min-w-0">
        <div class="flex items-center gap-2">
          <LuCloud class="h-4 w-4 shrink-0 text-primary" />
          <h3 class="text-title-medium text-on-surface">
            {{ sectionTitle }}
          </h3>
          <span
            class="text-label-small rounded-corner-full bg-surface-container-high px-2.5 py-0.5 tabular-nums text-on-surface-variant"
          >
            {{ managedSources.length }}
          </span>
        </div>
        <p class="text-body-small mt-0.5 text-on-surface-variant">
          {{
            tr(
              'settings.remoteServersHint',
              '添加后的 OPDS / OpenSubsonic 服务器会自动出现在对应域的资源来源筛选中。',
              'Configured OPDS / OpenSubsonic servers appear automatically in the domain resource source switcher.',
            )
          }}
        </p>
      </div>

      <MdButton
        variant="tonal"
        size="sm"
        :icon="LuPlus"
        @click="openAddSourceDialog(embeddedProtocol ?? 'opds')"
      >
        {{ t('remote.addSource') }}
      </MdButton>
    </div>

    <div
      v-if="initLoading"
      class="text-body-small flex items-center justify-center gap-2 py-12 text-on-surface-variant"
    >
      <template v-if="showInitSpinner">
        <LuLoader2 class="h-4 w-4 animate-spin" />
        <span>{{ t('common.loading') }}</span>
      </template>
    </div>

    <MdEmptyState
      v-else-if="managedSources.length === 0"
      compact
      :icon="embeddedProtocol === 'subsonic' ? LuDisc3 : LuBookOpen"
      :title="
        embeddedProtocol === 'subsonic'
          ? t('remote.subsonicEmptySourcesTitle')
          : t('remote.opdsEmptySourcesTitle')
      "
      :description="
        embeddedProtocol === 'subsonic'
          ? t('remote.subsonicEmptySourcesHint')
          : t('remote.opdsEmptySourcesHint')
      "
    >
      <template #action>
        <MdButton
          variant="filled"
          size="sm"
          :icon="embeddedProtocol === 'subsonic' ? LuDisc3 : LuBookOpen"
          @click="openAddSourceDialog(embeddedProtocol ?? 'opds')"
        >
          {{
            embeddedProtocol === 'subsonic'
              ? t('remote.subsonicAddBtn')
              : t('remote.opdsAddBtn')
          }}
        </MdButton>
      </template>
    </MdEmptyState>

    <ul v-else class="divide-y divide-outline-variant/50">
      <li
        v-for="source in managedSources"
        :key="source.id"
        class="flex flex-col gap-3 px-5 py-4 sm:flex-row sm:items-center sm:justify-between"
      >
        <div class="min-w-0 flex-1 space-y-1">
          <div class="flex flex-wrap items-center gap-2">
            <span
              class="text-label-small inline-flex shrink-0 items-center gap-1 rounded-corner-full px-2.5 py-0.5"
              :class="
                source.protocol === 'opds'
                  ? 'bg-primary-container text-on-primary-container'
                  : 'bg-tertiary-container text-on-tertiary-container'
              "
            >
              <LuBookOpen v-if="source.protocol === 'opds'" class="h-3 w-3" />
              <LuDisc3 v-else class="h-3 w-3" />
              <span>{{ source.protocol === 'opds' ? 'OPDS' : 'OpenSubsonic' }}</span>
            </span>

            <span class="text-body-large truncate text-on-surface">
              {{ source.name || t('remote.unnamed') }}
            </span>

            <span
              v-if="source.allow_insecure_tls"
              class="inline-flex shrink-0 items-center text-on-surface-variant"
              :title="t('remote.insecureBadge')"
            >
              <LuShieldAlert class="h-3.5 w-3.5" />
            </span>
          </div>

          <p class="text-body-small truncate font-mono text-on-surface-variant">
            {{ source.url }}
          </p>

          <div
            v-if="source.username || source.has_password"
            class="text-label-small flex flex-wrap items-center gap-1.5 text-on-surface-variant"
          >
            <span v-if="source.username" class="rounded-corner-xs bg-surface-container-high px-2 py-0.5 font-mono">
              @{{ source.username }}
            </span>
            <span v-if="source.has_password" class="rounded-corner-xs bg-surface-container-high px-2 py-0.5">
              {{ t('remote.passwordSavedBadge') }}
            </span>
          </div>

          <!-- Inline connection test feedback -->
          <div
            v-if="cardTestResults[source.id]"
            class="text-label-medium mt-1.5 inline-flex max-w-full items-center gap-1.5 rounded-corner-sm px-2.5 py-1"
            :class="
              cardTestResults[source.id]!.ok
                ? 'bg-success-container text-on-success-container'
                : 'bg-error-container text-on-error-container'
            "
          >
            <LuCheckCircle2 v-if="cardTestResults[source.id]!.ok" class="h-3.5 w-3.5 shrink-0" />
            <LuAlertCircle v-else class="h-3.5 w-3.5 shrink-0" />
            <span class="truncate">
              <template v-if="cardTestResults[source.id]!.ok">
                {{ t('remote.testOk', { latency: cardTestResults[source.id]!.latency_ms }) }}
                <span v-if="cardTestResults[source.id]!.detail">
                  · {{ cardTestResults[source.id]!.detail }}
                </span>
              </template>
              <template v-else>
                {{ cardTestResults[source.id]!.error || t('remote.testFailed') }}
              </template>
            </span>
          </div>
        </div>

        <div class="flex shrink-0 items-center gap-1.5">
          <MdButton
            variant="tonal"
            size="sm"
            :icon="LuExternalLink"
            @click="openInLibrary(source)"
          >
            {{ tr('settings.openInLibraryBtn', '在资源中打开', 'Open in Resources') }}
          </MdButton>

          <MdIconButton
            size="sm"
            :icon="LuWifi"
            :loading="testingSourceId === source.id"
            :tooltip="t('remote.testBtn')"
            :aria-label="t('remote.testBtn')"
            @click="handleTestSavedSource(source)"
          />

          <MdIconButton
            size="sm"
            :icon="LuPencil"
            :tooltip="t('remote.editBtn')"
            :aria-label="t('remote.editBtn')"
            @click="openEditSourceDialog(source)"
          />

          <MdIconButton
            size="sm"
            :icon="LuTrash2"
            :loading="deletingSourceId === source.id"
            :tooltip="t('common.delete')"
            :aria-label="t('common.delete')"
            class="hover:text-error"
            @click="handleDeleteSource(source)"
          />
        </div>
      </li>
    </ul>

    <RemoteSourceFormDialog
      :open="formDialogOpen"
      :editing-source="editingSource"
      :default-protocol="formDefaultProtocol"
      :locked-protocol="embeddedProtocol"
      @close="formDialogOpen = false"
      @saved="handleSourceSaved"
    />
  </MdCard>
</template>
