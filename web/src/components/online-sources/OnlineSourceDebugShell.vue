<script setup lang="ts" generic="M extends string, Tab extends string">
import { computed, type Component } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  LuArrowRight,
  LuCheckCircle2,
  LuClock,
  LuCopy,
  LuFlaskConical,
  LuLoader2,
  LuPlay,
  LuSearch,
  LuSquare,
  LuX,
  LuXCircle,
} from 'vue-icons-plus/lu'
import MdSegmentedButton, { type SegmentedOption } from '@/components/ui/MdSegmentedButton.vue'
import MdSideSheet from '@/components/ui/MdSideSheet.vue'
import type { BaseSourceTestReportDto } from './types'

export interface DebugResultTabItem<T extends string = string> {
  id: T
  label: string
  icon: Component
  badge?: number
}

const props = defineProps<{
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
}>()

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
</script>

<template>
  <MdSideSheet
    :open="open"
    :title="title"
    :subtitle="subtitle"
    width-class="w-full max-w-[42rem]"
    @close="emit('close')"
  >
    <template #subheader>
      <div class="flex flex-col gap-3 border-b border-[var(--md-sys-color-outline-variant)]/60 pb-3.5">
        <div class="flex flex-wrap items-center justify-between gap-2">
          <MdSegmentedButton
            :model-value="mode"
            :options="modeOptions"
            size="sm"
            :show-check="false"
            @update:model-value="emit('update:mode', $event)"
          />

          <button
            v-if="report"
            type="button"
            class="md-btn md-btn-sm md-btn-tonal md-state"
            @click="emit('copy-report')"
          >
            <LuCopy class="h-3.5 w-3.5" />
            <span>{{ t('onlineSources.debug.copyReportBtn') }}</span>
          </button>
        </div>

        <form class="flex items-center gap-2" @submit.prevent="emit('run')">
          <div class="relative flex-1">
            <LuSearch
              class="pointer-events-none absolute top-1/2 left-3.5 h-4 w-4 -translate-y-1/2 text-[var(--md-sys-color-on-surface-variant)]"
            />
            <input
              :value="queryInput"
              type="text"
              :placeholder="inputPlaceholder"
              class="md-input w-full py-2 pr-9 pl-10 text-sm"
              @input="emit('update:queryInput', ($event.target as HTMLInputElement).value)"
            />
            <button
              v-if="queryInput"
              type="button"
              class="absolute top-1/2 right-2.5 -translate-y-1/2 rounded-full p-1 text-[var(--md-sys-color-on-surface-variant)] hover:bg-[var(--md-sys-color-surface-container-highest)]"
              :title="t('onlineSources.debug.clearInputTitle')"
              @click="emit('update:queryInput', '')"
            >
              <LuX class="h-3.5 w-3.5" />
            </button>
          </div>

          <button
            v-if="running"
            type="button"
            class="md-btn md-btn-outlined md-state shrink-0 !border-[var(--md-sys-color-error)] !text-[var(--md-sys-color-error)]"
            @click="emit('stop')"
          >
            <LuSquare class="h-3.5 w-3.5 fill-current" />
            <span>{{ t('onlineSources.debug.stopBtn') }}</span>
          </button>
          <button
            v-else
            type="submit"
            class="md-btn md-btn-filled md-state shrink-0"
          >
            <LuPlay class="h-4 w-4" />
            <span>{{ t('onlineSources.debug.runBtn') }}</span>
          </button>
        </form>

        <slot name="extra-controls" />

        <div
          v-if="hasQuickHint"
          class="flex flex-wrap items-center gap-1.5 text-[11px]"
        >
          <span class="text-[var(--md-sys-color-on-surface-variant)]">
            {{ t('onlineSources.debug.quickHintLabel') }}
          </span>
          <slot name="quick-hints" />
        </div>
      </div>
    </template>

    <div class="flex flex-col gap-4 pt-1">
      <!-- Request / Validation Error -->
      <div
        v-if="requestError"
        class="flex items-start gap-2.5 rounded-2xl bg-[var(--md-sys-color-error-container)] px-4 py-3 text-xs text-[var(--md-sys-color-on-error-container)]"
      >
        <LuXCircle class="mt-0.5 h-4 w-4 shrink-0" />
        <div class="min-w-0 flex-1 whitespace-pre-wrap break-all">{{ requestError }}</div>
      </div>

      <!-- Running Skeleton -->
      <div
        v-if="running && !report"
        class="flex flex-col items-center justify-center gap-3 rounded-3xl bg-[var(--md-sys-color-surface-container)] py-16 text-center"
      >
        <LuLoader2 class="h-7 w-7 animate-spin text-[var(--md-sys-color-primary)]" />
        <p class="text-sm font-medium text-[var(--md-sys-color-on-surface)]">
          {{ t('onlineSources.debug.runningTitle') }}
        </p>
        <p class="text-xs text-[var(--md-sys-color-on-surface-variant)]">
          {{ t('onlineSources.debug.runningHint') }}
        </p>
      </div>

      <!-- Idle Empty State -->
      <div
        v-else-if="!report && !running"
        class="rounded-3xl bg-[var(--md-sys-color-surface-container)] p-6 text-center"
      >
        <div
          class="mx-auto flex h-12 w-12 items-center justify-center rounded-2xl bg-[var(--md-sys-color-secondary-container)] text-[var(--md-sys-color-on-secondary-container)]"
        >
          <LuFlaskConical class="h-6 w-6" />
        </div>
        <h4 class="mt-3 text-sm font-semibold text-[var(--md-sys-color-on-surface)]">
          {{ t('onlineSources.debug.idleTitle', { domain: domainLabel }) }}
        </h4>
        <p class="mx-auto mt-1.5 max-w-md text-xs leading-relaxed text-[var(--md-sys-color-on-surface-variant)]">
          {{ t('onlineSources.debug.idleHint') }}
        </p>
        <button
          type="button"
          class="md-btn md-btn-tonal md-state mt-4"
          @click="emit('run')"
        >
          <LuPlay class="h-4 w-4" />
          <span>{{ t('onlineSources.debug.runFullBtn') }}</span>
        </button>
      </div>

      <!-- Debug Report -->
      <template v-if="report">
        <div class="rounded-2xl bg-[var(--md-sys-color-surface-container)] p-4">
          <div class="flex flex-wrap items-center justify-between gap-2">
            <div class="flex items-center gap-2">
              <span
                class="inline-flex items-center gap-1.5 rounded-full px-3 py-1 text-xs font-semibold"
                :class="
                  report.ok
                    ? 'bg-[var(--md-sys-color-success-container)] text-[var(--md-sys-color-on-success-container)]'
                    : 'bg-[var(--md-sys-color-error-container)] text-[var(--md-sys-color-on-error-container)]'
                "
              >
                <LuCheckCircle2 v-if="report.ok" class="h-3.5 w-3.5" />
                <LuXCircle v-else class="h-3.5 w-3.5" />
                <span>
                  {{
                    report.ok
                      ? t('onlineSources.debug.passedBadge')
                      : t('onlineSources.debug.failedBadge')
                  }}
                </span>
              </span>
              <span
                class="inline-flex items-center gap-1 rounded-full bg-[var(--md-sys-color-surface-container-high)] px-2.5 py-1 text-[11px] font-medium text-[var(--md-sys-color-on-surface-variant)]"
              >
                <LuClock class="h-3 w-3" />
                <span>{{ t('onlineSources.debug.elapsedMs', { ms: report.total_elapsed_ms }) }}</span>
              </span>
            </div>

            <span
              :title="report.keyword"
              class="max-w-[16rem] truncate text-xs text-[var(--md-sys-color-on-surface-variant)]"
            >
              {{ t('onlineSources.debug.inputLabel') }}
              <span class="font-mono text-[var(--md-sys-color-on-surface)]">{{ report.keyword }}</span>
            </span>
          </div>

          <!-- Sample Titles Chips (always visible immediately inside [role="dialog"]!) -->
          <div
            v-if="report.sample_titles.length > 0"
            class="mt-2.5 flex flex-wrap items-center gap-1.5"
          >
            <span class="text-[11px] text-[var(--md-sys-color-on-surface-variant)]">
              {{ t('onlineSources.debug.sampleTitlesLabel') }}
            </span>
            <span
              v-for="(title, sIdx) in report.sample_titles"
              :key="`${title}-${sIdx}`"
              class="rounded-full bg-[var(--md-sys-color-secondary-container)]/70 px-2.5 py-0.5 text-[11px] font-medium text-[var(--md-sys-color-on-secondary-container)]"
            >
              {{ title }}
            </span>
          </div>

          <!-- Stage Steps Strip -->
          <div class="mt-3 grid grid-cols-1 gap-2 sm:grid-cols-2">
            <button
              v-for="(step, idx) in report.steps"
              :key="`${step.stage}-${idx}`"
              type="button"
              class="md-state flex items-start gap-2.5 rounded-xl bg-[var(--md-sys-color-surface-container-low)] px-3 py-2.5 text-left transition"
              :class="activeTab === stageToTab(step.stage) ? 'ring-1 ring-[var(--md-sys-color-primary)]' : ''"
              @click="emit('update:activeTab', stageToTab(step.stage))"
            >
              <LuCheckCircle2
                v-if="step.ok"
                class="mt-0.5 h-4 w-4 shrink-0 text-[var(--md-sys-color-success)]"
              />
              <LuXCircle
                v-else
                class="mt-0.5 h-4 w-4 shrink-0 text-[var(--md-sys-color-error)]"
              />
              <div class="min-w-0 flex-1">
                <div class="flex items-center justify-between gap-2">
                  <span class="text-xs font-semibold text-[var(--md-sys-color-on-surface)]">
                    {{ step.label }}
                  </span>
                  <span class="shrink-0 font-mono text-[10px] text-[var(--md-sys-color-on-surface-variant)]">
                    {{ step.elapsed_ms }}ms
                  </span>
                </div>
                <p class="mt-0.5 truncate text-[11px] text-[var(--md-sys-color-on-surface-variant)]">
                  {{ step.summary }}
                </p>
              </div>
            </button>
          </div>
        </div>

        <!-- Result Tabs -->
        <div
          class="flex flex-wrap items-center gap-1.5 border-b border-[var(--md-sys-color-outline-variant)]/50 pb-2.5"
        >
          <button
            v-for="tab in resultTabs"
            :key="tab.id"
            type="button"
            class="md-state inline-flex items-center gap-1.5 rounded-full px-3.5 py-1.5 text-xs font-medium transition"
            :class="
              activeTab === tab.id
                ? 'bg-[var(--md-sys-color-secondary-container)] text-[var(--md-sys-color-on-secondary-container)]'
                : 'text-[var(--md-sys-color-on-surface-variant)] hover:bg-[var(--md-sys-color-surface-container-high)]'
            "
            @click="emit('update:activeTab', tab.id)"
          >
            <component :is="tab.icon" class="h-3.5 w-3.5" />
            <span>{{ tab.label }}</span>
            <span
              v-if="tab.id === timelineTabId && totalDiagnosticsCount > 0"
              class="rounded-full bg-[var(--md-sys-color-surface-container-highest)] px-1.5 text-[10px]"
            >
              {{ totalDiagnosticsCount }}
            </span>
          </button>
        </div>

        <!-- Timeline Tab -->
        <div v-if="activeTab === timelineTabId" class="flex flex-col gap-3">
          <div
            v-for="(step, idx) in report.steps"
            :key="`${step.stage}-${idx}`"
            class="rounded-2xl bg-[var(--md-sys-color-surface-container)] p-4"
          >
            <div class="flex flex-wrap items-center justify-between gap-2">
              <div class="flex items-center gap-2">
                <span
                  class="flex h-5 w-5 items-center justify-center rounded-full text-[11px] font-bold"
                  :class="
                    step.ok
                      ? 'bg-[var(--md-sys-color-success-container)] text-[var(--md-sys-color-on-success-container)]'
                      : 'bg-[var(--md-sys-color-error-container)] text-[var(--md-sys-color-on-error-container)]'
                  "
                >
                  {{ idx + 1 }}
                </span>
                <span class="text-sm font-semibold text-[var(--md-sys-color-on-surface)]">
                  {{ step.label }}
                </span>
              </div>
              <div class="flex items-center gap-2">
                <span class="font-mono text-[11px] text-[var(--md-sys-color-on-surface-variant)]">
                  {{ step.elapsed_ms }} ms
                </span>
                <button
                  type="button"
                  class="md-btn md-btn-sm md-btn-text md-state"
                  @click="emit('update:activeTab', stageToTab(step.stage))"
                >
                  <span>{{ t('onlineSources.debug.inspectStepBtn') }}</span>
                  <LuArrowRight class="h-3.5 w-3.5" />
                </button>
              </div>
            </div>

            <p class="mt-2 text-xs text-[var(--md-sys-color-on-surface)]">
              {{ step.summary }}
            </p>

            <div
              v-if="step.error"
              class="mt-2.5 rounded-xl bg-[var(--md-sys-color-error-container)] px-3 py-2 font-mono text-xs whitespace-pre-wrap break-all text-[var(--md-sys-color-on-error-container)]"
            >
              {{ step.error }}
            </div>

            <div
              v-if="
                step.diagnostics.logs.length > 0 ||
                step.diagnostics.toasts.length > 0 ||
                step.diagnostics.browser_urls.length > 0
              "
              class="mt-2.5 space-y-1.5 rounded-xl bg-[var(--md-sys-color-surface-container-low)] p-3 font-mono text-[11px]"
            >
              <div
                v-for="(toast, ti) in step.diagnostics.toasts"
                :key="`toast-${ti}`"
                class="text-[var(--md-sys-color-primary)]"
              >
                [toast] {{ toast }}
              </div>
              <div
                v-for="(log, li) in step.diagnostics.logs"
                :key="`log-${li}`"
                class="whitespace-pre-wrap break-all text-[var(--md-sys-color-on-surface-variant)]"
              >
                [log] {{ log }}
              </div>
              <div
                v-for="(hint, hi) in step.diagnostics.browser_urls"
                :key="`url-${hi}`"
                class="break-all text-[var(--md-sys-color-tertiary)]"
              >
                [browser] {{ hint.title || 'redirect' }}: {{ hint.url }}
              </div>
            </div>
          </div>
        </div>

        <slot v-else name="tab-content" />
      </template>
    </div>
  </MdSideSheet>
</template>
