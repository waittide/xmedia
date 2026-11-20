<script setup lang="ts">
import MdDialog from './MdDialog.vue'

defineProps<{
  open: boolean
  title: string
  maxWidthClass?: string
}>()

const emit = defineEmits<{
  close: []
}>()
</script>

<template>
  <MdDialog
    :open="open"
    :title="title"
    :max-width="maxWidthClass || 'max-w-lg'"
    footer-align="end"
    @close="emit('close')"
  >
    <slot />
    <template v-if="$slots.footer" #footer>
      <slot name="footer" />
    </template>
  </MdDialog>
</template>
