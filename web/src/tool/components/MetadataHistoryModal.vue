<script setup lang="ts">
import { ref, watch } from 'vue'
import {
  LuAlertCircle,
  LuChevronDown,
  LuChevronUp,
  LuClock,
  LuFileSignature,
  LuLoader2,
  LuPencil,
  LuRotateCcw,
} from 'vue-icons-plus/lu'

import MdSegmentedButton from '@/components/ui/MdSegmentedButton.vue'
import MdSideSheet from '@/components/ui/MdSideSheet.vue'
import { useSnackbarStore } from '@/stores/snackbar'
import { toolApi } from '../api'
import type { MetadataHistoryBatchDto, RenameHistoryBatchDto } from '../types'

const props = defineProps<{
  open: boolean
  rootId?: string | null
}>()

const emit = defineEmits<{
  close: []
  restored: []
}>()

const snackbar = useSnackbarStore()

const activeTab = ref<'metadata' | 'rename'>('metadata')
const scopeAllRoots = ref(false)
const loading = ref(false)
const undoingId = ref<string | null>(null)
const errorMsg = ref<string | null>(null)

const metadataBatches = ref<MetadataHistoryBatchDto[]>([])
const renameBatches = ref<RenameHistoryBatchDto[]>([])
const expandedIds = ref<Set<string>>(new Set())

async function loadHistory(): Promise<void> {
  loading.value = true
  errorMsg.value = null
  const rid = scopeAllRoots.value ? undefined : props.rootId || undefined
  try {
    const [metaRes, renameRes] = await Promise.all([
      toolApi.listMetadataHistory(rid),
      toolApi.listRenameHistory(rid),
    ])
    metadataBatches.value = metaRes
    renameBatches.value = renameRes
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
  } finally {
    loading.value = false
  }
}

watch(
  () => props.open,
  (isOpen) => {
    if (isOpen) {
      errorMsg.value = null
      void loadHistory()
    }
  },
)

watch(scopeAllRoots, () => {
  if (props.open) void loadHistory()
})

function toggleExpand(id: string): void {
  const next = new Set(expandedIds.value)
  if (next.has(id)) next.delete(id)
  else next.add(id)
  expandedIds.value = next
}

function formatUnix(secs: number): string {
  return new Date(secs * 1000).toLocaleString('zh-CN', {
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
  })
}

function domainBadge(kind: string): string {
  switch (kind) {
    case 'book':
      return '电子书'
    case 'comic':
      return '漫画'
    case 'music':
      return '音乐'
    case 'video':
      return '视频'
    default:
      return kind
  }
}

async function handleUndoMetadata(batch: MetadataHistoryBatchDto): Promise<void> {
  if (undoingId.value) return
  undoingId.value = batch.batch_id
  errorMsg.value = null
  try {
    const report = await toolApi.undoMetadataBatch(batch.batch_id)
    await loadHistory()
    emit('restored')
    snackbar.show(
      `已回滚「${batch.label}」：恢复 ${report.restored_count} 个文件${
        report.failed_paths.length > 0 ? `（${report.failed_paths.length} 项失败）` : ''
      }`,
    )
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
  } finally {
    undoingId.value = null
  }
}

async function handleUndoRename(batch: RenameHistoryBatchDto): Promise<void> {
  if (undoingId.value) return
  undoingId.value = batch.batch_id
  errorMsg.value = null
  try {
    const report = await toolApi.undoRenameBatch(batch.batch_id)
    await loadHistory()
    emit('restored')
    snackbar.show(
      `已撤销重命名批次：还原 ${report.restored_count} 个路径${
        report.skipped_conflicts.length > 0 ? `（${report.skipped_conflicts.length} 项冲突跳过）` : ''
      }`,
    )
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
  } finally {
    undoingId.value = null
  }
}
</script>

