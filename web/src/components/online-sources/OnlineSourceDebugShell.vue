<script setup lang="ts" generic="M extends string, Tab extends string">
import { computed, type Component } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  LuArrowRight,
  LuCheckCircle2,
  LuClock,
  LuCopy,
  LuFlaskConical,
  LuPlay,
  LuSquare,
  LuXCircle,
} from 'vue-icons-plus/lu'
import {
  MdButton,
  MdCircularProgress,
  MdDialog,
  MdEmptyState,
  MdTabs,
  MdTextField,
  type MdTabItem,
  type SegmentedOption,
} from '@/components/ui'
import type { BaseSourceTestReportDto } from './types'

export interface DebugResultTabItem<T extends string = string> {
  id: T
  label: string
  icon: Component
  badge?: number
}

const props = withDefaults(
  defineProps<{
    open: boolean
    title: string
    subtitle?: string
    domainLabel: string
    mode: M
    modeOptions: readonly SegmentedOption<M>[]
    queryInput: string
    inputPlaceholder: string
    running: boolean
    requestError: string | null
    report: BaseSourceTestReportDto<M> | null
    activeTab: Tab
    timelineTabId: Tab
    resultTabs: DebugResultTabItem<Tab>[]
    hasQuickHint?: boolean
    stageToTab: (stage: string) => Tab
    /** Render as MD3 full-screen dialog (default: true). */
    fullscreen?: boolean
  }>(),
  {
    subtitle: undefined,
    hasQuickHint: false,
    fullscreen: true,
  },
)

const emit = defineEmits<{
  close: []
  'update:mode': [mode: M]
  'update:queryInput': [value: string]
  'update:activeTab': [tab: Tab]
  run: []
  stop: []
  'copy-report': []
}>()

const { t } = useI18n()

const totalDiagnosticsCount = computed(() => {
  if (!props.report) return 0
  const d = props.report.diagnostics
  return d.logs.length + d.toasts.length + d.browser_urls.length
})

const modeTabs = computed<MdTabItem<M>[]>(() =>
  props.modeOptions.map((opt) => ({
    value: opt.value,
    label: opt.label,
    icon: opt.icon,
    disabled: opt.disabled,
  })),
)

const resultMdTabs = computed<MdTabItem<Tab>[]>(() =>
  props.resultTabs.map((tab) => ({
    value: tab.id,
    label: tab.label,
    icon: tab.icon,
    badge:
      tab.id === props.timelineTabId
        ? totalDiagnosticsCount.value > 0
          ? totalDiagnosticsCount.value
          : undefined
        : tab.badge,
  })),
)
</script>

