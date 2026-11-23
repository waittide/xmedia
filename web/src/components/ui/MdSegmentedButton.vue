<script setup lang="ts" generic="T extends string">
import { ref, type Component } from 'vue'
import { LuCheck } from 'vue-icons-plus/lu'

export interface SegmentedOption<V extends string = string> {
  value: V
  label: string
  icon?: Component
  title?: string
  disabled?: boolean
}

const props = withDefaults(
  defineProps<{
    modelValue: T
    options: readonly SegmentedOption<T>[]
    size?: 'sm' | 'md'
    showCheck?: boolean
    fullWidth?: boolean
    disabled?: boolean
    ariaLabel?: string
  }>(),
  {
    size: 'md',
    showCheck: true,
    fullWidth: false,
    disabled: false,
    ariaLabel: '',
  },
)

const emit = defineEmits<{
  'update:modelValue': [value: T]
}>()

const btnRefs = ref<(HTMLButtonElement | null)[]>([])

function setBtnRef(el: unknown, idx: number): void {
  btnRefs.value[idx] = el as HTMLButtonElement | null
}

function isOptionDisabled(opt: SegmentedOption<T>): boolean {
  return props.disabled || Boolean(opt.disabled)
}

function selectOption(opt: SegmentedOption<T>): void {
  if (isOptionDisabled(opt)) return
  emit('update:modelValue', opt.value)
}

function onKeydown(ev: KeyboardEvent, currentIdx: number): void {
  const len = props.options.length
  if (len <= 1) return
  let step = 0
  let targetIdx: number | null = null
  if (ev.key === 'ArrowRight' || ev.key === 'ArrowDown') step = 1
  else if (ev.key === 'ArrowLeft' || ev.key === 'ArrowUp') step = -1
  else if (ev.key === 'Home') targetIdx = 0
  else if (ev.key === 'End') targetIdx = len - 1
  else return

  ev.preventDefault()
  if (step !== 0) {
    for (let offset = 1; offset <= len; offset += 1) {
      const candIdx = (currentIdx + step * offset + len) % len
      const cand = props.options[candIdx]
      if (cand && !isOptionDisabled(cand)) {
        targetIdx = candIdx
        break
      }
    }
  } else if (targetIdx !== null) {
    const dir = targetIdx === 0 ? 1 : -1
    for (let offset = 0; offset < len; offset += 1) {
      const candIdx = (targetIdx + dir * offset + len) % len
      const cand = props.options[candIdx]
      if (cand && !isOptionDisabled(cand)) {
        targetIdx = candIdx
        break
      }
    }
  }

  if (targetIdx !== null) {
    const picked = props.options[targetIdx]
    if (picked && !isOptionDisabled(picked)) {
      emit('update:modelValue', picked.value)
      btnRefs.value[targetIdx]?.focus()
    }
  }
}
</script>

<template>
  <div
    role="radiogroup"
    :aria-label="ariaLabel || undefined"
    class="max-w-full items-center overflow-hidden rounded-full border border-[var(--md-sys-color-outline-variant)] bg-[var(--md-sys-color-surface-container-lowest)]/65"
    :class="[fullWidth ? 'flex w-full' : 'inline-flex', disabled ? 'opacity-38' : '']"
  >
    <button
      v-for="(opt, idx) in options"
      :key="opt.value"
      :ref="(el) => setBtnRef(el, idx)"
      type="button"
      role="radio"
      :aria-checked="modelValue === opt.value"
      :tabindex="modelValue === opt.value ? 0 : -1"
      :disabled="isOptionDisabled(opt)"
      :title="opt.title || opt.label"
      class="md-state md-touch-target inline-flex cursor-pointer items-center justify-center whitespace-nowrap font-medium transition focus-visible:z-10 disabled:cursor-not-allowed disabled:opacity-38"
      :class="[
        fullWidth ? 'min-w-0 flex-1' : '',
        size === 'sm'
          ? fullWidth
            ? 'h-9 gap-1 px-1.5 text-xs'
            : 'h-9 gap-1.5 px-3.5 text-xs'
          : fullWidth
            ? 'text-label-large h-10 gap-1.5 px-2.5'
            : 'text-label-large h-10 gap-1.5 px-4',
        idx > 0 ? 'border-l border-[var(--md-sys-color-outline-variant)]/80' : '',
        modelValue === opt.value
          ? 'bg-[var(--md-sys-color-secondary-container)] text-[var(--md-sys-color-on-secondary-container)]'
          : 'text-[var(--md-sys-color-on-surface-variant)] hover:text-[var(--md-sys-color-on-surface)]',
      ]"
      @click="selectOption(opt)"
      @keydown="onKeydown($event, idx)"
    >
      <LuCheck
        v-if="showCheck && modelValue === opt.value"
        :class="size === 'sm' ? 'h-3.5 w-3.5 shrink-0' : 'h-4 w-4 shrink-0'"
      />
      <component
        :is="opt.icon"
        v-else-if="opt.icon"
        :class="size === 'sm' ? 'h-3.5 w-3.5 shrink-0' : 'h-4 w-4 shrink-0'"
      />
      <span class="truncate">{{ opt.label }}</span>
    </button>
  </div>
</template>
