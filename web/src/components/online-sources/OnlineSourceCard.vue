<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import { LuGripVertical, LuKeyRound, LuMoreVertical, LuPencil } from 'vue-icons-plus/lu'
import AppCheckbox from '@/components/ui/AppCheckbox.vue'
import MdSwitch from '@/components/ui/MdSwitch.vue'
import { hasSourceAuthOrSettings, type OnlineSourceSummaryDto } from './types'

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
  login: []
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
  if (src.last_test_ok === true) return 'bg-success'
  if (src.last_test_ok === false) return 'bg-error'
  return 'bg-outline/55'
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
    class="relative flex flex-col gap-2.5 rounded-corner-lg px-4 py-3.5 transition-colors duration-150 ease-standard sm:flex-row sm:items-center sm:justify-between sm:gap-3"
    :class="[
      selectionMode
        ? selected
          ? 'cursor-pointer select-none bg-secondary-container/45 ring-1 ring-primary/50'
          : 'cursor-pointer select-none bg-surface-container-low hover:bg-surface-container'
        : 'bg-surface-container-low hover:bg-surface-container',
      !source.enabled && !selected ? 'opacity-60' : '',
      dragging ? 'opacity-40 ring-1 ring-primary/50' : '',
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
      class="pointer-events-none absolute -top-1.5 right-3 left-3 h-0.5 rounded-corner-full bg-primary"
    />
    <div
      v-if="dropIndicator === 'after'"
      class="pointer-events-none absolute right-3 -bottom-1.5 left-3 h-0.5 rounded-corner-full bg-primary"
    />

    <div class="flex min-w-0 flex-1 items-center gap-2.5">
      <div v-if="selectionMode" class="flex shrink-0 items-center">
        <AppCheckbox :model-value="selected" :aria-label="source.name" />
      </div>
      <div
        v-else-if="totalCount > 1"
        class="flex h-7 w-5 shrink-0 items-center justify-center rounded-corner-xs text-on-surface-variant transition-colors"
        :class="
          canReorder
            ? 'cursor-grab hover:text-on-surface active:cursor-grabbing'
            : 'cursor-not-allowed opacity-38'
        "
        :title="
          canReorder
            ? t('onlineSources.card.dragTitle', { domain: domainLabel })
            : t('onlineSources.card.dragDisabledTitle')
        "
      >
        <LuGripVertical class="h-4 w-4" aria-hidden="true" />
      </div>

      <div class="min-w-0 flex-1">
        <div class="flex flex-wrap items-center gap-x-2 gap-y-1">
          <span
            class="text-title-small min-w-0 truncate text-on-surface"
            :title="source.name"
          >
            {{ source.name }}
          </span>
          <span
            v-if="source.format === 'js_module'"
            class="text-label-small shrink-0 rounded-corner-full bg-primary-container px-2 py-0.5 font-mono text-on-primary-container"
          >
            {{ source.version ? `JS v${source.version}` : 'JS' }}
          </span>
          <span
            v-if="source.group_name"
            class="text-label-small max-w-full shrink-0 truncate rounded-corner-full bg-surface-container-highest px-2 py-0.5 text-on-surface-variant"
          >
            {{ source.group_name }}
          </span>
        </div>

        <div class="mt-1.5 flex flex-wrap items-center gap-2">
          <span
            class="text-label-small inline-flex shrink-0 items-center gap-1.5 rounded-corner-full bg-surface-container-high px-2 py-0.5 text-on-surface-variant"
            :title="statusDotTitle(source)"
          >
            <span
              class="h-2 w-2 shrink-0 rounded-corner-full"
              :class="statusDotClass(source)"
              aria-hidden="true"
            />
            <span>{{ statusDotTitle(source) }}</span>
          </span>
          <span class="text-body-small truncate font-mono text-on-surface-variant">
            {{ formatSourceHost(source.source_url) }}
          </span>
          <span
            v-if="source.cookie_hosts.length > 0"
            class="text-label-small shrink-0 text-on-surface-variant"
          >
            · Cookie({{ source.cookie_hosts.length }})
          </span>
        </div>
      </div>
    </div>

    <div v-if="selectionMode" class="flex shrink-0 justify-end">
      <span
        class="text-label-small rounded-corner-full px-2.5 py-0.5"
        :class="
          source.enabled
            ? 'bg-success-container text-on-success-container'
            : 'bg-surface-container-highest text-on-surface-variant'
        "
      >
        {{
          source.enabled
            ? t('onlineSources.card.enabledBadge')
            : t('onlineSources.card.disabledBadge')
        }}
      </span>
    </div>

    <div v-else class="flex shrink-0 items-center justify-end gap-1.5 pl-7 sm:pl-0" @mousedown.stop>
      <MdSwitch
        :model-value="source.enabled"
        :disabled="busy"
        :title="
          source.enabled
            ? t('onlineSources.card.disableAction', { domain: domainLabel })
            : t('onlineSources.card.enableAction', { domain: domainLabel })
        "
        :aria-label="
          `${source.name} · ${
            source.enabled
              ? t('onlineSources.card.disableAction', { domain: domainLabel })
              : t('onlineSources.card.enableAction', { domain: domainLabel })
          }`
        "
        @update:model-value="(val) => emit('toggle-enabled', val)"
      />

      <button
        v-if="hasSourceAuthOrSettings(source)"
        type="button"
        class="md-icon-btn md-icon-btn-sm md-state"
        :title="t('onlineSources.menu.login')"
        :aria-label="`${source.name} · ${t('onlineSources.menu.login')}`"
        @click="emit('login')"
      >
        <LuKeyRound class="h-4 w-4" aria-hidden="true" />
      </button>

      <button
        type="button"
        class="md-icon-btn md-icon-btn-sm md-state"
        :title="t('onlineSources.card.editAction', { domain: domainLabel })"
        :aria-label="`${source.name} · ${t('onlineSources.card.editAction', { domain: domainLabel })}`"
        @click="emit('edit')"
      >
        <LuPencil class="h-4 w-4" aria-hidden="true" />
      </button>

      <button
        type="button"
        class="md-icon-btn md-icon-btn-sm md-state"
        :aria-expanded="menuOpen"
        aria-haspopup="menu"
        :title="t('onlineSources.card.moreAction')"
        :aria-label="`${source.name} · ${t('onlineSources.card.moreAction')}`"
        @click.stop="emit('toggle-menu', $event)"
      >
        <LuMoreVertical class="h-4 w-4" aria-hidden="true" />
      </button>
    </div>
  </div>
</template>
