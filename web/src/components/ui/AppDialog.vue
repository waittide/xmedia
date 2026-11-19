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
    <Transition name="md-fade">
      <div
        v-if="open"
        class="fixed inset-0 z-[9980] flex items-center justify-center bg-black/45 p-4 backdrop-blur-[2px]"
        @mousedown.self="emit('close')"
      >
        <Transition name="md-dialog" appear>
          <div
            role="dialog"
            aria-modal="true"
            class="w-full overflow-hidden rounded-[28px] bg-[var(--md-sys-color-surface-container-high)] text-[var(--md-sys-color-on-surface)] shadow-[var(--md-sys-elevation-3)]"
            :class="maxWidthClass || 'max-w-lg'"
          >
            <div class="flex items-center justify-between px-6 pt-6 pb-3">
              <h3 class="text-lg font-semibold tracking-tight">{{ title }}</h3>
              <button
                type="button"
                class="md-icon-btn md-icon-btn-sm md-state"
                @click="emit('close')"
              >
                <LuX class="h-4 w-4" />
              </button>
            </div>
            <div class="max-h-[78vh] overflow-y-auto px-6 py-2">
              <slot />
            </div>
            <div
              v-if="$slots.footer"
              class="flex items-center justify-end gap-2 px-6 pt-3 pb-5"
            >
              <slot name="footer" />
            </div>
          </div>
        </Transition>
      </div>
    </Transition>
  </Teleport>
</template>
