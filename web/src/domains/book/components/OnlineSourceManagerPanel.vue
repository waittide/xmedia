<script setup lang="ts">
import { ref } from 'vue'
import {
  LuCheckCircle2,
  LuDownload,
  LuFlaskConical,
  LuKeyRound,
  LuLoader2,
  LuPlus,
  LuTrash2,
  LuUpload,
  LuXCircle,
} from 'vue-icons-plus/lu'
import AppSwitch from '@/components/ui/AppSwitch.vue'
import { useDialogStore } from '@/stores/dialog'
import { useSnackbarStore } from '@/stores/snackbar'
import { onlineBookApi, type BookSourceSummaryDto } from '../onlineApi'
import OnlineSourceDebugSheet from './OnlineSourceDebugSheet.vue'
import OnlineSourceLoginModal from './OnlineSourceLoginModal.vue'

const props = defineProps<{
  sources: BookSourceSummaryDto[]
}>()

const emit = defineEmits<{
  refresh: []
}>()

const snackbar = useSnackbarStore()
const dialogStore = useDialogStore()

const showImportBox = ref(false)
const importJsonText = ref('')
const importing = ref(false)
const importError = ref<string | null>(null)
const fileInputRef = ref<HTMLInputElement | null>(null)

const busyId = ref<string | null>(null)
const loginModalSource = ref<BookSourceSummaryDto | null>(null)
const debugSheetSource = ref<BookSourceSummaryDto | null>(null)

function triggerFileImport(): void {
  fileInputRef.value?.click()
}

async function handleFileSelected(ev: Event): Promise<void> {
  const input = ev.target as HTMLInputElement
  const file = input.files?.[0]
  input.value = ''
  if (!file) return
  try {
    const text = await file.text()
    importJsonText.value = text
    showImportBox.value = true
    await handleImportSubmit()
  } catch (err) {
    importError.value = err instanceof Error ? err.message : String(err)
  }
}

async function handleImportSubmit(): Promise<void> {
  if (!importJsonText.value.trim() || importing.value) return
  importing.value = true
  importError.value = null
  try {
    const report = await onlineBookApi.importSources(importJsonText.value)
    snackbar.show({
      message: `书源导入完成：新增 ${report.imported_count} 条，更新 ${report.updated_count} 条`,
    })
    importJsonText.value = ''
    showImportBox.value = false
    emit('refresh')
  } catch (err) {
    importError.value = err instanceof Error ? err.message : String(err)
  } finally {
    importing.value = false
  }
}

async function handleToggleEnabled(src: BookSourceSummaryDto, nextVal: boolean): Promise<void> {
  busyId.value = src.id
  try {
    await onlineBookApi.updateSource(src.id, { enabled: nextVal })
    emit('refresh')
  } catch (err) {
    snackbar.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  } finally {
    busyId.value = null
  }
}

async function handleDeleteSource(src: BookSourceSummaryDto): Promise<void> {
  const ok = await dialogStore.confirm({
    title: '删除书源',
    message: `确定删除书源「${src.name}」吗？该书源的本地章节缓存也将一并清理。`,
    confirmText: '删除',
    cancelText: '取消',
    danger: true,
  })
  if (!ok) return
  busyId.value = src.id
  try {
    await onlineBookApi.deleteSource(src.id)
    snackbar.show({ message: `已删除书源「${src.name}」` })
    emit('refresh')
  } catch (err) {
    snackbar.show({
      message: err instanceof Error ? err.message : String(err),
      tone: 'error',
    })
  } finally {
    busyId.value = null
  }
}

function handleSourceUpdated(updated: BookSourceSummaryDto): void {
  loginModalSource.value = updated
  emit('refresh')
}
</script>

