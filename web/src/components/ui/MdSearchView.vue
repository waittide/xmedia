<script setup lang="ts">
import { computed, nextTick, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { LuArrowLeft, LuLoader2, LuX } from 'vue-icons-plus/lu'
import { useEscapeStack } from '@/composables/useEscapeStack'
import { useFocusTrap } from '@/composables/useFocusTrap'
import { stripAriaModalOnLeave } from '@/utils/dom'
import MdIconButton from './MdIconButton.vue'

const props = withDefaults(
  defineProps<{
    open: boolean
    modelValue?: string
    placeholder?: string
    loading?: boolean
    ariaLabel?: string
  }>(),
  {
    modelValue: '',
    placeholder: '',
    loading: false,
    ariaLabel: undefined,
  },
)

const emit = defineEmits<{
  'update:modelValue': [value: string]
  close: []
  submit: [value: string]
  clear: []
}>()

const { t } = useI18n()
const escapeStack = useEscapeStack()
const dialogRef = ref<HTMLElement | null>(null)
const inputRef = ref<HTMLInputElement | null>(null)

useFocusTrap(dialogRef, () => props.open, { initialFocusRef: inputRef })

escapeStack.register(
  () => props.open,
  () => emit('close'),
)

watch(
  () => props.open,
  async (isOpen) => {
    if (!isOpen) return
    await nextTick()
    inputRef.value?.focus()
  },
)

const trimmedQuery = computed(() => props.modelValue.trim())
const hasNonEmptyQuery = computed(() => trimmedQuery.value.length > 0)

function onInput(ev: Event): void {
  emit('update:modelValue', (ev.target as HTMLInputElement).value)
}

function onClear(): void {
  emit('update:modelValue', '')
  emit('clear')
  inputRef.value?.focus()
}

function onKeydown(ev: KeyboardEvent): void {
  if (ev.key === 'Enter' && !ev.isComposing) {
    ev.preventDefault()
    emit('submit', props.modelValue)
  }
}

defineExpose({
  inputRef,
  focusInput: () => inputRef.value?.focus(),
})
</script>

<template>
  <Teleport to="body">
    <Transition name="md-fade" @before-leave="stripAriaModalOnLeave">
      <div
        v-if="open"
        class="fixed inset-0 z-[var(--z-modal)] flex items-start justify-center bg-scrim/32 px-0 pt-0 sm:px-4 sm:pt-[10vh]"
        @mousedown.self="emit('close')"
      >
        <Transition name="md-dialog" appear @before-leave="stripAriaModalOnLeave">
          <div
            ref="dialogRef"
            role="dialog"
            :aria-modal="open ? 'true' : undefined"
            tabindex="-1"
            :aria-label="ariaLabel || t('common.search')"
            class="flex h-dvh w-full max-w-xl flex-col overflow-hidden rounded-corner-none bg-surface-container-high text-on-surface shadow-elevation-3 outline-none sm:h-auto sm:max-h-[75dvh] sm:rounded-corner-xl"
          >
            <!-- MD3 Search View Input Header -->
            <div class="safe-pt shrink-0 border-b border-outline-variant">
              <div class="flex h-14 items-center gap-2 px-2 sm:px-3">
                <MdIconButton
                  size="sm"
                  :icon="LuArrowLeft"
                  :aria-label="t('common.close')"
                  @click="emit('close')"
                />

                <input
                  ref="inputRef"
                  :value="modelValue"
                  type="search"
                  :placeholder="placeholder || t('common.search')"
                  :aria-label="ariaLabel || placeholder || t('common.search')"
                  class="text-body-large min-w-0 flex-1 bg-transparent text-on-surface placeholder:text-on-surface-variant caret-primary outline-none [&::-webkit-search-cancel-button]:hidden"
                  @input="onInput"
                  @keydown="onKeydown"
                />

                <LuLoader2
                  v-if="loading"
                  class="h-4 w-4 shrink-0 animate-spin text-primary"
                  aria-hidden="true"
                />

                <MdIconButton
                  v-if="modelValue.length > 0"
                  size="sm"
                  :icon="LuX"
                  :aria-label="t('common.clear')"
                  @click="onClear"
                />

                <slot name="header-actions" />
              </div>
            </div>

            <!-- Scrollable Body: History, Results, and Online Quick Actions -->
            <div class="safe-pb flex-1 overflow-y-auto px-3 py-3 sm:pb-3">
              <slot>
                <div v-if="!hasNonEmptyQuery && $slots.history">
                  <slot name="history" />
                </div>

                <div v-if="$slots.results">
                  <slot name="results" />
                </div>

                <div
                  v-if="hasNonEmptyQuery && $slots['online-actions']"
                  class="mt-3 border-t border-outline-variant pt-3"
                >
                  <slot name="online-actions" />
                </div>
              </slot>
            </div>

            <div
              v-if="$slots.footer"
              class="flex shrink-0 items-center justify-between gap-2 border-t border-outline-variant bg-surface-container px-4 py-2.5"
            >
              <slot name="footer" />
            </div>
          </div>
        </Transition>
      </div>
    </Transition>
  </Teleport>
</template>
