<script setup lang="ts">
import { useThemeStore } from '@/stores/theme'
import AppDialogHost from '@/components/ui/AppDialogHost.vue'

useThemeStore()
</script>

<template>
  <RouterView />
  <AppDialogHost />
</template>
