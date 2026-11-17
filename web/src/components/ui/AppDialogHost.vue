<script setup lang="ts">
import { nextTick, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { useDialogStore } from '@/stores/dialog'

const { t } = useI18n()
const dialogStore = useDialogStore()
const promptInputRef = ref<HTMLInputElement | null>(null)

watch(
  () => dialogStore.active,
  async (val) => {
    if (val?.mode === 'prompt') {
      await nextTick()
      promptInputRef.value?.focus()
      promptInputRef.value?.select()
    }
  },
)
</script>

<template>
  <Teleport to="body">
    <div
      v-if="dialogStore.active"
      class="fixed inset-0 z-[10000] flex items-center justify-center bg-black/50 p-4 backdrop-blur-[2px]"
      @mousedown.self="dialogStore.cancel()"
    >
      <form
        class="w-full max-w-sm rounded-3xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] p-6 text-[var(--ctp-text)] shadow-2xl"
        @submit.prevent="dialogStore.submit()"
      >
        <h3 v-if="dialogStore.active.title" class="mb-1.5 text-base font-bold">
          {{ dialogStore.active.title }}
        </h3>
        <p class="text-sm leading-relaxed text-[var(--ctp-subtext1)]">
          {{ dialogStore.active.message }}
        </p>

        <input
          v-if="dialogStore.active.mode === 'prompt'"
          ref="promptInputRef"
          v-model="dialogStore.inputValue"
          type="text"
          :placeholder="dialogStore.active.placeholder"
          class="mt-4 w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)] px-3.5 py-2 text-sm text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
        />

        <div class="mt-6 flex justify-end gap-2">
          <button
            type="button"
            class="rounded-xl px-4 py-2 text-xs font-medium text-[var(--ctp-subtext1)] transition hover:bg-[var(--ctp-surface0)] hover:text-[var(--ctp-text)]"
            @click="dialogStore.cancel()"
          >
            {{ dialogStore.active.cancelText || t('common.cancel') }}
          </button>
          <button
            type="submit"
            class="rounded-xl px-5 py-2 text-xs font-semibold shadow-sm transition hover:opacity-90"
            :class="
              dialogStore.active.danger
                ? 'bg-[var(--ctp-red)] text-white'
                : 'bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)]'
            "
          >
            {{
              dialogStore.active.confirmText ||
              (dialogStore.active.danger ? t('common.delete') : t('common.confirm'))
            }}
          </button>
        </div>
      </form>
    </div>
  </Teleport>
</template>