<template>
  <MdDialog
    :open="open"
    :title="title"
    :subtitle="subtitle"
    :fullscreen="fullscreen"
    size="lg"
    @close="emit('close')"
  >
    <template v-if="report" #headerActions>
      <MdButton
        variant="tonal"
        :icon="LuCopy"
        @click="emit('copy-report')"
      >
        {{ t('onlineSources.debug.copyReportBtn') }}
      </MdButton>
    </template>

    <div class="mx-auto flex w-full max-w-5xl flex-col gap-4">
      <!-- Mode Tabs + Input Controls -->
      <div class="flex flex-col gap-3 rounded-corner-lg bg-surface-container-low p-4">
        <MdTabs
          :model-value="mode"
          :tabs="modeTabs"
          variant="secondary"
          @update:model-value="emit('update:mode', $event)"
        />

        <form class="flex items-center gap-2" @submit.prevent="emit('run')">
          <div class="min-w-0 flex-1">
            <MdTextField
              :model-value="queryInput"
              type="search"
              size="sm"
              clearable
              :placeholder="inputPlaceholder"
              @update:model-value="emit('update:queryInput', $event)"
              @clear="emit('update:queryInput', '')"
            />
          </div>

          <MdButton
            v-if="running"
            type="button"
            variant="outlined"
            tone="error"
            :icon="LuSquare"
            class="shrink-0"
            @click="emit('stop')"
          >
            {{ t('onlineSources.debug.stopBtn') }}
          </MdButton>
          <MdButton
            v-else
            type="submit"
            variant="filled"
            :icon="LuPlay"
            class="shrink-0"
          >
            {{ t('onlineSources.debug.runBtn') }}
          </MdButton>
        </form>

        <slot name="extra-controls" />

        <div v-if="hasQuickHint" class="flex flex-wrap items-center gap-1.5 text-label-small">
          <span class="text-on-surface-variant">
            {{ t('onlineSources.debug.quickHintLabel') }}
          </span>
          <slot name="quick-hints" />
        </div>
      </div>

      <!-- Request / Validation Error -->
      <div
        v-if="requestError"
        role="alert"
        class="flex items-start gap-2.5 rounded-corner-md bg-error-container px-4 py-3 text-body-small text-on-error-container"
      >
        <LuXCircle class="mt-0.5 h-4 w-4 shrink-0" aria-hidden="true" />
        <div class="min-w-0 flex-1 whitespace-pre-wrap break-all">{{ requestError }}</div>
      </div>

      <!-- Running Skeleton -->
      <div
        v-if="running && !report"
        class="flex flex-col items-center justify-center gap-3 rounded-corner-xl bg-surface-container py-16 text-center"
      >
        <MdCircularProgress indeterminate :size="36" :stroke-width="3" />
        <p class="text-title-small text-on-surface">
          {{ t('onlineSources.debug.runningTitle') }}
        </p>
        <p class="text-body-small text-on-surface-variant">
          {{ t('onlineSources.debug.runningHint') }}
        </p>
      </div>

      <!-- Idle Empty State -->
      <MdEmptyState
        v-else-if="!report && !running"
        :icon="LuFlaskConical"
        :title="t('onlineSources.debug.idleTitle', { domain: domainLabel })"
        :description="t('onlineSources.debug.idleHint')"
        class="rounded-corner-xl bg-surface-container"
      >
        <template #action>
          <MdButton variant="tonal" :icon="LuPlay" @click="emit('run')">
            {{ t('onlineSources.debug.runFullBtn') }}
          </MdButton>
        </template>
      </MdEmptyState>

      <!-- Debug Report -->
      <template v-if="report">
        <div class="rounded-corner-lg bg-surface-container p-4">
          <div class="flex flex-wrap items-center justify-between gap-2">
            <div class="flex flex-wrap items-center gap-2">
              <span
                class="text-label-medium inline-flex items-center gap-1.5 rounded-corner-full px-3 py-1"
                :class="
                  report.ok
                    ? 'bg-success-container text-on-success-container'
                    : 'bg-error-container text-on-error-container'
                "
              >
                <LuCheckCircle2 v-if="report.ok" class="h-3.5 w-3.5" aria-hidden="true" />
                <LuXCircle v-else class="h-3.5 w-3.5" aria-hidden="true" />
                <span>
                  {{
                    report.ok
                      ? t('onlineSources.debug.passedBadge')
                      : t('onlineSources.debug.failedBadge')
                  }}
                </span>
              </span>
              <span
                class="text-label-small inline-flex items-center gap-1 rounded-corner-full bg-surface-container-high px-2.5 py-1 text-on-surface-variant"
              >
                <LuClock class="h-3 w-3" aria-hidden="true" />
                <span class="tabular-nums">
                  {{ t('onlineSources.debug.elapsedMs', { ms: report.total_elapsed_ms }) }}
                </span>
              </span>
            </div>

            <span
              :title="report.keyword"
              class="text-body-small max-w-xs truncate text-on-surface-variant"
            >
              {{ t('onlineSources.debug.inputLabel') }}
              <span class="font-mono text-on-surface">{{ report.keyword }}</span>
            </span>
          </div>

          <!-- Sample Titles Chips (always visible immediately inside [role="dialog"]!) -->
          <div
            v-if="report.sample_titles.length > 0"
            class="mt-3 flex flex-wrap items-center gap-1.5"
          >
            <span class="text-label-small text-on-surface-variant">
              {{ t('onlineSources.debug.sampleTitlesLabel') }}
            </span>
            <span
              v-for="(sampleTitle, sIdx) in report.sample_titles"
              :key="`${sampleTitle}-${sIdx}`"
              class="text-label-small rounded-corner-full bg-secondary-container px-2.5 py-0.5 text-on-secondary-container"
            >
              {{ sampleTitle }}
            </span>
          </div>

          <!-- Stage Steps Strip -->
          <div class="mt-3 grid grid-cols-1 gap-2 sm:grid-cols-2 lg:grid-cols-3">
            <button
              v-for="(step, idx) in report.steps"
              :key="`${step.stage}-${idx}`"
              type="button"
              class="md-state flex cursor-pointer items-start gap-2.5 rounded-corner-md bg-surface-container-low px-3 py-2.5 text-left transition-colors duration-150 ease-standard"
              :class="activeTab === stageToTab(step.stage) ? 'ring-1 ring-primary' : ''"
              @click="emit('update:activeTab', stageToTab(step.stage))"
            >
              <LuCheckCircle2
                v-if="step.ok"
                class="mt-0.5 h-4 w-4 shrink-0 text-success"
                aria-hidden="true"
              />
              <LuXCircle
                v-else
                class="mt-0.5 h-4 w-4 shrink-0 text-error"
                aria-hidden="true"
              />
              <div class="min-w-0 flex-1">
                <div class="flex items-center justify-between gap-2">
                  <span class="text-label-medium text-on-surface">
                    {{ step.label }}
                  </span>
                  <span class="text-label-small shrink-0 font-mono tabular-nums text-on-surface-variant">
                    {{ step.elapsed_ms }}ms
                  </span>
                </div>
                <p class="text-body-small mt-0.5 truncate text-on-surface-variant">
                  {{ step.summary }}
                </p>
              </div>
            </button>
          </div>
        </div>

        <!-- Result Tabs -->
        <MdTabs
          :model-value="activeTab"
          :tabs="resultMdTabs"
          variant="secondary"
          @update:model-value="emit('update:activeTab', $event)"
        />

        <!-- Timeline Tab -->
        <div v-if="activeTab === timelineTabId" class="flex flex-col gap-3">
          <div
            v-for="(step, idx) in report.steps"
            :key="`${step.stage}-${idx}`"
            class="rounded-corner-lg bg-surface-container p-4"
          >
            <div class="flex flex-wrap items-center justify-between gap-2">
              <div class="flex items-center gap-2">
                <span
                  class="text-label-small flex h-5 w-5 items-center justify-center rounded-corner-full tabular-nums"
                  :class="
                    step.ok
                      ? 'bg-success-container text-on-success-container'
                      : 'bg-error-container text-on-error-container'
                  "
                >
                  {{ idx + 1 }}
                </span>
                <span class="text-title-small text-on-surface">
                  {{ step.label }}
                </span>
              </div>
              <div class="flex items-center gap-2">
                <span class="text-label-small font-mono tabular-nums text-on-surface-variant">
                  {{ step.elapsed_ms }} ms
                </span>
                <MdButton
                  variant="text"
                  @click="emit('update:activeTab', stageToTab(step.stage))"
                >
                  <span>{{ t('onlineSources.debug.inspectStepBtn') }}</span>
                  <template #trailing>
                    <LuArrowRight class="h-3.5 w-3.5" aria-hidden="true" />
                  </template>
                </MdButton>
              </div>
            </div>

            <p class="text-body-small mt-2 text-on-surface">
              {{ step.summary }}
            </p>

            <div
              v-if="step.error"
              class="text-body-small mt-2.5 rounded-corner-md bg-error-container px-3 py-2 font-mono whitespace-pre-wrap break-all text-on-error-container"
            >
              {{ step.error }}
            </div>

            <div
              v-if="
                step.diagnostics.logs.length > 0 ||
                step.diagnostics.toasts.length > 0 ||
                step.diagnostics.browser_urls.length > 0
              "
              class="text-label-small mt-2.5 space-y-1.5 rounded-corner-md bg-surface-container-low p-3 font-mono"
            >
              <div
                v-for="(toast, ti) in step.diagnostics.toasts"
                :key="`toast-${ti}`"
                class="text-primary"
              >
                [toast] {{ toast }}
              </div>
              <div
                v-for="(log, li) in step.diagnostics.logs"
                :key="`log-${li}`"
                class="whitespace-pre-wrap break-all text-on-surface-variant"
              >
                [log] {{ log }}
              </div>
              <div
                v-for="(hint, hi) in step.diagnostics.browser_urls"
                :key="`url-${hi}`"
                class="break-all text-tertiary"
              >
                [browser] {{ hint.title || 'redirect' }}: {{ hint.url }}
              </div>
            </div>
          </div>
        </div>

        <slot v-else name="tab-content" />
      </template>
    </div>
  </MdDialog>
</template>
