<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { LuX } from 'vue-icons-plus/lu'
import { useEscapeStack } from '@/composables/useEscapeStack'

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
    widthClass: 'w-full max-w-lg',
  },
)

const emit = defineEmits<{
  close: []
}>()

const { t, te, locale } = useI18n()
const escapeStack = useEscapeStack()
const closeLabel = computed(() =>
  te('common.close')
    ? t('common.close')
    : String(locale.value).toLowerCase().startsWith('zh')
      ? '关闭'
      : 'Close',
)

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
        class="fixed inset-0 z-[var(--z-sheet)] flex justify-end bg-[var(--md-sys-color-scrim)]/35 backdrop-blur-[1px]"
        @mousedown.self="emit('close')"
      >
        <Transition name="md-sheet-right" appear>
          <aside
            role="dialog"
            aria-modal="true"
            class="flex h-full flex-col overflow-hidden rounded-l-[28px] bg-[var(--md-sys-color-surface-container-low)] text-[var(--md-sys-color-on-surface)] shadow-[var(--md-sys-elevation-3)]"
            :class="widthClass"
          >
            <!-- Side Sheet Header -->
            <div class="flex items-start justify-between gap-3 px-6 pt-6 pb-4">
              <div class="min-w-0">
                <h3 class="md-title-large leading-snug tracking-tight">
                  {{ title }}
                </h3>
                <p
                  v-if="subtitle"
                  class="md-body-small mt-1 text-[var(--md-sys-color-on-surface-variant)]"
                >
                  {{ subtitle }}
                </p>
              </div>
              <button
                type="button"
                class="md-icon-btn md-icon-btn-sm md-state"
                :aria-label="closeLabel"
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
