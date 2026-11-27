<script setup lang="ts">
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  LuAlertCircle,
  LuCheckCircle2,
  LuGlobe,
  LuKeyRound,
  LuLink2,
  LuPlus,
  LuTrash2,
  LuUpload,
} from 'vue-icons-plus/lu'
import OnlineSourceLoginModal from '@/components/online-sources/OnlineSourceLoginModal.vue'
import {
  extractApiError,
  hasSourceAuthOrSettings,
  type OnlineSourceManagerAdapter,
  type OnlineSourceSummaryDto,
} from '@/components/online-sources/types'
import {
  MdButton,
  MdCard,
  MdDialog,
  MdEmptyState,
  MdIconButton,
  MdSwitch,
  MdTextField,
  RefreshButton,
} from '@/components/ui'
import { useDialogStore } from '@/stores/dialog'
import { useSnackbarStore } from '@/stores/snackbar'

const props = defineProps<{
  sources: OnlineSourceSummaryDto[]
  adapter: OnlineSourceManagerAdapter<OnlineSourceSummaryDto>
}>()

const emit = defineEmits<{
  refresh: []
  enableAdvanced: []
}>()

const { t } = useI18n()
const dialogStore = useDialogStore()
const snackbar = useSnackbarStore()

const addDialogOpen = ref(false)
const subscriptionUrl = ref('')
const urlImporting = ref(false)
const fileImporting = ref(false)
const dialogError = ref('')
const urlFetchBlocked = ref(false)
const busyIds = ref<Set<string>>(new Set())
const fileInputRef = ref<HTMLInputElement | null>(null)
const loginModalSource = ref<OnlineSourceSummaryDto | null>(null)

function handleSourceUpdated(updated: OnlineSourceSummaryDto): void {
  if (loginModalSource.value?.id === updated.id) {
    loginModalSource.value = updated
  }
  emit('refresh')
}

const domainLabel = computed(() => t(`onlineSources.domainLabel.${props.adapter.domain}`))
const enabledCount = computed(() => props.sources.filter((s) => s.enabled).length)
const isAddBusy = computed(() => urlImporting.value || fileImporting.value)

function isBusy(id: string): boolean {
  return busyIds.value.has(id)
}

function setBusy(id: string, busy: boolean): void {
  const next = new Set(busyIds.value)
  if (busy) next.add(id)
  else next.delete(id)
  busyIds.value = next
}

function formatHost(rawUrl: string): string {
  try {
    const parsed = new URL(rawUrl)
    return parsed.host || rawUrl
  } catch {
    return rawUrl
  }
}

function healthInfo(src: OnlineSourceSummaryDto): {
  label: string
  dotClass: string
  pillClass: string
} {
  if (src.last_test_ok === true) {
    return {
      label: t('settings.sourceHealthOk'),
      dotClass: 'bg-success',
      pillClass: 'bg-success-container text-on-success-container',
    }
  }
  if (src.last_test_ok === false) {
    return {
      label: t('settings.sourceHealthFailing'),
      dotClass: 'bg-error',
      pillClass: 'bg-error-container text-on-error-container',
    }
  }
  return {
    label: t('settings.sourceHealthUntested'),
    dotClass: 'bg-outline',
    pillClass: 'bg-surface-container-high text-on-surface-variant',
  }
}

function openAddDialog(): void {
  dialogError.value = ''
  urlFetchBlocked.value = false
  subscriptionUrl.value = ''
  addDialogOpen.value = true
}

async function handleToggleEnabled(src: OnlineSourceSummaryDto, nextVal: boolean): Promise<void> {
  if (isBusy(src.id)) return
  setBusy(src.id, true)
  try {
    await props.adapter.updateSource(src.id, { enabled: nextVal })
    emit('refresh')
  } catch (err) {
    snackbar.show({ message: extractApiError(err), tone: 'error' })
  } finally {
    setBusy(src.id, false)
  }
}

