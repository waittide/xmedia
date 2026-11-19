<script setup lang="ts">
import { onBeforeUnmount, watch } from 'vue'
import { LuX } from 'vue-icons-plus/lu'

const props = withDefaults(
  defineProps<{
    open: boolean
    title?: string
    subtitle?: string
    widthClass?: string
  }>(),
  {
    title: '',
    subtitle: '',
    widthClass: 'w-full max-w-lg',
  },
)

const emit = defineEmits<{
  close: []
}>()

function onKeydown(e: KeyboardEvent): void {
  if (e.key === 'Escape' && props.open) {
    emit('close')
  }
}

watch(
  () => props.open,
  (isOpen) => {
    if (isOpen) {
      window.addEventListener('keydown', onKeydown)
    } else {
      window.removeEventListener('keydown', onKeydown)
    }
  },
  { immediate: true },
)

onBeforeUnmount(() => {
  window.removeEventListener('keydown', onKeydown)
})
</script>

<template>
  <Teleport to="body">
    <Transition name="md-fade">
      <div
        v-if="open"
        class="fixed inset-0 z-50 flex justify-end bg-black/38 backdrop-blur-[1px]"
        @click.self="emit('close')"
      >
        <Transition name="md-sheet-right" appear>
          <aside
            role="dialog"
            aria-modal="true"
            class="flex h-full flex-col overflow-hidden rounded-l-[28px] bg-[var(--md-sys-color-surface-container-low)] text-[var(--md-sys-color-on-surface)] shadow-[var(--md-sys-elevation-3)]"
            :class="widthClass"
          >
            <!-- Side Sheet Header -->
            <div class="flex items-start justify-between gap-3 px-6 pt-6 pb-4">
              <div class="min-w-0">
                <h3 class="text-lg leading-snug font-semibold tracking-tight">
                  {{ title }}
                </h3>
                <p
                  v-if="subtitle"
                  class="mt-1 text-xs text-[var(--md-sys-color-on-surface-variant)]"
                >
                  {{ subtitle }}
                </p>
              </div>
              <button
                type="button"
                class="md-icon-btn md-icon-btn-sm md-state"
                aria-label="关闭侧栏"
                @click="emit('close')"
              >
                <LuX class="h-4 w-4" />
              </button>
            </div>

            <div v-if="$slots.subheader" class="px-6 pb-3">
              <slot name="subheader" />
            </div>

            <div class="flex-1 overflow-y-auto px-6 pb-6">
              <slot />
            </div>

            <div
              v-if="$slots.footer"
              class="flex items-center justify-end gap-2 bg-[var(--md-sys-color-surface-container)]/60 px-6 py-4"
            >
              <slot name="footer" />
            </div>
          </aside>
        </Transition>
      </div>
    </Transition>
  </Teleport>
</template>
