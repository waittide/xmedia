import { computed, type ComputedRef } from 'vue'
import { useRoute } from 'vue-router'
import { usePlayerStore } from '@/domains/music/player'
import { useAuthStore } from '@/stores/auth'
import { useDomainStore } from '@/stores/domain'

export function useShowMusicBar(): ComputedRef<boolean> {
  const route = useRoute()
  const authStore = useAuthStore()
  const domainStore = useDomainStore()
  const playerStore = usePlayerStore()

  const isFullscreenRoute = computed(() =>
    route.matched.some((record) => Boolean(record.meta.fullscreen)),
  )

  return computed(
    () =>
      authStore.isAuthenticated &&
      !isFullscreenRoute.value &&
      authStore.hasModule('music') &&
      (domainStore.activeDomain === 'music' || playerStore.currentTrack !== null),
  )
}