async function handleDelete(src: OnlineSourceSummaryDto): Promise<void> {
  const ok = await dialogStore.confirm({
    title: t('onlineSources.messages.deleteTitle', { domain: domainLabel.value }),
    message: t('onlineSources.messages.deleteMsg', {
      domain: domainLabel.value,
      name: src.name,
    }),
    confirmText: t('common.delete'),
    cancelText: t('common.cancel'),
    danger: true,
  })
  if (!ok || isBusy(src.id)) return
  setBusy(src.id, true)
  try {
    await props.adapter.deleteSource(src.id)
    snackbar.show({
      message: t('onlineSources.messages.deleted', {
        domain: domainLabel.value,
        name: src.name,
      }),
    })
    emit('refresh')
  } catch (err) {
    snackbar.show({ message: extractApiError(err), tone: 'error' })
  } finally {
    setBusy(src.id, false)
  }
}

function triggerFilePicker(): void {
  dialogError.value = ''
  urlFetchBlocked.value = false
  fileInputRef.value?.click()
}

async function handleFileSelected(ev: Event): Promise<void> {
  const input = ev.target as HTMLInputElement
  const file = input.files?.[0]
  input.value = ''
  if (!file || fileImporting.value) return
  dialogError.value = ''
  urlFetchBlocked.value = false
  fileImporting.value = true
  try {
    const text = await file.text()
    const report = await props.adapter.importSources(text)
    snackbar.show({
      message: t('onlineSources.messages.importSuccess', {
        domain: domainLabel.value,
        imported: report.imported_count,
        updated: report.updated_count,
      }),
    })
    addDialogOpen.value = false
    emit('refresh')
  } catch (err) {
    dialogError.value = extractApiError(err)
  } finally {
    fileImporting.value = false
  }
}

async function handleImportFromUrl(): Promise<void> {
  const raw = subscriptionUrl.value.trim()
  if (!raw || urlImporting.value) return
  dialogError.value = ''
  urlFetchBlocked.value = false

  let parsed: URL
  try {
    parsed = new URL(raw)
    if (parsed.protocol !== 'http:' && parsed.protocol !== 'https:') {
      throw new Error('protocol')
    }
  } catch {
    dialogError.value = t('settings.sourceUrlInvalid')
    return
  }

  urlImporting.value = true
  const controller = new AbortController()
  const timeoutId = setTimeout(() => controller.abort(), 15_000)

  let text = ''
  try {
    const res = await fetch(parsed.toString(), {
      method: 'GET',
      mode: 'cors',
      credentials: 'omit',
      referrerPolicy: 'no-referrer',
      headers: { Accept: 'application/json, text/plain, */*' },
      signal: controller.signal,
    })
    if (!res.ok) {
      throw new Error(`HTTP ${res.status} ${res.statusText}`.trim())
    }
    text = await res.text()
  } catch (err) {
    const isAbort = err instanceof DOMException && err.name === 'AbortError'
    const detail = isAbort ? 'Timeout (15s)' : extractApiError(err)
    urlFetchBlocked.value = true
    dialogError.value = t('settings.sourceUrlImportFailed', { detail })
    urlImporting.value = false
    clearTimeout(timeoutId)
    return
  } finally {
    clearTimeout(timeoutId)
  }

  try {
    const trimmed = text.trim()
    if (!trimmed) {
      throw new Error(t('onlineSources.edit.jsonEmptyError'))
    }
    const report = await props.adapter.importSources(trimmed)
    snackbar.show({
      message: t('onlineSources.messages.importSuccess', {
        domain: domainLabel.value,
        imported: report.imported_count,
        updated: report.updated_count,
      }),
    })
    subscriptionUrl.value = ''
    addDialogOpen.value = false
    emit('refresh')
  } catch (err) {
    dialogError.value = extractApiError(err)
  } finally {
    urlImporting.value = false
  }
}

function switchToAdvanced(): void {
  addDialogOpen.value = false
  emit('enableAdvanced')
}
</script>

