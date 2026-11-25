<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  LuAlertTriangle,
  LuArrowRight,
  LuCheck,
  LuFileSignature,
  LuRefreshCw,
  LuUndo2,
} from 'vue-icons-plus/lu'
import AppCheckbox from '@/components/ui/AppCheckbox.vue'
import MdButton from '@/components/ui/MdButton.vue'
import MdChip from '@/components/ui/MdChip.vue'
import MdDialog from '@/components/ui/MdDialog.vue'
import MdLinearProgress from '@/components/ui/MdLinearProgress.vue'
import MdSegmentedButton from '@/components/ui/MdSegmentedButton.vue'
import MdTextField from '@/components/ui/MdTextField.vue'
import { useActionFeedback } from '@/composables/useActionFeedback'
import type { MediaKind } from '@/types/api'
import { toolApi } from '../api'
import { useToolTaskStore } from '../tasks'
import { DOMAIN_RENAME_TEMPLATE_SPECS } from '../templates'
import type {
  MediaFileEntry,
  RenameConflictPolicy,
  RenamePlanItem,
  RenameTemplatesDto,
} from '../types'

const props = defineProps<{
  open: boolean
  rootId: string
  entries: MediaFileEntry[]
  skippedOrganizedCount?: number
  templates: RenameTemplatesDto
}>()

const emit = defineEmits<{
  close: []
  renamed: [movedCount: number, movedMap: Record<string, string>]
  openHistory: []
}>()

const { t } = useI18n()

const templateInput = ref('')
const episodeTemplateInput = ref('')
const conflictPolicy = ref<RenameConflictPolicy>('skip')
const focusedVideoInput = ref<'movie' | 'episode'>('movie')
const movieFieldRef = ref<InstanceType<typeof MdTextField> | null>(null)
const episodeFieldRef = ref<InstanceType<typeof MdTextField> | null>(null)
const genericFieldRef = ref<InstanceType<typeof MdTextField> | null>(null)
const plans = ref<RenamePlanItem[]>([])
const excludedPaths = ref<Set<string>>(new Set())
const showUnchanged = ref(false)
const taskStore = useToolTaskStore()
const loadingPreview = ref(false)
const previewFeedback = useActionFeedback()
const previewSpinning = computed(() => loadingPreview.value || previewFeedback.running.value)
const executing = ref(false)
const inlineProgressPct = ref<number | null>(null)
const errorMsg = ref<string | null>(null)

const dominantKind = computed<MediaKind>(() => props.entries[0]?.media_kind ?? 'music')

const tokenChips = computed<string[]>(() => DOMAIN_RENAME_TEMPLATE_SPECS[dominantKind.value].quickChips)

function seedTemplatesForEntries(): void {
  const k = dominantKind.value
  if (k === 'music') {
    templateInput.value = props.templates.music
    episodeTemplateInput.value = ''
  } else if (k === 'comic') {
    templateInput.value = props.templates.comic
    episodeTemplateInput.value = ''
  } else if (k === 'book') {
    templateInput.value = props.templates.book
    episodeTemplateInput.value = ''
  } else {
    templateInput.value = props.templates.video_movie
    episodeTemplateInput.value = props.templates.video_episode
    const looksLikeEpisode = props.entries.some((e) => /s\d{1,2}e\d{1,3}/i.test(e.file_name))
    focusedVideoInput.value = looksLikeEpisode ? 'episode' : 'movie'
  }
}

let debounceTimer: ReturnType<typeof setTimeout> | null = null
let requestSeq = 0
let suppressPreviewWatch = false

function clearDebounceTimer(): void {
  if (debounceTimer) {
    clearTimeout(debounceTimer)
    debounceTimer = null
  }
}

