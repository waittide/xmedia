<script setup lang="ts">
import { computed, ref, useId } from 'vue'
import { useI18n } from 'vue-i18n'
import { LuX } from 'vue-icons-plus/lu'
import { useEscapeStack } from '@/composables/useEscapeStack'
import { useFocusTrap } from '@/composables/useFocusTrap'
import { stripAriaModalOnLeave } from '@/utils/dom'
import MdIconButton from './MdIconButton.vue'

export type MdDialogSize = 'sm' | 'md' | 'lg'

const props = withDefaults(
  defineProps<{
    open: boolean
    title?: string
    subtitle?: string
    size?: MdDialogSize
    maxWidth?: string
    fullscreen?: boolean
    hideHeader?: boolean
    noBodyPadding?: boolean
    closeOnBackdrop?: boolean
    hideClose?: boolean
    footerAlign?: 'between' | 'end'
  }>(),
  {
    title: '',
    subtitle: '',
    size: undefined,
    maxWidth: '',
    fullscreen: false,
    hideHeader: false,
    noBodyPadding: false,
    closeOnBackdrop: true,
    hideClose: false,
    footerAlign: 'end',
  },
)

const emit = defineEmits<{
  close: []
}>()

const { t } = useI18n()
const escapeStack = useEscapeStack()
const dialogRef = ref<HTMLElement | null>(null)
const titleId = useId()
const subtitleId = useId()

useFocusTrap(dialogRef, () => props.open)

escapeStack.register(
  () => props.open,
  () => emit('close'),
)

const resolvedWidthClass = computed(() => {
  if (props.fullscreen) return ''
  if (props.maxWidth) return props.maxWidth
  switch (props.size) {
    case 'sm':
      return 'max-w-md'
    case 'md':
      return 'max-w-xl'
    case 'lg':
    default:
      return 'max-w-2xl'
  }
})
</script>

<template>
  <Teleport to="body">
    <Transition name="md-fade" @before-leave="stripAriaModalOnLeave">
      <div
        v-if="open"
        class="fixed inset-0 z-[var(--z-modal)] flex items-center justify-center bg-scrim/32"
        :class="fullscreen ? 'p-0' : 'p-4'"
        @mousedown.self="!fullscreen && closeOnBackdrop && emit('close')"
      >
        <Transition name="md-dialog" appear @before-leave="stripAriaModalOnLeave">
          <div
            ref="dialogRef"
            role="dialog"
            :aria-modal="open ? 'true' : undefined"
            tabindex="-1"
            :aria-labelledby="title ? titleId : undefined"
            :aria-describedby="subtitle ? subtitleId : undefined"
            class="flex w-full flex-col overflow-hidden text-on-surface outline-none"
            :class="
              fullscreen
                ? 'h-dvh max-h-none max-w-none rounded-corner-none bg-surface shadow-none'
                : [
                    'max-h-[90vh] rounded-corner-xl bg-surface-container-high shadow-elevation-3',
                    resolvedWidthClass,
                  ]
            "
          >
            <!-- MD3 Full-Screen Dialog Top App Bar -->
            <div
              v-if="fullscreen && !hideHeader"
              class="safe-pt safe-px flex min-h-14 shrink-0 items-center justify-between gap-3 border-b border-outline-variant bg-surface-container px-4 py-2"
            >
              <div class="flex min-w-0 items-center gap-2">
                <MdIconButton
                  v-if="!hideClose"
                  size="sm"
                  :icon="LuX"
                  :aria-label="t('common.close')"
                  @click="emit('close')"
                />
                <slot name="header">
                  <div class="min-w-0">
                    <h3 :id="titleId" class="text-title-large truncate text-on-surface">
                      {{ title }}
                    </h3>
                    <p
                      v-if="subtitle"
                      :id="subtitleId"
                      class="text-body-small truncate text-on-surface-variant"
                    >
                      {{ subtitle }}
                    </p>
                  </div>
                </slot>
              </div>
              <div class="flex shrink-0 items-center gap-2">
                <slot name="headerActions" />
              </div>
            </div>

            <!-- MD3 Basic Dialog Header -->
            <div
              v-else-if="!hideHeader && (title || $slots.header || $slots.icon)"
              class="flex items-start justify-between gap-4 px-6 pt-6 pb-3"
            >
              <slot name="header">
                <div class="flex min-w-0 items-start gap-3.5">
                  <div
                    v-if="$slots.icon"
                    class="flex h-10 w-10 shrink-0 items-center justify-center rounded-corner-lg bg-secondary-container text-on-secondary-container"
                  >
                    <slot name="icon" />
                  </div>
                  <div class="min-w-0">
                    <h3 :id="titleId" class="text-headline-small text-on-surface">
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
                </div>
              </slot>

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

            <!-- Scrollable Content -->
            <div
              class="min-h-0 flex-1"
              :class="
                noBodyPadding
                  ? 'flex flex-col overflow-hidden'
                  : fullscreen
                    ? 'safe-px safe-pb overflow-y-auto px-6 py-5'
                    : 'overflow-y-auto px-6 py-2'
              "
            >
              <slot />
            </div>

            <!-- MD3 Dialog Action Bar -->
            <div
              v-if="$slots.footer"
              class="flex flex-wrap items-center gap-2 px-6"
              :class="[
                fullscreen
                  ? 'safe-px safe-pb border-t border-outline-variant bg-surface-container py-4'
                  : 'pt-3 pb-5',
                footerAlign === 'end' ? 'justify-end' : 'justify-between',
              ]"
            >
              <slot name="footer" />
            </div>
          </div>
        </Transition>
      </div>
    </Transition>
  </Teleport>
</template>
