import { computed, ref } from 'vue'
import { defineStore } from 'pinia'
import { api, getStoredToken, setStoredToken } from '@/api/client'
import { setHomeCacheUser } from '@/config/homeCache'
import { resetAllSessionState } from '@/stores/sessionReset'
import { useThemeStore } from '@/stores/theme'
import type { MediaKind, SystemCapabilitiesDto, UserDto } from '@/types/api'

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
    const themeStore = useThemeStore()
    try {
      const [cap, status] = await Promise.all([api.capabilities(), api.setupStatus()])
      capabilities.value = cap
      needsSetup.value = status.needs_setup
      if (!status.needs_setup && getStoredToken()) {
        try {
          const me = await api.me()
          user.value = me
          setHomeCacheUser(me.id)
          themeStore.hydrateFromUser(me.preferences)
        } catch {
          clearLocalSession()
        }
      }
    } finally {
      initialized.value = true
    }
  }

  function applyAuthenticatedUser(nextUser: UserDto): void {
    const themeStore = useThemeStore()
    if (user.value && user.value.id !== nextUser.id) {
      resetAllSessionState()
    }
    user.value = nextUser
    setHomeCacheUser(nextUser.id)
    themeStore.hydrateFromUser(nextUser.preferences)
  }

  async function setupAdmin(payload: {
    username: string
    display_name?: string
    password: string
  }): Promise<void> {
    const session = await api.setupAdmin(payload)
    resetAllSessionState()
    setStoredToken(session.token)
    needsSetup.value = false
    applyAuthenticatedUser(session.user)
  }

  async function login(username: string, password: string): Promise<void> {
    const session = await api.login({ username, password })
    resetAllSessionState()
    setStoredToken(session.token)
    applyAuthenticatedUser(session.user)
  }

  function clearLocalSession(): void {
    setStoredToken(null)
    user.value = null
    setHomeCacheUser(null)
    resetAllSessionState()
  }

  async function logout(): Promise<void> {
    try {
      await api.logout()
    } catch {
      // ignore
    }
    clearLocalSession()
  }

  function setUser(next: UserDto): void {
    user.value = next
    setHomeCacheUser(next.id)
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
    clearLocalSession,
    logout,
    setUser,
  }
})
