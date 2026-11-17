<script setup lang="ts">
import { onBeforeUnmount, onMounted } from 'vue'
import { LuX } from 'vue-icons-plus/lu'

const props = defineProps<{
  open: boolean
  title: string
  maxWidthClass?: string
}>()

const emit = defineEmits<{
  close: []
}>()

function onKeydown(e: KeyboardEvent): void {
  if (e.key === 'Escape' && props.open) {
    emit('close')
  }
}

onMounted(() => window.addEventListener('keydown', onKeydown))
onBeforeUnmount(() => window.removeEventListener('keydown', onKeydown))
</script>

<template>
  <Teleport to="body">
    <div
      v-if="open"
      class="fixed inset-0 z-[9980] flex items-center justify-center bg-black/55 p-4 backdrop-blur-xs"
      @mousedown.self="emit('close')"
    >
      <div
        class="w-full overflow-hidden rounded-2xl border border-[var(--border-subtle)] bg-[var(--bg-surface)] shadow-2xl"
        :class="maxWidthClass || 'max-w-lg'"
      >
        <div class="flex items-center justify-between border-b border-[var(--border-subtle)] px-5 py-4">
          <h3 class="text-base font-semibold text-[var(--text-primary)]">{{ title }}</h3>
          <button
            type="button"
            class="rounded-lg p-1.5 text-[var(--text-secondary)] transition hover:bg-[var(--surface-hover)] hover:text-[var(--text-primary)]"
            @click="emit('close')"
          >
            <LuX class="h-4 w-4" />
          </button>
        </div>
        <div class="max-h-[80vh] overflow-y-auto p-5">
          <slot />
        </div>
        <div
          v-if="$slots.footer"
          class="flex items-center justify-end gap-2.5 border-t border-[var(--border-subtle)] bg-[var(--bg-mantle)]/45 px-5 py-3.5"
        >
          <slot name="footer" />
        </div>
      </div>
    </div>
  </Teleport>
</template>
