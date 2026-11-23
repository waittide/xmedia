<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from 'vue'
import { LuAlertCircle, LuLock, LuRefreshCw, LuWrench } from 'vue-icons-plus/lu'
import AppDialogHost from '@/components/ui/AppDialogHost.vue'
import MdSnackbarHost from '@/components/ui/MdSnackbarHost.vue'
import { useThemeStore } from '@/stores/theme'
import { toolApi } from './api'
import ToolWorkspace from './components/ToolWorkspace.vue'
import { useToolText } from './i18n'
import { useToolTaskStore } from './tasks'
import type { ToolStatusDto } from './types'

// Initialize MD3 theme & dark/light class immediately
useThemeStore()
const taskStore = useToolTaskStore()
const txt = useToolText()

const booting = ref(true)
const bootError = ref<string | null>(null)
const status = ref<ToolStatusDto | null>(null)
const passwordInput = ref('')
const unlocking = ref(false)
const unlockError = ref<string | null>(null)
let statusInFlight: Promise<void> | null = null

async function fetchStatus(silent = false): Promise<void> {
  if (statusInFlight) return statusInFlight
  if (!silent || !status.value) {
    booting.value = true
  }
  bootError.value = null
  const task = (async () => {
    try {
      status.value = await toolApi.getStatus()
    } catch (err) {
      status.value = null
      bootError.value = err instanceof Error ? err.message : String(err)
    } finally {
      booting.value = false
      statusInFlight = null
    }
  })()
  statusInFlight = task
  return task
}

async function handleUnlock(): Promise<void> {
  if (!passwordInput.value.trim() || unlocking.value) return
  unlocking.value = true
  unlockError.value = null
  try {
    await toolApi.unlock(passwordInput.value.trim())
    passwordInput.value = ''
    await fetchStatus()
  } catch (err) {
    unlockError.value = err instanceof Error ? err.message : String(err)
  } finally {
    unlocking.value = false
  }
}

async function handleLock(): Promise<void> {
  taskStore.resetState()
  try {
    await toolApi.lock()
  } catch {
    // ignore lock network error and still refresh status
  }
  await fetchStatus()
}

function handleUnauthorized(): void {
  taskStore.resetState()
  void fetchStatus(true)
}

onMounted(() => {
  window.addEventListener('xmedia-tool:unauthorized', handleUnauthorized)
  void fetchStatus()
})

onBeforeUnmount(() => {
  window.removeEventListener('xmedia-tool:unauthorized', handleUnauthorized)
})
</script>

<template>
  <div
    v-if="booting"
    class="safe-px safe-py flex min-h-dvh w-screen items-center justify-center bg-[var(--md-sys-color-surface-container)] text-xs text-[var(--md-sys-color-on-surface-variant)]"
  >
    <LuRefreshCw class="mr-2 h-4 w-4 animate-spin text-[var(--md-sys-color-primary)]" />
    <span>{{ txt.boot.connecting }}</span>
  </div>

  <!-- Connection Error / Retry State -->
  <div
    v-else-if="bootError || !status"
    class="safe-px safe-py flex min-h-dvh w-screen items-center justify-center bg-[var(--md-sys-color-surface-container)] p-4 text-[var(--md-sys-color-on-surface)]"
  >
    <div
      class="w-full max-w-sm space-y-4 rounded-[28px] border border-[var(--md-sys-color-outline-variant)]/50 bg-[var(--md-sys-color-surface-container-high)] p-6 shadow-[var(--md-sys-elevation-3)]"
    >
      <div class="flex items-center gap-3">
        <div
          class="flex h-11 w-11 items-center justify-center rounded-2xl bg-[var(--md-sys-color-error-container)] text-[var(--md-sys-color-on-error-container)]"
        >
          <LuAlertCircle class="h-5 w-5" />
        </div>
        <div>
          <h1 class="text-base font-bold">{{ txt.boot.failedTitle }}</h1>
          <p class="text-[11px] text-[var(--md-sys-color-on-surface-variant)]">{{ txt.brand }}</p>
        </div>
      </div>

      <p class="text-xs leading-relaxed text-[var(--md-sys-color-on-surface-variant)]">
        {{ txt.boot.failedDesc }}
      </p>

      <div
        v-if="bootError"
        class="rounded-xl bg-[var(--md-sys-color-error-container)] px-3 py-2 font-mono text-[11px] break-all text-[var(--md-sys-color-on-error-container)]"
      >
        {{ bootError }}
      </div>

      <button
        type="button"
        class="md-btn md-btn-filled md-state w-full"
        @click="fetchStatus()"
      >
        <LuRefreshCw class="h-3.5 w-3.5" />
        <span>{{ txt.boot.retry }}</span>
      </button>
    </div>
  </div>

  <!-- Password Gate when `--password` / `XMEDIA_TOOL_PASSWORD` is configured -->
  <div
    v-else-if="status?.requires_password && !status.authenticated"
    class="safe-px safe-py flex min-h-dvh w-screen items-center justify-center bg-[var(--md-sys-color-surface-container)] p-4 text-[var(--md-sys-color-on-surface)]"
  >
    <form
      class="w-full max-w-sm space-y-4 rounded-[28px] border border-[var(--md-sys-color-outline-variant)]/50 bg-[var(--md-sys-color-surface-container-high)] p-6 shadow-[var(--md-sys-elevation-3)]"
      @submit.prevent="handleUnlock"
    >
      <div class="flex items-center gap-3">
        <div
          class="flex h-11 w-11 items-center justify-center rounded-2xl bg-[var(--md-sys-color-primary-container)] text-[var(--md-sys-color-on-primary-container)]"
        >
          <LuWrench class="h-5 w-5" />
        </div>
        <div>
          <h1 class="text-base font-bold">{{ txt.unlock.title }}</h1>
          <p class="text-[11px] text-[var(--md-sys-color-on-surface-variant)]">v{{ status.version }}</p>
        </div>
      </div>

      <p class="text-xs leading-relaxed text-[var(--md-sys-color-on-surface-variant)]">
        {{ txt.unlock.subtitle }}
      </p>

      <div
        v-if="unlockError"
        class="rounded-xl bg-[var(--md-sys-color-error-container)] px-3 py-2 text-xs text-[var(--md-sys-color-on-error-container)]"
      >
        {{ unlockError }}
      </div>

      <div class="relative">
        <LuLock
          class="pointer-events-none absolute top-1/2 left-3.5 h-4 w-4 -translate-y-1/2 text-[var(--md-sys-color-on-surface-variant)]"
        />
        <input
          v-model="passwordInput"
          type="password"
          autofocus
          :placeholder="txt.unlock.placeholder"
          class="md-input h-10 w-full pr-3 pl-10 text-xs"
        />
      </div>

      <button
        type="submit"
        :disabled="!passwordInput.trim() || unlocking"
        class="md-btn md-btn-filled md-state w-full"
      >
        <LuRefreshCw v-if="unlocking" class="h-3.5 w-3.5 animate-spin" />
        <span>{{ txt.unlock.submit }}</span>
      </button>
    </form>
  </div>

  <!-- Main Workbench -->
  <ToolWorkspace v-else :status="status" @lock="handleLock" />

  <AppDialogHost />
  <MdSnackbarHost />
</template>
