<script setup lang="ts">
import { computed, ref } from 'vue'
import {
  LuAlertCircle,
  LuCheckCircle2,
  LuGlobe,
  LuLink2,
  LuPlus,
  LuTrash2,
  LuUpload,
} from 'vue-icons-plus/lu'
import {
  extractApiError,
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
import { useSettingsI18n } from './i18nHelper'

const props = defineProps<{
  sources: OnlineSourceSummaryDto[]
  adapter: OnlineSourceManagerAdapter<OnlineSourceSummaryDto>
}>()

const emit = defineEmits<{
  refresh: []
  enableAdvanced: []
}>()

const { t, tr } = useSettingsI18n()
const dialogStore = useDialogStore()
const snackbar = useSnackbarStore()

const addDialogOpen = ref(false)
const subscriptionUrl = ref('')
const urlImporting = ref(false)
const fileImporting = ref(false)
const dialogError = ref('')
const busyIds = ref<Set<string>>(new Set())
const fileInputRef = ref<HTMLInputElement | null>(null)

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
      label: tr('settings.sourceHealthOk', '正常', 'Healthy'),
      dotClass: 'bg-success',
      pillClass: 'bg-success-container text-on-success-container',
    }
  }
  if (src.last_test_ok === false) {
    return {
      label: tr('settings.sourceHealthFailing', '异常', 'Failing'),
      dotClass: 'bg-error',
      pillClass: 'bg-error-container text-on-error-container',
    }
  }
  return {
    label: tr('settings.sourceHealthUntested', '未检测', 'Untested'),
    dotClass: 'bg-outline',
    pillClass: 'bg-surface-container-high text-on-surface-variant',
  }
}

function openAddDialog(): void {
  dialogError.value = ''
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
  fileInputRef.value?.click()
}

