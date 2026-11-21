<script setup lang="ts">
import { nextTick, ref, useId, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useEscapeStack } from '@/composables/useEscapeStack'
import { useFocusTrap } from '@/composables/useFocusTrap'
import { useDialogStore } from '@/stores/dialog'

const { t } = useI18n()
const dialogStore = useDialogStore()
const escapeStack = useEscapeStack()
const dialogRef = ref<HTMLElement | null>(null)
const promptInputRef = ref<HTMLInputElement | null>(null)
const titleId = useId()
const descId = useId()

useFocusTrap(dialogRef, () => Boolean(dialogStore.active), {
  initialFocusRef: promptInputRef,
})

escapeStack.register(
  () => Boolean(dialogStore.active),
  () => dialogStore.cancel(),
)

watch(
  () => dialogStore.active,
  async (val) => {
    if (val?.mode === 'prompt') {
      await nextTick()
      promptInputRef.value?.focus()
      promptInputRef.value?.select()
    }
  },
  { immediate: true },
)
</script>

<template>
  <Teleport to="body">
    <Transition name="md-fade">
      <div
        v-if="dialogStore.active"
        class="fixed inset-0 z-[var(--z-confirm)] flex items-center justify-center bg-[var(--md-sys-color-scrim)]/32 p-4"
        @mousedown.self="dialogStore.cancel()"
      >
        <Transition name="md-dialog" appear>
          <form
            ref="dialogRef"
            role="dialog"
            aria-modal="true"
            tabindex="-1"
            :aria-labelledby="dialogStore.active.title ? titleId : undefined"
            :aria-describedby="dialogStore.active.message ? descId : undefined"
            class="w-full max-w-sm rounded-[28px] bg-[var(--md-sys-color-surface-container-high)] p-6 text-[var(--md-sys-color-on-surface)] shadow-[var(--md-sys-elevation-3)] outline-none"
            @submit.prevent="dialogStore.submit()"
          >
            <h3 v-if="dialogStore.active.title" :id="titleId" class="md-headline-small mb-2">
              {{ dialogStore.active.title }}
            </h3>
            <p :id="descId" class="md-body-medium text-[var(--md-sys-color-on-surface-variant)]">
              {{ dialogStore.active.message }}
            </p>

            <input
              v-if="dialogStore.active.mode === 'prompt'"
              ref="promptInputRef"
              v-model="dialogStore.inputValue"
              type="text"
              :placeholder="dialogStore.active.placeholder"
              class="md-input mt-4 h-11 w-full px-3.5 text-sm"
            />

            <div class="mt-6 flex justify-end gap-2">
              <button
                type="button"
                class="md-btn md-btn-sm md-btn-text md-state"
                @click="dialogStore.cancel()"
              >
                {{ dialogStore.active.cancelText || t('common.cancel') }}
              </button>
              <button
                type="submit"
                class="md-btn md-btn-sm md-state"
                :class="
                  dialogStore.active.danger
                    ? 'bg-[var(--md-sys-color-error)] text-[var(--md-sys-color-on-error)]'
                    : 'md-btn-filled'
                "
              >
                {{
                  dialogStore.active.confirmText ||
                  (dialogStore.active.danger ? t('common.delete') : t('common.confirm'))
                }}
              </button>
            </div>
          </form>
        </Transition>
      </div>
    </Transition>
  </Teleport>
</template>
