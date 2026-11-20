import { computed, ref } from 'vue'
import { defineStore } from 'pinia'
import {
  appHostApi,
  type CreateServerPayload,
  type ServerProfile,
  type TestServerPayload,
  type TestServerResponse,
} from '@/api/appHost'
import { useAuthStore } from '@/stores/auth'
import { resetAllSessionState } from '@/stores/sessionReset'

export const useAppServersStore = defineStore('appServers', () => {
  const initialized = ref(false)
  const isClientApp = ref(false)
  const appVersion = ref<string | null>(null)
  const servers = ref<ServerProfile[]>([])
  const activeServerId = ref<string | null>(null)
  const loading = ref(false)

  const activeServer = computed<ServerProfile | null>(() => {
    if (!activeServerId.value) return null
    return servers.value.find((item) => item.id === activeServerId.value) ?? null
  })

  const hasActiveServer = computed(() => activeServer.value !== null)

  let bootstrapPromise: Promise<void> | null = null

  async function bootstrap(): Promise<void> {
    if (initialized.value) return
    if (bootstrapPromise) return bootstrapPromise

    bootstrapPromise = (async () => {
      try {
        const health = await appHostApi.probe()
        if (!health) {
          isClientApp.value = false
          appVersion.value = null
          servers.value = []
          activeServerId.value = null
          return
        }

        isClientApp.value = true
        appVersion.value = health.version || null
        activeServerId.value = health.active_server?.id ?? null
        if (health.active_server) {
          servers.value = [health.active_server]
        }

        try {
          await refreshServers()
        } catch {
          // Keep health probe snapshot if server list request fails transiently
        }
      } finally {
        initialized.value = true
        bootstrapPromise = null
      }
    })()

    return bootstrapPromise
  }

  async function refreshServers(): Promise<void> {
    if (!isClientApp.value) return
    loading.value = true
    try {
      const res = await appHostApi.listServers()
      servers.value = res.servers
      activeServerId.value = res.active_server_id
    } finally {
      loading.value = false
    }
  }

  function resetAuthForServerSwitch(): void {
    const authStore = useAuthStore()
    resetAllSessionState()
    authStore.clearLocalSession()
    authStore.initialized = false
    authStore.capabilities = null
    authStore.needsSetup = false
  }

  async function rebootstrapActiveServer(): Promise<'/setup' | '/login' | '/'> {
    const authStore = useAuthStore()
    resetAuthForServerSwitch()
    try {
      await authStore.bootstrap()
    } catch (err) {
      authStore.initialized = false
      throw err
    }
    if (authStore.needsSetup) return '/setup'
    return authStore.isAuthenticated ? '/' : '/login'
  }

  async function testServer(payload: TestServerPayload): Promise<TestServerResponse> {
    return appHostApi.testServer(payload)
  }

  async function addServer(
    payload: CreateServerPayload,
  ): Promise<{ server: ServerProfile; nextRoute: '/setup' | '/login' | '/' | null }> {
    const res = await appHostApi.addServer(payload)
    servers.value = res.servers
    activeServerId.value = res.active_server_id
    if (payload.activate !== false) {
      const nextRoute = await rebootstrapActiveServer()
      return { server: res.server, nextRoute }
    }
    return { server: res.server, nextRoute: null }
  }

  async function activateServer(id: string): Promise<'/setup' | '/login' | '/'> {
    const res = await appHostApi.activateServer(id)
    servers.value = res.servers
    activeServerId.value = res.active_server_id
    return rebootstrapActiveServer()
  }

  async function deleteServer(id: string): Promise<void> {
    const wasActive = activeServerId.value === id
    const res = await appHostApi.deleteServer(id)
    servers.value = res.servers
    activeServerId.value = res.active_server_id
    if (wasActive) {
      resetAuthForServerSwitch()
    }
  }

  return {
    initialized,
    isClientApp,
    appVersion,
    servers,
    activeServerId,
    activeServer,
    hasActiveServer,
    loading,
    bootstrap,
    refreshServers,
    testServer,
    addServer,
    activateServer,
    deleteServer,
  }
})
