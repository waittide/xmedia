<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, useAttrs, useId, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { LuCheck, LuChevronDown, LuSearch } from 'vue-icons-plus/lu'
import { useEscapeStack } from '@/composables/useEscapeStack'

export interface SelectOption<T extends string = string> {
  value: T
  label: string
  badge?: string | number
}

defineOptions({ inheritAttrs: false })

const props = withDefaults(
  defineProps<{
    modelValue: string
    options: SelectOption[]
    placeholder?: string
    variant?: 'field' | 'inline' | 'compact'
    size?: 'sm' | 'md'
    searchable?: boolean
    searchPlaceholder?: string
    disabled?: boolean
    teleportTo?: string | HTMLElement | null
  }>(),
  {
    placeholder: '',
    variant: 'field',
    size: 'md',
    searchable: false,
    searchPlaceholder: '',
    disabled: false,
    teleportTo: null,
  },
)

const emit = defineEmits<{
  'update:modelValue': [value: string]
  change: [value: string]
}>()

const attrs = useAttrs()
const { t } = useI18n()
const escapeStack = useEscapeStack()
const listboxId = useId()
const open = ref(false)
const query = ref('')
const highlightedIdx = ref(0)
const triggerRef = ref<HTMLButtonElement | null>(null)
const menuRef = ref<HTMLDivElement | null>(null)
const searchInputRef = ref<HTMLInputElement | null>(null)
const menuStyle = ref<Record<string, string>>({})

function optionDomId(idx: number): string {
  return `${listboxId}-opt-${idx}`
}

const activeDescendantId = computed<string | undefined>(() => {
  if (!open.value || filteredOptions.value.length === 0) return undefined
  const idx = Math.max(0, Math.min(highlightedIdx.value, filteredOptions.value.length - 1))
  return optionDomId(idx)
})

const resolvedTeleportTarget = computed<string | HTMLElement>(() => {
  if (props.teleportTo) return props.teleportTo
  if (typeof document !== 'undefined' && document.fullscreenElement instanceof HTMLElement) {
    return document.fullscreenElement
  }
  return 'body'
})

const selectedOption = computed(
  () => props.options.find((opt) => opt.value === props.modelValue) ?? null,
)

const filteredOptions = computed(() => {
  const q = query.value.trim().toLowerCase()
  if (!q) return props.options
  return props.options.filter((opt) => opt.label.toLowerCase().includes(q))
})

const showSearchBox = computed(() => props.searchable || props.options.length > 8)

const triggerVariantClass = computed(() => {
  if (props.variant === 'inline') {
    return 'bg-transparent text-label-large text-on-surface-variant hover:text-on-surface'
  }
  const shape =
    props.variant === 'compact'
      ? 'h-8 px-3 text-label-medium'
      : props.size === 'sm'
        ? 'h-9 w-full px-3 text-body-medium'
        : 'h-11 w-full px-3.5 text-body-medium'
  const border = open.value
    ? 'border-primary ring-1 ring-primary'
    : 'border-outline-variant hover:border-outline'
  return `${shape} rounded-corner-md border bg-surface-container-lowest text-on-surface ${border}`
})

function updatePosition(): void {
  if (!open.value) return
  const el = triggerRef.value
  if (!el) return
  const rect = el.getBoundingClientRect()
  const viewportH = window.innerHeight
  const viewportW = window.innerWidth
  const estimatedHeight = Math.min(
    280,
    filteredOptions.value.length * 40 + (showSearchBox.value ? 52 : 12),
  )
  const openUpward =
    rect.bottom + estimatedHeight + 12 > viewportH && rect.top > estimatedHeight

  const minWidth = Math.max(rect.width, 176)
  const left = Math.min(rect.left, Math.max(8, viewportW - minWidth - 8))

  menuStyle.value = {
    position: 'fixed',
    left: `${Math.round(left)}px`,
    minWidth: `${Math.round(minWidth)}px`,
    maxWidth: 'min(22rem, calc(100vw - 16px))',
    ...(openUpward
      ? { bottom: `${Math.round(viewportH - rect.top + 6)}px` }
      : { top: `${Math.round(rect.bottom + 6)}px` }),
  }
}

async function openMenu(): Promise<void> {
  if (props.disabled) return
  open.value = true
  query.value = ''
  const currentIdx = props.options.findIndex((o) => o.value === props.modelValue)
  highlightedIdx.value = currentIdx >= 0 ? currentIdx : 0
  updatePosition()
  await nextTick()
  updatePosition()
  if (showSearchBox.value) {
    searchInputRef.value?.focus()
  }
}

function closeMenu(): void {
  open.value = false
}

escapeStack.register(
  () => open.value,
  () => {
    closeMenu()
    triggerRef.value?.focus()
  },
)

function toggleMenu(): void {
  if (open.value) {
    closeMenu()
  } else {
    void openMenu()
  }
}

function selectValue(val: string): void {
  emit('update:modelValue', val)
  emit('change', val)
  closeMenu()
  triggerRef.value?.focus()
}

function onTriggerKeydown(ev: KeyboardEvent): void {
  if (props.disabled) return
  if (ev.key === 'ArrowDown' || ev.key === 'ArrowUp' || ev.key === 'Enter' || ev.key === ' ') {
    if (!open.value) {
      ev.preventDefault()
      void openMenu()
      return
    }
  }
  onMenuKeydown(ev)
}

