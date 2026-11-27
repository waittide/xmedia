<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { LuAlertCircle, LuLock, LuRefreshCw, LuWrench } from 'vue-icons-plus/lu'
import AppDialogHost from '@/components/ui/AppDialogHost.vue'
import MdButton from '@/components/ui/MdButton.vue'
import MdCard from '@/components/ui/MdCard.vue'
import MdCircularProgress from '@/components/ui/MdCircularProgress.vue'
import MdSnackbarHost from '@/components/ui/MdSnackbarHost.vue'
import MdTextField from '@/components/ui/MdTextField.vue'
import { useThemeStore } from '@/stores/theme'
import { ensureCustomFontsLoaded } from '@/theme/customFonts'
import { toolApi } from './api'
import ToolWorkspace from './components/ToolWorkspace.vue'
import { useToolTaskStore } from './tasks'
import type { ToolStatusDto } from './types'

// Initialize MD3 theme & dark/light class immediately
useThemeStore()
const taskStore = useToolTaskStore()
const { t } = useI18n()

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
    void ensureCustomFontsLoaded(true)
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
    class="safe-px safe-py text-body-small flex min-h-dvh w-screen items-center justify-center gap-2.5 bg-surface-container text-on-surface-variant"
  >
    <MdCircularProgress :size="20" :aria-label="t('tool.boot.connecting')" />
    <span>{{ t('tool.boot.connecting') }}</span>
  </div>

  <!-- Connection Error / Retry State -->
  <div
    v-else-if="bootError || !status"
    class="flex min-h-dvh w-screen items-center justify-center bg-surface-container pt-[calc(var(--safe-area-top)+1rem)] pb-[calc(var(--safe-area-bottom)+1rem)] pl-[calc(var(--safe-area-left)+1rem)] pr-[calc(var(--safe-area-right)+1rem)] text-on-surface"
  >
    <MdCard variant="elevated" padding="lg" class="w-full max-w-sm space-y-4 rounded-corner-xl">
      <div class="flex items-center gap-3">
        <div
          class="flex h-11 w-11 items-center justify-center rounded-corner-lg bg-error-container text-on-error-container"
        >
          <LuAlertCircle class="h-5 w-5" aria-hidden="true" />
        </div>
        <div>
          <h1 class="text-title-medium text-on-surface">{{ t('tool.boot.failedTitle') }}</h1>
          <p class="text-label-small text-on-surface-variant">{{ t('tool.brand') }}</p>
        </div>
      </div>

      <p class="text-body-small text-on-surface-variant">
        {{ t('tool.boot.failedDesc') }}
      </p>

      <div
        v-if="bootError"
        role="alert"
        class="text-body-small rounded-corner-md bg-error-container px-3 py-2 font-mono break-all text-on-error-container"
      >
        {{ bootError }}
      </div>

      <MdButton variant="filled" block :icon="LuRefreshCw" @click="fetchStatus()">
        {{ t('tool.boot.retry') }}
      </MdButton>
    </MdCard>
  </div>

  <!-- Password Gate when `--password` / `XMEDIA_TOOL_PASSWORD` is configured -->
  <div
    v-else-if="status?.requires_password && !status.authenticated"
    class="flex min-h-dvh w-screen items-center justify-center bg-surface-container pt-[calc(var(--safe-area-top)+1rem)] pb-[calc(var(--safe-area-bottom)+1rem)] pl-[calc(var(--safe-area-left)+1rem)] pr-[calc(var(--safe-area-right)+1rem)] text-on-surface"
  >
    <MdCard variant="elevated" padding="lg" class="w-full max-w-sm rounded-corner-xl">
      <form class="space-y-4" @submit.prevent="handleUnlock">
        <div class="flex items-center gap-3">
          <div
            class="flex h-11 w-11 items-center justify-center rounded-corner-lg bg-primary-container text-on-primary-container"
          >
            <LuWrench class="h-5 w-5" aria-hidden="true" />
          </div>
          <div>
            <h1 class="text-title-medium text-on-surface">{{ t('tool.unlock.title') }}</h1>
            <p class="text-label-small text-on-surface-variant">v{{ status.version }}</p>
          </div>
        </div>

        <p class="text-body-small text-on-surface-variant">
          {{ t('tool.unlock.subtitle') }}
        </p>

        <div
          v-if="unlockError"
          role="alert"
          class="text-body-small rounded-corner-md bg-error-container px-3 py-2 text-on-error-container"
        >
          {{ unlockError }}
        </div>

        <MdTextField
          v-model="passwordInput"
          type="password"
          :label="t('tool.unlock.subtitle')"
          :placeholder="t('tool.unlock.placeholder')"
          autofocus
        >
          <template #leading>
            <LuLock class="h-4 w-4" aria-hidden="true" />
          </template>
        </MdTextField>

        <MdButton
          type="submit"
          variant="filled"
          block
          :loading="unlocking"
          :disabled="!passwordInput.trim()"
        >
          {{ t('tool.unlock.submit') }}
        </MdButton>
      </form>
    </MdCard>
  </div>

  <!-- Main Workbench -->
  <ToolWorkspace v-else :status="status" @lock="handleLock" />

  <AppDialogHost />
  <MdSnackbarHost />
</template>
