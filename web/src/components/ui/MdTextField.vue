<script setup lang="ts">
import { computed, ref, useId, useSlots, type Component } from 'vue'
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
    leadingIcon?: Component
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
    multiline?: boolean
    rows?: number | string
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
    leadingIcon: undefined,
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
    multiline: false,
    rows: 3,
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
const slots = useSlots()
const inputId = useId()
const helperId = useId()
const inputRef = ref<HTMLInputElement | HTMLTextAreaElement | null>(null)
const focused = ref(false)

const stringValue = computed(() => (props.modelValue == null ? '' : String(props.modelValue)))
const hasError = computed(() => Boolean(props.error))
const errorMessage = computed(() => (typeof props.error === 'string' ? props.error : ''))
const helperText = computed(() => errorMessage.value || props.supportingText)
const hasLeading = computed(
  () => !props.multiline && Boolean(slots.leading || props.leadingIcon || props.type === 'search'),
)
const isAlwaysFloatingType = computed(
  () =>
    props.type === 'date' ||
    props.type === 'time' ||
    props.type === 'datetime-local' ||
    props.type === 'month' ||
    props.type === 'week',
)
const isFloating = computed(
  () => focused.value || stringValue.value.length > 0 || isAlwaysFloatingType.value,
)
const hideNativePlaceholder = computed(() => Boolean(props.label) && !isFloating.value)
const showClear = computed(
  () =>
    !props.multiline &&
    props.clearable &&
    !props.disabled &&
    !props.readonly &&
    stringValue.value.length > 0,
)

const containerHeightClass = computed(() => {
  if (props.multiline) {
    return props.variant === 'filled' && props.label ? 'min-h-20 pt-5 pb-2' : 'min-h-16 py-2.5'
  }
  if (props.variant === 'filled' && props.label) {
    return props.size === 'sm' ? 'h-12' : 'h-14'
  }
  return props.size === 'sm' ? 'h-9' : 'h-12'
})

const containerClass = computed(() => {
  if (props.variant === 'filled') {
    if (props.disabled) {
      return `${containerHeightClass.value} rounded-t-corner-xs rounded-b-corner-none border-b border-on-surface/12 bg-on-surface/4 text-on-surface/38 cursor-not-allowed`
    }
    const indicator = hasError.value
      ? 'border-b border-error after:scale-x-100 after:bg-error'
      : focused.value
        ? 'border-b border-primary after:scale-x-100 after:bg-primary'
        : 'border-b border-on-surface-variant hover:border-on-surface after:scale-x-0 after:bg-primary'
    return `${containerHeightClass.value} rounded-t-corner-xs rounded-b-corner-none bg-surface-container-highest after:pointer-events-none after:absolute after:inset-x-0 after:bottom-0 after:h-0.5 after:transition-transform after:duration-150 after:ease-standard ${indicator}`
  }

  return `${containerHeightClass.value} rounded-corner-xs bg-transparent ${props.disabled ? 'cursor-not-allowed text-on-surface/38' : ''}`
})

const outlinedFieldsetClass = computed(() => {
  if (props.disabled) {
    return 'border border-on-surface/12'
  }
  if (hasError.value) {
    return focused.value ? 'border-2 border-error' : 'border border-error group-hover/tf:border-error'
  }
  if (focused.value) {
    return 'border-2 border-primary'
  }
  return 'border border-outline group-hover/tf:border-on-surface'
})

const labelColorClass = computed(() => {
  if (props.disabled) return 'text-on-surface/38'
  if (hasError.value) return 'text-error'
  if (focused.value) return 'text-primary'
  return 'text-on-surface-variant'
})

const floatingLabelPositionClass = computed(() => {
  if (props.variant === 'filled') {
    if (isFloating.value) {
      return [
        'top-1.5 translate-y-0 text-label-small',
        hasLeading.value ? 'left-10 right-3.5' : 'left-3.5 right-3.5',
      ]
    }
    return [
      props.multiline ? 'top-3.5 translate-y-0' : 'top-1/2 -translate-y-1/2',
      props.size === 'sm' ? 'text-body-medium' : 'text-body-large sm:text-body-medium',
      hasLeading.value ? 'left-10 right-3.5' : 'left-3.5 right-3.5',
    ]
  }

  if (isFloating.value) {
    return 'top-0 left-3.5 max-w-[calc(100%-1.75rem)] -translate-y-1/2 text-label-small'
  }
  return [
    props.multiline ? 'top-3 translate-y-0' : 'top-1/2 -translate-y-1/2',
    props.size === 'sm' ? 'text-body-medium' : 'text-body-large sm:text-body-medium',
    hasLeading.value ? 'left-10 right-3.5' : 'left-3.5 right-3.5',
  ]
})