function onMenuKeydown(ev: KeyboardEvent): void {
  if (!open.value || ev.isComposing) return
  const len = filteredOptions.value.length
  if (ev.key === 'ArrowDown' && len > 0) {
    ev.preventDefault()
    ev.stopPropagation()
    highlightedIdx.value = (highlightedIdx.value + 1) % len
  } else if (ev.key === 'ArrowUp' && len > 0) {
    ev.preventDefault()
    ev.stopPropagation()
    highlightedIdx.value = (highlightedIdx.value - 1 + len) % len
  } else if (ev.key === 'Home' && len > 0) {
    ev.preventDefault()
    ev.stopPropagation()
    highlightedIdx.value = 0
  } else if (ev.key === 'End' && len > 0) {
    ev.preventDefault()
    ev.stopPropagation()
    highlightedIdx.value = len - 1
  } else if (ev.key === 'Enter' && len > 0) {
    ev.preventDefault()
    ev.stopPropagation()
    const picked = filteredOptions.value[highlightedIdx.value]
    if (picked) selectValue(picked.value)
  } else if (ev.key === ' ') {
    if (document.activeElement !== searchInputRef.value) {
      ev.stopPropagation()
    }
  }
}

function handleOutsideMouseDown(ev: MouseEvent): void {
  if (!open.value) return
  const target = ev.target as Node
  if (triggerRef.value?.contains(target) || menuRef.value?.contains(target)) {
    return
  }
  closeMenu()
}

function handleViewportChange(): void {
  if (open.value) updatePosition()
}

watch(filteredOptions, () => {
  highlightedIdx.value = 0
  if (open.value) void nextTick(updatePosition)
})

function bindGlobalListeners(): void {
  document.addEventListener('mousedown', handleOutsideMouseDown)
  window.addEventListener('resize', handleViewportChange)
  window.addEventListener('scroll', handleViewportChange, true)
}

function unbindGlobalListeners(): void {
  document.removeEventListener('mousedown', handleOutsideMouseDown)
  window.removeEventListener('resize', handleViewportChange)
  window.removeEventListener('scroll', handleViewportChange, true)
}

watch(open, (isOpen) => {
  if (isOpen) bindGlobalListeners()
  else unbindGlobalListeners()
})

onBeforeUnmount(unbindGlobalListeners)
</script>

<template>
  <button
    v-bind="attrs"
    ref="triggerRef"
    type="button"
    role="combobox"
    aria-haspopup="listbox"
    :aria-expanded="open"
    :aria-controls="open ? listboxId : undefined"
    :aria-activedescendant="activeDescendantId"
    :disabled="disabled"
    class="inline-flex cursor-pointer items-center justify-between gap-2 text-left transition-colors duration-150 ease-standard outline-none select-none focus-visible:outline-3 focus-visible:outline-offset-2 focus-visible:outline-secondary disabled:cursor-not-allowed disabled:opacity-38"
    :class="triggerVariantClass"
    @click="toggleMenu"
    @keydown="onTriggerKeydown"
  >
    <span class="truncate">
      {{ selectedOption ? selectedOption.label : placeholder }}
    </span>
    <LuChevronDown
      class="h-4 w-4 shrink-0 text-on-surface-variant transition-transform duration-150 ease-standard"
      :class="open ? 'rotate-180 text-primary' : ''"
      aria-hidden="true"
    />
  </button>

  <Teleport :to="resolvedTeleportTarget">
    <Transition name="md-dialog">
      <div
        v-if="open"
        :id="listboxId"
        ref="menuRef"
        role="listbox"
        :style="menuStyle"
        class="z-[var(--z-popover)] overflow-hidden rounded-corner-md bg-surface-container p-1.5 text-on-surface shadow-elevation-2"
        @keydown="onMenuKeydown"
      >
        <div v-if="showSearchBox" class="relative mb-1.5 px-1 pt-0.5">
          <LuSearch
            class="pointer-events-none absolute top-1/2 left-3.5 h-3.5 w-3.5 -translate-y-1/2 text-on-surface-variant"
            aria-hidden="true"
          />
          <input
            ref="searchInputRef"
            v-model="query"
            type="text"
            :placeholder="searchPlaceholder || t('common.filterPlaceholder')"
            class="text-body-small h-8 w-full rounded-corner-sm border border-outline-variant bg-surface-container-lowest pr-2.5 pl-8 text-on-surface placeholder:text-on-surface-variant caret-primary outline-none focus:border-primary focus:ring-1 focus:ring-primary"
          />
        </div>

        <div class="max-h-60 space-y-0.5 overflow-y-auto">
          <button
            v-for="(opt, idx) in filteredOptions"
            :id="optionDomId(idx)"
            :key="`${opt.value}-${idx}`"
            type="button"
            role="option"
            :aria-selected="opt.value === modelValue"
            class="md-state text-body-medium flex min-h-10 w-full cursor-pointer items-center justify-between gap-3 rounded-corner-sm px-3 py-2 text-left"
            :class="[
              opt.value === modelValue
                ? 'bg-secondary-container text-on-secondary-container'
                : idx === highlightedIdx
                  ? 'bg-on-surface/8 text-on-surface'
                  : 'text-on-surface',
            ]"
            @mouseenter="highlightedIdx = idx"
            @click="selectValue(opt.value)"
          >
            <span class="truncate">{{ opt.label }}</span>
            <span class="flex shrink-0 items-center gap-1.5">
              <span
                v-if="opt.badge !== undefined"
                class="text-label-small rounded-corner-xs bg-surface-container-highest px-1.5 py-0.5 text-on-surface-variant"
              >
                {{ opt.badge }}
              </span>
              <LuCheck
                v-if="opt.value === modelValue"
                class="h-4 w-4 text-on-secondary-container"
                aria-hidden="true"
              />
            </span>
          </button>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>
