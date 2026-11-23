<script setup lang="ts">
import { computed, ref, useId } from 'vue'
import { useI18n } from 'vue-i18n'
import { LuX } from 'vue-icons-plus/lu'
import { useEscapeStack } from '@/composables/useEscapeStack'
import { useFocusTrap } from '@/composables/useFocusTrap'
import { stripAriaModalOnLeave } from '@/utils/dom'
import MdIconButton from './MdIconButton.vue'

export type MdSheetSize = 'sm' | 'md' | 'lg'

const props = withDefaults(
  defineProps<{
    open: boolean
    title?: string
    subtitle?: string
    size?: MdSheetSize
    closeOnBackdrop?: boolean
    hideClose?: boolean
    darkScheme?: boolean
    teleportTo?: string | HTMLElement | null
  }>(),
  {
    title: '',
    subtitle: '',
    size: 'md',
    closeOnBackdrop: true,
    hideClose: false,
    darkScheme: false,
    teleportTo: 'body',
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

const sizeClass = computed(() => {
  switch (props.size) {
    case 'sm':
      return 'sm:w-[360px]'
    case 'lg':
      return 'sm:w-[640px]'
    case 'md':
    default:
      return 'sm:w-[440px]'
  }
})
const resolvedTeleportTo = computed<string | HTMLElement>(() => props.teleportTo || 'body')

defineExpose({
  sheetRef,
})
</script>

<template>
  <Teleport :to="resolvedTeleportTo">
    <Transition name="md-fade" @before-leave="stripAriaModalOnLeave">
      <div
        v-if="open"
        class="fixed inset-0 z-[var(--z-sheet)] flex items-end justify-center bg-scrim/32 sm:items-stretch sm:justify-end"
        @mousedown.self="closeOnBackdrop && emit('close')"
      >
        <Transition name="md-sheet-adaptive" appear @before-leave="stripAriaModalOnLeave">
          <aside
            ref="sheetRef"
            role="dialog"
            :aria-modal="open ? 'true' : undefined"
            tabindex="-1"
            :aria-labelledby="title ? titleId : undefined"
            :aria-describedby="subtitle ? subtitleId : undefined"
            class="safe-pb flex max-h-[85dvh] w-full flex-col overflow-hidden rounded-t-corner-xl bg-surface-container-low text-on-surface shadow-elevation-1 outline-none sm:h-full sm:max-h-none sm:max-w-[calc(100vw-3rem)] sm:rounded-t-corner-none sm:rounded-l-corner-lg sm:pb-0"
            :class="[sizeClass, darkScheme ? 'md-scheme-dark' : '']"
          >
            <!-- Compact (<600px) bottom sheet drag handle -->
            <div class="flex justify-center pt-3 pb-1 sm:hidden" aria-hidden="true">
              <span class="h-1 w-8 rounded-corner-full bg-on-surface-variant/40" />
            </div>

            <!-- Sheet Header -->
            <div class="flex items-start justify-between gap-3 px-6 pt-3 pb-4 sm:pt-6">
              <div class="min-w-0">
                <h3 :id="titleId" class="text-title-large text-on-surface">
                  {{ title }}
                </h3>
                <p
                  v-if="subtitle"
                  :id="subtitleId"
                  class="text-body-small mt-1 text-on-surface-variant"
                >
                  {{ subtitle }}
                </p>
              </div>
              <div class="flex shrink-0 items-center gap-1">
                <slot name="headerActions" />
                <MdIconButton
                  v-if="!hideClose"
                  size="sm"
                  :icon="LuX"
                  :aria-label="t('common.close')"
                  @click="emit('close')"
                />
              </div>
            </div>

            <div v-if="$slots.subheader" class="px-6 pb-3">
              <slot name="subheader" />
            </div>

            <div class="flex-1 overflow-y-auto px-6 pb-6">
              <slot />
            </div>

            <div
              v-if="$slots.footer"
              class="flex items-center justify-end gap-2 bg-surface-container px-6 py-4"
            >
              <slot name="footer" />
            </div>
          </aside>
        </Transition>
      </div>
    </Transition>
  </Teleport>
</template>