async function runPreview(): Promise<void> {
  if (!props.open || props.entries.length === 0) return
  const seq = ++requestSeq
  loadingPreview.value = true
  inlineProgressPct.value = 0
  errorMsg.value = null
  const taskId = 'rename:preview'
  taskStore.startTask({
    id: taskId,
    kind: 'rename',
    title: t('tool.modals.batchRename.title', { count: props.entries.length }),
    total: props.entries.length,
    ephemeral: true,
  })
  try {
    const nextPlans = await toolApi.previewRename(
      props.rootId,
      props.entries.map((e) => e.rel_path),
      templateInput.value.trim() || null,
      dominantKind.value,
      dominantKind.value === 'video' ? episodeTemplateInput.value.trim() || null : undefined,
      (prog) => {
        if (seq !== requestSeq) return
        taskStore.updateTask(taskId, prog)
        if (prog.total && prog.total > 0) {
          inlineProgressPct.value = Math.min(100, Math.round((prog.processed / prog.total) * 100))
        }
      },
      conflictPolicy.value,
    )
    if (seq !== requestSeq || !props.open) return
    plans.value = nextPlans
    taskStore.completeTask(taskId, `${nextPlans.length}`)
  } catch (err) {
    if (seq !== requestSeq) return
    plans.value = []
    const msg = err instanceof Error ? err.message : String(err)
    errorMsg.value = msg
    taskStore.failTask(taskId, msg)
  } finally {
    if (seq === requestSeq) {
      loadingPreview.value = false
      inlineProgressPct.value = null
    }
  }
}

watch(
  () => props.open,
  (isOpen) => {
    clearDebounceTimer()
    if (isOpen) {
      suppressPreviewWatch = true
      excludedPaths.value = new Set()
      showUnchanged.value = false
      conflictPolicy.value = 'skip'
      seedTemplatesForEntries()
      void nextTick(() => {
        suppressPreviewWatch = false
      })
      void runPreview()
    } else {
      requestSeq += 1
    }
  },
  { immediate: true },
)

watch([templateInput, episodeTemplateInput], () => {
  if (!props.open || suppressPreviewWatch) return
  clearDebounceTimer()
  debounceTimer = setTimeout(() => {
    debounceTimer = null
    void runPreview()
  }, 260)
})

watch(conflictPolicy, () => {
  if (!props.open || suppressPreviewWatch) return
  clearDebounceTimer()
  void runPreview()
})

onBeforeUnmount(() => {
  clearDebounceTimer()
  requestSeq += 1
})

function resolveInputEl(field: InstanceType<typeof MdTextField> | null): HTMLInputElement | null {
  const raw = field?.inputRef
  return raw instanceof HTMLInputElement ? raw : null
}

async function insertTokenAtCursor(
  model: { value: string },
  inputEl: HTMLInputElement | null,
  tok: string,
): Promise<void> {
  if (!inputEl) {
    model.value = `${model.value}${tok}`
    return
  }
  const start = inputEl.selectionStart ?? model.value.length
  const end = inputEl.selectionEnd ?? start
  const nextVal = `${model.value.slice(0, start)}${tok}${model.value.slice(end)}`
  model.value = nextVal
  await nextTick()
  inputEl.focus()
  const caret = start + tok.length
  inputEl.setSelectionRange(caret, caret)
}

function appendChip(tok: string): void {
  if (dominantKind.value === 'video' && focusedVideoInput.value === 'episode') {
    void insertTokenAtCursor(episodeTemplateInput, resolveInputEl(episodeFieldRef.value), tok)
  } else if (dominantKind.value === 'video') {
    void insertTokenAtCursor(templateInput, resolveInputEl(movieFieldRef.value), tok)
  } else {
    void insertTokenAtCursor(templateInput, resolveInputEl(genericFieldRef.value), tok)
  }
}

function toggleRowExcluded(fromRelPath: string): void {
  const next = new Set(excludedPaths.value)
  if (next.has(fromRelPath)) {
    next.delete(fromRelPath)
  } else {
    next.add(fromRelPath)
  }
  excludedPaths.value = next
}

const activePlans = computed(() =>
  plans.value.filter((p) => !excludedPaths.value.has(p.from_rel_path)),
)

const unchangedCount = computed(() => plans.value.filter((p) => p.unchanged).length)
const visiblePlans = computed(() =>
  showUnchanged.value ? plans.value : plans.value.filter((p) => !p.unchanged),
)

