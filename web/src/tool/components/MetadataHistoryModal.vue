<script setup lang="ts">
import { ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
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
import AppCheckbox from '@/components/ui/AppCheckbox.vue'
import MdButton from '@/components/ui/MdButton.vue'
import MdIconButton from '@/components/ui/MdIconButton.vue'
import MdSegmentedButton from '@/components/ui/MdSegmentedButton.vue'
import MdSheet from '@/components/ui/MdSheet.vue'
import { useSnackbarStore } from '@/stores/snackbar'
import { toolApi } from '../api'
import type { MetadataHistoryBatchDto, RenameHistoryBatchDto } from '../types'

const props = withDefaults(
  defineProps<{
    open: boolean
    rootId?: string | null
    initialTab?: 'metadata' | 'rename'
  }>(),
  {
    rootId: null,
    initialTab: 'metadata',
  },
)

const emit = defineEmits<{
  close: []
  restored: []
}>()

const { t } = useI18n()
const snackbar = useSnackbarStore()

const activeTab = ref<'metadata' | 'rename'>('metadata')
const scopeAllRoots = ref(false)
const loading = ref(false)
const undoingId = ref<string | null>(null)
const errorMsg = ref<string | null>(null)

const metadataBatches = ref<MetadataHistoryBatchDto[]>([])
const renameBatches = ref<RenameHistoryBatchDto[]>([])
const expandedIds = ref<Set<string>>(new Set())

let historyLoadSeq = 0

async function loadHistory(): Promise<void> {
  const seq = ++historyLoadSeq
  loading.value = true
  errorMsg.value = null
  const rid = scopeAllRoots.value ? undefined : props.rootId || undefined
  try {
    const [metaRes, renameRes] = await Promise.all([
      toolApi.listMetadataHistory(rid),
      toolApi.listRenameHistory(rid),
    ])
    if (seq !== historyLoadSeq || !props.open) return
    metadataBatches.value = metaRes
    renameBatches.value = renameRes
  } catch (err) {
    if (seq !== historyLoadSeq) return
    errorMsg.value = err instanceof Error ? err.message : String(err)
  } finally {
    if (seq === historyLoadSeq) {
      loading.value = false
    }
  }
}

watch(
  () => props.open,
  (isOpen) => {
    if (isOpen) {
      activeTab.value = props.initialTab
      errorMsg.value = null
      void loadHistory()
    }
  },
  { immediate: true },
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
  return new Date(secs * 1000).toLocaleString([], {
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
  })
}

function domainBadge(kind: string): string {
  if (kind === 'book' || kind === 'comic' || kind === 'music' || kind === 'video') {
    return t(`tool.domains.${kind}`)
  }
  return kind
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
      t('tool.modals.history.undoneMeta', {
        label: batch.label,
        count: report.restored_count,
      }),
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
      t('tool.modals.history.undoneRename', {
        count: report.restored_count,
      }),
    )
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
  } finally {
    undoingId.value = null
  }
}
</script>

