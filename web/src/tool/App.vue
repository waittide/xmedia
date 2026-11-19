<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from 'vue'
import { LuAlertCircle, LuLock, LuRefreshCw, LuWrench } from 'vue-icons-plus/lu'
import AppDialogHost from '@/components/ui/AppDialogHost.vue'
import MdSnackbarHost from '@/components/ui/MdSnackbarHost.vue'
import { useThemeStore } from '@/stores/theme'
import { setToolToken, toolApi } from './api'
import ToolWorkspace from './components/ToolWorkspace.vue'
import { useToolText } from './i18n'
import type { ToolStatusDto } from './types'

// Initialize Catppuccin palette & dark/light class immediately
useThemeStore()
const txt = useToolText()

const booting = ref(true)
const bootError = ref<string | null>(null)
const status = ref<ToolStatusDto | null>(null)
const passwordInput = ref('')
const unlocking = ref(false)
const unlockError = ref<string | null>(null)

async function fetchStatus(): Promise<void> {
  booting.value = true
  bootError.value = null
  try {
    status.value = await toolApi.getStatus()
  } catch (err) {
    status.value = null
    bootError.value = err instanceof Error ? err.message : String(err)
  } finally {
    booting.value = false
  }
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
  setToolToken(null)
  await fetchStatus()
}

function handleUnauthorized(): void {
  void fetchStatus()
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
    class="flex h-screen w-screen items-center justify-center bg-[var(--ctp-shell)] text-xs text-[var(--ctp-subtext0)]"
  >
    <LuRefreshCw class="mr-2 h-4 w-4 animate-spin text-[var(--ctp-accent)]" />
    <span>{{ txt.boot.connecting }}</span>
  </div>

  <!-- Connection Error / Retry State -->
  <div
    v-else-if="bootError || !status"
    class="flex h-screen w-screen items-center justify-center bg-[var(--ctp-shell)] p-4 text-[var(--ctp-text)]"
  >
    <div
      class="w-full max-w-sm space-y-4 rounded-3xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] p-6 shadow-2xl"
    >
      <div class="flex items-center gap-3">
        <div
          class="flex h-11 w-11 items-center justify-center rounded-2xl bg-red-500/15 text-red-500"
        >
          <LuAlertCircle class="h-5 w-5" />
        </div>
        <div>
          <h1 class="text-base font-bold">{{ txt.boot.failedTitle }}</h1>
          <p class="text-[11px] text-[var(--ctp-subtext0)]">{{ txt.brand }}</p>
        </div>
      </div>

      <p class="text-xs leading-relaxed text-[var(--ctp-subtext0)]">
        {{ txt.boot.failedDesc }}
      </p>

      <div
        v-if="bootError"
        class="rounded-xl border border-red-500/35 bg-red-500/10 px-3 py-2 font-mono text-[11px] break-all text-red-400"
      >
        {{ bootError }}
      </div>

      <button
        type="button"
        class="flex w-full items-center justify-center gap-2 rounded-xl bg-[var(--ctp-accent)] py-2.5 text-xs font-semibold text-[var(--ctp-on-accent)] shadow-sm transition hover:opacity-95"
        @click="fetchStatus"
      >
        <LuRefreshCw class="h-3.5 w-3.5" />
        <span>{{ txt.boot.retry }}</span>
      </button>
    </div>
  </div>

  <!-- Password Gate when `--password` / `XMEDIA_TOOL_PASSWORD` is configured -->
  <div
    v-else-if="status?.requires_password && !status.authenticated"
    class="flex h-screen w-screen items-center justify-center bg-[var(--ctp-shell)] p-4 text-[var(--ctp-text)]"
  >
    <form
      class="w-full max-w-sm space-y-4 rounded-3xl border border-[var(--ctp-surface0)] bg-[var(--ctp-canvas)] p-6 shadow-2xl"
      @submit.prevent="handleUnlock"
    >
      <div class="flex items-center gap-3">
        <div
          class="flex h-11 w-11 items-center justify-center rounded-2xl bg-[var(--ctp-accent)] text-[var(--ctp-on-accent)] shadow-sm"
        >
          <LuWrench class="h-5 w-5" />
        </div>
        <div>
          <h1 class="text-base font-bold">{{ txt.unlock.title }}</h1>
          <p class="text-[11px] text-[var(--ctp-subtext0)]">v{{ status.version }}</p>
        </div>
      </div>

      <p class="text-xs leading-relaxed text-[var(--ctp-subtext0)]">
        {{ txt.unlock.subtitle }}
      </p>

      <div
        v-if="unlockError"
        class="rounded-xl border border-red-500/35 bg-red-500/10 px-3 py-2 text-xs text-red-400"
      >
        {{ unlockError }}
      </div>

      <div class="relative">
        <LuLock
          class="pointer-events-none absolute top-1/2 left-3.5 h-4 w-4 -translate-y-1/2 text-[var(--ctp-subtext0)]"
        />
        <input
          v-model="passwordInput"
          type="password"
          autofocus
          :placeholder="txt.unlock.placeholder"
          class="w-full rounded-xl border border-[var(--ctp-surface0)] bg-[var(--ctp-mantle)]/65 py-2.5 pr-3 pl-10 text-xs text-[var(--ctp-text)] outline-none focus:border-[var(--ctp-accent)]"
        />
      </div>

      <button
        type="submit"
        :disabled="!passwordInput.trim() || unlocking"
        class="flex w-full items-center justify-center gap-2 rounded-xl bg-[var(--ctp-accent)] py-2.5 text-xs font-semibold text-[var(--ctp-on-accent)] shadow-sm transition hover:opacity-95 disabled:opacity-50"
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
