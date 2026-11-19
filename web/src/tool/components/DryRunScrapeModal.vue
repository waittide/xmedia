<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue'
import {
  LuAlertTriangle,
  LuCheckCircle2,
  LuLoader2,
  LuSearchCheck,
  LuSparkles,
  LuX,
} from 'vue-icons-plus/lu'

import AppCheckbox from '@/components/ui/AppCheckbox.vue'
import { toolApi } from '../api'
import type { DryRunScrapeItemDto, MediaFileEntry } from '../types'

const props = defineProps<{
  open: boolean
  rootId: string | null
  entries: MediaFileEntry[]
}>()

const emit = defineEmits<{
  close: []
  confirm: [selectedEntries: MediaFileEntry[]]
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

const selectedCount = computed(() => checkedPaths.value.size)
const lowConfidenceCount = computed(() => items.value.filter((i) => i.low_confidence).length)

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
    )
    items.value = res
    // Auto-select high-confidence matched items; leave low-confidence unchecked for manual review
    const initial = new Set<string>()
    for (const item of res) {
      if (item.matched && !item.low_confidence) {
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

function handleKeydown(ev: KeyboardEvent): void {
  if (props.open && ev.key === 'Escape') {
    ev.preventDefault()
    emit('close')
  }
}

watch(
  () => props.open,
  (isOpen) => {
    if (isOpen) {
      window.addEventListener('keydown', handleKeydown)
      void runDryCheck()
    } else {
      window.removeEventListener('keydown', handleKeydown)
    }
  },
  { immediate: true },
)

onBeforeUnmount(() => window.removeEventListener('keydown', handleKeydown))

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
  emit('confirm', selected)
  emit('close')
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
        class="flex max-h-[86vh] w-full max-w-3xl flex-col overflow-hidden rounded-2xl border border-[var(--ctp-surface0)] bg-[var(--ctp-panel)] text-[var(--ctp-text)] shadow-2xl"
      >
        <div class="flex items-center justify-between border-b border-[var(--ctp-surface0)] px-5 py-3.5">
          <div class="flex items-center gap-2.5">
            <div class="flex h-8 w-8 items-center justify-center rounded-xl bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)]">
              <LuSearchCheck class="h-4 w-4" />
            </div>
            <div>
              <h3 class="text-sm font-semibold">批量刮削预检 (Dry-Run) · 共 {{ entries.length }} 项</h3>
              <p class="text-[11px] text-[var(--ctp-subtext0)]">
                仅做只读搜索与置信度评分，不修改本地文件；低置信度项已自动标黄取消勾选供复核
              </p>
            </div>
          </div>
          <button
            type="button"
            class="rounded-xl p-1.5 text-[var(--ctp-subtext0)] hover:bg-[var(--ctp-surface0)]"
            @click="emit('close')"
          >
            <LuX class="h-4 w-4" />
          </button>
        </div>

        <div class="min-h-0 flex-1 overflow-y-auto p-4 text-xs">
          <div v-if="loading" class="flex h-48 flex-col items-center justify-center gap-2 text-[var(--ctp-subtext0)]">
            <LuLoader2 class="h-6 w-6 animate-spin text-[var(--ctp-accent)]" />
            <span>正在对 {{ entries.length }} 个文件执行只读预检匹配…</span>
          </div>

          <div v-else-if="errorMsg" class="rounded-xl bg-rose-500/10 p-3 text-rose-500">
            {{ errorMsg }}
          </div>

          <div v-else class="space-y-2">
            <div class="mb-2 flex items-center justify-between text-[11px] text-[var(--ctp-subtext0)]">
              <span>
                已勾选 <strong class="text-[var(--ctp-text)]">{{ selectedCount }}</strong> / {{ items.length }} 项
                <span v-if="lowConfidenceCount > 0" class="ml-2 text-amber-500">
                  · {{ lowConfidenceCount }} 项低置信度需人工确认
                </span>
              </span>
              <button
                type="button"
                class="rounded-lg border border-[var(--ctp-surface0)] px-2 py-1 hover:border-[var(--ctp-accent)]"
                @click="selectAllMatched"
              >
                全选已匹配项
              </button>
            </div>

            <label
              v-for="item in items"
              :key="item.rel_path"
              class="flex cursor-pointer items-start justify-between gap-3 rounded-xl border p-3 transition"
              :class="
                item.low_confidence
                  ? 'border-amber-500/40 bg-amber-500/5'
                  : 'border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)]/70'
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
                  <div class="truncate text-[11px] text-[var(--ctp-subtext0)]">{{ item.rel_path }}</div>
                  <div v-if="item.matched" class="mt-1 flex flex-wrap items-center gap-1.5 text-[11px]">
                    <span class="font-semibold text-[var(--ctp-accent)]">→ {{ item.matched_title }}</span>
                    <span v-if="item.matched_creator" class="text-[var(--ctp-subtext0)]">· {{ item.matched_creator }}</span>
                    <span v-if="item.matched_year" class="text-[var(--ctp-subtext0)]">({{ item.matched_year }})</span>
                    <span v-if="item.matched_provider" class="rounded bg-[var(--ctp-surface0)] px-1.5 py-0.5 text-[10px]">
                      {{ item.matched_provider }}
                    </span>
                  </div>
                  <div v-else class="mt-1 text-[11px] text-rose-500">
                    {{ item.error || '未找到匹配条目' }}
                  </div>
                </div>
              </div>

              <div class="flex shrink-0 items-center gap-1 rounded-lg px-2 py-1 text-[11px] font-semibold" :class="item.low_confidence ? 'bg-amber-500/15 text-amber-500' : 'bg-emerald-500/15 text-emerald-500'">
                <LuAlertTriangle v-if="item.low_confidence" class="h-3.5 w-3.5" />
                <LuCheckCircle2 v-else class="h-3.5 w-3.5" />
                <span>{{ item.confidence }}%</span>
              </div>
            </label>
          </div>
        </div>

        <div class="flex items-center justify-end gap-2 border-t border-[var(--ctp-surface0)] px-5 py-3">
          <button
            type="button"
            class="rounded-xl border border-[var(--ctp-surface0)] px-3.5 py-1.5 text-xs hover:bg-[var(--ctp-surface0)]/60"
            @click="emit('close')"
          >
            取消
          </button>
          <button
            type="button"
            :disabled="loading || selectedCount === 0"
            class="inline-flex items-center gap-1.5 rounded-xl bg-[var(--ctp-accent)] px-4 py-1.5 text-xs font-medium text-[var(--ctp-on-accent)] disabled:opacity-50"
            @click="handleConfirm"
          >
            <LuSparkles class="h-3.5 w-3.5" />
            确认执行刮削（{{ selectedCount }} 项）
          </button>
        </div>
      </div>
    </div>
  </Teleport>
</template>
