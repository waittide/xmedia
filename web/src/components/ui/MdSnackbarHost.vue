<script setup lang="ts">
import { computed } from 'vue'
import { useI18n } from 'vue-i18n'
import { LuLoader2, LuX } from 'vue-icons-plus/lu'
import { useSnackbarStore } from '@/stores/snackbar'

const { t, te, locale } = useI18n()
const snackbar = useSnackbarStore()

const closeLabel = computed(() =>
  te('common.close')
    ? t('common.close')
    : String(locale.value).toLowerCase().startsWith('zh')
      ? '关闭'
      : 'Close',
)
</script>

<template>
  <Teleport to="body">
    <div
      class="pointer-events-none fixed inset-x-0 bottom-[calc(var(--md-snackbar-bottom,5.75rem)+env(safe-area-inset-bottom,0px))] z-[var(--z-snackbar)] flex justify-center px-4 sm:justify-start sm:pl-6"
    >
      <Transition name="md-snackbar" mode="out-in">
        <div
          v-if="snackbar.current"
          :key="snackbar.current.id"
          role="status"
          aria-live="polite"
          class="pointer-events-auto flex gap-2 overflow-hidden rounded-[var(--md-sys-shape-corner-xs)] py-2.5 pr-2 pl-4 shadow-[var(--md-sys-elevation-3)]"
          :class="[
            snackbar.current.message.includes('\n')
              ? 'max-w-2xl items-start'
              : 'min-h-12 max-w-lg items-center',
            snackbar.current.tone === 'error'
              ? 'bg-[var(--md-sys-color-error-container)] text-[var(--md-sys-color-on-error-container)]'
              : 'bg-[var(--md-sys-color-inverse-surface)] text-[var(--md-sys-color-inverse-on-surface)]',
          ]"
        >
          <div
            v-if="snackbar.current.message.includes('\n')"
            class="max-h-[60vh] min-w-0 flex-1 overflow-y-auto whitespace-pre-wrap break-words font-mono text-xs leading-relaxed [tab-size:2]"
          >
            {{ snackbar.current.message }}
          </div>
          <span v-else class="md-body-medium min-w-0 flex-1">{{ snackbar.current.message }}</span>

          <button
            v-if="snackbar.current.actionLabel"
            type="button"
            :disabled="snackbar.runningAction"
            class="md-state md-label-large inline-flex h-8 shrink-0 items-center gap-1.5 rounded-full px-3 transition disabled:opacity-50"
            :class="
              snackbar.current.tone === 'error'
                ? 'text-[var(--md-sys-color-on-error-container)]'
                : 'text-[var(--md-sys-color-inverse-primary)]'
            "
            @click="snackbar.triggerAction()"
          >
            <LuLoader2 v-if="snackbar.runningAction" class="h-3.5 w-3.5 animate-spin" />
            <span>{{ snackbar.current.actionLabel }}</span>
          </button>

          <button
            type="button"
            class="md-state flex h-8 w-8 shrink-0 items-center justify-center rounded-full opacity-85 transition hover:opacity-100"
            :aria-label="closeLabel"
            @click="snackbar.dismiss()"
          >
            <LuX class="h-4 w-4" />
          </button>
        </div>
      </Transition>
    </div>
  </Teleport>
</template>
