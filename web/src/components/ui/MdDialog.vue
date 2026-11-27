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
    showClose?: boolean
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
    showClose: false,
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
            <!-- MD3 Full-Screen Dialog Top App Bar (tonal surface container without decorative border) -->
            <div
              v-if="fullscreen && !hideHeader"
              class="flex min-h-[calc(3.5rem+var(--safe-area-top))] shrink-0 items-center justify-between gap-3 bg-surface-container pt-[calc(var(--safe-area-top)+0.5rem)] pb-2 pl-[calc(var(--safe-area-left)+1rem)] pr-[calc(var(--safe-area-right)+1rem)]"
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

            <!-- MD3 Basic Dialog Custom Header Slot -->
            <div
              v-else-if="!hideHeader && $slots.header"
              class="flex items-start justify-between gap-4 px-6 pt-6 pb-3"
            >
              <slot name="header" />

              <div
                v-if="$slots.headerActions || (showClose && !hideClose)"
                class="flex shrink-0 items-center gap-1"
              >
                <slot name="headerActions" />
                <MdIconButton
                  v-if="showClose && !hideClose"
                  size="sm"
                  :icon="LuX"
                  :aria-label="t('common.close')"
                  @click="emit('close')"
                />
              </div>
            </div>

            <!-- MD3 Basic Dialog Header (centered 24dp secondary icon + centered headline) -->
            <div
              v-else-if="
                !hideHeader &&
                (title || $slots.icon || $slots.headerActions || (showClose && !hideClose))
              "
              class="relative flex flex-col items-center px-6 pt-6 pb-3 text-center"
            >
              <div
                v-if="$slots.icon"
                class="mb-4 flex items-center justify-center text-secondary [&_svg]:h-6 [&_svg]:w-6"
              >
                <slot name="icon" />
              </div>

              <h3
                v-if="title"
                :id="titleId"
                class="text-headline-small text-center text-on-surface"
                :class="showClose && !hideClose ? 'px-8' : ''"
              >
                {{ title }}
              </h3>
              <p
                v-if="subtitle"
                :id="subtitleId"
                class="text-body-medium mt-2 text-center text-on-surface-variant"
              >
                {{ subtitle }}
              </p>

              <div
                v-if="$slots.headerActions || (showClose && !hideClose)"
                class="absolute top-4 right-4 flex items-center gap-1"
              >
                <slot name="headerActions" />
                <MdIconButton
                  v-if="showClose && !hideClose"
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
                    ? 'overflow-y-auto pt-5 pb-[calc(var(--safe-area-bottom)+1.25rem)] pl-[calc(var(--safe-area-left)+1.5rem)] pr-[calc(var(--safe-area-right)+1.5rem)]'
                    : 'overflow-y-auto px-6 py-2'
              "
            >
              <slot />
            </div>

            <!-- MD3 Dialog Action Bar -->
            <div
              v-if="$slots.footer"
              class="flex flex-wrap items-center gap-2"
              :class="[
                fullscreen
                  ? 'bg-surface-container pt-4 pb-[calc(var(--safe-area-bottom)+1rem)] pl-[calc(var(--safe-area-left)+1.5rem)] pr-[calc(var(--safe-area-right)+1.5rem)]'
                  : 'px-6 pt-3 pb-5',
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
