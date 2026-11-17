import { computed, ref } from 'vue'
import { defineStore } from 'pinia'
import { api, getStoredToken, setStoredToken } from '@/api/client'
import type { MediaKind, SystemCapabilitiesDto, UserDto, UserPreferences } from '@/types/api'

export const useAuthStore = defineStore('auth', () => {
  const user = ref<UserDto | null>(null)
  const capabilities = ref<SystemCapabilitiesDto | null>(null)
  const needsSetup = ref(false)
  const initialized = ref(false)

  const isAuthenticated = computed(() => user.value !== null)
  const isAdmin = computed(() => user.value?.role === 'admin')
  const enabledModules = computed<MediaKind[]>(
    () => capabilities.value?.modules ?? ['book', 'comic', 'music', 'video'],
  )

  function hasModule(kind: MediaKind): boolean {
    return enabledModules.value.includes(kind)
  }

  async function bootstrap(): Promise<void> {
    if (initialized.value) return
    try {
      const [cap, status] = await Promise.all([api.capabilities(), api.setupStatus()])
      capabilities.value = cap
      needsSetup.value = status.needs_setup
      if (!status.needs_setup && getStoredToken()) {
        try {
          user.value = await api.getMe()
        } catch {
          setStoredToken(null)
          user.value = null
        }
      }
    } finally {
      initialized.value = true
    }
  }

  async function setupAdmin(payload: {
    username: string
    display_name?: string
    password: string
  }): Promise<void> {
    const session = await api.setupAdmin(payload)
    setStoredToken(session.token)
    user.value = session.user
    needsSetup.value = false
  }

  async function login(username: string, password: string): Promise<void> {
    const session = await api.login({ username, password })
    setStoredToken(session.token)
    user.value = session.user
  }

  async function logout(): Promise<void> {
    try {
      await api.logout()
    } catch {
      // ignore network error on logout
    }
    setStoredToken(null)
    user.value = null
  }

  async function savePreferences(prefs: UserPreferences): Promise<void> {
    if (!user.value) return
    const updated = await api.updatePreferences(prefs)
    user.value = updated
  }

  async function rotateApiKey(): Promise<void> {
    const updated = await api.rotateApiKey()
    user.value = updated
  }

  return {
    user,
    capabilities,
    needsSetup,
    initialized,
    isAuthenticated,
    isAdmin,
    enabledModules,
    hasModule,
    bootstrap,
    setupAdmin,
    login,
    logout,
    savePreferences,
    rotateApiKey,
  }
})
