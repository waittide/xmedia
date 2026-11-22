<script setup lang="ts">
import { computed, ref, useId } from 'vue'
import { useI18n } from 'vue-i18n'
import { LuSearch, LuX } from 'vue-icons-plus/lu'

export type MdTextFieldVariant = 'outlined' | 'filled'
export type MdTextFieldSize = 'sm' | 'md'

const props = withDefaults(
  defineProps<{
    modelValue?: string | number | null
    variant?: MdTextFieldVariant
    size?: MdTextFieldSize
    label?: string
    placeholder?: string
    supportingText?: string
    error?: string | boolean
    type?: string
    clearable?: boolean
    disabled?: boolean
    readonly?: boolean
    required?: boolean
    autocomplete?: string
    name?: string
    maxlength?: number | string
    min?: number | string
    max?: number | string
    step?: number | string
    inputmode?: 'none' | 'text' | 'tel' | 'url' | 'email' | 'numeric' | 'decimal' | 'search'
    ariaLabel?: string
    autofocus?: boolean
  }>(),
  {
    modelValue: '',
    variant: 'outlined',
    size: 'md',
    label: '',
    placeholder: '',
    supportingText: '',
    error: false,
    type: 'text',
    clearable: false,
    disabled: false,
    readonly: false,
    required: false,
    autocomplete: undefined,
    name: undefined,
    maxlength: undefined,
    min: undefined,
    max: undefined,
    step: undefined,
    inputmode: undefined,
    ariaLabel: undefined,
    autofocus: false,
  },
)

const emit = defineEmits<{
  'update:modelValue': [value: string]
  clear: []
  enter: [value: string]
  focus: [ev: FocusEvent]
  blur: [ev: FocusEvent]
}>()

const { t } = useI18n()
const inputId = useId()
const helperId = useId()
const inputRef = ref<HTMLInputElement | null>(null)
const focused = ref(false)

const stringValue = computed(() => (props.modelValue == null ? '' : String(props.modelValue)))
const hasError = computed(() => Boolean(props.error))
const errorMessage = computed(() => (typeof props.error === 'string' ? props.error : ''))
const helperText = computed(() => errorMessage.value || props.supportingText)
const showClear = computed(
  () => props.clearable && !props.disabled && !props.readonly && stringValue.value.length > 0,
)

const containerClass = computed(() => {
  const height = props.size === 'sm' ? 'h-9' : 'h-11'
  if (props.disabled) {
    return props.variant === 'filled'
      ? `${height} rounded-t-corner-md rounded-b-corner-none border-b border-on-surface/12 bg-on-surface/4 text-on-surface/38 cursor-not-allowed`
      : `${height} rounded-corner-md border border-on-surface/12 bg-transparent text-on-surface/38 cursor-not-allowed`
  }

  if (props.variant === 'filled') {
    const border = hasError.value
      ? 'border-b-2 border-error'
      : focused.value
        ? 'border-b-2 border-primary'
        : 'border-b border-outline-variant hover:border-outline'
    return `${height} rounded-t-corner-md rounded-b-corner-none bg-surface-container-highest ${border}`
  }

  const border = hasError.value
    ? focused.value
      ? 'border-error ring-1 ring-error'
      : 'border-error'
    : focused.value
      ? 'border-primary ring-1 ring-primary'
      : 'border-outline-variant hover:border-outline'
  return `${height} rounded-corner-md border bg-surface-container-lowest ${border}`
})

function onInput(ev: Event): void {
  const val = (ev.target as HTMLInputElement).value
  emit('update:modelValue', val)
}

function onClear(): void {
  emit('update:modelValue', '')
  emit('clear')
  inputRef.value?.focus()
}

function onFocus(ev: FocusEvent): void {
  focused.value = true
  emit('focus', ev)
}

function onBlur(ev: FocusEvent): void {
  focused.value = false
  emit('blur', ev)
}

function onKeydown(ev: KeyboardEvent): void {
  if (ev.key === 'Enter' && !ev.isComposing) {
    emit('enter', stringValue.value)
  }
}

defineExpose({
  inputRef,
  focus: () => inputRef.value?.focus(),
  blur: () => inputRef.value?.blur(),
  select: () => inputRef.value?.select(),
})
</script>

<template>
  <div class="flex flex-col gap-1">
    <label
      v-if="label"
      :for="inputId"
      class="text-label-medium"
      :class="hasError ? 'text-error' : disabled ? 'text-on-surface/38' : 'text-on-surface-variant'"
    >
      <span>{{ label }}</span>
      <span v-if="required" class="ml-0.5 text-error" aria-hidden="true">*</span>
    </label>

    <div
      class="relative flex items-center gap-2 px-3 transition-colors duration-150 ease-standard"
      :class="containerClass"
    >
      <span
        v-if="$slots.leading || type === 'search'"
        class="flex shrink-0 items-center text-on-surface-variant"
        :class="disabled ? 'opacity-38' : ''"
      >
        <slot name="leading">
          <LuSearch v-if="type === 'search'" class="h-4 w-4" aria-hidden="true" />
        </slot>
      </span>

      <input
        :id="inputId"
        ref="inputRef"
        :value="stringValue"
        :type="type"
        :name="name"
        :placeholder="placeholder"
        :disabled="disabled"
        :readonly="readonly"
        :required="required"
        :autocomplete="autocomplete"
        :maxlength="maxlength"
        :min="min"
        :max="max"
        :step="step"
        :inputmode="inputmode"
        :autofocus="autofocus"
        :aria-label="ariaLabel || (!label ? placeholder || undefined : undefined)"
        :aria-invalid="hasError ? 'true' : undefined"
        :aria-describedby="helperText ? helperId : undefined"
        class="text-body-medium min-w-0 flex-1 bg-transparent text-on-surface placeholder:text-on-surface-variant caret-primary outline-none disabled:cursor-not-allowed disabled:text-on-surface/38 [&::-webkit-search-cancel-button]:hidden"
        @input="onInput"
        @focus="onFocus"
        @blur="onBlur"
        @keydown="onKeydown"
      />

      <button
        v-if="showClear"
        type="button"
        class="md-state md-touch-target inline-flex h-6 w-6 shrink-0 cursor-pointer items-center justify-center rounded-corner-full text-on-surface-variant hover:text-on-surface"
        :aria-label="t('common.clear') || t('common.cancel')"
        @click="onClear"
      >
        <LuX class="h-4 w-4" aria-hidden="true" />
      </button>

      <span
        v-if="$slots.trailing"
        class="flex shrink-0 items-center text-on-surface-variant"
        :class="disabled ? 'opacity-38' : ''"
      >
        <slot name="trailing" />
      </span>
    </div>

    <p
      v-if="helperText"
      :id="helperId"
      class="text-body-small px-1"
      :class="hasError ? 'text-error' : 'text-on-surface-variant'"
      :role="hasError ? 'alert' : undefined"
    >
      {{ helperText }}
    </p>
  </div>
</template>
