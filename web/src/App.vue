<script setup lang="ts">
import { computed, onBeforeUnmount, watch } from 'vue'
import { useRoute } from 'vue-router'
import { useAuthStore } from '@/stores/auth'
import { useDomainStore } from '@/stores/domain'
import { useEventsStore } from '@/stores/events'
import { useThemeStore } from '@/stores/theme'
import { usePlayerStore } from '@/domains/music/player'
import AppDialogHost from '@/components/ui/AppDialogHost.vue'
import MdSnackbarHost from '@/components/ui/MdSnackbarHost.vue'
import PlayerBar from '@/domains/music/components/PlayerBar.vue'
import QueueDrawer from '@/domains/music/components/QueueDrawer.vue'
import LyricsOverlay from '@/domains/music/components/LyricsOverlay.vue'

useThemeStore()

const route = useRoute()
const authStore = useAuthStore()
const domainStore = useDomainStore()
const eventsStore = useEventsStore()
const playerStore = usePlayerStore()

watch(
  () => authStore.user?.id,
  (uid) => {
    if (uid) {
      eventsStore.connect()
      if (authStore.hasModule('music')) {
        void playerStore.restoreRemoteQueue(false)
      }
    } else {
      eventsStore.disconnect()
    }
  },
  { immediate: true },
)

onBeforeUnmount(() => {
  eventsStore.disconnect()
})

const isFullscreenRoute = computed(() =>
  route.matched.some((record) => Boolean(record.meta.fullscreen)),
)

const showMusicBar = computed(
  () =>
    authStore.isAuthenticated &&
    !isFullscreenRoute.value &&
    authStore.hasModule('music') &&
    (domainStore.activeDomain === 'music' || playerStore.currentTrack !== null),
)
</script>

<template>
  <RouterView />
  <template v-if="showMusicBar">
    <PlayerBar />
    <QueueDrawer />
    <LyricsOverlay />
  </template>
  <AppDialogHost />
  <MdSnackbarHost />
</template>
