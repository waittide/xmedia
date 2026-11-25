<script setup lang="ts" generic="T extends string">
import { computed } from 'vue'
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
import {
  MdButton,
  MdDialog,
  MdTabs,
  MdTextField,
  type MdTabItem,
} from '@/components/ui'
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

const mdTabs = computed<MdTabItem[]>(() =>
  props.tabs.map((tab) => ({
    value: tab.id,
    label: tab.label,
    icon: tab.icon,
    badge: tab.activeDot ? '•' : undefined,
  })),
)

function handleTabUpdate(next: string): void {
  emit('switch-tab', next as T)
}

function handleJsonInput(val: string): void {
  emit('update:jsonText', val)
  emit('mark-json-dirty')
}
</script>

<template>
  <MdDialog
    :open="open"
    :title="title"
    :subtitle="subtitle"
    fullscreen
    @close="emit('close')"
  >
    <div class="mx-auto flex w-full max-w-4xl flex-col gap-4">
      <MdTabs
        :model-value="activeTab"
        :tabs="mdTabs"
        variant="secondary"
        @update:model-value="handleTabUpdate"
      />

      <div
        v-if="loading"
        class="text-body-medium flex items-center justify-center gap-2 py-20 text-on-surface-variant"
      >
        <LuLoader2 class="h-4 w-4 animate-spin" aria-hidden="true" />
        <span>{{ t('onlineSources.edit.loading', { domain: domainLabel }) }}</span>
      </div>

      <div
        v-else-if="loadError"
        role="alert"
        class="text-body-small rounded-corner-lg bg-error-container px-4 py-3 text-on-error-container"
      >
        {{ loadError }}
      </div>

      <div v-else class="flex flex-col gap-4 pt-1">
        <section
          v-if="activeTab === jsonTabId"
          class="flex flex-col gap-3 rounded-corner-lg bg-surface-container p-4"
        >
          <div class="flex flex-wrap items-center justify-between gap-2">
            <div class="flex items-center gap-2">
              <span class="text-title-small text-on-surface">
                {{ t('onlineSources.edit.jsonTitle') }}
              </span>
              <span
                v-if="jsonValidation.valid"
                class="text-label-small inline-flex items-center gap-1 rounded-corner-full bg-success-container px-2.5 py-0.5 text-on-success-container"
              >
                <LuCheckCircle2 class="h-3.5 w-3.5" aria-hidden="true" />
                <span>{{ t('onlineSources.edit.jsonValid') }}</span>
              </span>
              <span
                v-else
                class="text-label-small inline-flex items-center gap-1 rounded-corner-full bg-error-container px-2.5 py-0.5 text-on-error-container"
              >
                <LuXCircle class="h-3.5 w-3.5" aria-hidden="true" />
                <span>{{ t('onlineSources.edit.jsonInvalid') }}</span>
              </span>
            </div>

            <div class="flex items-center gap-2">
              <MdButton
                variant="tonal"
                :icon="LuCode2"
                @click="emit('format-json')"
              >
                {{ t('onlineSources.edit.formatJsonBtn') }}
              </MdButton>
              <MdButton
                variant="outlined"
                :icon="LuCopy"
                @click="emit('copy-json')"
              >
                {{ t('onlineSources.edit.copyJsonBtn') }}
              </MdButton>
            </div>
          </div>

          <p
            v-if="!jsonValidation.valid && jsonValidation.error"
            role="alert"
            class="text-body-small text-error"
          >
            {{ jsonValidation.error }}
          </p>

          <MdTextField
            :model-value="jsonText"
            multiline
            :rows="22"
            :aria-label="t('onlineSources.edit.jsonTitle')"
            :error="!jsonValidation.valid"
            class="font-mono"
            @update:model-value="handleJsonInput"
          />
        </section>

        <slot v-else />
      </div>
    </div>

    <template #footer>
      <MdButton variant="text" :disabled="props.saving" @click="emit('close')">
        {{ t('common.cancel') }}
      </MdButton>
      <MdButton
        variant="tonal"
        :icon="LuFlaskConical"
        :disabled="props.loading || props.saving || Boolean(props.loadError) || !props.jsonValidation.valid"
        @click="emit('save', true)"
      >
        {{ t('onlineSources.edit.saveAndDebugBtn') }}
      </MdButton>
      <MdButton
        variant="filled"
        :icon="LuSave"
        :loading="props.saving"
        :disabled="props.loading || props.saving || Boolean(props.loadError) || !props.jsonValidation.valid"
        @click="emit('save', false)"
      >
        {{ props.saving ? t('onlineSources.edit.savingBtn') : t('onlineSources.edit.saveBtn') }}
      </MdButton>
    </template>
  </MdDialog>
</template>
