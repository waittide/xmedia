<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import {
  LuAlertTriangle,
  LuCheckCircle2,
  LuColumns3,
  LuLoader2,
  LuSearchCheck,
  LuSparkles,
} from 'vue-icons-plus/lu'

import AppCheckbox from '@/components/ui/AppCheckbox.vue'
import MdDialog from '@/components/ui/MdDialog.vue'
import { toolApi } from '../api'
import type { AutoScrapeOptionsDto, DryRunScrapeItemDto, MediaFileEntry } from '../types'

const props = defineProps<{
  open: boolean
  rootId: string | null
  entries: MediaFileEntry[]
  options?: AutoScrapeOptionsDto | null
}>()

const emit = defineEmits<{
  close: []
  confirm: [selectedEntries: MediaFileEntry[], options?: AutoScrapeOptionsDto]
  manualScrape: [entries: MediaFileEntry[]]
}>()

const loading = ref(false)
const errorMsg = ref('')
const items = ref<DryRunScrapeItemDto[]>([])
const checkedPaths = ref<Set<string>>(new Set())

const entryMap = computed(() => {
  const m = new Map<string, MediaFileEntry>()
  for (const e of props.entries) {
    m.set(e.rel_path, e)
  }
  return m
})

function isItemLowConfidence(item: DryRunScrapeItemDto): boolean {
  const mode = props.options?.match_mode
  if (mode === 'strict') return !item.matched || item.confidence < 70
  if (mode === 'standard') return !item.matched || item.confidence < 45
  if (mode === 'loose') return !item.matched
  return item.low_confidence
}

const selectedCount = computed(() => checkedPaths.value.size)
const lowConfidenceCount = computed(
  () => items.value.filter((i) => isItemLowConfidence(i)).length,
)
const unmatchedEntries = computed<MediaFileEntry[]>(() => {
  const out: MediaFileEntry[] = []
  for (const item of items.value) {
    if (!item.matched || isItemLowConfidence(item)) {
      const entry = entryMap.value.get(item.rel_path)
      if (entry) out.push(entry)
    }
  }
  return out
})

async function runDryCheck(): Promise<void> {
  if (!props.rootId || props.entries.length === 0) return
  loading.value = true
  errorMsg.value = ''
  items.value = []
  checkedPaths.value = new Set()
  try {
    const res = await toolApi.dryRunScrape(
      props.rootId,
      props.entries.map((e) => e.rel_path),
      undefined,
      props.options ?? undefined,
    )
    items.value = res
    const initial = new Set<string>()
    for (const item of res) {
      if (item.matched && !isItemLowConfidence(item)) {
        initial.add(item.rel_path)
      }
    }
    checkedPaths.value = initial
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
      void runDryCheck()
    }
  },
  { immediate: true },
)

function togglePath(path: string): void {
  const next = new Set(checkedPaths.value)
  if (next.has(path)) {
    next.delete(path)
  } else {
    next.add(path)
  }
  checkedPaths.value = next
}

function selectAllMatched(): void {
  const next = new Set<string>()
  for (const item of items.value) {
    if (item.matched) next.add(item.rel_path)
  }
  checkedPaths.value = next
}

function handleConfirm(): void {
  const selected: MediaFileEntry[] = []
  for (const p of checkedPaths.value) {
    const entry = entryMap.value.get(p)
    if (entry) selected.push(entry)
  }
  if (selected.length === 0) return
  emit('confirm', selected, props.options ?? undefined)
  emit('close')
}

function handleManualScrape(): void {
  if (unmatchedEntries.value.length === 0) return
  emit('manualScrape', unmatchedEntries.value)
  emit('close')
}
</script>

