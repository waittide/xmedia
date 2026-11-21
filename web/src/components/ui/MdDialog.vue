<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import { LuX } from 'vue-icons-plus/lu'
import { useEscapeStack } from '@/composables/useEscapeStack'

const props = withDefaults(
  defineProps<{
    open: boolean
    title?: string
    subtitle?: string
    maxWidth?: string
    closeOnBackdrop?: boolean
    hideClose?: boolean
    footerAlign?: 'between' | 'end'
  }>(),
  {
    title: '',
    subtitle: '',
    maxWidth: 'max-w-2xl',
    closeOnBackdrop: true,
    hideClose: false,
    footerAlign: 'between',
  },
)

const emit = defineEmits<{
  close: []
}>()

const { t } = useI18n()
const escapeStack = useEscapeStack()

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
        class="fixed inset-0 z-[var(--z-modal)] flex items-center justify-center bg-[var(--md-sys-color-scrim)]/40 p-4 backdrop-blur-[2px]"
        @mousedown.self="closeOnBackdrop && emit('close')"
      >
        <Transition name="md-dialog" appear>
          <div
            role="dialog"
            aria-modal="true"
            class="flex max-h-[90vh] w-full flex-col overflow-hidden rounded-[28px] bg-[var(--md-sys-color-surface-container-high)] text-[var(--md-sys-color-on-surface)] shadow-[var(--md-sys-elevation-3)]"
            :class="maxWidth"
          >
            <!-- MD3 Dialog Header -->
            <div
              v-if="title || $slots.header || $slots.icon"
              class="flex items-start justify-between gap-4 px-6 pt-6 pb-3"
            >
              <slot name="header">
                <div class="flex min-w-0 items-start gap-3.5">
                  <div
                    v-if="$slots.icon"
                    class="flex h-10 w-10 shrink-0 items-center justify-center rounded-2xl bg-[var(--md-sys-color-secondary-container)] text-[var(--md-sys-color-on-secondary-container)]"
                  >
                    <slot name="icon" />
                  </div>
                  <div class="min-w-0">
                    <h3 class="md-title-large leading-snug tracking-tight">
                      {{ title }}
                    </h3>
                    <p
                      v-if="subtitle"
                      class="md-body-small mt-1 leading-relaxed text-[var(--md-sys-color-on-surface-variant)]"
                    >
                      {{ subtitle }}
                    </p>
                  </div>
                </div>
              </slot>

              <div class="flex shrink-0 items-center gap-1">
                <slot name="headerActions" />
                <button
                  v-if="!hideClose"
                  type="button"
                  class="md-icon-btn md-icon-btn-sm md-state"
                  :aria-label="t('common.close')"
                  @click="emit('close')"
                >
                  <LuX class="h-4 w-4" />
                </button>
              </div>
            </div>

            <!-- Scrollable Content -->
            <div class="flex-1 overflow-y-auto px-6 py-2">
              <slot />
            </div>

            <!-- MD3 Dialog Action Bar -->
            <div
              v-if="$slots.footer"
              class="flex flex-wrap items-center gap-2 px-6 pt-3 pb-5"
              :class="footerAlign === 'end' ? 'justify-end' : 'justify-between'"
            >
              <slot name="footer" />
            </div>
          </div>
        </Transition>
      </div>
    </Transition>
  </Teleport>
</template>