<template>
  <div class="space-y-4">
    <!-- Top Actions -->
    <div class="flex flex-wrap items-center justify-between gap-2">
      <p class="text-xs text-[var(--md-sys-color-on-surface-variant)]">
        导入 Legado JSON 书源
      </p>

      <div class="flex items-center gap-2">
        <input
          ref="fileInputRef"
          type="file"
          accept=".json,application/json"
          class="hidden"
          @change="handleFileSelected"
        />
        <button
          type="button"
          class="md-btn md-btn-sm md-btn-tonal md-state"
          @click="triggerFileImport"
        >
          <LuUpload class="h-3.5 w-3.5" />
          <span>上传 JSON 文件</span>
        </button>
        <button
          type="button"
          class="md-btn md-btn-sm md-btn-filled md-state"
          @click="showImportBox = !showImportBox"
        >
          <LuPlus class="h-3.5 w-3.5" />
          <span>粘贴导入书源</span>
        </button>
      </div>
    </div>

    <!-- Paste Import Box -->
    <div
      v-if="showImportBox"
      class="md-card-filled space-y-3 p-4"
    >
      <div
        v-if="importError"
        class="rounded-xl bg-[var(--md-sys-color-error-container)] px-3 py-2 text-xs text-[var(--md-sys-color-on-error-container)]"
      >
        {{ importError }}
      </div>
      <textarea
        v-model="importJsonText"
        rows="7"
        placeholder="粘贴单条或多条 Legado 书源 JSON（[...] 或 {...}）…"
        class="md-input w-full py-2.5 font-mono text-xs"
      />
      <div class="flex items-center justify-end gap-2">
        <button
          type="button"
          class="md-btn md-btn-sm md-btn-text md-state"
          @click="showImportBox = false"
        >
          取消
        </button>
        <button
          type="button"
          :disabled="!importJsonText.trim() || importing"
          class="md-btn md-btn-sm md-btn-filled md-state"
          @click="handleImportSubmit"
        >
          <LuLoader2 v-if="importing" class="h-3.5 w-3.5 animate-spin" />
          <span>{{ importing ? '导入中…' : '确认导入' }}</span>
        </button>
      </div>
    </div>

    <!-- Empty State -->
    <div
      v-if="props.sources.length === 0"
      class="rounded-[24px] bg-[var(--md-sys-color-surface-container-low)] py-14 text-center"
    >
      <p class="text-sm font-semibold text-[var(--md-sys-color-on-surface)]">尚未导入任何书源</p>
      <p class="mt-1 text-xs text-[var(--md-sys-color-on-surface-variant)]">
        上传或粘贴 Legado JSON 导入书源
      </p>
    </div>

    <!-- Sources List -->
    <div v-else class="space-y-2.5">
      <div
        v-for="src in props.sources"
        :key="src.id"
        class="md-card-filled space-y-2.5 p-4 transition"
        :class="!src.enabled ? 'opacity-65' : ''"
      >
        <div class="flex flex-wrap items-center justify-between gap-3">
          <div class="min-w-0 flex-1">
            <div class="flex flex-wrap items-center gap-2">
              <span class="text-sm font-semibold text-[var(--md-sys-color-on-surface)]">
                {{ src.name }}
              </span>
              <span
                v-if="src.group_name"
                class="rounded-full bg-[var(--md-sys-color-surface-container-high)] px-2 py-0.5 text-[10px] font-medium text-[var(--md-sys-color-on-surface-variant)]"
              >
                {{ src.group_name }}
              </span>
              <span
                v-if="src.cookie_hosts.length > 0"
                class="rounded-full bg-[var(--md-sys-color-success-container)] px-2 py-0.5 text-[10px] font-medium text-[var(--md-sys-color-on-success-container)]"
              >
                已存 Cookie ({{ src.cookie_hosts.length }})
              </span>
              <span
                v-if="src.last_test_ok === true"
                :title="src.last_test_msg || '测试通过'"
                class="inline-flex max-w-xs items-center gap-1 rounded-full bg-[var(--md-sys-color-success-container)] px-2 py-0.5 text-[10px] font-medium text-[var(--md-sys-color-on-success-container)]"
              >
                <LuCheckCircle2 class="h-3 w-3 shrink-0" />
                <span class="truncate">{{ src.last_test_msg || '测试通过' }}</span>
              </span>
              <span
                v-else-if="src.last_test_ok === false"
                :title="src.last_test_msg || '测试失败'"
                class="inline-flex max-w-xs items-center gap-1 rounded-full bg-[var(--md-sys-color-error-container)] px-2 py-0.5 text-[10px] font-medium text-[var(--md-sys-color-on-error-container)]"
              >
                <LuXCircle class="h-3 w-3 shrink-0" />
                <span class="truncate">{{ src.last_test_msg || '测试失败' }}</span>
              </span>
            </div>
            <p class="mt-0.5 truncate font-mono text-[11px] text-[var(--md-sys-color-on-surface-variant)]">
              {{ src.source_url }}
            </p>
            <p
              v-if="src.comment"
              class="mt-1 line-clamp-2 text-[11px] text-[var(--md-sys-color-on-surface-variant)]"
            >
              {{ src.comment }}
            </p>
          </div>

          <!-- Controls -->
          <div class="flex shrink-0 flex-wrap items-center gap-1.5">
            <AppSwitch
              :model-value="src.enabled"
              :disabled="busyId === src.id"
              :title="src.enabled ? '禁用书源' : '启用书源'"
              @update:model-value="(val) => handleToggleEnabled(src, val)"
            />

            <button
              v-if="src.has_login_url || src.login_ui.length > 0"
              type="button"
              class="md-btn md-btn-sm md-btn-tonal md-state"
              title="打开书源登录面板"
              @click="loginModalSource = src"
            >
              <LuKeyRound class="h-3.5 w-3.5" />
              <span>登录</span>
            </button>

            <button
              type="button"
              class="md-btn md-btn-sm md-btn-outlined md-state"
              title="打开书源规则调试器，指定关键词测试各条规则"
              @click="debugSheetSource = src"
            >
              <LuFlaskConical class="h-3.5 w-3.5" />
              <span>测试</span>
            </button>

            <a
              :href="onlineBookApi.exportSourceUrl(src.id)"
              :download="`${src.name}.json`"
              class="md-icon-btn md-icon-btn-sm md-state"
              title="导出书源 JSON"
            >
              <LuDownload class="h-4 w-4" />
            </a>

            <button
              type="button"
              :disabled="busyId === src.id"
              class="md-icon-btn md-icon-btn-sm md-state !text-[var(--md-sys-color-error)] hover:bg-[var(--md-sys-color-error-container)]/50"
              title="删除书源"
              @click="handleDeleteSource(src)"
            >
              <LuTrash2 class="h-4 w-4" />
            </button>
          </div>
        </div>
      </div>
    </div>

    <OnlineSourceLoginModal
      :open="loginModalSource !== null"
      :source="loginModalSource"
      @close="loginModalSource = null"
      @updated="handleSourceUpdated"
    />

    <OnlineSourceDebugSheet
      :open="debugSheetSource !== null"
      :source="debugSheetSource"
      @close="debugSheetSource = null"
      @tested="emit('refresh')"
    />
  </div>
</template>