<template>
  <MdSideSheet
    :open="open"
    title="变更历史与撤销"
    subtitle="最近会话内的元数据编辑与重命名批次均支持一键无损回滚"
    width-class="w-full max-w-lg"
    @close="emit('close')"
  >
    <template #subheader>
      <div class="flex flex-wrap items-center justify-between gap-2">
        <MdSegmentedButton
          v-model="activeTab"
          :options="[
            { value: 'metadata', label: `元数据 (${metadataBatches.length})`, icon: LuPencil },
            { value: 'rename', label: `重命名 (${renameBatches.length})`, icon: LuFileSignature },
          ]"
          size="sm"
        />

        <label
          v-if="rootId"
          class="inline-flex cursor-pointer items-center gap-1.5 text-xs text-[var(--md-sys-color-on-surface-variant)]"
        >
          <input
            v-model="scopeAllRoots"
            type="checkbox"
            class="h-3.5 w-3.5 accent-[var(--md-sys-color-primary)]"
          />
          <span>全部目录</span>
        </label>
      </div>
    </template>

    <div class="space-y-3">
      <div
        v-if="errorMsg"
        class="flex items-start gap-2 rounded-2xl bg-[var(--md-sys-color-error-container)] px-4 py-3 text-xs text-[var(--md-sys-color-on-error-container)]"
      >
        <LuAlertCircle class="mt-0.5 h-4 w-4 shrink-0" />
        <span>{{ errorMsg }}</span>
      </div>

      <div v-if="loading" class="flex items-center justify-center py-14">
        <LuLoader2 class="h-6 w-6 animate-spin text-[var(--md-sys-color-primary)]" />
      </div>

      <!-- Tab 1: Metadata History -->
      <template v-else-if="activeTab === 'metadata'">
        <div
          v-if="metadataBatches.length === 0"
          class="rounded-2xl bg-[var(--md-sys-color-surface-container)] py-14 text-center"
        >
          <LuClock class="mx-auto mb-2.5 h-7 w-7 text-[var(--md-sys-color-on-surface-variant)]" />
          <p class="text-sm font-semibold">暂无元数据修改记录</p>
          <p class="mt-1 text-xs text-[var(--md-sys-color-on-surface-variant)]">
            单条保存或批量编辑元数据后会自动生成可回滚快照
          </p>
        </div>

        <div
          v-for="batch in metadataBatches"
          :key="batch.batch_id"
          class="overflow-hidden rounded-2xl bg-[var(--md-sys-color-surface)]"
        >
          <div class="flex flex-wrap items-center justify-between gap-2.5 px-4 py-3.5">
            <div class="min-w-0 flex-1">
              <div class="flex flex-wrap items-center gap-1.5">
                <span class="truncate text-xs font-semibold">
                  {{ batch.label }}
                </span>
                <span
                  class="rounded-full bg-[var(--md-sys-color-secondary-container)] px-2 py-0.5 text-[10px] font-semibold text-[var(--md-sys-color-on-secondary-container)]"
                >
                  {{ domainBadge(batch.media_kind) }}
                </span>
              </div>
              <p class="mt-1 font-mono text-[11px] text-[var(--md-sys-color-on-surface-variant)]">
                {{ formatUnix(batch.created_at_unix) }} · {{ batch.updated_count }} 个文件
              </p>
            </div>

            <div class="flex shrink-0 items-center gap-1">
              <button
                type="button"
                class="md-icon-btn md-icon-btn-sm md-state"
                title="查看文件明细"
                @click="toggleExpand(batch.batch_id)"
              >
                <LuChevronUp v-if="expandedIds.has(batch.batch_id)" class="h-4 w-4" />
                <LuChevronDown v-else class="h-4 w-4" />
              </button>

              <button
                type="button"
                :disabled="undoingId !== null"
                class="md-btn md-btn-sm md-btn-tonal md-state"
                @click="handleUndoMetadata(batch)"
              >
                <LuLoader2
                  v-if="undoingId === batch.batch_id"
                  class="h-3.5 w-3.5 animate-spin"
                />
                <LuRotateCcw v-else class="h-3.5 w-3.5" />
                <span>撤销</span>
              </button>
            </div>
          </div>

          <div
            v-if="expandedIds.has(batch.batch_id)"
            class="max-h-44 divide-y divide-[var(--md-sys-color-outline-variant)]/35 overflow-y-auto bg-[var(--md-sys-color-surface-container)]/50 px-4 py-1"
          >
            <div v-for="rec in batch.records" :key="rec.rel_path" class="py-1.5 text-[11px]">
              <div class="truncate font-medium">{{ rec.file_name }}</div>
              <div class="truncate font-mono text-[10px] text-[var(--md-sys-color-on-surface-variant)]">
                {{ rec.rel_path }}
              </div>
            </div>
          </div>
        </div>
      </template>

      <!-- Tab 2: Rename History -->
      <template v-else>
        <div
          v-if="renameBatches.length === 0"
          class="rounded-2xl bg-[var(--md-sys-color-surface-container)] py-14 text-center"
        >
          <LuClock class="mx-auto mb-2.5 h-7 w-7 text-[var(--md-sys-color-on-surface-variant)]" />
          <p class="text-sm font-semibold">暂无重命名记录</p>
          <p class="mt-1 text-xs text-[var(--md-sys-color-on-surface-variant)]">
            执行批量重命名后可在此一键还原文件及伴随文件
          </p>
        </div>

        <div
          v-for="batch in renameBatches"
          :key="batch.batch_id"
          class="overflow-hidden rounded-2xl bg-[var(--md-sys-color-surface)]"
        >
          <div class="flex flex-wrap items-center justify-between gap-2.5 px-4 py-3.5">
            <div class="min-w-0 flex-1">
              <div class="text-xs font-semibold">
                批量重命名 · {{ batch.moved_count }} 个文件
              </div>
              <p class="mt-1 font-mono text-[11px] text-[var(--md-sys-color-on-surface-variant)]">
                {{ formatUnix(batch.created_at_unix) }}
              </p>
            </div>

            <div class="flex shrink-0 items-center gap-1">
              <button
                type="button"
                class="md-icon-btn md-icon-btn-sm md-state"
                title="查看路径明细"
                @click="toggleExpand(batch.batch_id)"
              >
                <LuChevronUp v-if="expandedIds.has(batch.batch_id)" class="h-4 w-4" />
                <LuChevronDown v-else class="h-4 w-4" />
              </button>

              <button
                type="button"
                :disabled="undoingId !== null"
                class="md-btn md-btn-sm md-btn-tonal md-state"
                @click="handleUndoRename(batch)"
              >
                <LuLoader2
                  v-if="undoingId === batch.batch_id"
                  class="h-3.5 w-3.5 animate-spin"
                />
                <LuRotateCcw v-else class="h-3.5 w-3.5" />
                <span>撤销</span>
              </button>
            </div>
          </div>

          <div
            v-if="expandedIds.has(batch.batch_id)"
            class="max-h-44 divide-y divide-[var(--md-sys-color-outline-variant)]/35 overflow-y-auto bg-[var(--md-sys-color-surface-container)]/50 px-4 py-1 font-mono text-[10px]"
          >
            <div v-for="mv in batch.moves" :key="mv.to_rel_path" class="space-y-0.5 py-1.5">
              <div class="truncate text-[var(--md-sys-color-on-surface-variant)]">
                原：{{ mv.from_rel_path }}
              </div>
              <div class="truncate text-[var(--md-sys-color-primary)]">
                现：{{ mv.to_rel_path }}
              </div>
            </div>
          </div>
        </div>
      </template>
    </div>
  </MdSideSheet>
</template>
