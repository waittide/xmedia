<script setup lang="ts" generic="T extends string">
import { useI18n } from 'vue-i18n'
import {
  LuCheckCircle2,
  LuCode2,
  LuCopy,
  LuFlaskConical,
  LuLoader2,
  LuSave,
  LuXCircle,
} from 'vue-icons-plus/lu'
import MdSideSheet from '@/components/ui/MdSideSheet.vue'
import type { EditTabItem } from './useOnlineSourceEdit'

const props = defineProps<{
  open: boolean
  title: string
  subtitle?: string
  domainLabel: string
  tabs: EditTabItem<T>[]
  activeTab: T
  jsonTabId: T
  loading: boolean
  saving: boolean
  loadError: string | null
  jsonText: string
  jsonValidation: { valid: boolean; error: string | null }
}>()

const emit = defineEmits<{
  close: []
  'switch-tab': [tab: T]
  'update:jsonText': [value: string]
  'mark-json-dirty': []
  'format-json': []
  'copy-json': []
  save: [openDebugAfter: boolean]
}>()

const { t } = useI18n()
</script>

<template>
  <MdSideSheet
    :open="open"
    :title="title"
    :subtitle="subtitle"
    width-class="w-[min(42rem,100vw)]"
    @close="emit('close')"
  >
    <template #subheader>
      <div class="flex flex-wrap items-center gap-1.5 border-b border-[var(--md-sys-color-outline-variant)]/55 pb-3">
        <button
          v-for="tab in tabs"
          :key="tab.id"
          type="button"
          class="md-state inline-flex items-center gap-1.5 rounded-full px-3.5 py-1.5 text-xs font-medium transition"
          :class="
            activeTab === tab.id
              ? 'bg-[var(--md-sys-color-secondary-container)] text-[var(--md-sys-color-on-secondary-container)]'
              : 'text-[var(--md-sys-color-on-surface-variant)] hover:bg-[var(--md-sys-color-surface-container-high)]'
          "
          @click="emit('switch-tab', tab.id)"
        >
          <component :is="tab.icon" class="h-3.5 w-3.5" />
          <span>{{ tab.label }}</span>
          <span
            v-if="tab.activeDot"
            class="h-1.5 w-1.5 rounded-full bg-[var(--md-sys-color-primary)]"
          />
        </button>
      </div>
    </template>

    <div
      v-if="loading"
      class="flex items-center justify-center gap-2 py-20 text-sm text-[var(--md-sys-color-on-surface-variant)]"
    >
      <LuLoader2 class="h-4 w-4 animate-spin" />
      <span>{{ t('onlineSources.edit.loading', { domain: domainLabel }) }}</span>
    </div>

    <div
      v-else-if="loadError"
      class="rounded-2xl bg-[var(--md-sys-color-error-container)] px-4 py-3 text-xs text-[var(--md-sys-color-on-error-container)]"
    >
      {{ loadError }}
    </div>

    <div v-else class="flex flex-col gap-4 pt-1">
      <section
        v-if="activeTab === jsonTabId"
        class="flex flex-col gap-2.5 rounded-2xl bg-[var(--md-sys-color-surface-container)] p-4"
      >
        <div class="flex flex-wrap items-center justify-between gap-2">
          <div class="flex items-center gap-2">
            <span class="text-xs font-semibold text-[var(--md-sys-color-on-surface)]">
              {{ t('onlineSources.edit.jsonTitle') }}
            </span>
            <span
              v-if="jsonValidation.valid"
              class="inline-flex items-center gap-1 rounded-full bg-[var(--md-sys-color-success-container)] px-2 py-0.5 text-[10px] font-medium text-[var(--md-sys-color-on-success-container)]"
            >
              <LuCheckCircle2 class="h-3 w-3" />
              <span>{{ t('onlineSources.edit.jsonValid') }}</span>
            </span>
            <span
              v-else
              class="inline-flex items-center gap-1 rounded-full bg-[var(--md-sys-color-error-container)] px-2 py-0.5 text-[10px] font-medium text-[var(--md-sys-color-on-error-container)]"
            >
              <LuXCircle class="h-3 w-3" />
              <span>{{ t('onlineSources.edit.jsonInvalid') }}</span>
            </span>
          </div>

          <div class="flex items-center gap-1.5">
            <button
              type="button"
              class="md-btn md-btn-sm md-btn-tonal md-state"
              @click="emit('format-json')"
            >
              <LuCode2 class="h-3.5 w-3.5" />
              <span>{{ t('onlineSources.edit.formatJsonBtn') }}</span>
            </button>
            <button
              type="button"
              class="md-btn md-btn-sm md-btn-outlined md-state"
              @click="emit('copy-json')"
            >
              <LuCopy class="h-3.5 w-3.5" />
              <span>{{ t('onlineSources.edit.copyJsonBtn') }}</span>
            </button>
          </div>
        </div>

        <p
          v-if="!jsonValidation.valid && jsonValidation.error"
          class="text-[11px] text-[var(--md-sys-color-error)]"
        >
          {{ jsonValidation.error }}
        </p>

        <textarea
          :value="jsonText"
          rows="22"
          spellcheck="false"
          class="md-input w-full py-2.5 font-mono text-xs leading-5"
          @input="
            emit('update:jsonText', ($event.target as HTMLTextAreaElement).value);
            emit('mark-json-dirty')
          "
        />
      </section>

      <slot v-else />
    </div>

    <template #footer>
      <button
        type="button"
        :disabled="props.saving"
        class="md-btn md-btn-text md-state"
        @click="emit('close')"
      >
        {{ t('common.cancel') }}
      </button>
      <button
        type="button"
        :disabled="props.loading || props.saving || Boolean(props.loadError) || !props.jsonValidation.valid"
        class="md-btn md-btn-tonal md-state"
        @click="emit('save', true)"
      >
        <LuFlaskConical class="h-4 w-4" />
        <span>{{ t('onlineSources.edit.saveAndDebugBtn') }}</span>
      </button>
      <button
        type="button"
        :disabled="props.loading || props.saving || Boolean(props.loadError) || !props.jsonValidation.valid"
        class="md-btn md-btn-filled md-state"
        @click="emit('save', false)"
      >
        <LuLoader2 v-if="props.saving" class="h-4 w-4 animate-spin" />
        <LuSave v-else class="h-4 w-4" />
        <span>
          {{ props.saving ? t('onlineSources.edit.savingBtn') : t('onlineSources.edit.saveBtn') }}
        </span>
      </button>
    </template>
  </MdSideSheet>
</template>