const actionableCount = computed(
  () => activePlans.value.filter((p) => !p.unchanged && !p.conflict).length,
)

const conflictCount = computed(() => activePlans.value.filter((p) => Boolean(p.conflict)).length)
const overwriteCount = computed(
  () => activePlans.value.filter((p) => !p.unchanged && !p.conflict && p.will_overwrite).length,
)

const skippedSummaryText = computed(() => {
  const org = props.skippedOrganizedCount ?? 0
  const unch = unchangedCount.value
  if (org > 0 && unch > 0) {
    return t('tool.modals.batchRename.skippedBoth', { organized: org, unchanged: unch })
  }
  if (org > 0) {
    return t('tool.modals.batchRename.skippedOrganized', { count: org })
  }
  if (unch > 0) {
    return t('tool.modals.batchRename.skippedUnchanged', { count: unch })
  }
  return ''
})

async function handleExecute(): Promise<void> {
  if (actionableCount.value === 0 || executing.value) return
  executing.value = true
  inlineProgressPct.value = 0
  errorMsg.value = null
  const totalToMove = actionableCount.value
  const taskId = `rename:execute:${Date.now()}`
  taskStore.startTask({
    id: taskId,
    kind: 'rename',
    title: `${t('tool.actions.batchRename')} (${totalToMove})`,
    total: totalToMove,
    ephemeral: false,
  })
  try {
    const selected = activePlans.value.filter((p) => !p.unchanged && !p.conflict)
    if (selected.length === 0) return
    const movedMap: Record<string, string> = {}
    for (const p of selected) {
      movedMap[p.from_rel_path] = p.to_rel_path
    }
    const res = await toolApi.executeRename(props.rootId, selected, (prog) => {
      taskStore.updateTask(taskId, prog)
      if (prog.total && prog.total > 0) {
        inlineProgressPct.value = Math.min(100, Math.round((prog.processed / prog.total) * 100))
      }
    })
    taskStore.completeTask(taskId, `${res.moved_count}`)
    emit('renamed', res.moved_count, movedMap)
    emit('close')
  } catch (err) {
    const msg = err instanceof Error ? err.message : String(err)
    errorMsg.value = msg
    taskStore.failTask(taskId, msg)
  } finally {
    executing.value = false
    inlineProgressPct.value = null
  }
}
</script>