<template>
  <MdSheet
    :open="open"
    :title="t('tool.modals.history.title')"
    :subtitle="t('tool.modals.history.subtitle')"
    size="md"
    @close="emit('close')"
  >
    <template #subheader>
      <div class="flex flex-wrap items-center justify-between gap-2">
        <MdSegmentedButton
          v-model="activeTab"
          :options="[
            {
              value: 'metadata',
              label: t('tool.modals.history.tabMetadata', { count: metadataBatches.length }),
              icon: LuPencil,
            },
            {
              value: 'rename',
              label: t('tool.modals.history.tabRename', { count: renameBatches.length }),
              icon: LuFileSignature,
            },
          ]"
          size="sm"
        />

        <AppCheckbox v-if="rootId" v-model="scopeAllRoots" :label="t('tool.modals.history.allRoots')" />
      </div>
    </template>

    <div class="space-y-3">
      <div
        v-if="errorMsg"
        role="alert"
        class="text-body-small flex items-start gap-2 rounded-corner-lg bg-error-container px-4 py-3 text-on-error-container"
      >
        <LuAlertCircle class="mt-0.5 h-4 w-4 shrink-0" aria-hidden="true" />
        <span>{{ errorMsg }}</span>
      </div>

      <div v-if="loading" class="flex items-center justify-center py-14">
        <LuLoader2 class="h-6 w-6 animate-spin text-primary" aria-hidden="true" />
      </div>

      <!-- Tab 1: Metadata History -->
      <template v-else-if="activeTab === 'metadata'">
        <div
          v-if="metadataBatches.length === 0"
          class="rounded-corner-lg bg-surface-container py-14 text-center"
        >
          <LuClock class="mx-auto mb-2.5 h-7 w-7 text-on-surface-variant" aria-hidden="true" />
          <p class="text-title-small text-on-surface">{{ t('tool.modals.history.emptyMetaTitle') }}</p>
          <p class="text-body-small mt-1 text-on-surface-variant">
            {{ t('tool.modals.history.emptyMetaDesc') }}
          </p>
        </div>

        <div
          v-for="batch in metadataBatches"
          :key="batch.batch_id"
          class="overflow-hidden rounded-corner-lg bg-surface"
        >
          <div class="flex flex-wrap items-center justify-between gap-2.5 px-4 py-3.5">
            <div class="min-w-0 flex-1">
              <div class="flex flex-wrap items-center gap-1.5">
                <span class="text-title-small truncate text-on-surface">
                  {{ batch.label }}
                </span>
                <span
                  class="text-label-small rounded-corner-full bg-secondary-container px-2 py-0.5 text-on-secondary-container"
                >
                  {{ domainBadge(batch.media_kind) }}
                </span>
              </div>
              <p class="text-label-small mt-1 font-mono text-on-surface-variant">
                {{ formatUnix(batch.created_at_unix) }} ·
                {{ t('tool.modals.history.filesCount', { count: batch.updated_count }) }}
              </p>
            </div>

            <div class="flex shrink-0 items-center gap-1">
              <MdIconButton
                size="sm"
                :icon="expandedIds.has(batch.batch_id) ? LuChevronUp : LuChevronDown"
                :tooltip="t('tool.modals.history.viewDetails')"
                :aria-label="t('tool.modals.history.viewDetails')"
                @click="toggleExpand(batch.batch_id)"
              />

              <MdButton
                variant="tonal"
                size="sm"
                :icon="LuRotateCcw"
                :loading="undoingId === batch.batch_id"
                :disabled="undoingId !== null"
                @click="handleUndoMetadata(batch)"
              >
                {{ t('tool.modals.history.undo') }}
              </MdButton>
            </div>
          </div>

          <div
            v-if="expandedIds.has(batch.batch_id)"
            class="max-h-44 divide-y divide-outline-variant/35 overflow-y-auto bg-surface-container/50 px-4 py-1"
          >
            <div v-for="rec in batch.records" :key="rec.rel_path" class="py-1.5">
              <div class="text-body-small truncate font-medium text-on-surface">{{ rec.file_name }}</div>
              <div class="text-label-small truncate font-mono text-on-surface-variant">
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
          class="rounded-corner-lg bg-surface-container py-14 text-center"
        >
          <LuClock class="mx-auto mb-2.5 h-7 w-7 text-on-surface-variant" aria-hidden="true" />
          <p class="text-title-small text-on-surface">{{ t('tool.modals.history.emptyRenameTitle') }}</p>
          <p class="text-body-small mt-1 text-on-surface-variant">
            {{ t('tool.modals.history.emptyRenameDesc') }}
          </p>
        </div>

        <div
          v-for="batch in renameBatches"
          :key="batch.batch_id"
          class="overflow-hidden rounded-corner-lg bg-surface"
        >
          <div class="flex flex-wrap items-center justify-between gap-2.5 px-4 py-3.5">
            <div class="min-w-0 flex-1">
              <div class="text-title-small text-on-surface">
                {{ t('tool.modals.history.batchRenameTitle', { count: batch.moved_count }) }}
              </div>
              <p class="text-label-small mt-1 font-mono text-on-surface-variant">
                {{ formatUnix(batch.created_at_unix) }}
              </p>
            </div>

            <div class="flex shrink-0 items-center gap-1">
              <MdIconButton
                size="sm"
                :icon="expandedIds.has(batch.batch_id) ? LuChevronUp : LuChevronDown"
                :tooltip="t('tool.modals.history.viewDetails')"
                :aria-label="t('tool.modals.history.viewDetails')"
                @click="toggleExpand(batch.batch_id)"
              />

              <MdButton
                variant="tonal"
                size="sm"
                :icon="LuRotateCcw"
                :loading="undoingId === batch.batch_id"
                :disabled="undoingId !== null"
                @click="handleUndoRename(batch)"
              >
                {{ t('tool.modals.history.undo') }}
              </MdButton>
            </div>
          </div>

          <div
            v-if="expandedIds.has(batch.batch_id)"
            class="text-label-small max-h-44 divide-y divide-outline-variant/35 overflow-y-auto bg-surface-container/50 px-4 py-1 font-mono"
          >
            <div v-for="mv in batch.moves" :key="mv.to_rel_path" class="space-y-0.5 py-1.5">
              <div class="truncate text-on-surface-variant">
                {{ t('tool.modals.history.fromPrefix') }}{{ mv.from_rel_path }}
              </div>
              <div class="truncate text-primary">
                {{ t('tool.modals.history.toPrefix') }}{{ mv.to_rel_path }}
              </div>
            </div>
          </div>
        </div>
      </template>
    </div>
  </MdSheet>
</template>