async function handleFileSelected(ev: Event): Promise<void> {
  const input = ev.target as HTMLInputElement
  const file = input.files?.[0]
  input.value = ''
  if (!file || fileImporting.value) return
  dialogError.value = ''
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

  let parsed: URL
  try {
    parsed = new URL(raw)
    if (parsed.protocol !== 'http:' && parsed.protocol !== 'https:') {
      throw new Error('protocol')
    }
  } catch {
    dialogError.value = tr(
      'settings.sourceUrlInvalid',
      '请输入有效的 http:// 或 https:// 订阅地址',
      'Enter a valid http:// or https:// subscription URL',
    )
    return
  }

  urlImporting.value = true
  try {
    const res = await fetch(parsed.toString())
    if (!res.ok) {
      throw new Error(`HTTP ${res.status} ${res.statusText}`.trim())
    }
    const text = await res.text()
    const report = await props.adapter.importSources(text)
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
    const detail = extractApiError(err)
    dialogError.value = tr(
      'settings.sourceUrlImportFailed',
      '无法从该地址获取 JSON（可能受跨域限制）：{detail}。请下载文件后使用本地文件导入，或开启高级模式粘贴 JSON。',
      'Could not fetch JSON from URL (may be blocked by CORS): {detail}. Download the file to import locally or turn on Advanced mode to paste JSON.',
      { detail },
    )
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
  <MdCard variant="outlined" padding="none" class="overflow-hidden">
    <!-- Section Header -->
    <div class="flex flex-wrap items-center justify-between gap-3 border-b border-outline-variant/60 px-5 py-4">
      <div class="flex items-center gap-2.5">
        <h3 class="text-title-medium text-on-surface">
          {{ tr('settings.onlineSourcesHeading', '在线源', 'Online Sources') }}
        </h3>
        <span
          class="text-label-small rounded-corner-full bg-surface-container-high px-2.5 py-0.5 tabular-nums text-on-surface-variant"
        >
          {{ enabledCount }} / {{ sources.length }}
        </span>
      </div>

      <div class="flex items-center gap-2">
        <RefreshButton size="sm" :action="() => emit('refresh')" />
        <MdButton variant="filled" size="sm" :icon="LuPlus" @click="openAddDialog">
          {{ tr('settings.addSourceBtn', '添加源', 'Add Source') }}
        </MdButton>
      </div>
    </div>

    <!-- Empty State -->
    <MdEmptyState
      v-if="sources.length === 0"
      compact
      :icon="LuGlobe"
      :title="t('onlineSources.panel.emptyTitle', { domain: domainLabel })"
      :description="
        tr(
          'settings.onlineSourcesEmptyBasic',
          '通过订阅链接或本地 JSON 文件添加在线源。',
          'Add online sources from a subscription URL or local JSON file.',
        )
      "
    >
      <template #action>
        <MdButton variant="filled" size="sm" :icon="LuPlus" @click="openAddDialog">
          {{ tr('settings.addSourceBtn', '添加源', 'Add Source') }}
        </MdButton>
      </template>
    </MdEmptyState>

    <!-- Clean Consumer List -->
    <ul v-else class="divide-y divide-outline-variant/50">
      <li
        v-for="src in sources"
        :key="src.id"
        data-source-row
        class="flex items-center justify-between gap-3 px-5 py-3.5 transition-colors hover:bg-surface-container-low"
        :class="src.enabled ? '' : 'opacity-65'"
      >
        <div class="min-w-0 flex-1">
          <div class="flex flex-wrap items-center gap-2">
            <span class="text-body-large truncate text-on-surface" :title="src.name">
              {{ src.name }}
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

  <!-- Hidden File Input -->
  <input
    ref="fileInputRef"
    type="file"
    accept=".json,application/json"
    class="hidden"
    @change="handleFileSelected"
  />

  <!-- Add Source Dialog (Local File / Subscription URL) -->
  <MdDialog
    :open="addDialogOpen"
    :title="tr('settings.addSourceDialogTitle', '添加在线源', 'Add Online Source')"
    :subtitle="
      tr(
        'settings.addSourceDialogSubtitle',
        '选择一种方式添加到「{domain}」',
        'Choose how to add sources to {domain}',
        { domain: t(`app.domains.${adapter.domain}`) },
      )
    "
    size="sm"
    @close="!isAddBusy && (addDialogOpen = false)"
  >
    <div class="flex flex-col gap-4 py-1">
      <div
        v-if="dialogError"
        role="alert"
        class="text-body-small flex items-start gap-2 rounded-corner-md bg-error-container px-3.5 py-2.5 text-on-error-container"
      >
        <LuAlertCircle class="mt-0.5 h-4 w-4 shrink-0" />
        <span class="break-words">{{ dialogError }}</span>
      </div>

      <!-- 1. Local JSON File -->
      <div class="flex items-center justify-between gap-3 rounded-corner-md bg-surface-container p-4">
        <div class="min-w-0">
          <p class="text-title-small flex items-center gap-1.5 text-on-surface">
            <LuUpload class="h-4 w-4 text-primary" />
            <span>{{ tr('settings.fileImportTitle', '本地 JSON 文件', 'Local JSON File') }}</span>
          </p>
          <p class="text-body-small mt-0.5 text-on-surface-variant">
            {{
              tr(
                'settings.fileImportDesc',
                '从电脑选择 .json 源配置文件直接导入。',
                'Select a .json source file from your device.',
              )
            }}
          </p>
        </div>
        <MdButton
          variant="outlined"
          size="sm"
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
            <span>{{ tr('settings.urlImportTitle', '从 URL 订阅导入', 'Import from URL') }}</span>
          </p>
          <p class="text-body-small mt-0.5 text-on-surface-variant">
            {{
              tr(
                'settings.urlImportDesc',
                '输入返回源 JSON 的公开链接（http / https）。',
                'Enter a public http/https link returning source JSON.',
              )
            }}
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
            size="sm"
            :loading="urlImporting"
            :disabled="isAddBusy || !subscriptionUrl.trim()"
            class="mt-0.5"
          >
            {{ tr('settings.fetchAndAddBtn', '拉取', 'Fetch') }}
          </MdButton>
        </div>
      </form>

      <!-- Hint to switch to Advanced mode -->
      <div class="flex items-center justify-between gap-2 pt-1 text-on-surface-variant">
        <span class="text-body-small flex items-center gap-1.5">
          <LuCheckCircle2 class="h-3.5 w-3.5 shrink-0" />
          <span>
            {{
              tr(
                'settings.needAdvancedHint',
                '需要粘贴 JSON、编辑规则或调试？',
                'Need to paste JSON, edit rules, or debug?',
              )
            }}
          </span>
        </span>
        <MdButton variant="text" size="sm" @click="switchToAdvanced">
          {{ tr('settings.openAdvancedLink', '开启高级模式', 'Turn on Advanced') }}
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
