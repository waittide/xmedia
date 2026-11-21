<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import { LuGripVertical, LuMoreVertical, LuPencil } from 'vue-icons-plus/lu'
import AppCheckbox from '@/components/ui/AppCheckbox.vue'
import AppSwitch from '@/components/ui/AppSwitch.vue'
import type { OnlineSourceSummaryDto } from './types'

defineProps<{
  domainLabel: string
  source: OnlineSourceSummaryDto
  totalCount: number
  selectionMode: boolean
  selected: boolean
  canReorder: boolean
  dragging: boolean
  dropIndicator: 'before' | 'after' | null
  busy: boolean
  menuOpen: boolean
}>()

const emit = defineEmits<{
  'toggle-select': [shiftKey: boolean]
  'toggle-enabled': [enabled: boolean]
  edit: []
  'toggle-menu': [ev: MouseEvent]
  'drag-start': [ev: DragEvent]
  'drag-over': [ev: DragEvent]
  'drag-leave': [ev: DragEvent]
  drop: [ev: DragEvent]
  'drag-end': []
}>()

const { t } = useI18n()

function formatSourceHost(rawUrl: string): string {
  try {
    const u = new URL(rawUrl)
    return u.host || rawUrl
  } catch {
    return rawUrl
  }
}

function statusDotClass(src: OnlineSourceSummaryDto): string {
  if (src.last_test_ok === true) return 'bg-[var(--md-sys-color-success)]'
  if (src.last_test_ok === false) return 'bg-[var(--md-sys-color-error)]'
  return 'bg-[var(--md-sys-color-outline)]/55'
}

function statusDotTitle(src: OnlineSourceSummaryDto): string {
  if (src.last_test_ok === true) {
    return src.last_test_msg || t('onlineSources.card.testPassed')
  }
  if (src.last_test_ok === false) {
    return src.last_test_msg || t('onlineSources.card.testFailed')
  }
  return t('onlineSources.card.notTested')
}
</script>

<template>
  <div
    :draggable="canReorder"
    class="relative flex items-center justify-between gap-3 rounded-2xl px-3.5 py-3.5 transition"
    :class="[
      selectionMode
        ? selected
          ? 'cursor-pointer select-none bg-[var(--md-sys-color-secondary-container)]/35 ring-1 ring-[var(--md-sys-color-primary)]/55'
          : 'cursor-pointer select-none bg-[var(--md-sys-color-surface-container)] hover:bg-[var(--md-sys-color-surface-container-high)]/75'
        : 'bg-[var(--md-sys-color-surface-container)] hover:bg-[var(--md-sys-color-surface-container-high)]/75',
      !source.enabled && !selected ? 'opacity-65' : '',
      dragging ? 'opacity-40 ring-1 ring-[var(--md-sys-color-primary)]/50' : '',
    ]"
    @click="selectionMode ? emit('toggle-select', $event.shiftKey) : undefined"
    @dragstart="emit('drag-start', $event)"
    @dragover="emit('drag-over', $event)"
    @dragleave="emit('drag-leave', $event)"
    @drop="emit('drop', $event)"
    @dragend="emit('drag-end')"
  >
    <div
      v-if="dropIndicator === 'before'"
      class="pointer-events-none absolute -top-1.5 right-3 left-3 h-0.5 rounded-full bg-[var(--md-sys-color-primary)]"
    />
    <div
      v-if="dropIndicator === 'after'"
      class="pointer-events-none absolute right-3 -bottom-1.5 left-3 h-0.5 rounded-full bg-[var(--md-sys-color-primary)]"
    />

    <div class="flex min-w-0 flex-1 items-center gap-2.5">
      <div v-if="selectionMode" class="flex shrink-0 items-center">
        <AppCheckbox :model-value="selected" />
      </div>
      <div
        v-else-if="totalCount > 1"
        class="flex h-7 w-5 shrink-0 items-center justify-center rounded-md text-[var(--md-sys-color-on-surface-variant)]/60 transition"
        :class="
          canReorder
            ? 'cursor-grab hover:text-[var(--md-sys-color-on-surface)] active:cursor-grabbing'
            : 'cursor-not-allowed opacity-35'
        "
        :title="
          canReorder
            ? t('onlineSources.card.dragTitle', { domain: domainLabel })
            : t('onlineSources.card.dragDisabledTitle')
        "
      >
        <LuGripVertical class="h-4 w-4" />
      </div>

      <div class="min-w-0 flex-1">
        <div class="flex items-center gap-2">
          <span class="truncate text-sm font-semibold text-[var(--md-sys-color-on-surface)]">
            {{ source.name }}
          </span>
          <span
            v-if="source.group_name"
            class="shrink-0 rounded-full bg-[var(--md-sys-color-surface-container-highest)] px-2 py-0.5 text-[10px] font-medium text-[var(--md-sys-color-on-surface-variant)]"
          >
            {{ source.group_name }}
          </span>
        </div>

        <div class="mt-1.5 flex items-center gap-2">
          <span
            class="h-2.5 w-2.5 shrink-0 rounded-full"
            :class="statusDotClass(source)"
            :title="statusDotTitle(source)"
          />
          <span class="truncate font-mono text-[11px] text-[var(--md-sys-color-on-surface-variant)]">
            {{ formatSourceHost(source.source_url) }}
          </span>
          <span
            v-if="source.cookie_hosts.length > 0"
            class="shrink-0 text-[10px] text-[var(--md-sys-color-on-surface-variant)]/85"
          >
            · Cookie({{ source.cookie_hosts.length }})
          </span>
        </div>
      </div>
    </div>

    <div v-if="selectionMode" class="shrink-0">
      <span
        class="rounded-full px-2.5 py-0.5 text-[11px] font-medium"
        :class="
          source.enabled
            ? 'bg-[var(--md-sys-color-success-container)] text-[var(--md-sys-color-on-success-container)]'
            : 'bg-[var(--md-sys-color-surface-container-highest)] text-[var(--md-sys-color-on-surface-variant)]'
        "
      >
        {{
          source.enabled
            ? t('onlineSources.card.enabledBadge')
            : t('onlineSources.card.disabledBadge')
        }}
      </span>
    </div>

    <div v-else class="flex shrink-0 items-center gap-1.5" @mousedown.stop>
      <AppSwitch
        :model-value="source.enabled"
        :disabled="busy"
        :title="
          source.enabled
            ? t('onlineSources.card.disableAction', { domain: domainLabel })
            : t('onlineSources.card.enableAction', { domain: domainLabel })
        "
        @update:model-value="(val) => emit('toggle-enabled', val)"
      />

      <button
        type="button"
        class="md-icon-btn md-icon-btn-sm md-state"
        :title="t('onlineSources.card.editAction', { domain: domainLabel })"
        :aria-label="t('onlineSources.card.editAction', { domain: domainLabel })"
        @click="emit('edit')"
      >
        <LuPencil class="h-4 w-4" />
      </button>

      <button
        type="button"
        class="md-icon-btn md-icon-btn-sm md-state"
        :aria-expanded="menuOpen"
        aria-haspopup="menu"
        :title="t('onlineSources.card.moreAction')"
        :aria-label="t('onlineSources.card.moreAction')"
        @click.stop="emit('toggle-menu', $event)"
      >
        <LuMoreVertical class="h-4 w-4" />
      </button>
    </div>
  </div>
</template>
