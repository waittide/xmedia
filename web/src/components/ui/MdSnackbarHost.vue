<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import { LuAlertCircle, LuLoader2, LuX } from 'vue-icons-plus/lu'
import { useSnackbarStore } from '@/stores/snackbar'

const { t } = useI18n()
const snackbar = useSnackbarStore()
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
          class="pointer-events-auto flex gap-2.5 overflow-hidden rounded-corner-xs bg-inverse-surface py-2.5 pr-2 pl-4 text-inverse-on-surface shadow-elevation-3"
          :class="
            snackbar.current.message.includes('\n')
              ? 'max-w-xl items-start'
              : 'min-h-12 max-w-lg items-center'
          "
        >
          <LuAlertCircle
            v-if="snackbar.current.tone === 'error'"
            class="h-4 w-4 shrink-0 text-inverse-on-surface"
            :class="snackbar.current.message.includes('\n') ? 'mt-0.5' : ''"
            aria-hidden="true"
          />

          <div
            v-if="snackbar.current.message.includes('\n')"
            class="text-body-small max-h-40 min-w-0 flex-1 overflow-y-auto whitespace-pre-wrap break-words font-mono [tab-size:2]"
          >
            {{ snackbar.current.message }}
          </div>
          <span v-else class="text-body-medium min-w-0 flex-1">{{ snackbar.current.message }}</span>

          <button
            v-if="snackbar.current.actionLabel"
            type="button"
            :disabled="snackbar.runningAction"
            class="md-state md-touch-target md-focus-inverse text-label-large inline-flex h-8 shrink-0 cursor-pointer items-center gap-1.5 rounded-corner-full px-3 text-inverse-primary transition disabled:cursor-not-allowed disabled:opacity-38"
            @click="snackbar.triggerAction()"
          >
            <LuLoader2 v-if="snackbar.runningAction" class="h-3.5 w-3.5 animate-spin" aria-hidden="true" />
            <span>{{ snackbar.current.actionLabel }}</span>
          </button>

          <button
            type="button"
            class="md-state md-touch-target md-focus-inverse flex h-8 w-8 shrink-0 cursor-pointer items-center justify-center rounded-corner-full opacity-85 transition hover:opacity-100"
            :aria-label="t('common.close')"
            @click="snackbar.dismiss()"
          >
            <LuX class="h-4 w-4" aria-hidden="true" />
          </button>
        </div>
      </Transition>
    </div>
  </Teleport>
</template>