<template>
  <MdDialog
    :open="open"
    :title="`批量刮削预检 (Dry-Run) · 共 ${entries.length} 项`"
    subtitle="仅做只读搜索与置信度评分，不修改本地文件；低置信度项已自动标黄取消勾选供复核"
    max-width="max-w-3xl"
    @close="emit('close')"
  >
    <template #icon>
      <LuSearchCheck class="h-5 w-5" />
    </template>

    <div class="py-1 text-xs">
      <div v-if="loading" class="flex h-48 flex-col items-center justify-center gap-2 text-[var(--md-sys-color-on-surface-variant)]">
        <LuLoader2 class="h-6 w-6 animate-spin text-[var(--md-sys-color-primary)]" />
        <span>正在对 {{ entries.length }} 个文件执行只读预检匹配…</span>
      </div>

      <div
        v-else-if="errorMsg"
        class="rounded-2xl bg-[var(--md-sys-color-error-container)] p-3.5 text-[var(--md-sys-color-on-error-container)]"
      >
        {{ errorMsg }}
      </div>

      <div v-else class="space-y-2">
        <div class="mb-2 flex items-center justify-between text-[11px] text-[var(--md-sys-color-on-surface-variant)]">
          <span>
            已勾选 <strong class="text-[var(--md-sys-color-on-surface)]">{{ selectedCount }}</strong> / {{ items.length }} 项
            <span v-if="lowConfidenceCount > 0" class="ml-2 text-[var(--md-sys-color-warning)]">
              · {{ lowConfidenceCount }} 项低置信度需人工确认
            </span>
          </span>
          <button
            type="button"
            class="md-btn md-btn-sm md-btn-outlined md-state !h-7 !px-2.5 !text-[11px]"
            @click="selectAllMatched"
          >
            全选已匹配项
          </button>
        </div>

        <label
          v-for="item in items"
          :key="item.rel_path"
          class="flex cursor-pointer items-start justify-between gap-3 rounded-2xl border p-3 transition"
          :class="
            isItemLowConfidence(item)
              ? 'border-[var(--md-sys-color-warning)]/45 bg-[var(--md-sys-color-warning-container)]/35'
              : 'border-[var(--md-sys-color-outline-variant)]/55 bg-[var(--md-sys-color-surface-container-low)]'
          "
        >
          <div class="flex min-w-0 items-start gap-2.5">
            <div class="pt-0.5">
              <AppCheckbox
                :model-value="checkedPaths.has(item.rel_path)"
                @update:model-value="() => togglePath(item.rel_path)"
              />
            </div>
            <div class="min-w-0">
              <div class="truncate font-medium">{{ item.display_title }}</div>
              <div class="truncate text-[11px] text-[var(--md-sys-color-on-surface-variant)]">{{ item.rel_path }}</div>
              <div v-if="item.matched" class="mt-1 flex flex-wrap items-center gap-1.5 text-[11px]">
                <span class="font-semibold text-[var(--md-sys-color-primary)]">→ {{ item.matched_title }}</span>
                <span v-if="item.matched_creator" class="text-[var(--md-sys-color-on-surface-variant)]">· {{ item.matched_creator }}</span>
                <span v-if="item.matched_year" class="text-[var(--md-sys-color-on-surface-variant)]">({{ item.matched_year }})</span>
                <span
                  v-if="item.matched_provider"
                  class="rounded-full bg-[var(--md-sys-color-surface-container-high)] px-2 py-0.5 text-[10px]"
                >
                  {{ item.matched_provider }}
                </span>
              </div>
              <div v-else class="mt-1 text-[11px] text-[var(--md-sys-color-error)]">
                {{ item.error || '未找到匹配条目' }}
              </div>
            </div>
          </div>

          <div
            class="flex shrink-0 items-center gap-1 rounded-full px-2.5 py-1 text-[11px] font-semibold"
            :class="
              isItemLowConfidence(item)
                ? 'bg-[var(--md-sys-color-warning-container)] text-[var(--md-sys-color-on-warning-container)]'
                : 'bg-[var(--md-sys-color-success-container)] text-[var(--md-sys-color-on-success-container)]'
            "
          >
            <LuAlertTriangle v-if="isItemLowConfidence(item)" class="h-3.5 w-3.5" />
            <LuCheckCircle2 v-else class="h-3.5 w-3.5" />
            <span>{{ item.confidence }}%</span>
          </div>
        </label>
      </div>
    </div>

    <template #footer>
      <div>
        <button
          v-if="!loading && unmatchedEntries.length > 0"
          type="button"
          class="md-btn md-btn-sm md-state bg-[var(--md-sys-color-warning-container)] text-[var(--md-sys-color-on-warning-container)]"
          title="在三栏刮削工作台中逐项手动处理未匹配或低置信度条目"
          @click="handleManualScrape"
        >
          <LuColumns3 class="h-3.5 w-3.5" />
          <span>手动刮削（{{ unmatchedEntries.length }} 项）</span>
        </button>
      </div>
      <div class="flex items-center gap-2">
        <button
          type="button"
          class="md-btn md-btn-sm md-btn-text md-state"
          @click="emit('close')"
        >
          取消
        </button>
        <button
          type="button"
          :disabled="loading || selectedCount === 0"
          class="md-btn md-btn-sm md-btn-filled md-state"
          @click="handleConfirm"
        >
          <LuSparkles class="h-3.5 w-3.5" />
          <span>确认执行刮削（{{ selectedCount }} 项）</span>
        </button>
      </div>
    </template>
  </MdDialog>
</template>
