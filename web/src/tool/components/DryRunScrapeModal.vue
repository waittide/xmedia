<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  LuAlertTriangle,
  LuCheckCircle2,
  LuColumns3,
  LuLoader2,
  LuSearchCheck,
  LuSparkles,
} from 'vue-icons-plus/lu'
import AppCheckbox from '@/components/ui/AppCheckbox.vue'
import MdButton from '@/components/ui/MdButton.vue'
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

const { t } = useI18n()

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

let dryCheckSeq = 0

async function runDryCheck(): Promise<void> {
  if (!props.rootId || props.entries.length === 0) return
  const seq = ++dryCheckSeq
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
    if (seq !== dryCheckSeq || !props.open) return
    items.value = res
    const initial = new Set<string>()
    for (const item of res) {
      if (item.matched && !isItemLowConfidence(item)) {
        initial.add(item.rel_path)
      }
    }
    checkedPaths.value = initial
  } catch (err) {
    if (seq !== dryCheckSeq) return
    errorMsg.value = err instanceof Error ? err.message : String(err)
  } finally {
    if (seq === dryCheckSeq) {
      loading.value = false
    }
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
  const selected = props.entries.filter((entry) => checkedPaths.value.has(entry.rel_path))
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
    :title="t('tool.modals.dryRun.title', { count: entries.length })"
    :subtitle="t('tool.modals.dryRun.subtitle')"
    max-width="max-w-3xl"
    footer-align="between"
    @close="emit('close')"
  >
    <template #icon>
      <LuSearchCheck class="h-5 w-5" aria-hidden="true" />
    </template>

    <div class="py-1">
      <div
        v-if="loading"
        class="text-body-small flex h-48 flex-col items-center justify-center gap-2 text-on-surface-variant"
      >
        <LuLoader2 class="h-6 w-6 animate-spin text-primary" aria-hidden="true" />
        <span>{{ t('tool.modals.dryRun.checkingCount', { count: entries.length }) }}</span>
      </div>

      <div
        v-else-if="errorMsg"
        role="alert"
        class="text-body-small rounded-corner-lg bg-error-container p-3.5 text-on-error-container"
      >
        {{ errorMsg }}
      </div>

      <div v-else class="space-y-2">
        <div class="text-body-small mb-2 flex items-center justify-between text-on-surface-variant">
          <span>
            {{
              t('tool.modals.dryRun.checkedSummary', {
                selected: selectedCount,
                total: items.length,
              })
            }}
            <span v-if="lowConfidenceCount > 0" class="ml-2 text-warning">
              {{ t('tool.modals.dryRun.lowConfidenceNotice', { count: lowConfidenceCount }) }}
            </span>
          </span>
          <MdButton variant="outlined" size="sm" @click="selectAllMatched">
            {{ t('tool.modals.dryRun.selectAllMatched') }}
          </MdButton>
        </div>

        <!-- Non-Interactive Row Container + Sibling AppCheckbox & Row Toggle Button (Zero Nested role="checkbox") -->
        <div
          v-for="item in items"
          :key="item.rel_path"
          class="flex items-start justify-between gap-3 rounded-corner-lg p-3 transition"
          :class="
            isItemLowConfidence(item)
              ? 'bg-warning-container/35'
              : 'bg-surface'
          "
        >
          <div class="flex min-w-0 flex-1 items-start gap-2.5">
            <div class="pt-0.5">
              <AppCheckbox
                :model-value="checkedPaths.has(item.rel_path)"
                :aria-label="item.display_title"
                @update:model-value="() => togglePath(item.rel_path)"
              />
            </div>

            <button
              type="button"
              class="md-state min-w-0 flex-1 cursor-pointer rounded-corner-sm text-left outline-none focus-visible:outline-2 focus-visible:outline-secondary"
              @click="togglePath(item.rel_path)"
            >
              <div class="text-title-small truncate text-on-surface">{{ item.display_title }}</div>
              <div class="text-body-small truncate text-on-surface-variant">{{ item.rel_path }}</div>
              <div v-if="item.matched" class="text-body-small mt-1 flex flex-wrap items-center gap-1.5">
                <span class="font-medium text-primary">→ {{ item.matched_title }}</span>
                <span v-if="item.matched_creator" class="text-on-surface-variant">
                  · {{ item.matched_creator }}
                </span>
                <span v-if="item.matched_year != null" class="text-on-surface-variant">
                  ({{ item.matched_year }})
                </span>
                <span
                  v-if="item.matched_provider"
                  class="text-label-small rounded-corner-full bg-surface-container-high px-2 py-0.5"
                >
                  {{ item.matched_provider }}
                </span>
              </div>
              <div v-else class="text-body-small mt-1 text-error">
                {{ item.error || t('tool.modals.dryRun.noMatch') }}
              </div>
            </button>
          </div>

          <div
            class="text-label-medium flex shrink-0 items-center gap-1 rounded-corner-full px-2.5 py-1"
            :class="
              isItemLowConfidence(item)
                ? 'bg-warning-container text-on-warning-container'
                : 'bg-success-container text-on-success-container'
            "
          >
            <LuAlertTriangle v-if="isItemLowConfidence(item)" class="h-3.5 w-3.5" aria-hidden="true" />
            <LuCheckCircle2 v-else class="h-3.5 w-3.5" aria-hidden="true" />
            <span>{{ item.confidence }}%</span>
          </div>
        </div>
      </div>
    </div>

    <template #footer>
      <div>
        <MdButton
          v-if="!loading && unmatchedEntries.length > 0"
          variant="tonal"
          size="sm"
          :icon="LuColumns3"
          :title="t('tool.modals.dryRun.manualScrapeHint')"
          @click="handleManualScrape"
        >
          {{ t('tool.modals.dryRun.manualScrapeCount', { count: unmatchedEntries.length }) }}
        </MdButton>
      </div>
      <div class="flex items-center gap-2">
        <MdButton variant="text" size="sm" @click="emit('close')">
          {{ t('tool.common.cancel') }}
        </MdButton>
        <MdButton
          variant="filled"
          size="sm"
          :icon="LuSparkles"
          :disabled="loading || selectedCount === 0"
          @click="handleConfirm"
        >
          {{ t('tool.modals.dryRun.confirmScrapeCount', { count: selectedCount }) }}
        </MdButton>
      </div>
    </template>
  </MdDialog>
</template>
