<script setup lang="ts">
import { ref, useId } from 'vue'
import { useI18n } from 'vue-i18n'
import { LuX } from 'vue-icons-plus/lu'
import { useEscapeStack } from '@/composables/useEscapeStack'
import { useFocusTrap } from '@/composables/useFocusTrap'

const props = withDefaults(
  defineProps<{
    open: boolean
    title?: string
    subtitle?: string
    widthClass?: string
  }>(),
  {
    title: '',
    subtitle: '',
    widthClass: 'w-full max-w-[400px]',
  },
)

const emit = defineEmits<{
  close: []
}>()

const { t } = useI18n()
const escapeStack = useEscapeStack()
const sheetRef = ref<HTMLElement | null>(null)
const titleId = useId()
const subtitleId = useId()

useFocusTrap(sheetRef, () => props.open)

escapeStack.register(
  () => props.open,
  () => emit('close'),
)
</script>

<template>
  <Teleport to="body">
    <Transition name="md-fade">
      <div
        v-if="open"
        class="fixed inset-0 z-[var(--z-sheet)] flex items-end justify-center bg-[var(--md-sys-color-scrim)]/32 sm:items-stretch sm:justify-end"
        @mousedown.self="emit('close')"
      >
        <Transition name="md-sheet-right" appear>
          <aside
            ref="sheetRef"
            role="dialog"
            aria-modal="true"
            tabindex="-1"
            :aria-labelledby="title ? titleId : undefined"
            :aria-describedby="subtitle ? subtitleId : undefined"
            class="safe-pb flex max-h-[85dvh] w-full flex-col overflow-hidden rounded-t-[var(--md-sys-shape-corner-xl)] bg-[var(--md-sys-color-surface-container-low)] text-[var(--md-sys-color-on-surface)] shadow-[var(--md-sys-elevation-1)] outline-none sm:h-full sm:max-h-none sm:rounded-t-none sm:rounded-l-[var(--md-sys-shape-corner-lg)] sm:pb-0"
            :class="widthClass"
          >
            <!-- Compact (<600px) Bottom Sheet Drag Handle -->
            <div class="flex justify-center pt-3 pb-1 sm:hidden" aria-hidden="true">
              <span class="h-1 w-8 rounded-full bg-[var(--md-sys-color-on-surface-variant)]/40" />
            </div>

            <!-- Side Sheet Header -->
            <div class="flex items-start justify-between gap-3 px-6 pt-3 pb-4 sm:pt-6">
              <div class="min-w-0">
                <h3 :id="titleId" class="md-title-large">
                  {{ title }}
                </h3>
                <p
                  v-if="subtitle"
                  :id="subtitleId"
                  class="md-body-small mt-1 text-[var(--md-sys-color-on-surface-variant)]"
                >
                  {{ subtitle }}
                </p>
              </div>
              <button
                type="button"
                class="md-icon-btn md-icon-btn-sm md-state"
                :aria-label="t('common.close')"
                @click="emit('close')"
              >
                <LuX class="h-4 w-4" />
              </button>
            </div>

            <div v-if="$slots.subheader" class="px-6 pb-3">
              <slot name="subheader" />
            </div>

            <div class="flex-1 overflow-y-auto px-6 pb-6">
              <slot />
            </div>

            <div
              v-if="$slots.footer"
              class="flex items-center justify-end gap-2 bg-[var(--md-sys-color-surface-container)]/60 px-6 py-4"
            >
              <slot name="footer" />
            </div>
          </aside>
        </Transition>
      </div>
    </Transition>
  </Teleport>
</template>