<template>
  <MdDialog
    :open="open"
    :title="t('tool.modals.batchRename.title', { count: entries.length })"
    :subtitle="t('tool.modals.batchRename.subtitle')"
    max-width="max-w-4xl"
    footer-align="between"
    :close-on-backdrop="!executing"
    @close="!executing && emit('close')"
  >
    <template #icon>
      <LuFileSignature class="h-5 w-5" aria-hidden="true" />
    </template>

    <div class="space-y-4 py-1">
      <MdLinearProgress
        v-if="(loadingPreview || executing) && inlineProgressPct !== null"
        :value="inlineProgressPct"
        :aria-label="t('tool.modals.batchRename.progressAria')"
      />

      <!-- Template Editor -->
      <div class="space-y-3 rounded-corner-lg bg-surface p-4">
        <template v-if="dominantKind === 'video'">
          <div class="grid grid-cols-1 gap-2.5 sm:grid-cols-12">
            <div class="sm:col-span-5">
              <MdTextField
                ref="movieFieldRef"
                v-model="templateInput"
                size="sm"
                :label="t('tool.modals.batchRename.movieTemplate')"
                placeholder="{title} ({year})/{title} ({year})"
                @focus="focusedVideoInput = 'movie'"
              />
            </div>
            <div class="sm:col-span-5">
              <MdTextField
                ref="episodeFieldRef"
                v-model="episodeTemplateInput"
                size="sm"
                :label="t('tool.modals.batchRename.episodeTemplate')"
                placeholder="{show} ({year})/Season {season:2}/{show} - S{season:2}E{episode:2}"
                @focus="focusedVideoInput = 'episode'"
              />
            </div>
            <div class="flex items-end sm:col-span-2">
              <MdButton
                variant="outlined"
                size="sm"
                block
                :icon="previewFeedback.done.value && !previewSpinning ? LuCheck : LuRefreshCw"
                :loading="previewSpinning"
                @click="previewFeedback.run(runPreview)"
              >
                {{
                  previewFeedback.done.value && !previewSpinning
                    ? t('tool.common.refreshed')
                    : t('tool.common.refresh')
                }}
              </MdButton>
            </div>
          </div>
        </template>
        <div v-else class="flex items-end gap-2">
          <div class="min-w-0 flex-1">
            <MdTextField
              ref="genericFieldRef"
              v-model="templateInput"
              size="sm"
              :label="t('tool.modals.batchRename.genericLabel')"
              :placeholder="t('tool.modals.batchRename.genericPlaceholder')"
            />
          </div>
          <MdButton
            variant="outlined"
            size="sm"
            :icon="previewFeedback.done.value && !previewSpinning ? LuCheck : LuRefreshCw"
            :loading="previewSpinning"
            @click="previewFeedback.run(runPreview)"
          >
            {{
              previewFeedback.done.value && !previewSpinning
                ? t('tool.common.refreshed')
                : t('tool.common.refresh')
            }}
          </MdButton>
        </div>

        <div class="flex flex-wrap items-center gap-1.5">
          <span class="text-label-medium text-on-surface-variant">
            {{ t('tool.modals.batchRename.tokensLabel') }}
          </span>
          <MdChip
            v-for="chip in tokenChips"
            :key="chip"
            variant="suggestion"
            :label="chip"
            @click="appendChip(chip)"
          />
        </div>

        <div class="flex flex-wrap items-center gap-2 border-t border-outline-variant/40 pt-2.5">
          <span class="text-label-medium text-on-surface-variant">
            {{ t('tool.modals.batchRename.conflictLabel') }}
          </span>
          <MdSegmentedButton
            v-model="conflictPolicy"
            :options="[
              {
                value: 'skip',
                label: t('tool.modals.batchRename.policySkip'),
                title: t('tool.modals.batchRename.policySkipHint'),
              },
              {
                value: 'auto_suffix',
                label: t('tool.modals.batchRename.policySuffix'),
                title: t('tool.modals.batchRename.policySuffixHint'),
              },
              {
                value: 'overwrite',
                label: t('tool.modals.batchRename.policyOverwrite'),
                title: t('tool.modals.batchRename.policyOverwriteHint'),
              },
            ]"
            size="sm"
          />
          <span
            v-if="conflictCount > 0 && conflictPolicy === 'skip'"
            class="text-body-small text-warning"
          >
            {{ t('tool.modals.batchRename.conflictNotice', { count: conflictCount }) }}
          </span>
          <span v-else-if="overwriteCount > 0" class="text-body-small text-warning">
            {{ t('tool.modals.batchRename.overwriteNotice', { count: overwriteCount }) }}
          </span>
        </div>
      </div>

      <!-- Preview Diff List -->
      <div class="space-y-2">
        <div
          v-if="errorMsg"
          role="alert"
          class="text-body-small rounded-corner-lg bg-error-container px-3.5 py-2.5 text-on-error-container"
        >
          {{ errorMsg }}
        </div>

        <div
          v-if="skippedSummaryText"
          class="text-body-small flex flex-wrap items-center justify-between gap-2 rounded-corner-lg bg-surface px-3.5 py-2 text-on-surface-variant"
        >
          <span>{{ skippedSummaryText }}</span>
          <MdButton
            v-if="unchangedCount > 0"
            variant="text"
            size="sm"
            @click="showUnchanged = !showUnchanged"
          >
            {{
              showUnchanged
                ? t('tool.modals.batchRename.hideUnchanged')
                : t('tool.modals.batchRename.showUnchanged', { count: unchangedCount })
            }}
          </MdButton>
        </div>

        <div
          v-if="!loadingPreview && visiblePlans.length === 0 && plans.length > 0"
          class="text-body-small rounded-corner-lg bg-surface py-8 text-center text-on-surface-variant"
        >
          {{ t('tool.modals.batchRename.allMatchedEmpty') }}
        </div>

        <div
          v-for="item in visiblePlans"
          :key="item.from_rel_path"
          class="rounded-corner-lg bg-surface p-3 transition"
          :class="item.unchanged || excludedPaths.has(item.from_rel_path) ? 'opacity-50' : ''"
        >
          <div class="text-body-small flex flex-wrap items-center gap-2">
            <AppCheckbox
              :disabled="item.unchanged"
              :aria-label="item.from_rel_path"
              :model-value="!item.unchanged && !excludedPaths.has(item.from_rel_path)"
              @change="toggleRowExcluded(item.from_rel_path)"
            />
            <span class="font-mono text-on-surface-variant">{{ item.from_rel_path }}</span>
            <LuArrowRight class="h-3.5 w-3.5 shrink-0 text-on-surface-variant" aria-hidden="true" />
            <span
              class="font-mono font-medium"
              :class="item.unchanged ? 'text-on-surface-variant' : 'text-primary'"
            >
              {{ item.to_rel_path }}
            </span>

            <span
              v-if="item.unchanged"
              class="text-label-small ml-auto rounded-corner-full bg-surface-container-high px-2 py-0.5 text-on-surface-variant"
            >
              {{ t('tool.modals.batchRename.unchangedBadge') }}
            </span>
            <span
              v-else-if="item.conflict"
              class="text-label-small ml-auto inline-flex items-center gap-1 rounded-corner-full bg-error-container px-2.5 py-0.5 text-on-error-container"
            >
              <LuAlertTriangle class="h-3 w-3" aria-hidden="true" />
              {{ item.conflict }}
            </span>
            <span
              v-else-if="item.will_overwrite"
              class="text-label-small ml-auto inline-flex items-center gap-1 rounded-corner-full bg-warning-container px-2.5 py-0.5 text-on-warning-container"
            >
              <LuAlertTriangle class="h-3 w-3" aria-hidden="true" />
              {{ t('tool.modals.batchRename.willOverwriteBadge') }}
            </span>
            <span
              v-else
              class="text-label-small ml-auto inline-flex items-center gap-1 rounded-corner-full bg-success-container px-2.5 py-0.5 text-on-success-container"
            >
              <LuCheck class="h-3 w-3" aria-hidden="true" />
              {{ t('tool.modals.batchRename.safeArchiveBadge') }}
            </span>
          </div>

          <div
            v-if="item.companion_moves.length > 0"
            class="text-body-small mt-1.5 flex flex-wrap items-center gap-1.5 text-on-surface-variant"
          >
            <span>{{ t('tool.modals.batchRename.companionsLabel') }}</span>
            <span
              v-for="[cFrom, cTo] in item.companion_moves"
              :key="cFrom"
              class="text-label-small rounded-corner-xs bg-surface-container-low px-2 py-0.5 font-mono"
            >
              {{ cFrom }} → {{ cTo }}
            </span>
          </div>
        </div>
      </div>
    </div>

    <template #footer>
      <div class="flex flex-wrap items-center gap-2">
        <span class="text-body-small text-on-surface-variant">
          {{ t('tool.modals.batchRename.footerSummary', { count: actionableCount }) }}
        </span>
        <MdButton variant="text" size="sm" :icon="LuUndo2" @click="emit('openHistory')">
          {{ t('tool.modals.batchRename.undoHistory') }}
        </MdButton>
      </div>
      <div class="flex items-center gap-2">
        <MdButton variant="text" size="sm" @click="emit('close')">
          {{ t('tool.common.cancel') }}
        </MdButton>
        <MdButton
          variant="filled"
          size="sm"
          :icon="LuCheck"
          :loading="executing"
          :disabled="actionableCount === 0"
          @click="handleExecute"
        >
          {{ t('tool.modals.batchRename.executeCount', { count: actionableCount }) }}
        </MdButton>
      </div>
    </template>
  </MdDialog>
</template>