<template>
  <MdCard variant="tonal" padding="none" class="overflow-hidden">
    <!-- Section Header -->
    <div class="flex flex-wrap items-center justify-between gap-3 px-5 pt-4 pb-2">
      <div class="flex items-center gap-2.5">
        <h3 class="text-title-medium text-on-surface">
          {{ t('settings.onlineSourcesHeading') }}
        </h3>
        <span
          class="text-label-small rounded-corner-full bg-surface-container-high px-2.5 py-0.5 tabular-nums text-on-surface-variant"
        >
          {{ enabledCount }} / {{ sources.length }}
        </span>
      </div>

      <div class="flex items-center gap-2">
        <RefreshButton :action="() => emit('refresh')" />
        <MdButton variant="filled" :icon="LuPlus" @click="openAddDialog">
          {{ t('settings.addSourceBtn') }}
        </MdButton>
      </div>
    </div>

    <!-- Empty State -->
    <MdEmptyState
      v-if="sources.length === 0"
      compact
      :icon="LuGlobe"
      :title="t('onlineSources.panel.emptyTitle', { domain: domainLabel })"
      :description="t('settings.onlineSourcesEmptyBasic')"
    >
      <template #action>
        <MdButton variant="filled" :icon="LuPlus" @click="openAddDialog">
          {{ t('settings.addSourceBtn') }}
        </MdButton>
      </template>
    </MdEmptyState>

    <!-- Clean Consumer List -->
    <ul v-else class="divide-y divide-on-surface/6">
      <li
        v-for="src in sources"
        :key="src.id"
        data-source-row
        class="flex items-center justify-between gap-3 px-5 py-3.5 transition-colors hover:bg-surface-container"
        :class="src.enabled ? '' : 'opacity-65'"
      >
        <div class="min-w-0 flex-1">
          <div class="flex flex-wrap items-center gap-2">
            <span class="text-body-large truncate text-on-surface" :title="src.name">
              {{ src.name }}
            </span>
            <span
              v-if="src.format === 'js_module'"
              class="text-label-small shrink-0 rounded-corner-full bg-primary-container px-2 py-0.5 font-mono text-on-primary-container"
            >
              {{ src.version ? `JS v${src.version}` : 'JS' }}
            </span>
            <span
              class="text-label-small inline-flex shrink-0 items-center gap-1.5 rounded-corner-full px-2 py-0.5"
              :class="healthInfo(src).pillClass"
              :title="src.last_test_msg || healthInfo(src).label"
            >
              <span class="h-1.5 w-1.5 rounded-corner-full" :class="healthInfo(src).dotClass" />
              <span>{{ healthInfo(src).label }}</span>
            </span>
          </div>
          <p class="text-body-small mt-0.5 truncate font-mono text-on-surface-variant">
            {{ formatHost(src.source_url) }}
          </p>
        </div>

        <div class="flex shrink-0 items-center gap-1.5">
          <MdIconButton
            v-if="hasSourceAuthOrSettings(src)"
            size="sm"
            :icon="LuKeyRound"
            :disabled="isBusy(src.id)"
            :tooltip="t('onlineSources.menu.login')"
            :aria-label="t('onlineSources.menu.login')"
            @click="loginModalSource = src"
          />
          <MdSwitch
            :model-value="src.enabled"
            :disabled="isBusy(src.id)"
            :aria-label="
              src.enabled
                ? t('onlineSources.card.disableAction', { domain: domainLabel })
                : t('onlineSources.card.enableAction', { domain: domainLabel })
            "
            @update:model-value="handleToggleEnabled(src, $event)"
          />
          <MdIconButton
            size="sm"
            :icon="LuTrash2"
            :disabled="isBusy(src.id)"
            :tooltip="t('common.delete')"
            :aria-label="t('common.delete')"
            class="hover:text-error"
            @click="handleDelete(src)"
          />
        </div>
      </li>
    </ul>
  </MdCard>

  <OnlineSourceLoginModal
    :open="loginModalSource !== null"
    :domain-label="domainLabel"
    :source="loginModalSource"
    :get-source-variable="adapter.getSourceVariable"
    :run-login-action="adapter.runLoginAction"
    :run-plugin-auth="adapter.runPluginAuth"
    :update-plugin-settings="adapter.updatePluginSettings"
    :update-source="adapter.updateSource"
    :build-proxy-image-url="adapter.buildProxyImageUrl"
    @close="loginModalSource = null"
    @updated="handleSourceUpdated"
  />

  <!-- Hidden File Input -->
  <input
    ref="fileInputRef"
    type="file"
    accept=".json,.js,.mjs,application/json,text/javascript"
    class="hidden"
    @change="handleFileSelected"
  />

  <!-- Add Source Dialog (Local File / Subscription URL) -->
  <MdDialog
    :open="addDialogOpen"
    :title="t('settings.addSourceDialogTitle')"
    :subtitle="
      t('settings.addSourceDialogSubtitle', {
        domain: t(`app.domains.${adapter.domain}`),
      })
    "
    size="sm"
    @close="!isAddBusy && (addDialogOpen = false)"
  >
    <div class="flex flex-col gap-4 py-1">
      <div
        v-if="dialogError"
        role="alert"
        class="text-body-small flex flex-col gap-2 rounded-corner-md bg-error-container px-3.5 py-2.5 text-on-error-container"
      >
        <div class="flex items-start gap-2">
          <LuAlertCircle class="mt-0.5 h-4 w-4 shrink-0" aria-hidden="true" />
          <span class="break-words">{{ dialogError }}</span>
        </div>
        <div v-if="urlFetchBlocked" class="flex flex-wrap items-center gap-2 pl-6">
          <MdButton variant="tonal" :disabled="isAddBusy" @click="triggerFilePicker">
            {{ t('onlineSources.toolbar.uploadFileBtn') }}
          </MdButton>
          <MdButton variant="text" :disabled="isAddBusy" @click="switchToAdvanced">
            {{ t('settings.openAdvancedLink') }}
          </MdButton>
        </div>
      </div>

      <!-- 1. Local JSON File -->
      <div class="flex items-center justify-between gap-3 rounded-corner-md bg-surface-container p-4">
        <div class="min-w-0">
          <p class="text-title-small flex items-center gap-1.5 text-on-surface">
            <LuUpload class="h-4 w-4 text-primary" />
            <span>{{ t('settings.fileImportTitle') }}</span>
          </p>
          <p class="text-body-small mt-0.5 text-on-surface-variant">
            {{ t('settings.fileImportDesc') }}
          </p>
        </div>
        <MdButton
          variant="outlined"
          :loading="fileImporting"
          :disabled="isAddBusy"
          @click="triggerFilePicker"
        >
          {{ t('onlineSources.toolbar.uploadFileBtn') }}
        </MdButton>
      </div>

      <!-- 2. URL Subscription -->
      <form
        class="flex flex-col gap-3 rounded-corner-md bg-surface-container p-4"
        @submit.prevent="handleImportFromUrl"
      >
        <div>
          <p class="text-title-small flex items-center gap-1.5 text-on-surface">
            <LuLink2 class="h-4 w-4 text-primary" />
            <span>{{ t('settings.urlImportTitle') }}</span>
          </p>
          <p class="text-body-small mt-0.5 text-on-surface-variant">
            {{ t('settings.urlImportDesc') }}
          </p>
        </div>

        <div class="flex items-start gap-2">
          <MdTextField
            v-model="subscriptionUrl"
            type="url"
            inputmode="url"
            size="sm"
            placeholder="https://example.com/sources.json"
            :disabled="isAddBusy"
            class="min-w-0 flex-1"
          />
          <MdButton
            type="submit"
            variant="filled"
            :loading="urlImporting"
            :disabled="isAddBusy || !subscriptionUrl.trim()"
          >
            {{ t('settings.fetchAndAddBtn') }}
          </MdButton>
        </div>
      </form>

      <!-- Hint to switch to Advanced mode -->
      <div class="flex items-center justify-between gap-2 pt-1 text-on-surface-variant">
        <span class="text-body-small flex items-center gap-1.5">
          <LuCheckCircle2 class="h-3.5 w-3.5 shrink-0" />
          <span>
            {{ t('settings.needAdvancedHint') }}
          </span>
        </span>
        <MdButton variant="text" @click="switchToAdvanced">
          {{ t('settings.openAdvancedLink') }}
        </MdButton>
      </div>
    </div>

    <template #footer>
      <MdButton variant="text" :disabled="isAddBusy" @click="addDialogOpen = false">
        {{ t('common.close') }}
      </MdButton>
    </template>
  </MdDialog>
</template>
