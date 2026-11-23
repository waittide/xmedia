<script setup lang="ts">
import { computed, nextTick, ref, useId, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useEscapeStack } from '@/composables/useEscapeStack'
import { useFocusTrap } from '@/composables/useFocusTrap'
import { useDialogStore } from '@/stores/dialog'
import { stripAriaModalOnLeave } from '@/utils/dom'
import MdButton from './MdButton.vue'
import MdTextField from './MdTextField.vue'

const { t } = useI18n()
const dialogStore = useDialogStore()
const escapeStack = useEscapeStack()
const dialogRef = ref<HTMLElement | null>(null)
const promptFieldRef = ref<InstanceType<typeof MdTextField> | null>(null)
const promptInputRef = computed<HTMLElement | null>(() => promptFieldRef.value?.inputRef ?? null)
const titleId = useId()
const descId = useId()

useFocusTrap(dialogRef, () => Boolean(dialogStore.active), {
  initialFocusRef: promptInputRef,
  focusKey: () => dialogStore.active,
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
      promptFieldRef.value?.focus()
      promptFieldRef.value?.select()
    }
  },
  { immediate: true },
)
</script>

<template>
  <Teleport to="body">
    <Transition name="md-fade" @before-leave="stripAriaModalOnLeave">
      <div
        v-if="dialogStore.active"
        class="fixed inset-0 z-[var(--z-confirm)] flex items-center justify-center bg-scrim/32 p-4"
        @mousedown.self="dialogStore.cancel()"
      >
        <Transition name="md-dialog" appear @before-leave="stripAriaModalOnLeave">
          <form
            ref="dialogRef"
            role="dialog"
            :aria-modal="dialogStore.active ? 'true' : undefined"
            tabindex="-1"
            :aria-labelledby="dialogStore.active.title ? titleId : undefined"
            :aria-describedby="dialogStore.active.message ? descId : undefined"
            class="w-full max-w-sm rounded-corner-xl bg-surface-container-high p-6 text-on-surface shadow-elevation-3 outline-none"
            @submit.prevent="dialogStore.submit()"
          >
            <h3 v-if="dialogStore.active.title" :id="titleId" class="text-headline-small mb-2">
              {{ dialogStore.active.title }}
            </h3>
            <p :id="descId" class="text-body-medium text-on-surface-variant">
              {{ dialogStore.active.message }}
            </p>

            <div v-if="dialogStore.active.mode === 'prompt'" class="mt-4">
              <MdTextField
                ref="promptFieldRef"
                v-model="dialogStore.inputValue"
                :placeholder="dialogStore.active.placeholder"
                :aria-label="dialogStore.active.placeholder || dialogStore.active.title || t('common.confirm')"
              />
            </div>

            <div class="mt-6 flex justify-end gap-2">
              <MdButton variant="text" size="sm" @click="dialogStore.cancel()">
                {{ dialogStore.active.cancelText || t('common.cancel') }}
              </MdButton>
              <MdButton
                type="submit"
                variant="filled"
                size="sm"
                :tone="dialogStore.active.danger ? 'error' : 'primary'"
              >
                {{
                  dialogStore.active.confirmText ||
                  (dialogStore.active.danger ? t('common.delete') : t('common.confirm'))
                }}
              </MdButton>
            </div>
          </form>
        </Transition>
      </div>
    </Transition>
  </Teleport>
</template>
