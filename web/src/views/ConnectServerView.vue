<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { useI18n } from 'vue-i18n'
import {
  LuAlertCircle,
  LuArrowLeft,
  LuArrowRight,
  LuCheckCircle2,
  LuLayers,
  LuLoader2,
  LuServer,
  LuShieldAlert,
  LuTrash2,
} from 'vue-icons-plus/lu'
import type { ServerProfile, TestServerResponse } from '@/api/appHost'
import ThemePopover from '@/components/ThemePopover.vue'
import AppSwitch from '@/components/ui/AppSwitch.vue'
import { useAppServersStore } from '@/stores/appServers'
import { useAuthStore } from '@/stores/auth'
import { useDialogStore } from '@/stores/dialog'
import { useSnackbarStore } from '@/stores/snackbar'

const { t } = useI18n()
const route = useRoute()
const router = useRouter()
const appServersStore = useAppServersStore()
const authStore = useAuthStore()
const dialogStore = useDialogStore()
const snackbarStore = useSnackbarStore()

const serverUrl = ref('')
const serverName = ref('')
const allowInsecureTls = ref(false)
const testing = ref(false)
const connecting = ref(false)
const activatingId = ref<string | null>(null)
const deletingId = ref<string | null>(null)
const errorMsg = ref('')
const testResult = ref<TestServerResponse | null>(null)

const isBusy = computed(
  () => testing.value || connecting.value || activatingId.value !== null || deletingId.value !== null,
)

const canGoBack = computed(() => appServersStore.hasActiveServer && !isBusy.value)

function normalizeServerInput(raw: string): string {
  const trimmed = raw.trim()
  if (!trimmed) return ''
  if (/^[a-zA-Z][a-zA-Z\d+\-.]*:\/\//.test(trimmed)) {
    return trimmed
  }
  return `http://${trimmed}`
}

function extractDefaultServerName(raw: string): string {
  const normalized = normalizeServerInput(raw)
  if (!normalized) return ''
  try {
    const parsed = new URL(normalized)
    return parsed.host || normalized
  } catch {
    return normalized.replace(/^https?:\/\//i, '').replace(/\/.*$/, '') || normalized
  }
}

watch([serverUrl, allowInsecureTls], () => {
  testResult.value = null
  if (errorMsg.value) {
    errorMsg.value = ''
  }
})

onMounted(() => {
  if (typeof route.query.error === 'string' && route.query.error.trim()) {
    errorMsg.value = route.query.error.trim()
  }
  if (appServersStore.isClientApp) {
    void appServersStore.refreshServers().catch(() => {
      // keep existing state
    })
  }
})

async function handleBack(): Promise<void> {
  if (!appServersStore.hasActiveServer) return
  if (authStore.isAuthenticated) {
    await router.replace('/')
    return
  }
  await router.replace(authStore.needsSetup ? '/setup' : '/login')
}

async function handleTest(): Promise<void> {
  const normalized = normalizeServerInput(serverUrl.value)
  if (!normalized) {
    errorMsg.value = t('server.urlRequired')
    return
  }

  errorMsg.value = ''
  testResult.value = null
  testing.value = true
  try {
    const res = await appServersStore.testServer({
      url: normalized,
      allow_insecure_tls: allowInsecureTls.value,
    })
    testResult.value = res
    if (res.ok && res.normalized_url) {
      serverUrl.value = res.normalized_url
    }
  } catch (err) {
    testResult.value = {
      ok: false,
      latency_ms: 0,
      normalized_url: normalized,
      error: err instanceof Error ? err.message : String(err),
    }
  } finally {
    testing.value = false
  }
}

async function handleConnect(): Promise<void> {
  const normalized = normalizeServerInput(serverUrl.value)
  if (!normalized) {
    errorMsg.value = t('server.urlRequired')
    return
  }

  errorMsg.value = ''
  connecting.value = true
  try {
    const name = serverName.value.trim() || extractDefaultServerName(normalized)
    const { nextRoute } = await appServersStore.addServer({
      name,
      url: normalized,
      allow_insecure_tls: allowInsecureTls.value,
      activate: true,
    })
    await router.replace(nextRoute ?? '/login')
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
  } finally {
    connecting.value = false
  }
}

async function handleSelectServer(server: ServerProfile): Promise<void> {
  if (isBusy.value) return
  if (server.id === appServersStore.activeServerId && authStore.isAuthenticated) {
    await router.replace('/')
    return
  }

  errorMsg.value = ''
  activatingId.value = server.id
  try {
    const nextRoute = await appServersStore.activateServer(server.id)
    await router.replace(nextRoute)
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
  } finally {
    activatingId.value = null
  }
}

async function handleDeleteServer(server: ServerProfile): Promise<void> {
  if (isBusy.value) return
  const ok = await dialogStore.confirm({
    title: t('server.deleteTitle'),
    message: t('server.deleteConfirm', { name: server.name }),
    danger: true,
    confirmText: t('common.delete'),
    cancelText: t('common.cancel'),
  })
  if (!ok) return

  errorMsg.value = ''
  deletingId.value = server.id
  try {
    await appServersStore.deleteServer(server.id)
    snackbarStore.show(t('server.deleted'))
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err)
  } finally {
    deletingId.value = null
  }
}
</script>

