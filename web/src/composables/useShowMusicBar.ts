import { computed, type ComputedRef } from 'vue'
import { useRoute } from 'vue-router'
import { usePlayerStore } from '@/domains/music/player'
import { useAuthStore } from '@/stores/auth'

export function useShowMusicBar(): ComputedRef<boolean> {
  const route = useRoute()
  const authStore = useAuthStore()
  const playerStore = usePlayerStore()

  const isFullscreenRoute = computed(() =>
    route.matched.some((record) => Boolean(record.meta.fullscreen)),
  )

  const isMusicRoute = computed(
    () => route.meta.domain === 'music' || route.path.startsWith('/music'),
  )

  return computed(
    () =>
      authStore.isAuthenticated &&
      !isFullscreenRoute.value &&
      authStore.hasModule('music') &&
      (isMusicRoute.value || playerStore.currentTrack !== null),
  )
}