function onInput(ev: Event): void {
  const val = (ev.target as HTMLInputElement | HTMLTextAreaElement).value
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
  <div class="flex flex-col gap-1" :class="variant === 'outlined' && label ? 'pt-1.5' : ''">
    <div
      class="group/tf relative flex gap-2 px-3.5 transition-colors duration-150 ease-standard"
      :class="[multiline ? 'items-start' : 'items-center', containerClass]"
    >
      <!-- MD3 Outlined Notched Border -->
      <fieldset
        v-if="variant === 'outlined'"
        aria-hidden="true"
        class="pointer-events-none absolute -top-[5px] right-0 bottom-0 left-0 m-0 min-w-0 rounded-corner-xs px-2.5 transition-colors duration-150 ease-standard"
        :class="outlinedFieldsetClass"
      >
        <legend
          class="text-label-small invisible block h-[11px] overflow-hidden leading-[11px] whitespace-nowrap transition-[max-width,padding] duration-150 ease-standard"
          :class="label && isFloating ? 'max-w-full px-1' : 'max-w-0 px-0'"
        >
          <span>{{ label || '\u00a0' }}</span>
          <span v-if="label && required" class="ml-0.5">*</span>
        </legend>
      </fieldset>

      <!-- MD3 Floating / Resting In-Field Label -->
      <label
        v-if="label"
        :for="inputId"
        class="pointer-events-none absolute z-10 origin-top-left truncate transition-all duration-150 ease-standard select-none"
        :class="[labelColorClass, floatingLabelPositionClass]"
      >
        <span>{{ label }}</span>
        <span v-if="required" class="ml-0.5 text-error" aria-hidden="true">*</span>
      </label>

      <span
        v-if="hasLeading"
        class="relative z-10 flex shrink-0 items-center text-on-surface-variant"
        :class="disabled ? 'opacity-38' : ''"
      >
        <slot name="leading">
          <component :is="leadingIcon" v-if="leadingIcon" class="h-4 w-4" aria-hidden="true" />
          <LuSearch v-else-if="type === 'search'" class="h-4 w-4" aria-hidden="true" />
        </slot>
      </span>

      <textarea
        v-if="multiline"
        :id="inputId"
        ref="inputRef"
        :value="stringValue"
        :rows="rows"
        :name="name"
        :placeholder="placeholder"
        :disabled="disabled"
        :readonly="readonly"
        :required="required"
        :maxlength="maxlength"
        :autofocus="autofocus"
        :aria-label="ariaLabel || (!label ? placeholder || undefined : undefined)"
        :aria-invalid="hasError ? 'true' : undefined"
        :aria-describedby="helperText ? helperId : undefined"
        class="relative z-10 min-w-0 flex-1 resize-y bg-transparent text-base text-on-surface caret-primary outline-none sm:text-body-medium disabled:cursor-not-allowed disabled:text-on-surface/38"
        :class="
          hideNativePlaceholder
            ? 'placeholder:text-transparent'
            : 'placeholder:text-on-surface-variant'
        "
        @input="onInput"
        @focus="onFocus"
        @blur="onBlur"
      />

      <input
        v-else
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
        class="relative z-10 min-w-0 flex-1 bg-transparent text-base text-on-surface caret-primary outline-none sm:text-body-medium disabled:cursor-not-allowed disabled:text-on-surface/38 [&::-webkit-search-cancel-button]:hidden"
        :class="[
          variant === 'filled' && label ? 'pt-4 pb-0.5' : '',
          hideNativePlaceholder
            ? 'placeholder:text-transparent'
            : 'placeholder:text-on-surface-variant',
        ]"
        @input="onInput"
        @focus="onFocus"
        @blur="onBlur"
        @keydown="onKeydown"
      />

      <button
        v-if="showClear"
        type="button"
        class="md-state md-touch-target relative z-10 -mr-1 inline-flex h-8 w-8 shrink-0 cursor-pointer items-center justify-center rounded-corner-full text-on-surface-variant hover:text-on-surface"
        :aria-label="t('common.clear') || t('common.cancel')"
        @click="onClear"
      >
        <LuX class="h-4 w-4" aria-hidden="true" />
      </button>

      <span
        v-if="$slots.trailing"
        class="relative z-10 flex shrink-0 items-center text-on-surface-variant"
        :class="disabled ? 'opacity-38' : ''"
      >
        <slot name="trailing" />
      </span>
    </div>

    <p
      v-if="helperText"
      :id="helperId"
      class="text-body-small px-4"
      :class="hasError ? 'text-error' : 'text-on-surface-variant'"
      :role="hasError ? 'alert' : undefined"
    >
      {{ helperText }}
    </p>
  </div>
</template>
