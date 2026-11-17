import { computed, ref } from 'vue'
import { defineStore } from 'pinia'
import { api, getStoredToken, setStoredToken } from '@/api/client'
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
          themeStore.hydrateFromUser(me.preferences)
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
    const themeStore = useThemeStore()
    const session = await api.setupAdmin(payload)
    setStoredToken(session.token)
    user.value = session.user
    needsSetup.value = false
    themeStore.hydrateFromUser(session.user.preferences)
  }

  async function login(username: string, password: string): Promise<void> {
    const themeStore = useThemeStore()
    const session = await api.login({ username, password })
    setStoredToken(session.token)
    user.value = session.user
    themeStore.hydrateFromUser(session.user.preferences)
  }

  async function logout(): Promise<void> {
    try {
      await api.logout()
    } catch {
      // ignore
    }
    setStoredToken(null)
    user.value = null
  }

  function setUser(next: UserDto): void {
    user.value = next
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
    setUser,
  }
})
