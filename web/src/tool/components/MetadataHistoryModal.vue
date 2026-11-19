<script setup lang="ts">
import { ref, watch } from 'vue'
import {
  LuAlertCircle,
  LuCheckCircle2,
  LuChevronDown,
  LuChevronUp,
  LuClock,
  LuFileSignature,
  LuLoader2,
  LuPencil,
  LuRotateCcw,
  LuUndo2,
  LuX,
} from 'vue-icons-plus/lu'

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

const activeTab = ref<'metadata' | 'rename'>('metadata')
const scopeAllRoots = ref(false)
const loading = ref(false)
const undoingId = ref<string | null>(null)
const errorMsg = ref<string | null>(null)
const bannerMsg = ref<string | null>(null)

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
      bannerMsg.value = null
      errorMsg.value = null
      void loadHistory()
    }
  },
)

watch(scopeAllRoots, () => {
  if (props.open) {
    void loadHistory()
  }
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
  bannerMsg.value = null
  try {
    const report = await toolApi.undoMetadataBatch(batch.batch_id)
    bannerMsg.value = `已撤销「${batch.label}」：成功恢复 ${report.restored_count} 个文件的元数据${
      report.failed_paths.length > 0 ? `（${report.failed_paths.length} 项失败）` : ''
    }`
    await loadHistory()
    emit('restored')
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
  bannerMsg.value = null
  try {
    const report = await toolApi.undoRenameBatch(batch.batch_id)
    bannerMsg.value = `已撤销重命名批次：成功还原 ${report.restored_count} 个文件路径${
      report.skipped_conflicts.length > 0
        ? `（${report.skipped_conflicts.length} 项因路径冲突跳过）`
        : ''
    }`
    await loadHistory()
    emit('restored')
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
  } finally {
    undoingId.value = null
  }
}
</script>

<template>
  <Teleport to="body">
    <div
      v-if="open"
      class="fixed inset-0 z-50 flex items-center justify-center bg-black/55 p-4 backdrop-blur-xs"
      @click.self="emit('close')"
    >
      <div
        class="flex max-h-[86vh] w-full max-w-2xl flex-col overflow-hidden rounded-3xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] shadow-2xl"
      >
        <!-- Header -->
        <div
          class="flex items-center justify-between gap-3 border-b border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)]/60 px-6 py-4"
        >
          <div class="flex items-center gap-3">
            <div
              class="flex h-10 w-10 items-center justify-center rounded-2xl bg-[var(--ctp-accent-soft)] text-[var(--ctp-accent)]"
            >
              <LuUndo2 class="h-5 w-5" />
            </div>
            <div>
              <h3 class="text-base font-bold text-[var(--ctp-text)]">变更历史与一键撤销</h3>
              <p class="mt-0.5 text-xs text-[var(--ctp-subtext0)]">
                最近会话内的元数据编辑与批量重命名均自动记录快照，支持按批次无损回滚
              </p>
            </div>
          </div>

          <button
            type="button"
            class="rounded-xl p-1.5 text-[var(--ctp-subtext0)] transition hover:bg-[var(--ctp-surface0)]/60 hover:text-[var(--ctp-text)]"
            @click="emit('close')"
          >
            <LuX class="h-4 w-4" />
          </button>
        </div>

        <!-- Tabs + Scope Toggle -->
        <div
          class="flex flex-wrap items-center justify-between gap-2 border-b border-[var(--ctp-surface0)]/70 px-6 py-3"
        >
          <div class="inline-flex gap-1 rounded-xl bg-[var(--ctp-mantle)] p-1">
            <button
              type="button"
              class="inline-flex items-center gap-1.5 rounded-lg px-3 py-1.5 text-xs font-semibold transition"
              :class="
                activeTab === 'metadata'
                  ? 'bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)]'
                  : 'text-[var(--ctp-subtext1)] hover:text-[var(--ctp-text)]'
              "
              @click="activeTab = 'metadata'"
            >
              <LuPencil class="h-3.5 w-3.5" />
              <span>元数据编辑 ({{ metadataBatches.length }})</span>
            </button>
            <button
              type="button"
              class="inline-flex items-center gap-1.5 rounded-lg px-3 py-1.5 text-xs font-semibold transition"
              :class="
                activeTab === 'rename'
                  ? 'bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)]'
                  : 'text-[var(--ctp-subtext1)] hover:text-[var(--ctp-text)]'
              "
              @click="activeTab = 'rename'"
            >
              <LuFileSignature class="h-3.5 w-3.5" />
              <span>批量重命名 ({{ renameBatches.length }})</span>
            </button>
          </div>

          <label
            v-if="rootId"
            class="inline-flex cursor-pointer items-center gap-1.5 text-xs text-[var(--ctp-subtext1)]"
          >
            <input
              v-model="scopeAllRoots"
              type="checkbox"
              class="h-3.5 w-3.5 accent-[var(--ctp-accent)]"
            />
            <span>显示全部工作目录</span>
          </label>
        </div>

        <!-- Body -->
        <div class="flex-1 space-y-3 overflow-y-auto px-6 py-4">
          <div
            v-if="errorMsg"
            class="flex items-start gap-2 rounded-2xl border border-red-500/35 bg-red-500/10 px-4 py-3 text-xs text-red-400"
          >
            <LuAlertCircle class="mt-0.5 h-4 w-4 shrink-0" />
            <span>{{ errorMsg }}</span>
          </div>

          <div
            v-if="bannerMsg"
            class="flex items-center gap-2 rounded-2xl border border-emerald-500/35 bg-emerald-500/10 px-4 py-3 text-xs font-medium text-emerald-500"
          >
            <LuCheckCircle2 class="h-4 w-4 shrink-0" />
            <span>{{ bannerMsg }}</span>
          </div>

          <div v-if="loading" class="flex items-center justify-center py-12">
            <LuLoader2 class="h-6 w-6 animate-spin text-[var(--ctp-accent)]" />
          </div>

          <!-- Tab 1: Metadata History -->
          <template v-else-if="activeTab === 'metadata'">
            <div
              v-if="metadataBatches.length === 0"
              class="rounded-2xl border border-dashed border-[var(--ctp-surface1)] bg-[var(--ctp-mantle)]/40 py-14 text-center"
            >
              <LuClock class="mx-auto mb-2.5 h-7 w-7 text-[var(--ctp-subtext0)]" />
              <p class="text-sm font-semibold text-[var(--ctp-text)]">暂无可撤销的元数据修改记录</p>
              <p class="mt-1 text-xs text-[var(--ctp-subtext0)]">
                单条保存或批量编辑元数据后，此处会自动保留修改前快照
              </p>
            </div>

            <div
              v-for="batch in metadataBatches"
              :key="batch.batch_id"
              class="overflow-hidden rounded-2xl border border-[var(--ctp-surface0)]/80 bg-[var(--ctp-mantle)]/45"
            >
              <div class="flex flex-wrap items-center justify-between gap-3 px-4 py-3">
                <div class="min-w-0 flex-1">
                  <div class="flex flex-wrap items-center gap-2">
                    <span class="truncate text-xs font-bold text-[var(--ctp-text)]">
                      {{ batch.label }}
                    </span>
                    <span
                      class="rounded-full bg-[var(--ctp-accent-soft)] px-2 py-0.5 text-[10px] font-semibold text-[var(--ctp-accent)]"
                    >
                      {{ domainBadge(batch.media_kind) }}
                    </span>
                    <span class="font-mono text-[10px] text-[var(--ctp-subtext0)]">
                      {{ formatUnix(batch.created_at_unix) }}
                    </span>
                  </div>
                  <p class="mt-0.5 text-[11px] text-[var(--ctp-subtext0)]">
                    涉及 {{ batch.updated_count }} 个文件
                  </p>
                </div>

                <div class="flex shrink-0 items-center gap-1.5">
                  <button
                    type="button"
                    class="inline-flex h-8 items-center gap-1 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-2.5 text-[11px] font-medium text-[var(--ctp-subtext1)] transition hover:text-[var(--ctp-text)]"
                    @click="toggleExpand(batch.batch_id)"
                  >
                    <span>文件明细</span>
                    <LuChevronUp v-if="expandedIds.has(batch.batch_id)" class="h-3.5 w-3.5" />
                    <LuChevronDown v-else class="h-3.5 w-3.5" />
                  </button>

                  <button
                    type="button"
                    :disabled="undoingId !== null"
                    class="inline-flex h-8 items-center gap-1 rounded-xl border border-amber-500/40 bg-amber-500/10 px-3 text-[11px] font-semibold text-amber-500 transition hover:bg-amber-500/20 disabled:opacity-45"
                    @click="handleUndoMetadata(batch)"
                  >
                    <LuLoader2
                      v-if="undoingId === batch.batch_id"
                      class="h-3.5 w-3.5 animate-spin"
                    />
                    <LuRotateCcw v-else class="h-3.5 w-3.5" />
                    <span>撤销回滚</span>
                  </button>
                </div>
              </div>

              <div
                v-if="expandedIds.has(batch.batch_id)"
                class="max-h-48 divide-y divide-[var(--ctp-surface0)]/50 overflow-y-auto border-t border-[var(--ctp-surface0)]/70 bg-[var(--ctp-canvas)]/70 px-4 py-1.5"
              >
                <div
                  v-for="rec in batch.records"
                  :key="rec.rel_path"
                  class="py-1.5 text-[11px]"
                >
                  <div class="truncate font-medium text-[var(--ctp-text)]">{{ rec.file_name }}</div>
                  <div class="truncate font-mono text-[10px] text-[var(--ctp-subtext0)]">
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
              class="rounded-2xl border border-dashed border-[var(--ctp-surface1)] bg-[var(--ctp-mantle)]/40 py-14 text-center"
            >
              <LuClock class="mx-auto mb-2.5 h-7 w-7 text-[var(--ctp-subtext0)]" />
              <p class="text-sm font-semibold text-[var(--ctp-text)]">暂无可撤销的重命名记录</p>
              <p class="mt-1 text-xs text-[var(--ctp-subtext0)]">
                执行批量重命名与归档后，可在此一键还原文件及伴随文件（.nfo / .lrc / 封面）
              </p>
            </div>

            <div
              v-for="batch in renameBatches"
              :key="batch.batch_id"
              class="overflow-hidden rounded-2xl border border-[var(--ctp-surface0)]/80 bg-[var(--ctp-mantle)]/45"
            >
              <div class="flex flex-wrap items-center justify-between gap-3 px-4 py-3">
                <div class="min-w-0 flex-1">
                  <div class="flex flex-wrap items-center gap-2">
                    <span class="text-xs font-bold text-[var(--ctp-text)]">
                      批量重命名 · {{ batch.moved_count }} 个文件
                    </span>
                    <span class="font-mono text-[10px] text-[var(--ctp-subtext0)]">
                      {{ formatUnix(batch.created_at_unix) }}
                    </span>
                  </div>
                </div>

                <div class="flex shrink-0 items-center gap-1.5">
                  <button
                    type="button"
                    class="inline-flex h-8 items-center gap-1 rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-2.5 text-[11px] font-medium text-[var(--ctp-subtext1)] transition hover:text-[var(--ctp-text)]"
                    @click="toggleExpand(batch.batch_id)"
                  >
                    <span>路径明细</span>
                    <LuChevronUp v-if="expandedIds.has(batch.batch_id)" class="h-3.5 w-3.5" />
                    <LuChevronDown v-else class="h-3.5 w-3.5" />
                  </button>

                  <button
                    type="button"
                    :disabled="undoingId !== null"
                    class="inline-flex h-8 items-center gap-1 rounded-xl border border-amber-500/40 bg-amber-500/10 px-3 text-[11px] font-semibold text-amber-500 transition hover:bg-amber-500/20 disabled:opacity-45"
                    @click="handleUndoRename(batch)"
                  >
                    <LuLoader2
                      v-if="undoingId === batch.batch_id"
                      class="h-3.5 w-3.5 animate-spin"
                    />
                    <LuRotateCcw v-else class="h-3.5 w-3.5" />
                    <span>撤销重命名</span>
                  </button>
                </div>
              </div>

              <div
                v-if="expandedIds.has(batch.batch_id)"
                class="max-h-48 divide-y divide-[var(--ctp-surface0)]/50 overflow-y-auto border-t border-[var(--ctp-surface0)]/70 bg-[var(--ctp-canvas)]/70 px-4 py-1.5 font-mono text-[10px]"
              >
                <div
                  v-for="mv in batch.moves"
                  :key="mv.to_rel_path"
                  class="space-y-0.5 py-1.5"
                >
                  <div class="truncate text-[var(--ctp-subtext0)]">原：{{ mv.from_rel_path }}</div>
                  <div class="truncate text-[var(--ctp-accent)]">现：{{ mv.to_rel_path }}</div>
                </div>
              </div>
            </div>
          </template>
        </div>

        <!-- Footer -->
        <div
          class="flex items-center justify-end border-t border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)]/50 px-6 py-3.5"
        >
          <button
            type="button"
            class="rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] px-4 py-2 text-xs font-semibold text-[var(--ctp-text)] transition hover:border-[var(--ctp-accent)]"
            @click="emit('close')"
          >
            关闭
          </button>
        </div>
      </div>
    </div>
  </Teleport>
</template>