<template>
  <div
    class="safe-pb flex min-h-screen flex-col bg-[var(--md-sys-color-surface-container-low)] text-[var(--md-sys-color-on-surface)]"
  >
    <header class="safe-pt safe-px">
      <div class="flex items-center justify-between px-5 py-4 sm:px-6">
        <div class="flex items-center gap-2.5">
          <button
            v-if="canGoBack"
            type="button"
            class="md-icon-btn md-state -ml-1"
            :title="t('server.backBtn')"
            :aria-label="t('server.backBtn')"
            @click="handleBack"
          >
            <LuArrowLeft class="h-5 w-5" />
          </button>

          <div
            class="flex h-10 w-10 items-center justify-center rounded-2xl bg-[var(--md-sys-color-primary)] text-[var(--md-sys-color-on-primary)] shadow-[var(--md-sys-elevation-1)]"
          >
            <LuLayers class="h-5 w-5" />
          </div>
          <div class="flex items-baseline gap-2">
            <span class="md-title-medium tracking-tight">{{ t('app.name') }}</span>
            <span
              v-if="appServersStore.appVersion"
              class="md-label-small rounded-full bg-[var(--md-sys-color-surface-container-high)] px-2 py-0.5 font-mono text-[var(--md-sys-color-on-surface-variant)]"
            >
              v{{ appServersStore.appVersion }}
            </span>
          </div>
        </div>

        <ThemePopover show-language />
      </div>
    </header>

    <main class="safe-px flex flex-1 flex-col items-center justify-center">
      <div class="flex w-full flex-col items-center gap-5 px-4 py-6">
      <!-- Connect / Add Server Card -->
      <section
        class="w-full max-w-lg rounded-[28px] border border-[var(--md-sys-color-outline-variant)]/60 bg-[var(--md-sys-color-surface)] p-6 shadow-[var(--md-sys-elevation-2)] sm:p-8"
      >
        <div class="mb-6 flex items-center gap-3">
          <div
            class="flex h-11 w-11 shrink-0 items-center justify-center rounded-2xl bg-[var(--md-sys-color-primary-container)] text-[var(--md-sys-color-on-primary-container)]"
          >
            <LuServer class="h-5 w-5" />
          </div>
          <h1 class="md-headline-small text-[var(--md-sys-color-on-surface)]">
            {{ t('server.connectTitle') }}
          </h1>
        </div>

        <div
          v-if="errorMsg"
          class="mb-4 flex items-start gap-2 rounded-[var(--md-sys-shape-corner-md)] bg-[var(--md-sys-color-error-container)] px-4 py-3 text-xs font-medium text-[var(--md-sys-color-on-error-container)]"
        >
          <LuAlertCircle class="mt-0.5 h-4 w-4 shrink-0" />
          <span class="break-all">{{ errorMsg }}</span>
        </div>

        <form class="space-y-4" @submit.prevent="handleConnect">
          <div>
            <label
              class="md-label-medium mb-1.5 block text-[var(--md-sys-color-on-surface-variant)]"
            >
              {{ t('server.urlLabel') }}
            </label>
            <input
              v-model="serverUrl"
              type="text"
              inputmode="url"
              autocomplete="url"
              autocapitalize="off"
              spellcheck="false"
              required
              :disabled="isBusy"
              :placeholder="t('server.urlPlaceholder')"
              class="md-input h-11 w-full font-mono text-sm"
            />
          </div>

          <div>
            <label
              class="md-label-medium mb-1.5 block text-[var(--md-sys-color-on-surface-variant)]"
            >
              {{ t('server.nameLabel') }}
            </label>
            <input
              v-model="serverName"
              type="text"
              :disabled="isBusy"
              :placeholder="t('server.namePlaceholder')"
              class="md-input h-11 w-full text-sm"
            />
          </div>

          <div
            class="flex items-center justify-between gap-3 rounded-2xl bg-[var(--md-sys-color-surface-container-low)] px-4 py-3"
          >
            <AppSwitch
              v-model="allowInsecureTls"
              :disabled="isBusy"
              :label="t('server.allowInsecureTls')"
            />
          </div>

          <!-- Inline Test Result -->
          <div
            v-if="testResult"
            class="flex items-center gap-2 rounded-[var(--md-sys-shape-corner-md)] px-3.5 py-2.5 text-xs font-medium"
            :class="
              testResult.ok
                ? 'bg-[color-mix(in_srgb,var(--md-sys-color-success,#16a34a)_14%,transparent)] text-[var(--md-sys-color-success,#15803d)]'
                : 'bg-[var(--md-sys-color-error-container)] text-[var(--md-sys-color-on-error-container)]'
            "
          >
            <LuCheckCircle2 v-if="testResult.ok" class="h-4 w-4 shrink-0" />
            <LuAlertCircle v-else class="h-4 w-4 shrink-0" />
            <span class="min-w-0 flex-1 break-all">
              <template v-if="testResult.ok">
                {{ t('server.testOk', { latency: testResult.latency_ms }) }}
                <span v-if="testResult.version" class="ml-1.5 font-mono opacity-80">
                  · {{ testResult.version }}
                </span>
              </template>
              <template v-else>
                {{ testResult.error || t('server.testFailed') }}
              </template>
            </span>
          </div>

          <div class="grid grid-cols-1 gap-2.5 pt-1 sm:grid-cols-2">
            <button
              type="button"
              :disabled="isBusy || !serverUrl.trim()"
              class="md-btn md-btn-tonal h-11 w-full"
              @click="handleTest"
            >
              <LuLoader2 v-if="testing" class="h-4 w-4 animate-spin" />
              <span>{{ testing ? t('server.testingBtn') : t('server.testBtn') }}</span>
            </button>

            <button
              type="submit"
              :disabled="isBusy || !serverUrl.trim()"
              class="md-btn md-btn-filled h-11 w-full"
            >
              <LuLoader2 v-if="connecting" class="h-4 w-4 animate-spin" />
              <span>{{ connecting ? t('server.connectingBtn') : t('server.connectBtn') }}</span>
            </button>
          </div>
        </form>
      </section>

      <!-- Saved Servers List Card -->
      <section
        v-if="appServersStore.servers.length > 0"
        class="w-full max-w-lg rounded-[28px] border border-[var(--md-sys-color-outline-variant)]/60 bg-[var(--md-sys-color-surface)] p-5 shadow-[var(--md-sys-elevation-1)] sm:p-6"
      >
        <div class="mb-3.5 flex items-center justify-between px-1">
          <h2 class="md-title-small text-[var(--md-sys-color-on-surface)]">
            {{ t('server.savedServers') }}
          </h2>
          <span
            class="md-label-small rounded-full bg-[var(--md-sys-color-surface-container-high)] px-2.5 py-0.5 tabular-nums text-[var(--md-sys-color-on-surface-variant)]"
          >
            {{ appServersStore.servers.length }}
          </span>
        </div>

        <div class="space-y-2">
          <div
            v-for="item in appServersStore.servers"
            :key="item.id"
            class="flex items-center justify-between gap-3 rounded-2xl px-3.5 py-3 transition-colors"
            :class="
              item.id === appServersStore.activeServerId
                ? 'bg-[var(--md-sys-color-secondary-container)]/55 ring-1 ring-[var(--md-sys-color-primary)]/40'
                : 'bg-[var(--md-sys-color-surface-container-low)] hover:bg-[var(--md-sys-color-surface-container)]'
            "
          >
            <div class="min-w-0 flex-1">
              <div class="flex items-center gap-2">
                <span class="truncate text-sm font-semibold text-[var(--md-sys-color-on-surface)]">
                  {{ item.name || t('server.unnamed') }}
                </span>
                <span
                  v-if="item.id === appServersStore.activeServerId"
                  class="md-label-small shrink-0 rounded-full bg-[var(--md-sys-color-primary)] px-2 py-0.5 text-[var(--md-sys-color-on-primary)]"
                >
                  {{ t('server.activeBadge') }}
                </span>
                <span
                  v-if="item.allow_insecure_tls"
                  class="inline-flex shrink-0 items-center text-[var(--md-sys-color-on-surface-variant)]"
                  :title="t('server.insecureBadge')"
                >
                  <LuShieldAlert class="h-3.5 w-3.5" />
                </span>
              </div>
              <p class="mt-0.5 truncate font-mono text-xs text-[var(--md-sys-color-on-surface-variant)]">
                {{ item.url }}
              </p>
            </div>

            <div class="flex shrink-0 items-center gap-1">
              <button
                type="button"
                :disabled="isBusy"
                class="md-btn md-btn-sm"
                :class="
                  item.id === appServersStore.activeServerId
                    ? 'md-btn-tonal'
                    : 'md-btn-outlined'
                "
                @click="handleSelectServer(item)"
              >
                <LuLoader2 v-if="activatingId === item.id" class="h-3.5 w-3.5 animate-spin" />
                <template v-else>
                  <span>
                    {{
                      item.id === appServersStore.activeServerId
                        ? t('server.enterBtn')
                        : t('server.switchBtn')
                    }}
                  </span>
                  <LuArrowRight class="h-3.5 w-3.5" />
                </template>
              </button>

              <button
                type="button"
                :disabled="isBusy"
                class="md-icon-btn md-icon-btn-sm md-state hover:text-[var(--md-sys-color-error)]"
                :title="t('common.delete')"
                :aria-label="t('common.delete')"
                @click="handleDeleteServer(item)"
              >
                <LuLoader2 v-if="deletingId === item.id" class="h-4 w-4 animate-spin" />
                <LuTrash2 v-else class="h-4 w-4" />
              </button>
            </div>
          </div>
        </div>
      </section>
      </div>
    </main>
  </div>
</template>
