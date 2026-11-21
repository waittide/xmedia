<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { LuCheck, LuChevronDown, LuSearch } from 'vue-icons-plus/lu'
import { useEscapeStack } from '@/composables/useEscapeStack'

export interface SelectOption<T extends string = string> {
  value: T
  label: string
  badge?: string | number
}

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
  }>(),
  {
    placeholder: '',
    variant: 'field',
    size: 'md',
    searchable: false,
    searchPlaceholder: '',
    disabled: false,
  },
)

const emit = defineEmits<{
  'update:modelValue': [value: string]
  change: [value: string]
}>()

const { t } = useI18n()
const escapeStack = useEscapeStack()
const open = ref(false)
const query = ref('')
const highlightedIdx = ref(0)
const triggerRef = ref<HTMLButtonElement | null>(null)
const menuRef = ref<HTMLDivElement | null>(null)
const searchInputRef = ref<HTMLInputElement | null>(null)
const menuStyle = ref<Record<string, string>>({})

const selectedOption = computed(
  () => props.options.find((opt) => opt.value === props.modelValue) ?? null,
)

const filteredOptions = computed(() => {
  const q = query.value.trim().toLowerCase()
  if (!q) return props.options
  return props.options.filter((opt) => opt.label.toLowerCase().includes(q))
})

const showSearchBox = computed(() => props.searchable || props.options.length > 8)

function updatePosition(): void {
  if (!open.value) return
  const el = triggerRef.value
  if (!el) return
  const rect = el.getBoundingClientRect()
  const viewportH = window.innerHeight
  const viewportW = window.innerWidth
  const estimatedHeight = Math.min(
    280,
    filteredOptions.value.length * 36 + (showSearchBox.value ? 52 : 12),
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
  if (!open.value) return
  const len = filteredOptions.value.length
  if (ev.key === 'ArrowDown' && len > 0) {
    ev.preventDefault()
    highlightedIdx.value = (highlightedIdx.value + 1) % len
  } else if (ev.key === 'ArrowUp' && len > 0) {
    ev.preventDefault()
    highlightedIdx.value = (highlightedIdx.value - 1 + len) % len
  } else if (ev.key === 'Enter' && len > 0) {
    ev.preventDefault()
    const picked = filteredOptions.value[highlightedIdx.value]
    if (picked) selectValue(picked.value)
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
    ref="triggerRef"
    type="button"
    role="combobox"
    :aria-expanded="open"
    :disabled="disabled"
    class="inline-flex cursor-pointer items-center justify-between gap-2 text-left text-xs transition outline-none select-none focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--md-sys-color-primary)] disabled:cursor-not-allowed disabled:opacity-45"
    :class="[
      variant === 'inline'
        ? 'bg-transparent font-medium text-[var(--md-sys-color-on-surface-variant)] hover:text-[var(--md-sys-color-on-surface)]'
        : variant === 'compact'
          ? 'h-8 rounded-[var(--md-sys-shape-corner-sm)] border bg-[var(--md-sys-color-surface)] px-3 font-medium text-[var(--md-sys-color-on-surface)]'
          : size === 'sm'
            ? 'h-9 w-full rounded-[var(--md-sys-shape-corner-sm)] border bg-[var(--md-sys-color-surface)] px-3 font-medium text-[var(--md-sys-color-on-surface)]'
            : 'h-10 w-full rounded-[var(--md-sys-shape-corner-sm)] border bg-[var(--md-sys-color-surface)] px-3.5 font-medium text-[var(--md-sys-color-on-surface)]',
      variant !== 'inline' &&
        (open
          ? 'border-[var(--md-sys-color-primary)] ring-1 ring-[var(--md-sys-color-primary)]'
          : 'border-[var(--md-sys-color-outline)] hover:border-[var(--md-sys-color-on-surface)]'),
    ]"
    @click="toggleMenu"
    @keydown="onTriggerKeydown"
  >
    <span class="truncate">
      {{ selectedOption ? selectedOption.label : placeholder }}
    </span>
    <LuChevronDown
      class="h-3.5 w-3.5 shrink-0 text-[var(--md-sys-color-on-surface-variant)] transition-transform duration-150"
      :class="open ? 'rotate-180 text-[var(--md-sys-color-primary)]' : ''"
    />
  </button>

  <Teleport to="body">
    <div
      v-if="open"
      ref="menuRef"
      role="listbox"
      :style="menuStyle"
      class="z-[var(--z-popover)] overflow-hidden rounded-[var(--md-sys-shape-corner-sm)] bg-[var(--md-sys-color-surface-container)] p-1.5 text-[var(--md-sys-color-on-surface)] shadow-[var(--md-sys-elevation-2)] ring-1 ring-[var(--md-sys-color-outline-variant)]/40"
      @keydown="onMenuKeydown"
    >
      <div v-if="showSearchBox" class="relative mb-1.5 px-1 pt-0.5">
        <LuSearch
          class="pointer-events-none absolute top-1/2 left-3 h-3.5 w-3.5 -translate-y-1/2 text-[var(--md-sys-color-on-surface-variant)]"
        />
        <input
          ref="searchInputRef"
          v-model="query"
          type="text"
          :placeholder="searchPlaceholder || t('common.filterPlaceholder')"
          class="md-input h-8 w-full !pr-2.5 !pl-8 text-xs"
        />
      </div>

      <div class="max-h-60 space-y-0.5 overflow-y-auto">
        <button
          v-for="(opt, idx) in filteredOptions"
          :key="`${opt.value}-${idx}`"
          type="button"
          role="option"
          :aria-selected="opt.value === modelValue"
          class="md-state flex w-full cursor-pointer items-center justify-between gap-3 rounded-[var(--md-sys-shape-corner-xs)] px-3 py-2 text-left text-xs transition"
          :class="[
            opt.value === modelValue
              ? 'bg-[var(--md-sys-color-secondary-container)] font-semibold text-[var(--md-sys-color-on-secondary-container)]'
              : idx === highlightedIdx
                ? 'bg-[color-mix(in_srgb,var(--md-sys-color-on-surface)_8%,transparent)] text-[var(--md-sys-color-on-surface)]'
                : 'text-[var(--md-sys-color-on-surface)]',
          ]"
          @mouseenter="highlightedIdx = idx"
          @click="selectValue(opt.value)"
        >
          <span class="truncate">{{ opt.label }}</span>
          <span class="flex shrink-0 items-center gap-1.5">
            <span
              v-if="opt.badge !== undefined"
              class="rounded-md bg-[var(--md-sys-color-surface-container-highest)] px-1.5 py-0.5 text-[10px] font-medium text-[var(--md-sys-color-on-surface-variant)]"
            >
              {{ opt.badge }}
            </span>
            <LuCheck
              v-if="opt.value === modelValue"
              class="h-3.5 w-3.5 text-[var(--md-sys-color-on-secondary-container)]"
            />
          </span>
        </button>
      </div>
    </div>
  </Teleport>
</template>
