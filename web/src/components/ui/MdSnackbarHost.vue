<script setup lang="ts">
import { LuLoader2, LuX } from 'vue-icons-plus/lu'
import { useSnackbarStore } from '@/stores/snackbar'

const snackbar = useSnackbarStore()
</script>

<template>
  <Teleport to="body">
    <div
      class="pointer-events-none fixed inset-x-0 bottom-5 z-[70] flex justify-center px-4 sm:justify-start sm:pl-6"
    >
      <Transition name="md-snackbar" mode="out-in">
        <div
          v-if="snackbar.current"
          :key="snackbar.current.id"
          role="status"
          aria-live="polite"
          class="pointer-events-auto flex max-w-lg items-center gap-3 overflow-hidden rounded-[var(--md-sys-shape-corner-md)] border py-2.5 pr-2.5 pl-3.5 text-[13px] font-medium shadow-[var(--md-sys-elevation-3)] backdrop-blur-md"
          :class="
            snackbar.current.tone === 'error'
              ? 'border-[var(--md-sys-color-error)]/40 bg-[var(--md-sys-color-error-container)] text-[var(--md-sys-color-on-error-container)]'
              : 'border-[var(--md-sys-color-primary)]/30 bg-[var(--md-sys-color-surface-container-highest)] text-[var(--md-sys-color-on-surface)]'
          "
        >
          <span
            class="h-5 w-1 shrink-0 rounded-full"
            :class="
              snackbar.current.tone === 'error'
                ? 'bg-[var(--md-sys-color-error)]'
                : 'bg-[var(--md-sys-color-primary)]'
            "
          />
          <span class="min-w-0 flex-1 leading-snug">{{ snackbar.current.message }}</span>

          <button
            v-if="snackbar.current.actionLabel"
            type="button"
            :disabled="snackbar.runningAction"
            class="md-state inline-flex shrink-0 items-center gap-1 rounded-full px-3 py-1.5 text-xs font-semibold tracking-wide transition disabled:opacity-50"
            :class="
              snackbar.current.tone === 'error'
                ? 'bg-[var(--md-sys-color-error)]/15 text-[var(--md-sys-color-on-error-container)]'
                : 'bg-[var(--md-sys-color-primary-container)] text-[var(--md-sys-color-on-primary-container)]'
            "
            @click="snackbar.triggerAction()"
          >
            <LuLoader2 v-if="snackbar.runningAction" class="h-3.5 w-3.5 animate-spin" />
            <span>{{ snackbar.current.actionLabel }}</span>
          </button>

          <button
            type="button"
            class="md-state flex h-7 w-7 shrink-0 items-center justify-center rounded-full opacity-80 transition hover:opacity-100"
            aria-label="关闭提示"
            @click="snackbar.dismiss()"
          >
            <LuX class="h-3.5 w-3.5" />
          </button>
        </div>
      </Transition>
    </div>
  </Teleport>
</template>
