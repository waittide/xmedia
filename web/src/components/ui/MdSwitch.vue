<script setup lang="ts">
import { useI18n } from 'vue-i18n'
import { LuCheck } from 'vue-icons-plus/lu'

const props = withDefaults(
  defineProps<{
    modelValue: boolean
    label?: string
    ariaLabel?: string
    disabled?: boolean
  }>(),
  {
    label: '',
    ariaLabel: '',
    disabled: false,
  },
)

const emit = defineEmits<{
  'update:modelValue': [value: boolean]
  change: [value: boolean]
}>()

const { t, te } = useI18n()

function toggle(): void {
  if (props.disabled) return
  const next = !props.modelValue
  emit('update:modelValue', next)
  emit('change', next)
}
</script>

<template>
  <button
    type="button"
    role="switch"
    :aria-checked="modelValue"
    :aria-label="ariaLabel || (!label ? (te('common.toggle') ? t('common.toggle') : 'Toggle') : undefined)"
    :disabled="disabled"
    class="group/switch md-touch-target text-label-large inline-flex min-h-10 cursor-pointer items-center gap-3 text-on-surface select-none focus-visible:outline-none disabled:cursor-not-allowed disabled:text-on-surface/38"
    @click="toggle"
  >
    <!-- MD3 Switch Track: 52x32dp, 2dp border -->
    <span
      class="relative inline-flex h-8 w-[52px] shrink-0 items-center rounded-corner-full border-2 transition-colors duration-200 ease-standard group-focus-visible/switch:outline-3 group-focus-visible/switch:outline-offset-2 group-focus-visible/switch:outline-secondary"
      :class="
        disabled
          ? modelValue
            ? 'border-transparent bg-on-surface/12'
            : 'border-on-surface/12 bg-surface-container-highest/12'
          : modelValue
            ? 'border-primary bg-primary'
            : 'border-outline bg-surface-container-highest'
      "
    >
      <!-- MD3 Switch Thumb: 16dp off / 24dp on + State Layer Halo -->
      <span
        class="relative flex items-center justify-center rounded-corner-full transition-[transform,width,height,background-color] duration-200 ease-standard"
        :class="
          modelValue
            ? [
                'h-6 w-6 translate-x-[22px] group-active/switch:scale-110',
                disabled
                  ? 'bg-surface text-on-surface/38'
                  : 'bg-on-primary text-primary',
              ]
            : [
                'h-4 w-4 translate-x-[6px] group-active/switch:scale-125',
                disabled
                  ? 'bg-on-surface/38'
                  : 'bg-outline group-hover/switch:bg-on-surface-variant',
              ]
        "
      >
        <span
          v-if="!disabled"
          aria-hidden="true"
          class="pointer-events-none absolute -inset-2 rounded-corner-full opacity-0 transition-opacity duration-150 group-hover/switch:opacity-8 group-focus-visible/switch:opacity-10 group-active/switch:opacity-10"
          :class="modelValue ? 'bg-primary' : 'bg-on-surface'"
        />
        <LuCheck v-if="modelValue" class="h-4 w-4 stroke-[2.5]" aria-hidden="true" />
      </span>
    </span>
    <span v-if="label">{{ label }}</span>
    <slot />
  </button>
</template>
