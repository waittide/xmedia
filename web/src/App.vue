<script setup lang="ts">
import { onBeforeUnmount, onMounted, watch } from 'vue'
import { useActivityTracker } from '@/composables/useActivityTracker'
import { useAuthStore } from '@/stores/auth'
import { useEventsStore } from '@/stores/events'
import { useThemeStore } from '@/stores/theme'
import { usePlayerStore } from '@/domains/music/player'
import AppDialogHost from '@/components/ui/AppDialogHost.vue'
import MdSnackbarHost from '@/components/ui/MdSnackbarHost.vue'

useThemeStore()
useActivityTracker()

const authStore = useAuthStore()
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

function handlePageHide(): void {
  eventsStore.disconnect()
}

function handlePageShow(event: PageTransitionEvent): void {
  if (event.persisted && authStore.isAuthenticated) {
    eventsStore.connect()
  }
}

onMounted(() => {
  window.addEventListener('pagehide', handlePageHide)
  window.addEventListener('pageshow', handlePageShow)
})

onBeforeUnmount(() => {
  window.removeEventListener('pagehide', handlePageHide)
  window.removeEventListener('pageshow', handlePageShow)
  eventsStore.disconnect()
})
</script>

<template>
  <RouterView />
  <AppDialogHost />
  <MdSnackbarHost />
</template>
